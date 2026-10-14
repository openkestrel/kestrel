//! The boundary a Client reaches the control plane over, specified by `openapi/operator/`.
//! It authenticates nobody, so it is served apart from the link and on loopback (ADR-0015).

use std::net::IpAddr;
use std::time::Duration;

use axum::Router;
use axum::extract::Request;
use axum::http::header::{HOST, ORIGIN};
use axum::http::uri::Authority;
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post, put};
use kestrel_operator_types as wire;
use serde::Deserialize;
use tokio_util::sync::CancellationToken;

use crate::capability::Images;
use crate::integration;
use crate::sign_in;
use crate::start_readiness;
use crate::store::Store;
use crate::stream;

mod credentials;
mod declarations;
mod diagnosis;
mod events;
mod github_app;
mod integrations;
mod organizations;
mod queue;
mod readiness;
mod records;
mod refusal;
mod sessions;
mod sign_ins;
mod starts;
mod streams;
mod triggers;
mod workspaces;

use credentials::*;
use declarations::*;
use diagnosis::*;
use events::*;
use github_app::*;
use integrations::*;
use organizations::*;
use queue::*;
use readiness::*;
use records::*;
use refusal::*;
use sessions::*;
use sign_ins::*;
use starts::*;
use streams::*;
use triggers::*;
use workspaces::*;

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
        let fragments =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../openapi/operator");
        let document =
            kestrel_operator_types::document::read(&fragments).expect("a valid operator document");
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
