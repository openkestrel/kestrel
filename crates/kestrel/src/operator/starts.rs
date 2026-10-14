use axum::Json;
use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use axum::http::StatusCode;
use kestrel_operator_types as wire;

use crate::declaration;
use crate::integration::github::Github;
use crate::repository;
use crate::start;

use super::{ControlPlane, Refused, session_record, workspace_record};

pub(super) async fn start(
    State(control_plane): State<ControlPlane>,
    plan: Result<Json<wire::StartPlan>, JsonRejection>,
) -> Result<(StatusCode, Json<wire::Started>), Refused> {
    let Json(plan) = plan?;
    let plan = start::Plan {
        organization: plan.organization,
        brief: plan.brief,
        project: declaration::Project {
            name: plan.project.name,
            repositories: plan.project.repositories,
            branch: plan.project.branch,
            integrations: plan
                .project
                .integrations
                .unwrap_or_default()
                .into_iter()
                .map(|selection| crate::repository::Selection {
                    repository: selection.repository,
                    integration: selection.integration,
                })
                .collect(),
        },
        agent: declaration::Agent {
            name: plan.agent.name,
            harness: plan.agent.harness,
            model: plan.agent.model.flatten(),
            mode: plan.agent.mode.flatten(),
            thought_level: plan.agent.thought_level.flatten(),
        },
        credentials: plan
            .credentials
            .unwrap_or_default()
            .into_iter()
            .map(|credential| start::Credential {
                variable: credential.variable,
                secret: credential.secret,
            })
            .collect(),
    };
    let started = start::start(
        &control_plane.store,
        &control_plane.images,
        &Github::unfenced()?,
        &plan,
    )
    .await?;

    Ok((
        StatusCode::CREATED,
        Json(wire::Started {
            organization: wire::Settled {
                name: started.organization.name,
                created: started.organization.created,
            },
            project: wire::Settled {
                name: started.project.name,
                created: started.project.created,
            },
            agent: wire::Settled {
                name: started.agent.name,
                created: started.agent.created,
            },
            workspace: workspace_record(&control_plane.store, started.workspace).await?,
            session: session_record(
                started.session,
                &plan.organization,
                &control_plane.live.summaries,
            ),
        }),
    ))
}

pub(super) async fn resolve_repositories(
    State(control_plane): State<ControlPlane>,
    request: Result<Json<wire::RepositoryResolutionRequest>, JsonRejection>,
) -> Result<Json<wire::RepositoryResolution>, Refused> {
    const OPERATION: &str = "resolve_repositories";
    let Json(request) = request?;
    let purpose = match request.purpose.unwrap_or_default() {
        wire::RepositoryPurpose::Declaration => repository::Purpose::Declaration,
        wire::RepositoryPurpose::PublicSetup => repository::Purpose::PublicSetup,
    };
    let selections = request.integrations.unwrap_or_default();
    let resolved = repository::resolution(
        &control_plane.store,
        &repository::Selecting {
            operation: OPERATION,
            repositories_field: "repositories",
            integrations_field: "integrations",
            organization: request.organization.as_deref().unwrap_or_default(),
            repositories: &request.repositories,
            selections: &selections,
        },
        purpose,
    )
    .await?;
    let warnings = match purpose {
        repository::Purpose::Declaration => Vec::new(),
        repository::Purpose::PublicSetup => vec![wire::RepositoryWarning {
            code: wire::RepositoryWarningCode::NoAppPushAuthority,
            message: "no GitHub App is selected, so nothing App-backed can push to these \
                      repositories or open pull requests on them; select an Integration to give \
                      a Workspace that authority"
                .to_owned(),
        }],
    };

    Ok(Json(wire::RepositoryResolution {
        repositories: resolved
            .into_iter()
            .map(|bound| wire::ResolvedRepository {
                given: bound.resolved.given,
                address: bound.resolved.address,
                checkout_directory: bound.resolved.directory,
                integration: bound.integration.map(|integration| integration.name),
            })
            .collect(),
        warnings,
    }))
}
