use anyhow::Result;
use kestrel_operator_types as wire;
use serde::Serialize;

use crate::BINARY;
use crate::api::ControlPlane;
use crate::output::Presentation;
use crate::scope::Derived;
use crate::{typed, view};

#[derive(Serialize)]
pub(crate) struct StatusLocation {
    pub(crate) control_plane: String,
    pub(crate) control_plane_source: String,
}

#[derive(Serialize)]
pub(crate) struct Status {
    #[serde(flatten)]
    pub(crate) location: StatusLocation,
    pub(crate) organization: String,
    pub(crate) organization_source: String,
    pub(crate) projects: usize,
    pub(crate) agents: usize,
    pub(crate) triggers: usize,
    pub(crate) workspaces: usize,
    pub(crate) integrations: usize,
    pub(crate) credentials: usize,
    pub(crate) profiles: usize,
    pub(crate) next: String,
}

#[derive(Serialize)]
pub(crate) struct UnresolvedStatus {
    #[serde(flatten)]
    pub(crate) location: StatusLocation,
    pub(crate) organization: Option<String>,
    pub(crate) organization_source: Option<String>,
    pub(crate) organizations: Vec<String>,
    pub(crate) next: String,
}

pub(crate) async fn show(
    api: &ControlPlane,
    presentation: &Presentation,
    location: StatusLocation,
    derived: Derived,
) -> Result<()> {
    let scope = match derived {
        Derived::Scope(scope) => scope,
        Derived::Unnamed { existing } => {
            let next = if existing.is_empty() {
                format!("{BINARY} organization declare <name>")
            } else {
                format!("{BINARY} status --organization <name>")
            };
            typed::show(
                presentation,
                &view::UNRESOLVED,
                &UnresolvedStatus {
                    location,
                    organization: None,
                    organization_source: None,
                    organizations: existing,
                    next,
                },
            )?;
            return Ok(());
        }
    };

    let organization = scope.organization.as_str();
    let (projects, agents, triggers, workspaces, integrations, credentials, profiles) = tokio::try_join!(
        async {
            api.get_as::<wire::ListProjectsResponse>(&["organizations", organization, "projects"])
                .await
        },
        async {
            api.get_as::<wire::ListAgentsResponse>(&["organizations", organization, "agents"])
                .await
        },
        async {
            api.get_as::<wire::ListTriggersResponse>(&["organizations", organization, "triggers"])
                .await
        },
        async {
            api.get_as::<wire::ListWorkspacesResponse>(&[
                "organizations",
                organization,
                "workspaces",
            ])
            .await
        },
        async {
            api.get_as::<wire::ListIntegrationsResponse>(&[
                "organizations",
                organization,
                "integrations",
            ])
            .await
        },
        async {
            api.get_as::<wire::ListProviderCredentialsResponse>(&[
                "organizations",
                organization,
                "credentials",
            ])
            .await
        },
        async {
            api.get_as::<wire::ListSubscriptionProfilesResponse>(&[
                "organizations",
                organization,
                "profiles",
            ])
            .await
        },
    )?;
    typed::show(
        presentation,
        &view::STATUS,
        &Status {
            location,
            organization: organization.to_owned(),
            organization_source: scope.source.to_string(),
            projects: projects.len(),
            agents: agents.len(),
            triggers: triggers.len(),
            workspaces: workspaces.len(),
            integrations: integrations.len(),
            credentials: credentials.len(),
            profiles: profiles.len(),
            next: next_command(projects.first(), agents.first()),
        },
    )?;

    Ok(())
}

fn next_command(project: Option<&wire::Project>, agent: Option<&wire::Agent>) -> String {
    match (project, agent) {
        (Some(project), Some(agent)) => {
            format!(
                "{BINARY} workspace open --project {} --agent {}",
                project.name, agent.name
            )
        }
        (Some(_), None) => format!("{BINARY} agent declare <name>"),
        (None, _) => {
            format!("{BINARY} project declare <name> --repository <url> --branch <branch>")
        }
    }
}
