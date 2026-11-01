//! `kestrel queue` in words and as JSON, agreeing with the operator read.

use crate::support;

use serde_json::{Value, json};
use support::Kestrel;
use support::client;
use support::fixture::Fixture;
use support::operator_queue::{
    a_queued_workspace_in, a_waiting_session, an_unbriefed_session, jacks_workspace, queue_read,
    sharing_a_serialized_profile,
};
use support::operator_requests::client;

#[tokio::test]
async fn a_client_reads_the_whole_queue_with_kestrel_queue_json() {
    let kestrel = Kestrel::boot().await;
    kestrel.record_dispatch(8, "local-exec").await;
    Fixture::acme().declare(&kestrel).await;

    let first = a_queued_workspace_in(&kestrel, "acme", "kestrel").await;
    let second = a_queued_workspace_in(&kestrel, "acme", "kestrel").await;
    let third = a_queued_workspace_in(&kestrel, "acme", "kestrel").await;
    let blocker = kestrel.enqueue_session(first).await;
    let between = kestrel.enqueue_session(second).await;
    let blocked = kestrel.enqueue_session(third).await;
    kestrel.block_session(&blocked, &blocker).await;

    let queue = client(&kestrel, &["queue", "--organization", "acme", "--json"])
        .await
        .json();
    let rows = queue["queued"].as_array().expect("the queued Sessions");
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0]["position"].as_u64(), Some(1));
    assert_eq!(rows[0]["name"], blocker.name);
    assert_eq!(rows[1]["position"].as_u64(), Some(2));
    assert_eq!(rows[1]["name"], between.name);
    assert_eq!(rows[2]["position"].as_u64(), None);
    assert_eq!(rows[2]["name"], blocked.name);
    assert_eq!(
        rows[2]["reasons"],
        json!([{ "kind": "dependencies", "sessions": [blocker.name] }])
    );

    assert_eq!(queue["active_work"]["limit"], 8);
    assert_eq!(queue["active_work"]["occupied"], 0);
    assert_eq!(queue["active_work"]["elsewhere"], 0);
    assert_eq!(queue["instances"]["limit"], Value::Null);
    assert_eq!(queue["instances"]["count"], 0);

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_terminal_reads_the_kestrel_queue_with_the_limits_said_first() {
    let kestrel = Kestrel::boot().await;
    Fixture::acme().declare(&kestrel).await;
    let first = a_queued_workspace_in(&kestrel, "acme", "kestrel").await;
    let second = a_queued_workspace_in(&kestrel, "acme", "kestrel").await;
    let blocker = kestrel.enqueue_session(first).await;
    let queued = kestrel.enqueue_session(second).await;

    let shown = client::ran_on_a_terminal_by(
        &kestrel,
        &["queue", "--organization", "acme"],
        client::Invocation::default(),
        "",
    )
    .await;
    assert!(shown.status.success(), "{}", shown.said);
    let said = shown.said;
    assert!(
        said.contains("active-work slots  no dispatch configuration recorded; 0 occupied"),
        "{said}"
    );
    assert!(
        said.contains("live instances     unbounded; 0 counted"),
        "{said}"
    );
    assert!(
        said.lines().any(|line| line.starts_with("environment")
            && line.contains("no dispatch configuration recorded")),
        "the environment is not said first: {said}"
    );
    // The table's own labels and the enqueued times are truncated on a terminal this wide.
    assert!(
        said.contains("position  name"),
        "the columns come without their labels: {said}"
    );
    assert!(
        said.contains(&format!("-         {}", blocker.name)),
        "{said}"
    );
    assert!(
        said.contains(&format!("-         {}", queued.name)),
        "{said}"
    );

    kestrel.teardown().await;
}

struct EveryKind {
    holding: kestrel::domain::Session,
    ready: kestrel::domain::Session,
    blocked: kestrel::domain::Session,
    prompted: kestrel::domain::Session,
    waiting: kestrel::domain::Session,
    unbriefed: kestrel::domain::Session,
}

async fn a_queue_of_every_kind(kestrel: &Kestrel) -> EveryKind {
    sharing_a_serialized_profile(kestrel, None, 4).await;
    let waiting = a_waiting_session(
        kestrel,
        a_queued_workspace_in(kestrel, "acme", "kestrel").await,
    )
    .await;
    let jacks = jacks_workspace(kestrel).await;
    let prompted = a_waiting_session(kestrel, jacks).await;
    let holding = kestrel
        .dispatch_session(jacks_workspace(kestrel).await)
        .await;
    kestrel
        .post_while_busy(jacks, "jack", "one more thing")
        .await;
    let (getting_ready, unbriefed) =
        an_unbriefed_session(kestrel, "acme", "kestrel", "docker/getting-ready").await;
    kestrel.report_ready(&unbriefed).await;
    kestrel
        .post_while_busy(getting_ready, "alice", "the brief")
        .await;
    kestrel
        .post_while_busy(getting_ready, "alice", "one more thing")
        .await;
    let ready = kestrel
        .enqueue_session(a_queued_workspace_in(kestrel, "acme", "kestrel").await)
        .await;
    let blocked = kestrel
        .enqueue_session(a_queued_workspace_in(kestrel, "acme", "kestrel").await)
        .await;
    kestrel.block_session(&blocked, &holding).await;

    EveryKind {
        holding,
        ready,
        blocked,
        prompted,
        waiting,
        unbriefed,
    }
}

#[tokio::test]
async fn kestrel_queue_says_each_reason_in_words_with_the_waiting_sessions_after_the_queue() {
    let kestrel = Kestrel::boot().await;
    let every = a_queue_of_every_kind(&kestrel).await;

    // Wide enough that no reason is truncated.
    let operator = kestrel.operator();
    let shown = tokio::task::spawn_blocking(move || {
        client::ran_on_a_terminal(&operator, &["queue", "--organization", "acme"], 240, "")
    })
    .await
    .expect("the client should run");
    assert!(shown.status.success(), "{}", shown.said);
    let lines: Vec<&str> = shown.said.lines().collect();
    let line_of = |name: &str| {
        lines
            .iter()
            .position(|line| line.split_whitespace().nth(1) == Some(name))
            .unwrap_or_else(|| panic!("no line names {name}: {}", shown.said))
    };
    assert!(
        line_of(&every.ready.name) < line_of(&every.blocked.name)
            && line_of(&every.blocked.name) < line_of(&every.prompted.name)
            && line_of(&every.prompted.name) < line_of(&every.waiting.name)
            && line_of(&every.waiting.name) < line_of(&every.unbriefed.name),
        "{}",
        shown.said
    );
    for (session, said) in [
        (&every.ready, format!("behind {}", every.unbriefed.name)),
        (&every.blocked, format!("waits on {}", every.holding.name)),
        (
            &every.prompted,
            format!(
                "the codex sign-in in the subscription profile jack is in use by the session {}",
                every.holding.name
            ),
        ),
        (&every.waiting, "waiting for a turn".to_owned()),
        (&every.unbriefed, "harness ready".to_owned()),
    ] {
        let line = lines[line_of(&session.name)];
        assert!(line.contains(&said), "{line} does not say {said}");
    }
    assert!(lines[line_of(&every.prompted.name)].contains("input since "));
    assert!(lines[line_of(&every.unbriefed.name)].contains("input since "));

    assert!(
        shown.said.contains(&format!(
            "kestrel session show {} --organization acme",
            every.holding.name
        )),
        "{}",
        shown.said
    );
    assert!(
        shown.said.contains(&format!(
            "kestrel session stop {} --organization acme",
            every.holding.id
        )),
        "{}",
        shown.said
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn kestrel_queue_json_agrees_with_the_operator_read() {
    let kestrel = Kestrel::boot().await;
    a_queue_of_every_kind(&kestrel).await;

    let queue = queue_read(&kestrel).await;
    let shown = client(&kestrel, &["queue", "--organization", "acme", "--json"])
        .await
        .json();

    assert_eq!(shown, queue);

    let decoded: kestrel_operator_types::Queue = serde_json::from_value(shown.clone()).unwrap();
    assert_eq!(serde_json::to_value(&decoded).unwrap(), queue);
    let listed = client(
        &kestrel,
        &["workspace", "list", "--organization", "acme", "--json"],
    )
    .await
    .json();
    let workspaces: kestrel_operator_types::ListWorkspacesResponse =
        serde_json::from_value(listed).unwrap();
    for workspace in workspaces {
        let standing = workspace.queue.as_ref();
        let expected = decoded
            .queued
            .iter()
            .find(|session| session.workspace == workspace.id)
            .map(|session| (session.position, &session.reasons, None))
            .or_else(|| {
                decoded
                    .waiting
                    .iter()
                    .find(|session| session.workspace == workspace.id)
                    .map(|session| {
                        (
                            session.position,
                            &session.reasons,
                            session.pending_since.as_ref(),
                        )
                    })
            })
            .or_else(|| {
                decoded
                    .unbriefed
                    .iter()
                    .find(|session| session.workspace == workspace.id)
                    .map(|session| {
                        (
                            session.position,
                            &session.reasons,
                            session
                                .brief_since
                                .as_ref()
                                .or(session.pending_since.as_ref()),
                        )
                    })
            });
        match expected {
            Some((position, reasons, pending_since)) => {
                let standing =
                    standing.expect("a queued, waiting or unbriefed Workspace has a standing");
                assert_eq!(standing.position, position);
                assert_eq!(
                    serde_json::to_value(&standing.reasons).unwrap(),
                    serde_json::to_value(reasons).unwrap()
                );
                assert_eq!(standing.pending_since.as_ref(), pending_since);
                assert!(workspace.session.is_some());
            }
            None => assert!(standing.is_none()),
        }
    }

    kestrel.teardown().await;
}
