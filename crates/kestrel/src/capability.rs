use std::collections::{BTreeSet, HashMap};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use tokio::process::Command;

use crate::compute::Driver;
use crate::declined::Reason;

/// What an image declares the harnesses it carries in (ADR-0048).
pub const LABEL: &str = "dev.kestrel.harnesses";

/// A daemon that does not answer is unavailable, not a reason to hold a request open.
const PATIENCE: Duration = Duration::from_secs(10);
const REMEMBERED: usize = 16;

/// The configured image's capabilities, inspected afresh on every read: a mutable tag may name
/// another image by the next one, so only what was parsed is kept, by the image's own identity.
#[derive(Clone, Default)]
pub struct Images {
    inspecting: Option<Arc<Inspecting>>,
}

struct Inspecting {
    image: String,
    parsed: Mutex<HashMap<String, Arc<BTreeSet<String>>>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Capability {
    /// The compute driver runs no image, so there is nothing to inspect.
    Unchecked,
    Inspected {
        image: String,
        identity: String,
        harnesses: Arc<BTreeSet<String>>,
    },
    Unavailable {
        image: String,
        cause: Uninspectable,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Uninspectable {
    /// Never pulled here: a pull is the operator's decision.
    Missing,
    Failed(String),
}

impl Images {
    pub fn of(driver: &Driver) -> Self {
        match driver {
            Driver::Docker(docker) => Self::inspecting(docker.image()),
            Driver::LocalExec(_) => Self::default(),
        }
    }

    pub fn inspecting(image: impl Into<String>) -> Self {
        Self {
            inspecting: Some(Arc::new(Inspecting {
                image: image.into(),
                parsed: Mutex::default(),
            })),
        }
    }

    pub async fn read(&self) -> Capability {
        let Some(inspecting) = &self.inspecting else {
            return Capability::Unchecked;
        };
        let image = inspecting.image.clone();
        match inspected(&image).await {
            Ok(metadata) => match inspecting.remember(&metadata) {
                Ok(harnesses) => Capability::Inspected {
                    image,
                    identity: metadata.identity,
                    harnesses,
                },
                Err(cause) => Capability::Unavailable { image, cause },
            },
            Err(cause) => Capability::Unavailable { image, cause },
        }
    }

    /// Refuses before anything is written, so a refused declaration or start leaves no part of
    /// itself behind.
    pub async fn admit(
        &self,
        harness: &str,
        operation: &'static str,
        field: &'static str,
    ) -> anyhow::Result<()> {
        self.read()
            .await
            .admits(harness, operation)
            .map_err(|reason| reason.concerning(field).into())
    }
}

impl Capability {
    /// `None` when nothing established whether it does: never a stale or invented answer.
    pub fn carries(&self, harness: &str) -> Option<bool> {
        match self {
            Capability::Inspected { harnesses, .. } => Some(harnesses.contains(harness)),
            Capability::Unchecked | Capability::Unavailable { .. } => None,
        }
    }

    pub fn admits(self, harness: &str, operation: &'static str) -> Result<(), Box<Reason>> {
        match self {
            Capability::Unchecked => Ok(()),
            Capability::Inspected { harnesses, .. } if harnesses.contains(harness) => Ok(()),
            Capability::Inspected {
                image, harnesses, ..
            } => Err(Box::new(Reason::HarnessNotCarried {
                operation,
                message: format!(
                    "the image {image} does not carry the harness {harness}; it declares {}",
                    declared(&harnesses)
                ),
                harness: harness.to_owned(),
                image,
                carried: harnesses.iter().cloned().collect(),
            })),
            Capability::Unavailable { image, cause } => Err(Box::new(Reason::ImageUnavailable {
                operation,
                message: unavailable(&image, &cause, Some(harness)),
                harness: Some(harness.to_owned()),
                image,
                missing: cause == Uninspectable::Missing,
            })),
        }
    }
}

pub fn unavailable(image: &str, cause: &Uninspectable, harness: Option<&str>) -> String {
    let unknown = match harness {
        Some(harness) => format!("whether it carries the harness {harness} is unknown"),
        None => "which harnesses it carries is unknown".to_owned(),
    };
    match cause {
        Uninspectable::Missing => {
            format!(
                "the image {image} is not on this machine, so {unknown}; kestrel never pulls it"
            )
        }
        Uninspectable::Failed(why) => {
            format!("the image {image} could not be inspected, so {unknown}: {why}")
        }
    }
}

fn declared(harnesses: &BTreeSet<String>) -> String {
    if harnesses.is_empty() {
        return "none".to_owned();
    }
    harnesses.iter().cloned().collect::<Vec<_>>().join(", ")
}

struct Metadata {
    identity: String,
    labels: String,
}

impl Inspecting {
    fn remember(&self, metadata: &Metadata) -> Result<Arc<BTreeSet<String>>, Uninspectable> {
        let mut parsed = self.parsed.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(harnesses) = parsed.get(&metadata.identity) {
            return Ok(harnesses.clone());
        }
        let harnesses = Arc::new(harnesses(&metadata.labels)?);
        if parsed.len() >= REMEMBERED {
            parsed.clear();
        }
        parsed.insert(metadata.identity.clone(), harnesses.clone());
        Ok(harnesses)
    }
}

fn harnesses(labels: &str) -> Result<BTreeSet<String>, Uninspectable> {
    let labels: Option<HashMap<String, String>> = serde_json::from_str(labels)
        .map_err(|error| Uninspectable::Failed(format!("its labels did not read: {error}")))?;
    Ok(labels
        .unwrap_or_default()
        .get(LABEL)
        .map(|declared| {
            declared
                .split(',')
                .map(str::trim)
                .filter(|harness| !harness.is_empty())
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default())
}

async fn inspected(image: &str) -> Result<Metadata, Uninspectable> {
    let inspecting = Command::new("docker")
        .args([
            "image",
            "inspect",
            "--format",
            "{{.Id}} {{json .Config.Labels}}",
            "--",
            image,
        ])
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true)
        .output();
    let ran = tokio::time::timeout(PATIENCE, inspecting)
        .await
        .map_err(|_| {
            Uninspectable::Failed(format!(
                "the Docker daemon did not answer within {}s",
                PATIENCE.as_secs()
            ))
        })?
        .map_err(|error| Uninspectable::Failed(format!("docker could not run: {error}")))?;

    if !ran.status.success() {
        let said = String::from_utf8_lossy(&ran.stderr).trim().to_owned();
        let lowered = said.to_lowercase();
        if lowered.contains("no such image") || lowered.contains("no such object") {
            return Err(Uninspectable::Missing);
        }
        return Err(Uninspectable::Failed(said));
    }

    let out = String::from_utf8_lossy(&ran.stdout);
    let (identity, labels) = out
        .trim()
        .split_once(' ')
        .ok_or_else(|| Uninspectable::Failed(format!("docker answered {out:?}")))?;
    Ok(Metadata {
        identity: identity.to_owned(),
        labels: labels.to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inspecting() -> Inspecting {
        Inspecting {
            image: "ghcr.io/openkestrel/kestrel-env:main".to_owned(),
            parsed: Mutex::default(),
        }
    }

    fn metadata(identity: &str, labels: &str) -> Metadata {
        Metadata {
            identity: identity.to_owned(),
            labels: labels.to_owned(),
        }
    }

    fn set(names: &[&str]) -> Arc<BTreeSet<String>> {
        Arc::new(names.iter().map(|name| (*name).to_owned()).collect())
    }

    fn inspected(names: &[&str]) -> Capability {
        Capability::Inspected {
            image: "kestrel-env".to_owned(),
            identity: "sha256:one".to_owned(),
            harnesses: set(names),
        }
    }

    #[test]
    fn the_label_lists_the_harnesses_an_image_declares() {
        assert_eq!(
            harnesses(r#"{"dev.kestrel.harnesses":" opencode, claude,,codex "}"#),
            Ok((*set(&["claude", "codex", "opencode"])).clone())
        );
    }

    #[test]
    fn an_image_without_the_label_or_any_labels_declares_nothing() {
        assert_eq!(harnesses("null"), Ok(BTreeSet::new()));
        assert_eq!(
            harnesses(r#"{"org.opencontainers.image.source":"x"}"#),
            Ok(BTreeSet::new())
        );
    }

    #[test]
    fn parsed_labels_are_kept_by_image_identity_rather_than_by_reference() {
        let inspecting = inspecting();
        let first = r#"{"dev.kestrel.harnesses":"opencode,claude"}"#;

        assert_eq!(
            inspecting.remember(&metadata("sha256:one", first)),
            Ok(set(&["claude", "opencode"]))
        );
        // Labels are fixed by an identity, so the same identity is never parsed again.
        assert_eq!(
            inspecting.remember(&metadata("sha256:one", "not json")),
            Ok(set(&["claude", "opencode"]))
        );
        assert_eq!(
            inspecting.remember(&metadata(
                "sha256:two",
                r#"{"dev.kestrel.harnesses":"opencode"}"#
            )),
            Ok(set(&["opencode"]))
        );
    }

    #[test]
    fn a_declared_harness_is_admitted_and_an_undeclared_one_is_refused_naming_the_image() {
        assert!(
            inspected(&["opencode", "my-agent"])
                .admits("my-agent", "declare_agent")
                .is_ok()
        );

        let refused = inspected(&["opencode"])
            .admits("claude", "declare_agent")
            .expect_err("claude is not carried");
        match *refused {
            Reason::HarnessNotCarried {
                harness,
                image,
                carried,
                ..
            } => {
                assert_eq!(harness, "claude");
                assert_eq!(image, "kestrel-env");
                assert_eq!(carried, ["opencode"]);
            }
            other => panic!("refused as {other:?}"),
        }
    }

    #[test]
    fn an_inspection_that_failed_is_unavailable_never_absent() {
        for cause in [Uninspectable::Missing, Uninspectable::Failed("down".into())] {
            let capability = Capability::Unavailable {
                image: "kestrel-env".to_owned(),
                cause: cause.clone(),
            };
            assert_eq!(capability.carries("opencode"), None);
            match capability
                .admits("opencode", "start")
                .map_err(|reason| *reason)
            {
                Err(Reason::ImageUnavailable { missing, image, .. }) => {
                    assert_eq!(missing, cause == Uninspectable::Missing);
                    assert_eq!(image, "kestrel-env");
                }
                other => panic!("admitted as {other:?}"),
            }
        }
    }

    #[test]
    fn a_driver_with_no_image_admits_every_harness_and_establishes_nothing() {
        assert_eq!(Capability::Unchecked.carries("claude"), None);
        assert!(Capability::Unchecked.admits("claude", "start").is_ok());
    }
}
