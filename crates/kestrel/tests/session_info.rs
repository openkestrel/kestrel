mod support;

use std::fs;
use std::time::Duration;

use kestrel::domain::{SessionId, SessionState};
use kestrel::log::Entry;
use kestrel_scripted_agent::{
    COMMAND, COMMAND_DESCRIPTION, COMMAND_HINT, CUSTOM_CATEGORY, CUSTOM_OPTION, MODE_OPTION,
    OTHER_MODEL, STARTING_MODE, SWITCHED_MODE, TITLE,
};
use reqwest::StatusCode;
use reqwest::header::{ETAG, IF_NONE_MATCH};
use serde_json::Value;
use support::scripted_agent::{self, Script};
use support::supervisor;
use support::{HARNESS, Kestrel};

const PATIENCE: Duration = Duration::from_secs(30);

async fn a_workspace(kestrel: &Kestrel, model: Option<&str>) -> kestrel::domain::Workspace {
    let organization = kestrel.declare_organization("acme").await;
    kestrel
        .declare_project(
            &organization,
            support::repository::NAME,
            &[support::repository::url().to_owned()],
            support::repository::BRANCH,
        )
        .await;
    kestrel
        .declare_agent(&organization, "builder", HARNESS, model)
        .await;
    kestrel
        .hold_provider_credential(
            &organization,
            support::PROVIDER_KEY,
            support::A_PROVIDER_KEY,
        )
        .await;

    kestrel.open_workspace("acme", "kestrel", "builder").await
}

struct Shown {
    status: StatusCode,
    etag: Option<String>,
    body: Value,
}

async fn read_session(kestrel: &Kestrel, session: &str, etag: Option<&str>) -> Shown {
    let path = kestrel::operator::SESSION
        .replace("{organization}", "acme")
        .replace("{session}", session);
    let mut request = reqwest::Client::new().get(format!("{}{path}", kestrel.operator()));
    if let Some(etag) = etag {
        request = request.header(IF_NONE_MATCH, etag);
    }
    let response = request
        .send()
        .await
        .expect("the operator boundary should answer");
    let status = response.status();
    let etag = response
        .headers()
        .get(ETAG)
        .and_then(|tag| tag.to_str().ok())
        .map(str::to_owned);
    let body = if status == StatusCode::NOT_MODIFIED {
        Value::Null
    } else {
        response.json().await.expect("the session as JSON")
    };

    Shown { status, etag, body }
}

async fn until_shown(
    kestrel: &Kestrel,
    session: SessionId,
    what: &str,
    ready: impl Fn(&Value) -> bool,
) -> Value {
    let deadline = tokio::time::Instant::now() + PATIENCE;

    loop {
        let shown = read_session(kestrel, &session.to_string(), None).await;
        if ready(&shown.body) {
            return shown.body;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the session {session} never showed {what}. the read is {}",
            shown.body
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

fn option<'a>(shown: &'a Value, category: &str) -> &'a Value {
    shown["options"]
        .as_array()
        .expect("the session's options")
        .iter()
        .find(|option| option["category"] == category)
        .unwrap_or_else(|| panic!("no option in the category {category}: {shown}"))
}

fn command<'a>(shown: &'a Value, name: &str) -> &'a Value {
    shown["commands"]
        .as_array()
        .expect("the session's commands")
        .iter()
        .find(|command| command["name"] == name)
        .unwrap_or_else(|| panic!("no command named {name}: {shown}"))
}

#[tokio::test]
async fn unchanged_bookkeeping_is_not_reported_again_on_the_same_connection() {
    let kestrel = Kestrel::boot().await;
    let workspace = a_workspace(&kestrel, None).await;
    let (session, on) = kestrel.dispatch_to_the_link(workspace.id).await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("the observing link should bind");
    let address = listener.local_addr().expect("the link's address");
    let base = kestrel.link();
    let client = reqwest::Client::new();
    let (reported, mut reports) = tokio::sync::mpsc::unbounded_channel();
    let router = axum::Router::new().fallback(move |request: axum::extract::Request| {
        let base = base.clone();
        let client = client.clone();
        let reported = reported.clone();
        async move {
            let (parts, body) = request.into_parts();
            let body = axum::body::to_bytes(body, 1024 * 1024)
                .await
                .expect("a bounded link request");
            if parts.uri.path().ends_with("/reports") {
                let report: Value = serde_json::from_slice(&body).expect("a report as JSON");
                if report["kind"] == "session_info" {
                    reported.send(report).expect("the test is still receiving");
                }
            }
            let response = client
                .request(parts.method, format!("{base}{}", parts.uri))
                .headers(parts.headers)
                .body(body)
                .send()
                .await
                .expect("the control plane should answer");
            let status = response.status();
            let headers = response.headers().clone();
            let mut forwarded = axum::response::Response::new(axum::body::Body::from_stream(
                response.bytes_stream(),
            ));
            *forwarded.status_mut() = status;
            *forwarded.headers_mut() = headers;
            forwarded
        }
    });
    let observing = tokio::spawn(async move {
        axum::serve(listener, router)
            .await
            .expect("the link serves");
    });
    let mut supervisor = supervisor::Supervisor::provision_playing(
        &format!("http://{address}"),
        &on,
        Script::Repeats,
    );
    kestrel.start(&session, supervisor.harness()).await;

    let first = tokio::time::timeout(PATIENCE, reports.recv())
        .await
        .expect("bookkeeping should reach the link")
        .expect("a bookkeeping report");
    assert_eq!(first["session"], session.id.to_string());
    assert!(first["seq"].is_null(), "bookkeeping is unnumbered");
    let repeated = tokio::time::timeout(Duration::from_secs(3), reports.recv()).await;
    kestrel.stop_session(session.id).await;
    supervisor.lets_go_of(session.id).await;

    assert!(
        repeated.is_err(),
        "unchanged bookkeeping was sent again: {repeated:?}"
    );

    supervisor.destroy();
    observing.abort();
    kestrel.teardown().await;
}

#[tokio::test]
async fn a_session_read_shows_the_title_its_options_and_its_commands() {
    let kestrel = Kestrel::dispatching_to(
        supervisor::binary(),
        &scripted_agent::playing(Script::Announces),
    )
    .await;
    let workspace = a_workspace(&kestrel, Some(OTHER_MODEL)).await;
    let session = kestrel.enqueue_session(workspace.id).await;

    let shown = until_shown(&kestrel, session.id, "its bookkeeping", |shown| {
        !shown["title"].is_null()
            && !shown["options"].as_array().is_none_or(Vec::is_empty)
            && !shown["commands"].as_array().is_none_or(Vec::is_empty)
    })
    .await;

    assert_eq!(shown["title"], TITLE);
    assert_eq!(shown["worked_model"], OTHER_MODEL);

    let model = option(&shown, "model");
    assert_eq!(model["kind"], "select");
    assert_eq!(model["current"], OTHER_MODEL);
    assert_eq!(
        model["warns_cache"], true,
        "changing a model does not warn that the context is re-read uncached"
    );
    assert!(
        model["values"]
            .as_array()
            .is_some_and(|values| values.iter().any(|value| value["value"] == OTHER_MODEL)),
        "the model option offers no values: {model}"
    );

    assert_eq!(
        option(&shown, "mode")["warns_cache"],
        false,
        "changing a mode warns about the cache, and ADR-0041 says it does not"
    );
    assert_eq!(
        option(&shown, "thought_level")["warns_cache"],
        true,
        "an opencode harness keeps the cache for a thought level, and this one does not"
    );

    let custom = option(&shown, CUSTOM_CATEGORY);
    assert_eq!(custom["id"], CUSTOM_OPTION);
    assert_eq!(custom["kind"], "boolean");
    assert_eq!(custom["current"], false);

    let compact = command(&shown, COMMAND);
    assert_eq!(compact["description"], COMMAND_DESCRIPTION);
    assert_eq!(compact["input_hint"], COMMAND_HINT);
    assert!(command(&shown, "init")["input_hint"].is_null());

    kestrel.stop_session(session.id).await;
    kestrel.teardown().await;
}

#[tokio::test]
async fn a_current_mode_update_changes_the_mode_options_current_value() {
    let kestrel = Kestrel::dispatching_to(
        supervisor::binary(),
        &scripted_agent::playing(Script::Announces),
    )
    .await;
    let workspace = a_workspace(&kestrel, Some(OTHER_MODEL)).await;
    let session = kestrel.enqueue_session(workspace.id).await;

    let shown = until_shown(&kestrel, session.id, "the mode it switched to", |shown| {
        !shown["options"].as_array().is_none_or(Vec::is_empty)
    })
    .await;

    let mode = option(&shown, "mode");
    assert_eq!(mode["id"], MODE_OPTION);
    assert_eq!(mode["current"], SWITCHED_MODE);
    assert!(
        mode["values"]
            .as_array()
            .is_some_and(|values| values.iter().any(|value| value["value"] == STARTING_MODE)),
        "the mode option offers no values: {mode}"
    );

    kestrel.answered(session.id, 1).await;
    let path = kestrel::operator::SESSION_OPTIONS
        .replace("{organization}", "acme")
        .replace("{session}", &session.id.to_string());
    let response = reqwest::Client::new()
        .post(format!("{}{path}", kestrel.operator()))
        .json(&serde_json::json!({
            "participant": "operator",
            "option": MODE_OPTION,
            "value": STARTING_MODE,
        }))
        .send()
        .await
        .expect("the operator boundary should answer");
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    until_shown(&kestrel, session.id, "the mode it was set to", |shown| {
        option(shown, "mode")["current"] == STARTING_MODE
    })
    .await;
    let deadline = tokio::time::Instant::now() + PATIENCE;
    let changes = loop {
        let changes: Vec<_> = kestrel
            .transcript(workspace.id)
            .await
            .into_iter()
            .filter_map(|entry| match entry.entry {
                Entry::OptionChanged { participant, .. } => Some(participant),
                _ => None,
            })
            .collect();
        if !changes.is_empty() {
            break changes;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the person's mode change was never recorded"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    };
    assert_eq!(
        changes,
        ["operator"],
        "the agent's own mode switch was recorded as a person's change"
    );

    kestrel.stop_session(session.id).await;
    kestrel.teardown().await;
}

#[tokio::test]
async fn a_later_option_list_replaces_the_mode_from_an_earlier_notification() {
    let kestrel = Kestrel::dispatching_to(
        supervisor::binary(),
        &scripted_agent::playing(Script::SwitchesModeBack),
    )
    .await;
    let workspace = a_workspace(&kestrel, None).await;
    let session = kestrel.enqueue_session(workspace.id).await;

    let shown = until_shown(&kestrel, session.id, "the updated option list", |shown| {
        shown["worked_model"] == OTHER_MODEL
    })
    .await;
    assert_eq!(option(&shown, "mode")["current"], STARTING_MODE);

    kestrel.stop_session(session.id).await;
    kestrel.teardown().await;
}

#[tokio::test]
async fn a_harness_that_offers_only_legacy_modes_lists_a_mode_option() {
    let kestrel = Kestrel::dispatching_to(
        supervisor::binary(),
        &scripted_agent::playing(Script::LegacyModes),
    )
    .await;
    let workspace = a_workspace(&kestrel, None).await;
    let session = kestrel.enqueue_session(workspace.id).await;

    let shown = until_shown(&kestrel, session.id, "a synthesized mode option", |shown| {
        !shown["options"].as_array().is_none_or(Vec::is_empty)
    })
    .await;

    let mode = option(&shown, "mode");
    assert_eq!(mode["id"], MODE_OPTION);
    assert_eq!(mode["current"], SWITCHED_MODE);
    let offered: Vec<&str> = mode["values"]
        .as_array()
        .expect("the modes it offers")
        .iter()
        .filter_map(|value| value["value"].as_str())
        .collect();
    assert_eq!(offered, [STARTING_MODE, SWITCHED_MODE, "review"]);

    kestrel.stop_session(session.id).await;
    kestrel.teardown().await;
}

#[tokio::test]
async fn the_agent_switching_its_model_mid_turn_changes_worked_model() {
    let kestrel = Kestrel::dispatching_to(
        supervisor::binary(),
        &scripted_agent::playing(Script::SwitchesModel),
    )
    .await;
    let workspace = a_workspace(&kestrel, Some(kestrel_scripted_agent::DEFAULT_MODEL)).await;
    let session = kestrel.enqueue_session(workspace.id).await;

    let shown = until_shown(&kestrel, session.id, "the model it switched to", |shown| {
        shown["worked_model"] == OTHER_MODEL
    })
    .await;

    assert_eq!(option(&shown, "model")["current"], OTHER_MODEL);

    kestrel.stop_session(session.id).await;
    kestrel.teardown().await;
}

#[tokio::test]
async fn what_the_harness_said_about_a_session_survives_a_control_plane_restart() {
    let kestrel = Kestrel::dispatching_to(
        supervisor::binary(),
        &scripted_agent::playing(Script::Announces),
    )
    .await;
    let workspace = a_workspace(&kestrel, Some(OTHER_MODEL)).await;
    let session = kestrel.enqueue_session(workspace.id).await;
    let before = until_shown(&kestrel, session.id, "its bookkeeping", |shown| {
        !shown["title"].is_null()
    })
    .await;

    let kestrel = kestrel.kill().await.restart().await;
    let after = read_session(&kestrel, &session.id.to_string(), None).await;

    assert_eq!(after.status, StatusCode::OK);
    assert_eq!(after.body["title"], before["title"]);
    assert_eq!(after.body["options"], before["options"]);
    assert_eq!(after.body["commands"], before["commands"]);

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_state_that_changed_while_the_link_was_down_reaches_the_read_after_the_reconnect() {
    let kestrel = Kestrel::dispatching_to(
        supervisor::binary(),
        &scripted_agent::playing(Script::Announces),
    )
    .await;
    let workspace = a_workspace(&kestrel, Some(OTHER_MODEL)).await;
    let session = kestrel.enqueue_session(workspace.id).await;

    let deadline = tokio::time::Instant::now() + PATIENCE;
    while kestrel.session(session.id).await.state != SessionState::Working {
        assert!(
            tokio::time::Instant::now() < deadline,
            "the session never started"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    let kestrel = kestrel.kill().await;
    // The harness says what it has to say with nothing there to take it, and the supervisor keeps
    // it until the link is back.
    tokio::time::sleep(Duration::from_secs(2)).await;
    let kestrel = kestrel.restart().await;

    let shown = until_shown(
        &kestrel,
        session.id,
        "what it said while the link was down",
        |shown| shown["title"] == TITLE,
    )
    .await;
    assert!(
        !shown["options"].as_array().is_none_or(Vec::is_empty),
        "the read lost the options held across the outage: {shown}"
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_repeat_read_with_the_etag_answers_304_and_a_change_answers_200_with_a_new_etag() {
    let kestrel = Kestrel::boot().await;
    let workspace = a_workspace(&kestrel, Some(OTHER_MODEL)).await;
    let session = kestrel.enqueue_session(workspace.id).await;
    let session = session.id.to_string();

    let first = read_session(&kestrel, &session, None).await;
    assert_eq!(first.status, StatusCode::OK);
    let etag = first.etag.clone().expect("the read should serve an ETag");
    assert!(
        etag.starts_with('"') && etag.ends_with('"'),
        "the ETag is not strong: {etag}"
    );

    let repeated = read_session(&kestrel, &session, Some(&etag)).await;
    assert_eq!(repeated.status, StatusCode::NOT_MODIFIED);
    assert!(repeated.body.is_null());
    assert_eq!(repeated.etag.as_deref(), Some(etag.as_str()));

    kestrel
        .stop_session(session.parse().expect("a session id"))
        .await;
    let changed = read_session(&kestrel, &session, Some(&etag)).await;
    assert_eq!(changed.status, StatusCode::OK);
    assert_ne!(
        changed.etag.as_deref(),
        Some(etag.as_str()),
        "the ETag did not change with the Session"
    );

    kestrel.teardown().await;
}

#[test]
fn the_published_operator_document_describes_the_sessions_bookkeeping_and_its_etag() {
    let document: Value = serde_json::from_str(
        &fs::read_to_string(support::crate_root().join("../../openapi/operator.json"))
            .expect("a readable openapi document"),
    )
    .expect("valid json");

    let required = |schema: &Value| {
        schema["required"]
            .as_array()
            .expect("an array of required fields")
            .iter()
            .filter_map(|field| field.as_str().map(str::to_owned))
            .collect::<Vec<_>>()
    };
    let session = &document["components"]["schemas"]["Session"];
    for field in ["title", "options", "commands"] {
        assert!(
            required(session).contains(&field.to_owned()),
            "the document does not require {field} on a Session"
        );
    }

    let option = &document["components"]["schemas"]["SessionOption"];
    for field in ["category", "kind", "current", "values", "warns_cache"] {
        assert!(
            required(option).contains(&field.to_owned()),
            "the document does not require {field} on a Session option"
        );
    }

    let read = &document["paths"][kestrel::operator::SESSION]["get"]["responses"];
    assert!(
        read["200"]["headers"]["ETag"].is_object(),
        "the document describes no ETag on the Session read"
    );
    assert!(
        read["304"].is_object(),
        "the document describes no 304 for the Session read"
    );
}
