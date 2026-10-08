use std::ffi::OsString;
use std::io;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};

use agent_client_protocol::Error;
use agent_client_protocol::schema::v1::ErrorCode;

use crate::link::{Evidence, OsError, OsErrorKind};

const SUMMARY_LIMIT: usize = 1024;

/// Why a Session failed: `because` for a person to read, `evidence` for the control plane to act
/// on (ADR-0052).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    pub because: String,
    pub evidence: Option<Evidence>,
}

impl Failure {
    pub fn kestrels(because: impl Into<String>) -> Self {
        Self {
            because: because.into(),
            evidence: None,
        }
    }

    /// Only the error's code establishes anything; whatever its message says stays unknown.
    pub fn answered(error: &Error, offered: &[String], method: Option<&str>) -> Self {
        let because = error.to_string();
        let evidence = match error.code {
            ErrorCode::AuthRequired => Evidence::AuthenticationRequired {
                code: i32::from(error.code),
                methods: offered.to_vec(),
                method: method.map(str::to_owned),
            },
            _ => Evidence::Unknown {
                summary: because.clone(),
            },
        };

        Self {
            because,
            evidence: Some(evidence),
        }
    }

    pub fn unspawnable(command: &Path, error: &io::Error) -> Self {
        let kind = match error.kind() {
            io::ErrorKind::NotFound => OsErrorKind::NotFound,
            io::ErrorKind::PermissionDenied => OsErrorKind::PermissionDenied,
            _ => OsErrorKind::Other,
        };

        Self {
            because: format!(
                "the harness {} could not be spawned: {error}",
                command.display()
            ),
            evidence: Some(Evidence::ExecutableMissing {
                command: command.to_string_lossy().into_owned(),
                error: OsError {
                    kind,
                    code: error.raw_os_error(),
                },
            }),
        }
    }

    #[must_use]
    pub fn redacted(self, secrets: &Secrets) -> Self {
        let evidence = self.evidence.map(|evidence| match evidence {
            Evidence::Unknown { summary } => Evidence::Unknown {
                summary: bounded(&secrets.redacted(&summary)),
            },
            established => established,
        });

        Self {
            because: secrets.redacted(&self.because),
            evidence,
        }
    }
}

/// What the supervisor handed the harness, which nothing it reports may repeat.
#[derive(Debug, Clone, Default)]
pub struct Secrets(Vec<String>);

impl Secrets {
    /// A login file is redacted token by token as well as whole, since a harness that logs one
    /// logs a token out of it.
    pub fn of<'a>(handed: impl IntoIterator<Item = &'a String>) -> Self {
        let mut secrets = Vec::new();
        for value in handed {
            secrets.push(value.clone());
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(value) {
                leaves(&parsed, &mut secrets);
            }
        }
        // A short value such as `1` would redact every digit it shares with the text around it.
        secrets.retain(|secret| secret.len() >= 8);
        secrets.sort_by_key(|secret| std::cmp::Reverse(secret.len()));
        secrets.dedup();

        Self(secrets)
    }

    fn redacted(&self, text: &str) -> String {
        self.0.iter().fold(text.to_owned(), |text, secret| {
            text.replace(secret, "[redacted]")
        })
    }
}

fn leaves(value: &serde_json::Value, into: &mut Vec<String>) {
    match value {
        serde_json::Value::String(leaf) => into.push(leaf.clone()),
        serde_json::Value::Array(items) => items.iter().for_each(|item| leaves(item, into)),
        serde_json::Value::Object(fields) => fields.values().for_each(|field| leaves(field, into)),
        _ => {}
    }
}

fn bounded(text: &str) -> String {
    if text.len() <= SUMMARY_LIMIT {
        return text.to_owned();
    }
    let mut end = SUMMARY_LIMIT;
    while !text.is_char_boundary(end) {
        end -= 1;
    }

    text[..end].to_owned()
}

/// Resolved here rather than by `env`, which would only say so on its stderr: the harness is
/// exec'd through `env`, so its own spawn never fails.
pub fn executable(command: &Path, path: Option<OsString>) -> io::Result<PathBuf> {
    if command.components().count() > 1 {
        return runnable(command).map(|()| command.to_owned());
    }

    path.iter()
        .flat_map(std::env::split_paths)
        .map(|directory| directory.join(command))
        .find(|candidate| runnable(candidate).is_ok())
        .ok_or_else(|| io::ErrorKind::NotFound.into())
}

fn runnable(candidate: &Path) -> io::Result<()> {
    let metadata = std::fs::metadata(candidate)?;
    if metadata.is_file() && metadata.permissions().mode() & 0o111 != 0 {
        return Ok(());
    }

    Err(io::ErrorKind::PermissionDenied.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_authentication_error_is_evidence_of_one_whatever_it_says() {
        let failure = Failure::answered(
            &Error::auth_required().data("anything at all"),
            &["its-own".to_owned()],
            None,
        );

        assert_eq!(
            failure.evidence,
            Some(Evidence::AuthenticationRequired {
                code: -32000,
                methods: vec!["its-own".to_owned()],
                method: None,
            })
        );
    }

    #[test]
    fn an_error_that_only_reads_like_an_expired_login_is_unknown() {
        let failure = Failure::answered(
            &Error::internal_error().data("401 Unauthorized: login expired"),
            &[],
            None,
        );

        assert!(matches!(
            failure.evidence,
            Some(Evidence::Unknown { summary }) if summary.contains("401")
        ));
    }

    #[test]
    fn what_was_handed_to_the_harness_is_redacted_from_everything_reported() {
        let login = r#"{"token":"a-token-from-a-login-file"}"#.to_owned();
        let key = "a-provider-key".to_owned();
        let secrets = Secrets::of([&key, &login]);
        let failure = Failure::answered(
            &Error::internal_error().data("rejected a-provider-key and a-token-from-a-login-file"),
            &[],
            None,
        )
        .redacted(&secrets);

        for reported in [failure.because.clone(), format!("{:?}", failure.evidence)] {
            assert!(!reported.contains("a-provider-key"), "{reported}");
            assert!(
                !reported.contains("a-token-from-a-login-file"),
                "{reported}"
            );
        }
    }

    #[test]
    fn an_unknown_summary_is_bounded() {
        let failure = Failure::answered(
            &Error::internal_error().data("é".repeat(SUMMARY_LIMIT)),
            &[],
            None,
        )
        .redacted(&Secrets::default());

        let Some(Evidence::Unknown { summary }) = failure.evidence else {
            panic!("unknown evidence");
        };
        assert!(summary.len() <= SUMMARY_LIMIT);
    }

    #[test]
    fn a_command_on_no_path_is_not_found() {
        let error = executable(
            Path::new("kestrel-no-such-harness"),
            Some("/usr/bin:/bin".into()),
        )
        .expect_err("nothing to find");

        assert_eq!(error.kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn a_path_that_does_not_exist_carries_the_operating_systems_error() {
        let error = executable(Path::new("/no/such/harness"), None).expect_err("nothing there");

        assert_eq!(error.kind(), io::ErrorKind::NotFound);
        assert!(error.raw_os_error().is_some());
    }

    #[test]
    fn a_command_on_the_path_is_resolved_against_it() {
        assert_eq!(
            executable(Path::new("sh"), Some("/nowhere:/bin".into())).expect("sh is on the path"),
            PathBuf::from("/bin/sh")
        );
    }
}
