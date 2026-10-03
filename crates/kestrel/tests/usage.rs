mod support;

use std::time::Duration;

use kestrel::domain::{Exit, SessionId, SessionState, Workspace, WorkspaceId};
use kestrel_scripted_agent::BURSTED_USAGE;
use serde_json::Value;
use support::Kestrel;
use support::link_client::{Events, Next};
use support::scripted_agent::{self, Script};
use support::supervisor;

const PATIENCE: Duration = Duration::from_secs(30);

async fn dispatching(script: Script) -> Kestrel {
    Kestrel::dispatching_to(supervisor::binary(), &scripted_agent::playing(script)).await
}

async fn a_workspace(kestrel: &Kestrel) -> Workspace {
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
        .declare_agent(&organization, "builder", support::HARNESS, None)
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

async fn follow(kestrel: &Kestrel, workspace: WorkspaceId) -> Events {
    let base = format!(
        "{}/operator/organizations/acme/workspaces/{workspace}/transcript",
        kestrel.operator()
    );

    Events::over(
        reqwest::Client::new()
            .get(&base)
            .send()
            .await
            .expect("the operator boundary should answer"),
    )
}

/// The Session as the operator boundary serves it, where live state is merged in.
async fn shown(kestrel: &Kestrel, session: SessionId) -> Value {
    let path = kestrel::operator::SESSION
        .replace("{organization}", "acme")
        .replace("{session}", &session.to_string());

    reqwest::Client::new()
        .get(format!("{}{path}", kestrel.operator()))
        .send()
        .await
        .expect("the operator boundary should answer")
        .json()
        .await
        .expect("the session as JSON")
}

async fn until_named(stream: &mut Events, name: &str) -> support::link_client::Event {
    let deadline = tokio::time::Instant::now() + PATIENCE;

    loop {
        let patience = deadline.saturating_duration_since(tokio::time::Instant::now());
        match stream.next_within(patience).await {
            Next::Event(event) if event.name.as_deref() == Some(name) => return event,
            Next::Event(_) => {}
            Next::Quiet => panic!("the stream never said {name}"),
            Next::Closed => panic!("the stream closed before it said {name}"),
        }
    }
}

#[tokio::test]
async fn a_burst_of_usage_updates_reaches_a_follower_once_with_the_last_value() {
    let kestrel = dispatching(Script::BurstsUsage).await;
    let workspace = a_workspace(&kestrel).await;
    let mut stream = follow(&kestrel, workspace.id).await;
    kestrel.enqueue_session(workspace.id).await;

    let mut states = Vec::new();
    loop {
        match stream.next_within(PATIENCE).await {
            Next::Event(event) if event.name.as_deref() == Some("session_state") => {
                assert_eq!(event.id, None, "a session_state event carries no id");
                let state: Value = serde_json::from_str(&event.data).unwrap();
                let settled = state["usage"]["context_used"].as_u64() == Some(BURSTED_USAGE);
                states.push(state);
                if settled {
                    break;
                }
            }
            Next::Event(_) => {}
            Next::Quiet | Next::Closed => break,
        }
    }

    let usages: Vec<u64> = states
        .iter()
        .filter_map(|state| state["usage"]["context_used"].as_u64())
        .collect();
    assert!(
        !usages.is_empty(),
        "no session_state carried the usage: {states:?}"
    );
    assert_eq!(
        usages.last(),
        Some(&BURSTED_USAGE),
        "the last usage was not the value the burst settled on"
    );
    assert!(
        usages.iter().all(|used| *used == BURSTED_USAGE),
        "an intermediate value reached the follower: {usages:?}"
    );

    let transcript: Vec<String> = kestrel
        .transcript(workspace.id)
        .await
        .iter()
        .map(|recorded| recorded.entry.to_string())
        .collect();
    assert!(
        !transcript.join("\n").contains("context_used"),
        "a usage report entered the Transcript: {transcript:?}"
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_follower_connecting_mid_turn_gets_current_usage_in_its_snapshot() {
    let kestrel = dispatching(Script::BurstsUsage).await;
    let workspace = a_workspace(&kestrel).await;
    let session = kestrel.enqueue_session(workspace.id).await;

    let deadline = tokio::time::Instant::now() + PATIENCE;
    loop {
        let live = shown(&kestrel, session.id).await;
        if live["usage"]["context_used"] == BURSTED_USAGE {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the live usage never reached the read: {live}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    let mut stream = follow(&kestrel, workspace.id).await;
    let snapshot = until_named(&mut stream, "session_state").await;
    assert_eq!(snapshot.id, None);
    let state: Value = serde_json::from_str(&snapshot.data).unwrap();
    assert_eq!(
        state["usage"]["context_used"], BURSTED_USAGE,
        "the snapshot did not carry the current usage: {state}"
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn usage_reported_during_a_turn_reaches_the_read_once_the_session_ends() {
    let kestrel = dispatching(Script::BurstsUsage).await;
    let workspace = a_workspace(&kestrel).await;
    let session = kestrel.enqueue_session(workspace.id).await;

    let answered = kestrel.after_one_turn(session.id).await;
    assert_eq!(answered.exit, Some(Exit::Succeeded), "{:?}", answered.exit);

    let shown = shown(&kestrel, session.id).await;
    assert_eq!(shown["state"], "ended");
    assert_eq!(
        shown["usage"]["context_used"], BURSTED_USAGE,
        "the usage the Turn reported was not recorded: {shown}"
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn usage_reported_while_trailing_reaches_the_read_once_the_session_settles() {
    let kestrel = dispatching(Script::AnswersThenKeepsBooks).await;
    let workspace = a_workspace(&kestrel).await;
    let session = kestrel.enqueue_session(workspace.id).await;

    let waiting = kestrel.answered(session.id, 1).await;
    assert_eq!(waiting.state, SessionState::Waiting);

    let recorded = kestrel.session(session.id).await;
    // The answer carried no usage, so anything recorded can only have come from the trailing work.
    assert!(
        recorded
            .usage
            .as_ref()
            .is_some_and(|usage| usage.context_used > 0),
        "the usage reported while trailing was not kept when the Session settled: {:?}",
        recorded.usage
    );

    kestrel.teardown().await;
}
