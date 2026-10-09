mod offer;
mod step;

use std::io::{IsTerminal as _, Write as _};
use std::sync::atomic::{AtomicBool, Ordering};

use kestrel_operator_types as wire;
use reqwest::{Response, StatusCode, header};
use serde_json::json;

use crate::exit::{Exit, Failed};

const SERVICE: &str = "control plane";
const EVIDENCE: usize = 240;

static WROTE: AtomicBool = AtomicBool::new(false);

/// Once a write has been sent, nothing this invocation offers may send it again.
pub fn writing() {
    WROTE.store(true, Ordering::Relaxed);
}

pub struct Invocation {
    pub control_plane: String,
    /// The control plane differs from the default, so every rendered command must name it.
    pub elsewhere: bool,
    pub operation: String,
    pub args: Vec<String>,
    pub json: bool,
}

macro_rules! each_kind {
    ($diagnostic:expr, $d:ident => $body:expr) => {{
        use wire::Diagnostic as D;
        match $diagnostic {
            D::MissingReferenceDiagnostic($d) => $body,
            D::AmbiguousReferenceDiagnostic($d) => $body,
            D::MalformedRequestDiagnostic($d) => $body,
            D::ForbiddenActionDiagnostic($d) => $body,
            D::StateConflictDiagnostic($d) => $body,
            D::ExpiredResourceDiagnostic($d) => $body,
            D::InvalidFieldDiagnostic($d) => $body,
            D::SetupGapDiagnostic($d) => $body,
            D::UnavailableDiagnostic($d) => $body,
            D::InstanceTimeoutDiagnostic($d) => $body,
            D::AuthenticationFailedDiagnostic($d) => $body,
            D::ExecutableMissingDiagnostic($d) => $body,
            D::UnknownFailureDiagnostic($d) => $body,
            D::ConnectionFailedDiagnostic($d) => $body,
            D::ClientFailureDiagnostic($d) => $body,
            D::UnknownResponseDiagnostic($d) => $body,
        }
    }};
}

pub fn message(diagnostic: &wire::Diagnostic) -> &str {
    each_kind!(diagnostic, d => &d.message)
}

fn next_steps(diagnostic: &wire::Diagnostic) -> &[wire::Action] {
    each_kind!(diagnostic, d => &d.next_steps)
}

/// The kind decides the exit; the status only places the kinds a Client cannot classify.
pub fn exit_for(diagnostic: &wire::Diagnostic, status: StatusCode) -> Exit {
    use wire::Diagnostic as D;
    match diagnostic {
        D::MissingReferenceDiagnostic(_) | D::AmbiguousReferenceDiagnostic(_) => Exit::Unresolved,
        D::MalformedRequestDiagnostic(_)
        | D::ForbiddenActionDiagnostic(_)
        | D::StateConflictDiagnostic(_)
        | D::ExpiredResourceDiagnostic(_)
        | D::InvalidFieldDiagnostic(_) => Exit::Rejected,
        D::SetupGapDiagnostic(_) => Exit::NotReady,
        D::UnavailableDiagnostic(_)
        | D::InstanceTimeoutDiagnostic(_)
        | D::ConnectionFailedDiagnostic(_) => Exit::Unavailable,
        D::ClientFailureDiagnostic(_) => Exit::Failure,
        D::AuthenticationFailedDiagnostic(_)
        | D::ExecutableMissingDiagnostic(_)
        | D::UnknownFailureDiagnostic(_)
        | D::UnknownResponseDiagnostic(_) => by_status(status),
    }
}

/// The operator boundary answers 404 for a reference that resolves to no record or to several.
pub fn by_status(status: StatusCode) -> Exit {
    match status {
        StatusCode::NOT_FOUND => Exit::Unresolved,
        status if status.is_client_error() => Exit::Rejected,
        _ => Exit::Unavailable,
    }
}

#[derive(Clone, Copy)]
pub struct Request<'a> {
    pub operation: &'a str,
    pub write: bool,
}

impl Request<'_> {
    /// An answer lost after the request was sent: a write may have landed, a read may be retried.
    fn lost(self, retry_after: Option<i64>) -> wire::Action {
        if self.write {
            inspect_operation(self.operation, true)
        } else {
            wire::Action::RetryReadAction(wire::RetryReadAction {
                action: json!("retry_read"),
                operation: self.operation.to_owned(),
                resource: None,
                retry_after_seconds: retry_after,
            })
        }
    }
}

pub async fn refusal(response: Response, request: Request<'_>) -> Failed {
    let status = response.status();
    let retry_after = response
        .headers()
        .get(header::RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse().ok());
    let body = response.bytes().await.unwrap_or_default();
    refused(status, retry_after, &body, request)
}

fn refused(
    status: StatusCode,
    retry_after: Option<i64>,
    body: &[u8],
    request: Request<'_>,
) -> Failed {
    if let Ok(diagnostic) = serde_json::from_slice::<wire::Diagnostic>(body) {
        return Failed::diagnosed(exit_for(&diagnostic, status), diagnostic);
    }
    let (message, evidence) = match serde_json::from_slice::<wire::Refusal>(body) {
        Ok(refusal) => (refusal.message, None),
        Err(_) => (
            format!("the control plane answered {status} with nothing kestrel recognises"),
            bounded(&String::from_utf8_lossy(body)),
        ),
    };
    let next = if status.is_server_error() {
        request.lost(retry_after)
    } else {
        inspect_operation(request.operation, false)
    };
    Failed::diagnosed(
        by_status(status),
        unknown_response(message, request.operation, Some(status), evidence, next),
    )
}

pub fn unreadable(
    status: StatusCode,
    request: Request<'_>,
    error: &dyn std::fmt::Display,
) -> Failed {
    Failed::diagnosed(
        Exit::Unavailable,
        unknown_response(
            "the control plane's answer could not be read".to_owned(),
            request.operation,
            Some(status),
            bounded(&error.to_string()),
            request.lost(None),
        ),
    )
}

/// `sent` says whether the request may have reached the control plane before the connection
/// failed; one that never connected cannot have taken effect.
pub fn unreachable(control_plane: &str, request: Request<'_>, sent: bool) -> Failed {
    let control_plane = control_plane.trim_end_matches('/');
    let mut next_steps = vec![check_connection()];
    if request.write && sent {
        next_steps.push(inspect_operation(request.operation, true));
    }
    Failed::diagnosed(
        Exit::Unavailable,
        wire::Diagnostic::ConnectionFailedDiagnostic(wire::ConnectionFailedDiagnostic {
            kind: json!("connection_failed"),
            message: format!("the control plane at {control_plane} could not be reached"),
            field: None,
            context: wire::ConnectionFailedContext {
                url: control_plane.to_owned(),
                operation: request.operation.to_owned(),
            },
            next_steps,
        }),
    )
}

fn check_connection() -> wire::Action {
    wire::Action::CheckConnectionAction(wire::CheckConnectionAction {
        action: json!("check_connection"),
        service: SERVICE.to_owned(),
        compose: false,
    })
}

fn unknown_response(
    message: String,
    operation: &str,
    status: Option<StatusCode>,
    evidence: Option<String>,
    next: wire::Action,
) -> wire::Diagnostic {
    wire::Diagnostic::UnknownResponseDiagnostic(wire::UnknownResponseDiagnostic {
        kind: json!("unknown_response"),
        message,
        field: None,
        context: wire::UnknownResponseContext {
            service: SERVICE.to_owned(),
            operation: operation.to_owned(),
            status: status.map(|status| i64::from(status.as_u16())),
            evidence,
        },
        next_steps: vec![next],
    })
}

fn inspect_operation(operation: &str, uncertain: bool) -> wire::Action {
    wire::Action::InspectOperationAction(wire::InspectOperationAction {
        action: json!("inspect_operation"),
        operation: operation.to_owned(),
        resource: None,
        uncertain,
    })
}

/// Evidence is a bounded single line, so an answer can neither flood a terminal nor steer it.
fn bounded(text: &str) -> Option<String> {
    let line: String = text
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .filter(|character| !character.is_control())
        .collect();
    if line.is_empty() {
        return None;
    }
    let mut kept: String = line.chars().take(EVIDENCE).collect();
    if kept.len() < line.len() {
        kept.push('…');
    }
    Some(kept)
}

/// A failure nothing classified still offers a next step. An `Unavailable` one had an answer
/// the Client could not read; any other happened on this machine.
fn fallback(
    error: &anyhow::Error,
    exit: Exit,
    invocation: &Invocation,
    wrote: bool,
) -> wire::Diagnostic {
    let message = bounded(&error.to_string()).unwrap_or_else(|| "kestrel failed".to_owned());
    let evidence = bounded(&format!("{error:#}"));
    if exit == Exit::Unavailable {
        let request = Request {
            operation: &invocation.operation,
            write: wrote,
        };
        return unknown_response(
            message,
            request.operation,
            None,
            evidence,
            request.lost(None),
        );
    }
    wire::Diagnostic::ClientFailureDiagnostic(wire::ClientFailureDiagnostic {
        kind: json!("client_failure"),
        message,
        field: None,
        context: wire::ClientFailureContext {
            operation: invocation.operation.clone(),
            evidence,
        },
        next_steps: vec![inspect_operation(&invocation.operation, wrote)],
    })
}

/// The exit is the original failure's, whatever a step chosen afterwards does.
pub fn report(error: &anyhow::Error, invocation: &Invocation) -> Exit {
    let exit = Exit::of(error);
    let wrote = WROTE.load(Ordering::Relaxed);
    let diagnostic = error
        .downcast_ref::<Failed>()
        .and_then(Failed::diagnostic)
        .cloned()
        .unwrap_or_else(|| fallback(error, exit, invocation, wrote));

    let mut stderr = std::io::stderr().lock();
    if invocation.json {
        let _ = writeln!(
            stderr,
            "{}",
            serde_json::to_string(&diagnostic).unwrap_or_default()
        );
        return exit;
    }
    let steps = step::steps(&diagnostic, invocation, wrote);
    let _ = write!(
        stderr,
        "error: {}\n{}",
        message(&diagnostic),
        step::described(&diagnostic, &steps, invocation)
    );
    if std::io::stdin().is_terminal() && stderr.is_terminal() {
        let chosen = offer::chosen(
            &steps,
            invocation,
            &mut std::io::stdin().lock(),
            &mut stderr,
        );
        drop(stderr);
        if let Err(error) = chosen.and_then(|argv| argv.map(offer::launched).transpose()) {
            eprintln!("error: {error:#}");
        }
    }
    exit
}

/// A Session's durable failure, as part of a successful read.
pub fn inline(diagnostic: &wire::Diagnostic, invocation: &Invocation) -> String {
    let steps = step::steps(diagnostic, invocation, false);
    format!(
        "{}\n{}",
        message(diagnostic),
        step::described(diagnostic, &steps, invocation)
    )
    .trim_end()
    .to_owned()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub fn invocation() -> Invocation {
        Invocation {
            control_plane: "http://127.0.0.1:7718".to_owned(),
            elsewhere: false,
            operation: "workspace open".to_owned(),
            args: [
                "workspace",
                "open",
                "--project",
                "absent",
                "--agent",
                "worker",
            ]
            .map(str::to_owned)
            .to_vec(),
            json: false,
        }
    }

    pub fn diagnostic(value: serde_json::Value) -> wire::Diagnostic {
        serde_json::from_value(value).expect("a diagnostic")
    }

    pub fn missing_project() -> wire::Diagnostic {
        diagnostic(json!({
            "kind": "missing_reference",
            "message": "no project named absent in the organization Acme East",
            "field": null,
            "context": { "resource": "project", "reference": "absent", "organization": "Acme East" },
            "next_steps": [{
                "action": "declare_project",
                "organization": "Acme East",
                "name": "absent",
                "repositories": null,
                "branch": null,
                "missing": ["repositories", "branch"],
            }],
        }))
    }

    pub fn stopping() -> wire::Diagnostic {
        diagnostic(json!({
            "kind": "state_conflict",
            "message": "the workspace w already has the session s",
            "field": null,
            "context": {
                "operation": "enqueue_session",
                "resource": "workspace",
                "reference": "w",
                "organization": "acme",
                "state": "session_in_flight",
                "holding_session": "s",
            },
            "next_steps": [
                {
                    "action": "inspect_resource",
                    "resource": "session",
                    "reference": "s",
                    "organization": "acme",
                },
                {
                    "action": "stop_session",
                    "organization": "acme",
                    "session": "s",
                    "consequence": "Stopping the session now records it failed.",
                    "effect": "fails_session",
                    "requires_choice": true,
                },
            ],
        }))
    }

    pub fn rendered(diagnostic: &wire::Diagnostic, invocation: &Invocation, wrote: bool) -> String {
        step::described(
            diagnostic,
            &step::steps(diagnostic, invocation, wrote),
            invocation,
        )
    }

    const READ: Request<'static> = Request {
        operation: "session show",
        write: false,
    };
    const WRITE: Request<'static> = Request {
        operation: "session stop",
        write: true,
    };

    #[test]
    fn the_kind_decides_the_exit_whatever_the_status() {
        let cases = [
            (missing_project(), StatusCode::CONFLICT, Exit::Unresolved),
            (stopping(), StatusCode::NOT_FOUND, Exit::Rejected),
            (
                diagnostic(json!({
                    "kind": "setup_gap",
                    "message": "name the Operator",
                    "field": null,
                    "context": {
                        "prerequisite": "operator",
                        "resource": null,
                        "reference": null,
                        "organization": null,
                        "harness": null,
                        "method": null,
                        "sign_in": null,
                    },
                    "next_steps": [{ "action": "name_operator", "name": null, "missing": ["name"] }],
                })),
                StatusCode::CONFLICT,
                Exit::NotReady,
            ),
        ];

        for (diagnostic, status, exit) in cases {
            assert_eq!(exit_for(&diagnostic, status), exit);
        }
    }

    #[test]
    fn rewording_a_refusal_changes_nothing_but_its_sentence() {
        let mut reworded = serde_json::to_value(missing_project()).expect("json");
        reworded["message"] = json!("there is no such Project as the stop session thing");
        let reworded = diagnostic(reworded);

        assert_eq!(
            exit_for(&reworded, StatusCode::NOT_FOUND),
            exit_for(&missing_project(), StatusCode::NOT_FOUND)
        );
        assert_eq!(
            rendered(&reworded, &invocation(), false),
            rendered(&missing_project(), &invocation(), false)
        );
    }

    #[test]
    fn an_unrecognised_answer_is_bounded_evidence_with_an_inspection() {
        let failed = refused(
            StatusCode::BAD_GATEWAY,
            None,
            format!("<html>\u{1b}[31m{}</html>", "x".repeat(1000)).as_bytes(),
            WRITE,
        );

        assert_eq!(failed.exit(), Exit::Unavailable);
        let said = rendered(
            failed.diagnostic().expect("a diagnostic"),
            &invocation(),
            true,
        );
        assert!(said.contains("may already have taken effect"), "{said}");
        assert!(!said.contains('\u{1b}'), "{said}");
        assert!(said.len() < 600, "{said}");
    }

    #[test]
    fn a_lost_read_offers_a_retry_with_its_delay_and_a_lost_write_never_does() {
        let read = refused(StatusCode::SERVICE_UNAVAILABLE, Some(3), b"", READ);
        let said = rendered(
            read.diagnostic().expect("a diagnostic"),
            &invocation(),
            false,
        );
        assert!(said.contains("read again after 3 seconds"), "{said}");

        let write = unreachable("http://127.0.0.1:7718/", WRITE, true);
        let said = rendered(
            write.diagnostic().expect("a diagnostic"),
            &invocation(),
            true,
        );
        assert!(!said.contains("read again"), "{said}");
        assert!(said.contains("may already have taken effect"), "{said}");
        assert!(
            said.contains("at http://127.0.0.1:7718 is running"),
            "{said}"
        );
    }

    #[test]
    fn a_retry_after_a_write_is_said_and_never_offered_to_run() {
        let read = refused(StatusCode::SERVICE_UNAVAILABLE, None, b"", READ);

        let said = rendered(
            read.diagnostic().expect("a diagnostic"),
            &invocation(),
            true,
        );

        assert!(said.contains("read again"), "{said}");
        assert!(!said.contains("kestrel workspace open"), "{said}");
    }

    #[test]
    fn a_plain_refusal_keeps_its_sentence_and_its_status() {
        let failed = refused(
            StatusCode::CONFLICT,
            None,
            br#"{"message":"that name is taken"}"#,
            WRITE,
        );

        assert_eq!(failed.exit(), Exit::Rejected);
        assert_eq!(
            message(failed.diagnostic().expect("a diagnostic")),
            "that name is taken"
        );
    }

    #[test]
    fn an_unread_answer_is_not_a_connection_failure() {
        let error = anyhow::anyhow!(serde_json::Error::io(std::io::Error::other("eof"))).context(
            Failed::new(Exit::Unavailable, "reading the applied triggers"),
        );

        let diagnostic = fallback(&error, Exit::Unavailable, &invocation(), false);

        let value = serde_json::to_value(&diagnostic).expect("json");
        assert_eq!(value["kind"], "unknown_response");
        assert_eq!(value["message"], "reading the applied triggers");
    }

    #[test]
    fn a_local_failure_before_any_write_cannot_have_taken_effect() {
        let error = anyhow::anyhow!("writing to standard output");

        let before = rendered(
            &fallback(&error, Exit::Failure, &invocation(), false),
            &invocation(),
            false,
        );
        let after = rendered(
            &fallback(&error, Exit::Failure, &invocation(), true),
            &invocation(),
            true,
        );

        assert!(
            !before.contains("may already have taken effect"),
            "{before}"
        );
        assert!(after.contains("may already have taken effect"), "{after}");
    }
}
