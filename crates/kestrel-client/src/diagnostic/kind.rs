use kestrel_operator_types as wire;
use reqwest::StatusCode;

use crate::exit::{Exit, Failed};

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
            D::CredentialRejectedDiagnostic($d) => $body,
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

pub fn after_save(error: anyhow::Error, saved: &str) -> anyhow::Error {
    let Some(mut diagnostic) = error
        .downcast_ref::<Failed>()
        .and_then(Failed::diagnostic)
        .cloned()
    else {
        return error.context(saved.to_owned());
    };
    each_kind!(&mut diagnostic, d => {
        d.message = format!("{saved}. {}", d.message);
    });
    anyhow::anyhow!(Failed::diagnosed(Exit::of(&error), diagnostic))
}

pub(super) fn next_steps(diagnostic: &wire::Diagnostic) -> &[wire::Action] {
    each_kind!(diagnostic, d => &d.next_steps)
}

/// The kind decides the exit; the status only places the kinds a Client cannot classify.
pub(super) fn exit_for(diagnostic: &wire::Diagnostic, status: StatusCode) -> Exit {
    use wire::Diagnostic as D;
    match diagnostic {
        D::MissingReferenceDiagnostic(_) | D::AmbiguousReferenceDiagnostic(_) => Exit::Unresolved,
        D::MalformedRequestDiagnostic(_)
        | D::ForbiddenActionDiagnostic(_)
        | D::StateConflictDiagnostic(_)
        | D::ExpiredResourceDiagnostic(_)
        | D::InvalidFieldDiagnostic(_)
        | D::CredentialRejectedDiagnostic(_) => Exit::Rejected,
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
pub(super) fn by_status(status: StatusCode) -> Exit {
    match status {
        StatusCode::NOT_FOUND => Exit::Unresolved,
        status if status.is_client_error() => Exit::Rejected,
        _ => Exit::Unavailable,
    }
}
