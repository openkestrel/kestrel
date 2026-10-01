//! Server-sent events down, POST up, plain HTTP (ADR-0002), specified by `openapi/link.json`
//! rather than shared as types with the supervisor that dials in over it.

pub mod credential;

use std::collections::BTreeMap;
use std::time::Duration;

use anyhow::Result;
use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{BoxError, Json, Router};
use futures_core::Stream;
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

use crate::domain::{Checkout, Session, SessionId, Workspace};
use crate::link::credential::Secret;
use crate::log::{Cursor, Unreadable, Window};
use crate::profile;
use crate::provider;
use crate::role::serve;
use crate::store::workspace::Linked;
use crate::store::{self, Store, Tx};
use crate::work::{self, ReportRefused, Reported};
use crate::workspace;

pub(crate) const ON_THE_LINK: jiff::SignedDuration = jiff::SignedDuration::from_secs(6);

pub const ANSWERS: &str = "/link/instances/{instance}/answers/{request}";
pub const CREDENTIALS: &str = "/link/instances/{instance}/credentials";
/// The Transcript of the Workspace holding the Instance. Named for what crosses the link rather
/// than for what it is, because the supervisor is a courier and may not know (ADR-0002).
pub const ENTRIES: &str = "/link/instances/{instance}/entries";
pub const INSTRUCTIONS: &str = "/link/instances/{instance}/instructions";
pub const REPORTS: &str = "/link/instances/{instance}/reports";

/// Nothing subscribes to `Fanout` at 0.1 (ADR-0005), so a held-open stream learns of a new
/// instruction by asking `Store` again rather than by being told.
const POLL: Duration = Duration::from_millis(100);
const KEEP_ALIVE: Duration = Duration::from_secs(15);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Instruction {
    Start {
        checkout: Checkout,
        prompt: String,
        harness: Harness,
    },
    Prompt {
        prompt: String,
    },
    /// Ends the Session's harness; the supervisor stays on the link.
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

/// What the supervisor spawns for one Session, since each Session in a Workspace may choose its
/// own Agent (ADR-0031), and what that Session declared for the Harness's options (ADR-0041).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Harness {
    pub command: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thought_level: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SentInstruction {
    pub seq: i64,
    pub session: SessionId,
    pub instruction: Instruction,
}

#[derive(Serialize)]
struct Carried<'a> {
    session: SessionId,
    #[serde(flatten)]
    instruction: &'a Instruction,
}

#[derive(Clone)]
struct ControlPlane {
    store: Store,
    shutdown: CancellationToken,
    summaries: crate::live_work::Summaries,
    reads: crate::live_read::Reads,
}

struct Waiting {
    instructions: Vec<SentInstruction>,
    still_linked: bool,
}

#[derive(Deserialize)]
struct Asking {
    session: String,
}

#[derive(Deserialize)]
struct Paging {
    kinds: Option<String>,
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
    kind: crate::log::Kind,
    session_id: Option<SessionId>,
    seq: i64,
    appended_at: String,
    entry: serde_json::Value,
}

pub fn router(
    store: Store,
    shutdown: CancellationToken,
    summaries: crate::live_work::Summaries,
    reads: crate::live_read::Reads,
) -> Router {
    Router::new()
        .route(ANSWERS, post(answer))
        .route(CREDENTIALS, get(credentials).patch(refresh_credentials))
        .route(ENTRIES, get(entries))
        .route(INSTRUCTIONS, get(instructions))
        .route(REPORTS, post(report))
        .with_state(ControlPlane {
            store,
            shutdown,
            summaries,
            reads,
        })
}

/// A Brief nothing has followed is the agent's whole prompt, verbatim, so a harness still
/// recognises the skill invocation it may lead with. Otherwise the instruction is the message or
/// messages that started this Session; a Session the control plane finds neither for is not
/// started with an improvised one.
pub async fn start(store: &Store, session: &Session, harness: Harness) -> Result<SentInstruction> {
    let mut tx = store.begin().await?;
    let workspace = tx.workspaces().get(session.workspace).await?;
    workspace.accepts("turn")?;
    let prompt = instruction(&mut tx, &workspace).await?.ok_or_else(|| {
        anyhow::anyhow!(
            "the workspace {} has no unfollowed brief and nothing posted since its last session \
             ended, so this session has no instruction",
            workspace.id
        )
    })?;
    let sent = sent_on(
        &mut tx,
        session,
        Instruction::Start {
            checkout: workspace.checkout.clone(),
            prompt,
            harness,
        },
    )
    .await?;
    tx.workspaces().first_turn(session).await?;
    tx.commit().await?;

    Ok(sent)
}

/// A Brief nothing has followed, exactly as it was written; otherwise the message or messages
/// that started this Session, with everything the Transcript held before them labeled as context
/// ahead of them.
async fn instruction(tx: &mut Tx<'_>, workspace: &Workspace) -> Result<Option<String>> {
    if let Some(brief) = tx.log().unfollowed_brief(workspace).await? {
        return Ok(Some(brief));
    }

    let messages = tx.log().starting_messages(workspace).await?;
    if messages.is_empty() {
        return Ok(None);
    }
    let instruction = crate::work::follow_up(&messages);

    let context = tx.log().context_before_starting(workspace).await?;
    if context.is_empty() {
        return Ok(Some(instruction));
    }
    let context = context
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");

    Ok(Some(format!(
        "Earlier context, oldest first:\n{context}\n\n{instruction}"
    )))
}

/// The next turn of a waiting Session, in the same agent conversation (ADR-0024).
pub(crate) async fn prompt(tx: &mut Tx<'_>, session: &Session, prompt: String) -> Result<()> {
    sent_on(tx, session, Instruction::Prompt { prompt }).await?;
    tx.workspaces()
        .prompt_turn(session)
        .await?
        .ok_or_else(|| anyhow::anyhow!("the session {} is not waiting for a prompt", session.id))?;

    Ok(())
}

/// Sealing needs every Session ended first, which is what makes this refusal unreachable.
pub async fn instruct(
    store: &Store,
    session: &Session,
    instruction: Instruction,
) -> Result<SentInstruction> {
    let mut tx = store.begin().await?;
    let workspace = tx.workspaces().get(session.workspace).await?;
    workspace.accepts("turn")?;
    let sent = sent_on(&mut tx, session, instruction).await?;
    tx.commit().await?;

    Ok(sent)
}

async fn sent_on(
    tx: &mut Tx<'_>,
    session: &Session,
    instruction: Instruction,
) -> Result<SentInstruction> {
    tx.workspaces()
        .send_instruction(session, instruction)
        .await?
        .ok_or_else(|| anyhow::anyhow!("the session {} is on no instance to instruct", session.id))
}

async fn instructions(
    State(control_plane): State<ControlPlane>,
    Path(instance): Path<String>,
    headers: HeaderMap,
) -> Result<Sse<impl Stream<Item = Result<Event, BoxError>>>, Refused> {
    let (linked, digest) = authenticated(&control_plane, &headers, &instance).await?;
    let mut cursor = last_event_id(&headers);
    info!(
        instance = linked.instance,
        cursor, "a supervisor is on the link"
    );

    let connection = control_plane.summaries.connected(&linked.instance);
    let (mut asked, reading) = control_plane.reads.connected(&linked.instance);
    let stream = async_stream::try_stream! {
        let _connection = connection;
        let _reading = reading;
        loop {
            let waiting = waiting(&control_plane.store, &linked.instance, &digest, cursor).await?;
            for sent in waiting.instructions {
                cursor = sent.seq;
                yield Event::default()
                    .id(sent.seq.to_string())
                    .event(sent.instruction.kind())
                    .json_data(Carried {
                        session: sent.session,
                        instruction: &sent.instruction,
                    })?;
            }

            if !waiting.still_linked {
                break;
            }

            let read = tokio::select! {
                () = tokio::time::sleep(POLL) => None,
                read = asked.recv() => read,
                () = control_plane.shutdown.cancelled() => break,
            };
            if let Some(read) = read {
                yield Event::default().event("read").json_data(&read)?;
            }
        }
    };

    Ok(Sse::new(stream).keep_alive(KeepAlive::new().interval(KEEP_ALIVE)))
}

/// The Provider Credentials of the Session's Organization and the Subscription Profile its
/// Workspace names, decrypted here and held nowhere else: a supervisor asks as it spawns a
/// Session's Harness, and one between Sessions has nothing to ask for (ADR-0010).
async fn credentials(
    State(control_plane): State<ControlPlane>,
    Path(instance): Path<String>,
    Query(asking): Query<Asking>,
    headers: HeaderMap,
) -> Result<Json<Credentials>, Refused> {
    let (linked, _) = authenticated(&control_plane, &headers, &instance).await?;
    let session = carried(&control_plane, &linked, &asking.session).await?;
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
        instance = linked.instance,
        variables = variables.keys().cloned().collect::<Vec<_>>().join(", "),
        files = files.keys().cloned().collect::<Vec<_>>().join(", "),
        "an instance took the credentials its session needs"
    );

    Ok(Json(Credentials { variables, files }))
}

async fn refresh_credentials(
    State(control_plane): State<ControlPlane>,
    Path(instance): Path<String>,
    Query(asking): Query<Asking>,
    headers: HeaderMap,
    Json(refreshed): Json<Refreshed>,
) -> Result<StatusCode, Refused> {
    let (linked, _) = authenticated(&control_plane, &headers, &instance).await?;
    let session = carried(&control_plane, &linked, &asking.session).await?;
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
        "an instance handed back the logins its harness refreshed"
    );

    Ok(StatusCode::NO_CONTENT)
}

async fn entries(
    State(control_plane): State<ControlPlane>,
    Path(instance): Path<String>,
    Query(paging): Query<Paging>,
    headers: HeaderMap,
) -> Result<Json<Entries>, Refused> {
    let (linked, _) = authenticated(&control_plane, &headers, &instance).await?;
    let from = paging
        .cursor
        .as_deref()
        .map(str::parse::<Cursor>)
        .transpose()
        .map_err(|error| Refused::BadRequest(error.to_string()))?;
    let window = Window::or_default(paging.window)
        .map_err(|error| Refused::BadRequest(error.to_string()))?;

    let kinds = paging
        .kinds
        .as_deref()
        .map(str::parse::<crate::log::Kinds>)
        .transpose()
        .map_err(|error| Refused::BadRequest(error.to_string()))?
        .unwrap_or_default();
    let mut tx = control_plane.store.read().await?;
    let workspace = tx.workspaces().get(linked.workspace).await?;
    let page = tx
        .log()
        .stored_page(&workspace, from, window, &kinds)
        .await?;

    Ok(Json(Entries {
        entries: page
            .entries
            .into_iter()
            .map(|entry| Recorded {
                kind: entry.kind,
                session_id: entry.session_id,
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
    Path(instance): Path<String>,
    headers: HeaderMap,
    Json(reported): Json<Reported>,
) -> Result<Response, Refused> {
    let (linked, _) = authenticated(&control_plane, &headers, &instance).await?;
    if let work::Report::Work { repositories } = reported.report {
        control_plane.summaries.report(&instance, repositories);
        return Ok(StatusCode::ACCEPTED.into_response());
    }
    if let work::Report::SessionState {
        tools,
        message_buffering,
        thought_buffering,
    } = &reported.report
    {
        work::report(&control_plane.store, &linked.instance, reported.clone()).await?;
        let session = reported.session.expect("validated session report");
        control_plane.summaries.report_session(
            &instance,
            &session.to_string(),
            crate::live_work::SessionState {
                tools: tools.clone(),
                message_buffering: *message_buffering,
                thought_buffering: *thought_buffering,
            },
        );
        return Ok(StatusCode::ACCEPTED.into_response());
    }
    let connected = matches!(reported.report, work::Report::Connected { .. });
    work::report(&control_plane.store, &linked.instance, reported).await?;
    if connected {
        let checkout = control_plane
            .store
            .read()
            .await?
            .workspaces()
            .get(linked.workspace)
            .await?
            .checkout;
        return Ok((StatusCode::ACCEPTED, Json(checkout)).into_response());
    }
    Ok(StatusCode::ACCEPTED.into_response())
}

async fn answer(
    State(control_plane): State<ControlPlane>,
    Path((instance, request)): Path<(String, String)>,
    headers: HeaderMap,
    body: Body,
) -> Result<StatusCode, Refused> {
    authenticated(&control_plane, &headers, &instance).await?;
    let request = request
        .parse()
        .map_err(|_| Refused::BadRequest(format!("{request} is not a read")))?;
    let json = headers
        .get(header::CONTENT_TYPE)
        .and_then(|kind| kind.to_str().ok())
        .is_some_and(|kind| kind.starts_with("application/json"));
    let taken = control_plane
        .reads
        .answer(&instance, request, body, json)
        .ok_or_else(|| Refused::Gone("no read is waiting on this answer".to_owned()))?;
    let _ = taken.await;

    Ok(StatusCode::NO_CONTENT)
}

async fn waiting(store: &Store, instance: &str, digest: &str, cursor: i64) -> Result<Waiting> {
    let mut tx = store.read().await?;
    let still_linked = tx
        .workspaces()
        .linked(digest)
        .await?
        .is_some_and(|linked| linked.instance == instance);

    Ok(Waiting {
        instructions: tx.workspaces().instructions_after(instance, cursor).await?,
        still_linked,
    })
}

async fn authenticated(
    control_plane: &ControlPlane,
    headers: &HeaderMap,
    instance: &str,
) -> Result<(Linked, String), Refused> {
    let secret = bearer(headers).ok_or(Refused::Unauthorized("no credential presented"))?;
    let digest = secret.digest();

    let linked = control_plane
        .store
        .read()
        .await?
        .workspaces()
        .linked(&digest)
        .await?
        .ok_or(Refused::Unauthorized(
            "no credential kestrel issued for an instance it still holds",
        ))?;
    if linked.instance != instance {
        return Err(Refused::Forbidden(
            "the credential belongs to another instance",
        ));
    }

    Ok((linked, digest))
}

/// A Session whose lease has lapsed is refused as if it had ended, since the sweep is about to end it.
async fn carried(
    control_plane: &ControlPlane,
    linked: &Linked,
    session: &str,
) -> Result<Session, Refused> {
    let id: SessionId = session
        .parse()
        .map_err(|_| Refused::BadRequest(format!("{session} is not a session")))?;

    control_plane
        .store
        .read()
        .await?
        .workspaces()
        .carried(&linked.instance, id)
        .await?
        .ok_or_else(|| Refused::Gone(ReportRefused::Gone(id).to_string()))
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
    Unauthorized(&'static str),
    Forbidden(&'static str),
    Gone(String),
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
            ReportRefused::MissingSession
            | ReportRefused::MissingSequence
            | ReportRefused::SkippedSequence(_) => Self::BadRequest(refused.to_string()),
            ReportRefused::Gone(_) => Self::Gone(refused.to_string()),
            ReportRefused::Unavailable(error) => Self::Unavailable(error),
        }
    }
}

impl IntoResponse for Refused {
    fn into_response(self) -> Response {
        let busy = matches!(&self, Refused::Unavailable(error) if store::busy(error));
        let (status, message) = match self {
            Refused::BadRequest(why) => (StatusCode::BAD_REQUEST, why),
            Refused::Unauthorized(why) => (StatusCode::UNAUTHORIZED, why.to_owned()),
            Refused::Forbidden(why) => (StatusCode::FORBIDDEN, why.to_owned()),
            Refused::Gone(why) => (StatusCode::GONE, why),
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
        let sent = serde_json::json!({"kind": "said", "completion": {"started_at": "2026-09-29T12:00:00Z", "finished_at": "2026-09-29T12:00:00Z", "turn_outcome": null}, "seq": 3, "message": "what it said"});

        let reported: Reported = serde_json::from_value(sent.clone()).expect("a report");

        assert_eq!(reported.seq, Some(3));
        assert_eq!(
            reported.report,
            Report::Said {
                message: "what it said".to_owned(),
                completion: crate::log::Completion::at("2026-09-29T12:00:00Z".parse().unwrap())
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
