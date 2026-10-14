use axum::Json;
use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use kestrel_operator_types as wire;

use crate::capability::{self, Capability};
use crate::declined::{Locator, Resource};
use crate::domain::{self, Session};
use crate::profile::{self, Entry};
use crate::sign_in;
use crate::sign_in::handoff::{self, Attribution};

use super::{ControlPlane, Refused, image_unavailable, inspect, malformed, wire_resource};

pub(super) async fn harnesses(
    State(control_plane): State<ControlPlane>,
) -> Json<Vec<wire::HarnessCatalogueEntry>> {
    let capability = control_plane.images.read().await;
    Json(
        crate::catalogue::harnesses()
            .iter()
            .map(|row| wire::HarnessCatalogueEntry {
                availability: Some(availability(&capability, &row.name)),
                ..row.clone()
            })
            .collect(),
    )
}

pub(super) fn availability(capability: &Capability, harness: &str) -> wire::HarnessAvailability {
    let (state, image, identity, diagnostic) = match capability {
        Capability::Unchecked => (wire::HarnessAvailabilityState::Unchecked, None, None, None),
        Capability::Inspected {
            image,
            identity,
            harnesses,
        } => (
            if harnesses.contains(harness) {
                wire::HarnessAvailabilityState::Available
            } else {
                wire::HarnessAvailabilityState::NotCarried
            },
            Some(image.clone()),
            Some(identity.clone()),
            None,
        ),
        Capability::Unavailable { image, cause } => (
            wire::HarnessAvailabilityState::Unavailable,
            Some(image.clone()),
            None,
            Some(wire::Diagnostic::UnavailableDiagnostic(image_unavailable(
                "list_harnesses",
                Some(harness.to_owned()),
                image.clone(),
                capability::unavailable(image, cause, Some(harness)),
                None,
            ))),
        ),
    };
    wire::HarnessAvailability {
        state,
        image,
        identity,
        diagnostic,
    }
}

pub(super) async fn sign_in_method(
    Path((harness, method)): Path<(String, String)>,
) -> Result<Json<wire::SignInMethod>, Refused> {
    crate::catalogue::sign_in_method("show_sign_in_method", &harness, &method)
        .cloned()
        .map(Json)
        .map_err(Into::into)
}

pub(super) fn supplied_material(
    harness: &str,
    supplied: &domain::Supplied,
) -> wire::SuppliedMaterial {
    let entry = Entry {
        kind: supplied.kind,
        name: supplied.name.clone(),
    };
    wire::SuppliedMaterial {
        resource: match supplied.profile {
            Some(_) => wire::SuppliedMaterialResource::SubscriptionProfile,
            None => wire::SuppliedMaterialResource::ProviderCredential,
        },
        reference: supplied
            .profile
            .clone()
            .unwrap_or_else(|| supplied.name.clone()),
        kind: match supplied.kind {
            profile::Kind::Variable => wire::SuppliedMaterialKind::Variable,
            profile::Kind::File => wire::SuppliedMaterialKind::File,
        },
        name: supplied.name.clone(),
        method: handoff::slot(harness, &entry).map(|method| method.id.clone()),
        revision: supplied.revision,
        current: supplied.current,
        handed_at: supplied.handed_at.to_string(),
    }
}

pub(super) fn holding(
    method: &wire::SignInMethod,
    supplied: &domain::Supplied,
) -> wire::SignInHolding {
    wire::SignInHolding {
        resource: match supplied.profile {
            Some(_) => wire::SignInHoldingResource::SubscriptionProfile,
            None => wire::SignInHoldingResource::ProviderCredential,
        },
        reference: supplied
            .profile
            .clone()
            .unwrap_or_else(|| supplied.name.clone()),
        fills: method.fills.clone(),
    }
}

/// No image is recorded against a Session and no evidence a supervisor carries establishes
/// expiry or coverage, so none of the three is ever claimed.
pub(super) fn diagnosed_failure(
    session: &Session,
    organization: &str,
    evidence: domain::Evidence,
) -> wire::Diagnostic {
    let harness = session.agent.harness.clone();
    let inspect_session = inspect(
        Locator::new(Resource::Session, session.id.to_string()),
        Some(organization.to_owned()),
    );
    match evidence {
        domain::Evidence::AuthenticationRequired {
            code,
            methods,
            method,
        } => {
            let needed = format!("the {harness} harness needed a sign-in before it would work");
            let evidence = wire::AuthenticationRequiredEvidence {
                kind: serde_json::json!("authentication_required"),
                code: code.into(),
                methods,
                method: method.clone(),
            };
            let sign_in = |method: Option<String>, held_in: Option<String>| {
                wire::Action::SignInAction(wire::SignInAction {
                    action: serde_json::json!("sign_in"),
                    harness: Some(harness.clone()),
                    method,
                    sign_in: held_in,
                })
            };
            let unattributed = wire::AuthenticationFailedContext {
                session: session.id.to_string(),
                harness: harness.clone(),
                image: None,
                evidence,
                sign_in: session.sign_in_method.clone(),
                attribution: wire::AuthenticationFailedContextAttribution::Unattributed,
                candidates: Vec::new(),
                holding: None,
                revision: None,
                current: None,
                expired: None,
                covered: None,
            };
            let (message, next_steps, context) = match handoff::attribution(session) {
                Attribution::Established(used, supplied) => {
                    let holding = holding(used, &supplied);
                    (
                        format!(
                            "{needed}; it was handed the {} sign-in held in the {} {}{}",
                            used.name,
                            match supplied.profile {
                                Some(_) => Resource::SubscriptionProfile.noun(),
                                None => Resource::ProviderCredential.noun(),
                            },
                            holding.reference,
                            match supplied.current {
                                true => "",
                                false => ", which has been replaced since",
                            }
                        ),
                        vec![
                            sign_in(Some(used.id.clone()), Some(holding.reference.clone())),
                            inspect_session,
                        ],
                        wire::AuthenticationFailedContext {
                            sign_in: Some(used.id.clone()),
                            attribution: wire::AuthenticationFailedContextAttribution::Established,
                            holding: Some(holding),
                            revision: Some(supplied.revision),
                            current: Some(supplied.current),
                            ..unattributed
                        },
                    )
                }
                Attribution::Uncertain(candidates) => {
                    let ids: Vec<String> = candidates
                        .iter()
                        .map(|(method, _)| method.id.clone())
                        .collect();
                    let mut next_steps = Vec::new();
                    if let Some(profile) = candidates
                        .iter()
                        .find_map(|(_, supplied)| supplied.profile.clone())
                    {
                        next_steps.push(inspect(
                            Locator::new(Resource::SubscriptionProfile, profile),
                            Some(organization.to_owned()),
                        ));
                    }
                    next_steps.push(inspect_session);
                    (
                        format!(
                            "{needed}; it was handed several sign-ins ({}), and nothing \
                             established which one it refused",
                            ids.join(", ")
                        ),
                        next_steps,
                        wire::AuthenticationFailedContext {
                            sign_in: None,
                            attribution: wire::AuthenticationFailedContextAttribution::Uncertain,
                            candidates: ids,
                            ..unattributed
                        },
                    )
                }
                Attribution::Unattributed => (
                    needed,
                    vec![
                        sign_in(session.sign_in_method.clone().or(method), None),
                        inspect_session,
                    ],
                    unattributed,
                ),
            };
            wire::Diagnostic::AuthenticationFailedDiagnostic(wire::AuthenticationFailedDiagnostic {
                kind: serde_json::json!("authentication_failed"),
                message,
                field: None,
                next_steps,
                context,
            })
        }
        domain::Evidence::ExecutableMissing { command, error } => {
            wire::Diagnostic::ExecutableMissingDiagnostic(wire::ExecutableMissingDiagnostic {
                kind: serde_json::json!("executable_missing"),
                message: format!(
                    "the {harness} harness's executable {command} could not be spawned"
                ),
                field: None,
                next_steps: vec![
                    wire::Action::InspectHarnessImageAction(wire::InspectHarnessImageAction {
                        action: serde_json::json!("inspect_harness_image"),
                        harness: Some(harness.clone()),
                        image: None,
                        command: Some(command.clone()),
                    }),
                    inspect_session,
                ],
                context: wire::ExecutableMissingContext {
                    session: session.id.to_string(),
                    harness,
                    image: None,
                    executable: command.clone(),
                    evidence: wire::ExecutableMissingEvidence {
                        kind: serde_json::json!("executable_missing"),
                        command,
                        error: wire::OsError {
                            kind: match error.kind {
                                domain::OsErrorKind::NotFound => wire::OsErrorKind::NotFound,
                                domain::OsErrorKind::PermissionDenied => {
                                    wire::OsErrorKind::PermissionDenied
                                }
                                domain::OsErrorKind::Other => wire::OsErrorKind::Other,
                            },
                            code: error.code.map(i64::from),
                        },
                    },
                },
            })
        }
        domain::Evidence::Unknown { summary } => {
            wire::Diagnostic::UnknownFailureDiagnostic(wire::UnknownFailureDiagnostic {
                kind: serde_json::json!("unknown_failure"),
                message: "the session failed for a reason kestrel could not establish".to_owned(),
                field: None,
                next_steps: vec![inspect_session],
                context: wire::UnknownFailureContext {
                    session: Some(session.id.to_string()),
                    resource: Some(wire_resource(Resource::Session).as_str().to_owned()),
                    evidence: Some(wire::UnknownEvidence {
                        kind: serde_json::json!("unknown"),
                        summary,
                    }),
                },
            })
        }
    }
}

impl From<sign_in::SignIn> for wire::SavedSignIn {
    fn from(saved: sign_in::SignIn) -> Self {
        let authentication = saved.authentication;
        Self {
            harness: saved.harness,
            method: saved.method.id.clone(),
            kind: match saved.method.kind {
                wire::SignInMethodKind::Subscription => wire::SavedSignInKind::Subscription,
                wire::SignInMethodKind::Key => wire::SavedSignInKind::Key,
            },
            holding: match saved.holding {
                sign_in::Holding::Profile(profile) => wire::SignInHolding {
                    resource: wire::SignInHoldingResource::SubscriptionProfile,
                    reference: profile.name,
                    fills: saved.method.fills.clone(),
                },
                sign_in::Holding::ProviderCredential(variable) => wire::SignInHolding {
                    resource: wire::SignInHoldingResource::ProviderCredential,
                    reference: variable,
                    fills: saved.method.fills.clone(),
                },
            },
            revision: saved.revision,
            refreshed_from: saved.refreshed_from,
            saved_at: saved.saved_at.to_string(),
            authentication: wire::AuthenticationEvidence {
                state: match authentication.state {
                    sign_in::State::Unchecked => wire::AuthenticationEvidenceState::Unchecked,
                    sign_in::State::LoginCompleted => {
                        wire::AuthenticationEvidenceState::LoginCompleted
                    }
                    sign_in::State::CredentialAccepted => {
                        wire::AuthenticationEvidenceState::CredentialAccepted
                    }
                    sign_in::State::AuthenticationFailed => {
                        wire::AuthenticationEvidenceState::AuthenticationFailed
                    }
                    sign_in::State::Expired => wire::AuthenticationEvidenceState::Expired,
                    sign_in::State::NotCovered => wire::AuthenticationEvidenceState::NotCovered,
                },
                source: match authentication.source {
                    sign_in::Source::Import => wire::AuthenticationEvidenceSource::Import,
                    sign_in::Source::ProviderCheck => {
                        wire::AuthenticationEvidenceSource::ProviderCheck
                    }
                    sign_in::Source::Relay => wire::AuthenticationEvidenceSource::Relay,
                    sign_in::Source::GenericWrite => {
                        wire::AuthenticationEvidenceSource::GenericWrite
                    }
                    sign_in::Source::Refresh => wire::AuthenticationEvidenceSource::Refresh,
                    sign_in::Source::Session => wire::AuthenticationEvidenceSource::Session,
                    sign_in::Source::ModelTest => wire::AuthenticationEvidenceSource::ModelTest,
                },
                observed_at: authentication.observed_at.to_string(),
                provider_check: authentication
                    .provider_check
                    .map(|checked| wire::ProviderCheck {
                        provider: match checked.provider {
                            sign_in::check::Provider::Anthropic => {
                                wire::ProviderCheckProvider::Anthropic
                            }
                            sign_in::check::Provider::OpenAi => wire::ProviderCheckProvider::Openai,
                        },
                        outcome: match checked.outcome {
                            sign_in::check::Outcome::Accepted => {
                                wire::ProviderCheckOutcome::Accepted
                            }
                            sign_in::check::Outcome::Rejected => {
                                wire::ProviderCheckOutcome::Rejected
                            }
                            sign_in::check::Outcome::Inconclusive => {
                                wire::ProviderCheckOutcome::Inconclusive
                            }
                            sign_in::check::Outcome::Unavailable => {
                                wire::ProviderCheckOutcome::Unavailable
                            }
                        },
                        status: checked.status.map(i64::from),
                        provider_error: checked.provider_error,
                    }),
            },
            model_use: saved
                .model_use
                .into_iter()
                .map(|used| wire::ModelUseEvidence {
                    harness: used.harness,
                    model: used.model,
                    image: used.image,
                    result: match used.result {
                        sign_in::UseResult::Worked => wire::ModelUseEvidenceResult::Worked,
                        sign_in::UseResult::AuthenticationFailed => {
                            wire::ModelUseEvidenceResult::AuthenticationFailed
                        }
                        sign_in::UseResult::NotCovered => wire::ModelUseEvidenceResult::NotCovered,
                    },
                    source: match used.source {
                        sign_in::UseSource::ModelTest => wire::ModelUseEvidenceSource::ModelTest,
                        sign_in::UseSource::Session => wire::ModelUseEvidenceSource::Session,
                    },
                    observed_at: used.observed_at.to_string(),
                })
                .collect(),
        }
    }
}

pub(super) async fn sign_ins(
    State(control_plane): State<ControlPlane>,
    Path(organization): Path<String>,
) -> Result<Json<Vec<wire::SavedSignIn>>, Refused> {
    let saved = sign_in::saved(&control_plane.store, &organization).await?;

    Ok(Json(saved.into_iter().map(Into::into).collect()))
}

pub(super) async fn save_sign_in(
    State(control_plane): State<ControlPlane>,
    Path((organization, harness, method)): Path<(String, String, String)>,
    material: Result<Json<wire::SignInMaterial>, JsonRejection>,
) -> Result<Json<wire::SavedSignIn>, Refused> {
    let Json(material) = material?;
    let saved = sign_in::save(
        &control_plane.store,
        &control_plane.providers,
        &organization,
        &harness,
        &method,
        &material.value,
    )
    .await?;

    let Some(provider) = saved.unavailable else {
        return Ok(Json(saved.sign_in.into()));
    };
    let holding = inspect(
        match &saved.sign_in.holding {
            sign_in::Holding::Profile(profile) => {
                Locator::new(Resource::SubscriptionProfile, profile.name.clone())
            }
            sign_in::Holding::ProviderCredential(variable) => {
                Locator::new(Resource::ProviderCredential, variable.clone())
            }
        },
        Some(organization),
    );
    Err(Refused::Diagnosed {
        status: StatusCode::SERVICE_UNAVAILABLE,
        diagnostic: Box::new(wire::Diagnostic::UnavailableDiagnostic(
            wire::UnavailableDiagnostic {
                kind: serde_json::json!("unavailable"),
                message: format!(
                    "{} could not be asked whether it accepts this key, so it was saved unchecked",
                    provider.as_str()
                ),
                field: None,
                context: wire::UnavailableContext {
                    service: provider.as_str().to_owned(),
                    resource: None,
                    operation: sign_in::SAVE.to_owned(),
                    retry_after_seconds: None,
                    saved: Some(saved.sign_in.into()),
                },
                next_steps: vec![holding],
            },
        )),
    })
}

impl From<sign_in::model_test::Test> for wire::SignInTest {
    fn from(test: sign_in::model_test::Test) -> Self {
        Self {
            id: test.id,
            harness: test.harness,
            method: test.method,
            revision: test.revision,
            requested_model: test.requested_model,
            model: test.model,
            image: test.image,
            state: match test.state {
                sign_in::TestState::Pending => wire::SignInTestState::Pending,
                sign_in::TestState::Running => wire::SignInTestState::Running,
                sign_in::TestState::Completed => wire::SignInTestState::Completed,
                sign_in::TestState::Failed => wire::SignInTestState::Failed,
                sign_in::TestState::Interrupted => wire::SignInTestState::Interrupted,
            },
            outcome: test.outcome.map(|outcome| match outcome {
                sign_in::TestOutcome::Worked => wire::SignInTestOutcome::Worked,
                sign_in::TestOutcome::AuthenticationFailed => {
                    wire::SignInTestOutcome::AuthenticationFailed
                }
                sign_in::TestOutcome::NotCovered => wire::SignInTestOutcome::NotCovered,
                sign_in::TestOutcome::Unavailable => wire::SignInTestOutcome::Unavailable,
                sign_in::TestOutcome::HarnessFailed => wire::SignInTestOutcome::HarnessFailed,
                sign_in::TestOutcome::MaterialReplaced => wire::SignInTestOutcome::MaterialReplaced,
            }),
            message: test.message,
            created_at: test.created_at.to_string(),
            started_at: test.started_at.map(|at| at.to_string()),
            finished_at: test.finished_at.map(|at| at.to_string()),
        }
    }
}

/// A body is optional: no model named tests the harness's own default.
pub(super) async fn test_sign_in(
    State(control_plane): State<ControlPlane>,
    Path((organization, harness, method)): Path<(String, String, String)>,
    body: axum::body::Bytes,
) -> Result<(StatusCode, Json<wire::SignInTest>), Refused> {
    let asked: wire::SignInTestRequest = if body.is_empty() {
        wire::SignInTestRequest::default()
    } else {
        serde_json::from_slice(&body).map_err(|error| malformed(None, error.to_string()))?
    };
    let test = sign_in::model_test::start(
        &control_plane.store,
        &control_plane.images,
        &organization,
        &harness,
        &method,
        asked.model.as_deref(),
    )
    .await?;

    Ok((StatusCode::ACCEPTED, Json(test.into())))
}

pub(super) async fn sign_in_test(
    State(control_plane): State<ControlPlane>,
    Path((organization, harness, method, test)): Path<(String, String, String, String)>,
) -> Result<Json<wire::SignInTest>, Refused> {
    let test = sign_in::model_test::read(
        &control_plane.store,
        &organization,
        &harness,
        &method,
        &test,
    )
    .await?;

    Ok(Json(test.into()))
}
