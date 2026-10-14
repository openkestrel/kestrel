use axum::extract::Request;
use axum::extract::rejection::JsonRejection;
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::{BoxError, Json};
use kestrel_operator_types as wire;
use tracing::warn;

use crate::declined::{
    self as declined, Concerning, Consequence, Declined, Locator, Reason, Resource, Step, Within,
};
use crate::log::Unreadable;
use crate::role::serve;
use crate::store;

use super::{diagnosed, inspect_operation, invalid_value, operation, retry_read};

pub(super) enum Refused {
    /// A request the boundary could not read; its operation is the route's.
    Malformed {
        field: Option<&'static str>,
        why: String,
    },
    Forbidden(String),
    NotFound(String),
    Conflict(String),
    Unprocessable(String),
    Diagnosed {
        status: StatusCode,
        diagnostic: Box<wire::Diagnostic>,
    },
    /// The control plane could not answer; its operation is the route's.
    Unavailable(anyhow::Error),
}

pub(super) fn malformed(field: Option<&'static str>, why: impl Into<String>) -> Refused {
    Refused::Malformed {
        field,
        why: why.into(),
    }
}

impl Refused {
    pub(super) fn into_error(self) -> BoxError {
        match self {
            Refused::Malformed { why, .. }
            | Refused::Forbidden(why)
            | Refused::NotFound(why)
            | Refused::Conflict(why)
            | Refused::Unprocessable(why) => why.into(),
            Refused::Diagnosed { diagnostic, .. } => {
                diagnostic_message(&diagnostic).to_owned().into()
            }
            Refused::Unavailable(error) => error.into(),
        }
    }
}

/// Never offered for an ambiguous reference: declaring another record cannot resolve one.
pub(super) fn declare_or_inspect(
    resource: Resource,
    reference: &str,
    organization: Option<&str>,
) -> Vec<wire::Action> {
    match (resource, organization) {
        (Resource::Organization, _) => vec![wire::Action::DeclareOrganizationAction(
            wire::DeclareOrganizationAction {
                action: serde_json::json!("declare_organization"),
                name: Some(reference.to_owned()),
                missing: Vec::new(),
            },
        )],
        (Resource::Project, Some(organization)) => vec![wire::Action::DeclareProjectAction(
            wire::DeclareProjectAction {
                action: serde_json::json!("declare_project"),
                organization: organization.to_owned(),
                name: Some(reference.to_owned()),
                repositories: None,
                branch: None,
                missing: vec!["repositories".to_owned(), "branch".to_owned()],
            },
        )],
        (Resource::Agent, Some(organization)) => {
            vec![wire::Action::DeclareAgentAction(wire::DeclareAgentAction {
                action: serde_json::json!("declare_agent"),
                organization: organization.to_owned(),
                name: Some(reference.to_owned()),
                harness: None,
                missing: vec!["harness".to_owned()],
            })]
        }
        (Resource::SubscriptionProfile, Some(organization)) => {
            vec![wire::Action::DeclareSubscriptionProfileAction(
                wire::DeclareSubscriptionProfileAction {
                    action: serde_json::json!("declare_subscription_profile"),
                    organization: organization.to_owned(),
                    name: Some(reference.to_owned()),
                    owner: None,
                    missing: vec!["owner".to_owned()],
                },
            )]
        }
        (Resource::ProviderCredential, Some(organization)) => {
            vec![wire::Action::SetProviderCredentialAction(
                wire::SetProviderCredentialAction {
                    action: serde_json::json!("set_provider_credential"),
                    organization: organization.to_owned(),
                    name: reference.to_owned(),
                },
            )]
        }
        (resource, organization) => list_resources(resource, organization),
    }
}

pub(super) fn list_resources(resource: Resource, organization: Option<&str>) -> Vec<wire::Action> {
    vec![wire::Action::ListResourcesAction(
        wire::ListResourcesAction {
            action: serde_json::json!("list_resources"),
            organization: organization.map(str::to_owned),
            resource: wire_resource(resource),
        },
    )]
}

pub(super) fn inspect(locator: Locator, organization: Option<String>) -> wire::Action {
    wire::Action::InspectResourceAction(wire::InspectResourceAction {
        action: serde_json::json!("inspect_resource"),
        resource: wire_resource(locator.resource),
        reference: Some(locator.reference),
        organization,
    })
}

/// A step needing an Organization is never offered without one, though every reason that
/// carries one is scoped.
pub(super) fn next_steps(next: declined::Next, organization: Option<&str>) -> Vec<wire::Action> {
    let mut steps = vec![inspect(next.inspect, organization.map(str::to_owned))];
    let Some(organization) = organization else {
        return steps;
    };
    steps.extend(next.then.into_iter().map(|step| match step {
        Step::StopSession {
            session,
            consequence,
        } => wire::Action::StopSessionAction(wire::StopSessionAction {
            action: serde_json::json!("stop_session"),
            organization: organization.to_owned(),
            session,
            consequence: explained(consequence).to_owned(),
            effect: wire_consequence(consequence),
            requires_choice: serde_json::json!(true),
        }),
        Step::EnqueueSession { workspace } => {
            wire::Action::EnqueueSessionAction(wire::EnqueueSessionAction {
                action: serde_json::json!("enqueue_session"),
                organization: organization.to_owned(),
                workspace,
                missing: Vec::new(),
            })
        }
        Step::EnableIntegration { integration } => {
            wire::Action::EnableIntegrationAction(wire::EnableIntegrationAction {
                action: serde_json::json!("enable_integration"),
                organization: organization.to_owned(),
                integration,
            })
        }
        Step::ReplacePrivateKey { integration } => {
            replace_private_key(organization.to_owned(), integration, None)
        }
        Step::ReleaseInstance {
            workspace,
            instance,
        } => wire::Action::ReleaseInstanceAction(wire::ReleaseInstanceAction {
            action: serde_json::json!("release_instance"),
            organization: organization.to_owned(),
            workspace,
            instance: Some(instance),
            consequence: explained(Consequence::DiscardsUnpublishedWork).to_owned(),
            effect: wire_consequence(Consequence::DiscardsUnpublishedWork),
            requires_choice: serde_json::json!(true),
        }),
    }));
    steps
}

pub(super) fn replace_private_key(
    organization: String,
    integration: String,
    retry_after_seconds: Option<i64>,
) -> wire::Action {
    wire::Action::ReplaceIntegrationPrivateKeyAction(wire::ReplaceIntegrationPrivateKeyAction {
        action: serde_json::json!("replace_integration_private_key"),
        organization,
        integration,
        retry_after_seconds,
    })
}

pub(super) const fn explained(consequence: Consequence) -> &'static str {
    match consequence {
        Consequence::FailsSession => "Stopping the session now records it failed.",
        Consequence::EndsSession => "Stopping the session ends it; what it has done stands.",
        Consequence::DiscardsUnpublishedWork => {
            "Releasing the instance discards work that may exist nowhere else."
        }
    }
}

pub(super) const fn wire_consequence(consequence: Consequence) -> wire::Consequence {
    match consequence {
        Consequence::FailsSession => wire::Consequence::FailsSession,
        Consequence::EndsSession => wire::Consequence::EndsSession,
        Consequence::DiscardsUnpublishedWork => wire::Consequence::DiscardsUnpublishedWork,
    }
}

pub(super) const fn wire_resource(resource: Resource) -> wire::Resource {
    match resource {
        Resource::Organization => wire::Resource::Organization,
        Resource::Project => wire::Resource::Project,
        Resource::Agent => wire::Resource::Agent,
        Resource::SubscriptionProfile => wire::Resource::SubscriptionProfile,
        Resource::ProviderCredential => wire::Resource::ProviderCredential,
        Resource::Integration => wire::Resource::Integration,
        Resource::Trigger => wire::Resource::Trigger,
        Resource::Event => wire::Resource::Event,
        Resource::Workspace => wire::Resource::Workspace,
        Resource::Session => wire::Resource::Session,
        Resource::Instance => wire::Resource::Instance,
        Resource::HeldMessage => wire::Resource::HeldMessage,
        Resource::TranscriptPayload => wire::Resource::TranscriptPayload,
        Resource::SignInTest => wire::Resource::SignInTest,
    }
}

pub(super) fn diagnostic_message(diagnostic: &wire::Diagnostic) -> &str {
    match diagnostic {
        wire::Diagnostic::MissingReferenceDiagnostic(d) => &d.message,
        wire::Diagnostic::AmbiguousReferenceDiagnostic(d) => &d.message,
        wire::Diagnostic::MalformedRequestDiagnostic(d) => &d.message,
        wire::Diagnostic::ForbiddenActionDiagnostic(d) => &d.message,
        wire::Diagnostic::StateConflictDiagnostic(d) => &d.message,
        wire::Diagnostic::ExpiredResourceDiagnostic(d) => &d.message,
        wire::Diagnostic::InvalidFieldDiagnostic(d) => &d.message,
        wire::Diagnostic::SetupGapDiagnostic(d) => &d.message,
        wire::Diagnostic::UnavailableDiagnostic(d) => &d.message,
        wire::Diagnostic::InstanceTimeoutDiagnostic(d) => &d.message,
        wire::Diagnostic::CredentialRejectedDiagnostic(d) => &d.message,
        wire::Diagnostic::AuthenticationFailedDiagnostic(d) => &d.message,
        wire::Diagnostic::ExecutableMissingDiagnostic(d) => &d.message,
        wire::Diagnostic::UnknownFailureDiagnostic(d) => &d.message,
        wire::Diagnostic::ConnectionFailedDiagnostic(d) => &d.message,
        wire::Diagnostic::ClientFailureDiagnostic(d) => &d.message,
        wire::Diagnostic::UnknownResponseDiagnostic(d) => &d.message,
    }
}

impl From<anyhow::Error> for Refused {
    fn from(error: anyhow::Error) -> Self {
        let error = match error.downcast::<Reason>() {
            Ok(reason) => return diagnosed(reason, None),
            Err(error) => error,
        };
        let error = match error.downcast::<Concerning>() {
            Ok(concerning) => {
                return diagnosed(concerning.reason, Some(concerning.field.to_owned()));
            }
            Err(error) => error,
        };
        let error = match error.downcast::<Within>() {
            Ok(Within {
                record,
                field,
                reason,
            }) => {
                let path = |field: &str| format!("{record}.{field}");
                return match reason {
                    Reason::InvalidField {
                        field,
                        operation,
                        constraint,
                        allowed,
                        message,
                    } => {
                        let (status, diagnostic) =
                            invalid_value(operation, &path(field), constraint, allowed, message);
                        Refused::Diagnosed {
                            status,
                            diagnostic: Box::new(diagnostic),
                        }
                    }
                    reason => diagnosed(reason, field.map(path)),
                };
            }
            Err(error) => error,
        };
        match error.downcast::<Declined>() {
            Ok(Declined::Unacceptable(why)) => Refused::Unprocessable(why),
            Ok(Declined::Missing(why) | Declined::Ambiguous(why)) => Refused::NotFound(why),
            Ok(Declined::Taken(why)) => Refused::Conflict(why),
            Err(error) => Refused::Unavailable(error),
        }
    }
}

impl From<JsonRejection> for Refused {
    fn from(rejection: JsonRejection) -> Self {
        Refused::Malformed {
            field: None,
            why: rejection.body_text(),
        }
    }
}

impl From<Unreadable> for Refused {
    fn from(unreadable: Unreadable) -> Self {
        match unreadable {
            Unreadable::Cursor(why) => Refused::Malformed { field: None, why },
            Unreadable::Unavailable(error) => Refused::Unavailable(error),
        }
    }
}

/// What a refusal raised without its operation leaves for `diagnosing`, which knows the route.
#[derive(Clone)]
pub(super) enum Undiagnosed {
    Malformed {
        field: Option<&'static str>,
        message: String,
    },
    Unavailable {
        busy: bool,
    },
}

impl IntoResponse for Refused {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            Refused::Diagnosed { status, diagnostic } => {
                return (status, Json(diagnostic)).into_response();
            }
            Refused::Malformed { field, why } => {
                let mut response = StatusCode::BAD_REQUEST.into_response();
                response.extensions_mut().insert(Undiagnosed::Malformed {
                    field,
                    message: why,
                });
                return response;
            }
            Refused::Unavailable(error) => {
                let busy = store::busy(&error);
                warn!(%error, busy, "the operator boundary could not answer");
                let mut response = StatusCode::SERVICE_UNAVAILABLE.into_response();
                response
                    .extensions_mut()
                    .insert(Undiagnosed::Unavailable { busy });
                return response;
            }
            Refused::Forbidden(why) => (StatusCode::FORBIDDEN, why),
            Refused::NotFound(why) => (StatusCode::NOT_FOUND, why),
            Refused::Conflict(why) => (StatusCode::CONFLICT, why),
            Refused::Unprocessable(why) => (StatusCode::UNPROCESSABLE_ENTITY, why),
        };

        (
            status,
            Json(wire::Refusal {
                field: None,
                message,
            }),
        )
            .into_response()
    }
}

/// A malformed request or an unanswerable one is diagnosed against the route it reached, so
/// every handler need not name its own operation.
pub(super) async fn diagnosing(request: Request, next: Next) -> Response {
    let read = request.method() == axum::http::Method::GET;
    let operation = request
        .extensions()
        .get::<axum::extract::MatchedPath>()
        .and_then(|path| operation(request.method().as_str(), path.as_str()))
        .unwrap_or("unknown");
    let response = next.run(request).await;
    let Some(undiagnosed) = response.extensions().get::<Undiagnosed>().cloned() else {
        return response;
    };

    let (status, busy, diagnostic) = match undiagnosed {
        Undiagnosed::Malformed { field, message } => (
            StatusCode::BAD_REQUEST,
            false,
            wire::Diagnostic::MalformedRequestDiagnostic(wire::MalformedRequestDiagnostic {
                kind: serde_json::json!("malformed_request"),
                message,
                field: field.map(str::to_owned),
                context: wire::MalformedRequestContext {
                    operation: operation.to_owned(),
                    field: field.map(str::to_owned),
                },
                next_steps: vec![match field {
                    Some(field) => wire::Action::CorrectFieldAction(wire::CorrectFieldAction {
                        action: serde_json::json!("correct_field"),
                        operation: operation.to_owned(),
                        resource: None,
                        field: field.to_owned(),
                        constraint: "well_formed".to_owned(),
                        allowed_values: None,
                    }),
                    None => inspect_operation(operation, false),
                }],
            }),
        ),
        Undiagnosed::Unavailable { busy } => (
            StatusCode::SERVICE_UNAVAILABLE,
            busy,
            wire::Diagnostic::UnavailableDiagnostic(wire::UnavailableDiagnostic {
                kind: serde_json::json!("unavailable"),
                message: "the control plane could not answer".to_owned(),
                field: None,
                context: wire::UnavailableContext {
                    service: "control_plane".to_owned(),
                    resource: None,
                    operation: operation.to_owned(),
                    retry_after_seconds: busy.then_some(serve::BUSY_RETRY_AFTER_SECONDS),
                    saved: None,
                },
                // A write that went unanswered may have landed, so it is inspected, never replayed.
                next_steps: vec![if read {
                    retry_read(
                        operation,
                        None,
                        busy.then_some(serve::BUSY_RETRY_AFTER_SECONDS),
                    )
                } else {
                    inspect_operation(operation, true)
                }],
            }),
        ),
    };

    serve::refusal(status, busy, Json(diagnostic))
}
