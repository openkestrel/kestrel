use crate::support;

use std::time::Duration;

use kestrel::domain::Workspace;
use kestrel::operator;
use kestrel::work::{Report, Reported};
use reqwest::StatusCode;
use reqwest::header::ORIGIN;
use serde_json::{Value, json};
use support::Kestrel;
use support::link_client::Link;

const PATIENCE: Duration = Duration::from_secs(10);
/// Longer than the hubs' poll and coalescing windows, so a quiet stream is really quiet.
const QUIET: Duration = Duration::from_millis(600);

struct Connection {
    response: reqwest::Response,
    buffered: String,
}

#[derive(Debug)]
struct Frame {
    name: String,
    subscription: String,
    cursor: Option<String>,
    data: Value,
}

impl Connection {
    async fn open(kestrel: &Kestrel, token: &str) -> Self {
        let response = reqwest::Client::new()
            .get(stream_url(kestrel, token))
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

    async fn until_quiet(&mut self) -> Vec<Frame> {
        let mut frames = Vec::new();
        while let Some(frame) = self.next(QUIET).await {
            frames.push(frame);
        }

        frames
    }

    /// The next frame for `subscription` named `name`, passing over everything else.
    async fn until(&mut self, subscription: &str, name: &str) -> Frame {
        loop {
            let frame = self
                .next(PATIENCE)
                .await
                .unwrap_or_else(|| panic!("no {name} for {subscription} before the deadline"));
            if frame.subscription == subscription && frame.name == name {
                return frame;
            }
        }
    }

    /// The first frame for each (subscription, name), in whichever order they arrive.
    async fn each(&mut self, wanted: &[(&str, &str)]) -> Vec<Frame> {
        let mut found: Vec<Option<Frame>> = wanted.iter().map(|_| None).collect();
        while found.iter().any(Option::is_none) {
            let frame = self
                .next(PATIENCE)
                .await
                .unwrap_or_else(|| panic!("not all of {wanted:?} before the deadline: {found:?}"));
            if let Some(slot) = wanted.iter().position(|(subscription, name)| {
                frame.subscription == *subscription && frame.name == *name
            }) && found[slot].is_none()
            {
                found[slot] = Some(frame);
            }
        }

        found.into_iter().flatten().collect()
    }

    fn take_frame(&mut self) -> Option<Frame> {
        loop {
            let end = self.buffered.find("\n\n")?;
            let frame: String = self.buffered.drain(..end + 2).collect();

            let mut name = String::new();
            let mut data = String::new();
            for line in frame.lines() {
                assert!(
                    !line.starts_with("id:"),
                    "the stream carries several sequences, so no event has an sse id: {frame}"
                );
                if let Some(value) = line.strip_prefix("event:") {
                    name = value.trim().to_owned();
                } else if let Some(value) = line.strip_prefix("data:") {
                    data.push_str(value.trim());
                }
            }
            if name.is_empty() && data.is_empty() {
                continue;
            }

            let mut envelope: Value = serde_json::from_str(&data)
                .unwrap_or_else(|error| panic!("{data} is not JSON: {error}"));
            return Some(Frame {
                name,
                subscription: envelope["subscription"]
                    .as_str()
                    .unwrap_or_else(|| panic!("{envelope} names no subscription"))
                    .to_owned(),
                cursor: envelope
                    .get("cursor")
                    .map(|cursor| cursor.as_str().expect("a cursor string").to_owned()),
                data: envelope["data"].take(),
            });
        }
    }
}

fn stream_url(kestrel: &Kestrel, token: &str) -> String {
    format!(
        "{}{}",
        kestrel.operator(),
        operator::STREAM.replace("{token}", token)
    )
}

fn subscription_url(kestrel: &Kestrel, token: &str, id: &str) -> String {
    format!(
        "{}{}",
        kestrel.operator(),
        operator::STREAM_SUBSCRIPTION
            .replace("{token}", token)
            .replace("{id}", id)
    )
}

async fn reserve(kestrel: &Kestrel) -> String {
    let response = reqwest::Client::new()
        .put(format!("{}{}", kestrel.operator(), operator::STREAMS))
        .send()
        .await
        .expect("the operator boundary should answer");
    assert_eq!(response.status(), StatusCode::CREATED);
    let reserved: Value = response.json().await.expect("a reservation as JSON");

    reserved["token"].as_str().expect("a token").to_owned()
}

async fn subscribe(kestrel: &Kestrel, token: &str, id: &str, subscription: Value) -> StatusCode {
    reqwest::Client::new()
        .put(subscription_url(kestrel, token, id))
        .json(&subscription)
        .send()
        .await
        .expect("the operator boundary should answer")
        .status()
}

async fn unsubscribe(kestrel: &Kestrel, token: &str, id: &str) -> StatusCode {
    reqwest::Client::new()
        .delete(subscription_url(kestrel, token, id))
        .send()
        .await
        .expect("the operator boundary should answer")
        .status()
}

fn notices() -> Value {
    json!({"kind": "notices", "organization": "acme"})
}

fn transcript(workspace: &Workspace) -> Value {
    json!({"kind": "transcript", "organization": "acme", "workspace": workspace.id.to_string()})
}

async fn declared(kestrel: &Kestrel) {
    let organization = kestrel.declare_organization("acme").await;
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

async fn an_open_workspace(kestrel: &Kestrel) -> Workspace {
    declared(kestrel).await;
    kestrel.open_workspace("acme", "kestrel", "builder").await
}

#[tokio::test]
async fn one_connection_interleaves_notices_and_a_transcript_by_subscription() {
    let kestrel = Kestrel::boot().await;
    let workspace = an_open_workspace(&kestrel).await;
    kestrel.post(workspace.id, "operator", "go").await;

    let token = reserve(&kestrel).await;
    let mut connection = Connection::open(&kestrel, &token).await;
    assert_eq!(
        subscribe(&kestrel, &token, "org", notices()).await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        subscribe(&kestrel, &token, "ws", transcript(&workspace)).await,
        StatusCode::NO_CONTENT
    );

    let frames = connection.until_quiet().await;
    let open = frames
        .iter()
        .find(|frame| frame.subscription == "org")
        .expect("the notices subscription opened");
    assert_eq!(open.name, "open");
    assert_eq!(open.cursor, None, "a notice is never a cursor");
    let entries: Vec<&Frame> = frames
        .iter()
        .filter(|frame| frame.subscription == "ws" && frame.name == "entry")
        .collect();
    assert!(!entries.is_empty(), "the transcript replayed nothing");
    for entry in &entries {
        assert_eq!(
            entry.cursor.as_deref(),
            Some(format!("{}:{}", workspace.id, entry.data["seq"]).as_str()),
            "an entry carries its own sequence's cursor"
        );
    }
    for frame in frames.iter().filter(|frame| {
        matches!(
            frame.name.as_str(),
            "session_state" | "follower" | "presence"
        )
    }) {
        assert_eq!(frame.subscription, "ws");
        assert_eq!(frame.cursor, None, "{} moved a cursor", frame.name);
    }

    kestrel
        .post_while_busy(workspace.id, "operator", "more")
        .await;
    kestrel.open_workspace("acme", "kestrel", "builder").await;
    let [appended, changed] = &connection.each(&[("ws", "entry"), ("org", "change")]).await[..]
    else {
        unreachable!()
    };
    assert_eq!(
        appended.data["seq"].as_i64(),
        Some(entries.last().unwrap().data["seq"].as_i64().unwrap() + 1)
    );
    assert_eq!(changed.cursor, None);

    kestrel.teardown().await;
}

/// The last cursor each subscription was handed, and the last entry seq among them.
fn last_seen(frames: &[Frame], subscription: &str) -> (String, i64) {
    let cursor = frames
        .iter()
        .rev()
        .filter(|frame| frame.subscription == subscription)
        .find_map(|frame| frame.cursor.clone())
        .unwrap_or_else(|| panic!("{subscription} was handed no cursor"));
    let seq = frames
        .iter()
        .rev()
        .find(|frame| frame.subscription == subscription && frame.name == "entry")
        .and_then(|frame| frame.data["seq"].as_i64())
        .unwrap_or_else(|| panic!("{subscription} replayed no entry"));

    (cursor, seq)
}

async fn forgotten(kestrel: &Kestrel, token: &str) {
    let deadline = tokio::time::Instant::now() + PATIENCE;
    while subscribe(kestrel, token, "probe", notices()).await != StatusCode::NOT_FOUND {
        assert!(
            tokio::time::Instant::now() < deadline,
            "the reservation outlived its connection"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

#[tokio::test]
async fn two_transcripts_resume_from_their_own_cursors_on_a_new_reservation() {
    let kestrel = Kestrel::boot().await;
    declared(&kestrel).await;
    let first = kestrel.open_workspace("acme", "kestrel", "builder").await;
    let second = kestrel.open_workspace("acme", "kestrel", "builder").await;
    kestrel.post(first.id, "operator", "go").await;
    kestrel.post(second.id, "operator", "go").await;
    kestrel
        .post_while_busy(second.id, "operator", "and more")
        .await;

    let token = reserve(&kestrel).await;
    let mut connection = Connection::open(&kestrel, &token).await;
    subscribe(&kestrel, &token, "first", transcript(&first)).await;
    subscribe(&kestrel, &token, "second", transcript(&second)).await;
    let frames = connection.until_quiet().await;
    let (first_cursor, first_seq) = last_seen(&frames, "first");
    let (second_cursor, second_seq) = last_seen(&frames, "second");
    assert_ne!(
        first_seq, second_seq,
        "the two Transcripts need distinct positions"
    );
    drop(connection);
    forgotten(&kestrel, &token).await;

    let token = reserve(&kestrel).await;
    let mut connection = Connection::open(&kestrel, &token).await;
    let mut resumed = transcript(&first);
    resumed["after"] = json!(first_cursor);
    subscribe(&kestrel, &token, "first", resumed).await;
    let mut resumed = transcript(&second);
    resumed["after"] = json!(second_cursor);
    subscribe(&kestrel, &token, "second", resumed).await;
    let replayed = connection.until_quiet().await;
    assert!(
        !replayed.iter().any(|frame| frame.name == "entry"),
        "a resumed subscription replayed what it had seen: {replayed:?}"
    );

    kestrel
        .post_while_busy(first.id, "operator", "first again")
        .await;
    kestrel
        .post_while_busy(second.id, "operator", "second again")
        .await;
    let [first_entry, second_entry] = &connection
        .each(&[("first", "entry"), ("second", "entry")])
        .await[..]
    else {
        unreachable!()
    };
    assert_eq!(first_entry.data["seq"].as_i64(), Some(first_seq + 1));
    assert_eq!(second_entry.data["seq"].as_i64(), Some(second_seq + 1));

    kestrel.teardown().await;
}

#[tokio::test]
async fn an_unknown_or_unconnected_reservation_is_not_found_and_an_open_one_opens_once() {
    let lease = Duration::from_secs(2);
    let kestrel = Kestrel::boot_with_follow_lease(lease).await;
    declared(&kestrel).await;
    let unknown = uuid::Uuid::now_v7().to_string();

    for status in [
        reqwest::Client::new()
            .get(stream_url(&kestrel, &unknown))
            .send()
            .await
            .unwrap()
            .status(),
        subscribe(&kestrel, &unknown, "org", notices()).await,
        unsubscribe(&kestrel, &unknown, "org").await,
        subscribe(&kestrel, "not-a-token", "org", notices()).await,
    ] {
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    let open = reserve(&kestrel).await;
    let _connection = Connection::open(&kestrel, &open).await;
    let again = reqwest::Client::new()
        .get(stream_url(&kestrel, &open))
        .send()
        .await
        .unwrap();
    assert_eq!(again.status(), StatusCode::CONFLICT);

    let idle = reserve(&kestrel).await;
    assert_eq!(
        subscribe(&kestrel, &idle, "org", notices()).await,
        StatusCode::NO_CONTENT,
        "a subscription may come before its connection"
    );
    tokio::time::sleep(lease + Duration::from_secs(1)).await;
    assert_eq!(
        subscribe(&kestrel, &idle, "org", notices()).await,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        reqwest::Client::new()
            .get(stream_url(&kestrel, &idle))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        subscribe(&kestrel, &open, "org", notices()).await,
        StatusCode::NO_CONTENT,
        "an open reservation never expires"
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_subscription_is_refused_what_the_per_resource_read_refuses() {
    let kestrel = Kestrel::boot().await;
    let workspace = an_open_workspace(&kestrel).await;
    let token = reserve(&kestrel).await;

    let mut nameless = transcript(&workspace);
    nameless.as_object_mut().unwrap().remove("workspace");
    let mut elsewhere = transcript(&workspace);
    elsewhere["after"] = json!(format!("{}:1", uuid::Uuid::now_v7()));
    let mut unreadable = transcript(&workspace);
    unreadable["after"] = json!("nowhere");
    let mut unknown_kind = transcript(&workspace);
    unknown_kind["kinds"] = json!("gossip");
    for (subscription, status) in [
        (nameless, StatusCode::BAD_REQUEST),
        (unreadable, StatusCode::BAD_REQUEST),
        (elsewhere, StatusCode::BAD_REQUEST),
        (unknown_kind, StatusCode::BAD_REQUEST),
        (
            json!({"kind": "gossip", "organization": "acme"}),
            StatusCode::BAD_REQUEST,
        ),
        (
            json!({"kind": "notices", "organization": "nobody"}),
            StatusCode::NOT_FOUND,
        ),
        (
            json!({"kind": "transcript", "organization": "acme", "workspace": "nothing"}),
            StatusCode::NOT_FOUND,
        ),
    ] {
        assert_eq!(
            subscribe(&kestrel, &token, "sub", subscription.clone()).await,
            status,
            "{subscription}"
        );
    }

    kestrel.teardown().await;
}

async fn watched(kestrel: &Kestrel, workspace: &Workspace) -> reqwest::Response {
    reqwest::Client::new()
        .get(format!(
            "{}{}",
            kestrel.operator(),
            operator::TRANSCRIPT
                .replace("{organization}", "acme")
                .replace("{workspace}", &workspace.id.to_string())
        ))
        .send()
        .await
        .unwrap()
}

/// The presence a per-resource follow of `workspace` is handed next.
struct Watcher(reqwest::Response, String);

impl Watcher {
    async fn of(kestrel: &Kestrel, workspace: &Workspace) -> Self {
        Self(watched(kestrel, workspace).await, String::new())
    }

    async fn presence(&mut self) -> Value {
        let deadline = tokio::time::Instant::now() + PATIENCE;
        loop {
            if let Some(end) = self.1.find("\n\n") {
                let frame: String = self.1.drain(..end + 2).collect();
                if frame.contains("event: presence") || frame.contains("event:presence") {
                    let data = frame
                        .lines()
                        .find_map(|line| line.strip_prefix("data:"))
                        .expect("presence data");
                    return serde_json::from_str(data.trim()).unwrap();
                }
                continue;
            }
            let chunk = tokio::time::timeout_at(deadline, self.0.chunk())
                .await
                .expect("a presence before the deadline")
                .unwrap()
                .expect("the follow stayed open");
            self.1.push_str(&String::from_utf8_lossy(&chunk));
        }
    }
}

async fn renew(kestrel: &Kestrel, workspace: &Workspace, follower: &str) -> StatusCode {
    reqwest::Client::new()
        .post(format!(
            "{}{}",
            kestrel.operator(),
            operator::FOLLOWER_LEASE
                .replace("{organization}", "acme")
                .replace("{workspace}", &workspace.id.to_string())
                .replace("{id}", follower)
        ))
        .send()
        .await
        .unwrap()
        .status()
}

#[tokio::test]
async fn a_transcript_subscription_is_a_follower_until_replaced_unsubscribed_or_dropped() {
    let kestrel = Kestrel::boot().await;
    declared(&kestrel).await;
    let workspace = kestrel.open_workspace("acme", "kestrel", "builder").await;
    let elsewhere = kestrel.open_workspace("acme", "kestrel", "builder").await;
    let mut watcher = Watcher::of(&kestrel, &workspace).await;
    assert_eq!(
        watcher.presence().await,
        json!({"named": [], "anonymous": 1})
    );

    let token = reserve(&kestrel).await;
    let mut connection = Connection::open(&kestrel, &token).await;
    let follow = |id: &str| {
        let id = id.to_owned();
        let token = token.clone();
        let kestrel = &kestrel;
        let workspace = &workspace;
        async move { subscribe(kestrel, &token, &id, transcript(workspace)).await }
    };

    assert_eq!(follow("ws").await, StatusCode::NO_CONTENT);
    let follower = connection.until("ws", "follower").await;
    assert_eq!(follower.cursor, None);
    let id = follower.data["id"].as_str().unwrap().to_owned();
    assert_eq!(follower.data["lease_seconds"], 60);
    let presence = connection.until("ws", "presence").await;
    assert_eq!(presence.cursor, None);
    assert_eq!(presence.data, json!({"named": [], "anonymous": 2}));
    assert_eq!(
        watcher.presence().await,
        json!({"named": [], "anonymous": 2})
    );
    assert_eq!(
        renew(&kestrel, &workspace, &id).await,
        StatusCode::NO_CONTENT
    );

    assert_eq!(
        subscribe(&kestrel, &token, "ws", transcript(&elsewhere)).await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        watcher.presence().await,
        json!({"named": [], "anonymous": 1}),
        "replacing the subscription left its follower behind"
    );
    assert_eq!(
        renew(&kestrel, &workspace, &id).await,
        StatusCode::NOT_FOUND
    );

    assert_eq!(follow("again").await, StatusCode::NO_CONTENT);
    let id = connection.until("again", "follower").await.data["id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(
        watcher.presence().await,
        json!({"named": [], "anonymous": 2})
    );
    assert_eq!(
        unsubscribe(&kestrel, &token, "again").await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        watcher.presence().await,
        json!({"named": [], "anonymous": 1}),
        "unsubscribing left its follower behind"
    );
    assert_eq!(
        renew(&kestrel, &workspace, &id).await,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        unsubscribe(&kestrel, &token, "again").await,
        StatusCode::NO_CONTENT,
        "an ended subscription is already ended"
    );

    assert_eq!(follow("last").await, StatusCode::NO_CONTENT);
    connection.until("last", "follower").await;
    assert_eq!(
        watcher.presence().await,
        json!({"named": [], "anonymous": 2})
    );
    drop(connection);
    assert_eq!(
        watcher.presence().await,
        json!({"named": [], "anonymous": 1}),
        "a dropped connection left its follower behind"
    );
    forgotten(&kestrel, &token).await;

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_transcript_subscription_follows_under_the_participant_it_names() {
    let kestrel = Kestrel::boot().await;
    let workspace = an_open_workspace(&kestrel).await;
    let token = reserve(&kestrel).await;
    let mut connection = Connection::open(&kestrel, &token).await;

    let mut named = transcript(&workspace);
    named["participant"] = json!("jack");
    assert_eq!(
        subscribe(&kestrel, &token, "ws", named).await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        connection.until("ws", "presence").await.data,
        json!({"named": ["jack"], "anonymous": 0})
    );

    let mut agent = transcript(&workspace);
    agent["participant"] = json!("builder");
    let response = reqwest::Client::new()
        .put(subscription_url(&kestrel, &token, "agent"))
        .json(&agent)
        .send()
        .await
        .expect("the operator boundary should answer");
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let refusal: Value = response.json().await.expect("a refusal as JSON");
    assert_eq!(refusal["field"], "participant");

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_lapsed_follower_closes_its_connection_and_forgets_the_reservation() {
    let kestrel = Kestrel::boot_with_follow_lease(Duration::from_secs(2)).await;
    let workspace = an_open_workspace(&kestrel).await;
    let token = reserve(&kestrel).await;
    let mut connection = Connection::open(&kestrel, &token).await;
    subscribe(&kestrel, &token, "org", notices()).await;
    subscribe(&kestrel, &token, "ws", transcript(&workspace)).await;
    connection.until("ws", "follower").await;

    loop {
        if connection.next(PATIENCE).await.is_none() {
            break;
        }
    }
    forgotten(&kestrel, &token).await;

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_reservation_holds_a_bounded_number_of_subscriptions() {
    let kestrel = Kestrel::boot().await;
    declared(&kestrel).await;
    let token = reserve(&kestrel).await;

    for id in 0..kestrel::stream::MOST_SUBSCRIPTIONS {
        assert_eq!(
            subscribe(&kestrel, &token, &id.to_string(), notices()).await,
            StatusCode::NO_CONTENT
        );
    }
    assert_eq!(
        subscribe(&kestrel, &token, "one-more", notices()).await,
        StatusCode::CONFLICT
    );
    assert_eq!(
        subscribe(&kestrel, &token, "0", notices()).await,
        StatusCode::NO_CONTENT,
        "replacing a subscription holds no more"
    );
    unsubscribe(&kestrel, &token, "0").await;
    assert_eq!(
        subscribe(&kestrel, &token, "one-more", notices()).await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        subscribe(&kestrel, &token, &"x".repeat(65), notices()).await,
        StatusCode::BAD_REQUEST
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn the_stream_routes_refuse_a_foreign_origin() {
    let kestrel = Kestrel::boot().await;
    declared(&kestrel).await;
    let token = reserve(&kestrel).await;
    let client = reqwest::Client::new();

    for request in [
        client.put(format!("{}{}", kestrel.operator(), operator::STREAMS)),
        client.get(stream_url(&kestrel, &token)),
        client
            .put(subscription_url(&kestrel, &token, "org"))
            .json(&notices()),
        client.delete(subscription_url(&kestrel, &token, "org")),
    ] {
        let response = request
            .header(ORIGIN, "https://evil.example")
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_transcript_subscription_keeps_activity_and_session_state_semantics() {
    let kestrel = Kestrel::boot().await;
    declared(&kestrel).await;
    let workspace = kestrel.open_workspace("acme", "kestrel", "builder").await;
    let (session, on) = kestrel.dispatch_to_the_link(workspace.id).await;
    let link = Link::to(&kestrel.link());
    let _instructions = link.open(&on.instance, &on.credential, None).await;
    let token = reserve(&kestrel).await;
    let mut connection = Connection::open(&kestrel, &token).await;
    subscribe(&kestrel, &token, "ws", transcript(&workspace)).await;
    connection.until("ws", "presence").await;

    let transient = Reported {
        session: Some(session.id),
        seq: None,
        report: Report::SessionState {
            tools: vec![kestrel::live_work::RunningTool {
                call_id: "running".into(),
                title: "read source".into(),
                tool_kind: "read".into(),
                status: kestrel::log::ToolStatus::InProgress,
                started_at: "2026-09-29T12:00:00Z".parse().unwrap(),
            }],
            units: vec![],
            message_buffering: true,
            thought_buffering: false,
            usage: None,
            last_activity_at: None,
        },
    };
    link.report(&on.instance, Some(&on.credential), &transient)
        .await;
    let state = connection.until("ws", "session_state").await;
    assert_eq!(
        state.cursor, None,
        "transient Session state moved the cursor"
    );
    assert_eq!(state.data["tools"][0]["title"], "read source");

    let completion = kestrel::log::Completion::at("2026-09-29T12:00:00Z".parse().unwrap());
    let thought = Reported {
        session: Some(session.id),
        seq: Some(1),
        report: Report::Thought {
            text: "reasoning".into(),
            completion: completion.clone(),
        },
    };
    link.report(&on.instance, Some(&on.credential), &thought)
        .await;
    let open = connection.until("ws", "activity").await;
    assert_eq!(open.data["closed"], false);
    let first_seq = open.data["first_seq"].as_i64().unwrap();
    assert_eq!(
        open.cursor,
        Some(format!("{}:{}", workspace.id, open.data["last_seq"])),
        "an Activity carries the cursor past what it summarizes"
    );
    let said = Reported {
        session: Some(session.id),
        seq: Some(2),
        report: Report::Said {
            message: "done".into(),
            completion,
        },
    };
    link.report(&on.instance, Some(&on.credential), &said).await;
    let closed = connection.until("ws", "activity").await;
    assert_eq!(closed.data["closed"], true);
    assert_eq!(closed.data["first_seq"], first_seq);
    assert_eq!(
        connection.until("ws", "entry").await.data["entry"]["message"],
        "done"
    );

    drop(connection);
    let token = reserve(&kestrel).await;
    let mut connection = Connection::open(&kestrel, &token).await;
    let mut resumed = transcript(&workspace);
    resumed["after"] = json!(open.cursor.unwrap());
    subscribe(&kestrel, &token, "ws", resumed).await;
    let replaced = connection.until("ws", "activity").await;
    assert_eq!(replaced.data["first_seq"], first_seq);
    assert_eq!(replaced.data["closed"], true);
    assert_eq!(
        connection.until("ws", "entry").await.data["entry"]["message"],
        "done"
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_subscription_held_over_and_over_before_its_connection_opens_once() {
    let kestrel = Kestrel::boot().await;
    declared(&kestrel).await;
    let token = reserve(&kestrel).await;
    for _ in 0..5 {
        subscribe(&kestrel, &token, "org", notices()).await;
    }
    subscribe(&kestrel, &token, "gone", notices()).await;
    unsubscribe(&kestrel, &token, "gone").await;

    let mut connection = Connection::open(&kestrel, &token).await;
    let frames = connection.until_quiet().await;
    assert_eq!(
        frames
            .iter()
            .map(|frame| (frame.subscription.as_str(), frame.name.as_str()))
            .collect::<Vec<_>>(),
        [("org", "open")]
    );

    kestrel.teardown().await;
}
