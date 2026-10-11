use std::io::{IsTerminal as _, Write};

use anyhow::{Context as _, Result, bail};
use kestrel_operator_types as wire;

use crate::BINARY;
use crate::api::ControlPlane;
use crate::diagnostic::{self, Invocation};
use crate::exit::{Exit, Failed};
use crate::scope::{self, Derived, Scoping, Source};
use crate::shell::quoted;

const GREEN: &str = "1;32";
const YELLOW: &str = "1;33";
const RED: &str = "1;31";

/// The summary is a successful read on stdout whatever it found; a gap's repair goes through
/// the common diagnostics, which is what makes the exit 78 or 5.
pub async fn show(api: &ControlPlane, scoping: Scoping<'_>, invocation: &Invocation) -> Result<()> {
    let (organization, named) = match scoping.derive().await? {
        Derived::Scope(scope) => {
            let named = matches!(scope.source, Source::Flag);
            (Some(scope.organization), named)
        }
        Derived::Unnamed { existing } if existing.is_empty() => (None, false),
        Derived::Unnamed { existing } => bail!(scope::several(&existing)),
    };
    let scoped: Vec<(&str, &str)> = organization
        .iter()
        .map(|organization| ("organization", organization.as_str()))
        .collect();
    let answer = api.get_where(&["readiness"], &scoped).await?;
    let read: wire::Readiness = serde_json::from_value(answer.clone())
        .with_context(|| Failed::new(Exit::Unavailable, "reading readiness"))?;

    let mut out = std::io::stdout().lock();
    if invocation.json {
        writeln!(out, "{answer}")?;
    } else {
        let decorated = out.is_terminal() && !undecorated_by_request();
        write!(out, "{}", summary(&read, invocation, named, decorated))?;
    }
    out.flush()?;
    drop(out);

    if !invocation.json {
        let mut err = std::io::stderr().lock();
        // A failed inspection behind a gap would otherwise surface only once the gap is fixed.
        let behind = (read.outcome == wire::ReadinessOutcome::Blocked)
            .then_some(&read.unavailable)
            .into_iter()
            .flatten();
        for warning in read.warnings.iter().chain(behind) {
            writeln!(err, "warning: {}", diagnostic::inline(warning, invocation))?;
        }
    }

    let (exit, blocking) = match read.outcome {
        wire::ReadinessOutcome::Ready => return Ok(()),
        wire::ReadinessOutcome::Blocked => (Exit::NotReady, read.gaps.into_iter().next()),
        wire::ReadinessOutcome::Unavailable => {
            (Exit::Unavailable, read.unavailable.into_iter().next())
        }
    };
    match blocking {
        Some(diagnostic) => bail!(Failed::diagnosed(exit, diagnostic)),
        None => bail!(Failed::new(
            Exit::Unavailable,
            "the control plane answered that work cannot start, and not why"
        )),
    }
}

fn undecorated_by_request() -> bool {
    std::env::var("NO_COLOR").is_ok_and(|value| !value.is_empty())
}

fn summary(
    read: &wire::Readiness,
    invocation: &Invocation,
    named: bool,
    decorated: bool,
) -> String {
    let heading = |colour: &str, text: &str| {
        if decorated {
            format!("\u{1b}[{colour}m{text}\u{1b}[0m")
        } else {
            text.to_owned()
        }
    };
    let mut said = match read.outcome {
        wire::ReadinessOutcome::Ready => {
            let mut path = vec![heading(GREEN, "Ready")];
            path.extend(
                read.organization
                    .iter()
                    .map(|organization| format!("Organization {organization}")),
            );
            path.extend(read.selected.harness.clone());
            match (&read.selected.profile, &read.selected.sign_in_method) {
                (Some(profile), _) => path.push(format!("Profile {profile}")),
                (None, Some(method)) => path.push(method.clone()),
                (None, None) => {}
            }
            format!("{}\n", path.join(" · "))
        }
        wire::ReadinessOutcome::Blocked => format!(
            "{}: {}\n",
            heading(YELLOW, "Not ready"),
            read.gaps
                .first()
                .map_or("a setup prerequisite is missing", diagnostic::message)
        ),
        wire::ReadinessOutcome::Unavailable => format!(
            "{}: {}\n",
            heading(RED, "Readiness unavailable"),
            read.unavailable
                .first()
                .map_or("an inspection failed", diagnostic::message)
        ),
    };
    said.push_str(&format!("Client: {}\n", read.client_url));
    if read.outcome == wire::ReadinessOutcome::Ready {
        let mut next = vec![
            BINARY.to_owned(),
            "start".to_owned(),
            "--brief".to_owned(),
            "<BRIEF>".to_owned(),
        ];
        if let (true, Some(organization)) = (named, &read.organization) {
            next.extend(["--organization".to_owned(), quoted(organization)]);
        }
        if invocation.elsewhere {
            next.extend([
                "--control-plane".to_owned(),
                quoted(&invocation.control_plane),
            ]);
        }
        said.push_str(&format!("Next: {}\n", next.join(" ")));
    }

    said
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn read(outcome: &str, more: serde_json::Value) -> wire::Readiness {
        let mut read = json!({
            "outcome": outcome,
            "organization": "Acme East",
            "client_url": "http://localhost:7719",
            "initial_setup_complete": false,
            "selected": {
                "harness": null, "model": null, "profile": null,
                "sign_in_method": null, "project": null,
            },
            "sign_in": null,
            "saved": { "operator": null, "sign_ins": [], "github_integrations": [], "projects": [] },
            "gaps": [],
            "warnings": [],
            "unavailable": [],
        });
        read.as_object_mut()
            .expect("an object")
            .extend(more.as_object().expect("an object").clone());
        serde_json::from_value(read).expect("a readiness answer")
    }

    fn gap(message: &str) -> serde_json::Value {
        json!({
            "kind": "setup_gap",
            "message": message,
            "field": null,
            "context": {
                "prerequisite": "sign_in", "resource": null, "reference": null,
                "organization": "Acme East", "harness": null, "method": null,
                "sign_in": null, "image": null,
            },
            "next_steps": [],
        })
    }

    fn elsewhere() -> Invocation {
        Invocation {
            control_plane: "http://10.0.0.2:7718".to_owned(),
            elsewhere: true,
            operation: String::new(),
            args: Vec::new(),
            json: false,
        }
    }

    #[test]
    fn a_ready_summary_names_only_what_was_established_and_keeps_its_scope() {
        let ready = read(
            "ready",
            json!({ "selected": {
                "harness": "claude", "model": null, "profile": "jack",
                "sign_in_method": "claude-setup-token", "project": null,
            } }),
        );

        assert_eq!(
            summary(&ready, &elsewhere(), true, false),
            "Ready · Organization Acme East · claude · Profile jack\n\
             Client: http://localhost:7719\n\
             Next: kestrel start --brief <BRIEF> --organization 'Acme East' \
             --control-plane http://10.0.0.2:7718\n"
        );
        assert_eq!(
            summary(
                &read("ready", json!({})),
                &diagnostic::tests::invocation(),
                false,
                false
            ),
            "Ready · Organization Acme East\n\
             Client: http://localhost:7719\n\
             Next: kestrel start --brief <BRIEF>\n"
        );
    }

    #[test]
    fn only_the_first_blocking_gap_is_summarised() {
        let blocked = read(
            "blocked",
            json!({ "gaps": [gap("no harness is signed in"), gap("no repository is available")] }),
        );

        assert_eq!(
            summary(&blocked, &diagnostic::tests::invocation(), false, false),
            "Not ready: no harness is signed in\nClient: http://localhost:7719\n"
        );
    }

    #[test]
    fn decoration_is_the_headings_colour_and_nothing_else() {
        let blocked = read(
            "blocked",
            json!({ "gaps": [gap("no harness is signed in")] }),
        );

        let plain = summary(&blocked, &diagnostic::tests::invocation(), false, false);
        let decorated = summary(&blocked, &diagnostic::tests::invocation(), false, true);

        assert!(!plain.contains('\u{1b}'), "{plain}");
        assert_eq!(
            decorated
                .replace("\u{1b}[1;33m", "")
                .replace("\u{1b}[0m", ""),
            plain
        );
    }
}
