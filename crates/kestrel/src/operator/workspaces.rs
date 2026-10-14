use axum::Json;
use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use kestrel_operator_types as wire;
use serde::Deserialize;

use crate::declined::{self as declined, Locator, Reason, Resource};
use crate::domain::{Declared, Workspace};
use crate::scheduling;
use crate::store::workspace::HeldMessageRefusal;
use crate::{work, workspace};

use super::{
    Browsing, ControlPlane, Reading, Refused, diagnosed, held_message, malformed, queue_standing,
    resolved, session_record, workspace_record, workspace_work,
};

pub(super) async fn workspaces(
    State(control_plane): State<ControlPlane>,
    Path(organization): Path<String>,
) -> Result<Json<wire::ListWorkspacesResponse>, Refused> {
    let workspaces = workspace::workspaces(&control_plane.store, &organization).await?;
    let snapshot = scheduling::snapshot(&control_plane.store, &organization).await?;
    let mut records = Vec::with_capacity(workspaces.len());
    for workspace in workspaces {
        let id = workspace.id;
        let session = work::sessions(&control_plane.store, id)
            .await?
            .pop()
            .map(|session| session_record(session, &organization, &control_plane.live.summaries));
        let workspace = workspace_record(&control_plane.store, workspace).await?;
        records.push(wire::WorkspaceListed {
            checkout: workspace.checkout,
            continued_by: workspace.continued_by,
            continues: workspace.continues,
            correlation: workspace.correlation,
            held: workspace.held,
            held_messages: workspace.held_messages,
            id: workspace.id,
            instance: workspace.instance,
            integrations: workspace.integrations,
            last_active_at: workspace.last_active_at,
            name: workspace.name,
            opened_at: workspace.opened_at,
            opened_with: workspace.opened_with,
            organization: workspace.organization,
            profile: workspace.profile,
            project: workspace.project,
            pull_requests: workspace.pull_requests,
            sealed_at: workspace.sealed_at,
            started_by: workspace.started_by,
            state: workspace.state,
            unfinished_session: workspace.unfinished_session,
            session,
            queue: queue_standing(&snapshot, id),
        });
    }

    Ok(Json(records))
}

pub(super) async fn open_workspace(
    State(control_plane): State<ControlPlane>,
    Path(organization): Path<String>,
    declaration: Result<Json<wire::WorkspaceDeclaration>, JsonRejection>,
) -> Result<(StatusCode, Json<wire::Opened>), Refused> {
    let Json(declaration) = declaration?;
    let (workspace, session) = workspace::open(
        &control_plane.store,
        &organization,
        workspace::Open {
            project: &declaration.project,
            agent: &declaration.agent,
            profile: declaration
                .profile
                .as_ref()
                .and_then(|value| value.as_deref()),
            sign_in_method: declaration
                .sign_in_method
                .as_ref()
                .and_then(|value| value.as_deref()),
            branch: declaration
                .branch
                .as_ref()
                .and_then(|value| value.as_deref()),
            continues: declaration
                .continues
                .as_ref()
                .and_then(|value| value.as_deref()),
            declared: Declared {
                model: declaration.model.clone().flatten(),
                mode: declaration.mode.clone().flatten(),
                thought_level: declaration.thought_level.clone().flatten(),
            },
            brief: declaration
                .brief
                .as_ref()
                .and_then(|value| value.as_deref()),
            participant: declaration
                .participant
                .as_ref()
                .and_then(|value| value.as_deref()),
            depends_on: declaration
                .depends_on
                .as_ref()
                .and_then(|value| value.as_deref())
                .unwrap_or_default(),
        },
    )
    .await?;

    Ok((
        StatusCode::CREATED,
        Json(wire::Opened {
            workspace: workspace_record(&control_plane.store, workspace).await?,
            session: session_record(session, &organization, &control_plane.live.summaries),
        }),
    ))
}

pub(super) async fn work_summary(
    State(control_plane): State<ControlPlane>,
    Path((organization, reference)): Path<(String, String)>,
) -> Result<Json<wire::WorkspaceWork>, Refused> {
    let work = crate::live_work::read(
        &control_plane.store,
        &control_plane.live.summaries,
        &organization,
        &reference,
    )
    .await?;

    Ok(Json(workspace_work(work)))
}

#[derive(Deserialize)]
pub(super) struct ChangesQuery {
    scope: Option<String>,
}

pub(super) async fn workspace_changes(
    State(control_plane): State<ControlPlane>,
    Path((organization, reference)): Path<(String, String)>,
    Query(query): Query<ChangesQuery>,
    axum::extract::RawQuery(raw): axum::extract::RawQuery,
) -> Result<crate::live_read::AnswerBody, Refused> {
    let scope = query.scope.unwrap_or_else(|| "unpublished".to_owned());
    if !matches!(scope.as_str(), "unpublished" | "changed" | "staged")
        && !scope
            .strip_prefix("commit:")
            .is_some_and(|sha| !sha.is_empty() && sha.bytes().all(|byte| byte.is_ascii_hexdigit()))
    {
        return Err(malformed(
            Some("scope"),
            "scope must be unpublished, changed, staged or commit:<sha>",
        ));
    }
    let url = reqwest::Url::parse(&format!("http://localhost/?{}", raw.unwrap_or_default()))
        .map_err(|_| malformed(None, "invalid changes query"))?;
    let paths = url
        .query_pairs()
        .filter(|(name, _)| name == "path")
        .map(|(_, path)| path.into_owned())
        .collect();
    Ok(crate::live_read::read(
        &control_plane.store,
        &control_plane.live.reads,
        &organization,
        &reference,
        crate::live_read::Read::Changes { scope, paths },
    )
    .await?)
}

pub(super) async fn workspace_commits(
    State(control_plane): State<ControlPlane>,
    Path((organization, reference)): Path<(String, String)>,
) -> Result<crate::live_read::AnswerBody, Refused> {
    Ok(crate::live_read::read(
        &control_plane.store,
        &control_plane.live.reads,
        &organization,
        &reference,
        crate::live_read::Read::Commits,
    )
    .await?)
}

pub(super) async fn workspace_stashes(
    State(control_plane): State<ControlPlane>,
    Path((organization, reference)): Path<(String, String)>,
) -> Result<crate::live_read::AnswerBody, Refused> {
    Ok(crate::live_read::read(
        &control_plane.store,
        &control_plane.live.reads,
        &organization,
        &reference,
        crate::live_read::Read::Stashes,
    )
    .await?)
}

pub(super) async fn workspace_files(
    State(control_plane): State<ControlPlane>,
    Path((organization, reference)): Path<(String, String)>,
    Query(browsing): Query<Browsing>,
) -> Result<crate::live_read::AnswerBody, Refused> {
    Ok(crate::live_read::read(
        &control_plane.store,
        &control_plane.live.reads,
        &organization,
        &reference,
        crate::live_read::Read::Files {
            path: browsing.path.filter(|path| !path.is_empty()),
        },
    )
    .await?)
}

pub(super) async fn workspace_file(
    State(control_plane): State<ControlPlane>,
    Path((organization, reference)): Path<(String, String)>,
    Query(reading): Query<Reading>,
) -> Result<crate::live_read::AnswerBody, Refused> {
    Ok(crate::live_read::read(
        &control_plane.store,
        &control_plane.live.reads,
        &organization,
        &reference,
        crate::live_read::Read::File {
            path: reading.path,
            raw: reading.raw,
        },
    )
    .await?)
}

pub(super) async fn show_workspace(
    State(control_plane): State<ControlPlane>,
    Path((organization, workspace)): Path<(String, String)>,
) -> Result<Json<wire::Workspace>, Refused> {
    let workspace = resolved(&control_plane, &organization, &workspace).await?;

    Ok(Json(
        workspace_record(&control_plane.store, workspace).await?,
    ))
}

pub(super) async fn post_to_workspace(
    State(control_plane): State<ControlPlane>,
    Path((organization, workspace)): Path<(String, String)>,
    message: Result<Json<wire::WorkspaceMessage>, JsonRejection>,
) -> Result<Json<wire::Posted>, Refused> {
    let Json(message) = message?;
    let workspace = resolved(&control_plane, &organization, &workspace).await?;
    let posted = workspace::post(
        &control_plane.store,
        workspace.id,
        message.participant.as_deref(),
        &message.message,
    )
    .await?;

    Ok(Json(wire::Posted {
        session: posted
            .session
            .map(|session| session_record(session, &organization, &control_plane.live.summaries)),
        held_message: posted.held_message.map(held_message),
    }))
}

pub(super) async fn edit_workspace_message(
    State(control_plane): State<ControlPlane>,
    Path((organization, workspace, id)): Path<(String, String, String)>,
    edit: Result<Json<wire::WorkspaceMessage>, JsonRejection>,
) -> Result<Json<wire::HeldMessage>, Refused> {
    let Json(edit) = edit?;
    let workspace = resolved(&control_plane, &organization, &workspace).await?;
    let id = held_id(&organization, &workspace, &id)?;
    let edited = workspace::edit_message(
        &control_plane.store,
        workspace.id,
        id,
        edit.participant.as_deref(),
        &edit.message,
    )
    .await
    .map_err(|error| {
        held_refusal(
            error,
            "edit_workspace_message",
            &organization,
            &workspace,
            id,
        )
    })?;

    Ok(Json(held_message(edited)))
}

pub(super) async fn withdraw_workspace_message(
    State(control_plane): State<ControlPlane>,
    Path((organization, workspace, id)): Path<(String, String, String)>,
    withdrawal: Result<Json<wire::WorkspaceMessageWithdrawal>, JsonRejection>,
) -> Result<StatusCode, Refused> {
    let Json(withdrawal) = withdrawal?;
    let workspace = resolved(&control_plane, &organization, &workspace).await?;
    let id = held_id(&organization, &workspace, &id)?;
    workspace::withdraw_message(
        &control_plane.store,
        workspace.id,
        id,
        withdrawal.participant.as_deref(),
    )
    .await
    .map_err(|error| {
        held_refusal(
            error,
            "withdraw_workspace_message",
            &organization,
            &workspace,
            id,
        )
    })?;

    Ok(StatusCode::NO_CONTENT)
}

/// An id the Workspace never held answers `404` like one it did and no longer can change.
pub(super) fn held_id(organization: &str, workspace: &Workspace, id: &str) -> Result<i64, Refused> {
    id.parse().map_err(|_| {
        never_held(
            organization,
            workspace,
            id,
            format!("the workspace never held a message with the id {id}"),
        )
    })
}

pub(super) fn never_held(
    organization: &str,
    workspace: &Workspace,
    id: &str,
    message: String,
) -> Refused {
    diagnosed(
        Reason::MissingReference {
            resource: Resource::HeldMessage,
            reference: id.to_owned(),
            organization: Some(organization.to_owned()),
            within: Some(Locator::new(Resource::Workspace, workspace.id.to_string())),
            message,
        },
        None,
    )
}

/// Every Held Message refusal is answered by inspecting its Workspace; who may change one is
/// its author, never a Policy.
pub(super) fn held_refusal(
    error: anyhow::Error,
    operation: &'static str,
    organization: &str,
    workspace: &Workspace,
    id: i64,
) -> Refused {
    let Some(refused) = error.downcast_ref::<HeldMessageRefusal>() else {
        return error.into();
    };
    let message = refused.to_string();
    let reference = id.to_string();
    let next = declined::Next::inspect(Resource::Workspace, workspace.id.to_string());
    let conflict = |state| Reason::StateConflict {
        operation,
        resource: Resource::HeldMessage,
        reference: reference.clone(),
        organization: Some(organization.to_owned()),
        state,
        holding_session: None,
        next: next.clone(),
        message: message.clone(),
    };
    let reason = match refused {
        HeldMessageRefusal::NeverHeld(_) => {
            return never_held(organization, workspace, &reference, message);
        }
        HeldMessageRefusal::NotTheAuthor(_) => Reason::Forbidden {
            operation,
            resource: Resource::HeldMessage,
            reference: reference.clone(),
            organization: Some(organization.to_owned()),
            constraint: "author_only",
            next: next.clone(),
            message: message.clone(),
        },
        HeldMessageRefusal::AlreadyTaken => conflict("taken"),
        HeldMessageRefusal::AlreadyWithdrawn => conflict("withdrawn"),
    };
    diagnosed(reason, None)
}

pub(super) fn no_event(record: &str, organization: Option<&str>) -> Refused {
    diagnosed(
        Reason::MissingReference {
            resource: Resource::Event,
            reference: record.to_owned(),
            organization: organization.map(str::to_owned),
            within: None,
            message: format!("no event {record}"),
        },
        None,
    )
}

pub(super) async fn seal_workspace(
    State(control_plane): State<ControlPlane>,
    Path((organization, workspace)): Path<(String, String)>,
) -> Result<Json<wire::Workspace>, Refused> {
    let workspace = resolved(&control_plane, &organization, &workspace).await?;
    let workspace = workspace::seal(&control_plane.store, workspace.id).await?;

    Ok(Json(
        workspace_record(&control_plane.store, workspace).await?,
    ))
}
