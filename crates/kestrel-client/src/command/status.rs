use anyhow::Result;
use kestrel_operator_types as wire;
use serde::Serialize;
use std::collections::BTreeMap;

use crate::diagnostic::{self, Invocation};
use crate::readiness;
use crate::scope::Scoping;

use crate::api::ControlPlane;
use crate::output::Presentation;
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
    pub(crate) client_url: String,
    pub(crate) readiness: wire::Readiness,
    pub(crate) organization: Option<String>,
    pub(crate) organization_source: Option<String>,
    pub(crate) workspace_counts: BTreeMap<String, usize>,
    pub(crate) session_counts: BTreeMap<String, usize>,
    pub(crate) sessions: Vec<wire::Session>,
    pub(crate) projects: usize,
    pub(crate) agents: usize,
    pub(crate) triggers: usize,
    pub(crate) workspaces: usize,
    pub(crate) integrations: usize,
    pub(crate) credentials: usize,
    pub(crate) profiles: usize,
}

pub(crate) async fn show(
    api: &ControlPlane,
    presentation: &Presentation,
    location: StatusLocation,
    scoping: Scoping<'_>,
    invocation: &Invocation,
) -> Result<()> {
    let (read, source) = readiness::read(api, scoping).await?;
    let organization = read.organization.as_deref();
    let Some(organization) = organization else {
        let status = Status {
            location,
            client_url: read.client_url.clone(),
            organization: None,
            organization_source: source,
            readiness: read,
            projects: 0,
            agents: 0,
            triggers: 0,
            workspaces: 0,
            integrations: 0,
            credentials: 0,
            profiles: 0,
            workspace_counts: workspace_counts(),
            session_counts: session_counts(),
            sessions: Vec::new(),
        };
        return shown(presentation, &status, invocation);
    };

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
    let mut workspace_counts = workspace_counts();
    let mut session_counts = session_counts();
    let mut sessions = Vec::new();
    for workspace in &workspaces {
        *workspace_counts
            .entry(workspace.state.to_string())
            .or_default() += 1;
        let listed: wire::ListSessionsResponse = api
            .get_as(&[
                "organizations",
                organization,
                "workspaces",
                &workspace.id.to_string(),
                "sessions",
            ])
            .await?;
        for session in listed {
            *session_counts.entry(session.state.to_string()).or_default() += 1;
            sessions.push(session);
        }
    }
    shown(
        presentation,
        &Status {
            location,
            client_url: read.client_url.clone(),
            organization: Some(organization.to_owned()),
            organization_source: source,
            readiness: read,
            workspace_counts,
            session_counts,
            sessions,
            projects: projects.len(),
            agents: agents.len(),
            triggers: triggers.len(),
            workspaces: workspaces.len(),
            integrations: integrations.len(),
            credentials: credentials.len(),
            profiles: profiles.len(),
        },
        invocation,
    )
}

fn workspace_counts() -> BTreeMap<String, usize> {
    [wire::WorkspaceState::Open, wire::WorkspaceState::Sealed]
        .into_iter()
        .map(|state| (state.to_string(), 0))
        .collect()
}

fn session_counts() -> BTreeMap<String, usize> {
    use wire::SessionState::*;
    [
        Queued,
        Working,
        Trailing,
        Waiting,
        Unbriefed,
        Ended,
        Unreachable,
    ]
    .into_iter()
    .map(|state| (state.to_string(), 0))
    .collect()
}

fn shown(presentation: &Presentation, status: &Status, invocation: &Invocation) -> Result<()> {
    typed::show(presentation, &view::STATUS, status)?;
    if !presentation.json() {
        for session in &status.sessions {
            if let Some(diagnosis) = &session.diagnostic {
                eprintln!(
                    "Session {}: {}",
                    session.name,
                    diagnostic::inline(diagnosis, invocation)
                );
            }
        }
    }
    readiness::finish(&status.readiness, invocation)
}
