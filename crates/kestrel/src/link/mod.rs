//! Server-sent events down, POST up, plain HTTP (ADR-0002), specified by `openapi/link.json`
//! rather than shared as types with the supervisor that dials in over it.

pub mod credential;

use std::collections::BTreeMap;
use std::time::Duration;

use anyhow::Result;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{BoxError, Json, Router};
use futures_core::Stream;
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

use crate::domain::{Checkout, Session, SessionId, Workspace};
use crate::link::credential::Secret;
use crate::log::{self, Cursor, Unreadable, Window};
use crate::profile;
use crate::provider;
use crate::role::serve;
use crate::store::{self, Store, Tx};
use crate::work::{self, ReportRefused, Reported};
use crate::workspace;

pub const CREDENTIALS: &str = "/link/sessions/{session}/credentials";
/// The Transcript of the Workspace the Session belongs to. Named for what crosses the link rather
/// than for what it is, because the supervisor is a courier and may not know (ADR-0002).
pub const ENTRIES: &str = "/link/sessions/{session}/entries";
pub const INSTRUCTIONS: &str = "/link/sessions/{session}/instructions";
pub const REPORTS: &str = "/link/sessions/{session}/reports";

/// Nothing subscribes to `Fanout` at 0.1 (ADR-0005), so a held-open stream learns of a new
/// instruction by asking `Store` again rather than by being told.
const POLL: Duration = Duration::from_millis(100);
const KEEP_ALIVE: Duration = Duration::from_secs(15);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Instruction {
    Start {
        checkout: Checkout,
        #[serde(skip_serializing_if = "Option::is_none")]
        prompt: Option<String>,
    },
    Prompt {
        prompt: String,
    },
    Stop,
}

impl Instruction {
    pub const fn kind(&self) -> &'static str {
        match self {
            Instruction::Start { .. } => "start",
            Instruction::Prompt { .. } => "prompt",
            Instruction::Stop => "stop",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SentInstruction {
    pub seq: i64,
    pub instruction: Instruction,
}

#[derive(Clone)]
struct ControlPlane {
    store: Store,
    shutdown: CancellationToken,
}

struct Waiting {
    instructions: Vec<SentInstruction>,
    the_session_ended: bool,
}

#[derive(Deserialize)]
struct Paging {
    cursor: Option<String>,
    window: Option<usize>,
}

#[derive(Serialize)]
struct Credentials {
    variables: BTreeMap<String, String>,
    files: BTreeMap<String, String>,
}

#[derive(Deserialize)]
struct Refreshed {
    files: BTreeMap<String, String>,
}

#[derive(Serialize)]
struct Entries {
    entries: Vec<Recorded>,
    cursor: Option<String>,
    more: bool,
}

#[derive(Serialize)]
struct Recorded {
    seq: i64,
    appended_at: String,
    entry: log::Entry,
}

pub fn router(store: Store, shutdown: CancellationToken) -> Router {
    Router::new()
        .route(CREDENTIALS, get(credentials).patch(refresh_credentials))
        .route(ENTRIES, get(entries))
        .route(INSTRUCTIONS, get(instructions))
        .route(REPORTS, post(report))
        .with_state(ControlPlane { store, shutdown })
}

/// A Brief nothing has followed is the agent's whole prompt, verbatim, so a harness still
/// recognises the skill invocation it may lead with.
pub async fn start(store: &Store, session: &Session) -> Result<SentInstruction> {
    let mut tx = store.begin().await?;
    let workspace = tx.workspaces().get(session.workspace).await?;
    workspace.accepts("turn")?;
    let prompt = tx.log().unfollowed_brief(&workspace).await?;
    let sent = tx
        .workspaces()
        .send_instruction(
            session,
            Instruction::Start {
                checkout: workspace.checkout.clone(),
                prompt,
            },
        )
        .await?;
    tx.workspaces().first_turn(session).await?;
    tx.commit().await?;

    Ok(sent)
}

/// The next turn of a waiting Session, in the same agent conversation (ADR-0024).
pub(crate) async fn prompt(tx: &mut Tx<'_>, session: &Session, prompt: String) -> Result<()> {
    tx.workspaces()
        .send_instruction(session, Instruction::Prompt { prompt })
        .await?;
    tx.workspaces()
        .prompt_turn(session)
        .await?
        .ok_or_else(|| anyhow::anyhow!("the session {} is not waiting for a prompt", session.id))?;

    Ok(())
}

pub async fn instruct(
    store: &Store,
    session: &Session,
    instruction: Instruction,
) -> Result<SentInstruction> {
    sent(store, session, |_| instruction).await
}

/// Sealing needs every Session ended first, which is what makes this refusal unreachable.
async fn sent(
    store: &Store,
    session: &Session,
    instruction: impl FnOnce(&Workspace) -> Instruction,
) -> Result<SentInstruction> {
    let mut tx = store.begin().await?;
    let workspace = tx.workspaces().get(session.workspace).await?;
    workspace.accepts("turn")?;
    let sent = tx
        .workspaces()
        .send_instruction(session, instruction(&workspace))
        .await?;
    tx.commit().await?;

    Ok(sent)
}

async fn instructions(
    State(control_plane): State<ControlPlane>,
    Path(session): Path<String>,
    headers: HeaderMap,
) -> Result<Sse<impl Stream<Item = Result<Event, BoxError>>>, Refused> {
    let session = authenticated(&control_plane, &headers, &session).await?;
    let mut cursor = last_event_id(&headers);
    info!(session = %session.id, cursor, "an environment is on the link");

    let stream = async_stream::try_stream! {
        loop {
            let waiting = waiting(&control_plane.store, session.id, cursor).await?;
            for sent in waiting.instructions {
                cursor = sent.seq;
                yield Event::default()
                    .id(sent.seq.to_string())
                    .event(sent.instruction.kind())
                    .json_data(&sent.instruction)?;
            }

            if waiting.the_session_ended {
                break;
            }

            tokio::select! {
                () = tokio::time::sleep(POLL) => {}
                () = control_plane.shutdown.cancelled() => break,
            }
        }
    };

    Ok(Sse::new(stream).keep_alive(KeepAlive::new().interval(KEEP_ALIVE)))
}

/// The Provider Credentials of the Session's Organization and the Subscription Profile its
/// Workspace names, decrypted here and held nowhere else: a supervisor asks as it spawns its
/// Harness, and an idle one never asks.
async fn credentials(
    State(control_plane): State<ControlPlane>,
    Path(session): Path<String>,
    headers: HeaderMap,
) -> Result<Json<Credentials>, Refused> {
    let session = authenticated(&control_plane, &headers, &session).await?;
    let mut variables = provider::reaching(&control_plane.store, session.organization).await?;
    let mut files = BTreeMap::new();
    if let Some(named) = workspace::show(&control_plane.store, session.workspace)
        .await?
        .profile
    {
        let contents = profile::contents(&control_plane.store, &named).await?;
        variables.extend(contents.variables);
        files = contents.files;
    }
    info!(
        session = %session.id,
        variables = variables.keys().cloned().collect::<Vec<_>>().join(", "),
        files = files.keys().cloned().collect::<Vec<_>>().join(", "),
        "an environment took the credentials its session needs"
    );

    Ok(Json(Credentials { variables, files }))
}

async fn refresh_credentials(
    State(control_plane): State<ControlPlane>,
    Path(session): Path<String>,
    headers: HeaderMap,
    Json(refreshed): Json<Refreshed>,
) -> Result<StatusCode, Refused> {
    let session = authenticated(&control_plane, &headers, &session).await?;
    let Some(named) = workspace::show(&control_plane.store, session.workspace)
        .await?
        .profile
    else {
        return Err(Refused::BadRequest(
            "this session's workspace names no subscription profile to refresh".to_owned(),
        ));
    };
    let taken = profile::refresh(&control_plane.store, &named, &refreshed.files).await?;
    info!(
        session = %session.id,
        profile = %named.name,
        files = taken.join(", "),
        "an environment handed back the logins its harness refreshed"
    );

    Ok(StatusCode::NO_CONTENT)
}

async fn entries(
    State(control_plane): State<ControlPlane>,
    Path(session): Path<String>,
    Query(paging): Query<Paging>,
    headers: HeaderMap,
) -> Result<Json<Entries>, Refused> {
    let session = authenticated(&control_plane, &headers, &session).await?;
    let from = paging
        .cursor
        .as_deref()
        .map(str::parse::<Cursor>)
        .transpose()
        .map_err(|error| Refused::BadRequest(error.to_string()))?;
    let window = Window::or_default(paging.window)
        .map_err(|error| Refused::BadRequest(error.to_string()))?;

    let page = workspace::transcript(&control_plane.store, session.workspace, from, window).await?;

    Ok(Json(Entries {
        entries: page
            .entries
            .into_iter()
            .map(|entry| Recorded {
                seq: entry.seq,
                appended_at: entry.appended_at.to_string(),
                entry: entry.entry,
            })
            .collect(),
        cursor: page.cursor.map(|cursor| cursor.to_string()),
        more: page.more,
    }))
}

async fn report(
    State(control_plane): State<ControlPlane>,
    Path(session): Path<String>,
    headers: HeaderMap,
    Json(reported): Json<Reported>,
) -> Result<StatusCode, Refused> {
    let session = authenticated(&control_plane, &headers, &session).await?;
    work::report(&control_plane.store, &session, reported).await?;

    Ok(StatusCode::ACCEPTED)
}

async fn waiting(store: &Store, session: SessionId, cursor: i64) -> Result<Waiting> {
    let mut tx = store.begin().await?;
    let the_session_ended = tx.workspaces().session(session).await?.ended_at.is_some();

    Ok(Waiting {
        instructions: tx.workspaces().instructions_after(session, cursor).await?,
        the_session_ended,
    })
}

async fn authenticated(
    control_plane: &ControlPlane,
    headers: &HeaderMap,
    session: &str,
) -> Result<Session, Refused> {
    let session: SessionId = session.parse().map_err(|_| Refused::NoSuchSession)?;
    let secret = bearer(headers).ok_or(Refused::Unauthorized("no credential presented"))?;

    let mut tx = control_plane.store.begin().await?;
    let credential = tx
        .workspaces()
        .credential(&secret.digest())
        .await?
        .ok_or(Refused::Unauthorized("no credential kestrel issued"))?;

    if !credential.is_live_at(Timestamp::now()) {
        return Err(Refused::Unauthorized("the credential is no longer live"));
    }
    if credential.session != session {
        return Err(Refused::Forbidden(
            "the credential belongs to another session",
        ));
    }

    Ok(tx.workspaces().session(session).await?)
}

fn bearer(headers: &HeaderMap) -> Option<Secret> {
    headers
        .get(header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
        .map(Secret::presented)
}

fn last_event_id(headers: &HeaderMap) -> i64 {
    headers
        .get("last-event-id")
        .and_then(|cursor| cursor.to_str().ok())
        .and_then(|cursor| cursor.parse().ok())
        .unwrap_or(0)
}

enum Refused {
    BadRequest(String),
    NoSuchSession,
    Unauthorized(&'static str),
    Forbidden(&'static str),
    Unavailable(anyhow::Error),
}

impl From<anyhow::Error> for Refused {
    fn from(error: anyhow::Error) -> Self {
        Refused::Unavailable(error)
    }
}

impl From<Unreadable> for Refused {
    fn from(unreadable: Unreadable) -> Self {
        match unreadable {
            Unreadable::Cursor(why) => Refused::BadRequest(why),
            Unreadable::Unavailable(error) => Refused::Unavailable(error),
        }
    }
}

impl From<ReportRefused> for Refused {
    fn from(refused: ReportRefused) -> Self {
        match refused {
            ReportRefused::MissingSequence | ReportRefused::SkippedSequence(_) => {
                Self::BadRequest(refused.to_string())
            }
            ReportRefused::Unavailable(error) => Self::Unavailable(error),
        }
    }
}

impl IntoResponse for Refused {
    fn into_response(self) -> Response {
        let busy = matches!(&self, Refused::Unavailable(error) if store::busy(error));
        let (status, message) = match self {
            Refused::BadRequest(why) => (StatusCode::BAD_REQUEST, why),
            Refused::NoSuchSession => (StatusCode::NOT_FOUND, "no such session".to_owned()),
            Refused::Unauthorized(why) => (StatusCode::UNAUTHORIZED, why.to_owned()),
            Refused::Forbidden(why) => (StatusCode::FORBIDDEN, why.to_owned()),
            Refused::Unavailable(error) => {
                warn!(%error, busy, "the link could not answer");
                (
                    StatusCode::SERVICE_UNAVAILABLE,
                    "the link could not answer".to_owned(),
                )
            }
        };

        serve::refusal(status, busy, Json(Refusal { message }))
    }
}

#[derive(Serialize)]
struct Refusal {
    message: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::work::Report;

    #[test]
    fn a_numbered_report_carries_its_seq_beside_its_kind() {
        let sent = serde_json::json!({"kind": "said", "seq": 3, "message": "what it said"});

        let reported: Reported = serde_json::from_value(sent.clone()).expect("a report");

        assert_eq!(reported.seq, Some(3));
        assert_eq!(
            reported.report,
            Report::Said {
                message: "what it said".to_owned()
            }
        );
        assert_eq!(serde_json::to_value(&reported).expect("a report"), sent);
    }

    #[test]
    fn a_report_the_environment_does_not_number_carries_no_seq() {
        let sent = serde_json::json!({"kind": "heartbeat"});

        let reported: Reported = serde_json::from_value(sent.clone()).expect("a report");

        assert_eq!(reported.seq, None);
        assert_eq!(reported.report, Report::Heartbeat);
        assert_eq!(serde_json::to_value(&reported).expect("a report"), sent);
    }
}
