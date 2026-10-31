use kestrel_operator_types as wire;
use reqwest::{Response, StatusCode, header};
use serde_json::json;

use crate::exit::{Exit, Failed};

use super::kind::{by_status, exit_for};

const SERVICE: &str = "control plane";
const EVIDENCE: usize = 240;

#[derive(Clone, Copy)]
pub struct Request<'a> {
    pub operation: &'a str,
    pub write: bool,
}

impl Request<'_> {
    /// An answer lost after the request was sent: a write may have landed, a read may be retried.
    pub(super) fn lost(self, retry_after: Option<i64>) -> wire::Action {
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

pub(super) fn refused(
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

pub(super) fn unknown_response(
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

pub(super) fn inspect_operation(operation: &str, uncertain: bool) -> wire::Action {
    wire::Action::InspectOperationAction(wire::InspectOperationAction {
        action: json!("inspect_operation"),
        operation: operation.to_owned(),
        resource: None,
        uncertain,
    })
}

/// Evidence is a bounded single line, so an answer can neither flood a terminal nor steer it.
pub(super) fn bounded(text: &str) -> Option<String> {
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
