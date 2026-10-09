//! The boundary a Client reaches the control plane over, specified by `openapi/operator.json`.
//! It authenticates nobody, so it is served apart from the link and on loopback (ADR-0015).

use std::collections::HashMap;
use std::net::IpAddr;
use std::time::Duration;

use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, Query, Request, State};
use axum::http::header::{CONTENT_TYPE, ETAG, HOST, IF_NONE_MATCH, ORIGIN};
use axum::http::uri::Authority;
use axum::http::{HeaderMap, StatusCode};
use axum::middleware::{self, Next};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post, put};
use axum::{BoxError, Json, Router};
use futures_core::Stream;
use jiff::{SignedDuration, Timestamp};
use kestrel_operator_types as wire;
use serde::{Deserialize, Serialize};
use sha2::Digest as _;
use tokio_stream::{StreamExt as _, StreamMap};
use tokio_util::sync::CancellationToken;
use tracing::warn;

use crate::agent;
use crate::cron::Cron;
use crate::declaration;
use crate::declined::{
    self as declined, Concerning, Consequence, Constraint, Declined, Locator, Reason, Resource,
    Step,
};
use crate::domain::{
    self, Agent, Connection, Correlation, Declared, Direction, EventRecordId, EventRefusal, Fires,
    Firing, HeldMessage, Integration, Occurrence, Organization, Project, Schedule, Session,
    StartedBy, SubscriptionProfile, Templates, Trigger, Workspace, WorkspaceId, WorkspaceState,
};
use crate::fanout;
use crate::filter::Filter;
use crate::integration::github::{self, Github};
use crate::integration::{self, Connecting, Registration};
use crate::log::{self, Cursor, Unreadable, Window};
use crate::participant;
use crate::profile::{self, Entry};
use crate::provider::{self, Held};
use crate::role::serve;
use crate::scheduling;
use crate::store::workspace::HeldMessageRefusal;
use crate::store::{self, Declared as DeclaredRecord, Store};
use crate::stream::{self, Emitted};
use crate::template::Template;
use crate::trigger::{self, apply};
use crate::{instance, pull_request, start, work, workspace};

pub const HARNESSES: &str = "/operator/harnesses";
pub const SIGN_IN_METHOD: &str = "/operator/harnesses/{harness}/sign-in-methods/{method}";

pub const ORGANIZATIONS: &str = "/operator/organizations";
pub const STARTS: &str = "/operator/starts";
pub const PROJECTS: &str = "/operator/organizations/{organization}/projects";
pub const AGENTS: &str = "/operator/organizations/{organization}/agents";
pub const AGENT_MODEL: &str = "/operator/organizations/{organization}/agents/{agent}/model";
pub const DECLARATION: &str = "/operator/organizations/{organization}/declaration";
pub const DECLARATION_PREVIEW: &str = "/operator/organizations/{organization}/declaration/preview";
pub const CREDENTIALS: &str = "/operator/organizations/{organization}/credentials";
pub const CREDENTIAL: &str = "/operator/organizations/{organization}/credentials/{variable}";
pub const PROFILES: &str = "/operator/organizations/{organization}/profiles";
pub const PROFILE_VARIABLE: &str =
    "/operator/organizations/{organization}/profiles/{profile}/variables/{variable}";
/// One segment, with the path's slashes percent-encoded in it.
pub const PROFILE_FILE: &str =
    "/operator/organizations/{organization}/profiles/{profile}/files/{path}";
pub const GITHUB_APP: &str = "/operator/organizations/{organization}/github-app";
pub const INTEGRATIONS: &str = "/operator/organizations/{organization}/integrations";
pub const EVENT_REFUSAL: &str =
    "/operator/organizations/{organization}/integrations/{integration}/event-refusal";
pub const EVENTS: &str = "/operator/organizations/{organization}/events";
pub const EVENT: &str = "/operator/events/{record}";
pub const TRIGGERS: &str = "/operator/organizations/{organization}/triggers";
pub const TRIGGER: &str = "/operator/organizations/{organization}/triggers/{trigger}";
pub const TRIGGER_TEST: &str = "/operator/organizations/{organization}/triggers/{trigger}/test";
pub const TRIGGER_DISABLE: &str =
    "/operator/organizations/{organization}/triggers/{trigger}/disable";
pub const TRIGGER_ENABLE: &str = "/operator/organizations/{organization}/triggers/{trigger}/enable";
pub const TRIGGER_DISPATCH: &str =
    "/operator/organizations/{organization}/triggers/{trigger}/dispatch";
pub const APPLIED_TRIGGERS: &str = "/operator/organizations/{organization}/applied-triggers";
pub const APPLIED_TRIGGERS_PREVIEW: &str =
    "/operator/organizations/{organization}/applied-triggers/preview";
pub const INSTANCES: &str = "/operator/organizations/{organization}/instances";
pub const WORKSPACES: &str = "/operator/organizations/{organization}/workspaces";
pub const WORKSPACE: &str = "/operator/organizations/{organization}/workspaces/{workspace}";
pub const WORKSPACE_WORK: &str =
    "/operator/organizations/{organization}/workspaces/{workspace}/work";
pub const WORKSPACE_CHANGES: &str =
    "/operator/organizations/{organization}/workspaces/{workspace}/changes";
pub const WORKSPACE_COMMITS: &str =
    "/operator/organizations/{organization}/workspaces/{workspace}/commits";
pub const WORKSPACE_STASHES: &str =
    "/operator/organizations/{organization}/workspaces/{workspace}/stashes";
pub const WORKSPACE_FILES: &str =
    "/operator/organizations/{organization}/workspaces/{workspace}/files";
pub const WORKSPACE_FILE: &str =
    "/operator/organizations/{organization}/workspaces/{workspace}/file";
pub const WORKSPACE_MESSAGES: &str =
    "/operator/organizations/{organization}/workspaces/{workspace}/messages";
pub const WORKSPACE_MESSAGE: &str =
    "/operator/organizations/{organization}/workspaces/{workspace}/messages/{id}";
pub const WORKSPACE_SEAL: &str =
    "/operator/organizations/{organization}/workspaces/{workspace}/seal";
pub const WORKSPACE_INSTANCE_RELEASE: &str =
    "/operator/organizations/{organization}/workspaces/{workspace}/instance/release";
pub const SESSIONS: &str = "/operator/organizations/{organization}/workspaces/{workspace}/sessions";
pub const SESSION: &str = "/operator/organizations/{organization}/sessions/{session}";
pub const SESSION_INTERRUPT: &str =
    "/operator/organizations/{organization}/sessions/{session}/interrupt";
pub const SESSION_STOP: &str = "/operator/organizations/{organization}/sessions/{session}/stop";
pub const SESSION_OPTIONS: &str =
    "/operator/organizations/{organization}/sessions/{session}/options";
pub const TRANSCRIPT: &str =
    "/operator/organizations/{organization}/workspaces/{workspace}/transcript";
pub const TRANSCRIPT_PAYLOAD: &str =
    "/operator/organizations/{organization}/workspaces/{workspace}/transcript/payloads/{payload}";
pub const FOLLOWER_LEASE: &str =
    "/operator/organizations/{organization}/workspaces/{workspace}/followers/{id}/lease";
pub const CHANGES: &str = "/operator/organizations/{organization}/changes";
pub const STREAMS: &str = "/operator/streams";
pub const STREAM: &str = "/operator/streams/{token}";
pub const STREAM_SUBSCRIPTION: &str = "/operator/streams/{token}/subscriptions/{id}";
/// One read of a Workspace's queue: the limits, their occupancy, and the queued Sessions in
/// dispatch order.
pub const QUEUE: &str = "/operator/organizations/{organization}/queue";

const EVENTS_LISTED: usize = 50;

const POLL: Duration = Duration::from_millis(100);
const KEEP_ALIVE: Duration = Duration::from_secs(15);

#[derive(Clone)]
struct ControlPlane {
    store: Store,
    shutdown: CancellationToken,
    live: crate::live::Live,
    followers: crate::presence::Followers,
    streams: stream::Streams,
}

#[derive(Deserialize)]
struct Browsing {
    path: Option<String>,
}

#[derive(Deserialize)]
struct Reading {
    path: String,
    #[serde(default)]
    raw: bool,
}

#[derive(Deserialize)]
struct Following {
    summaries: Option<bool>,
    first_seq: Option<i64>,
    last_seq: Option<i64>,
    follow: Option<bool>,
    kinds: Option<String>,
    #[serde(rename = "as")]
    as_name: Option<String>,
}

#[derive(Serialize)]
struct Recorded {
    kind: log::Kind,
    session_id: Option<crate::domain::SessionId>,
    seq: i64,
    appended_at: String,
    entry: serde_json::Value,
}

#[derive(Serialize)]
struct End {
    because: Because,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum Because {
    CaughtUp,
    Sealed,
}

#[derive(Serialize)]
struct FollowerEvent {
    id: crate::presence::FollowerId,
    lease_seconds: u64,
}

#[derive(Serialize)]
struct Refetch {}

// The Workspace's name, because a Client addresses every read by it.
#[derive(Serialize)]
#[serde(tag = "resource", rename_all = "snake_case")]
enum Changed {
    Workspace {
        id: WorkspaceId,
        workspace: String,
    },
    Session {
        id: crate::domain::SessionId,
        workspace: String,
    },
    Queue,
}

impl Changed {
    async fn named(store: &Store, resource: fanout::Resource) -> anyhow::Result<Self> {
        let mut tx = store.read().await?;
        Ok(match resource {
            fanout::Resource::Workspace(id) => Self::Workspace {
                id,
                workspace: tx.workspaces().get(id).await?.name,
            },
            fanout::Resource::Session(id) => {
                let session = tx.workspaces().session(id).await?;
                Self::Session {
                    id,
                    workspace: tx.workspaces().get(session.workspace).await?.name,
                }
            }
            fanout::Resource::Queue => Self::Queue,
        })
    }
}

struct Read {
    page: log::TranscriptPage,
    session_state: TranscriptSessionState,
    sealed: bool,
}

#[derive(Clone, PartialEq, Serialize)]
struct TranscriptSessionState {
    session_id: Option<domain::SessionId>,
    #[serde(flatten)]
    state: crate::live_work::SessionState,
    observation: crate::live_work::Observation,
}

async fn harnesses() -> Json<Vec<wire::HarnessCatalogueEntry>> {
    Json(crate::catalogue::harnesses().to_vec())
}

async fn sign_in_method(
    Path((harness, method)): Path<(String, String)>,
) -> Result<Json<wire::SignInMethod>, Refused> {
    crate::catalogue::sign_in_method(&harness, &method)
        .cloned()
        .map(Json)
        .map_err(Into::into)
}

pub fn router(
    store: Store,
    shutdown: CancellationToken,
    live: crate::live::Live,
    followers: crate::presence::Followers,
    streams: stream::Streams,
) -> Router {
    Router::new()
        .route(HARNESSES, get(harnesses))
        .route(SIGN_IN_METHOD, get(sign_in_method))
        .route(ORGANIZATIONS, get(organizations).post(declare_organization))
        .route(STARTS, post(start))
        .route(PROJECTS, get(projects).post(declare_project))
        .route(AGENTS, get(agents).post(declare_agent))
        .route(AGENT_MODEL, put(set_agent_model))
        .route(DECLARATION, post(apply_declaration))
        .route(DECLARATION_PREVIEW, post(preview_declaration))
        .route(CREDENTIALS, get(credentials))
        .route(CREDENTIAL, put(hold_credential).delete(forget_credential))
        .route(PROFILES, get(profiles).post(declare_profile))
        .route(
            PROFILE_VARIABLE,
            put(hold_profile_variable).delete(forget_profile_variable),
        )
        .route(
            PROFILE_FILE,
            put(hold_profile_file).delete(forget_profile_file),
        )
        .route(INTEGRATIONS, get(integrations).post(register_integration))
        .route(GITHUB_APP, post(start_github_app))
        .route(integration::manifest::PAGE, get(github_app_page))
        .route(integration::manifest::CALLBACK, get(github_app_callback))
        .route(integration::manifest::INSTALLED, get(github_app_installed))
        .route(EVENT_REFUSAL, delete(acknowledge_event_refusal))
        .route(EVENTS, get(events))
        .route(EVENT, get(event))
        .route(TRIGGERS, get(triggers).post(declare_trigger))
        .route(TRIGGER, get(show_trigger))
        .route(TRIGGER_TEST, post(test_trigger))
        .route(TRIGGER_DISABLE, post(disable_trigger))
        .route(TRIGGER_ENABLE, post(enable_trigger))
        .route(TRIGGER_DISPATCH, post(dispatch_trigger))
        .route(APPLIED_TRIGGERS, post(apply_triggers))
        .route(APPLIED_TRIGGERS_PREVIEW, post(preview_applied_triggers))
        .route(INSTANCES, get(instances))
        .route(QUEUE, get(show_queue))
        .route(WORKSPACES, get(workspaces).post(open_workspace))
        .route(WORKSPACE, get(show_workspace))
        .route(WORKSPACE_WORK, get(work_summary))
        .route(WORKSPACE_FILES, get(workspace_files))
        .route(WORKSPACE_FILE, get(workspace_file))
        .route(WORKSPACE_CHANGES, get(workspace_changes))
        .route(WORKSPACE_COMMITS, get(workspace_commits))
        .route(WORKSPACE_STASHES, get(workspace_stashes))
        .route(WORKSPACE_MESSAGES, post(post_to_workspace))
        .route(
            WORKSPACE_MESSAGE,
            put(edit_workspace_message).delete(withdraw_workspace_message),
        )
        .route(WORKSPACE_SEAL, post(seal_workspace))
        .route(WORKSPACE_INSTANCE_RELEASE, post(release_instance))
        .route(SESSIONS, get(sessions).post(enqueue_session))
        .route(SESSION, get(show_session))
        .route(SESSION_INTERRUPT, post(interrupt_session))
        .route(SESSION_STOP, post(stop_session))
        .route(SESSION_OPTIONS, post(set_session_option))
        .route(TRANSCRIPT, get(transcript))
        .route(TRANSCRIPT_PAYLOAD, get(transcript_payload))
        .route(FOLLOWER_LEASE, post(renew_follower))
        .route(CHANGES, get(changes))
        .route(STREAMS, put(reserve_stream))
        .route(STREAM, get(open_stream))
        .route(
            STREAM_SUBSCRIPTION,
            put(subscribe_stream).delete(unsubscribe_stream),
        )
        .with_state(ControlPlane {
            store,
            shutdown,
            live,
            followers,
            streams,
        })
        .layer(middleware::from_fn(diagnosing))
        .layer(middleware::from_fn(addressed_here))
}

async fn addressed_here(request: Request, next: Next) -> Response {
    match addressed_from_here(&request) {
        Ok(()) => next.run(request).await,
        Err(refused) => refused.into_response(),
    }
}

fn addressed_from_here(request: &Request) -> Result<(), Refused> {
    let host = request
        .headers()
        .get(HOST)
        .and_then(|host| host.to_str().ok())
        .or_else(|| request.uri().authority().map(Authority::as_str))
        .filter(|host| is_loopback_host(host))
        .ok_or_else(|| {
            Refused::Forbidden("the request names a host other than this control plane".to_owned())
        })?;

    match request.headers().get(ORIGIN).map(|origin| origin.to_str()) {
        None => Ok(()),
        // Either scheme, because the web server in front terminates TLS and forwards plain HTTP.
        Some(Ok(origin))
            if ["http", "https"]
                .iter()
                .any(|scheme| origin.eq_ignore_ascii_case(&format!("{scheme}://{host}"))) =>
        {
            Ok(())
        }
        Some(_) => Err(Refused::Forbidden(
            "the request comes from an origin other than this control plane".to_owned(),
        )),
    }
}

// Any port, because compose and a tunnel publish the boundary on one it was never bound to. The
// allowlist is explicit, so a name a browser resolves to loopback is still refused.
fn is_loopback_host(host: &str) -> bool {
    let Ok(authority) = host.parse::<Authority>() else {
        return false;
    };
    if authority.as_str().contains('@') {
        return false;
    }

    let name = authority.host();
    name.eq_ignore_ascii_case("localhost")
        || name
            .trim_start_matches('[')
            .trim_end_matches(']')
            .parse::<IpAddr>()
            .is_ok_and(|address| address.is_loopback())
}

#[derive(Deserialize)]
struct OrganizationDeclaration {
    name: String,
    max_live_instances: Option<std::num::NonZeroUsize>,
}

#[derive(Deserialize)]
struct ProjectDeclaration {
    name: String,
    repositories: Vec<String>,
    branch: String,
}

#[derive(Deserialize)]
struct AgentDeclaration {
    name: String,
    harness: String,
    model: Option<String>,
    mode: Option<String>,
    thought_level: Option<String>,
}

impl AgentDeclaration {
    fn declared(&self) -> Declared {
        Declared::named(Declared {
            model: self.model.clone(),
            mode: self.mode.clone(),
            thought_level: self.thought_level.clone(),
        })
    }
}

#[derive(Deserialize)]
struct WorkspaceDeclaration {
    project: String,
    agent: String,
    profile: Option<String>,
    branch: Option<String>,
    continues: Option<String>,
    model: Option<String>,
    mode: Option<String>,
    thought_level: Option<String>,
    brief: Option<String>,
    participant: Option<String>,
    depends_on: Option<Vec<String>>,
}

impl WorkspaceDeclaration {
    fn declared(&self) -> Declared {
        Declared {
            model: self.model.clone(),
            mode: self.mode.clone(),
            thought_level: self.thought_level.clone(),
        }
    }
}

#[derive(Deserialize)]
struct WorkspaceMessage {
    participant: Option<String>,
    message: String,
}

#[derive(Deserialize)]
struct WorkspaceMessageWithdrawal {
    participant: Option<String>,
}

#[derive(Deserialize)]
struct SessionDeclaration {
    agent: Option<String>,
    model: Option<String>,
    mode: Option<String>,
    thought_level: Option<String>,
    depends_on: Option<Vec<String>>,
}

#[derive(Deserialize)]
struct SessionInterrupt {
    participant: Option<String>,
}

impl SessionDeclaration {
    fn declared(&self) -> Declared {
        Declared {
            model: self.model.clone(),
            mode: self.mode.clone(),
            thought_level: self.thought_level.clone(),
        }
    }
}

#[derive(Deserialize)]
struct OptionChange {
    participant: String,
    option: Option<String>,
    category: Option<String>,
    value: String,
}

#[derive(Deserialize)]
struct TriggerDeclaration {
    name: String,
    filter: Option<serde_json::Value>,
    every: Option<String>,
    cron: Option<String>,
    zone: Option<String>,
    brief: String,
    branch: Option<String>,
    correlation: Option<String>,
    on_miss: Option<String>,
    on_open_workspace: Option<String>,
    project: String,
    agent: String,
    #[serde(default)]
    allows: Vec<String>,
    profile: Option<String>,
    model: Option<String>,
    mode: Option<String>,
    thought_level: Option<String>,
}

impl TriggerDeclaration {
    fn declared(&self) -> Declared {
        Declared::named(Declared {
            model: self.model.clone(),
            mode: self.mode.clone(),
            thought_level: self.thought_level.clone(),
        })
    }
}

#[derive(Deserialize)]
struct TriggerTest {
    event: Option<String>,
    integration: Option<String>,
    issue: Option<i64>,
    instruction: Option<String>,
    agent: Option<String>,
    declared: Option<apply::File>,
}

#[derive(Deserialize)]
struct TriggerDispatch {
    integration: String,
    issue: i64,
    instruction: Option<String>,
    agent: Option<String>,
}

#[derive(Deserialize)]
struct AgentModel {
    model: Option<String>,
}

#[derive(Deserialize)]
struct Release {
    #[serde(default = "default_operator_participant")]
    participant: String,
}

fn default_operator_participant() -> String {
    "operator".to_owned()
}

#[derive(Serialize)]
struct OrganizationRecord {
    id: String,
    name: String,
    max_live_instances: Option<std::num::NonZeroUsize>,
}

#[derive(Serialize)]
struct ProjectRecord {
    id: String,
    name: String,
    repositories: Vec<String>,
    branch: String,
}

#[derive(Serialize)]
struct AgentRecord {
    id: String,
    name: String,
    harness: String,
    model: Option<String>,
    mode: Option<String>,
    thought_level: Option<String>,
}

#[derive(Serialize)]
struct WorkspaceRecord {
    id: String,
    name: String,
    organization: String,
    project: String,
    opened_with: String,
    profile: Option<String>,
    checkout: domain::Checkout,
    instance: Option<String>,
    held: Option<String>,
    correlation: Option<String>,
    state: String,
    opened_at: Timestamp,
    last_active_at: Timestamp,
    sealed_at: Option<Timestamp>,
    continues: Option<String>,
    started_by: Option<String>,
    continued_by: Vec<String>,
    held_messages: Vec<HeldMessage>,
    pull_requests: Vec<PullRequestAvailabilityRecord>,
    unfinished_session: Option<UnfinishedSessionRecord>,
}

#[derive(Serialize)]
struct WorkspaceListedRecord {
    #[serde(flatten)]
    workspace: WorkspaceRecord,
    session: Option<SessionRecord>,
    queue: Option<QueueStandingRecord>,
}

#[derive(Serialize)]
struct QueueStandingRecord {
    position: Option<usize>,
    reasons: Vec<ReasonRecord>,
    pending_since: Option<Timestamp>,
}

impl QueueStandingRecord {
    fn of(snapshot: &scheduling::Snapshot, workspace: WorkspaceId) -> Option<Self> {
        if let Some(queued) = snapshot
            .queued
            .iter()
            .find(|queued| queued.session.workspace == workspace)
        {
            return Some(Self {
                position: queued.position,
                reasons: reasons(queued.reasons.clone()),
                pending_since: None,
            });
        }
        if let Some(waiting) = snapshot
            .waiting
            .iter()
            .find(|waiting| waiting.session.workspace == workspace)
        {
            return Some(Self {
                position: waiting.position,
                reasons: reasons(waiting.reasons.clone()),
                pending_since: waiting.pending_since,
            });
        }
        snapshot
            .unbriefed
            .iter()
            .find(|unbriefed| unbriefed.session.workspace == workspace)
            .map(|unbriefed| Self {
                position: unbriefed.position,
                reasons: reasons(unbriefed.reasons.clone()),
                pending_since: unbriefed.brief_since.or(unbriefed.pending_since),
            })
    }
}

#[derive(Serialize)]
struct UnfinishedSessionRecord {
    id: String,
    name: String,
    state: String,
    preparing: Option<String>,
}

#[derive(Serialize)]
struct PostedRecord {
    session: Option<SessionRecord>,
    held_message: Option<HeldMessage>,
}

#[derive(Serialize)]
struct PullRequestAvailabilityRecord {
    repository: String,
    availability: &'static str,
    known: Option<Vec<PullRequestRecord>>,
}

#[derive(Serialize)]
struct PullRequestRecord {
    repository: String,
    number: i64,
    url: String,
    title: String,
    state: domain::PullRequestState,
    head_branch: String,
    head_revision: String,
    updated_at: Timestamp,
    event: String,
}

impl From<pull_request::Availability> for PullRequestAvailabilityRecord {
    fn from(availability: pull_request::Availability) -> Self {
        Self {
            repository: availability.repository,
            availability: match availability.known {
                Some(_) => "available",
                None => "unavailable",
            },
            known: availability.known.map(|known| {
                known
                    .into_iter()
                    .map(|pull_request| PullRequestRecord {
                        repository: pull_request.repository,
                        number: pull_request.number,
                        url: pull_request.url,
                        title: pull_request.title,
                        state: pull_request.state,
                        head_branch: pull_request.head_branch,
                        head_revision: pull_request.head_revision,
                        updated_at: pull_request.updated_at,
                        event: pull_request.event.to_string(),
                    })
                    .collect()
            }),
        }
    }
}

#[derive(Serialize)]
struct SessionRecord {
    id: String,
    name: String,
    workspace: String,
    state: String,
    preparing: Option<String>,
    exit: Option<domain::Exit>,
    outcome_message: Option<String>,
    instance: Option<String>,
    supervisor: Option<String>,
    agent: String,
    harness: String,
    model: Option<String>,
    mode: Option<String>,
    thought_level: Option<String>,
    worked_model: Option<String>,
    title: Option<String>,
    options: Vec<SessionOptionRecord>,
    changing_options: Vec<domain::ChangingOption>,
    commands: Vec<domain::SessionCommand>,
    interrupting: Option<domain::Interrupting>,
    enqueued_at: Timestamp,
    started_at: Option<Timestamp>,
    ended_at: Option<Timestamp>,
    lease_expires_at: Option<Timestamp>,
    connected_at: Option<Timestamp>,
    supervisor_version: Option<String>,
    usage: Option<domain::Usage>,
    depends_on: Vec<SessionReferenceRecord>,
    tools: Vec<crate::live_work::RunningTool>,
    units: Vec<crate::live_work::RunningUnit>,
    message_buffering: bool,
    thought_buffering: bool,
    last_activity_at: Option<Timestamp>,
    observation: crate::live_work::Observation,
}

#[derive(Serialize)]
struct SessionReferenceRecord {
    id: String,
    name: String,
}

#[derive(Serialize)]
struct SessionOptionRecord {
    #[serde(flatten)]
    option: domain::SessionOption,
    warns_cache: bool,
}

/// ADR-0041: no adapter reports which option categories keep the prompt cache, so kestrel lists
/// them per harness.
const CACHE_KEPT_ACROSS: &[(&str, &str)] = &[("claude", domain::SessionOption::THOUGHT_LEVEL)];

fn warns_cache(harness: &str, category: Option<&str>) -> bool {
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

#[derive(Serialize)]
struct TriggerRecord {
    id: String,
    organization: String,
    name: String,
    state: String,
    disabled_because: Option<String>,
    firing_budget: FiringBudgetRecord,
    filter: Option<serde_json::Value>,
    every: Option<String>,
    cron: Option<String>,
    zone: Option<String>,
    admits_outsiders: bool,
    brief: String,
    branch: Option<String>,
    correlation: Option<String>,
    on_miss: Option<String>,
    on_open_workspace: String,
    project: String,
    agent: String,
    allows: Vec<String>,
    profile: Option<String>,
    model: Option<String>,
    mode: Option<String>,
    thought_level: Option<String>,
    applied: bool,
    declared_at: Timestamp,
}

#[derive(Serialize)]
struct FiringBudgetRecord {
    limit: usize,
    window: String,
}

#[derive(Serialize)]
struct TriggerTestRecord {
    matches: bool,
    brief: String,
    branch: Option<String>,
    correlation: Option<String>,
    agent: String,
    would: &'static str,
    elapsing: Option<Timestamp>,
}

impl From<Organization> for OrganizationRecord {
    fn from(organization: Organization) -> Self {
        Self {
            id: organization.id.to_string(),
            name: organization.name,
            max_live_instances: organization.max_live_instances,
        }
    }
}

impl From<Project> for ProjectRecord {
    fn from(project: Project) -> Self {
        Self {
            id: project.id.to_string(),
            name: project.name,
            repositories: project.repositories,
            branch: project.branch,
        }
    }
}

impl From<Agent> for AgentRecord {
    fn from(agent: Agent) -> Self {
        Self {
            id: agent.id.to_string(),
            name: agent.name,
            harness: agent.harness,
            model: agent.declared.model,
            mode: agent.declared.mode,
            thought_level: agent.declared.thought_level,
        }
    }
}

impl WorkspaceRecord {
    async fn read(store: &Store, workspace: Workspace) -> Result<Self, Refused> {
        let continued_by = workspace::continuations(store, workspace.id)
            .await?
            .into_iter()
            .map(|workspace| workspace.to_string())
            .collect();
        let instance = work::instance(store, workspace.id).await?;
        let held = instance::held_by(store, workspace.id)
            .await?
            .map(|held| held.because);
        let pull_requests = pull_request::availability(store, &workspace)
            .await?
            .into_iter()
            .map(PullRequestAvailabilityRecord::from)
            .collect();
        let unfinished_session = workspace::unfinished(store, workspace.id)
            .await?
            .map(|session| UnfinishedSessionRecord {
                id: session.id.to_string(),
                name: session.name,
                state: session.state.as_str().to_owned(),
                preparing: session
                    .preparing
                    .map(|preparing| preparing.as_str().to_owned()),
            });
        let held_messages = workspace::held_messages(store, workspace.id).await?;

        Ok(Self {
            id: workspace.id.to_string(),
            name: workspace.name,
            organization: workspace.organization.name,
            project: workspace.project.name,
            opened_with: workspace.opened_with.name,
            profile: workspace.profile.map(|profile| profile.name),
            checkout: workspace.checkout,
            instance,
            held,
            correlation: workspace.correlation,
            state: workspace.state.as_str().to_owned(),
            opened_at: workspace.opened_at,
            last_active_at: workspace.last_active_at,
            sealed_at: workspace.sealed_at,
            continues: workspace.continues.map(|workspace| workspace.to_string()),
            started_by: workspace.started_by.map(|started| match started {
                StartedBy::Event(event) => event.to_string(),
                StartedBy::Participant(participant) => participant,
            }),
            continued_by,
            held_messages,
            pull_requests,
            unfinished_session,
        })
    }
}

impl SessionRecord {
    fn read(session: Session, summaries: &crate::live_work::Summaries) -> Self {
        let (observation, state) = summaries.observed(Some(&session));
        let trailing = session.state == domain::SessionState::Trailing;
        let harness = session.agent.harness.clone();
        let options = session
            .options
            .into_iter()
            .map(|option| SessionOptionRecord {
                warns_cache: warns_cache(&harness, option.category.as_deref()),
                option,
            })
            .collect();
        Self {
            id: session.id.to_string(),
            name: session.name,
            workspace: session.workspace.to_string(),
            state: session.state.as_str().to_owned(),
            preparing: session
                .preparing
                .map(|preparing| preparing.as_str().to_owned()),
            exit: session.exit,
            outcome_message: session.outcome_message,
            instance: session.instance,
            supervisor: session.supervisor,
            agent: session.agent.name,
            harness,
            model: session.agent.declared.model,
            mode: session.agent.declared.mode,
            thought_level: session.agent.declared.thought_level,
            worked_model: session.worked_model,
            title: session.title,
            options,
            changing_options: session.changing_options,
            commands: session.commands,
            interrupting: session.interrupting,
            enqueued_at: session.enqueued_at,
            started_at: session.started_at,
            ended_at: session.ended_at,
            lease_expires_at: session.lease_expires_at,
            connected_at: session.connected.as_ref().map(|connected| connected.at),
            supervisor_version: session.connected.map(|connected| connected.version),
            // The live figure is the Turn in flight; the recorded one is the last Turn answered.
            usage: state.usage.or(session.usage),
            depends_on: session
                .depends_on
                .into_iter()
                .map(|blocker| SessionReferenceRecord {
                    id: blocker.id.to_string(),
                    name: blocker.name,
                })
                .collect(),
            tools: state.tools,
            units: state.units,
            message_buffering: state.message_buffering,
            thought_buffering: state.thought_buffering,
            last_activity_at: state.last_activity_at.filter(|_| trailing),
            observation,
        }
    }
}

impl From<Trigger> for TriggerRecord {
    fn from(trigger: Trigger) -> Self {
        let (filter, every, cron, zone, admits_outsiders) = match trigger.fires {
            Fires::On(filter) => {
                let admits_outsiders = filter.admits_outsiders();
                (Some(filter.to_json()), None, None, None, admits_outsiders)
            }
            Fires::Scheduled(Schedule::Every(every)) => {
                (None, Some(format!("{every:#}")), None, None, false)
            }
            Fires::Scheduled(Schedule::Cron(cron)) => (
                None,
                None,
                Some(cron.to_string()),
                Some(cron.zone().to_owned()),
                false,
            ),
        };

        Self {
            id: trigger.id.to_string(),
            organization: trigger.organization.name,
            name: trigger.name,
            state: trigger.state.as_str().to_owned(),
            disabled_because: trigger.disabled_because,
            firing_budget: FiringBudgetRecord {
                limit: trigger.firing_budget.limit.get(),
                window: format!("{:#}", trigger.firing_budget.window),
            },
            filter,
            every,
            cron,
            zone,
            admits_outsiders,
            brief: trigger.templates.brief.to_string(),
            branch: trigger.templates.branch.map(|branch| branch.to_string()),
            correlation: trigger
                .templates
                .correlation
                .template()
                .map(ToString::to_string),
            on_miss: trigger
                .templates
                .correlation
                .on_miss()
                .map(|miss| miss.as_str().to_owned()),
            on_open_workspace: trigger
                .templates
                .correlation
                .on_open_workspace()
                .as_str()
                .to_owned(),
            project: trigger.project.name,
            agent: trigger.agent.name,
            allows: trigger.allows.into_iter().map(|agent| agent.name).collect(),
            profile: trigger.profile.map(|profile| profile.name),
            model: trigger.declared.model,
            mode: trigger.declared.mode,
            thought_level: trigger.declared.thought_level,
            applied: trigger.applied,
            declared_at: trigger.declared_at,
        }
    }
}

#[derive(Serialize)]
struct StartedRecord {
    organization: start::Settled,
    project: start::Settled,
    agent: start::Settled,
    workspace: WorkspaceRecord,
    session: SessionRecord,
}

#[derive(Serialize)]
struct OpenedRecord {
    workspace: WorkspaceRecord,
    session: SessionRecord,
}

async fn start(
    State(control_plane): State<ControlPlane>,
    plan: Result<Json<start::Plan>, JsonRejection>,
) -> Result<(StatusCode, Json<StartedRecord>), Refused> {
    let Json(plan) = plan?;
    let started = start::start(&control_plane.store, &plan).await?;

    Ok((
        StatusCode::CREATED,
        Json(StartedRecord {
            organization: started.organization,
            project: started.project,
            agent: started.agent,
            workspace: WorkspaceRecord::read(&control_plane.store, started.workspace).await?,
            session: SessionRecord::read(started.session, &control_plane.live.summaries),
        }),
    ))
}

async fn organizations(
    State(control_plane): State<ControlPlane>,
) -> Result<Json<Vec<OrganizationRecord>>, Refused> {
    let mut tx = control_plane.store.begin().await?;
    let organizations = tx.organizations().all().await?;

    Ok(Json(organizations.into_iter().map(Into::into).collect()))
}

async fn declare_organization(
    State(control_plane): State<ControlPlane>,
    declaration: Result<Json<OrganizationDeclaration>, JsonRejection>,
) -> Result<Response, Refused> {
    let Json(declaration) = declaration?;
    named("declare_organization", &declaration.name)?;

    let mut tx = control_plane.store.begin().await?;
    let declared = tx
        .organizations()
        .declare(&declaration.name, declaration.max_live_instances)
        .await?;
    tx.commit().await?;

    Ok(answered::<_, OrganizationRecord>(declared))
}

async fn projects(
    State(control_plane): State<ControlPlane>,
    Path(organization): Path<String>,
) -> Result<Json<Vec<ProjectRecord>>, Refused> {
    let mut tx = control_plane.store.begin().await?;
    let organization = tx.organizations().named(&organization).await?;
    let projects = tx.projects().all(&organization).await?;

    Ok(Json(projects.into_iter().map(Into::into).collect()))
}

async fn declare_project(
    State(control_plane): State<ControlPlane>,
    Path(organization): Path<String>,
    declaration: Result<Json<ProjectDeclaration>, JsonRejection>,
) -> Result<Response, Refused> {
    let Json(declaration) = declaration?;
    named("declare_project", &declaration.name)?;
    if declaration.repositories.is_empty() {
        return Err(invalid_field(
            "declare_project",
            "repositories",
            Constraint::NonEmpty,
            "a project names at least one repository",
        ));
    }
    if declaration.branch.is_empty() {
        return Err(invalid_field(
            "declare_project",
            "branch",
            Constraint::NonEmpty,
            "a project names the branch its work happens on",
        ));
    }
    if let Some(clash) = sharing_a_directory(&declaration.repositories) {
        return Err(invalid_field(
            "declare_project",
            "repositories",
            Constraint::DistinctCheckoutDirectories,
            clash,
        ));
    }

    let mut tx = control_plane.store.begin().await?;
    let organization = tx.organizations().named(&organization).await?;
    let declared = tx
        .projects()
        .declare(
            &organization,
            &declaration.name,
            &declaration.repositories,
            &declaration.branch,
        )
        .await?;
    tx.commit().await?;

    Ok(answered::<_, ProjectRecord>(declared))
}

async fn agents(
    State(control_plane): State<ControlPlane>,
    Path(organization): Path<String>,
) -> Result<Json<Vec<AgentRecord>>, Refused> {
    let agents = agent::agents(&control_plane.store, &organization).await?;

    Ok(Json(agents.into_iter().map(Into::into).collect()))
}

async fn set_agent_model(
    State(control_plane): State<ControlPlane>,
    Path((organization, name)): Path<(String, String)>,
    model: Result<Json<AgentModel>, JsonRejection>,
) -> Result<Json<AgentRecord>, Refused> {
    let Json(model) = model?;
    let agent = agent::set_model(
        &control_plane.store,
        &organization,
        &name,
        model.model.as_deref(),
    )
    .await
    .map_err(named_refusal)?;

    Ok(Json(agent.into()))
}

async fn declare_agent(
    State(control_plane): State<ControlPlane>,
    Path(organization): Path<String>,
    declaration: Result<Json<AgentDeclaration>, JsonRejection>,
) -> Result<Response, Refused> {
    let Json(declaration) = declaration?;
    named("declare_agent", &declaration.name)?;
    if declaration.harness.is_empty() {
        return Err(invalid_field(
            "declare_agent",
            "harness",
            Constraint::NonEmpty,
            "an agent names the harness that drives it",
        ));
    }

    let declared = agent::declare(
        &control_plane.store,
        &organization,
        &declaration.name,
        &declaration.harness,
        &declaration.declared(),
    )
    .await?;

    Ok(answered::<_, AgentRecord>(declared))
}

async fn apply_declaration(
    State(control_plane): State<ControlPlane>,
    Path(organization): Path<String>,
    declaration: Result<Json<declaration::Document>, JsonRejection>,
) -> Result<Json<declaration::Applied>, Refused> {
    declared_declaration(
        control_plane,
        organization,
        declaration,
        declaration::ApplyMode::Apply,
    )
    .await
}

async fn preview_declaration(
    State(control_plane): State<ControlPlane>,
    Path(organization): Path<String>,
    declaration: Result<Json<declaration::Document>, JsonRejection>,
) -> Result<Json<declaration::Applied>, Refused> {
    declared_declaration(
        control_plane,
        organization,
        declaration,
        declaration::ApplyMode::Preview,
    )
    .await
}

async fn declared_declaration(
    control_plane: ControlPlane,
    organization: String,
    declaration: Result<Json<declaration::Document>, JsonRejection>,
    mode: declaration::ApplyMode,
) -> Result<Json<declaration::Applied>, Refused> {
    let Json(declaration) = declaration?;
    let applied = declaration::apply(&control_plane.store, &organization, &declaration, mode)
        .await
        .map_err(named_refusal)?;

    Ok(Json(applied))
}

#[derive(Deserialize)]
struct Secret {
    secret: String,
}

#[derive(Serialize)]
struct CredentialRecord {
    variable: String,
    set_at: Timestamp,
}

impl From<Held> for CredentialRecord {
    fn from(held: Held) -> Self {
        Self {
            variable: held.variable,
            set_at: held.set_at,
        }
    }
}

async fn credentials(
    State(control_plane): State<ControlPlane>,
    Path(organization): Path<String>,
) -> Result<Json<Vec<CredentialRecord>>, Refused> {
    let held = provider::held(&control_plane.store, &organization).await?;

    Ok(Json(held.into_iter().map(Into::into).collect()))
}

async fn hold_credential(
    State(control_plane): State<ControlPlane>,
    Path((organization, variable)): Path<(String, String)>,
    secret: Result<Json<Secret>, JsonRejection>,
) -> Result<Json<CredentialRecord>, Refused> {
    let Json(Secret { secret }) = secret?;
    let held = provider::hold(&control_plane.store, &organization, &variable, &secret).await?;

    Ok(Json(held.into()))
}

async fn forget_credential(
    State(control_plane): State<ControlPlane>,
    Path((organization, variable)): Path<(String, String)>,
) -> Result<StatusCode, Refused> {
    provider::forget(&control_plane.store, &organization, &variable).await?;

    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct ProfileDeclaration {
    name: String,
    owner: String,
}

#[derive(Serialize)]
struct ProfileRecord {
    id: String,
    name: String,
    owner: String,
}

#[derive(Serialize)]
struct ListedProfile {
    #[serde(flatten)]
    profile: ProfileRecord,
    holds: Vec<LoginRecord>,
}

#[derive(Serialize)]
struct LoginRecord {
    kind: &'static str,
    name: String,
    set_at: Timestamp,
}

impl From<SubscriptionProfile> for ProfileRecord {
    fn from(profile: SubscriptionProfile) -> Self {
        Self {
            id: profile.id.to_string(),
            name: profile.name,
            owner: profile.owner,
        }
    }
}

impl From<profile::Held> for LoginRecord {
    fn from(held: profile::Held) -> Self {
        Self {
            kind: held.entry.kind.as_str(),
            name: held.entry.name,
            set_at: held.set_at,
        }
    }
}

async fn profiles(
    State(control_plane): State<ControlPlane>,
    Path(organization): Path<String>,
) -> Result<Json<Vec<ListedProfile>>, Refused> {
    let listed = profile::profiles(&control_plane.store, &organization).await?;

    Ok(Json(
        listed
            .into_iter()
            .map(|(profile, held)| ListedProfile {
                profile: profile.into(),
                holds: held.into_iter().map(Into::into).collect(),
            })
            .collect(),
    ))
}

async fn declare_profile(
    State(control_plane): State<ControlPlane>,
    Path(organization): Path<String>,
    declaration: Result<Json<ProfileDeclaration>, JsonRejection>,
) -> Result<Response, Refused> {
    let Json(declaration) = declaration?;
    named("declare_subscription_profile", &declaration.name)?;

    let declared = profile::declare(
        &control_plane.store,
        &organization,
        &declaration.name,
        &declaration.owner,
    )
    .await?;

    Ok(answered::<_, ProfileRecord>(declared))
}

async fn hold_profile_variable(
    State(control_plane): State<ControlPlane>,
    Path((organization, profile, variable)): Path<(String, String, String)>,
    secret: Result<Json<Secret>, JsonRejection>,
) -> Result<Json<LoginRecord>, Refused> {
    let entry = Entry::variable(&variable)?;
    held_in_profile(&control_plane, &organization, &profile, &entry, secret).await
}

async fn hold_profile_file(
    State(control_plane): State<ControlPlane>,
    Path((organization, profile, path)): Path<(String, String, String)>,
    secret: Result<Json<Secret>, JsonRejection>,
) -> Result<Json<LoginRecord>, Refused> {
    let entry = Entry::file(&path)?;
    held_in_profile(&control_plane, &organization, &profile, &entry, secret).await
}

async fn held_in_profile(
    control_plane: &ControlPlane,
    organization: &str,
    profile: &str,
    entry: &Entry,
    secret: Result<Json<Secret>, JsonRejection>,
) -> Result<Json<LoginRecord>, Refused> {
    let Json(Secret { secret }) = secret?;
    let held = profile::hold(&control_plane.store, organization, profile, entry, &secret).await?;

    Ok(Json(held.into()))
}

async fn forget_profile_variable(
    State(control_plane): State<ControlPlane>,
    Path((organization, profile, variable)): Path<(String, String, String)>,
) -> Result<StatusCode, Refused> {
    let entry = Entry::variable(&variable)?;
    profile::forget(&control_plane.store, &organization, &profile, &entry).await?;

    Ok(StatusCode::NO_CONTENT)
}

async fn forget_profile_file(
    State(control_plane): State<ControlPlane>,
    Path((organization, profile, path)): Path<(String, String, String)>,
) -> Result<StatusCode, Refused> {
    let entry = Entry::file(&path)?;
    profile::forget(&control_plane.store, &organization, &profile, &entry).await?;

    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct GithubAppQuery {
    state: String,
    code: Option<String>,
}

async fn start_github_app(
    State(control_plane): State<ControlPlane>,
    Path(organization): Path<String>,
    registration: Result<Json<integration::manifest::Start>, JsonRejection>,
) -> Result<(StatusCode, Json<serde_json::Value>), Refused> {
    let Json(registration) = registration?;
    named("start_integration_manifest", &registration.name)?;
    Ok((
        StatusCode::CREATED,
        Json(
            integration::manifest::start(&control_plane.store, &organization, registration).await?,
        ),
    ))
}

fn setup_page(html: String) -> Response {
    const POLICY: &str = "default-src 'none'; form-action https://github.com; frame-ancestors 'none'; base-uri 'none'";
    let headers = [
        ("cache-control", "no-store"),
        ("referrer-policy", "no-referrer"),
        ("content-security-policy", POLICY),
    ];
    (headers, axum::response::Html(html)).into_response()
}

async fn github_app_page(
    State(cp): State<ControlPlane>,
    Query(query): Query<GithubAppQuery>,
) -> Result<Response, Refused> {
    Ok(setup_page(
        integration::manifest::page(&cp.store, &query.state).await?,
    ))
}

async fn github_app_callback(
    State(cp): State<ControlPlane>,
    Query(query): Query<GithubAppQuery>,
) -> Result<Response, Refused> {
    let code = query
        .code
        .ok_or_else(|| Refused::Unprocessable("GitHub did not supply a manifest code".into()))?;
    Ok(setup_page(
        integration::manifest::callback(&cp.store, &Github::dialling_out()?, &query.state, &code)
            .await?,
    ))
}

async fn github_app_installed(
    State(cp): State<ControlPlane>,
    Query(query): Query<GithubAppQuery>,
) -> Result<Response, Refused> {
    integration::manifest::installed(&cp.store, &Github::dialling_out()?, &query.state).await?;
    Ok(setup_page("<!doctype html><title>GitHub App ready</title><h1>The GitHub Integration is ready</h1><p>You can close this tab.</p>".into()))
}

#[derive(Deserialize)]
struct IntegrationRegistration {
    name: String,
    carries: Option<Vec<Direction>>,
    #[serde(flatten)]
    connection: ConnectionRegistration,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum ConnectionRegistration {
    Github {
        repository: String,
        app_id: i64,
        installation: i64,
        private_key: String,
        interval: Option<String>,
        webhook_secret: Option<String>,
        api: Option<String>,
    },
    Webhook {
        secret: String,
    },
}

/// Never the private key or a webhook secret: what an Integration presents stays behind the
/// boundary. The bot login is not a secret — it is the name GitHub already shows in public.
#[derive(Serialize)]
struct IntegrationRecord {
    id: String,
    name: String,
    kind: &'static str,
    repository: Option<String>,
    bot_login: Option<String>,
    carries: Vec<Direction>,
    polled_every: Option<String>,
    webhook_path: Option<String>,
    last_event_refusal: Option<EventRefusalRecord>,
}

#[derive(Serialize)]
struct EventRefusalRecord {
    source: String,
    id: Option<String>,
    bytes: Option<usize>,
    reason: String,
    observed_at: Timestamp,
}

impl From<Integration> for IntegrationRecord {
    fn from(integration: Integration) -> Self {
        let webhook_path = integration.webhook_path();
        let kind = integration.kind().as_str();
        let (repository, bot_login, polled_every, webhook_path) = match integration.connection {
            Connection::Github(github) if !integration.carries.contains(&Direction::Inbound) => {
                (Some(github.repository), Some(github.bot_login), None, None)
            }
            Connection::Github(github) => (
                Some(github.repository),
                Some(github.bot_login),
                Some(format!("{:#}", github.interval)),
                Some(webhook_path),
            ),
            Connection::Webhook => (None, None, None, Some(webhook_path)),
        };

        Self {
            id: integration.id.to_string(),
            name: integration.name,
            kind,
            repository,
            bot_login,
            carries: integration.carries,
            polled_every,
            webhook_path,
            last_event_refusal: integration.last_event_refusal.map(Into::into),
        }
    }
}

impl From<EventRefusal> for EventRefusalRecord {
    fn from(refusal: EventRefusal) -> Self {
        Self {
            source: refusal.source,
            id: refusal.id,
            bytes: refusal.bytes,
            reason: refusal.reason,
            observed_at: refusal.observed_at,
        }
    }
}

async fn integrations(
    State(control_plane): State<ControlPlane>,
    Path(organization): Path<String>,
) -> Result<Json<Vec<IntegrationRecord>>, Refused> {
    let integrations = integration::integrations(&control_plane.store, &organization).await?;

    Ok(Json(integrations.into_iter().map(Into::into).collect()))
}

async fn register_integration(
    State(control_plane): State<ControlPlane>,
    Path(organization): Path<String>,
    registration: Result<Json<IntegrationRegistration>, JsonRejection>,
) -> Result<(StatusCode, Json<IntegrationRecord>), Refused> {
    let Json(registration) = registration?;
    named("register_integration", &registration.name)?;

    let (connecting, carries) = match &registration.connection {
        ConnectionRegistration::Github {
            repository,
            app_id,
            installation,
            private_key,
            interval,
            webhook_secret,
            api,
        } => (
            Connecting::Github {
                repository,
                api: api.as_deref().unwrap_or(github::API),
                app_id: *app_id,
                installation: *installation,
                private_key,
                interval: interval
                    .as_deref()
                    .map_or(Ok(SignedDuration::from_mins(1)), str::parse)
                    .map_err(|error| {
                        Refused::Unprocessable(format!("an interval is a duration: {error}"))
                    })?,
                signing_secret: webhook_secret.as_deref(),
            },
            &[Direction::Inbound, Direction::Outbound][..],
        ),
        ConnectionRegistration::Webhook { secret } => {
            (Connecting::Webhook { secret }, &[Direction::Inbound][..])
        }
    };
    let github = Github::dialling_out()?;
    let registered = integration::register(
        &control_plane.store,
        &github,
        Registration {
            organization: &organization,
            name: &registration.name,
            carries: registration.carries.as_deref().unwrap_or(carries),
            connecting,
        },
    )
    .await?;

    Ok((StatusCode::CREATED, Json(registered.into())))
}

async fn acknowledge_event_refusal(
    State(control_plane): State<ControlPlane>,
    Path((organization, integration)): Path<(String, String)>,
) -> Result<StatusCode, Refused> {
    integration::acknowledge_event_refusal(&control_plane.store, &organization, &integration)
        .await?;

    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct Limited {
    limit: Option<usize>,
}

#[derive(Serialize)]
struct EventRecord {
    record: String,
    organization: String,
    integration: Option<String>,
    recorded_at: Timestamp,
    event: Occurrence,
    firings: Vec<Firing>,
}

impl EventRecord {
    async fn read(store: &Store, event: domain::Event) -> Result<Self, Refused> {
        Ok(Self {
            record: event.record_id.to_string(),
            organization: event.organization.to_string(),
            integration: event.integration.map(|integration| integration.to_string()),
            recorded_at: event.recorded_at,
            firings: trigger::firings(store, event.record_id).await?,
            event: event.occurrence,
        })
    }
}

async fn events(
    State(control_plane): State<ControlPlane>,
    Path(organization): Path<String>,
    Query(limited): Query<Limited>,
) -> Result<Json<Vec<EventRecord>>, Refused> {
    let events = integration::events(
        &control_plane.store,
        &organization,
        limited.limit.unwrap_or(EVENTS_LISTED),
    )
    .await?;
    let mut records = Vec::new();
    for event in events {
        records.push(EventRecord::read(&control_plane.store, event).await?);
    }

    Ok(Json(records))
}

async fn event(
    State(control_plane): State<ControlPlane>,
    Path(record): Path<String>,
) -> Result<Json<EventRecord>, Refused> {
    let record: EventRecordId = record.parse().map_err(|_| no_event(&record, None))?;
    let event = integration::event(&control_plane.store, record).await?;

    Ok(Json(EventRecord::read(&control_plane.store, event).await?))
}

async fn triggers(
    State(control_plane): State<ControlPlane>,
    Path(organization): Path<String>,
) -> Result<Json<Vec<TriggerRecord>>, Refused> {
    let triggers = trigger::triggers(&control_plane.store, &organization)
        .await
        .map_err(named_refusal)?;

    Ok(Json(triggers.into_iter().map(Into::into).collect()))
}

async fn declare_trigger(
    State(control_plane): State<ControlPlane>,
    Path(organization): Path<String>,
    declaration: Result<Json<TriggerDeclaration>, JsonRejection>,
) -> Result<(StatusCode, Json<TriggerRecord>), Refused> {
    let Json(declaration) = declaration?;
    named("declare_trigger", &declaration.name)?;
    let (fires, templates) = parse_trigger_declaration(&declaration)?;
    let created = !trigger::triggers(&control_plane.store, &organization)
        .await
        .map_err(named_refusal)?
        .iter()
        .any(|trigger| trigger.name == declaration.name);
    let trigger = trigger::declare(
        &control_plane.store,
        trigger::Declaration {
            organization: &organization,
            name: &declaration.name,
            fires: &fires,
            templates: &templates,
            project: &declaration.project,
            agent: &declaration.agent,
            declared: &declaration.declared(),
            allows: &declaration.allows,
            profile: declaration.profile.as_deref(),
        },
    )
    .await
    .map_err(named_refusal)?;

    Ok((
        if created {
            StatusCode::CREATED
        } else {
            StatusCode::OK
        },
        Json(trigger.into()),
    ))
}

async fn show_trigger(
    State(control_plane): State<ControlPlane>,
    Path((organization, name)): Path<(String, String)>,
) -> Result<Json<TriggerRecord>, Refused> {
    let trigger = trigger::show(&control_plane.store, &organization, &name)
        .await
        .map_err(named_refusal)?;

    Ok(Json(trigger.into()))
}

async fn test_trigger(
    State(control_plane): State<ControlPlane>,
    Path((organization, name)): Path<(String, String)>,
    tested: Result<Json<TriggerTest>, JsonRejection>,
) -> Result<Json<TriggerTestRecord>, Refused> {
    let Json(tested) = tested?;
    let event = tested
        .event
        .as_deref()
        .map(|event| {
            event
                .parse()
                .map_err(|_| no_event(event, Some(&organization)))
        })
        .transpose()?;
    let github;
    let against = match (event, tested.integration.as_deref(), tested.issue) {
        (Some(_), _, Some(_)) => {
            return Err(malformed(
                Some("issue"),
                "a test renders against an event or an issue, not both",
            ));
        }
        (Some(event), _, None) => trigger::Against::Event(event),
        (None, Some(integration), Some(issue)) => {
            github = Github::dialling_out()?;
            trigger::Against::Issue {
                github: &github,
                integration,
                issue,
            }
        }
        (None, None, Some(_)) => {
            return Err(malformed(
                Some("integration"),
                "an issue is read through an integration, so a test naming one names both",
            ));
        }
        (None, Some(_), None) => {
            return Err(malformed(
                Some("issue"),
                "an integration is read for an issue, so a test naming one names both",
            ));
        }
        (None, None, None) => trigger::Against::NextElapsing,
    };
    let asked = trigger::Asked {
        instruction: tested.instruction.as_deref(),
        agent: tested.agent.as_deref(),
    };
    let tested = match tested.declared {
        Some(file) => {
            let declarations = apply::declarations(file)
                .map_err(|error| Refused::Unprocessable(format!("{error:#}")))?;
            let declared = declarations
                .iter()
                .find(|declared| declared.name == name)
                .ok_or_else(|| {
                    Refused::Unprocessable(format!(
                        "the declaration file declares no trigger {name}"
                    ))
                })?;
            trigger::test_declared(
                &control_plane.store,
                &organization,
                declared,
                against,
                asked,
            )
            .await
        }
        None => trigger::test(&control_plane.store, &organization, &name, against, asked).await,
    }
    .map_err(named_refusal)?;
    let rendered = tested
        .rendered
        .map_err(|error| Refused::Unprocessable(error.to_string()))?;
    let agent = tested
        .agent
        .map_err(|error| Refused::Unprocessable(error.to_string()))?;
    let would = tested
        .would
        .ok_or_else(|| anyhow::anyhow!("a trigger that rendered says what its firing would do"))?;

    Ok(Json(TriggerTestRecord {
        matches: tested.matches,
        brief: rendered.brief,
        branch: rendered.branch,
        correlation: rendered.correlation,
        agent,
        would: would.as_str(),
        elapsing: tested.elapsing,
    }))
}

async fn disable_trigger(
    State(control_plane): State<ControlPlane>,
    Path((organization, name)): Path<(String, String)>,
) -> Result<Json<TriggerRecord>, Refused> {
    let trigger = trigger::disable(&control_plane.store, &organization, &name)
        .await
        .map_err(named_refusal)?;

    Ok(Json(trigger.into()))
}

async fn enable_trigger(
    State(control_plane): State<ControlPlane>,
    Path((organization, name)): Path<(String, String)>,
) -> Result<Json<TriggerRecord>, Refused> {
    let trigger = trigger::enable(&control_plane.store, &organization, &name)
        .await
        .map_err(named_refusal)?;

    Ok(Json(trigger.into()))
}

#[derive(Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
enum FiredRecord {
    Opened {
        event: String,
        workspace: String,
        session: String,
    },
    Fed {
        event: String,
        workspace: String,
        session: Option<String>,
    },
    Ignored {
        event: String,
        correlation: String,
    },
}

async fn dispatch_trigger(
    State(control_plane): State<ControlPlane>,
    Path((organization, name)): Path<(String, String)>,
    dispatch: Result<Json<TriggerDispatch>, JsonRejection>,
) -> Result<Json<FiredRecord>, Refused> {
    let Json(dispatch) = dispatch?;
    let fired = trigger::dispatch(
        &control_plane.store,
        &Github::dialling_out()?,
        trigger::Dispatch {
            organization: &organization,
            trigger: &name,
            integration: &dispatch.integration,
            issue: dispatch.issue,
            asked: trigger::Asked {
                instruction: dispatch.instruction.as_deref(),
                agent: dispatch.agent.as_deref(),
            },
        },
    )
    .await
    .map_err(named_refusal)?;

    Ok(Json(match fired {
        trigger::Fired::Opened {
            event,
            workspace,
            session,
        } => FiredRecord::Opened {
            event: event.to_string(),
            workspace: workspace.to_string(),
            session: session.to_string(),
        },
        trigger::Fired::Fed {
            event,
            workspace,
            session,
        } => FiredRecord::Fed {
            event: event.to_string(),
            workspace: workspace.to_string(),
            session: session.map(|session| session.to_string()),
        },
        trigger::Fired::Ignored {
            event, correlation, ..
        } => FiredRecord::Ignored {
            event: event.to_string(),
            correlation,
        },
        trigger::Fired::Failed { because, .. }
        | trigger::Fired::Held { because, .. }
        | trigger::Fired::Canceled { because, .. } => {
            return Err(Refused::Unprocessable(because));
        }
    }))
}

async fn apply_triggers(
    State(control_plane): State<ControlPlane>,
    Path(organization): Path<String>,
    file: Result<Json<apply::File>, JsonRejection>,
) -> Result<Json<apply::Applied>, Refused> {
    applied_triggers(control_plane, organization, file, false).await
}

async fn preview_applied_triggers(
    State(control_plane): State<ControlPlane>,
    Path(organization): Path<String>,
    file: Result<Json<apply::File>, JsonRejection>,
) -> Result<Json<apply::Applied>, Refused> {
    applied_triggers(control_plane, organization, file, true).await
}

async fn applied_triggers(
    control_plane: ControlPlane,
    organization: String,
    file: Result<Json<apply::File>, JsonRejection>,
    dry_run: bool,
) -> Result<Json<apply::Applied>, Refused> {
    let Json(file) = file?;
    let declarations =
        apply::declarations(file).map_err(|error| Refused::Unprocessable(format!("{error:#}")))?;
    let applied = apply::apply(&control_plane.store, &organization, &declarations, dry_run)
        .await
        .map_err(named_refusal)?;

    Ok(Json(applied))
}

fn parse_trigger_declaration(
    declaration: &TriggerDeclaration,
) -> Result<(Fires, Templates), Refused> {
    let fires = match (
        &declaration.filter,
        &declaration.every,
        &declaration.cron,
        &declaration.zone,
    ) {
        (Some(filter), None, None, None) => Fires::On(
            Filter::from_json(filter)
                .map_err(|error| Refused::Unprocessable(format!("a trigger filter: {error}")))?,
        ),
        (None, Some(every), None, None) => {
            Fires::Scheduled(Schedule::Every(every.parse().map_err(|error| {
                Refused::Unprocessable(format!("a trigger schedule is a duration: {error}"))
            })?))
        }
        (None, None, Some(cron), Some(zone)) => {
            Fires::Scheduled(Schedule::Cron(Cron::new(cron, zone).map_err(|error| {
                Refused::Unprocessable(format!("a trigger schedule: {error:#}"))
            })?))
        }
        (None, None, Some(_), None) => {
            return Err(Refused::Unprocessable(
                "a cron expression declares the time zone it is read in".to_owned(),
            ));
        }
        (_, _, None, Some(_)) => {
            return Err(Refused::Unprocessable(
                "a time zone is declared only with a cron expression".to_owned(),
            ));
        }
        _ => {
            return Err(Refused::Unprocessable(
                "a trigger declares a filter, an interval or a cron expression, and only one"
                    .to_owned(),
            ));
        }
    };
    let correlation = Correlation::parse(
        declaration.correlation.as_deref(),
        declaration.on_miss.as_deref(),
        declaration.on_open_workspace.as_deref(),
    )
    .map_err(|error| Refused::Unprocessable(format!("{error:#}")))?;
    let templates = Templates {
        brief: declaration
            .brief
            .parse::<Template>()
            .map_err(|error| Refused::Unprocessable(format!("a trigger brief: {error}")))?,
        branch: declaration
            .branch
            .as_deref()
            .map(str::parse)
            .transpose()
            .map_err(|error| Refused::Unprocessable(format!("a trigger branch: {error}")))?,
        correlation,
    };

    Ok((fires, templates))
}

/// An untyped refusal here is about what the request named, never a control plane that could
/// not answer.
fn named_refusal(error: anyhow::Error) -> Refused {
    if error.is::<Reason>() || error.is::<Concerning>() {
        return error.into();
    }
    Refused::Unprocessable(error.to_string())
}

#[derive(Serialize)]
struct HeldRecord {
    workspace: String,
    instance: String,
    because: String,
}

async fn instances(
    State(control_plane): State<ControlPlane>,
    Path(organization): Path<String>,
) -> Result<Json<Vec<HeldRecord>>, Refused> {
    let held = instance::held(&control_plane.store, &organization).await?;

    Ok(Json(
        held.into_iter()
            .map(|held| HeldRecord {
                workspace: held.workspace.to_string(),
                instance: held.instance,
                because: held.because,
            })
            .collect(),
    ))
}

#[derive(Serialize)]
struct ReleasedRecord {
    instance: String,
}

#[derive(Serialize)]
struct WorkRoleRecord {
    active_work_slots: usize,
    serialized_harnesses: Vec<String>,
    driver: String,
}

#[derive(Serialize)]
struct ActiveWorkRecord {
    limit: Option<usize>,
    occupied: usize,
    occupants: Vec<OccupantRecord>,
    elsewhere: usize,
}

#[derive(Serialize)]
struct OccupantRecord {
    name: String,
    workspace: String,
    agent: String,
    phase: OccupantPhase,
    enqueued_at: Timestamp,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum OccupantPhase {
    Working,
    Trailing,
}

#[derive(Serialize)]
struct InstancesRecord {
    limit: Option<usize>,
    count: usize,
    counted: Vec<String>,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum ReasonRecord {
    Dependencies {
        sessions: Vec<String>,
    },
    SubscriptionProfile {
        profile: String,
        session: Option<String>,
    },
    InstanceArchiving {
        instance: String,
    },
    LiveInstanceLimit {
        limit: usize,
    },
    ActiveWorkSlots {
        limit: usize,
    },
    Ahead {
        sessions: Vec<String>,
        #[serde(skip_serializing_if = "is_zero")]
        elsewhere: usize,
    },
}

fn is_zero(value: &usize) -> bool {
    *value == 0
}

impl From<scheduling::Reason> for ReasonRecord {
    fn from(reason: scheduling::Reason) -> Self {
        match reason {
            scheduling::Reason::Dependencies(sessions) => Self::Dependencies { sessions },
            scheduling::Reason::SubscriptionProfile { profile, session } => {
                Self::SubscriptionProfile { profile, session }
            }
            scheduling::Reason::InstanceArchiving(instance) => Self::InstanceArchiving { instance },
            scheduling::Reason::LiveInstanceLimit(limit) => Self::LiveInstanceLimit { limit },
            scheduling::Reason::ActiveWorkSlots(limit) => Self::ActiveWorkSlots { limit },
            scheduling::Reason::Ahead {
                sessions,
                elsewhere,
            } => Self::Ahead {
                sessions,
                elsewhere,
            },
        }
    }
}

fn reasons(reasons: Vec<scheduling::Reason>) -> Vec<ReasonRecord> {
    reasons.into_iter().map(ReasonRecord::from).collect()
}

#[derive(Serialize)]
struct QueuedSessionRecord {
    position: Option<usize>,
    name: String,
    workspace: String,
    agent: String,
    reasons: Vec<ReasonRecord>,
    enqueued_at: Timestamp,
}

#[derive(Serialize)]
struct WaitingSessionRecord {
    position: Option<usize>,
    name: String,
    workspace: String,
    agent: String,
    pending_since: Option<Timestamp>,
    reasons: Vec<ReasonRecord>,
    enqueued_at: Timestamp,
}

#[derive(Serialize)]
struct UnbriefedSessionRecord {
    position: Option<usize>,
    brief_since: Option<Timestamp>,
    reasons: Vec<ReasonRecord>,
    name: String,
    workspace: String,
    agent: String,
    preparing: Option<String>,
    pending_since: Option<Timestamp>,
    enqueued_at: Timestamp,
}

#[derive(Serialize)]
struct QueueRecord {
    work_role: Option<WorkRoleRecord>,
    active_work: ActiveWorkRecord,
    instances: InstancesRecord,
    queued: Vec<QueuedSessionRecord>,
    waiting: Vec<WaitingSessionRecord>,
    unbriefed: Vec<UnbriefedSessionRecord>,
}

impl QueueRecord {
    fn of(snapshot: scheduling::Snapshot) -> Self {
        let recorded = snapshot.recorded.map(|recorded| WorkRoleRecord {
            active_work_slots: recorded.active_work_slots,
            serialized_harnesses: recorded.serialized_harnesses,
            driver: recorded.driver,
        });

        Self {
            work_role: recorded,
            active_work: ActiveWorkRecord {
                limit: snapshot.active_work.limit,
                occupied: snapshot.active_work.occupied,
                occupants: snapshot
                    .active_work
                    .occupants
                    .into_iter()
                    .map(|session| OccupantRecord {
                        name: session.name,
                        workspace: session.workspace.to_string(),
                        agent: session.agent.name,
                        phase: match session.state {
                            domain::SessionState::Trailing => OccupantPhase::Trailing,
                            _ => OccupantPhase::Working,
                        },
                        enqueued_at: session.enqueued_at,
                    })
                    .collect(),
                elsewhere: snapshot.active_work.elsewhere,
            },
            instances: InstancesRecord {
                limit: snapshot.instances.limit,
                count: snapshot.instances.count,
                counted: snapshot.instances.counted,
            },
            queued: snapshot
                .queued
                .into_iter()
                .map(|queued| QueuedSessionRecord {
                    position: queued.position,
                    name: queued.session.name,
                    workspace: queued.session.workspace.to_string(),
                    agent: queued.session.agent.name,
                    reasons: reasons(queued.reasons),
                    enqueued_at: queued.session.enqueued_at,
                })
                .collect(),
            waiting: snapshot
                .waiting
                .into_iter()
                .map(|waiting| WaitingSessionRecord {
                    position: waiting.position,
                    name: waiting.session.name,
                    workspace: waiting.session.workspace.to_string(),
                    agent: waiting.session.agent.name,
                    pending_since: waiting.pending_since,
                    reasons: reasons(waiting.reasons),
                    enqueued_at: waiting.session.enqueued_at,
                })
                .collect(),
            unbriefed: snapshot
                .unbriefed
                .into_iter()
                .map(|unbriefed| UnbriefedSessionRecord {
                    position: unbriefed.position,
                    brief_since: unbriefed.brief_since,
                    reasons: reasons(unbriefed.reasons),
                    name: unbriefed.session.name,
                    workspace: unbriefed.session.workspace.to_string(),
                    agent: unbriefed.session.agent.name,
                    preparing: unbriefed
                        .session
                        .preparing
                        .map(|preparing| preparing.as_str().to_owned()),
                    pending_since: unbriefed.pending_since,
                    enqueued_at: unbriefed.session.enqueued_at,
                })
                .collect(),
        }
    }
}

async fn show_queue(
    State(control_plane): State<ControlPlane>,
    Path(organization): Path<String>,
) -> Result<Json<QueueRecord>, Refused> {
    let snapshot = scheduling::snapshot(&control_plane.store, &organization).await?;

    Ok(Json(QueueRecord::of(snapshot)))
}

async fn release_instance(
    State(control_plane): State<ControlPlane>,
    Path((organization, workspace)): Path<(String, String)>,
    release: Result<Json<Release>, JsonRejection>,
) -> Result<Json<ReleasedRecord>, Refused> {
    let Json(release) = release?;
    let workspace = resolved(&control_plane, &organization, &workspace).await?;
    let instance =
        instance::release(&control_plane.store, workspace.id, &release.participant).await?;

    Ok(Json(ReleasedRecord { instance }))
}

async fn workspaces(
    State(control_plane): State<ControlPlane>,
    Path(organization): Path<String>,
) -> Result<Json<Vec<WorkspaceListedRecord>>, Refused> {
    let workspaces = workspace::workspaces(&control_plane.store, &organization).await?;
    let snapshot = scheduling::snapshot(&control_plane.store, &organization).await?;
    let mut records = Vec::with_capacity(workspaces.len());
    for workspace in workspaces {
        let id = workspace.id;
        let session = work::sessions(&control_plane.store, id)
            .await?
            .pop()
            .map(|session| SessionRecord::read(session, &control_plane.live.summaries));
        records.push(WorkspaceListedRecord {
            workspace: WorkspaceRecord::read(&control_plane.store, workspace).await?,
            session,
            queue: QueueStandingRecord::of(&snapshot, id),
        });
    }

    Ok(Json(records))
}

async fn open_workspace(
    State(control_plane): State<ControlPlane>,
    Path(organization): Path<String>,
    declaration: Result<Json<WorkspaceDeclaration>, JsonRejection>,
) -> Result<(StatusCode, Json<OpenedRecord>), Refused> {
    let Json(declaration) = declaration?;
    let (workspace, session) = workspace::open(
        &control_plane.store,
        &organization,
        workspace::Open {
            project: &declaration.project,
            agent: &declaration.agent,
            profile: declaration.profile.as_deref(),
            branch: declaration.branch.as_deref(),
            continues: declaration.continues.as_deref(),
            declared: declaration.declared(),
            brief: declaration.brief.as_deref(),
            participant: declaration.participant.as_deref(),
            depends_on: declaration.depends_on.as_deref().unwrap_or_default(),
        },
    )
    .await?;

    Ok((
        StatusCode::CREATED,
        Json(OpenedRecord {
            workspace: WorkspaceRecord::read(&control_plane.store, workspace).await?,
            session: SessionRecord::read(session, &control_plane.live.summaries),
        }),
    ))
}

async fn work_summary(
    State(control_plane): State<ControlPlane>,
    Path((organization, reference)): Path<(String, String)>,
) -> Result<Json<crate::live_work::Work>, Refused> {
    Ok(Json(
        crate::live_work::read(
            &control_plane.store,
            &control_plane.live.summaries,
            &organization,
            &reference,
        )
        .await?,
    ))
}

#[derive(Deserialize)]
struct ChangesQuery {
    scope: Option<String>,
}

async fn workspace_changes(
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

async fn workspace_commits(
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

async fn workspace_stashes(
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

async fn workspace_files(
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

async fn workspace_file(
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

async fn show_workspace(
    State(control_plane): State<ControlPlane>,
    Path((organization, workspace)): Path<(String, String)>,
) -> Result<Json<WorkspaceRecord>, Refused> {
    let workspace = resolved(&control_plane, &organization, &workspace).await?;

    Ok(Json(
        WorkspaceRecord::read(&control_plane.store, workspace).await?,
    ))
}

async fn post_to_workspace(
    State(control_plane): State<ControlPlane>,
    Path((organization, workspace)): Path<(String, String)>,
    message: Result<Json<WorkspaceMessage>, JsonRejection>,
) -> Result<Json<PostedRecord>, Refused> {
    let Json(message) = message?;
    let workspace = resolved(&control_plane, &organization, &workspace).await?;
    let posted = workspace::post(
        &control_plane.store,
        workspace.id,
        message.participant.as_deref().unwrap_or_default(),
        &message.message,
    )
    .await?;

    Ok(Json(PostedRecord {
        session: posted
            .session
            .map(|session| SessionRecord::read(session, &control_plane.live.summaries)),
        held_message: posted.held_message,
    }))
}

async fn edit_workspace_message(
    State(control_plane): State<ControlPlane>,
    Path((organization, workspace, id)): Path<(String, String, String)>,
    edit: Result<Json<WorkspaceMessage>, JsonRejection>,
) -> Result<Json<HeldMessage>, Refused> {
    let Json(edit) = edit?;
    let workspace = resolved(&control_plane, &organization, &workspace).await?;
    let id = held_id(&organization, &workspace, &id)?;
    let edited = workspace::edit_message(
        &control_plane.store,
        workspace.id,
        id,
        edit.participant.as_deref().unwrap_or_default(),
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

    Ok(Json(edited))
}

async fn withdraw_workspace_message(
    State(control_plane): State<ControlPlane>,
    Path((organization, workspace, id)): Path<(String, String, String)>,
    withdrawal: Result<Json<WorkspaceMessageWithdrawal>, JsonRejection>,
) -> Result<StatusCode, Refused> {
    let Json(withdrawal) = withdrawal?;
    let workspace = resolved(&control_plane, &organization, &workspace).await?;
    let id = held_id(&organization, &workspace, &id)?;
    workspace::withdraw_message(
        &control_plane.store,
        workspace.id,
        id,
        withdrawal.participant.as_deref().unwrap_or_default(),
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
fn held_id(organization: &str, workspace: &Workspace, id: &str) -> Result<i64, Refused> {
    id.parse().map_err(|_| {
        never_held(
            organization,
            workspace,
            id,
            format!("the workspace never held a message with the id {id}"),
        )
    })
}

fn never_held(organization: &str, workspace: &Workspace, id: &str, message: String) -> Refused {
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
fn held_refusal(
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

fn no_event(record: &str, organization: Option<&str>) -> Refused {
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

async fn seal_workspace(
    State(control_plane): State<ControlPlane>,
    Path((organization, workspace)): Path<(String, String)>,
) -> Result<Json<WorkspaceRecord>, Refused> {
    let workspace = resolved(&control_plane, &organization, &workspace).await?;
    let workspace = workspace::seal(&control_plane.store, workspace.id).await?;

    Ok(Json(
        WorkspaceRecord::read(&control_plane.store, workspace).await?,
    ))
}

async fn sessions(
    State(control_plane): State<ControlPlane>,
    Path((organization, workspace)): Path<(String, String)>,
) -> Result<Json<Vec<SessionRecord>>, Refused> {
    let workspace = resolved(&control_plane, &organization, &workspace).await?;
    let sessions = work::sessions(&control_plane.store, workspace.id).await?;

    Ok(Json(
        sessions
            .into_iter()
            .map(|session| SessionRecord::read(session, &control_plane.live.summaries))
            .collect(),
    ))
}

async fn enqueue_session(
    State(control_plane): State<ControlPlane>,
    Path((organization, workspace)): Path<(String, String)>,
    declaration: Result<Json<SessionDeclaration>, JsonRejection>,
) -> Result<(StatusCode, Json<SessionRecord>), Refused> {
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
    let session = work::enqueue(
        &control_plane.store,
        workspace.id,
        declaration.agent.as_deref(),
        declaration.declared(),
        declaration.depends_on.as_deref().unwrap_or_default(),
    )
    .await?;

    Ok((
        StatusCode::CREATED,
        Json(SessionRecord::read(session, &control_plane.live.summaries)),
    ))
}

async fn show_session(
    State(control_plane): State<ControlPlane>,
    Path((organization, session)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<Response, Refused> {
    let session = work::resolve_session(&control_plane.store, &organization, &session).await?;
    let record = SessionRecord::read(session, &control_plane.live.summaries);
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

fn strong_etag(body: &str) -> String {
    let digest = sha2::Sha256::digest(body.as_bytes());
    let hex: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();

    format!("\"{hex}\"")
}

/// RFC 9110's If-None-Match comparison: weak tags match weak, and `*` matches any.
fn matches_etag(asked: &str, tag: &str) -> bool {
    asked.split(',').any(|candidate| {
        let candidate = candidate.trim();
        candidate == "*" || candidate.strip_prefix("W/").unwrap_or(candidate) == tag
    })
}

async fn interrupt_session(
    State(control_plane): State<ControlPlane>,
    Path((organization, session)): Path<(String, String)>,
    interrupt: Result<Json<SessionInterrupt>, JsonRejection>,
) -> Result<(StatusCode, Json<SessionRecord>), Refused> {
    let Json(interrupt) = interrupt?;
    let session = work::resolve_session(&control_plane.store, &organization, &session).await?;
    let interrupting = work::interrupt(
        &control_plane.store,
        session.id,
        interrupt.participant.as_deref().unwrap_or_default(),
    )
    .await?;

    Ok((
        StatusCode::ACCEPTED,
        Json(SessionRecord::read(
            interrupting,
            &control_plane.live.summaries,
        )),
    ))
}

async fn stop_session(
    State(control_plane): State<ControlPlane>,
    Path((organization, session)): Path<(String, String)>,
) -> Result<Json<SessionRecord>, Refused> {
    let session = work::resolve_session(&control_plane.store, &organization, &session).await?;
    let running = control_plane.live.summaries.last_held(&session).tools;
    work::stop(&control_plane.store, session.id, &running).await?;
    control_plane.live.summaries.clear_session(&session);
    let session = work::session(&control_plane.store, session.id).await?;

    Ok(Json(SessionRecord::read(
        session,
        &control_plane.live.summaries,
    )))
}

async fn set_session_option(
    State(control_plane): State<ControlPlane>,
    Path((organization, session)): Path<(String, String)>,
    change: Result<Json<OptionChange>, JsonRejection>,
) -> Result<(StatusCode, Json<SessionRecord>), Refused> {
    let Json(change) = change?;
    let named = match (change.option.as_deref(), change.category.as_deref()) {
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
        &change.participant,
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
        Json(SessionRecord::read(
            written.session,
            &control_plane.live.summaries,
        )),
    ))
}

async fn resolved(
    control_plane: &ControlPlane,
    organization: &str,
    workspace: &str,
) -> Result<Workspace, Refused> {
    Ok(workspace::resolve(&control_plane.store, organization, workspace).await?)
}

fn sharing_a_directory(repositories: &[String]) -> Option<String> {
    let mut claimed = std::collections::HashMap::new();
    repositories.iter().find_map(|repository| {
        let directory = cloned_into(repository);
        claimed.insert(directory, repository).map(|earlier| {
            format!("{earlier} and {repository} would both be checked out into {directory}")
        })
    })
}

// Must name the directory kestrel-supervisor's checkout clones into.
fn cloned_into(repository: &str) -> &str {
    let name = repository
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or(repository);

    name.strip_suffix(".git").unwrap_or(name)
}

fn named(operation: &'static str, name: &str) -> Result<(), Refused> {
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

fn answered<T, R>(declared: DeclaredRecord<T>) -> Response
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

async fn transcript_payload(
    State(control_plane): State<ControlPlane>,
    Path((organization, workspace, payload)): Path<(String, String, String)>,
) -> Result<Response, Refused> {
    let workspace = resolved(&control_plane, &organization, &workspace).await?;
    let mut tx = control_plane.store.read().await?;
    let payload = match tx.log().payload(&workspace, &payload).await? {
        log::PayloadRead::Available(payload) => payload,
        log::PayloadRead::Gone => {
            return Err(diagnosed(
                Reason::Expired {
                    operation: "transcript_payload",
                    resource: Resource::TranscriptPayload,
                    reference: payload,
                    organization: Some(organization),
                    next: declined::Next::inspect(Resource::Workspace, workspace.id.to_string()),
                    message: "the Transcript payload has expired".to_owned(),
                },
                None,
            ));
        }
        log::PayloadRead::Missing => {
            return Err(diagnosed(
                Reason::MissingReference {
                    resource: Resource::TranscriptPayload,
                    reference: payload,
                    organization: Some(organization),
                    within: Some(Locator::new(Resource::Workspace, workspace.id.to_string())),
                    message: "no payload in this Workspace".to_owned(),
                },
                None,
            ));
        }
    };
    Ok((
        [(axum::http::header::CONTENT_TYPE, payload.media_type)],
        payload.content,
    )
        .into_response())
}

/// A stream that closes without an `end` event was cut off, and the reader resumes it from
/// the last id it was handed. A read that stays open past caught-up registers its follower and
/// carries who is following, transiently (ADR-0035).
async fn transcript(
    State(control_plane): State<ControlPlane>,
    Path((organization, workspace)): Path<(String, String)>,
    Query(following): Query<Following>,
    headers: HeaderMap,
) -> Result<Sse<impl Stream<Item = Result<Event, BoxError>>>, Refused> {
    let workspace = resolved(&control_plane, &organization, &workspace).await?;
    let name = match following.as_name.as_deref() {
        Some(name) => {
            let mut tx = control_plane.store.read().await?;
            Some(participant::accepted(&mut tx, &workspace.organization, name, "transcript").await?)
        }
        None => None,
    };
    let range = log::SeqRange {
        first_seq: following.first_seq,
        last_seq: following.last_seq,
    };
    range.validate()?;
    let transcribing = Transcribing {
        workspace: workspace.id,
        name,
        follow: following.follow.unwrap_or(true),
        kinds: kinds(following.kinds.as_deref())?,
        summaries: following.summaries.unwrap_or(true),
        range,
        from: last_event_id(&headers)?,
    };
    let read = transcribing.read(&control_plane).await?;

    Ok(per_resource(transcribed(control_plane, transcribing, read)))
}

struct Transcribing {
    workspace: WorkspaceId,
    name: Option<String>,
    follow: bool,
    kinds: log::Kinds,
    summaries: bool,
    range: log::SeqRange,
    from: Option<Cursor>,
}

impl Transcribing {
    async fn read(&self, control_plane: &ControlPlane) -> Result<Read, Refused> {
        reading(
            control_plane,
            self.workspace,
            self.from,
            &self.kinds,
            self.summaries,
            self.range,
        )
        .await
    }
}

fn kinds(kinds: Option<&str>) -> Result<log::Kinds, Refused> {
    kinds
        .map(str::parse::<log::Kinds>)
        .transpose()
        .map_err(|error| malformed(Some("kinds"), error.to_string()))
        .map(Option::unwrap_or_default)
}

fn transcribed(
    control_plane: ControlPlane,
    transcribing: Transcribing,
    mut read: Read,
) -> stream::Events {
    let Transcribing {
        workspace,
        name,
        follow,
        kinds,
        summaries,
        range,
        from,
    } = transcribing;

    Box::pin(async_stream::try_stream! {
        let mut joined: Option<crate::presence::Joined> = None;
        let mut delivered = from;
        let mut session_state = read.session_state.clone();
        if follow {
            yield Emitted::new("session_state", None, &session_state)?;
        }
        loop {
            if follow && read.session_state != session_state {
                session_state = read.session_state.clone();
                yield Emitted::new("session_state", None, &session_state)?;
            }
            let mut activities = read.page.activities.into_iter().peekable();
            for entry in read.page.entries {
                while activities.peek().is_some_and(|activity| activity.last_seq < entry.seq) {
                    let activity = activities.next().unwrap();
                    delivered = Some(Cursor::at(workspace, activity.last_seq));
                    yield Emitted::new("activity", delivered, &activity)?;
                }
                delivered = Some(Cursor::at(workspace, entry.seq));
                yield Emitted::new("entry", delivered, &Recorded {
                    kind: entry.kind,
                    session_id: entry.session_id,
                    seq: entry.seq,
                    appended_at: entry.appended_at.to_string(),
                    entry: entry.entry,
                })?;
            }
            for activity in activities {
                delivered = Some(Cursor::at(workspace, activity.last_seq));
                yield Emitted::new("activity", delivered, &activity)?;
            }
            if let Some(cursor) = read.page.cursor && delivered != Some(cursor) {
                delivered = Some(cursor);
                yield Emitted::new("cursor", delivered, &cursor.to_string())?;
            }
            if !read.page.more {
                let because = match (read.sealed, follow) {
                    (true, _) => Some(Because::Sealed),
                    (false, false) => Some(Because::CaughtUp),
                    (false, true) if range.last_seq.is_some() => Some(Because::CaughtUp),
                    (false, true) => None,
                };
                if let Some(because) = because {
                    yield Emitted::new(stream::END, None, &End { because })?;
                    break;
                }

                if joined.is_none() {
                    let mut follower = control_plane.followers.join(workspace, name.clone());
                    yield Emitted::new("follower", None, &FollowerEvent {
                        id: follower.id,
                        lease_seconds: follower.lease.as_secs(),
                    })?;
                    yield Emitted::new("presence", None, &follower.snapshot())?;
                    joined = Some(follower);
                }

                let follower = joined.as_mut().expect("registered just above");
                if *follower.expiration.borrow() {
                    break;
                }
                if follower.presence.has_changed().unwrap_or(false) {
                    yield Emitted::new("presence", None, &follower.snapshot())?;
                }

                tokio::select! {
                    () = tokio::time::sleep(POLL) => {}
                    () = control_plane.shutdown.cancelled() => break,
                }
            }

            read = reading(&control_plane, workspace, read.page.cursor, &kinds, summaries, range)
                .await
                .map_err(Refused::into_error)?;
        }
    })
}

/// An unknown or expired follower is a 404, never a fresh registration.
async fn renew_follower(
    State(control_plane): State<ControlPlane>,
    Path((organization, workspace, id)): Path<(String, String, String)>,
) -> Result<StatusCode, Refused> {
    let workspace = resolved(&control_plane, &organization, &workspace).await?;
    let id = id
        .parse::<crate::presence::FollowerId>()
        .map_err(|_| Refused::NotFound("no such follower".to_owned()))?;

    if control_plane.followers.renew(workspace.id, id) {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(Refused::NotFound(
            "no such follower, or its lease has passed".to_owned(),
        ))
    }
}

async fn changes(
    State(control_plane): State<ControlPlane>,
    Path(organization): Path<String>,
) -> Result<Sse<impl Stream<Item = Result<Event, BoxError>>>, Refused> {
    let notices = noticed(&control_plane, &organization).await?;

    Ok(per_resource(notices))
}

/// Every subscriber is told to refetch everything on connect, and again if it fell behind the
/// hub's buffer: a notice is a hint, so a missed one must never look like a quiet stream.
async fn noticed(
    control_plane: &ControlPlane,
    organization: &str,
) -> Result<stream::Events, Refused> {
    let mut tx = control_plane.store.read().await?;
    let organization = tx.organizations().named(organization).await?;
    let mut subscription = control_plane.store.notices().subscribe(organization.id);
    drop(tx);
    let control_plane = control_plane.clone();

    Ok(Box::pin(async_stream::try_stream! {
        yield Emitted::new("open", None, &Refetch {})?;
        loop {
            let watch = tokio::select! {
                () = control_plane.shutdown.cancelled() => break,
                watch = subscription.recv() => watch,
            };
            match watch {
                Some(fanout::Watch::Change(resource)) => {
                    match Changed::named(&control_plane.store, resource).await {
                        Ok(changed) => yield Emitted::new("change", None, &changed)?,
                        Err(_) => yield Emitted::new("resync", None, &Refetch {})?,
                    }
                }
                Some(fanout::Watch::Resync) => {
                    yield Emitted::new("resync", None, &Refetch {})?;
                }
                None => break,
            }
        }
    }))
}

fn per_resource(mut events: stream::Events) -> Sse<impl Stream<Item = Result<Event, BoxError>>> {
    let framed = async_stream::try_stream! {
        while let Some(emitted) = events.next().await {
            let emitted = emitted?;
            let mut event = Event::default().event(emitted.event).json_data(&emitted.data)?;
            if let Some(cursor) = emitted.cursor {
                event = event.id(cursor.to_string());
            }
            yield event;
        }
    };

    Sse::new(framed).keep_alive(KeepAlive::new().interval(KEEP_ALIVE))
}

async fn reserve_stream(
    State(control_plane): State<ControlPlane>,
) -> Result<(StatusCode, Json<wire::StreamReservation>), Refused> {
    let token = control_plane.streams.reserve().map_err(stream_refused)?;

    Ok((
        StatusCode::CREATED,
        Json(wire::StreamReservation { token: token.0 }),
    ))
}

async fn open_stream(
    State(control_plane): State<ControlPlane>,
    Path(token): Path<String>,
) -> Result<Sse<impl Stream<Item = Result<Event, BoxError>>>, Refused> {
    let connected = control_plane
        .streams
        .connect(reservation(&token)?)
        .map_err(stream_refused)?;
    let shutdown = control_plane.shutdown.clone();

    let framed = async_stream::try_stream! {
        let mut subscriptions = StreamMap::new();
        let mut generations = HashMap::new();
        loop {
            let (id, emitted) = tokio::select! {
                () = shutdown.cancelled() => break,
                () = connected.changed() => {
                    let wanted = connected.wanted();
                    for subscribed in wanted.unstarted {
                        generations.insert(subscribed.id.clone(), subscribed.generation);
                        subscriptions.insert(subscribed.id, cut_off_unless_ended(subscribed.events));
                    }
                    generations.retain(|id, generation| {
                        let kept = wanted.held.get(id) == Some(generation);
                        if !kept {
                            subscriptions.remove(id);
                        }
                        kept
                    });
                    continue;
                }
                Some(next) = subscriptions.next(), if !subscriptions.is_empty() => next,
            };
            let emitted: Emitted = match emitted {
                Ok(emitted) => emitted,
                Err(error) => {
                    warn!(%error, subscription = id, "closing a reserved stream");
                    break;
                }
            };
            let ended = emitted.ends();
            yield Event::default().event(emitted.event).json_data(wire::StreamEvent {
                subscription: id.clone(),
                cursor: emitted.cursor.map(|cursor| cursor.to_string()),
                data: emitted.data,
            })?;
            if ended {
                subscriptions.remove(&id);
                if let Some(generation) = generations.remove(&id) {
                    connected.ended(&id, generation);
                }
            }
        }
    };

    Ok(Sse::new(framed).keep_alive(KeepAlive::new().interval(KEEP_ALIVE)))
}

/// A subscription cut off short of `end` closes its whole connection, so the tab resumes every
/// subscription from its cursors rather than missing one that went quiet.
fn cut_off_unless_ended(mut events: stream::Events) -> stream::Events {
    Box::pin(async_stream::try_stream! {
        while let Some(emitted) = events.next().await {
            let emitted = emitted?;
            let ended = emitted.ends();
            yield emitted;
            if ended {
                return;
            }
        }
        Err::<(), BoxError>("a subscription was cut off".into())?;
    })
}

async fn subscribe_stream(
    State(control_plane): State<ControlPlane>,
    Path((token, id)): Path<(String, String)>,
    subscription: Result<Json<wire::StreamSubscription>, JsonRejection>,
) -> Result<StatusCode, Refused> {
    let token = reservation(&token)?;
    stream::bounded_id(&id).map_err(stream_refused)?;
    if !control_plane.streams.known(token) {
        return Err(stream_refused(stream::Refusal::Unknown));
    }
    let Json(subscription) = subscription?;
    let events = match subscription.kind {
        wire::StreamSubscriptionKind::Notices => {
            noticed(&control_plane, &subscription.organization).await?
        }
        wire::StreamSubscriptionKind::Transcript => {
            let workspace = subscription.workspace.as_deref().ok_or_else(|| {
                malformed(
                    Some("workspace"),
                    "a transcript subscription names its Workspace",
                )
            })?;
            let workspace = resolved(&control_plane, &subscription.organization, workspace).await?;
            let transcribing = Transcribing {
                workspace: workspace.id,
                name: None,
                follow: true,
                kinds: kinds(subscription.kinds.as_deref())?,
                summaries: true,
                range: log::SeqRange::default(),
                from: subscription
                    .after
                    .as_deref()
                    .map(str::parse)
                    .transpose()
                    .map_err(|error: anyhow::Error| malformed(Some("after"), error.to_string()))?,
            };
            let read = transcribing.read(&control_plane).await?;
            transcribed(control_plane.clone(), transcribing, read)
        }
    };
    control_plane
        .streams
        .subscribe(token, id, events)
        .map_err(stream_refused)?;

    Ok(StatusCode::NO_CONTENT)
}

async fn unsubscribe_stream(
    State(control_plane): State<ControlPlane>,
    Path((token, id)): Path<(String, String)>,
) -> Result<StatusCode, Refused> {
    control_plane
        .streams
        .unsubscribe(reservation(&token)?, id)
        .map_err(stream_refused)?;

    Ok(StatusCode::NO_CONTENT)
}

fn reservation(token: &str) -> Result<stream::Token, Refused> {
    token
        .parse()
        .map_err(|_| stream_refused(stream::Refusal::Unknown))
}

fn stream_refused(refusal: stream::Refusal) -> Refused {
    match refusal {
        stream::Refusal::Unknown => Refused::NotFound(
            "no such stream reservation, or it has expired; reserve again".to_owned(),
        ),
        stream::Refusal::AlreadyConnected => {
            Refused::Conflict("the reserved stream is already open".to_owned())
        }
        stream::Refusal::TooManySubscriptions => Refused::Conflict(format!(
            "a reserved stream holds at most {} subscriptions",
            stream::MOST_SUBSCRIPTIONS
        )),
        stream::Refusal::IdOutOfBounds => {
            malformed(None, "a subscription id is between 1 and 64 characters")
        }
        stream::Refusal::TooManyReservations => Refused::Unavailable(anyhow::anyhow!(
            "{} streams are already reserved",
            stream::MOST_RESERVATIONS
        )),
    }
}

/// The state is read in the transaction the page is, so a Workspace sealed between the two
/// cannot end the stream short of its last entry.
async fn reading(
    control_plane: &ControlPlane,
    id: WorkspaceId,
    from: Option<Cursor>,
    kinds: &log::Kinds,
    summaries: bool,
    range: log::SeqRange,
) -> Result<Read, Refused> {
    let mut tx = control_plane.store.read().await?;
    let workspace = tx
        .workspaces()
        .find(id)
        .await?
        .ok_or_else(|| Refused::NotFound("no workspace".to_owned()))?;
    let page = tx
        .log()
        .transcript_page(&workspace, from, Window::DEFAULT, kinds, summaries, range)
        .await?;

    let session = tx
        .workspaces()
        .unfinished_session(&workspace)
        .await?
        .map(|holding| holding.session);
    let (observation, state) = control_plane.live.summaries.observed(session.as_ref());
    let session_state = TranscriptSessionState {
        session_id: session.as_ref().map(|session| session.id),
        state,
        observation,
    };
    Ok(Read {
        page,
        session_state,
        sealed: workspace.state == WorkspaceState::Sealed,
    })
}

fn last_event_id(headers: &HeaderMap) -> Result<Option<Cursor>, Refused> {
    let Some(cursor) = headers.get("last-event-id") else {
        return Ok(None);
    };

    cursor
        .to_str()
        .map_err(|error| malformed(None, error.to_string()))?
        .parse()
        .map(Some)
        .map_err(|error: anyhow::Error| malformed(None, error.to_string()))
}

enum Refused {
    /// A request the boundary could not read; its operation is the route's.
    Malformed {
        field: Option<&'static str>,
        why: String,
    },
    Forbidden(String),
    NotFound(String),
    Conflict(String),
    Unprocessable(String),
    Diagnosed {
        status: StatusCode,
        diagnostic: Box<wire::Diagnostic>,
    },
    /// The control plane could not answer; its operation is the route's.
    Unavailable(anyhow::Error),
}

fn malformed(field: Option<&'static str>, why: impl Into<String>) -> Refused {
    Refused::Malformed {
        field,
        why: why.into(),
    }
}

impl Refused {
    fn into_error(self) -> BoxError {
        match self {
            Refused::Malformed { why, .. }
            | Refused::Forbidden(why)
            | Refused::NotFound(why)
            | Refused::Conflict(why)
            | Refused::Unprocessable(why) => why.into(),
            Refused::Diagnosed { diagnostic, .. } => {
                diagnostic_message(&diagnostic).to_owned().into()
            }
            Refused::Unavailable(error) => error.into(),
        }
    }
}

/// Never offered for an ambiguous reference: declaring another record cannot resolve one.
fn declare_or_inspect(
    resource: Resource,
    reference: &str,
    organization: Option<&str>,
) -> Vec<wire::Action> {
    match (resource, organization) {
        (Resource::Organization, _) => vec![wire::Action::DeclareOrganizationAction(
            wire::DeclareOrganizationAction {
                action: serde_json::json!("declare_organization"),
                name: Some(reference.to_owned()),
                missing: Vec::new(),
            },
        )],
        (Resource::Project, Some(organization)) => vec![wire::Action::DeclareProjectAction(
            wire::DeclareProjectAction {
                action: serde_json::json!("declare_project"),
                organization: organization.to_owned(),
                name: Some(reference.to_owned()),
                repositories: None,
                branch: None,
                missing: vec!["repositories".to_owned(), "branch".to_owned()],
            },
        )],
        (Resource::Agent, Some(organization)) => {
            vec![wire::Action::DeclareAgentAction(wire::DeclareAgentAction {
                action: serde_json::json!("declare_agent"),
                organization: organization.to_owned(),
                name: Some(reference.to_owned()),
                harness: None,
                missing: vec!["harness".to_owned()],
            })]
        }
        (Resource::SubscriptionProfile, Some(organization)) => {
            vec![wire::Action::DeclareSubscriptionProfileAction(
                wire::DeclareSubscriptionProfileAction {
                    action: serde_json::json!("declare_subscription_profile"),
                    organization: organization.to_owned(),
                    name: Some(reference.to_owned()),
                    owner: None,
                    missing: vec!["owner".to_owned()],
                },
            )]
        }
        (Resource::ProviderCredential, Some(organization)) => {
            vec![wire::Action::SetProviderCredentialAction(
                wire::SetProviderCredentialAction {
                    action: serde_json::json!("set_provider_credential"),
                    organization: organization.to_owned(),
                    name: reference.to_owned(),
                },
            )]
        }
        (resource, organization) => list_resources(resource, organization),
    }
}

fn list_resources(resource: Resource, organization: Option<&str>) -> Vec<wire::Action> {
    vec![wire::Action::ListResourcesAction(
        wire::ListResourcesAction {
            action: serde_json::json!("list_resources"),
            organization: organization.map(str::to_owned),
            resource: wire_resource(resource),
        },
    )]
}

fn inspect(locator: Locator, organization: Option<String>) -> wire::Action {
    wire::Action::InspectResourceAction(wire::InspectResourceAction {
        action: serde_json::json!("inspect_resource"),
        resource: wire_resource(locator.resource),
        reference: Some(locator.reference),
        organization,
    })
}

/// A step needing an Organization is never offered without one, though every reason that
/// carries one is scoped.
fn next_steps(next: declined::Next, organization: Option<&str>) -> Vec<wire::Action> {
    let mut steps = vec![inspect(next.inspect, organization.map(str::to_owned))];
    let Some(organization) = organization else {
        return steps;
    };
    steps.extend(next.then.into_iter().map(|step| match step {
        Step::StopSession {
            session,
            consequence,
        } => wire::Action::StopSessionAction(wire::StopSessionAction {
            action: serde_json::json!("stop_session"),
            organization: organization.to_owned(),
            session,
            consequence: explained(consequence).to_owned(),
            effect: wire_consequence(consequence),
            requires_choice: serde_json::json!(true),
        }),
        Step::EnqueueSession { workspace } => {
            wire::Action::EnqueueSessionAction(wire::EnqueueSessionAction {
                action: serde_json::json!("enqueue_session"),
                organization: organization.to_owned(),
                workspace,
                missing: Vec::new(),
            })
        }
        Step::ReleaseInstance {
            workspace,
            instance,
        } => wire::Action::ReleaseInstanceAction(wire::ReleaseInstanceAction {
            action: serde_json::json!("release_instance"),
            organization: organization.to_owned(),
            workspace,
            instance: Some(instance),
            consequence: explained(Consequence::DiscardsUnpublishedWork).to_owned(),
            effect: wire_consequence(Consequence::DiscardsUnpublishedWork),
            requires_choice: serde_json::json!(true),
        }),
    }));
    steps
}

const fn explained(consequence: Consequence) -> &'static str {
    match consequence {
        Consequence::FailsSession => "Stopping the session now records it failed.",
        Consequence::EndsSession => "Stopping the session ends it; what it has done stands.",
        Consequence::DiscardsUnpublishedWork => {
            "Releasing the instance discards work that may exist nowhere else."
        }
    }
}

const fn wire_consequence(consequence: Consequence) -> wire::Consequence {
    match consequence {
        Consequence::FailsSession => wire::Consequence::FailsSession,
        Consequence::EndsSession => wire::Consequence::EndsSession,
        Consequence::DiscardsUnpublishedWork => wire::Consequence::DiscardsUnpublishedWork,
    }
}

const fn wire_resource(resource: Resource) -> wire::Resource {
    match resource {
        Resource::Organization => wire::Resource::Organization,
        Resource::Project => wire::Resource::Project,
        Resource::Agent => wire::Resource::Agent,
        Resource::SubscriptionProfile => wire::Resource::SubscriptionProfile,
        Resource::ProviderCredential => wire::Resource::ProviderCredential,
        Resource::Integration => wire::Resource::Integration,
        Resource::Trigger => wire::Resource::Trigger,
        Resource::Event => wire::Resource::Event,
        Resource::Workspace => wire::Resource::Workspace,
        Resource::Session => wire::Resource::Session,
        Resource::Instance => wire::Resource::Instance,
        Resource::HeldMessage => wire::Resource::HeldMessage,
        Resource::TranscriptPayload => wire::Resource::TranscriptPayload,
    }
}

fn diagnostic_message(diagnostic: &wire::Diagnostic) -> &str {
    match diagnostic {
        wire::Diagnostic::MissingReferenceDiagnostic(d) => &d.message,
        wire::Diagnostic::AmbiguousReferenceDiagnostic(d) => &d.message,
        wire::Diagnostic::MalformedRequestDiagnostic(d) => &d.message,
        wire::Diagnostic::ForbiddenActionDiagnostic(d) => &d.message,
        wire::Diagnostic::StateConflictDiagnostic(d) => &d.message,
        wire::Diagnostic::ExpiredResourceDiagnostic(d) => &d.message,
        wire::Diagnostic::InvalidFieldDiagnostic(d) => &d.message,
        wire::Diagnostic::SetupGapDiagnostic(d) => &d.message,
        wire::Diagnostic::UnavailableDiagnostic(d) => &d.message,
        wire::Diagnostic::InstanceTimeoutDiagnostic(d) => &d.message,
        wire::Diagnostic::AuthenticationFailedDiagnostic(d) => &d.message,
        wire::Diagnostic::ExecutableMissingDiagnostic(d) => &d.message,
        wire::Diagnostic::UnknownFailureDiagnostic(d) => &d.message,
        wire::Diagnostic::ConnectionFailedDiagnostic(d) => &d.message,
        wire::Diagnostic::ClientFailureDiagnostic(d) => &d.message,
        wire::Diagnostic::UnknownResponseDiagnostic(d) => &d.message,
    }
}

fn diagnosed(reason: Reason, field: Option<&'static str>) -> Refused {
    let field = field.map(str::to_owned);
    let (status, diagnostic) = match reason {
        Reason::MissingReference {
            resource,
            reference,
            organization,
            within,
            message,
        } => (
            StatusCode::NOT_FOUND,
            wire::Diagnostic::MissingReferenceDiagnostic(wire::MissingReferenceDiagnostic {
                kind: serde_json::json!("missing_reference"),
                message,
                field,
                next_steps: match within {
                    Some(within) => vec![inspect(within, organization.clone())],
                    None => declare_or_inspect(resource, &reference, organization.as_deref()),
                },
                context: wire::MissingReferenceContext {
                    resource: wire_resource(resource),
                    reference,
                    organization,
                },
            }),
        ),
        Reason::AmbiguousReference {
            resource,
            reference,
            organization,
            candidates,
            message,
        } => (
            StatusCode::NOT_FOUND,
            wire::Diagnostic::AmbiguousReferenceDiagnostic(wire::AmbiguousReferenceDiagnostic {
                kind: serde_json::json!("ambiguous_reference"),
                message,
                field,
                next_steps: list_resources(resource, organization.as_deref()),
                context: wire::AmbiguousReferenceContext {
                    resource: wire_resource(resource),
                    reference,
                    organization,
                    candidates: candidates
                        .into_iter()
                        .map(|candidate| wire::Candidate {
                            id: candidate.id,
                            name: candidate.name,
                        })
                        .collect(),
                },
            }),
        ),
        Reason::InvalidField {
            field,
            operation,
            constraint,
            allowed,
            message,
        } => return invalid_value(operation, field, constraint, allowed, message),
        Reason::StateConflict {
            operation,
            resource,
            reference,
            organization,
            state,
            holding_session,
            next,
            message,
        } => (
            StatusCode::CONFLICT,
            wire::Diagnostic::StateConflictDiagnostic(wire::StateConflictDiagnostic {
                kind: serde_json::json!("state_conflict"),
                message,
                field,
                next_steps: next_steps(next, organization.as_deref()),
                context: wire::StateConflictContext {
                    operation: operation.to_owned(),
                    resource: wire_resource(resource),
                    reference,
                    organization,
                    state: state.to_owned(),
                    holding_session,
                },
            }),
        ),
        Reason::Forbidden {
            operation,
            resource,
            reference,
            organization,
            constraint,
            next,
            message,
        } => (
            StatusCode::FORBIDDEN,
            wire::Diagnostic::ForbiddenActionDiagnostic(wire::ForbiddenActionDiagnostic {
                kind: serde_json::json!("forbidden_action"),
                message,
                field,
                next_steps: next_steps(next, organization.as_deref()),
                context: wire::ForbiddenActionContext {
                    operation: operation.to_owned(),
                    resource: wire_resource(resource),
                    reference,
                    organization,
                    constraint: Some(constraint.to_owned()),
                },
            }),
        ),
        Reason::Expired {
            operation,
            resource,
            reference,
            organization,
            next,
            message,
        } => (
            StatusCode::GONE,
            wire::Diagnostic::ExpiredResourceDiagnostic(wire::ExpiredResourceDiagnostic {
                kind: serde_json::json!("expired_resource"),
                message,
                field,
                next_steps: next_steps(next, organization.as_deref()),
                context: wire::ExpiredResourceContext {
                    resource: wire_resource(resource),
                    reference,
                    organization,
                    operation: operation.to_owned(),
                },
            }),
        ),
        Reason::InstanceTimeout {
            operation,
            workspace,
            instance,
            message,
        } => (
            StatusCode::GATEWAY_TIMEOUT,
            wire::Diagnostic::InstanceTimeoutDiagnostic(wire::InstanceTimeoutDiagnostic {
                kind: serde_json::json!("instance_timeout"),
                message,
                field,
                next_steps: vec![retry_read(operation, Some(Resource::Workspace), None)],
                context: wire::InstanceTimeoutContext {
                    workspace,
                    instance: Some(instance),
                    operation: operation.to_owned(),
                },
            }),
        ),
    };
    Refused::Diagnosed {
        status,
        diagnostic: Box::new(diagnostic),
    }
}

fn retry_read(
    operation: &str,
    resource: Option<Resource>,
    retry_after_seconds: Option<i64>,
) -> wire::Action {
    wire::Action::RetryReadAction(wire::RetryReadAction {
        action: serde_json::json!("retry_read"),
        operation: operation.to_owned(),
        resource: resource.map(|resource| wire_resource(resource).as_str().to_owned()),
        retry_after_seconds,
    })
}

fn invalid_field(
    operation: &'static str,
    field: &'static str,
    constraint: Constraint,
    message: impl Into<String>,
) -> Refused {
    invalid_value(operation, field, constraint, None, message.into())
}

fn invalid_value(
    operation: &'static str,
    field: &'static str,
    constraint: Constraint,
    allowed: Option<Vec<String>>,
    message: String,
) -> Refused {
    Refused::Diagnosed {
        status: StatusCode::UNPROCESSABLE_ENTITY,
        diagnostic: Box::new(wire::Diagnostic::InvalidFieldDiagnostic(
            wire::InvalidFieldDiagnostic {
                kind: serde_json::json!("invalid_field"),
                message,
                field: Some(field.to_owned()),
                context: wire::InvalidFieldContext {
                    field: field.to_owned(),
                    constraint: constraint.as_str().to_owned(),
                    allowed_values: allowed.clone(),
                },
                next_steps: vec![wire::Action::CorrectFieldAction(wire::CorrectFieldAction {
                    action: serde_json::json!("correct_field"),
                    operation: operation.to_owned(),
                    resource: None,
                    field: field.to_owned(),
                    constraint: constraint.as_str().to_owned(),
                    allowed_values: allowed,
                })],
            },
        )),
    }
}

impl From<anyhow::Error> for Refused {
    fn from(error: anyhow::Error) -> Self {
        let error = match error.downcast::<Reason>() {
            Ok(reason) => return diagnosed(reason, None),
            Err(error) => error,
        };
        let error = match error.downcast::<Concerning>() {
            Ok(concerning) => return diagnosed(concerning.reason, Some(concerning.field)),
            Err(error) => error,
        };
        match error.downcast::<Declined>() {
            Ok(Declined::Unacceptable(why)) => Refused::Unprocessable(why),
            Ok(Declined::Missing(why) | Declined::Ambiguous(why)) => Refused::NotFound(why),
            Ok(Declined::Taken(why)) => Refused::Conflict(why),
            Err(error) => Refused::Unavailable(error),
        }
    }
}

impl From<JsonRejection> for Refused {
    fn from(rejection: JsonRejection) -> Self {
        Refused::Malformed {
            field: None,
            why: rejection.body_text(),
        }
    }
}

impl From<Unreadable> for Refused {
    fn from(unreadable: Unreadable) -> Self {
        match unreadable {
            Unreadable::Cursor(why) => Refused::Malformed { field: None, why },
            Unreadable::Unavailable(error) => Refused::Unavailable(error),
        }
    }
}

/// What a refusal raised without its operation leaves for `diagnosing`, which knows the route.
#[derive(Clone)]
enum Undiagnosed {
    Malformed {
        field: Option<&'static str>,
        message: String,
    },
    Unavailable {
        busy: bool,
    },
}

impl IntoResponse for Refused {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            Refused::Diagnosed { status, diagnostic } => {
                return (status, Json(diagnostic)).into_response();
            }
            Refused::Malformed { field, why } => {
                let mut response = StatusCode::BAD_REQUEST.into_response();
                response.extensions_mut().insert(Undiagnosed::Malformed {
                    field,
                    message: why,
                });
                return response;
            }
            Refused::Unavailable(error) => {
                let busy = store::busy(&error);
                warn!(%error, busy, "the operator boundary could not answer");
                let mut response = StatusCode::SERVICE_UNAVAILABLE.into_response();
                response
                    .extensions_mut()
                    .insert(Undiagnosed::Unavailable { busy });
                return response;
            }
            Refused::Forbidden(why) => (StatusCode::FORBIDDEN, why),
            Refused::NotFound(why) => (StatusCode::NOT_FOUND, why),
            Refused::Conflict(why) => (StatusCode::CONFLICT, why),
            Refused::Unprocessable(why) => (StatusCode::UNPROCESSABLE_ENTITY, why),
        };

        (status, Json(Refusal { message })).into_response()
    }
}

/// A malformed request or an unanswerable one is diagnosed against the route it reached, so
/// every handler need not name its own operation.
async fn diagnosing(request: Request, next: Next) -> Response {
    let read = request.method() == axum::http::Method::GET;
    let operation = request
        .extensions()
        .get::<axum::extract::MatchedPath>()
        .and_then(|path| operation(request.method().as_str(), path.as_str()))
        .unwrap_or("unknown");
    let response = next.run(request).await;
    let Some(undiagnosed) = response.extensions().get::<Undiagnosed>().cloned() else {
        return response;
    };

    let (status, busy, diagnostic) = match undiagnosed {
        Undiagnosed::Malformed { field, message } => (
            StatusCode::BAD_REQUEST,
            false,
            wire::Diagnostic::MalformedRequestDiagnostic(wire::MalformedRequestDiagnostic {
                kind: serde_json::json!("malformed_request"),
                message,
                field: field.map(str::to_owned),
                context: wire::MalformedRequestContext {
                    operation: operation.to_owned(),
                    field: field.map(str::to_owned),
                },
                next_steps: vec![match field {
                    Some(field) => wire::Action::CorrectFieldAction(wire::CorrectFieldAction {
                        action: serde_json::json!("correct_field"),
                        operation: operation.to_owned(),
                        resource: None,
                        field: field.to_owned(),
                        constraint: "well_formed".to_owned(),
                        allowed_values: None,
                    }),
                    None => inspect_operation(operation, false),
                }],
            }),
        ),
        Undiagnosed::Unavailable { busy } => (
            StatusCode::SERVICE_UNAVAILABLE,
            busy,
            wire::Diagnostic::UnavailableDiagnostic(wire::UnavailableDiagnostic {
                kind: serde_json::json!("unavailable"),
                message: "the control plane could not answer".to_owned(),
                field: None,
                context: wire::UnavailableContext {
                    service: "control_plane".to_owned(),
                    resource: None,
                    operation: operation.to_owned(),
                    retry_after_seconds: busy.then_some(serve::BUSY_RETRY_AFTER_SECONDS),
                },
                // A write that went unanswered may have landed, so it is inspected, never replayed.
                next_steps: vec![if read {
                    retry_read(
                        operation,
                        None,
                        busy.then_some(serve::BUSY_RETRY_AFTER_SECONDS),
                    )
                } else {
                    inspect_operation(operation, true)
                }],
            }),
        ),
    };

    serve::refusal(status, busy, Json(diagnostic))
}

fn inspect_operation(operation: &str, uncertain: bool) -> wire::Action {
    wire::Action::InspectOperationAction(wire::InspectOperationAction {
        action: serde_json::json!("inspect_operation"),
        operation: operation.to_owned(),
        resource: None,
        uncertain,
    })
}

fn operation(method: &str, path: &str) -> Option<&'static str> {
    Some(match (method, path) {
        ("GET", HARNESSES) => "list_harnesses",
        ("GET", SIGN_IN_METHOD) => "show_sign_in_method",
        ("GET", ORGANIZATIONS) => "list_organizations",
        ("POST", ORGANIZATIONS) => "declare_organization",
        ("POST", STARTS) => "start",
        ("GET", PROJECTS) => "list_projects",
        ("POST", PROJECTS) => "declare_project",
        ("GET", AGENTS) => "list_agents",
        ("POST", AGENTS) => "declare_agent",
        ("PUT", AGENT_MODEL) => "set_agent_model",
        ("POST", DECLARATION) => "apply_declaration",
        ("POST", DECLARATION_PREVIEW) => "preview_declaration",
        ("GET", CREDENTIALS) => "list_provider_credentials",
        ("PUT", CREDENTIAL) => "hold_provider_credential",
        ("DELETE", CREDENTIAL) => "forget_provider_credential",
        ("GET", PROFILES) => "list_subscription_profiles",
        ("POST", PROFILES) => "declare_subscription_profile",
        ("PUT", PROFILE_VARIABLE) => "hold_subscription_profile_variable",
        ("DELETE", PROFILE_VARIABLE) => "forget_subscription_profile_variable",
        ("PUT", PROFILE_FILE) => "hold_subscription_profile_file",
        ("DELETE", PROFILE_FILE) => "forget_subscription_profile_file",
        ("GET", INTEGRATIONS) => "list_integrations",
        ("POST", INTEGRATIONS) => "register_integration",
        ("POST", GITHUB_APP) => "start_github_app",
        ("GET", integration::manifest::PAGE) => "github_app_setup",
        ("GET", integration::manifest::CALLBACK) => "github_app_callback",
        ("GET", integration::manifest::INSTALLED) => "github_app_installed",
        ("DELETE", EVENT_REFUSAL) => "acknowledge_event_refusal",
        ("GET", EVENTS) => "list_events",
        ("GET", EVENT) => "show_event",
        ("GET", TRIGGERS) => "list_triggers",
        ("POST", TRIGGERS) => "declare_trigger",
        ("GET", TRIGGER) => "show_trigger",
        ("POST", TRIGGER_TEST) => "test_trigger",
        ("POST", TRIGGER_DISABLE) => "disable_trigger",
        ("POST", TRIGGER_ENABLE) => "enable_trigger",
        ("POST", TRIGGER_DISPATCH) => "dispatch_trigger",
        ("POST", APPLIED_TRIGGERS) => "apply_triggers",
        ("POST", APPLIED_TRIGGERS_PREVIEW) => "preview_applied_triggers",
        ("GET", INSTANCES) => "list_held_instances",
        ("GET", QUEUE) => "show_queue",
        ("GET", WORKSPACES) => "list_workspaces",
        ("POST", WORKSPACES) => "open_workspace",
        ("GET", WORKSPACE) => "show_workspace",
        ("GET", WORKSPACE_WORK) => "workspace_work",
        ("GET", WORKSPACE_CHANGES) => "workspace_changes",
        ("GET", WORKSPACE_COMMITS) => "workspace_commits",
        ("GET", WORKSPACE_STASHES) => "workspace_stashes",
        ("GET", WORKSPACE_FILES) => "workspace_files",
        ("GET", WORKSPACE_FILE) => "workspace_file",
        ("POST", WORKSPACE_MESSAGES) => "post_to_workspace",
        ("PUT", WORKSPACE_MESSAGE) => "edit_workspace_message",
        ("DELETE", WORKSPACE_MESSAGE) => "withdraw_workspace_message",
        ("POST", WORKSPACE_SEAL) => "seal_workspace",
        ("POST", WORKSPACE_INSTANCE_RELEASE) => "release_instance",
        ("GET", SESSIONS) => "list_sessions",
        ("POST", SESSIONS) => "enqueue_session",
        ("GET", SESSION) => "show_session",
        ("POST", SESSION_STOP) => "stop_session",
        ("POST", SESSION_INTERRUPT) => "interrupt_session",
        ("POST", SESSION_OPTIONS) => "set_session_option",
        ("GET", TRANSCRIPT) => "transcript",
        ("POST", FOLLOWER_LEASE) => "renew_follower",
        ("GET", TRANSCRIPT_PAYLOAD) => "transcript_payload",
        ("GET", CHANGES) => "changes",
        ("PUT", STREAMS) => "reserve_stream",
        ("GET", STREAM) => "open_stream",
        ("PUT", STREAM_SUBSCRIPTION) => "subscribe_stream",
        ("DELETE", STREAM_SUBSCRIPTION) => "unsubscribe_stream",
        _ => return None,
    })
}

#[derive(Serialize)]
struct Refusal {
    message: String,
}

#[cfg(test)]
mod tests {
    use super::warns_cache;

    #[test]
    fn the_cache_warning_table_is_per_category_and_per_harness() {
        for category in ["model", "thought_level", "model_config"] {
            assert!(
                warns_cache("opencode", Some(category)),
                "{category} warns for opencode"
            );
        }
        assert!(!warns_cache("opencode", Some("mode")));
        assert!(!warns_cache("opencode", None));
        assert!(!warns_cache("opencode", Some("_scripted")));

        assert!(
            !warns_cache("claude", Some("thought_level")),
            "Claude keeps the prompt cache across a change of effort"
        );
        assert!(warns_cache("claude", Some("model")));
    }

    #[test]
    fn every_route_is_diagnosed_under_the_operation_it_is_documented_as() {
        let document: serde_json::Value =
            serde_json::from_str(include_str!("../../../openapi/operator.json"))
                .expect("a valid operator document");
        let paths = document["paths"].as_object().expect("paths");

        for (path, operations) in paths {
            for (method, documented) in operations.as_object().expect("operations") {
                let Some(id) = documented["operationId"].as_str() else {
                    continue;
                };
                let snake: String = id
                    .chars()
                    .flat_map(|character| {
                        let lower = character.to_ascii_lowercase();
                        (character.is_ascii_uppercase())
                            .then_some('_')
                            .into_iter()
                            .chain([lower])
                    })
                    .collect();
                assert_eq!(
                    super::operation(&method.to_ascii_uppercase(), path),
                    Some(snake.as_str()),
                    "{method} {path}"
                );
            }
        }
    }

    #[test]
    fn a_refusals_sentence_never_decides_its_status_or_next_steps() {
        use crate::declined::{Consequence, Next, Reason, Resource, Step};

        let answered = |message: &str| {
            let reason = Reason::StateConflict {
                operation: "seal_workspace",
                resource: Resource::Workspace,
                reference: "w".to_owned(),
                organization: Some("acme".to_owned()),
                state: "session_in_flight",
                holding_session: Some("s".to_owned()),
                next: Next::inspect(Resource::Session, "s").then(Step::StopSession {
                    session: "s".to_owned(),
                    consequence: Consequence::FailsSession,
                }),
                message: message.to_owned(),
            };
            let super::Refused::Diagnosed { status, diagnostic } = super::diagnosed(reason, None)
            else {
                panic!("a typed reason is diagnosed");
            };
            let mut body = serde_json::to_value(diagnostic).expect("a serializable diagnostic");
            body["message"] = serde_json::Value::Null;
            (status, body)
        };

        assert_eq!(
            answered("the session s is still in flight in the workspace w"),
            answered("the workspace w has no instance to release")
        );
    }
}
