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
use axum::{BoxError, Form, Json, Router};
use futures_core::Stream;
use jiff::{SignedDuration, Timestamp};
use kestrel_operator_types as wire;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use sha2::Digest as _;
use tokio_stream::{StreamExt as _, StreamMap};
use tokio_util::sync::CancellationToken;
use tracing::warn;

use crate::agent;
use crate::capability::{self, Capability, Images};
use crate::declaration::{self, triggers};
use crate::declined::{
    self as declined, Again, Concerning, Consequence, Constraint, Declined, Locator, Reason,
    Resource, Step, Within,
};
use crate::domain::{
    self, Agent, Connection, CorrelationMiss, Declared, Direction, DisableReason, EventRecordId,
    Fires, HeldMessage, Integration, IntegrationKind, IntegrationState, OnOpenWorkspace,
    Organization, Project, Schedule, Session, StartedBy, SubscriptionProfile, Trigger,
    TriggerState, Workspace, WorkspaceId, WorkspaceState,
};
use crate::fanout;
use crate::integration::github::{self, Github};
use crate::integration::{self, Connecting, Registration};
use crate::log::{self, Cursor, Unreadable, Window};
use crate::participant;
use crate::profile::{self, Entry};
use crate::provider::{self, Held};
use crate::repository;
use crate::role::serve;
use crate::scheduling;
use crate::sign_in;
use crate::sign_in::handoff::{self, Attribution};
use crate::start_readiness;
use crate::store::workspace::HeldMessageRefusal;
use crate::store::{self, Declared as DeclaredRecord, Store};
use crate::stream::{self, Emitted};
use crate::trigger;
use crate::{instance, pull_request, start, work, workspace};

pub const ROOT: &str = "/";
pub const HARNESSES: &str = "/operator/harnesses";
pub const SIGN_IN_METHOD: &str = "/operator/harnesses/{harness}/sign-in-methods/{method}";
pub const INSTALL: &str = "/operator/install";
pub const READINESS: &str = "/operator/readiness";
pub const OPERATOR: &str = "/operator/operator";
pub const ORGANIZATIONS: &str = "/operator/organizations";
pub const STARTS: &str = "/operator/starts";
pub const REPOSITORY_RESOLUTIONS: &str = "/operator/repository-resolutions";
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
pub const SIGN_INS: &str = "/operator/organizations/{organization}/sign-ins";
pub const SIGN_IN: &str = "/operator/organizations/{organization}/sign-ins/{harness}/{method}";
pub const SIGN_IN_TESTS: &str =
    "/operator/organizations/{organization}/sign-ins/{harness}/{method}/tests";
pub const SIGN_IN_TEST: &str =
    "/operator/organizations/{organization}/sign-ins/{harness}/{method}/tests/{test}";
pub const GITHUB_APP: &str = "/operator/organizations/{organization}/github-app";
pub const INTEGRATIONS: &str = "/operator/organizations/{organization}/integrations";
pub const INTEGRATION: &str = "/operator/organizations/{organization}/integrations/{integration}";
pub const INTEGRATION_DISABLE: &str =
    "/operator/organizations/{organization}/integrations/{integration}/disable";
pub const INTEGRATION_ENABLE: &str =
    "/operator/organizations/{organization}/integrations/{integration}/enable";
pub const INTEGRATION_RETIRE: &str =
    "/operator/organizations/{organization}/integrations/{integration}/retire";
pub const INTEGRATION_PRIVATE_KEY: &str =
    "/operator/organizations/{organization}/integrations/{integration}/private-key";
pub const INTEGRATION_REPOSITORIES: &str =
    "/operator/organizations/{organization}/integrations/{integration}/repositories";
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
    providers: sign_in::check::Providers,
    images: Images,
    client: url::Url,
    github_app_api: String,
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

async fn harnesses(
    State(control_plane): State<ControlPlane>,
) -> Json<Vec<wire::HarnessCatalogueEntry>> {
    let capability = control_plane.images.read().await;
    Json(
        crate::catalogue::harnesses()
            .iter()
            .map(|row| wire::HarnessCatalogueEntry {
                availability: Some(availability(&capability, &row.name)),
                ..row.clone()
            })
            .collect(),
    )
}

fn availability(capability: &Capability, harness: &str) -> wire::HarnessAvailability {
    let (state, image, identity, diagnostic) = match capability {
        Capability::Unchecked => (wire::HarnessAvailabilityState::Unchecked, None, None, None),
        Capability::Inspected {
            image,
            identity,
            harnesses,
        } => (
            if harnesses.contains(harness) {
                wire::HarnessAvailabilityState::Available
            } else {
                wire::HarnessAvailabilityState::NotCarried
            },
            Some(image.clone()),
            Some(identity.clone()),
            None,
        ),
        Capability::Unavailable { image, cause } => (
            wire::HarnessAvailabilityState::Unavailable,
            Some(image.clone()),
            None,
            Some(wire::Diagnostic::UnavailableDiagnostic(image_unavailable(
                "list_harnesses",
                Some(harness.to_owned()),
                image.clone(),
                capability::unavailable(image, cause, Some(harness)),
                None,
            ))),
        ),
    };
    wire::HarnessAvailability {
        state,
        image,
        identity,
        diagnostic,
    }
}

async fn sign_in_method(
    Path((harness, method)): Path<(String, String)>,
) -> Result<Json<wire::SignInMethod>, Refused> {
    crate::catalogue::sign_in_method("show_sign_in_method", &harness, &method)
        .cloned()
        .map(Json)
        .map_err(Into::into)
}

#[allow(clippy::too_many_arguments)]
pub fn router(
    store: Store,
    shutdown: CancellationToken,
    live: crate::live::Live,
    followers: crate::presence::Followers,
    streams: stream::Streams,
    images: Images,
    client: url::Url,
    providers: sign_in::check::Providers,
    github_app_api: String,
) -> Router {
    let root = format!(
        "kestrel operator API\n\nClients reach this API under /operator. Open the browser Client at {client}\n"
    );
    Router::new()
        .route(ROOT, get(move || std::future::ready(root.clone())))
        .route(HARNESSES, get(harnesses))
        .route(SIGN_IN_METHOD, get(sign_in_method))
        .route(INSTALL, get(show_install))
        .route(READINESS, get(show_readiness))
        .route(OPERATOR, get(show_operator).put(name_operator))
        .route(ORGANIZATIONS, get(organizations).post(declare_organization))
        .route(STARTS, post(start))
        .route(REPOSITORY_RESOLUTIONS, post(resolve_repositories))
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
        .route(SIGN_INS, get(sign_ins))
        .route(SIGN_IN, put(save_sign_in))
        .route(SIGN_IN_TESTS, post(test_sign_in))
        .route(SIGN_IN_TEST, get(sign_in_test))
        .route(INTEGRATIONS, get(integrations).post(register_integration))
        .route(INTEGRATION, get(show_integration).patch(change_integration))
        .route(INTEGRATION_DISABLE, post(disable_integration))
        .route(INTEGRATION_ENABLE, post(enable_integration))
        .route(INTEGRATION_RETIRE, post(retire_integration))
        .route(INTEGRATION_REPOSITORIES, get(integration_repositories))
        .route(
            INTEGRATION_PRIVATE_KEY,
            put(replace_integration_private_key),
        )
        .route(GITHUB_APP, post(start_github_app))
        .route(
            integration::manifest::PAGE,
            get(github_app_page).post(github_app_setup_finish),
        )
        .route(integration::manifest::CALLBACK, get(github_app_callback))
        .route(integration::manifest::INSTALLED, get(github_app_installed))
        .route(
            integration::manifest::FLOW,
            get(github_app_status).delete(github_app_cancel),
        )
        .route(
            integration::manifest::INSTALLATIONS,
            get(github_app_installations),
        )
        .route(
            integration::manifest::REPOSITORIES,
            get(github_app_repositories),
        )
        .route(integration::manifest::FINISH, post(github_app_finish))
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
            providers,
            github_app_api,
            images,
            client,
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

fn agent_defaults(declaration: &wire::AgentDeclaration) -> Declared {
    Declared::named(Declared {
        model: declaration.model.clone().flatten(),
        mode: declaration.mode.clone().flatten(),
        thought_level: declaration.thought_level.clone().flatten(),
    })
}

fn selected(integrations: Vec<domain::SelectedIntegration>) -> Vec<wire::SelectedIntegration> {
    integrations
        .into_iter()
        .map(|selected| wire::SelectedIntegration {
            repository: selected.repository,
            integration: selected.name,
            integration_id: selected.integration.to_string().parse().expect("a uuid"),
        })
        .collect()
}

fn queue_standing(
    snapshot: &scheduling::Snapshot,
    workspace: WorkspaceId,
) -> Option<wire::QueueStanding> {
    if let Some(queued) = snapshot
        .queued
        .iter()
        .find(|queued| queued.session.workspace == workspace)
    {
        return Some(wire::QueueStanding {
            position: queued.position.map(|position| position as i64),
            reasons: reasons(queued.reasons.clone()),
            pending_since: None,
        });
    }
    if let Some(waiting) = snapshot
        .waiting
        .iter()
        .find(|waiting| waiting.session.workspace == workspace)
    {
        return Some(wire::QueueStanding {
            position: waiting.position.map(|position| position as i64),
            reasons: reasons(waiting.reasons.clone()),
            pending_since: waiting.pending_since.map(|since| since.to_string()),
        });
    }
    snapshot
        .unbriefed
        .iter()
        .find(|unbriefed| unbriefed.session.workspace == workspace)
        .map(|unbriefed| wire::QueueStanding {
            position: unbriefed.position.map(|position| position as i64),
            reasons: reasons(unbriefed.reasons.clone()),
            pending_since: unbriefed
                .brief_since
                .or(unbriefed.pending_since)
                .map(|since| since.to_string()),
        })
}

fn supplied_material(harness: &str, supplied: &domain::Supplied) -> wire::SuppliedMaterial {
    let entry = Entry {
        kind: supplied.kind,
        name: supplied.name.clone(),
    };
    wire::SuppliedMaterial {
        resource: match supplied.profile {
            Some(_) => wire::SuppliedMaterialResource::SubscriptionProfile,
            None => wire::SuppliedMaterialResource::ProviderCredential,
        },
        reference: supplied
            .profile
            .clone()
            .unwrap_or_else(|| supplied.name.clone()),
        kind: match supplied.kind {
            profile::Kind::Variable => wire::SuppliedMaterialKind::Variable,
            profile::Kind::File => wire::SuppliedMaterialKind::File,
        },
        name: supplied.name.clone(),
        method: handoff::slot(harness, &entry).map(|method| method.id.clone()),
        revision: supplied.revision,
        current: supplied.current,
        handed_at: supplied.handed_at.to_string(),
    }
}

fn holding(method: &wire::SignInMethod, supplied: &domain::Supplied) -> wire::SignInHolding {
    wire::SignInHolding {
        resource: match supplied.profile {
            Some(_) => wire::SignInHoldingResource::SubscriptionProfile,
            None => wire::SignInHoldingResource::ProviderCredential,
        },
        reference: supplied
            .profile
            .clone()
            .unwrap_or_else(|| supplied.name.clone()),
        fills: method.fills.clone(),
    }
}

/// No image is recorded against a Session and no evidence a supervisor carries establishes
/// expiry or coverage, so none of the three is ever claimed.
fn diagnosed_failure(
    session: &Session,
    organization: &str,
    evidence: domain::Evidence,
) -> wire::Diagnostic {
    let harness = session.agent.harness.clone();
    let inspect_session = inspect(
        Locator::new(Resource::Session, session.id.to_string()),
        Some(organization.to_owned()),
    );
    match evidence {
        domain::Evidence::AuthenticationRequired {
            code,
            methods,
            method,
        } => {
            let needed = format!("the {harness} harness needed a sign-in before it would work");
            let evidence = wire::AuthenticationRequiredEvidence {
                kind: serde_json::json!("authentication_required"),
                code: code.into(),
                methods,
                method: method.clone(),
            };
            let sign_in = |method: Option<String>, held_in: Option<String>| {
                wire::Action::SignInAction(wire::SignInAction {
                    action: serde_json::json!("sign_in"),
                    harness: Some(harness.clone()),
                    method,
                    sign_in: held_in,
                })
            };
            let unattributed = wire::AuthenticationFailedContext {
                session: session.id.to_string(),
                harness: harness.clone(),
                image: None,
                evidence,
                sign_in: session.sign_in_method.clone(),
                attribution: wire::AuthenticationFailedContextAttribution::Unattributed,
                candidates: Vec::new(),
                holding: None,
                revision: None,
                current: None,
                expired: None,
                covered: None,
            };
            let (message, next_steps, context) = match handoff::attribution(session) {
                Attribution::Established(used, supplied) => {
                    let holding = holding(used, &supplied);
                    (
                        format!(
                            "{needed}; it was handed the {} sign-in held in the {} {}{}",
                            used.name,
                            match supplied.profile {
                                Some(_) => Resource::SubscriptionProfile.noun(),
                                None => Resource::ProviderCredential.noun(),
                            },
                            holding.reference,
                            match supplied.current {
                                true => "",
                                false => ", which has been replaced since",
                            }
                        ),
                        vec![
                            sign_in(Some(used.id.clone()), Some(holding.reference.clone())),
                            inspect_session,
                        ],
                        wire::AuthenticationFailedContext {
                            sign_in: Some(used.id.clone()),
                            attribution: wire::AuthenticationFailedContextAttribution::Established,
                            holding: Some(holding),
                            revision: Some(supplied.revision),
                            current: Some(supplied.current),
                            ..unattributed
                        },
                    )
                }
                Attribution::Uncertain(candidates) => {
                    let ids: Vec<String> = candidates
                        .iter()
                        .map(|(method, _)| method.id.clone())
                        .collect();
                    let mut next_steps = Vec::new();
                    if let Some(profile) = candidates
                        .iter()
                        .find_map(|(_, supplied)| supplied.profile.clone())
                    {
                        next_steps.push(inspect(
                            Locator::new(Resource::SubscriptionProfile, profile),
                            Some(organization.to_owned()),
                        ));
                    }
                    next_steps.push(inspect_session);
                    (
                        format!(
                            "{needed}; it was handed several sign-ins ({}), and nothing \
                             established which one it refused",
                            ids.join(", ")
                        ),
                        next_steps,
                        wire::AuthenticationFailedContext {
                            sign_in: None,
                            attribution: wire::AuthenticationFailedContextAttribution::Uncertain,
                            candidates: ids,
                            ..unattributed
                        },
                    )
                }
                Attribution::Unattributed => (
                    needed,
                    vec![
                        sign_in(session.sign_in_method.clone().or(method), None),
                        inspect_session,
                    ],
                    unattributed,
                ),
            };
            wire::Diagnostic::AuthenticationFailedDiagnostic(wire::AuthenticationFailedDiagnostic {
                kind: serde_json::json!("authentication_failed"),
                message,
                field: None,
                next_steps,
                context,
            })
        }
        domain::Evidence::ExecutableMissing { command, error } => {
            wire::Diagnostic::ExecutableMissingDiagnostic(wire::ExecutableMissingDiagnostic {
                kind: serde_json::json!("executable_missing"),
                message: format!(
                    "the {harness} harness's executable {command} could not be spawned"
                ),
                field: None,
                next_steps: vec![
                    wire::Action::InspectHarnessImageAction(wire::InspectHarnessImageAction {
                        action: serde_json::json!("inspect_harness_image"),
                        harness: Some(harness.clone()),
                        image: None,
                        command: Some(command.clone()),
                    }),
                    inspect_session,
                ],
                context: wire::ExecutableMissingContext {
                    session: session.id.to_string(),
                    harness,
                    image: None,
                    executable: command.clone(),
                    evidence: wire::ExecutableMissingEvidence {
                        kind: serde_json::json!("executable_missing"),
                        command,
                        error: wire::OsError {
                            kind: match error.kind {
                                domain::OsErrorKind::NotFound => wire::OsErrorKind::NotFound,
                                domain::OsErrorKind::PermissionDenied => {
                                    wire::OsErrorKind::PermissionDenied
                                }
                                domain::OsErrorKind::Other => wire::OsErrorKind::Other,
                            },
                            code: error.code.map(i64::from),
                        },
                    },
                },
            })
        }
        domain::Evidence::Unknown { summary } => {
            wire::Diagnostic::UnknownFailureDiagnostic(wire::UnknownFailureDiagnostic {
                kind: serde_json::json!("unknown_failure"),
                message: "the session failed for a reason kestrel could not establish".to_owned(),
                field: None,
                next_steps: vec![inspect_session],
                context: wire::UnknownFailureContext {
                    session: Some(session.id.to_string()),
                    resource: Some(wire_resource(Resource::Session).as_str().to_owned()),
                    evidence: Some(wire::UnknownEvidence {
                        kind: serde_json::json!("unknown"),
                        summary,
                    }),
                },
            })
        }
    }
}

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

impl From<Organization> for wire::Organization {
    fn from(organization: Organization) -> Self {
        Self {
            id: organization
                .id
                .to_string()
                .parse()
                .expect("an Organization UUID"),
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
            id: project.id.to_string().parse().expect("a Project UUID"),
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
            id: agent.id.to_string().parse().expect("an Agent UUID"),
            name: agent.name,
            harness: agent.harness,
            model: agent.declared.model,
            mode: agent.declared.mode,
            thought_level: agent.declared.thought_level,
        }
    }
}

fn session_state(value: domain::SessionState) -> wire::SessionState {
    match value {
        domain::SessionState::Queued => wire::SessionState::Queued,
        domain::SessionState::Working => wire::SessionState::Working,
        domain::SessionState::Trailing => wire::SessionState::Trailing,
        domain::SessionState::Waiting => wire::SessionState::Waiting,
        domain::SessionState::Unbriefed => wire::SessionState::Unbriefed,
        domain::SessionState::Ended => wire::SessionState::Ended,
        domain::SessionState::Unreachable => wire::SessionState::Unreachable,
    }
}

fn unfinished_state(value: domain::SessionState) -> wire::UnfinishedSessionState {
    match value {
        domain::SessionState::Queued => wire::UnfinishedSessionState::Queued,
        domain::SessionState::Working => wire::UnfinishedSessionState::Working,
        domain::SessionState::Trailing => wire::UnfinishedSessionState::Trailing,
        domain::SessionState::Waiting => wire::UnfinishedSessionState::Waiting,
        domain::SessionState::Unbriefed => wire::UnfinishedSessionState::Unbriefed,
        domain::SessionState::Ended => wire::UnfinishedSessionState::Ended,
        domain::SessionState::Unreachable => wire::UnfinishedSessionState::Unreachable,
    }
}

fn session_preparing(value: domain::Preparing) -> wire::SessionPreparing {
    match value {
        domain::Preparing::Provisioning => wire::SessionPreparing::Provisioning,
        domain::Preparing::Cloning => wire::SessionPreparing::Cloning,
        domain::Preparing::StartingHarness => wire::SessionPreparing::StartingHarness,
        domain::Preparing::HarnessReady => wire::SessionPreparing::HarnessReady,
    }
}

fn unfinished_preparing(value: domain::Preparing) -> wire::UnfinishedSessionPreparing {
    match value {
        domain::Preparing::Provisioning => wire::UnfinishedSessionPreparing::Provisioning,
        domain::Preparing::Cloning => wire::UnfinishedSessionPreparing::Cloning,
        domain::Preparing::StartingHarness => wire::UnfinishedSessionPreparing::StartingHarness,
        domain::Preparing::HarnessReady => wire::UnfinishedSessionPreparing::HarnessReady,
    }
}

pub(crate) fn held_message(message: HeldMessage) -> wire::HeldMessage {
    wire::HeldMessage {
        id: message.id,
        participant: message.participant,
        message: message.message,
        posted_at: message.posted_at.to_string(),
        edited_at: message.edited_at.map(|at| at.to_string()),
    }
}

fn pull_request_availability(value: pull_request::Availability) -> wire::PullRequestAvailability {
    wire::PullRequestAvailability {
        repository: value.repository,
        availability: if value.known.is_some() {
            wire::PullRequestAvailabilityAvailability::Available
        } else {
            wire::PullRequestAvailabilityAvailability::Unavailable
        },
        known: value.known.map(|known| {
            known
                .into_iter()
                .map(|pr| wire::PullRequest {
                    repository: pr.repository,
                    number: pr.number,
                    url: pr.url,
                    title: pr.title,
                    state: match pr.state {
                        domain::PullRequestState::Open => wire::PullRequestState::Open,
                        domain::PullRequestState::Closed => wire::PullRequestState::Closed,
                        domain::PullRequestState::Merged => wire::PullRequestState::Merged,
                    },
                    head_branch: pr.head_branch,
                    head_revision: pr.head_revision,
                    updated_at: pr.updated_at.to_string(),
                    event: pr.event.to_string().parse().expect("an Event UUID"),
                })
                .collect()
        }),
    }
}

pub(crate) fn session_exit(exit: domain::Exit) -> wire::Exit {
    match exit {
        domain::Exit::Succeeded => wire::Exit {
            status: wire::ExitStatus::Succeeded,
            because: None,
        },
        domain::Exit::Failed { because } => wire::Exit {
            status: wire::ExitStatus::Failed,
            because: Some(because),
        },
    }
}

pub(crate) fn session_usage(usage: domain::Usage) -> wire::Usage {
    wire::Usage {
        context_used: usage
            .context_used
            .try_into()
            .expect("context usage fits a wire integer"),
        context_size: usage
            .context_size
            .try_into()
            .expect("context size fits a wire integer"),
        cost: usage.cost.map(|cost| wire::Cost {
            amount: cost.amount,
            currency: cost.currency,
        }),
    }
}

pub(crate) fn running_tool(tool: crate::live_work::RunningTool) -> wire::RunningTool {
    wire::RunningTool {
        call_id: tool.call_id,
        title: tool.title,
        tool_kind: tool.tool_kind,
        status: match tool.status {
            crate::log::ToolStatus::Pending => "pending",
            crate::log::ToolStatus::InProgress => "in_progress",
            crate::log::ToolStatus::Completed => "completed",
            crate::log::ToolStatus::Failed => "failed",
        }
        .to_owned(),
        started_at: tool.started_at.to_string(),
    }
}

pub(crate) fn running_unit(unit: crate::live_work::RunningUnit) -> wire::RunningUnit {
    wire::RunningUnit {
        id: unit.id,
        title: unit.title,
        started_at: unit.started_at.to_string(),
        kind: match unit.kind {
            crate::live_work::UnitKind::BackgroundTask => wire::RunningUnitKind::BackgroundTask,
            crate::live_work::UnitKind::Subagent => wire::RunningUnitKind::Subagent,
        },
    }
}

pub(crate) fn session_observation(
    observation: crate::live_work::Observation,
) -> wire::SessionObservation {
    match observation {
        crate::live_work::Observation::Current { observed_at } => {
            wire::SessionObservation::SessionObservationCurrent(wire::SessionObservationCurrent {
                availability: "current".into(),
                observed_at: observed_at.to_string(),
            })
        }
        crate::live_work::Observation::Unavailable { last } => {
            wire::SessionObservation::SessionObservationUnavailable(
                wire::SessionObservationUnavailable {
                    availability: "unavailable".into(),
                    last: last.map(|last| wire::LastObservation {
                        observed_at: last.observed_at.to_string(),
                        tools: last.tools.into_iter().map(running_tool).collect(),
                        units: last.units.into_iter().map(running_unit).collect(),
                    }),
                },
            )
        }
    }
}

/// What the Workspace's live work reads: a live report, no Instance, or an Instance that isn't
/// answering, each carrying the last durable report and every earlier Instance's.
pub(crate) fn workspace_work(work: crate::live_work::Work) -> wire::WorkspaceWork {
    let crate::live_work::Work {
        current,
        last_report,
        earlier_reports,
    } = work;
    let earlier = work_earlier_reports(earlier_reports);
    match current {
        crate::live_work::Current::Reported { summary } => {
            wire::WorkspaceWork::WorkReported(wire::WorkReported {
                state: serde_json::json!("reported"),
                repositories: summary
                    .repositories
                    .into_iter()
                    .map(work_repository)
                    .collect(),
                reported_at: summary.reported_at.to_string(),
                last_report: work_last_report(last_report),
                earlier_reports: earlier,
            })
        }
        crate::live_work::Current::NoInstance {
            branch,
            pull_request,
        } => wire::WorkspaceWork::WorkNoInstance(wire::WorkNoInstance {
            state: serde_json::json!("no_instance"),
            branch,
            pull_request,
            last_report: work_last_report(last_report),
            earlier_reports: earlier,
        }),
        crate::live_work::Current::NotAnswering { message } => {
            wire::WorkspaceWork::WorkNotAnswering(wire::WorkNotAnswering {
                state: serde_json::json!("not_answering"),
                message: message.to_owned(),
                last_report: work_last_report(last_report),
                earlier_reports: earlier,
            })
        }
    }
}

fn work_last_report(report: crate::live_work::LastReport) -> wire::WorkLastReport {
    match report {
        crate::live_work::LastReport::None => {
            wire::WorkLastReport::WorkNoReport(wire::WorkNoReport {
                report: serde_json::json!("none"),
            })
        }
        crate::live_work::LastReport::Received {
            instance,
            current_instance,
            summary,
        } => wire::WorkLastReport::WorkInstanceReport(work_instance_report(
            instance,
            current_instance,
            summary,
        )),
    }
}

/// Every earlier report is one an Instance sent: `live_work::read` collects them from stored rows.
fn work_earlier_reports(
    reports: Vec<crate::live_work::LastReport>,
) -> Vec<wire::WorkInstanceReport> {
    reports
        .into_iter()
        .filter_map(|report| match report {
            crate::live_work::LastReport::Received {
                instance,
                current_instance,
                summary,
            } => Some(work_instance_report(instance, current_instance, summary)),
            crate::live_work::LastReport::None => None,
        })
        .collect()
}

fn work_instance_report(
    instance: String,
    current_instance: bool,
    summary: crate::live_work::Summary,
) -> wire::WorkInstanceReport {
    wire::WorkInstanceReport {
        report: serde_json::json!("received"),
        instance,
        current_instance,
        repositories: summary
            .repositories
            .into_iter()
            .map(work_repository)
            .collect(),
        reported_at: summary.reported_at.to_string(),
    }
}

fn work_repository(repository: crate::live_work::Repository) -> wire::WorkRepository {
    let crate::live_work::Repository { repository, git } = repository;
    match git {
        crate::live_work::Git::Read {
            branch,
            changed,
            staged,
            committed,
            pushed,
            untracked,
            stashed,
        } => wire::WorkRepository::WorkRepositoryRead(wire::WorkRepositoryRead {
            repository,
            git: serde_json::json!("read"),
            branch,
            changed: work_changes(changed),
            staged: work_changes(staged),
            committed: work_commits(committed),
            pushed,
            untracked: wire_count(untracked),
            stashed: wire_count(stashed),
        }),
        crate::live_work::Git::Unreadable { because } => {
            wire::WorkRepository::WorkRepositoryUnreadable(wire::WorkRepositoryUnreadable {
                repository,
                git: serde_json::json!("unreadable"),
                because,
            })
        }
    }
}

fn work_changes(changes: crate::live_work::Changes) -> wire::WorkChanges {
    wire::WorkChanges {
        files: wire_count(changes.files),
        added: wire_count(changes.added),
        removed: wire_count(changes.removed),
    }
}

fn work_commits(commits: crate::live_work::Commits) -> wire::WorkCommits {
    wire::WorkCommits {
        commits: wire_count(commits.commits),
        added: wire_count(commits.added),
        removed: wire_count(commits.removed),
    }
}

fn wire_count(counted: u64) -> i64 {
    counted.try_into().expect("a count fits a wire integer")
}

pub(crate) async fn workspace_record(
    store: &Store,
    workspace: Workspace,
) -> anyhow::Result<wire::Workspace> {
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
        .map(pull_request_availability)
        .collect();
    let unfinished_session = workspace::unfinished(store, workspace.id)
        .await?
        .map(|session| wire::UnfinishedSession {
            id: session.id.to_string().parse().expect("a Session UUID"),
            name: session.name,
            state: unfinished_state(session.state),
            preparing: session.preparing.map(unfinished_preparing),
        });
    let held_messages = workspace::held_messages(store, workspace.id)
        .await?
        .into_iter()
        .map(held_message)
        .collect();

    Ok(wire::Workspace {
        id: workspace.id.to_string().parse().expect("a Workspace UUID"),
        name: workspace.name,
        organization: workspace.organization.name,
        project: workspace.project.name,
        opened_with: workspace.opened_with.name,
        profile: workspace.profile.map(|profile| profile.name),
        checkout: wire::Checkout {
            base: workspace.checkout.base,
            branch: workspace.checkout.branch,
            repositories: workspace.checkout.repositories,
        },
        integrations: Some(selected(workspace.integrations)),
        instance,
        held,
        correlation: workspace.correlation,
        state: match workspace.state {
            domain::WorkspaceState::Open => wire::WorkspaceState::Open,
            domain::WorkspaceState::Sealed => wire::WorkspaceState::Sealed,
        },
        opened_at: workspace.opened_at.to_string(),
        last_active_at: workspace.last_active_at.to_string(),
        sealed_at: workspace.sealed_at.map(|at| at.to_string()),
        continues: workspace
            .continues
            .map(|workspace| workspace.to_string().parse().expect("a Workspace UUID")),
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

pub(crate) fn session_record(
    session: Session,
    organization: &str,
    summaries: &crate::live_work::Summaries,
) -> wire::Session {
    let (observation, state) = summaries.observed(Some(&session));
    let trailing = session.state == domain::SessionState::Trailing;
    let harness = session.agent.harness.clone();
    let diagnostic = session
        .evidence
        .clone()
        .map(|evidence| diagnosed_failure(&session, organization, evidence));
    let supplied = session
        .supplied
        .iter()
        .map(|supplied| supplied_material(&harness, supplied))
        .collect();
    let options = session
        .options
        .into_iter()
        .map(|option| shown_option(option, &harness))
        .collect();
    wire::Session {
        id: session.id.to_string().parse().expect("a Session UUID"),
        name: session.name,
        workspace: session
            .workspace
            .to_string()
            .parse()
            .expect("a Workspace UUID"),
        state: session_state(session.state),
        preparing: session.preparing.map(session_preparing),
        exit: session.exit.map(session_exit),
        outcome_message: session.outcome_message,
        diagnostic,
        instance: session.instance,
        supervisor: session.supervisor,
        agent: session.agent.name,
        harness,
        model: session.agent.declared.model,
        mode: session.agent.declared.mode,
        thought_level: session.agent.declared.thought_level,
        worked_model: session.worked_model,
        sign_in_method: session.sign_in_method,
        supplied,
        title: session.title,
        options,
        changing_options: session
            .changing_options
            .into_iter()
            .map(|change| wire::ChangingOption {
                category: change.category,
                option: change.option,
                participant: change.participant,
                value: change.value,
            })
            .collect(),
        commands: session
            .commands
            .into_iter()
            .map(|command| wire::SessionCommand {
                name: command.name,
                description: command.description,
                input_hint: command.input_hint,
            })
            .collect(),
        interrupting: session.interrupting.map(|interrupt| wire::Interrupting {
            participant: interrupt.participant,
            requested_at: interrupt.requested_at.to_string(),
        }),
        enqueued_at: session.enqueued_at.to_string(),
        started_at: session.started_at.map(|at| at.to_string()),
        ended_at: session.ended_at.map(|at| at.to_string()),
        lease_expires_at: session.lease_expires_at.map(|at| at.to_string()),
        connected_at: session
            .connected
            .as_ref()
            .map(|connected| connected.at.to_string()),
        supervisor_version: session.connected.map(|connected| connected.version),
        // The live figure is the Turn in flight; the recorded one is the last Turn answered.
        usage: state.usage.or(session.usage).map(session_usage),
        depends_on: session
            .depends_on
            .into_iter()
            .map(|blocker| wire::SessionReference {
                id: blocker.id.to_string().parse().expect("a Session UUID"),
                name: blocker.name,
            })
            .collect(),
        tools: state.tools.into_iter().map(running_tool).collect(),
        units: state.units.into_iter().map(running_unit).collect(),
        message_buffering: state.message_buffering,
        thought_buffering: state.thought_buffering,
        last_activity_at: state
            .last_activity_at
            .filter(|_| trailing)
            .map(|at| at.to_string()),
        observation: session_observation(observation),
    }
}

impl From<Trigger> for wire::Trigger {
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
        let correlation = trigger.templates.correlation;

        Self {
            id: trigger.id.to_string().parse().expect("a Trigger UUID"),
            organization: trigger.organization.name,
            name: trigger.name,
            state: match trigger.state {
                TriggerState::Enabled => wire::TriggerState::Enabled,
                TriggerState::Disabled(DisableReason::Operator) => {
                    wire::TriggerState::DisabledOperator
                }
                TriggerState::Disabled(DisableReason::FiringBudget) => {
                    wire::TriggerState::DisabledFiringBudget
                }
            },
            disabled_because: trigger.disabled_because,
            firing_budget: wire::TriggerFiringBudget {
                limit: i64::try_from(trigger.firing_budget.limit.get())
                    .expect("a firing budget's limit fits the wire"),
                window: format!("{:#}", trigger.firing_budget.window),
            },
            filter,
            every,
            cron,
            zone,
            admits_outsiders,
            brief: trigger.templates.brief.to_string(),
            branch: trigger.templates.branch.map(|branch| branch.to_string()),
            correlation: correlation.template().map(ToString::to_string),
            on_miss: correlation.on_miss().map(|miss| match miss {
                CorrelationMiss::Open => wire::TriggerOnMiss::Open,
                CorrelationMiss::Ignore => wire::TriggerOnMiss::Ignore,
            }),
            on_open_workspace: match correlation.on_open_workspace() {
                OnOpenWorkspace::Continue => wire::TriggerOnOpenWorkspace::Continue_,
                OnOpenWorkspace::NewSession => wire::TriggerOnOpenWorkspace::NewSession,
            },
            project: trigger.project.name,
            agent: trigger.agent.name,
            allows: trigger.allows.into_iter().map(|agent| agent.name).collect(),
            profile: trigger.profile.map(|profile| profile.name),
            model: trigger.declared.model,
            mode: trigger.declared.mode,
            thought_level: trigger.declared.thought_level,
            applied: trigger.applied,
            declared_at: trigger.declared_at.to_string(),
        }
    }
}

async fn start(
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

async fn resolve_repositories(
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

async fn organizations(
    State(control_plane): State<ControlPlane>,
) -> Result<Json<wire::ListOrganizationsResponse>, Refused> {
    let mut tx = control_plane.store.begin().await?;
    let organizations = tx.organizations().all().await?;

    Ok(Json(organizations.into_iter().map(Into::into).collect()))
}

async fn declare_organization(
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

async fn projects(
    State(control_plane): State<ControlPlane>,
    Path(organization): Path<String>,
) -> Result<Json<wire::ListProjectsResponse>, Refused> {
    let mut tx = control_plane.store.begin().await?;
    let organization = tx.organizations().named(&organization).await?;
    let projects = tx.projects().all(&organization).await?;

    Ok(Json(projects.into_iter().map(Into::into).collect()))
}

async fn declare_project(
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

async fn agents(
    State(control_plane): State<ControlPlane>,
    Path(organization): Path<String>,
) -> Result<Json<wire::ListAgentsResponse>, Refused> {
    let agents = agent::agents(&control_plane.store, &organization).await?;

    Ok(Json(agents.into_iter().map(Into::into).collect()))
}

async fn set_agent_model(
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

async fn declare_agent(
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

async fn apply_declaration(
    State(control_plane): State<ControlPlane>,
    Path(organization): Path<String>,
    declaration: Result<Json<serde_json::Value>, JsonRejection>,
) -> Result<Json<wire::AppliedDeclaration>, Refused> {
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
    declaration: Result<Json<serde_json::Value>, JsonRejection>,
) -> Result<Json<wire::AppliedDeclaration>, Refused> {
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
    declaration: Result<Json<serde_json::Value>, JsonRejection>,
    mode: declaration::ApplyMode,
) -> Result<Json<wire::AppliedDeclaration>, Refused> {
    let Json(sent) = declaration?;
    let declaration = document(closed(&sent).map_err(|why| malformed(None, why))?)?;
    let applied = declaration::apply(
        &control_plane.store,
        &control_plane.images,
        &Github::unfenced()?,
        &organization,
        &declaration,
        mode,
    )
    .await
    .map_err(named_refusal)?;

    Ok(Json(wire::AppliedDeclaration {
        declarations: applied.declarations.into_iter().map(change).collect(),
        admitting_outsiders: applied.admitting_outsiders,
    }))
}

/// A generated type reads past a field it has no place for, so a body whose schema the
/// document closes is read here, where one is refused by its path.
fn closed<T: DeserializeOwned>(sent: &serde_json::Value) -> Result<T, String> {
    let mut stray = None;
    let mut ignored = |path: serde_ignored::Path<'_>| {
        stray.get_or_insert_with(|| path.to_string());
    };
    let read =
        serde_path_to_error::deserialize(serde_ignored::Deserializer::new(sent, &mut ignored));
    match stray {
        Some(field) => Err(format!("unknown field `{field}`")),
        None => read.map_err(|error| error.to_string()),
    }
}

fn project_declaration(declaration: wire::ProjectDeclaration) -> declaration::Project {
    declaration::Project {
        name: declaration.name,
        repositories: declaration.repositories,
        integrations: declaration.integrations.unwrap_or_default(),
        branch: declaration.branch,
    }
}

/// The document's trigger fires on a filter alone, which the generated type cannot say.
fn document(document: wire::DeclarationDocument) -> Result<declaration::Document, Refused> {
    let trigger = document.trigger;
    for (field, schedule) in [
        ("trigger.every", &trigger.every),
        ("trigger.cron", &trigger.cron),
        ("trigger.zone", &trigger.zone),
    ] {
        if schedule.is_some() {
            return Err(malformed(
                Some(field),
                "a declaration document's trigger fires on a filter, not a schedule",
            ));
        }
    }

    Ok(declaration::Document {
        project: project_declaration(document.project),
        agent: declaration::Agent {
            name: document.agent.name,
            harness: document.agent.harness,
            model: document.agent.model.flatten(),
            mode: document.agent.mode.flatten(),
            thought_level: document.agent.thought_level.flatten(),
        },
        trigger: declaration::TriggerDeclaration {
            name: trigger.name,
            filter: trigger.filter.ok_or_else(|| {
                malformed(
                    Some("trigger.filter"),
                    "a declaration document's trigger declares its filter",
                )
            })?,
            brief: trigger.brief,
            branch: trigger.branch.flatten(),
            correlation: trigger.correlation.flatten(),
            on_miss: trigger.on_miss.flatten(),
            on_open_workspace: trigger.on_open_workspace.flatten(),
            project: trigger.project,
            agent: trigger.agent,
            allows: trigger.allows.unwrap_or_default(),
            profile: trigger.profile.flatten(),
            model: trigger.model.flatten(),
            mode: trigger.mode.flatten(),
            thought_level: trigger.thought_level.flatten(),
        },
    })
}

/// The generated file flattens its entries out of serde's sight, so each is read alone first
/// for a refusal to name the entry it is in.
fn trigger_file(sent: &serde_json::Value) -> Result<triggers::File, Refused> {
    let entries = sent.get("triggers").and_then(serde_json::Value::as_object);
    for (name, entry) in entries.into_iter().flatten() {
        closed::<wire::TriggerFileEntry>(entry)
            .map_err(|why| malformed(None, format!("triggers.{name}: {why}")))?;
    }
    let file: wire::TriggerFile = closed(sent).map_err(|why| malformed(None, why))?;

    Ok(triggers::File {
        triggers: file
            .triggers
            .additional_properties
            .into_iter()
            .map(|(name, entry)| {
                (
                    name,
                    triggers::Entry {
                        filter: entry.filter,
                        brief: entry.brief,
                        branch: entry.branch.flatten(),
                        correlation: entry.correlation.flatten(),
                        on_miss: entry.on_miss.flatten(),
                        on_open_workspace: entry.on_open_workspace.flatten(),
                        project: entry.project,
                        agent: entry.agent,
                        allows: entry.allows.unwrap_or_default(),
                        profile: entry.profile.flatten(),
                        model: entry.model.flatten(),
                        mode: entry.mode.flatten(),
                        thought_level: entry.thought_level.flatten(),
                    },
                )
            })
            .collect(),
    })
}

fn change(change: declaration::Declaration) -> wire::DeclarationChange {
    wire::DeclarationChange {
        kind: match change.kind {
            declaration::Kind::Project => wire::DeclarationChangeKind::Project,
            declaration::Kind::Agent => wire::DeclarationChangeKind::Agent,
            declaration::Kind::Trigger => wire::DeclarationChangeKind::Trigger,
        },
        name: change.name,
        action: match change.action {
            declaration::Action::Add => wire::DeclarationChangeAction::Add,
            declaration::Action::Change => wire::DeclarationChangeAction::Change,
            declaration::Action::Unchanged => wire::DeclarationChangeAction::Unchanged,
            declaration::Action::Remove => wire::DeclarationChangeAction::Remove,
        },
        differences: differences(change.differences),
    }
}

fn differences(differences: Vec<declaration::Difference>) -> Vec<wire::DeclarationDifference> {
    differences
        .into_iter()
        .map(|difference| wire::DeclarationDifference {
            field: difference.field.to_owned(),
            was: difference.was,
            becomes: difference.becomes,
        })
        .collect()
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

impl From<SubscriptionProfile> for wire::SubscriptionProfile {
    fn from(profile: SubscriptionProfile) -> Self {
        Self {
            id: profile.id.to_string().parse().expect("a Profile UUID"),
            name: profile.name,
            owner: profile.owner,
            owner_operator: profile
                .owner_operator
                .map(|id| id.to_string().parse().expect("an Operator UUID")),
        }
    }
}

impl From<profile::Held> for wire::SubscriptionProfileLogin {
    fn from(held: profile::Held) -> Self {
        Self {
            kind: match held.entry.kind {
                profile::Kind::Variable => wire::SubscriptionProfileLoginKind::Variable,
                profile::Kind::File => wire::SubscriptionProfileLoginKind::File,
            },
            name: held.entry.name,
            set_at: held.set_at.to_string(),
        }
    }
}

async fn profiles(
    State(control_plane): State<ControlPlane>,
    Path(organization): Path<String>,
) -> Result<Json<wire::ListSubscriptionProfilesResponse>, Refused> {
    let listed = profile::profiles(&control_plane.store, &organization).await?;

    Ok(Json(
        listed
            .into_iter()
            .map(|(profile, held)| {
                let profile = wire::SubscriptionProfile::from(profile);
                wire::SubscriptionProfileListed {
                    id: profile.id,
                    name: profile.name,
                    owner: profile.owner,
                    owner_operator: profile.owner_operator,
                    holds: held.into_iter().map(Into::into).collect(),
                }
            })
            .collect(),
    ))
}

async fn declare_profile(
    State(control_plane): State<ControlPlane>,
    Path(organization): Path<String>,
    declaration: Result<Json<wire::SubscriptionProfileDeclaration>, JsonRejection>,
) -> Result<Response, Refused> {
    let Json(declaration) = declaration?;
    named("declare_subscription_profile", &declaration.name)?;

    let declared = profile::declare(
        &control_plane.store,
        &organization,
        &declaration.name,
        declaration.owner.as_deref(),
    )
    .await?;

    Ok(answered::<_, wire::SubscriptionProfile>(declared))
}

async fn hold_profile_variable(
    State(control_plane): State<ControlPlane>,
    Path((organization, profile, variable)): Path<(String, String, String)>,
    secret: Result<Json<Secret>, JsonRejection>,
) -> Result<Json<wire::SubscriptionProfileLogin>, Refused> {
    let entry = Entry::variable(&variable)?;
    held_in_profile(&control_plane, &organization, &profile, &entry, secret).await
}

async fn hold_profile_file(
    State(control_plane): State<ControlPlane>,
    Path((organization, profile, path)): Path<(String, String, String)>,
    secret: Result<Json<Secret>, JsonRejection>,
) -> Result<Json<wire::SubscriptionProfileLogin>, Refused> {
    let entry = Entry::file(&path)?;
    held_in_profile(&control_plane, &organization, &profile, &entry, secret).await
}

async fn held_in_profile(
    control_plane: &ControlPlane,
    organization: &str,
    profile: &str,
    entry: &Entry,
    secret: Result<Json<Secret>, JsonRejection>,
) -> Result<Json<wire::SubscriptionProfileLogin>, Refused> {
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

impl From<sign_in::SignIn> for wire::SavedSignIn {
    fn from(saved: sign_in::SignIn) -> Self {
        let authentication = saved.authentication;
        Self {
            harness: saved.harness,
            method: saved.method.id.clone(),
            kind: match saved.method.kind {
                wire::SignInMethodKind::Subscription => wire::SavedSignInKind::Subscription,
                wire::SignInMethodKind::Key => wire::SavedSignInKind::Key,
            },
            holding: match saved.holding {
                sign_in::Holding::Profile(profile) => wire::SignInHolding {
                    resource: wire::SignInHoldingResource::SubscriptionProfile,
                    reference: profile.name,
                    fills: saved.method.fills.clone(),
                },
                sign_in::Holding::ProviderCredential(variable) => wire::SignInHolding {
                    resource: wire::SignInHoldingResource::ProviderCredential,
                    reference: variable,
                    fills: saved.method.fills.clone(),
                },
            },
            revision: saved.revision,
            refreshed_from: saved.refreshed_from,
            saved_at: saved.saved_at.to_string(),
            authentication: wire::AuthenticationEvidence {
                state: match authentication.state {
                    sign_in::State::Unchecked => wire::AuthenticationEvidenceState::Unchecked,
                    sign_in::State::LoginCompleted => {
                        wire::AuthenticationEvidenceState::LoginCompleted
                    }
                    sign_in::State::CredentialAccepted => {
                        wire::AuthenticationEvidenceState::CredentialAccepted
                    }
                    sign_in::State::AuthenticationFailed => {
                        wire::AuthenticationEvidenceState::AuthenticationFailed
                    }
                    sign_in::State::Expired => wire::AuthenticationEvidenceState::Expired,
                    sign_in::State::NotCovered => wire::AuthenticationEvidenceState::NotCovered,
                },
                source: match authentication.source {
                    sign_in::Source::Import => wire::AuthenticationEvidenceSource::Import,
                    sign_in::Source::ProviderCheck => {
                        wire::AuthenticationEvidenceSource::ProviderCheck
                    }
                    sign_in::Source::Relay => wire::AuthenticationEvidenceSource::Relay,
                    sign_in::Source::GenericWrite => {
                        wire::AuthenticationEvidenceSource::GenericWrite
                    }
                    sign_in::Source::Refresh => wire::AuthenticationEvidenceSource::Refresh,
                    sign_in::Source::Session => wire::AuthenticationEvidenceSource::Session,
                    sign_in::Source::ModelTest => wire::AuthenticationEvidenceSource::ModelTest,
                },
                observed_at: authentication.observed_at.to_string(),
                provider_check: authentication
                    .provider_check
                    .map(|checked| wire::ProviderCheck {
                        provider: match checked.provider {
                            sign_in::check::Provider::Anthropic => {
                                wire::ProviderCheckProvider::Anthropic
                            }
                            sign_in::check::Provider::OpenAi => wire::ProviderCheckProvider::Openai,
                        },
                        outcome: match checked.outcome {
                            sign_in::check::Outcome::Accepted => {
                                wire::ProviderCheckOutcome::Accepted
                            }
                            sign_in::check::Outcome::Rejected => {
                                wire::ProviderCheckOutcome::Rejected
                            }
                            sign_in::check::Outcome::Inconclusive => {
                                wire::ProviderCheckOutcome::Inconclusive
                            }
                            sign_in::check::Outcome::Unavailable => {
                                wire::ProviderCheckOutcome::Unavailable
                            }
                        },
                        status: checked.status.map(i64::from),
                        provider_error: checked.provider_error,
                    }),
            },
            model_use: saved
                .model_use
                .into_iter()
                .map(|used| wire::ModelUseEvidence {
                    harness: used.harness,
                    model: used.model,
                    image: used.image,
                    result: match used.result {
                        sign_in::UseResult::Worked => wire::ModelUseEvidenceResult::Worked,
                        sign_in::UseResult::AuthenticationFailed => {
                            wire::ModelUseEvidenceResult::AuthenticationFailed
                        }
                        sign_in::UseResult::NotCovered => wire::ModelUseEvidenceResult::NotCovered,
                    },
                    source: match used.source {
                        sign_in::UseSource::ModelTest => wire::ModelUseEvidenceSource::ModelTest,
                        sign_in::UseSource::Session => wire::ModelUseEvidenceSource::Session,
                    },
                    observed_at: used.observed_at.to_string(),
                })
                .collect(),
        }
    }
}

async fn sign_ins(
    State(control_plane): State<ControlPlane>,
    Path(organization): Path<String>,
) -> Result<Json<Vec<wire::SavedSignIn>>, Refused> {
    let saved = sign_in::saved(&control_plane.store, &organization).await?;

    Ok(Json(saved.into_iter().map(Into::into).collect()))
}

async fn save_sign_in(
    State(control_plane): State<ControlPlane>,
    Path((organization, harness, method)): Path<(String, String, String)>,
    material: Result<Json<wire::SignInMaterial>, JsonRejection>,
) -> Result<Json<wire::SavedSignIn>, Refused> {
    let Json(material) = material?;
    let saved = sign_in::save(
        &control_plane.store,
        &control_plane.providers,
        &organization,
        &harness,
        &method,
        &material.value,
    )
    .await?;

    let Some(provider) = saved.unavailable else {
        return Ok(Json(saved.sign_in.into()));
    };
    let holding = inspect(
        match &saved.sign_in.holding {
            sign_in::Holding::Profile(profile) => {
                Locator::new(Resource::SubscriptionProfile, profile.name.clone())
            }
            sign_in::Holding::ProviderCredential(variable) => {
                Locator::new(Resource::ProviderCredential, variable.clone())
            }
        },
        Some(organization),
    );
    Err(Refused::Diagnosed {
        status: StatusCode::SERVICE_UNAVAILABLE,
        diagnostic: Box::new(wire::Diagnostic::UnavailableDiagnostic(
            wire::UnavailableDiagnostic {
                kind: serde_json::json!("unavailable"),
                message: format!(
                    "{} could not be asked whether it accepts this key, so it was saved unchecked",
                    provider.as_str()
                ),
                field: None,
                context: wire::UnavailableContext {
                    service: provider.as_str().to_owned(),
                    resource: None,
                    operation: sign_in::SAVE.to_owned(),
                    retry_after_seconds: None,
                    saved: Some(saved.sign_in.into()),
                },
                next_steps: vec![holding],
            },
        )),
    })
}

impl From<sign_in::model_test::Test> for wire::SignInTest {
    fn from(test: sign_in::model_test::Test) -> Self {
        Self {
            id: test.id,
            harness: test.harness,
            method: test.method,
            revision: test.revision,
            requested_model: test.requested_model,
            model: test.model,
            image: test.image,
            state: match test.state {
                sign_in::TestState::Pending => wire::SignInTestState::Pending,
                sign_in::TestState::Running => wire::SignInTestState::Running,
                sign_in::TestState::Completed => wire::SignInTestState::Completed,
                sign_in::TestState::Failed => wire::SignInTestState::Failed,
                sign_in::TestState::Interrupted => wire::SignInTestState::Interrupted,
            },
            outcome: test.outcome.map(|outcome| match outcome {
                sign_in::TestOutcome::Worked => wire::SignInTestOutcome::Worked,
                sign_in::TestOutcome::AuthenticationFailed => {
                    wire::SignInTestOutcome::AuthenticationFailed
                }
                sign_in::TestOutcome::NotCovered => wire::SignInTestOutcome::NotCovered,
                sign_in::TestOutcome::Unavailable => wire::SignInTestOutcome::Unavailable,
                sign_in::TestOutcome::HarnessFailed => wire::SignInTestOutcome::HarnessFailed,
                sign_in::TestOutcome::MaterialReplaced => wire::SignInTestOutcome::MaterialReplaced,
            }),
            message: test.message,
            created_at: test.created_at.to_string(),
            started_at: test.started_at.map(|at| at.to_string()),
            finished_at: test.finished_at.map(|at| at.to_string()),
        }
    }
}

/// A body is optional: no model named tests the harness's own default.
async fn test_sign_in(
    State(control_plane): State<ControlPlane>,
    Path((organization, harness, method)): Path<(String, String, String)>,
    body: axum::body::Bytes,
) -> Result<(StatusCode, Json<wire::SignInTest>), Refused> {
    let asked: wire::SignInTestRequest = if body.is_empty() {
        wire::SignInTestRequest::default()
    } else {
        serde_json::from_slice(&body).map_err(|error| malformed(None, error.to_string()))?
    };
    let test = sign_in::model_test::start(
        &control_plane.store,
        &control_plane.images,
        &organization,
        &harness,
        &method,
        asked.model.as_deref(),
    )
    .await?;

    Ok((StatusCode::ACCEPTED, Json(test.into())))
}

async fn sign_in_test(
    State(control_plane): State<ControlPlane>,
    Path((organization, harness, method, test)): Path<(String, String, String, String)>,
) -> Result<Json<wire::SignInTest>, Refused> {
    let test = sign_in::model_test::read(
        &control_plane.store,
        &organization,
        &harness,
        &method,
        &test,
    )
    .await?;

    Ok(Json(test.into()))
}

#[derive(Deserialize)]
struct GithubAppQuery {
    state: String,
    code: Option<String>,
    installation_id: Option<i64>,
    page: Option<u32>,
}

async fn start_github_app(
    State(control_plane): State<ControlPlane>,
    Path(organization): Path<String>,
    registration: Result<Json<wire::GithubAppStart>, JsonRejection>,
) -> Result<(StatusCode, Json<wire::GithubAppStarted>), Refused> {
    let Json(registration) = registration?;
    named("start_integration_manifest", &registration.name)?;
    Ok((
        StatusCode::CREATED,
        Json(
            integration::manifest::start(
                &control_plane.store,
                &organization,
                registration,
                &control_plane.github_app_api,
            )
            .await?,
        ),
    ))
}

fn setup_page(html: String) -> Response {
    const POLICY: &str = "default-src 'none'; form-action 'self' https://github.com; frame-ancestors 'none'; base-uri 'none'";
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
    let github = Github::unfenced()?;
    if let Some(installation) = query.installation_id {
        integration::manifest::installed(&cp.store, &github, &query.state, installation).await?;
    }
    Ok(setup_page(
        integration::manifest::page(&cp.store, &github, &query.state, query.page).await?,
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
        integration::manifest::callback(&cp.store, &Github::unfenced()?, &query.state, &code)
            .await?,
    ))
}

async fn github_app_installed(
    State(cp): State<ControlPlane>,
    Query(query): Query<GithubAppQuery>,
) -> Result<Response, Refused> {
    let Some(installation) = query.installation_id else {
        return Ok(setup_page(
            integration::manifest::discovery_page(
                &cp.store,
                &Github::unfenced()?,
                &query.state,
                query.page,
            )
            .await?,
        ));
    };
    let status = integration::manifest::installed(
        &cp.store,
        &Github::unfenced()?,
        &query.state,
        installation,
    )
    .await?;
    let mut client = cp
        .client
        .join("/setup")
        .map_err(|_| Refused::Unprocessable("The Client URL cannot return to setup".into()))?;
    client
        .query_pairs_mut()
        .append_pair("organization", &status.organization)
        .append_pair("github_app_flow", &query.state);
    let mut response = setup_page(format!(
        "<!doctype html><title>Choose a repository</title><h1>The App is installed</h1><p><a href=\"{}?state={}\">Choose one repository here</a> or return to the Client.</p>",
        integration::manifest::PAGE,
        query.state
    ));
    *response.status_mut() = StatusCode::SEE_OTHER;
    response.headers_mut().insert(
        "location",
        client
            .as_str()
            .parse()
            .map_err(|_| Refused::Unprocessable("The Client URL is invalid".into()))?,
    );
    Ok(response)
}

#[derive(Deserialize)]
struct GithubAppPages {
    page: Option<u32>,
    per_page: Option<u32>,
}

async fn github_app_status(
    State(cp): State<ControlPlane>,
    Path(flow): Path<String>,
) -> Result<Json<wire::GithubAppFlow>, Refused> {
    Ok(Json(integration::manifest::status(&cp.store, &flow).await?))
}

async fn github_app_cancel(
    State(cp): State<ControlPlane>,
    Path(flow): Path<String>,
) -> Result<Json<wire::GithubAppFlow>, Refused> {
    Ok(Json(integration::manifest::cancel(&cp.store, &flow).await?))
}

async fn github_app_installations(
    State(cp): State<ControlPlane>,
    Path(flow): Path<String>,
    Query(pages): Query<GithubAppPages>,
) -> Result<Json<wire::GithubAppInstallations>, Refused> {
    Ok(Json(
        integration::manifest::installations(
            &cp.store,
            &Github::unfenced()?,
            &flow,
            pages.page,
            pages.per_page,
        )
        .await?,
    ))
}

async fn github_app_repositories(
    State(cp): State<ControlPlane>,
    Path(flow): Path<String>,
    Query(pages): Query<GithubAppPages>,
) -> Result<Json<wire::GithubAppRepositories>, Refused> {
    Ok(Json(
        integration::manifest::repositories(
            &cp.store,
            &Github::unfenced()?,
            &flow,
            pages.page,
            pages.per_page,
        )
        .await?,
    ))
}

async fn github_app_finish(
    State(cp): State<ControlPlane>,
    Path(flow): Path<String>,
    request: Result<Json<wire::GithubAppFinish>, JsonRejection>,
) -> Result<Json<wire::GithubAppFlow>, Refused> {
    let Json(request) = request?;
    Ok(Json(
        integration::manifest::finish(&cp.store, &Github::unfenced()?, &flow, &request.repository)
            .await?,
    ))
}

async fn github_app_setup_finish(
    State(cp): State<ControlPlane>,
    Query(query): Query<GithubAppQuery>,
    Form(request): Form<wire::GithubAppFinish>,
) -> Result<Response, Refused> {
    integration::manifest::finish(
        &cp.store,
        &Github::unfenced()?,
        &query.state,
        &request.repository,
    )
    .await?;
    Ok(setup_page(
        integration::manifest::page(&cp.store, &Github::unfenced()?, &query.state, None).await?,
    ))
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
fn integration_record(integration: Integration, organization: &str) -> wire::Integration {
    let diagnostic = (!integration.enabled()).then(|| {
        diagnosis(
            integration::out_of_use(&integration, organization, "use_integration"),
            None,
        )
        .1
    });
    let webhook_path = integration.webhook_path();
    let kind = match integration.kind() {
        IntegrationKind::Github => wire::IntegrationKind::Github,
        IntegrationKind::Webhook => wire::IntegrationKind::Webhook,
    };
    let received = integration.carries(Direction::Inbound) && !integration.retired();
    let (repository, bot_login, polled_every, webhook_path) = match integration.connection {
        Connection::Github(github) if !received => {
            (Some(github.repository), Some(github.bot_login), None, None)
        }
        Connection::Github(github) => (
            Some(github.repository),
            Some(github.bot_login),
            Some(format!("{:#}", github.interval)),
            Some(webhook_path),
        ),
        Connection::Webhook => (None, None, None, received.then_some(webhook_path)),
    };

    wire::Integration {
        id: integration
            .id
            .to_string()
            .parse()
            .expect("an Integration UUID"),
        name: integration.name,
        kind,
        repository,
        bot_login,
        carries: integration
            .carries
            .into_iter()
            .map(wire_direction)
            .collect(),
        state: match integration.state {
            IntegrationState::Enabled => wire::IntegrationState::Enabled,
            IntegrationState::Disabled => wire::IntegrationState::Disabled,
            IntegrationState::Retired => wire::IntegrationState::Retired,
        },
        revision: integration.revision,
        disabled_at: integration.disabled_at.map(|at| at.to_string()),
        retired_at: integration.retired_at.map(|at| at.to_string()),
        canceled_posts: integration
            .canceled_posts
            .into_iter()
            .map(|post| wire::CanceledPost {
                session: post.session.to_string().parse().expect("a Session UUID"),
                turn: post.turn,
                canceled_at: post.canceled_at.to_string(),
                reason: post.because,
                attempted_at: post.attempted_at.map(|at| at.to_string()),
            })
            .collect(),
        polled_every,
        webhook_path,
        last_event_refusal: integration
            .last_event_refusal
            .map(|refusal| wire::EventRefusal {
                source: refusal.source,
                id: refusal.id,
                bytes: refusal
                    .bytes
                    .map(|bytes| i64::try_from(bytes).unwrap_or(i64::MAX)),
                reason: refusal.reason,
                observed_at: refusal.observed_at.to_string(),
            }),
        diagnostic,
    }
}

const fn wire_direction(direction: Direction) -> wire::Direction {
    match direction {
        Direction::Inbound => wire::Direction::Inbound,
        Direction::Outbound => wire::Direction::Outbound,
    }
}

const fn domain_direction(direction: &wire::Direction) -> Direction {
    match direction {
        wire::Direction::Inbound => Direction::Inbound,
        wire::Direction::Outbound => Direction::Outbound,
    }
}

async fn integrations(
    State(control_plane): State<ControlPlane>,
    Path(organization): Path<String>,
) -> Result<Json<Vec<wire::Integration>>, Refused> {
    let integrations = integration::integrations(&control_plane.store, &organization).await?;

    Ok(Json(
        integrations
            .into_iter()
            .map(|integration| integration_record(integration, &organization))
            .collect(),
    ))
}

async fn show_integration(
    State(control_plane): State<ControlPlane>,
    Path((organization, name)): Path<(String, String)>,
) -> Result<Json<wire::Integration>, Refused> {
    let integration = integration::integration(&control_plane.store, &organization, &name).await?;

    Ok(Json(integration_record(integration, &organization)))
}

/// What identifies the connection; naming one in a change is refused rather than ignored.
const FIXED: [&str; 7] = [
    "organization",
    "kind",
    "api",
    "repository",
    "app_id",
    "installation",
    "id",
];

async fn change_integration(
    State(control_plane): State<ControlPlane>,
    Path((organization, name)): Path<(String, String)>,
    change: Result<Json<serde_json::Value>, JsonRejection>,
) -> Result<Json<wire::Integration>, Refused> {
    let Json(change) = change?;
    if let Some(fixed) = FIXED.into_iter().find(|field| change.get(field).is_some()) {
        return Err(invalid_field(
            "change_integration",
            fixed,
            Constraint::Immutable,
            format!(
                "an integration's {fixed} identifies it and cannot change; register a new \
                 integration for a different one"
            ),
        ));
    }
    let change: wire::IntegrationChange =
        serde_json::from_value(change).map_err(|error| Refused::Malformed {
            field: None,
            why: error.to_string(),
        })?;
    if let Some(name) = &change.name {
        named("change_integration", name)?;
    }
    let interval = change
        .interval
        .as_deref()
        .map(str::parse::<SignedDuration>)
        .transpose()
        .map_err(|error| {
            invalid_field(
                "change_integration",
                "interval",
                Constraint::Positive,
                format!("an interval is a duration: {error}"),
            )
        })?;
    let carries: Option<Vec<Direction>> = change
        .carries
        .as_ref()
        .map(|carries| carries.iter().map(domain_direction).collect());
    let changed = integration::change(
        &control_plane.store,
        &organization,
        &name,
        integration::Change {
            name: change.name.as_deref(),
            carries: carries.as_deref(),
            interval,
            revision: change.revision,
        },
    )
    .await?;

    Ok(Json(integration_record(changed, &organization)))
}

#[derive(Deserialize)]
struct RepositoriesPage {
    page: Option<u32>,
    per_page: Option<u32>,
}

async fn integration_repositories(
    State(control_plane): State<ControlPlane>,
    Path((organization, name)): Path<(String, String)>,
    Query(asked): Query<RepositoriesPage>,
) -> Result<Json<wire::IntegrationRepositories>, Refused> {
    let reached = integration::repositories(
        &control_plane.store,
        &Github::unfenced()?,
        &organization,
        &name,
        asked.page,
        asked.per_page,
    )
    .await?;

    Ok(Json(wire::IntegrationRepositories {
        next_page: reached.next_page.map(i64::from),
        page: i64::from(reached.page),
        total: i64::try_from(reached.installed.total_count).unwrap_or(i64::MAX),
        repositories: reached
            .installed
            .repositories
            .into_iter()
            .map(|repository| wire::IntegrationRepository {
                address: github::https_address(&repository.full_name),
                watched: repository.full_name.eq_ignore_ascii_case(&reached.watched),
                repository: repository.full_name,
                default_branch: repository.default_branch,
                private: repository.private,
            })
            .collect(),
    }))
}

async fn disable_integration(
    State(control_plane): State<ControlPlane>,
    Path((organization, name)): Path<(String, String)>,
) -> Result<Json<wire::Integration>, Refused> {
    let disabled = integration::disable(&control_plane.store, &organization, &name).await?;

    Ok(Json(integration_record(disabled, &organization)))
}

async fn enable_integration(
    State(control_plane): State<ControlPlane>,
    Path((organization, name)): Path<(String, String)>,
) -> Result<Json<wire::Integration>, Refused> {
    let enabled = integration::enable(&control_plane.store, &organization, &name).await?;

    Ok(Json(integration_record(enabled, &organization)))
}

async fn retire_integration(
    State(control_plane): State<ControlPlane>,
    Path((organization, name)): Path<(String, String)>,
    retirement: Result<Json<wire::IntegrationRetirement>, JsonRejection>,
) -> Result<Json<wire::Integration>, Refused> {
    let Json(retirement) = retirement?;
    let retired = integration::retire(
        &control_plane.store,
        &organization,
        &name,
        retirement.revision,
    )
    .await?;

    Ok(Json(integration_record(retired, &organization)))
}

async fn replace_integration_private_key(
    State(control_plane): State<ControlPlane>,
    Path((organization, name)): Path<(String, String)>,
    replacement: Result<Json<wire::IntegrationPrivateKeyReplacement>, JsonRejection>,
) -> Result<Json<wire::Integration>, Refused> {
    let Json(replacement) = replacement?;
    let replaced = integration::replace_private_key(
        &control_plane.store,
        &Github::unfenced()?,
        &organization,
        &name,
        &replacement.private_key,
        replacement.revision,
    )
    .await?;

    Ok(Json(integration_record(replaced, &organization)))
}

async fn register_integration(
    State(control_plane): State<ControlPlane>,
    Path(organization): Path<String>,
    registration: Result<Json<IntegrationRegistration>, JsonRejection>,
) -> Result<(StatusCode, Json<wire::Integration>), Refused> {
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
    let github = Github::unfenced()?;
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

    Ok((
        StatusCode::CREATED,
        Json(integration_record(registered, &organization)),
    ))
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

async fn event_record(store: &Store, event: domain::Event) -> Result<wire::EventRecord, Refused> {
    let mut firings = Vec::new();
    for firing in trigger::firings(store, event.record_id).await? {
        firings.push(wire::Firing {
            outcome: match firing.outcome.as_str() {
                "opened" => wire::FiringOutcome::Opened,
                "fed" => wire::FiringOutcome::Fed,
                "ignored" => wire::FiringOutcome::Ignored,
                "failed" => wire::FiringOutcome::Failed,
                "held" => wire::FiringOutcome::Held,
                "canceled" => wire::FiringOutcome::Canceled,
                outcome => return Err(anyhow::anyhow!("a firing ended {outcome}").into()),
            },
            trigger: firing.trigger,
            workspace: firing.workspace.map(|workspace| wire_id(&workspace)),
            failure: firing.failure,
            worked_ahead: firing.worked_ahead,
        });
    }

    Ok(wire::EventRecord {
        record: wire_id(&event.record_id),
        organization: wire_id(&event.organization),
        integration: event.integration.map(|integration| wire_id(&integration)),
        recorded_at: event.recorded_at.to_string(),
        event: wire::CloudEvent {
            id: event.occurrence.id,
            source: event.occurrence.source,
            specversion: event.occurrence.specversion,
            r#type: event.occurrence.r#type,
            subject: event.occurrence.subject,
            time: event.occurrence.time.to_string(),
            data: event.occurrence.data,
        },
        firings,
    })
}

fn wire_id(id: &impl ToString) -> uuid::Uuid {
    id.to_string().parse().expect("an identifier is a UUID")
}

async fn events(
    State(control_plane): State<ControlPlane>,
    Path(organization): Path<String>,
    Query(limited): Query<Limited>,
) -> Result<Json<wire::ListEventsResponse>, Refused> {
    let events = integration::events(
        &control_plane.store,
        &organization,
        limited.limit.unwrap_or(EVENTS_LISTED),
    )
    .await?;
    let mut records = Vec::new();
    for event in events {
        records.push(event_record(&control_plane.store, event).await?);
    }

    Ok(Json(records))
}

async fn event(
    State(control_plane): State<ControlPlane>,
    Path(record): Path<String>,
) -> Result<Json<wire::EventRecord>, Refused> {
    let record: EventRecordId = record.parse().map_err(|_| no_event(&record, None))?;
    let event = integration::event(&control_plane.store, record).await?;

    Ok(Json(event_record(&control_plane.store, event).await?))
}

async fn triggers(
    State(control_plane): State<ControlPlane>,
    Path(organization): Path<String>,
) -> Result<Json<wire::ListTriggersResponse>, Refused> {
    let triggers = trigger::triggers(&control_plane.store, &organization)
        .await
        .map_err(named_refusal)?;

    Ok(Json(triggers.into_iter().map(Into::into).collect()))
}

async fn declare_trigger(
    State(control_plane): State<ControlPlane>,
    Path(organization): Path<String>,
    declaration: Result<Json<wire::TriggerDeclaration>, JsonRejection>,
) -> Result<(StatusCode, Json<wire::Trigger>), Refused> {
    let Json(declaration) = declaration?;
    let checked = declaration::check_trigger(&declaration::TriggerFields {
        name: &declaration.name,
        forms: declaration::ScheduleForms {
            filter: declaration.filter.as_ref(),
            every: declaration.every.as_deref(),
            cron: declaration.cron.as_deref(),
            zone: declaration.zone.as_deref(),
        },
        templates: declaration::TemplateFields {
            brief: &declaration.brief,
            branch: declaration.branch.as_ref().and_then(Option::as_deref),
            correlation: declaration.correlation.as_ref().and_then(Option::as_deref),
            on_miss: declaration.on_miss.as_ref().and_then(Option::as_deref),
            on_open_workspace: declaration
                .on_open_workspace
                .as_ref()
                .and_then(Option::as_deref),
        },
        project: &declaration.project,
        agent: &declaration.agent,
        declared: Declared::named(Declared {
            model: declaration.model.clone().flatten(),
            mode: declaration.mode.clone().flatten(),
            thought_level: declaration.thought_level.clone().flatten(),
        }),
        allows: declaration.allows.as_deref().unwrap_or_default(),
        profile: declaration.profile.as_ref().and_then(Option::as_deref),
    })
    .map_err(|error| declaration::scoped(declaration::Prefix::Root, "declare_trigger", error))?;
    let mut tx = control_plane.store.begin().await?;
    let organization = tx.organizations().named(&organization).await?;
    let placed = declaration::place_trigger(
        &mut tx,
        &organization,
        &checked,
        declaration::Provenance::Flags,
    )
    .await?;
    tx.commit().await?;

    Ok((
        match placed.change.action {
            declaration::Action::Add => StatusCode::CREATED,
            _ => StatusCode::OK,
        },
        Json(placed.trigger.into()),
    ))
}

async fn show_trigger(
    State(control_plane): State<ControlPlane>,
    Path((organization, name)): Path<(String, String)>,
) -> Result<Json<wire::Trigger>, Refused> {
    let trigger = trigger::show(&control_plane.store, &organization, &name)
        .await
        .map_err(named_refusal)?;

    Ok(Json(trigger.into()))
}

async fn test_trigger(
    State(control_plane): State<ControlPlane>,
    Path((organization, name)): Path<(String, String)>,
    tested: Result<Json<serde_json::Value>, JsonRejection>,
) -> Result<Json<wire::TriggerTestResult>, Refused> {
    let Json(sent) = tested?;
    let tested: wire::TriggerTest = serde_path_to_error::deserialize(&sent)
        .map_err(|error| malformed(None, error.to_string()))?;
    let declared = match tested.declared.flatten() {
        Some(_) => Some(trigger_file(&sent["declared"])?),
        None => None,
    };
    let event: Option<EventRecordId> = tested.event.flatten().map(|event| {
        event
            .to_string()
            .parse()
            .expect("an Event's identifier is a UUID")
    });
    let integration = tested.integration.flatten();
    let instruction = tested.instruction.flatten();
    let agent = tested.agent.flatten();
    let github;
    let against = match (event, integration.as_deref(), tested.issue.flatten()) {
        (Some(_), _, Some(_)) => {
            return Err(malformed(
                Some("issue"),
                "a test renders against an event or an issue, not both",
            ));
        }
        (Some(event), _, None) => trigger::Against::Event(event),
        (None, Some(integration), Some(issue)) => {
            github = Github::dialling_out(&control_plane.store)?;
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
        instruction: instruction.as_deref(),
        agent: agent.as_deref(),
    };
    let tested = match declared {
        Some(file) => {
            let declarations = triggers::declarations(file)
                .map_err(|error| {
                    declaration::scoped(declaration::Prefix::Root, "test_trigger", error)
                })
                .map_err(named_refusal)?;
            let declared = declarations
                .iter()
                .find(|declared| declared.name() == name)
                .ok_or_else(|| {
                    named_refusal(
                        Reason::MissingReference {
                            resource: Resource::Trigger,
                            reference: name.clone(),
                            organization: Some(organization.clone()),
                            within: None,
                            message: format!("the declaration file declares no trigger {name}"),
                        }
                        .into(),
                    )
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

    Ok(Json(wire::TriggerTestResult {
        matches: tested.matches,
        brief: rendered.brief,
        branch: rendered.branch,
        correlation: rendered.correlation,
        agent,
        would: match would {
            trigger::Would::Open => wire::TriggerTestResultWould::OpenWorkspace,
            trigger::Would::Continue => wire::TriggerTestResultWould::ContinueSession,
            trigger::Would::NewSession => wire::TriggerTestResultWould::NewSession,
            trigger::Would::Ignore => wire::TriggerTestResultWould::Ignore,
        },
        elapsing: tested.elapsing.map(|elapsing| elapsing.to_string()),
    }))
}

async fn disable_trigger(
    State(control_plane): State<ControlPlane>,
    Path((organization, name)): Path<(String, String)>,
) -> Result<Json<wire::Trigger>, Refused> {
    let trigger = trigger::disable(&control_plane.store, &organization, &name)
        .await
        .map_err(named_refusal)?;

    Ok(Json(trigger.into()))
}

async fn enable_trigger(
    State(control_plane): State<ControlPlane>,
    Path((organization, name)): Path<(String, String)>,
) -> Result<Json<wire::Trigger>, Refused> {
    let trigger = trigger::enable(&control_plane.store, &organization, &name)
        .await
        .map_err(named_refusal)?;

    Ok(Json(trigger.into()))
}

async fn dispatch_trigger(
    State(control_plane): State<ControlPlane>,
    Path((organization, name)): Path<(String, String)>,
    dispatch: Result<Json<wire::TriggerDispatch>, JsonRejection>,
) -> Result<Json<wire::Fired>, Refused> {
    let Json(dispatch) = dispatch?;
    let instruction = dispatch.instruction.flatten();
    let agent = dispatch.agent.flatten();
    let fired = trigger::dispatch(
        &control_plane.store,
        &Github::dialling_out(&control_plane.store)?,
        trigger::Dispatch {
            organization: &organization,
            trigger: &name,
            integration: &dispatch.integration,
            issue: dispatch.issue,
            asked: trigger::Asked {
                instruction: instruction.as_deref(),
                agent: agent.as_deref(),
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
        } => wire::Fired::FiredOpened(wire::FiredOpened {
            outcome: serde_json::json!("opened"),
            event: wire_id(&event),
            workspace: wire_id(&workspace),
            session: wire_id(&session),
        }),
        trigger::Fired::Fed {
            event,
            workspace,
            session,
        } => wire::Fired::FiredFed(wire::FiredFed {
            outcome: serde_json::json!("fed"),
            event: wire_id(&event),
            workspace: wire_id(&workspace),
            session: session.map(|session| wire_id(&session)),
        }),
        trigger::Fired::Ignored {
            event, correlation, ..
        } => wire::Fired::FiredIgnored(wire::FiredIgnored {
            outcome: serde_json::json!("ignored"),
            event: wire_id(&event),
            correlation,
        }),
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
    file: Result<Json<serde_json::Value>, JsonRejection>,
) -> Result<Json<wire::AppliedTriggers>, Refused> {
    applied_triggers(control_plane, organization, file, false).await
}

async fn preview_applied_triggers(
    State(control_plane): State<ControlPlane>,
    Path(organization): Path<String>,
    file: Result<Json<serde_json::Value>, JsonRejection>,
) -> Result<Json<wire::AppliedTriggers>, Refused> {
    applied_triggers(control_plane, organization, file, true).await
}

async fn applied_triggers(
    control_plane: ControlPlane,
    organization: String,
    file: Result<Json<serde_json::Value>, JsonRejection>,
    dry_run: bool,
) -> Result<Json<wire::AppliedTriggers>, Refused> {
    let Json(sent) = file?;
    let file = trigger_file(&sent)?;
    let operation = if dry_run {
        "preview_applied_triggers"
    } else {
        "apply_triggers"
    };
    let declarations = triggers::declarations(file)
        .map_err(|error| declaration::scoped(declaration::Prefix::Root, operation, error))
        .map_err(named_refusal)?;
    let applied = triggers::apply(&control_plane.store, &organization, &declarations, dry_run)
        .await
        .map_err(named_refusal)?;

    Ok(Json(wire::AppliedTriggers {
        changes: applied
            .changes
            .into_iter()
            .map(|change| wire::DeclarationChange {
                kind: wire::DeclarationChangeKind::Trigger,
                name: change.name,
                action: match change.action {
                    triggers::Action::Add => wire::DeclarationChangeAction::Add,
                    triggers::Action::Change => wire::DeclarationChangeAction::Change,
                    triggers::Action::Remove => wire::DeclarationChangeAction::Remove,
                },
                differences: differences(change.differences),
            })
            .collect(),
        admitting_outsiders: applied.admitting_outsiders,
    }))
}

/// An untyped refusal here is about what the request named, never a control plane that could
/// not answer.
fn named_refusal(error: anyhow::Error) -> Refused {
    if error.is::<Reason>() || error.is::<Concerning>() || error.is::<Within>() {
        return error.into();
    }
    Refused::Unprocessable(error.to_string())
}

async fn instances(
    State(control_plane): State<ControlPlane>,
    Path(organization): Path<String>,
) -> Result<Json<Vec<wire::HeldInstance>>, Refused> {
    let held = instance::held(&control_plane.store, &organization).await?;

    Ok(Json(
        held.into_iter()
            .map(|held| wire::HeldInstance {
                workspace: held.workspace.to_string().parse().expect("a uuid"),
                instance: held.instance,
                because: held.because,
            })
            .collect(),
    ))
}

fn queue_reason(reason: scheduling::Reason) -> wire::QueueReason {
    match reason {
        scheduling::Reason::Dependencies(sessions) => {
            wire::QueueReason::QueueReasonDependencies(wire::QueueReasonDependencies {
                kind: serde_json::json!("dependencies"),
                sessions,
            })
        }
        scheduling::Reason::SubscriptionProfile(busy) => {
            let profile = busy.profile.clone();
            let session = busy.holder.session().map(str::to_owned);
            let (_, diagnostic) = diagnosis(Reason::SignInBusy(busy), None);
            wire::QueueReason::QueueReasonSubscriptionProfile(
                wire::QueueReasonSubscriptionProfile {
                    profile,
                    session,
                    diagnostic,
                    kind: serde_json::json!("subscription_profile"),
                },
            )
        }
        scheduling::Reason::InstanceArchiving(instance) => {
            wire::QueueReason::QueueReasonInstanceArchiving(wire::QueueReasonInstanceArchiving {
                kind: serde_json::json!("instance_archiving"),
                instance,
            })
        }
        scheduling::Reason::LiveInstanceLimit(limit) => {
            wire::QueueReason::QueueReasonLiveInstanceLimit(wire::QueueReasonLiveInstanceLimit {
                kind: serde_json::json!("live_instance_limit"),
                limit: limit as i64,
            })
        }
        scheduling::Reason::ActiveWorkSlots(limit) => {
            wire::QueueReason::QueueReasonActiveWorkSlots(wire::QueueReasonActiveWorkSlots {
                kind: serde_json::json!("active_work_slots"),
                limit: limit as i64,
            })
        }
        scheduling::Reason::Ahead {
            sessions,
            elsewhere,
        } => wire::QueueReason::QueueReasonAhead(wire::QueueReasonAhead {
            sessions,
            elsewhere: (elsewhere != 0).then_some(elsewhere as i64),
            kind: serde_json::json!("ahead"),
        }),
    }
}

fn reasons(reasons: Vec<scheduling::Reason>) -> Vec<wire::QueueReason> {
    reasons.into_iter().map(queue_reason).collect()
}

fn queue_record(snapshot: scheduling::Snapshot) -> wire::Queue {
    let recorded = snapshot.recorded.map(|recorded| wire::QueueWorkRole {
        active_work_slots: recorded.active_work_slots as i64,
        serialized_harnesses: recorded.serialized_harnesses,
        driver: recorded.driver,
    });

    wire::Queue {
        work_role: recorded,
        active_work: wire::ActiveWork {
            limit: snapshot.active_work.limit.map(|limit| limit as i64),
            occupied: snapshot.active_work.occupied as i64,
            occupants: snapshot
                .active_work
                .occupants
                .into_iter()
                .map(|session| wire::Occupant {
                    name: session.name,
                    workspace: session.workspace.to_string().parse().expect("a uuid"),
                    agent: session.agent.name,
                    phase: match session.state {
                        domain::SessionState::Trailing => wire::OccupantPhase::Trailing,
                        _ => wire::OccupantPhase::Working,
                    },
                    enqueued_at: session.enqueued_at.to_string(),
                })
                .collect(),
            elsewhere: snapshot.active_work.elsewhere as i64,
        },
        instances: wire::QueueInstances {
            limit: snapshot.instances.limit.map(|limit| limit as i64),
            count: snapshot.instances.count as i64,
            counted: snapshot.instances.counted,
        },
        queued: snapshot
            .queued
            .into_iter()
            .map(|queued| wire::QueuedSession {
                position: queued.position.map(|position| position as i64),
                name: queued.session.name,
                workspace: queued
                    .session
                    .workspace
                    .to_string()
                    .parse()
                    .expect("a uuid"),
                agent: queued.session.agent.name,
                reasons: reasons(queued.reasons),
                enqueued_at: queued.session.enqueued_at.to_string(),
            })
            .collect(),
        waiting: snapshot
            .waiting
            .into_iter()
            .map(|waiting| wire::WaitingSession {
                position: waiting.position.map(|position| position as i64),
                name: waiting.session.name,
                workspace: waiting
                    .session
                    .workspace
                    .to_string()
                    .parse()
                    .expect("a uuid"),
                agent: waiting.session.agent.name,
                pending_since: waiting.pending_since.map(|since| since.to_string()),
                reasons: reasons(waiting.reasons),
                enqueued_at: waiting.session.enqueued_at.to_string(),
            })
            .collect(),
        unbriefed: snapshot
            .unbriefed
            .into_iter()
            .map(|unbriefed| wire::UnbriefedSession {
                position: unbriefed.position.map(|position| position as i64),
                brief_since: unbriefed.brief_since.map(|since| since.to_string()),
                reasons: reasons(unbriefed.reasons),
                name: unbriefed.session.name,
                workspace: unbriefed
                    .session
                    .workspace
                    .to_string()
                    .parse()
                    .expect("a uuid"),
                agent: unbriefed.session.agent.name,
                preparing: unbriefed
                    .session
                    .preparing
                    .map(|preparing| match preparing {
                        domain::Preparing::Provisioning => {
                            wire::UnbriefedSessionPreparing::Provisioning
                        }
                        domain::Preparing::Cloning => wire::UnbriefedSessionPreparing::Cloning,
                        domain::Preparing::StartingHarness => {
                            wire::UnbriefedSessionPreparing::StartingHarness
                        }
                        domain::Preparing::HarnessReady => {
                            wire::UnbriefedSessionPreparing::HarnessReady
                        }
                    }),
                pending_since: unbriefed.pending_since.map(|since| since.to_string()),
                enqueued_at: unbriefed.session.enqueued_at.to_string(),
            })
            .collect(),
    }
}

async fn show_queue(
    State(control_plane): State<ControlPlane>,
    Path(organization): Path<String>,
) -> Result<Json<wire::Queue>, Refused> {
    let snapshot = scheduling::snapshot(&control_plane.store, &organization).await?;

    Ok(Json(queue_record(snapshot)))
}

async fn release_instance(
    State(control_plane): State<ControlPlane>,
    Path((organization, workspace)): Path<(String, String)>,
    release: Result<Json<wire::InstanceRelease>, JsonRejection>,
) -> Result<Json<wire::ReleasedInstance>, Refused> {
    let Json(release) = release?;
    let workspace = resolved(&control_plane, &organization, &workspace).await?;
    let instance = instance::release(
        &control_plane.store,
        workspace.id,
        release
            .participant
            .as_ref()
            .and_then(|participant| participant.as_deref()),
    )
    .await?;

    Ok(Json(wire::ReleasedInstance { instance }))
}

async fn workspaces(
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

async fn open_workspace(
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

async fn work_summary(
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
) -> Result<Json<wire::Workspace>, Refused> {
    let workspace = resolved(&control_plane, &organization, &workspace).await?;

    Ok(Json(
        workspace_record(&control_plane.store, workspace).await?,
    ))
}

async fn post_to_workspace(
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

async fn edit_workspace_message(
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

async fn withdraw_workspace_message(
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
) -> Result<Json<wire::Workspace>, Refused> {
    let workspace = resolved(&control_plane, &organization, &workspace).await?;
    let workspace = workspace::seal(&control_plane.store, workspace.id).await?;

    Ok(Json(
        workspace_record(&control_plane.store, workspace).await?,
    ))
}

async fn sessions(
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

async fn enqueue_session(
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

async fn show_session(
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

async fn stop_session(
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

async fn set_session_option(
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

async fn resolved(
    control_plane: &ControlPlane,
    organization: &str,
    workspace: &str,
) -> Result<Workspace, Refused> {
    Ok(workspace::resolve(&control_plane.store, organization, workspace).await?)
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
    let name = follower_name(&control_plane, &workspace, following.as_name.as_deref()).await?;
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

async fn follower_name(
    control_plane: &ControlPlane,
    workspace: &Workspace,
    name: Option<&str>,
) -> Result<Option<String>, Refused> {
    let Some(name) = name else {
        return Ok(None);
    };
    let mut tx = control_plane.store.read().await?;

    Ok(Some(
        participant::accepted(&mut tx, &workspace.organization, name, "transcript").await?,
    ))
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
            let name = follower_name(
                &control_plane,
                &workspace,
                subscription.participant.as_deref(),
            )
            .await?;
            let transcribing = Transcribing {
                workspace: workspace.id,
                name,
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
        Step::EnableIntegration { integration } => {
            wire::Action::EnableIntegrationAction(wire::EnableIntegrationAction {
                action: serde_json::json!("enable_integration"),
                organization: organization.to_owned(),
                integration,
            })
        }
        Step::ReplacePrivateKey { integration } => {
            replace_private_key(organization.to_owned(), integration, None)
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

fn replace_private_key(
    organization: String,
    integration: String,
    retry_after_seconds: Option<i64>,
) -> wire::Action {
    wire::Action::ReplaceIntegrationPrivateKeyAction(wire::ReplaceIntegrationPrivateKeyAction {
        action: serde_json::json!("replace_integration_private_key"),
        organization,
        integration,
        retry_after_seconds,
    })
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
        Resource::SignInTest => wire::Resource::SignInTest,
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
        wire::Diagnostic::CredentialRejectedDiagnostic(d) => &d.message,
        wire::Diagnostic::AuthenticationFailedDiagnostic(d) => &d.message,
        wire::Diagnostic::ExecutableMissingDiagnostic(d) => &d.message,
        wire::Diagnostic::UnknownFailureDiagnostic(d) => &d.message,
        wire::Diagnostic::ConnectionFailedDiagnostic(d) => &d.message,
        wire::Diagnostic::ClientFailureDiagnostic(d) => &d.message,
        wire::Diagnostic::UnknownResponseDiagnostic(d) => &d.message,
    }
}

impl From<domain::Operator> for wire::Operator {
    fn from(operator: domain::Operator) -> Self {
        Self {
            id: operator.id.to_string().parse().expect("an Operator UUID"),
            name: operator.name,
        }
    }
}

async fn show_install(
    State(control_plane): State<ControlPlane>,
) -> Result<Json<wire::Install>, Refused> {
    Ok(Json(wire::Install {
        initial_setup_complete: crate::operator_identity::initial_setup_complete(
            &control_plane.store,
        )
        .await?,
    }))
}

async fn show_readiness(
    State(control_plane): State<ControlPlane>,
    Query(context): Query<start_readiness::Context>,
) -> Result<Json<wire::Readiness>, Refused> {
    let read = start_readiness::read(&control_plane.store, &control_plane.images, context).await?;

    Ok(Json(readiness(read, &control_plane.client)))
}

fn readiness(read: start_readiness::Readiness, client: &url::Url) -> wire::Readiness {
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

fn readiness_gap(gap: start_readiness::Gap, organization: Option<&str>) -> wire::Diagnostic {
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

fn setup_gap(
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

fn held_sign_in(
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
fn needs_attention(
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

async fn show_operator(
    State(control_plane): State<ControlPlane>,
) -> Result<Json<wire::Operator>, Refused> {
    Ok(Json(
        crate::operator_identity::current(&control_plane.store)
            .await?
            .into(),
    ))
}

async fn name_operator(
    State(control_plane): State<ControlPlane>,
    declaration: Result<Json<wire::OperatorDeclaration>, JsonRejection>,
) -> Result<Response, Refused> {
    let Json(declaration) = declaration?;
    let declared = crate::operator_identity::name(&control_plane.store, &declaration.name).await?;
    Ok(answered::<_, wire::Operator>(declared))
}

fn diagnosed(reason: Reason, field: Option<String>) -> Refused {
    let (status, diagnostic) = diagnosis(reason, field);
    Refused::Diagnosed {
        status,
        diagnostic: Box::new(diagnostic),
    }
}

fn diagnosis(reason: Reason, field: Option<String>) -> (StatusCode, wire::Diagnostic) {
    let request_field = field.clone();
    match reason {
        Reason::MissingOperator { operation } => (
            if operation == "show_operator" {
                StatusCode::NOT_FOUND
            } else {
                StatusCode::CONFLICT
            },
            wire::Diagnostic::SetupGapDiagnostic(wire::SetupGapDiagnostic {
                kind: serde_json::json!("setup_gap"),
                message: Reason::MissingOperator { operation }.to_string(),
                field,
                context: wire::SetupGapContext {
                    prerequisite: "operator".to_owned(),
                    resource: None,
                    reference: None,
                    organization: None,
                    harness: None,
                    method: None,
                    sign_in: None,
                    image: None,
                },
                next_steps: vec![wire::Action::NameOperatorAction(wire::NameOperatorAction {
                    action: serde_json::json!("name_operator"),
                    name: None,
                    missing: vec!["name".to_owned()],
                })],
            }),
        ),
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
        } => invalid_value(operation, field, constraint, allowed, message),
        Reason::SignInBusy(busy) => {
            let message = busy.to_string();
            let holding_session = busy.holder.session().map(str::to_owned);
            diagnosis(
                Reason::StateConflict {
                    operation: crate::sign_in::credential_use::ACQUIRE,
                    resource: Resource::SubscriptionProfile,
                    reference: busy.profile,
                    organization: Some(busy.organization),
                    state: "credential_in_use",
                    holding_session,
                    next: busy.next,
                    message,
                },
                request_field,
            )
        }
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
        Reason::CredentialRejected {
            operation: _,
            organization,
            harness,
            method,
            provider,
            status,
            provider_error,
            message,
        } => (
            StatusCode::UNPROCESSABLE_ENTITY,
            wire::Diagnostic::CredentialRejectedDiagnostic(wire::CredentialRejectedDiagnostic {
                kind: serde_json::json!("credential_rejected"),
                message,
                field: Some(field.unwrap_or_else(|| "value".to_owned())),
                next_steps: vec![wire::Action::SignInAction(wire::SignInAction {
                    action: serde_json::json!("sign_in"),
                    harness: Some(harness.clone()),
                    method: Some(method.clone()),
                    sign_in: None,
                })],
                context: wire::CredentialRejectedContext {
                    organization,
                    harness,
                    method,
                    provider: provider.to_owned(),
                    status: status.map(i64::from),
                    provider_error,
                },
            }),
        ),
        Reason::HarnessNotCarried {
            operation,
            harness,
            image,
            carried,
            message,
        } => (
            StatusCode::CONFLICT,
            wire::Diagnostic::SetupGapDiagnostic(wire::SetupGapDiagnostic {
                kind: serde_json::json!("setup_gap"),
                message,
                next_steps: vec![
                    inspect_harness_image(Some(harness.clone()), image.clone()),
                    wire::Action::CorrectFieldAction(wire::CorrectFieldAction {
                        action: serde_json::json!("correct_field"),
                        operation: operation.to_owned(),
                        resource: None,
                        field: field.clone().unwrap_or_else(|| "harness".to_owned()),
                        constraint: "carried_by_image".to_owned(),
                        allowed_values: Some(carried),
                    }),
                ],
                field,
                context: wire::SetupGapContext {
                    prerequisite: "harness_in_image".to_owned(),
                    resource: None,
                    reference: None,
                    organization: None,
                    harness: Some(harness),
                    method: None,
                    sign_in: None,
                    image: Some(image),
                },
            }),
        ),
        Reason::GithubUnavailable {
            operation,
            integration,
            organization,
            retry_after_seconds,
            again,
            message,
        } => (
            StatusCode::SERVICE_UNAVAILABLE,
            wire::Diagnostic::UnavailableDiagnostic(wire::UnavailableDiagnostic {
                kind: serde_json::json!("unavailable"),
                message,
                field,
                context: wire::UnavailableContext {
                    service: "github".to_owned(),
                    resource: Some(integration.clone()),
                    operation: operation.to_owned(),
                    retry_after_seconds,
                    saved: None,
                },
                next_steps: vec![match again {
                    Again::ReplacePrivateKey => {
                        replace_private_key(organization, integration, retry_after_seconds)
                    }
                    Again::Read => {
                        retry_read(operation, Some(Resource::Integration), retry_after_seconds)
                    }
                    Again::Request => wire::Action::RetryRequestAction(wire::RetryRequestAction {
                        action: serde_json::json!("retry_request"),
                        operation: operation.to_owned(),
                        resource: Some(integration),
                        retry_after_seconds,
                    }),
                }],
            }),
        ),
        Reason::ImageUnavailable {
            operation,
            harness,
            image,
            message,
        } => (
            StatusCode::SERVICE_UNAVAILABLE,
            wire::Diagnostic::UnavailableDiagnostic(image_unavailable(
                operation, harness, image, message, field,
            )),
        ),
    }
}

fn inspect_harness_image(harness: Option<String>, image: String) -> wire::Action {
    wire::Action::InspectHarnessImageAction(wire::InspectHarnessImageAction {
        action: serde_json::json!("inspect_harness_image"),
        harness,
        image: Some(image),
        command: None,
    })
}

/// Nothing was written, so the step after inspecting the image is to read its capabilities
/// again, never to replay a write blind.
fn image_unavailable(
    operation: &str,
    harness: Option<String>,
    image: String,
    message: String,
    field: Option<String>,
) -> wire::UnavailableDiagnostic {
    wire::UnavailableDiagnostic {
        kind: serde_json::json!("unavailable"),
        message,
        field,
        context: wire::UnavailableContext {
            service: "image_inspection".to_owned(),
            resource: Some(image.clone()),
            operation: operation.to_owned(),
            retry_after_seconds: None,
            saved: None,
        },
        next_steps: vec![
            inspect_harness_image(harness, image),
            retry_read("list_harnesses", None, None),
        ],
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
    let (status, diagnostic) = invalid_value(operation, field, constraint, None, message.into());
    Refused::Diagnosed {
        status,
        diagnostic: Box::new(diagnostic),
    }
}

fn invalid_value(
    operation: &'static str,
    field: &str,
    constraint: Constraint,
    allowed: Option<Vec<String>>,
    message: String,
) -> (StatusCode, wire::Diagnostic) {
    (
        StatusCode::UNPROCESSABLE_ENTITY,
        wire::Diagnostic::InvalidFieldDiagnostic(wire::InvalidFieldDiagnostic {
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
        }),
    )
}

impl From<anyhow::Error> for Refused {
    fn from(error: anyhow::Error) -> Self {
        let error = match error.downcast::<Reason>() {
            Ok(reason) => return diagnosed(reason, None),
            Err(error) => error,
        };
        let error = match error.downcast::<Concerning>() {
            Ok(concerning) => {
                return diagnosed(concerning.reason, Some(concerning.field.to_owned()));
            }
            Err(error) => error,
        };
        let error = match error.downcast::<Within>() {
            Ok(Within {
                record,
                field,
                reason,
            }) => {
                let path = |field: &str| format!("{record}.{field}");
                return match reason {
                    Reason::InvalidField {
                        field,
                        operation,
                        constraint,
                        allowed,
                        message,
                    } => {
                        let (status, diagnostic) =
                            invalid_value(operation, &path(field), constraint, allowed, message);
                        Refused::Diagnosed {
                            status,
                            diagnostic: Box::new(diagnostic),
                        }
                    }
                    reason => diagnosed(reason, field.map(path)),
                };
            }
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
                    saved: None,
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
        ("GET", ROOT) => "show_api_root",
        ("GET", HARNESSES) => "list_harnesses",
        ("GET", SIGN_IN_METHOD) => "show_sign_in_method",
        ("GET", INSTALL) => "show_install",
        ("GET", READINESS) => start_readiness::OPERATION,
        ("GET", OPERATOR) => "show_operator",
        ("PUT", OPERATOR) => "name_operator",
        ("GET", ORGANIZATIONS) => "list_organizations",
        ("POST", ORGANIZATIONS) => "declare_organization",
        ("POST", STARTS) => "start",
        ("POST", REPOSITORY_RESOLUTIONS) => "resolve_repositories",
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
        ("GET", SIGN_INS) => "list_sign_ins",
        ("PUT", SIGN_IN) => "save_sign_in",
        ("POST", SIGN_IN_TESTS) => sign_in::model_test::TEST,
        ("GET", SIGN_IN_TEST) => sign_in::model_test::READ,
        ("GET", INTEGRATIONS) => "list_integrations",
        ("POST", INTEGRATIONS) => "register_integration",
        ("GET", INTEGRATION) => "show_integration",
        ("PATCH", INTEGRATION) => "change_integration",
        ("POST", INTEGRATION_DISABLE) => "disable_integration",
        ("POST", INTEGRATION_ENABLE) => "enable_integration",
        ("POST", INTEGRATION_RETIRE) => "retire_integration",
        ("GET", INTEGRATION_REPOSITORIES) => "list_integration_repositories",
        ("PUT", INTEGRATION_PRIVATE_KEY) => "replace_integration_private_key",
        ("POST", GITHUB_APP) => "start_github_app",
        ("GET", integration::manifest::PAGE) => "github_app_setup",
        ("GET", integration::manifest::CALLBACK) => "github_app_callback",
        ("GET", integration::manifest::INSTALLED) => "github_app_installed",
        ("POST", integration::manifest::PAGE) => "finish_github_app_setup",
        ("GET", integration::manifest::FLOW) => "show_github_app_flow",
        ("DELETE", integration::manifest::FLOW) => "cancel_github_app_flow",
        ("GET", integration::manifest::INSTALLATIONS) => "list_github_app_installations",
        ("GET", integration::manifest::REPOSITORIES) => "list_github_app_repositories",
        ("POST", integration::manifest::FINISH) => "finish_github_app_flow",
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
