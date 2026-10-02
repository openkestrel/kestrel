mod support;

use std::fs;
use std::time::Duration;

use kestrel::operator;
use reqwest::StatusCode;
use serde_json::{Value, json};
use support::Kestrel;
use support::scripted_agent::{self, Script};
use support::supervisor;

/// Longer than the hub's 250 ms coalescing window, so a quiet gap is really quiet.
const QUIET: Duration = Duration::from_millis(600);

struct Stream {
    response: reqwest::Response,
    buffered: String,
}

struct Frame {
    name: String,
    data: Value,
}

impl Stream {
    async fn open(kestrel: &Kestrel, organization: &str) -> Self {
        Self::open_with(kestrel, organization, None).await
    }

    async fn open_with(kestrel: &Kestrel, organization: &str, last_event_id: Option<&str>) -> Self {
        let path = operator::CHANGES.replace("{organization}", organization);
        let mut request = reqwest::Client::new().get(format!("{}{path}", kestrel.operator()));
        if let Some(id) = last_event_id {
            request = request.header("last-event-id", id);
        }

        let response = request
            .send()
            .await
            .expect("the operator boundary should answer");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|kind| kind.to_str().ok()),
            Some("text/event-stream")
        );

        Self {
            response,
            buffered: String::new(),
        }
    }

    async fn next(&mut self, patience: Duration) -> Option<Frame> {
        let deadline = tokio::time::Instant::now() + patience;

        loop {
            if let Some(frame) = self.take_frame() {
                return Some(frame);
            }
            match tokio::time::timeout_at(deadline, self.response.chunk()).await {
                Ok(Ok(Some(chunk))) => self.buffered.push_str(&String::from_utf8_lossy(&chunk)),
                Ok(Ok(None)) => return None,
                Ok(Err(error)) => panic!("the stream failed: {error}"),
                Err(_) => return None,
            }
        }
    }

    fn take_frame(&mut self) -> Option<Frame> {
        loop {
            let end = self.buffered.find("\n\n")?;
            let frame: String = self.buffered.drain(..end + 2).collect();

            let mut id = None;
            let mut name = String::new();
            let mut data = String::new();
            for line in frame.lines() {
                if let Some(value) = line.strip_prefix("id:") {
                    id = Some(value.trim().to_owned());
                } else if let Some(value) = line.strip_prefix("event:") {
                    name = value.trim().to_owned();
                } else if let Some(value) = line.strip_prefix("data:") {
                    data.push_str(value.trim());
                }
            }
            assert!(
                id.is_none(),
                "a change notice carries no sse id, and this one carried {id:?}"
            );
            if name.is_empty() && data.is_empty() {
                continue;
            }

            let data = if data.is_empty() {
                Value::Null
            } else {
                serde_json::from_str(&data)
                    .unwrap_or_else(|error| panic!("{data} is not JSON: {error}"))
            };
            return Some(Frame { name, data });
        }
    }
}

async fn notices_until_quiet(stream: &mut Stream) -> Vec<Frame> {
    let mut frames = Vec::new();
    while let Some(frame) = stream.next(QUIET).await {
        frames.push(frame);
    }

    frames
}

fn names(frames: &[Frame]) -> Vec<String> {
    frames
        .iter()
        .map(|frame| format!("{}({})", frame.name, frame.data))
        .collect()
}

fn names_changed(frames: &[Frame], resource: &str, id: Option<&str>) -> bool {
    frames.iter().any(|frame| {
        frame.name == "change"
            && frame.data["resource"] == resource
            && id.is_none_or(|id| frame.data["id"] == id)
    })
}

async fn declare_organization(kestrel: &Kestrel, name: &str) {
    let organization = kestrel.declare_organization(name).await;
    kestrel
        .declare_project(
            &organization,
            "kestrel",
            &["https://github.com/jtmthf/kestrel".to_owned()],
            "main",
        )
        .await;
    kestrel
        .declare_agent(&organization, "builder", "opencode", None)
        .await;
}

async fn post(kestrel: &Kestrel, workspace: kestrel::domain::WorkspaceId, message: &str) -> Value {
    let path = operator::WORKSPACE_MESSAGES
        .replace("{organization}", "acme")
        .replace("{workspace}", &workspace.to_string());
    let response = reqwest::Client::new()
        .post(format!("{}{path}", kestrel.operator()))
        .json(&json!({ "participant": "operator", "message": message }))
        .send()
        .await
        .expect("the operator boundary should answer");

    assert_eq!(response.status(), StatusCode::OK);

    let posted: Value = response.json().await.expect("the post's answer as JSON");
    posted["session"].clone()
}

#[tokio::test]
async fn a_subscriber_gets_open_on_connect_and_again_on_reconnect() {
    let kestrel = Kestrel::boot().await;
    declare_organization(&kestrel, "acme").await;

    let mut first = Stream::open(&kestrel, "acme").await;
    let open = first.next(QUIET).await.expect("an open event");
    assert_eq!(open.name, "open");
    drop(first);

    let mut again = Stream::open_with(&kestrel, "acme", Some("7")).await;
    let open = again.next(QUIET).await.expect("an open event");
    assert_eq!(open.name, "open");

    kestrel.teardown().await;
}

#[tokio::test]
async fn opening_a_workspace_posting_a_message_and_ending_the_session_raise_their_notices() {
    let kestrel = Kestrel::boot().await;
    declare_organization(&kestrel, "acme").await;
    let mut changes = Stream::open(&kestrel, "acme").await;
    assert_eq!(
        changes.next(QUIET).await.expect("an open event").name,
        "open"
    );

    let workspace = kestrel.open_workspace("acme", "kestrel", "builder").await;
    let opened = notices_until_quiet(&mut changes).await;
    assert!(
        names_changed(&opened, "workspace", Some(&workspace.id.to_string())),
        "opening the workspace raised no workspace notice: {opened:?}",
        opened = names(&opened)
    );

    let session = post(&kestrel, workspace.id, "begin").await;
    let session_id = session["id"].as_str().expect("the session id").to_owned();
    let started = notices_until_quiet(&mut changes).await;
    assert!(
        names_changed(&started, "session", Some(&session_id)),
        "starting a session raised no session notice: {started:?}",
        started = names(&started)
    );
    assert!(
        names_changed(&started, "queue", None),
        "starting a session raised no queue notice: {started:?}",
        started = names(&started)
    );

    kestrel
        .stop_session(session_id.parse().expect("a session id"))
        .await;
    let ended = notices_until_quiet(&mut changes).await;
    assert!(
        names_changed(&ended, "session", Some(&session_id)),
        "ending the session raised no session notice: {ended:?}",
        ended = names(&ended)
    );
    assert!(
        names_changed(&ended, "queue", None),
        "ending the session raised no queue notice: {ended:?}",
        ended = names(&ended)
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_notice_never_arrives_before_a_get_shows_its_change() {
    let kestrel = Kestrel::boot().await;
    declare_organization(&kestrel, "acme").await;
    let mut changes = Stream::open(&kestrel, "acme").await;
    assert_eq!(
        changes.next(QUIET).await.expect("an open event").name,
        "open"
    );

    let workspace = kestrel.open_workspace("acme", "kestrel", "builder").await;
    let session = post(&kestrel, workspace.id, "begin").await;
    let session_id = session["id"].as_str().expect("the session id").to_owned();
    let session_name = session["name"]
        .as_str()
        .expect("the session name")
        .to_owned();
    kestrel
        .stop_session(session_id.parse().expect("a session id"))
        .await;

    let ended = notices_until_quiet(&mut changes).await;
    assert!(
        names_changed(&ended, "session", Some(&session_id)),
        "ending the session raised no session notice: {ended:?}",
        ended = names(&ended)
    );

    let path = operator::SESSION
        .replace("{organization}", "acme")
        .replace("{session}", &session_name);
    let shown: Value = reqwest::Client::new()
        .get(format!("{}{path}", kestrel.operator()))
        .send()
        .await
        .expect("the operator boundary should answer")
        .json()
        .await
        .expect("the session as JSON");

    assert_eq!(
        shown["state"], "ended",
        "the notice came before the change did"
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_refused_post_raises_nothing() {
    let kestrel = Kestrel::boot().await;
    declare_organization(&kestrel, "acme").await;
    let workspace = kestrel.open_workspace("acme", "kestrel", "builder").await;
    kestrel.seal_workspace(workspace.id).await;
    // The open and the seal raised notices of their own; letting the coalescing window pass
    // leaves only the refusal under test.
    tokio::time::sleep(Duration::from_millis(400)).await;

    let mut changes = Stream::open(&kestrel, "acme").await;
    assert_eq!(
        changes.next(QUIET).await.expect("an open event").name,
        "open"
    );

    let path = operator::WORKSPACE_MESSAGES
        .replace("{organization}", "acme")
        .replace("{workspace}", &workspace.id.to_string());
    let response = reqwest::Client::new()
        .post(format!("{}{path}", kestrel.operator()))
        .json(&json!({ "message": "too late" }))
        .send()
        .await
        .expect("the operator boundary should answer");
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);

    let refused = notices_until_quiet(&mut changes).await;
    assert!(
        refused.is_empty(),
        "a refused post raised notices: {refused:?}",
        refused = names(&refused)
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn notices_for_one_organization_never_reach_anothers_subscribers() {
    let kestrel = Kestrel::boot().await;
    declare_organization(&kestrel, "acme").await;
    declare_organization(&kestrel, "other").await;

    let mut other = Stream::open(&kestrel, "other").await;
    assert_eq!(other.next(QUIET).await.expect("an open event").name, "open");

    kestrel.open_workspace("acme", "kestrel", "builder").await;
    let theirs = notices_until_quiet(&mut other).await;
    assert!(
        theirs.is_empty(),
        "acme's change reached other's subscriber: {theirs:?}",
        theirs = names(&theirs)
    );

    let ours = kestrel.open_workspace("other", "kestrel", "builder").await;
    let noticed = notices_until_quiet(&mut other).await;
    assert!(
        names_changed(&noticed, "workspace", Some(&ours.id.to_string())),
        "other's own change raised no notice: {noticed:?}",
        noticed = names(&noticed)
    );

    kestrel.teardown().await;
}

#[test]
fn the_published_operator_document_describes_the_change_stream_and_its_events() {
    let document: Value = serde_json::from_str(
        &fs::read_to_string(support::crate_root().join("../../openapi/operator.json"))
            .expect("a readable openapi document"),
    )
    .expect("valid json");

    let stream = &document["paths"][operator::CHANGES]["get"]["responses"]["200"]["content"]["text/event-stream"]
        ["schema"];
    let events = resolve(
        &document,
        stream["$ref"].as_str().expect("a schema reference"),
    );

    let description = events["description"]
        .as_str()
        .expect("a description of the events");
    for name in ["open", "change", "resync"] {
        assert!(
            description.contains(name),
            "the document describes no {name} event: {description}"
        );
    }

    let schemas = events["oneOf"].as_array().expect("the documented events");
    let change = schemas
        .iter()
        .map(|schema| resolve(&document, schema["$ref"].as_str().expect("a reference")))
        .find(|schema| {
            schema["oneOf"].as_array().is_some_and(|options| {
                options
                    .iter()
                    .any(|option| option["properties"]["resource"]["const"] == "workspace")
            })
        })
        .expect("the document describes a change");

    let resources: Vec<&str> = change["oneOf"]
        .as_array()
        .expect("the changed resources")
        .iter()
        .map(|option| {
            option["properties"]["resource"]["const"]
                .as_str()
                .expect("a named resource")
        })
        .collect();
    assert_eq!(resources, ["workspace", "session", "queue"]);

    let queue = &change["oneOf"][2];
    assert!(
        queue["properties"].get("id").is_none() && queue["required"] == json!(["resource"]),
        "the document says a queue notice carries an id"
    );
}

fn resolve<'a>(document: &'a Value, reference: &str) -> &'a Value {
    reference
        .trim_start_matches("#/")
        .split('/')
        .fold(document, |document, step| &document[step])
}

#[tokio::test]
async fn live_usage_raises_no_change_notice() {
    let kestrel = Kestrel::dispatching_to(
        supervisor::binary(),
        &scripted_agent::playing(Script::BurstsUsage),
    )
    .await;
    declare_organization(&kestrel, "acme").await;
    let workspace = kestrel.open_workspace("acme", "kestrel", "builder").await;

    let mut changes = Stream::open(&kestrel, "acme").await;
    assert_eq!(
        changes.next(QUIET).await.expect("an open event").name,
        "open"
    );

    let session = kestrel.enqueue_session(workspace.id).await;
    let started = notices_until_quiet(&mut changes).await;
    assert!(
        names_changed(&started, "session", Some(&session.id.to_string())),
        "the started session raised no session notice: {started:?}",
        started = names(&started)
    );

    // The burst and its trailing-edge report land after the start's notices settled; neither
    // writes a row, so nothing more may arrive.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    let mut late = Vec::new();
    while tokio::time::Instant::now() < deadline {
        if let Some(frame) = changes.next(QUIET).await {
            late.push(frame);
        }
    }
    assert!(
        late.is_empty(),
        "live usage raised a notice: {late:?}",
        late = names(&late)
    );

    kestrel.teardown().await;
}
