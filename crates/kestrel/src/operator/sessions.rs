use axum::Json;
use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, State};
use axum::http::header::{CONTENT_TYPE, ETAG, IF_NONE_MATCH};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use kestrel_operator_types as wire;
use serde::Serialize;
use sha2::Digest as _;

use crate::declined::{self as declined, Constraint, Reason, Resource};
use crate::domain::{self, Declared, Workspace};
use crate::store::Declared as DeclaredRecord;
use crate::{work, workspace};

use super::{ControlPlane, Refused, diagnosed, invalid_field, session_record};

// The contract carries both lists on every option, which the domain's boolean has none of.
pub(crate) fn shown_option(option: domain::SessionOption, harness: &str) -> wire::SessionOption {
    let offered = |value: domain::SessionOptionValue| wire::SessionOptionValue {
        value: value.value,
        name: value.name,
        description: value.description,
    };
    let warns_cache = warns_cache(harness, option.category.as_deref());
    let (kind, current, values, groups) = match option.kind {
        domain::SessionOptionKind::Select {
            current,
            values,
            groups,
        } => (
            wire::SessionOptionKind::Select,
            wire::SessionOptionCurrent::SelectedOptionValue(current),
            values.into_iter().map(offered).collect(),
            groups
                .into_iter()
                .map(|group| wire::SessionOptionGroup {
                    group: group.group,
                    name: group.name,
                    values: group.values.into_iter().map(offered).collect(),
                })
                .collect(),
        ),
        domain::SessionOptionKind::Boolean { current } => (
            wire::SessionOptionKind::Boolean,
            wire::SessionOptionCurrent::ToggleOptionValue(current),
            Vec::new(),
            Vec::new(),
        ),
    };
    wire::SessionOption {
        id: option.id,
        name: option.name,
        description: option.description,
        category: option.category,
        kind,
        current,
        values,
        groups,
        warns_cache,
    }
}

/// ADR-0041: no adapter reports which option categories keep the prompt cache, so kestrel lists
/// them per harness.
pub(super) const CACHE_KEPT_ACROSS: &[(&str, &str)] =
    &[("claude", domain::SessionOption::THOUGHT_LEVEL)];

pub(super) fn warns_cache(harness: &str, category: Option<&str>) -> bool {
    let Some(category) = category else {
        return false;
    };
    if !matches!(
        category,
        domain::SessionOption::MODEL
            | domain::SessionOption::THOUGHT_LEVEL
            | domain::SessionOption::MODEL_CONFIG
    ) {
        return false;
    }

    !CACHE_KEPT_ACROSS.contains(&(harness, category))
}

pub(super) async fn sessions(
    State(control_plane): State<ControlPlane>,
    Path((organization, workspace)): Path<(String, String)>,
) -> Result<Json<Vec<wire::Session>>, Refused> {
    let workspace = resolved(&control_plane, &organization, &workspace).await?;
    let sessions = work::sessions(&control_plane.store, workspace.id).await?;

    Ok(Json(
        sessions
            .into_iter()
            .map(|session| session_record(session, &organization, &control_plane.live.summaries))
            .collect(),
    ))
}

pub(super) async fn enqueue_session(
    State(control_plane): State<ControlPlane>,
    Path((organization, workspace)): Path<(String, String)>,
    declaration: Result<Json<wire::SessionDeclaration>, JsonRejection>,
) -> Result<(StatusCode, Json<wire::Session>), Refused> {
    let Json(declaration) = declaration?;
    let workspace = resolved(&control_plane, &organization, &workspace).await?;
    if !work::has_had_session(&control_plane.store, workspace.id).await? {
        let reference = workspace.id.to_string();
        return Err(diagnosed(
            Reason::StateConflict {
                operation: "enqueue_session",
                resource: Resource::Workspace,
                reference: reference.clone(),
                organization: Some(organization),
                state: "never_had_session",
                holding_session: None,
                next: declined::Next::inspect(Resource::Workspace, reference),
                message: format!(
                    "the workspace {} has never had a session, and a workspace's first session \
                     starts with its open",
                    workspace.id
                ),
            },
            None,
        ));
    }
    let session = work::enqueue_selecting(
        &control_plane.store,
        workspace.id,
        declaration
            .agent
            .as_ref()
            .and_then(|value| value.as_deref()),
        Declared {
            model: declaration.model.clone().flatten(),
            mode: declaration.mode.clone().flatten(),
            thought_level: declaration.thought_level.clone().flatten(),
        },
        declaration
            .sign_in_method
            .as_ref()
            .and_then(|value| value.as_deref()),
        declaration
            .depends_on
            .as_ref()
            .and_then(|value| value.as_deref())
            .unwrap_or_default(),
    )
    .await?;

    Ok((
        StatusCode::CREATED,
        Json(session_record(
            session,
            &organization,
            &control_plane.live.summaries,
        )),
    ))
}

pub(super) async fn show_session(
    State(control_plane): State<ControlPlane>,
    Path((organization, session)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<Response, Refused> {
    let session = work::resolve_session(&control_plane.store, &organization, &session).await?;
    let record = session_record(session, &organization, &control_plane.live.summaries);
    let body = serde_json::to_string(&record).map_err(anyhow::Error::from)?;
    let tag = strong_etag(&body);

    if headers
        .get(IF_NONE_MATCH)
        .and_then(|asked| asked.to_str().ok())
        .is_some_and(|asked| matches_etag(asked, &tag))
    {
        return Ok((StatusCode::NOT_MODIFIED, [(ETAG, tag)]).into_response());
    }

    Ok((
        [(CONTENT_TYPE, "application/json".to_owned()), (ETAG, tag)],
        body,
    )
        .into_response())
}

pub(super) fn strong_etag(body: &str) -> String {
    let digest = sha2::Sha256::digest(body.as_bytes());
    let hex: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();

    format!("\"{hex}\"")
}

/// RFC 9110's If-None-Match comparison: weak tags match weak, and `*` matches any.
pub(super) fn matches_etag(asked: &str, tag: &str) -> bool {
    asked.split(',').any(|candidate| {
        let candidate = candidate.trim();
        candidate == "*" || candidate.strip_prefix("W/").unwrap_or(candidate) == tag
    })
}

pub(super) async fn interrupt_session(
    State(control_plane): State<ControlPlane>,
    Path((organization, session)): Path<(String, String)>,
    interrupt: Result<Json<wire::SessionInterrupt>, JsonRejection>,
) -> Result<(StatusCode, Json<wire::Session>), Refused> {
    let Json(interrupt) = interrupt?;
    let session = work::resolve_session(&control_plane.store, &organization, &session).await?;
    let interrupting = work::interrupt(
        &control_plane.store,
        session.id,
        interrupt.participant.as_deref(),
    )
    .await?;

    Ok((
        StatusCode::ACCEPTED,
        Json(session_record(
            interrupting,
            &organization,
            &control_plane.live.summaries,
        )),
    ))
}

pub(super) async fn stop_session(
    State(control_plane): State<ControlPlane>,
    Path((organization, session)): Path<(String, String)>,
) -> Result<Json<wire::Session>, Refused> {
    let session = work::resolve_session(&control_plane.store, &organization, &session).await?;
    let running = control_plane.live.summaries.last_held(&session).tools;
    work::stop(&control_plane.store, session.id, &running).await?;
    control_plane.live.summaries.clear_session(&session);
    let session = work::session(&control_plane.store, session.id).await?;

    Ok(Json(session_record(
        session,
        &organization,
        &control_plane.live.summaries,
    )))
}

pub(super) async fn set_session_option(
    State(control_plane): State<ControlPlane>,
    Path((organization, session)): Path<(String, String)>,
    change: Result<Json<wire::OptionChange>, JsonRejection>,
) -> Result<(StatusCode, Json<wire::Session>), Refused> {
    let Json(change) = change?;
    let named = match (
        change.option.as_ref().and_then(|value| value.as_deref()),
        change.category.as_ref().and_then(|value| value.as_deref()),
    ) {
        (Some(option), None) => work::Named::Option(option),
        (None, Some(category)) => work::Named::Category(category),
        (Some(_), Some(_)) => {
            return Err(Refused::Malformed {
                field: Some("option"),
                why: "an option change names an option or a category, not both".to_owned(),
            });
        }
        (None, None) => {
            return Err(Refused::Malformed {
                field: Some("option"),
                why: "an option change names an option or a category".to_owned(),
            });
        }
    };
    let written = work::set_option(
        &control_plane.store,
        &organization,
        &session,
        change
            .participant
            .as_ref()
            .and_then(|value| value.as_deref()),
        named,
        &change.value,
    )
    .await?;

    Ok((
        if written.live {
            StatusCode::ACCEPTED
        } else {
            StatusCode::OK
        },
        Json(session_record(
            written.session,
            &organization,
            &control_plane.live.summaries,
        )),
    ))
}

pub(super) async fn resolved(
    control_plane: &ControlPlane,
    organization: &str,
    workspace: &str,
) -> Result<Workspace, Refused> {
    Ok(workspace::resolve(&control_plane.store, organization, workspace).await?)
}

pub(super) fn named(operation: &'static str, name: &str) -> Result<(), Refused> {
    if name.is_empty() {
        return Err(invalid_field(
            operation,
            "name",
            Constraint::NonEmpty,
            "a name cannot be empty",
        ));
    }
    Ok(())
}

pub(super) fn answered<T, R>(declared: DeclaredRecord<T>) -> Response
where
    R: From<T> + Serialize,
{
    let status = if declared.created {
        StatusCode::CREATED
    } else {
        StatusCode::OK
    };

    (status, Json(R::from(declared.record))).into_response()
}
