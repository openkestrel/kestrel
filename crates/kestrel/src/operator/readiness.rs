use axum::Json;
use axum::extract::rejection::JsonRejection;
use axum::extract::{Query, State};
use axum::response::Response;
use kestrel_operator_types as wire;

use crate::declined::{Locator, Resource};
use crate::domain;
use crate::sign_in;
use crate::start_readiness;

use super::{ControlPlane, Refused, answered, diagnosis, inspect, wire_id, wire_resource};

impl From<domain::Operator> for wire::Operator {
    fn from(operator: domain::Operator) -> Self {
        Self {
            id: wire_id(&operator.id),
            name: operator.name,
        }
    }
}

pub(super) async fn show_install(
    State(control_plane): State<ControlPlane>,
) -> Result<Json<wire::Install>, Refused> {
    Ok(Json(wire::Install {
        initial_setup_complete: crate::operator_identity::initial_setup_complete(
            &control_plane.store,
        )
        .await?,
    }))
}

pub(super) async fn show_readiness(
    State(control_plane): State<ControlPlane>,
    Query(context): Query<start_readiness::Context>,
) -> Result<Json<wire::Readiness>, Refused> {
    let read = start_readiness::read(&control_plane.store, &control_plane.images, context).await?;

    Ok(Json(readiness(read, &control_plane.client)))
}

pub(super) fn readiness(read: start_readiness::Readiness, client: &url::Url) -> wire::Readiness {
    let outcome = match read.outcome() {
        start_readiness::Outcome::Ready => wire::ReadinessOutcome::Ready,
        start_readiness::Outcome::Blocked => wire::ReadinessOutcome::Blocked,
        start_readiness::Outcome::Unavailable => wire::ReadinessOutcome::Unavailable,
    };
    let organization = read.organization.map(|organization| organization.name);
    let scoped = organization.as_deref();

    wire::Readiness {
        outcome,
        client_url: client.as_str().trim_end_matches('/').to_owned(),
        initial_setup_complete: read.initial_setup_complete,
        selected: wire::ReadinessSelection {
            harness: read.selected.harness,
            model: read.selected.model,
            profile: read.selected.profile,
            sign_in_method: read.selected.sign_in_method,
            project: read.selected.project,
        },
        sign_in: read.sign_in.map(Into::into),
        saved: wire::ReadinessSaved {
            operator: read.operator.map(Into::into),
            sign_ins: read.sign_ins.into_iter().map(Into::into).collect(),
            github_integrations: read.github_integrations,
            projects: read.projects,
        },
        gaps: read
            .gaps
            .into_iter()
            .map(|gap| readiness_gap(gap, scoped))
            .collect(),
        warnings: read
            .warnings
            .into_iter()
            .map(|warning| match warning {
                start_readiness::Warning::Unchecked(sign_in) => {
                    let (locator, context) = held_sign_in("sign_in_unchecked", &sign_in, scoped);
                    setup_gap(
                        start_readiness::unchecked(&sign_in),
                        context,
                        vec![inspect(locator, organization.clone())],
                    )
                }
                start_readiness::Warning::NeedsAttention(attention) => {
                    needs_attention(&attention, scoped)
                }
                start_readiness::Warning::NotLending(reason) => diagnosis(reason, None).1,
            })
            .collect(),
        unavailable: read
            .uninspectable
            .into_iter()
            .map(|reason| diagnosis(reason, None).1)
            .collect(),
        organization,
    }
}

pub(super) fn readiness_gap(
    gap: start_readiness::Gap,
    organization: Option<&str>,
) -> wire::Diagnostic {
    let message = gap.message(organization);
    let context = |prerequisite: &str, resource: Option<Resource>| wire::SetupGapContext {
        prerequisite: prerequisite.to_owned(),
        resource: resource.map(wire_resource),
        reference: None,
        organization: organization.map(str::to_owned),
        harness: None,
        method: None,
        sign_in: None,
        image: None,
    };
    let declare_project = organization.map(|organization| {
        wire::Action::DeclareProjectAction(wire::DeclareProjectAction {
            action: serde_json::json!("declare_project"),
            organization: organization.to_owned(),
            name: None,
            repositories: None,
            branch: None,
            missing: vec![
                "name".to_owned(),
                "repositories".to_owned(),
                "branch".to_owned(),
            ],
        })
    });

    match gap {
        start_readiness::Gap::Refused(reason) => diagnosis(reason, None).1,
        start_readiness::Gap::NeedsAttention(attention) => {
            needs_attention(&attention, organization)
        }
        start_readiness::Gap::NoSignIn {
            harness,
            method,
            image,
        } => setup_gap(
            message,
            wire::SetupGapContext {
                harness: harness.clone(),
                method: method.clone(),
                image,
                ..context("sign_in", None)
            },
            vec![wire::Action::SignInAction(wire::SignInAction {
                action: serde_json::json!("sign_in"),
                harness,
                method,
                sign_in: None,
            })],
        ),
        // A public repository needs no Integration, so declaring one stays on offer.
        start_readiness::Gap::NoRepository => setup_gap(
            message,
            context("repository", Some(Resource::Project)),
            std::iter::once(wire::Action::ConnectGithubAction(
                wire::ConnectGithubAction {
                    action: serde_json::json!("connect_github"),
                    organization: organization.map(str::to_owned),
                },
            ))
            .chain(declare_project)
            .collect(),
        ),
        start_readiness::Gap::NoProject => setup_gap(
            message,
            context("project", Some(Resource::Project)),
            declare_project.into_iter().collect(),
        ),
    }
}

pub(super) fn setup_gap(
    message: String,
    context: wire::SetupGapContext,
    next_steps: Vec<wire::Action>,
) -> wire::Diagnostic {
    wire::Diagnostic::SetupGapDiagnostic(wire::SetupGapDiagnostic {
        kind: serde_json::json!("setup_gap"),
        message,
        field: None,
        context,
        next_steps,
    })
}

pub(super) fn held_sign_in(
    prerequisite: &str,
    sign_in: &sign_in::SignIn,
    organization: Option<&str>,
) -> (Locator, wire::SetupGapContext) {
    let locator = match &sign_in.holding {
        sign_in::Holding::Profile(profile) => {
            Locator::new(Resource::SubscriptionProfile, profile.name.clone())
        }
        sign_in::Holding::ProviderCredential(variable) => {
            Locator::new(Resource::ProviderCredential, variable.clone())
        }
    };
    let context = wire::SetupGapContext {
        prerequisite: prerequisite.to_owned(),
        resource: Some(wire_resource(locator.resource)),
        reference: Some(locator.reference.clone()),
        organization: organization.map(str::to_owned),
        harness: Some(sign_in.harness.clone()),
        method: Some(sign_in.method.id.clone()),
        sign_in: Some(locator.reference.clone()),
        image: None,
    };

    (locator, context)
}

/// Signing in again is offered only where it could help: nothing says it buys coverage.
pub(super) fn needs_attention(
    attention: &start_readiness::Attention,
    organization: Option<&str>,
) -> wire::Diagnostic {
    let sign_in = &attention.sign_in;
    let (locator, context) = held_sign_in("sign_in_attention", sign_in, organization);
    let again = (attention.state != sign_in::State::NotCovered).then(|| {
        wire::Action::SignInAction(wire::SignInAction {
            action: serde_json::json!("sign_in"),
            harness: Some(sign_in.harness.clone()),
            method: Some(sign_in.method.id.clone()),
            sign_in: Some(locator.reference.clone()),
        })
    });

    setup_gap(
        attention.message(),
        context,
        again
            .into_iter()
            .chain([inspect(locator, organization.map(str::to_owned))])
            .collect(),
    )
}

pub(super) async fn show_operator(
    State(control_plane): State<ControlPlane>,
) -> Result<Json<wire::Operator>, Refused> {
    Ok(Json(
        crate::operator_identity::current(&control_plane.store)
            .await?
            .into(),
    ))
}

pub(super) async fn name_operator(
    State(control_plane): State<ControlPlane>,
    declaration: Result<Json<wire::OperatorDeclaration>, JsonRejection>,
) -> Result<Response, Refused> {
    let Json(declaration) = declaration?;
    let declared = crate::operator_identity::name(&control_plane.store, &declaration.name).await?;
    Ok(answered::<_, wire::Operator>(declared))
}
