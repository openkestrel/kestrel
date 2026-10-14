use axum::http::StatusCode;
use kestrel_operator_types as wire;

use crate::declined::{Again, Constraint, Reason, Resource};

use super::{
    Refused, declare_or_inspect, inspect, list_resources, next_steps, replace_private_key,
    wire_resource,
};

pub(super) fn diagnosed(reason: Reason, field: Option<String>) -> Refused {
    let (status, diagnostic) = diagnosis(reason, field);
    Refused::Diagnosed {
        status,
        diagnostic: Box::new(diagnostic),
    }
}

pub(super) fn diagnosis(reason: Reason, field: Option<String>) -> (StatusCode, wire::Diagnostic) {
    let request_field = field.clone();
    match reason {
        Reason::MissingOperator { operation } => (
            if operation == "show_operator" {
                StatusCode::NOT_FOUND
            } else {
                StatusCode::CONFLICT
            },
            wire::Diagnostic::SetupGapDiagnostic(wire::SetupGapDiagnostic {
                kind: serde_json::json!("setup_gap"),
                message: Reason::MissingOperator { operation }.to_string(),
                field,
                context: wire::SetupGapContext {
                    prerequisite: "operator".to_owned(),
                    resource: None,
                    reference: None,
                    organization: None,
                    harness: None,
                    method: None,
                    sign_in: None,
                    image: None,
                },
                next_steps: vec![wire::Action::NameOperatorAction(wire::NameOperatorAction {
                    action: serde_json::json!("name_operator"),
                    name: None,
                    missing: vec!["name".to_owned()],
                })],
            }),
        ),
        Reason::MissingReference {
            resource,
            reference,
            organization,
            within,
            message,
        } => (
            StatusCode::NOT_FOUND,
            wire::Diagnostic::MissingReferenceDiagnostic(wire::MissingReferenceDiagnostic {
                kind: serde_json::json!("missing_reference"),
                message,
                field,
                next_steps: match within {
                    Some(within) => vec![inspect(within, organization.clone())],
                    None => declare_or_inspect(resource, &reference, organization.as_deref()),
                },
                context: wire::MissingReferenceContext {
                    resource: wire_resource(resource),
                    reference,
                    organization,
                },
            }),
        ),
        Reason::AmbiguousReference {
            resource,
            reference,
            organization,
            candidates,
            message,
        } => (
            StatusCode::NOT_FOUND,
            wire::Diagnostic::AmbiguousReferenceDiagnostic(wire::AmbiguousReferenceDiagnostic {
                kind: serde_json::json!("ambiguous_reference"),
                message,
                field,
                next_steps: list_resources(resource, organization.as_deref()),
                context: wire::AmbiguousReferenceContext {
                    resource: wire_resource(resource),
                    reference,
                    organization,
                    candidates: candidates
                        .into_iter()
                        .map(|candidate| wire::Candidate {
                            id: candidate.id,
                            name: candidate.name,
                        })
                        .collect(),
                },
            }),
        ),
        Reason::InvalidField {
            field,
            operation,
            constraint,
            allowed,
            message,
        } => invalid_value(operation, field, constraint, allowed, message),
        Reason::SignInBusy(busy) => {
            let message = busy.to_string();
            let holding_session = busy.holder.session().map(str::to_owned);
            diagnosis(
                Reason::StateConflict {
                    operation: crate::sign_in::credential_use::ACQUIRE,
                    resource: Resource::SubscriptionProfile,
                    reference: busy.profile,
                    organization: Some(busy.organization),
                    state: "credential_in_use",
                    holding_session,
                    next: busy.next,
                    message,
                },
                request_field,
            )
        }
        Reason::StateConflict {
            operation,
            resource,
            reference,
            organization,
            state,
            holding_session,
            next,
            message,
        } => (
            StatusCode::CONFLICT,
            wire::Diagnostic::StateConflictDiagnostic(wire::StateConflictDiagnostic {
                kind: serde_json::json!("state_conflict"),
                message,
                field,
                next_steps: next_steps(next, organization.as_deref()),
                context: wire::StateConflictContext {
                    operation: operation.to_owned(),
                    resource: wire_resource(resource),
                    reference,
                    organization,
                    state: state.to_owned(),
                    holding_session,
                },
            }),
        ),
        Reason::Forbidden {
            operation,
            resource,
            reference,
            organization,
            constraint,
            next,
            message,
        } => (
            StatusCode::FORBIDDEN,
            wire::Diagnostic::ForbiddenActionDiagnostic(wire::ForbiddenActionDiagnostic {
                kind: serde_json::json!("forbidden_action"),
                message,
                field,
                next_steps: next_steps(next, organization.as_deref()),
                context: wire::ForbiddenActionContext {
                    operation: operation.to_owned(),
                    resource: wire_resource(resource),
                    reference,
                    organization,
                    constraint: Some(constraint.to_owned()),
                },
            }),
        ),
        Reason::Expired {
            operation,
            resource,
            reference,
            organization,
            next,
            message,
        } => (
            StatusCode::GONE,
            wire::Diagnostic::ExpiredResourceDiagnostic(wire::ExpiredResourceDiagnostic {
                kind: serde_json::json!("expired_resource"),
                message,
                field,
                next_steps: next_steps(next, organization.as_deref()),
                context: wire::ExpiredResourceContext {
                    resource: wire_resource(resource),
                    reference,
                    organization,
                    operation: operation.to_owned(),
                },
            }),
        ),
        Reason::InstanceTimeout {
            operation,
            workspace,
            instance,
            message,
        } => (
            StatusCode::GATEWAY_TIMEOUT,
            wire::Diagnostic::InstanceTimeoutDiagnostic(wire::InstanceTimeoutDiagnostic {
                kind: serde_json::json!("instance_timeout"),
                message,
                field,
                next_steps: vec![retry_read(operation, Some(Resource::Workspace), None)],
                context: wire::InstanceTimeoutContext {
                    workspace,
                    instance: Some(instance),
                    operation: operation.to_owned(),
                },
            }),
        ),
        Reason::CredentialRejected {
            operation: _,
            organization,
            harness,
            method,
            provider,
            status,
            provider_error,
            message,
        } => (
            StatusCode::UNPROCESSABLE_ENTITY,
            wire::Diagnostic::CredentialRejectedDiagnostic(wire::CredentialRejectedDiagnostic {
                kind: serde_json::json!("credential_rejected"),
                message,
                field: Some(field.unwrap_or_else(|| "value".to_owned())),
                next_steps: vec![wire::Action::SignInAction(wire::SignInAction {
                    action: serde_json::json!("sign_in"),
                    harness: Some(harness.clone()),
                    method: Some(method.clone()),
                    sign_in: None,
                })],
                context: wire::CredentialRejectedContext {
                    organization,
                    harness,
                    method,
                    provider: provider.to_owned(),
                    status: status.map(i64::from),
                    provider_error,
                },
            }),
        ),
        Reason::HarnessNotCarried {
            operation,
            harness,
            image,
            carried,
            message,
        } => (
            StatusCode::CONFLICT,
            wire::Diagnostic::SetupGapDiagnostic(wire::SetupGapDiagnostic {
                kind: serde_json::json!("setup_gap"),
                message,
                next_steps: vec![
                    inspect_harness_image(Some(harness.clone()), image.clone()),
                    wire::Action::CorrectFieldAction(wire::CorrectFieldAction {
                        action: serde_json::json!("correct_field"),
                        operation: operation.to_owned(),
                        resource: None,
                        field: field.clone().unwrap_or_else(|| "harness".to_owned()),
                        constraint: "carried_by_image".to_owned(),
                        allowed_values: Some(carried),
                    }),
                ],
                field,
                context: wire::SetupGapContext {
                    prerequisite: "harness_in_image".to_owned(),
                    resource: None,
                    reference: None,
                    organization: None,
                    harness: Some(harness),
                    method: None,
                    sign_in: None,
                    image: Some(image),
                },
            }),
        ),
        Reason::GithubUnavailable {
            operation,
            integration,
            organization,
            retry_after_seconds,
            again,
            message,
        } => (
            StatusCode::SERVICE_UNAVAILABLE,
            wire::Diagnostic::UnavailableDiagnostic(wire::UnavailableDiagnostic {
                kind: serde_json::json!("unavailable"),
                message,
                field,
                context: wire::UnavailableContext {
                    service: "github".to_owned(),
                    resource: Some(integration.clone()),
                    operation: operation.to_owned(),
                    retry_after_seconds,
                    saved: None,
                },
                next_steps: vec![match again {
                    Again::ReplacePrivateKey => {
                        replace_private_key(organization, integration, retry_after_seconds)
                    }
                    Again::Read => {
                        retry_read(operation, Some(Resource::Integration), retry_after_seconds)
                    }
                    Again::Request => wire::Action::RetryRequestAction(wire::RetryRequestAction {
                        action: serde_json::json!("retry_request"),
                        operation: operation.to_owned(),
                        resource: Some(integration),
                        retry_after_seconds,
                    }),
                }],
            }),
        ),
        Reason::ImageUnavailable {
            operation,
            harness,
            image,
            message,
        } => (
            StatusCode::SERVICE_UNAVAILABLE,
            wire::Diagnostic::UnavailableDiagnostic(image_unavailable(
                operation, harness, image, message, field,
            )),
        ),
    }
}

pub(super) fn inspect_harness_image(harness: Option<String>, image: String) -> wire::Action {
    wire::Action::InspectHarnessImageAction(wire::InspectHarnessImageAction {
        action: serde_json::json!("inspect_harness_image"),
        harness,
        image: Some(image),
        command: None,
    })
}

/// Nothing was written, so the step after inspecting the image is to read its capabilities
/// again, never to replay a write blind.
pub(super) fn image_unavailable(
    operation: &str,
    harness: Option<String>,
    image: String,
    message: String,
    field: Option<String>,
) -> wire::UnavailableDiagnostic {
    wire::UnavailableDiagnostic {
        kind: serde_json::json!("unavailable"),
        message,
        field,
        context: wire::UnavailableContext {
            service: "image_inspection".to_owned(),
            resource: Some(image.clone()),
            operation: operation.to_owned(),
            retry_after_seconds: None,
            saved: None,
        },
        next_steps: vec![
            inspect_harness_image(harness, image),
            retry_read("list_harnesses", None, None),
        ],
    }
}

pub(super) fn retry_read(
    operation: &str,
    resource: Option<Resource>,
    retry_after_seconds: Option<i64>,
) -> wire::Action {
    wire::Action::RetryReadAction(wire::RetryReadAction {
        action: serde_json::json!("retry_read"),
        operation: operation.to_owned(),
        resource: resource.map(|resource| wire_resource(resource).as_str().to_owned()),
        retry_after_seconds,
    })
}

pub(super) fn invalid_field(
    operation: &'static str,
    field: &'static str,
    constraint: Constraint,
    message: impl Into<String>,
) -> Refused {
    let (status, diagnostic) = invalid_value(operation, field, constraint, None, message.into());
    Refused::Diagnosed {
        status,
        diagnostic: Box::new(diagnostic),
    }
}

pub(super) fn invalid_value(
    operation: &'static str,
    field: &str,
    constraint: Constraint,
    allowed: Option<Vec<String>>,
    message: String,
) -> (StatusCode, wire::Diagnostic) {
    (
        StatusCode::UNPROCESSABLE_ENTITY,
        wire::Diagnostic::InvalidFieldDiagnostic(wire::InvalidFieldDiagnostic {
            kind: serde_json::json!("invalid_field"),
            message,
            field: Some(field.to_owned()),
            context: wire::InvalidFieldContext {
                field: field.to_owned(),
                constraint: constraint.as_str().to_owned(),
                allowed_values: allowed.clone(),
            },
            next_steps: vec![wire::Action::CorrectFieldAction(wire::CorrectFieldAction {
                action: serde_json::json!("correct_field"),
                operation: operation.to_owned(),
                resource: None,
                field: field.to_owned(),
                constraint: constraint.as_str().to_owned(),
                allowed_values: allowed,
            })],
        }),
    )
}
