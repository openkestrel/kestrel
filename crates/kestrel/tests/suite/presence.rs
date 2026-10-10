use crate::support;

use std::time::Duration;

use kestrel::domain::Workspace;
use kestrel::operator;
use reqwest::StatusCode;
use serde_json::{Value, json};
use support::Kestrel;
use support::fixture::Fixture;

/// Short enough for a test to watch pass, long enough that scheduling cannot race it.
const LEASE: Duration = Duration::from_secs(5);
const PATIENCE: Duration = Duration::from_secs(10);
/// Longer than the hub's 100 ms poll, so a quiet stream is really quiet.
const QUIET: Duration = Duration::from_millis(600);

struct Stream {
    response: reqwest::Response,
    buffered: String,
}

struct Frame {
    name: String,
    data: Value,
    id: Option<String>,
}

impl Stream {
    async fn follow(kestrel: &Kestrel, workspace: &Workspace, as_name: Option<&str>) -> Self {
        Self::get(kestrel, workspace, as_name, None).await
    }

    async fn follow_from(
        kestrel: &Kestrel,
        workspace: &Workspace,
        as_name: Option<&str>,
        cursor: &str,
    ) -> Self {
        Self::get(kestrel, workspace, as_name, Some(cursor)).await
    }

    async fn get(
        kestrel: &Kestrel,
        workspace: &Workspace,
        as_name: Option<&str>,
        cursor: Option<&str>,
    ) -> Self {
        let mut path = format!("{}?follow=true", transcript("acme", workspace));
        if let Some(name) = as_name {
            path.push_str(&format!("&as={name}"));
        }
        let mut request = reqwest::Client::new().get(format!("{}{path}", kestrel.operator()));
        if let Some(cursor) = cursor {
            request = request.header("last-event-id", cursor);
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

    async fn follower(&mut self) -> (String, u64) {
        loop {
            let frame = self
                .next(PATIENCE)
                .await
                .expect("a follower event before the deadline");
            match frame.name.as_str() {
                "entry" | "cursor" | "activity" | "session_state" => continue,
                "follower" => {
                    assert!(frame.id.is_none(), "a follower event carries no id");

                    return (
                        frame.data["id"].as_str().expect("a follower id").to_owned(),
                        frame.data["lease_seconds"]
                            .as_u64()
                            .expect("a lease in seconds"),
                    );
                }
                other => panic!("expected a follower event, got {other}"),
            }
        }
    }

    async fn presence(&mut self) -> Value {
        loop {
            let frame = self
                .next(PATIENCE)
                .await
                .expect("a presence event before the deadline");
            match frame.name.as_str() {
                "entry" | "cursor" | "activity" | "session_state" => continue,
                "presence" => {
                    assert!(frame.id.is_none(), "a presence event carries no id");

                    return frame.data;
                }
                other => panic!("expected a presence event, got {other}"),
            }
        }
    }

    async fn presence_within(&mut self, patience: Duration) -> Option<Value> {
        let deadline = tokio::time::Instant::now() + patience;

        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            let frame = self.next(remaining).await?;
            match frame.name.as_str() {
                "entry" | "cursor" | "activity" | "session_state" => continue,
                "presence" => return Some(frame.data),
                other => panic!("expected a presence event, got {other}"),
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
            if name.is_empty() && data.is_empty() {
                continue;
            }

            let data = if data.is_empty() {
                Value::Null
            } else {
                serde_json::from_str(&data)
                    .unwrap_or_else(|error| panic!("{data} is not JSON: {error}"))
            };
            return Some(Frame { name, data, id });
        }
    }
}

async fn renew(kestrel: &Kestrel, workspace: &Workspace, id: &str) -> StatusCode {
    reqwest::Client::new()
        .post(lease_url(kestrel, workspace, id))
        .send()
        .await
        .expect("the operator boundary should answer")
        .status()
}

fn lease_url(kestrel: &Kestrel, workspace: &Workspace, id: &str) -> String {
    let path = operator::FOLLOWER_LEASE
        .replace("{organization}", "acme")
        .replace("{workspace}", &workspace.id.to_string())
        .replace("{id}", id);

    format!("{}{path}", kestrel.operator())
}

struct Keeping(tokio::task::JoinHandle<()>);

impl Drop for Keeping {
    fn drop(&mut self) {
        self.0.abort();
    }
}

fn keeping(kestrel: &Kestrel, workspace: &Workspace, id: &str) -> Keeping {
    let url = lease_url(kestrel, workspace, id);

    Keeping(tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;
            let _ = reqwest::Client::new().post(url.clone()).send().await;
        }
    }))
}

fn transcript(organization: &str, workspace: &Workspace) -> String {
    operator::TRANSCRIPT
        .replace("{organization}", organization)
        .replace("{workspace}", &workspace.id.to_string())
}

async fn an_open_workspace(kestrel: &Kestrel) -> Workspace {
    Fixture::acme().open(kestrel).await
}

#[tokio::test]
async fn a_follow_receives_its_follower_event_and_a_presence_snapshot_that_includes_it() {
    let kestrel = Kestrel::boot_with_follow_lease(LEASE).await;
    let workspace = an_open_workspace(&kestrel).await;

    let mut ada = Stream::follow(&kestrel, &workspace, Some("Ada")).await;
    let (id, lease_seconds) = ada.follower().await;
    assert_eq!(lease_seconds, LEASE.as_secs());
    assert_eq!(
        ada.presence().await,
        json!({"named": ["Ada"], "anonymous": 0})
    );

    assert_eq!(
        renew(&kestrel, &workspace, &id).await,
        StatusCode::NO_CONTENT
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn two_follows_as_ada_and_one_anonymous_show_the_whole_set() {
    let kestrel = Kestrel::boot_with_follow_lease(LEASE).await;
    let workspace = an_open_workspace(&kestrel).await;

    let mut first = Stream::follow(&kestrel, &workspace, Some("Ada")).await;
    first.follower().await;
    assert_eq!(
        first.presence().await,
        json!({"named": ["Ada"], "anonymous": 0})
    );

    let mut second = Stream::follow(&kestrel, &workspace, Some("Ada")).await;
    second.follower().await;
    assert_eq!(
        second.presence().await,
        json!({"named": ["Ada"], "anonymous": 0})
    );
    assert_eq!(
        first.presence().await,
        json!({"named": ["Ada"], "anonymous": 0})
    );

    let mut anonymous = Stream::follow(&kestrel, &workspace, None).await;
    anonymous.follower().await;
    assert_eq!(
        anonymous.presence().await,
        json!({"named": ["Ada"], "anonymous": 1})
    );
    assert_eq!(
        second.presence().await,
        json!({"named": ["Ada"], "anonymous": 1})
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn closing_one_of_adas_follows_leaves_her_named_and_closing_both_removes_her() {
    let kestrel = Kestrel::boot_with_follow_lease(LEASE).await;
    let workspace = an_open_workspace(&kestrel).await;

    let mut first = Stream::follow(&kestrel, &workspace, Some("Ada")).await;
    first.follower().await;
    first.presence().await;
    let mut second = Stream::follow(&kestrel, &workspace, Some("Ada")).await;
    second.follower().await;
    second.presence().await;
    let mut watcher = Stream::follow(&kestrel, &workspace, None).await;
    watcher.follower().await;
    watcher.presence().await;
    first.presence().await;
    second.presence().await;

    drop(second);
    assert_eq!(
        first.presence().await,
        json!({"named": ["Ada"], "anonymous": 1}),
        "one of Ada's follows left and she is still named"
    );
    assert_eq!(
        watcher.presence().await,
        json!({"named": ["Ada"], "anonymous": 1})
    );

    drop(first);
    assert_eq!(
        watcher.presence().await,
        json!({"named": [], "anonymous": 1}),
        "both of Ada's follows left and she is gone"
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_follow_that_stops_renewing_is_removed_and_its_stream_closes() {
    let kestrel = Kestrel::boot_with_follow_lease(LEASE).await;
    let workspace = an_open_workspace(&kestrel).await;

    let mut ada = Stream::follow(&kestrel, &workspace, Some("Ada")).await;
    let (id, _) = ada.follower().await;
    ada.presence().await;

    let mut watcher = Stream::follow(&kestrel, &workspace, None).await;
    let (watcher_id, _) = watcher.follower().await;
    let _keeping = keeping(&kestrel, &workspace, &watcher_id);
    assert_eq!(
        watcher.presence().await,
        json!({"named": ["Ada"], "anonymous": 1})
    );
    assert_eq!(
        ada.presence().await,
        json!({"named": ["Ada"], "anonymous": 1})
    );

    let closed = ada.next(LEASE + PATIENCE).await;
    assert!(closed.is_none(), "the expired follow's stream stayed open");
    assert_eq!(
        watcher.presence().await,
        json!({"named": [], "anonymous": 1}),
        "the expired follower stayed in the set"
    );

    assert_eq!(
        renew(&kestrel, &workspace, &id).await,
        StatusCode::NOT_FOUND,
        "renewing an expired follower registered it again"
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_reconnect_gets_a_fresh_snapshot_and_entries_resume_from_its_cursor() {
    let kestrel = Kestrel::boot_with_follow_lease(LEASE).await;
    let workspace = an_open_workspace(&kestrel).await;
    kestrel.post(workspace.id, "operator", "go").await;

    let mut first = Stream::follow(&kestrel, &workspace, Some("Ada")).await;
    let mut seqs = Vec::new();
    let mut cursor = None;
    loop {
        let frame = first
            .next(PATIENCE)
            .await
            .expect("a frame before the deadline");
        match frame.name.as_str() {
            "entry" => {
                seqs.push(frame.data["seq"].as_i64().expect("a seq"));
                cursor = frame.id;
            }
            "follower" => {}
            "activity" | "cursor" | "session_state" => {}
            "presence" => break,
            other => panic!("a follow delivered {other} before its presence"),
        }
    }
    assert!(!seqs.is_empty(), "the follow replayed no entries");
    let last = *seqs.last().expect("the last replayed seq");
    drop(first);

    let mut again = Stream::follow_from(
        &kestrel,
        &workspace,
        Some("Ada"),
        cursor.as_deref().unwrap(),
    )
    .await;
    again.follower().await;
    assert_eq!(
        again.presence().await,
        json!({"named": ["Ada"], "anonymous": 0}),
        "a reconnect got a fresh snapshot"
    );
    assert!(
        again.next(QUIET).await.is_none(),
        "a reconnect replayed entries it had already delivered"
    );

    kestrel
        .post_while_busy(workspace.id, "operator", "more")
        .await;
    let entry = again
        .next(PATIENCE)
        .await
        .expect("the appended entry before the deadline");
    assert_eq!(entry.name, "entry");
    assert_eq!(
        entry.data["seq"].as_i64(),
        Some(last + 1),
        "the appended entry did not resume from the cursor"
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_non_follow_read_registers_nobody() {
    let kestrel = Kestrel::boot_with_follow_lease(LEASE).await;
    let workspace = an_open_workspace(&kestrel).await;

    let mut watcher = Stream::follow(&kestrel, &workspace, None).await;
    watcher.follower().await;
    watcher.presence().await;

    let response = reqwest::Client::new()
        .get(format!(
            "{}{}?follow=false&as=Ada",
            kestrel.operator(),
            transcript("acme", &workspace)
        ))
        .send()
        .await
        .expect("the operator boundary should answer");
    let read = response.text().await.expect("the stream should end");
    assert!(read.contains("event: end"), "{read}");
    assert!(!read.contains("event: follower"), "{read}");
    assert!(!read.contains("event: presence"), "{read}");

    assert!(
        watcher.next(QUIET).await.is_none(),
        "a read that did not follow registered a presence"
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_follow_naming_an_agent_is_refused() {
    let kestrel = Kestrel::boot_with_follow_lease(LEASE).await;
    let workspace = an_open_workspace(&kestrel).await;

    let response = reqwest::Client::new()
        .get(format!(
            "{}{}?follow=true&as=builder",
            kestrel.operator(),
            transcript("acme", &workspace)
        ))
        .send()
        .await
        .expect("the operator boundary should answer");

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let refusal: Value = response.json().await.expect("a refusal as JSON");
    assert_eq!(refusal["field"], "participant");
    assert!(
        refusal["message"]
            .as_str()
            .expect("a message")
            .contains("builder"),
        "{refusal}"
    );

    kestrel.teardown().await;
}

/// The CLI's follow runs off this runtime, so its blocking line reads do too.
async fn printed(mut client: support::client::Client) -> (support::client::Client, String) {
    tokio::task::spawn_blocking(move || {
        let line = client.line();

        (client, line)
    })
    .await
    .expect("the client should print")
}

#[tokio::test]
async fn the_cli_follow_is_counted_renews_and_prints_no_presence() {
    let kestrel = Kestrel::boot_with_follow_lease(Duration::from_secs(3)).await;
    let workspace = an_open_workspace(&kestrel).await;
    kestrel.post(workspace.id, "operator", "go").await;

    let mut cli = support::client::Client::spawn(
        &kestrel.operator(),
        &[
            "workspace",
            "transcript",
            &workspace.id.to_string(),
            "--follow",
            "--as-participant",
            "Ada",
            "--no-summaries",
            "--json",
        ],
    );

    loop {
        let (returned, line) = printed(cli).await;
        cli = returned;
        let record: Value = serde_json::from_str(&line)
            .unwrap_or_else(|error| panic!("the CLI printed {line}: {error}"));
        assert!(
            record.get("named").is_none() && record.get("anonymous").is_none(),
            "the CLI printed presence: {line}"
        );
        if record["entry"]["message"] == "go" {
            break;
        }
    }

    let mut watcher = Stream::follow(&kestrel, &workspace, None).await;
    let (watcher_id, _) = watcher.follower().await;
    let _keeping = keeping(&kestrel, &workspace, &watcher_id);
    loop {
        let snapshot = watcher.presence().await;
        if snapshot["named"] == json!(["Ada"]) {
            assert_eq!(snapshot, json!({"named": ["Ada"], "anonymous": 1}));
            break;
        }
    }

    // An entry appended after the registration reaches the CLI as an entry: a presence the CLI
    // printed would have been read here instead.
    kestrel
        .post_while_busy(workspace.id, "operator", "more")
        .await;
    let (returned, line) = printed(cli).await;
    cli = returned;
    let record: Value = serde_json::from_str(&line)
        .unwrap_or_else(|error| panic!("the CLI printed {line}: {error}"));
    assert_eq!(record["entry"]["message"], "more", "{line}");

    // The lease is three seconds and the CLI renews a third of the way in; waiting past it with
    // no removal event proves the renewal.
    tokio::time::sleep(Duration::from_secs(4)).await;
    let lapsed = watcher.presence_within(QUIET).await;
    assert!(
        lapsed.is_none(),
        "the CLI's follow lapsed instead of renewing: {lapsed:?}"
    );

    drop(cli);
    assert_eq!(
        watcher.presence().await,
        json!({"named": [], "anonymous": 1}),
        "the CLI's follower stayed after its stream closed"
    );

    kestrel.teardown().await;
}
