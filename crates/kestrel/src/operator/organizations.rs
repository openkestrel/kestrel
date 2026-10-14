use axum::Json;
use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, State};
use axum::response::Response;
use kestrel_operator_types as wire;

use crate::agent;
use crate::declaration;
use crate::domain::{self, Agent, Declared, Organization, Project};
use crate::integration::github::Github;

use super::{ControlPlane, Refused, answered, named, named_refusal, project_declaration, wire_id};

pub(super) fn agent_defaults(declaration: &wire::AgentDeclaration) -> Declared {
    Declared::named(Declared {
        model: declaration.model.clone().flatten(),
        mode: declaration.mode.clone().flatten(),
        thought_level: declaration.thought_level.clone().flatten(),
    })
}

pub(super) fn selected(
    integrations: Vec<domain::SelectedIntegration>,
) -> Vec<wire::SelectedIntegration> {
    integrations
        .into_iter()
        .map(|selected| wire::SelectedIntegration {
            repository: selected.repository,
            integration: selected.name,
            integration_id: wire_id(&selected.integration),
        })
        .collect()
}

impl From<Organization> for wire::Organization {
    fn from(organization: Organization) -> Self {
        Self {
            id: wire_id(&organization.id),
            name: organization.name,
            max_live_instances: organization
                .max_live_instances
                .map(|most| i64::try_from(most.get()).unwrap_or(i64::MAX)),
        }
    }
}

impl From<Project> for wire::Project {
    fn from(project: Project) -> Self {
        Self {
            id: wire_id(&project.id),
            name: project.name,
            repositories: project.repositories,
            integrations: selected(project.integrations),
            branch: project.branch,
        }
    }
}

impl From<Agent> for wire::Agent {
    fn from(agent: Agent) -> Self {
        Self {
            id: wire_id(&agent.id),
            name: agent.name,
            harness: agent.harness,
            model: agent.declared.model,
            mode: agent.declared.mode,
            thought_level: agent.declared.thought_level,
        }
    }
}

pub(super) async fn organizations(
    State(control_plane): State<ControlPlane>,
) -> Result<Json<wire::ListOrganizationsResponse>, Refused> {
    let mut tx = control_plane.store.begin().await?;
    let organizations = tx.organizations().all().await?;

    Ok(Json(organizations.into_iter().map(Into::into).collect()))
}

pub(super) async fn declare_organization(
    State(control_plane): State<ControlPlane>,
    declaration: Result<Json<wire::OrganizationDeclaration>, JsonRejection>,
) -> Result<Response, Refused> {
    let Json(declaration) = declaration?;
    let max_live_instances = declaration
        .max_live_instances
        .flatten()
        .map(|most| {
            usize::try_from(most)
                .ok()
                .and_then(std::num::NonZeroUsize::new)
                .ok_or_else(|| Refused::Malformed {
                    field: Some("max_live_instances"),
                    why: format!("max_live_instances is a count of at least 1, not {most}"),
                })
        })
        .transpose()?;
    named("declare_organization", &declaration.name)?;

    let mut tx = control_plane.store.begin().await?;
    let declared = tx
        .organizations()
        .declare(&declaration.name, max_live_instances)
        .await?;
    tx.commit().await?;

    Ok(answered::<_, wire::Organization>(declared))
}

pub(super) async fn projects(
    State(control_plane): State<ControlPlane>,
    Path(organization): Path<String>,
) -> Result<Json<wire::ListProjectsResponse>, Refused> {
    let mut tx = control_plane.store.begin().await?;
    let organization = tx.organizations().named(&organization).await?;
    let projects = tx.projects().all(&organization).await?;

    Ok(Json(projects.into_iter().map(Into::into).collect()))
}

pub(super) async fn declare_project(
    State(control_plane): State<ControlPlane>,
    Path(organization): Path<String>,
    declaration: Result<Json<wire::ProjectDeclaration>, JsonRejection>,
) -> Result<Response, Refused> {
    let Json(declaration) = declaration?;
    let checked = declaration::check_project(
        &control_plane.store,
        &Github::unfenced()?,
        &organization,
        &project_declaration(declaration),
        "declare_project",
    )
    .await
    .map_err(|error| declaration::scoped(declaration::Prefix::Root, "declare_project", error))?;

    let mut tx = control_plane.store.begin().await?;
    let organization = tx.organizations().named(&organization).await?;
    let (declared, _) = declaration::place_project(
        &mut tx,
        &organization,
        &checked,
        declaration::OnExisting::Change,
    )
    .await?;
    tx.commit().await?;

    Ok(answered::<_, wire::Project>(declared))
}

pub(super) async fn agents(
    State(control_plane): State<ControlPlane>,
    Path(organization): Path<String>,
) -> Result<Json<wire::ListAgentsResponse>, Refused> {
    let agents = agent::agents(&control_plane.store, &organization).await?;

    Ok(Json(agents.into_iter().map(Into::into).collect()))
}

pub(super) async fn set_agent_model(
    State(control_plane): State<ControlPlane>,
    Path((organization, name)): Path<(String, String)>,
    model: Result<Json<wire::AgentModel>, JsonRejection>,
) -> Result<Json<wire::Agent>, Refused> {
    let Json(model) = model?;
    let agent = agent::set_model(
        &control_plane.store,
        &organization,
        &name,
        model.model.flatten().as_deref(),
    )
    .await
    .map_err(named_refusal)?;

    Ok(Json(agent.into()))
}

pub(super) async fn declare_agent(
    State(control_plane): State<ControlPlane>,
    Path(organization): Path<String>,
    declaration: Result<Json<wire::AgentDeclaration>, JsonRejection>,
) -> Result<Response, Refused> {
    let Json(declaration) = declaration?;
    let checked = declaration::check_agent(
        &control_plane.images,
        &declaration.name,
        &declaration.harness,
        &agent_defaults(&declaration),
        "declare_agent",
    )
    .await
    .map_err(|error| declaration::scoped(declaration::Prefix::Root, "declare_agent", error))?;

    let mut tx = control_plane.store.begin().await?;
    let organization = tx.organizations().named(&organization).await?;
    let (declared, _) = declaration::place_agent(
        &mut tx,
        &organization,
        &checked,
        declaration::OnExisting::Change,
    )
    .await?;
    tx.commit().await?;

    Ok(answered::<_, wire::Agent>(declared))
}
