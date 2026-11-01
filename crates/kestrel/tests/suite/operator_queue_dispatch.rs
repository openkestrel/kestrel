//! The queue's first position is the Session dispatch claims next, across inputs and
//! Organizations.

use crate::support;

use kestrel::domain::SessionState;
use kestrel::scheduling;
use reqwest::StatusCode;
use serde_json::{Value, json};
use support::Kestrel;
use support::fixture::Fixture;
use support::operator_queue::{
    a_queued_workspace_in, a_waiting_session, an_unbriefed_session, jacks_workspace, numbered,
    profile_reason, queue_read, sharing_a_serialized_profile, unbriefed_row, waiting_row,
};
use support::operator_requests::{client, failed, got};

#[tokio::test]
async fn the_session_dispatch_claims_next_is_the_queues_first_position() {
    let kestrel = Kestrel::boot().await;
    sharing_a_serialized_profile(&kestrel, None, 8).await;
    let holding = kestrel
        .dispatch_session(jacks_workspace(&kestrel).await)
        .await;
    let blocker = kestrel
        .enqueue_session(a_queued_workspace_in(&kestrel, "acme", "kestrel").await)
        .await;
    let held = kestrel
        .enqueue_session(jacks_workspace(&kestrel).await)
        .await;
    let blocked = kestrel
        .enqueue_session(a_queued_workspace_in(&kestrel, "acme", "kestrel").await)
        .await;
    kestrel.block_session(&blocked, &blocker).await;
    let ready = kestrel
        .enqueue_session(a_queued_workspace_in(&kestrel, "acme", "kestrel").await)
        .await;

    let mut claimed = Vec::new();
    loop {
        let queue = queue_read(&kestrel).await;
        let first = numbered(&queue)
            .into_iter()
            .find(|row| row["position"] == 1)
            .map(|row| row["name"].as_str().expect("a name").to_owned());
        let next = match kestrel.occupy_up_to(8).await {
            Some(scheduling::Occupied::Claimed(next)) => Some(next),
            Some(scheduling::Occupied::Resumed(_)) => panic!("nothing waits to resume"),
            None => None,
        };
        assert_eq!(
            next.as_ref().map(|session| session.name.clone()),
            first,
            "dispatch disagreed with the queue: {queue}"
        );
        let Some(next) = next else { break };
        claimed.push(next);
        if claimed.len() == 2 {
            kestrel.complete_session(&holding).await;
            kestrel.complete_session(&claimed[0]).await;
        }
    }

    assert_eq!(
        claimed.iter().map(|session| session.id).collect::<Vec<_>>(),
        [blocker.id, ready.id, held.id, blocked.id]
    );

    kestrel.teardown().await;
}

async fn mixed_slot_requests(kestrel: &Kestrel) -> Vec<kestrel::domain::Session> {
    sharing_a_serialized_profile(kestrel, None, 8).await;
    let (unbriefed_workspace, unbriefed) =
        an_unbriefed_session(kestrel, "acme", "kestrel", "docker/first-brief").await;
    kestrel
        .post_while_busy(unbriefed_workspace, "alice", "first Brief")
        .await;
    let queue = queue_read(kestrel).await;
    let preparing = unbriefed_row(&queue, &unbriefed.name);
    assert_eq!(preparing["position"], Value::Null);
    assert_eq!(preparing["brief_since"], Value::Null);
    assert!(preparing["pending_since"].is_string());

    let workspace = a_queued_workspace_in(kestrel, "acme", "kestrel").await;
    let waiting = a_waiting_session(kestrel, workspace).await;
    kestrel
        .post_while_busy(workspace, "alice", "next Turn")
        .await;
    kestrel.report_ready(&unbriefed).await;
    let queued = kestrel
        .enqueue_session(a_queued_workspace_in(kestrel, "acme", "kestrel").await)
        .await;
    kestrel
        .post_while_busy(unbriefed_workspace, "alice", "after the Brief")
        .await;
    vec![waiting, unbriefed, queued]
}

async fn assert_queue_dispatch_order(kestrel: &Kestrel, sessions: &[kestrel::domain::Session]) {
    for expected in sessions {
        let queue = queue_read(kestrel).await;
        let next: Vec<_> = ["queued", "waiting", "unbriefed"]
            .into_iter()
            .flat_map(|section| queue[section].as_array().unwrap())
            .filter(|row| row["position"] == 1)
            .collect();
        assert_eq!(next.len(), 1, "{queue}");
        assert_eq!(next[0]["name"], expected.name, "{queue}");
        let actual = match kestrel.occupy_up_to(8).await {
            Some(
                scheduling::Occupied::Claimed(session) | scheduling::Occupied::Resumed(session),
            ) => session,
            None => panic!("dispatch failed to take {}: {queue}", expected.name),
        };
        assert_eq!(actual.id, expected.id, "{queue}");
    }
}

#[tokio::test]
async fn queue_and_dispatch_agree_for_held_messages_first_briefs_and_queued_sessions() {
    let kestrel = Kestrel::boot().await;
    let requests = mixed_slot_requests(&kestrel).await;
    let queue = queue_read(&kestrel).await;
    assert_eq!(waiting_row(&queue, &requests[0].name)["position"], 1);
    let first = unbriefed_row(&queue, &requests[1].name);
    assert_eq!(first["position"], 2);
    assert!(first["brief_since"].as_str().unwrap() < first["pending_since"].as_str().unwrap());
    assert_eq!(
        first["reasons"],
        json!([{ "kind": "ahead", "sessions": [requests[0].name] }])
    );
    assert_eq!(numbered(&queue)[0]["position"], 3);
    assert_queue_dispatch_order(&kestrel, &requests).await;
    let (status, read) = got(
        &kestrel,
        &format!(
            "/operator/organizations/acme/workspaces/{}",
            requests[1].workspace
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{read}");
    assert_eq!(read["unfinished_session"]["id"], requests[1].id.to_string());
    kestrel.teardown().await;
}

#[tokio::test]
async fn queue_and_dispatch_preserve_input_priority_at_equal_timestamps() {
    let kestrel = Kestrel::stepped().await;
    let requests = mixed_slot_requests(&kestrel).await;
    assert_queue_dispatch_order(&kestrel, &requests).await;
    kestrel.teardown().await;
}

#[tokio::test]
async fn queue_positions_include_other_organizations_without_exposing_their_sessions() {
    let kestrel = Kestrel::boot().await;
    sharing_a_serialized_profile(&kestrel, None, 8).await;
    Fixture::acme()
        .organization("other")
        .declare(&kestrel)
        .await;
    let earlier = kestrel
        .enqueue_session(a_queued_workspace_in(&kestrel, "other", "kestrel").await)
        .await;
    let ours = kestrel
        .enqueue_session(a_queued_workspace_in(&kestrel, "acme", "kestrel").await)
        .await;
    let queue = queue_read(&kestrel).await;
    assert_eq!(numbered(&queue)[0]["position"], 2);
    assert_eq!(
        numbered(&queue)[0]["reasons"],
        json!([{ "kind": "ahead", "sessions": [], "elsewhere": 1 }])
    );
    let text = queue.to_string();
    for secret in [
        earlier.name.clone(),
        earlier.id.to_string(),
        earlier.workspace.to_string(),
    ] {
        assert!(!text.contains(&secret), "foreign identity leaked: {queue}");
    }
    let listed = got(&kestrel, "/operator/organizations/acme/workspaces")
        .await
        .1;
    assert_eq!(listed[0]["queue"]["position"], 2);
    assert_eq!(
        listed[0]["queue"]["reasons"],
        numbered(&queue)[0]["reasons"]
    );
    let output = client(&kestrel, &["queue", "--organization", "acme"]).await;
    assert!(output.status.success(), "{}", failed(&output));
    let shown = output.out.join("\n");
    assert!(
        shown.contains("1 Session in other Organizations"),
        "{shown}"
    );
    assert!(!shown.contains(&earlier.name), "{shown}");
    let claimed = kestrel.occupy_up_to(8).await.unwrap();
    assert!(matches!(claimed, scheduling::Occupied::Claimed(session) if session.id == earlier.id));
    let queue = queue_read(&kestrel).await;
    assert_eq!(numbered(&queue)[0]["position"], 1);
    assert_eq!(queue["active_work"]["elsewhere"], 1);
    assert_queue_dispatch_order(&kestrel, &[ours]).await;
    kestrel.teardown().await;
}

#[tokio::test]
async fn a_profile_held_first_brief_has_no_queue_position_until_the_profile_is_free() {
    let kestrel = Kestrel::boot().await;
    sharing_a_serialized_profile(&kestrel, None, 8).await;
    let workspace = jacks_workspace(&kestrel).await;
    let first = kestrel.enqueue_session_with_nothing_posted(workspace).await;
    let claimed = kestrel.claim_session().await.unwrap();
    assert_eq!(claimed.id, first.id);
    kestrel.executes_on(&claimed, "docker/profile-brief").await;
    kestrel.report_ready(&claimed).await;
    kestrel
        .post_while_busy(workspace, "jack", "first Brief")
        .await;
    let holding = kestrel
        .dispatch_session(jacks_workspace(&kestrel).await)
        .await;
    let ready = kestrel
        .enqueue_session(a_queued_workspace_in(&kestrel, "acme", "kestrel").await)
        .await;
    let queue = queue_read(&kestrel).await;
    assert_eq!(unbriefed_row(&queue, &first.name)["position"], Value::Null);
    assert_eq!(
        unbriefed_row(&queue, &first.name)["reasons"],
        json!([profile_reason(&holding)])
    );
    assert_eq!(numbered(&queue)[0]["position"], 1);
    kestrel.complete_session(&holding).await;
    assert_queue_dispatch_order(&kestrel, &[first, ready]).await;
    kestrel.teardown().await;
}

#[tokio::test]
async fn the_queue_counts_trailing_occupants_above_the_limit_and_dispatch_preserves_their_slots() {
    let kestrel = Kestrel::boot().await;
    sharing_a_serialized_profile(&kestrel, None, 1).await;
    let first = a_waiting_session(
        &kestrel,
        a_queued_workspace_in(&kestrel, "acme", "kestrel").await,
    )
    .await;
    let second = a_waiting_session(
        &kestrel,
        a_queued_workspace_in(&kestrel, "acme", "kestrel").await,
    )
    .await;
    kestrel
        .report_activity(&first, 3, "activity after settling")
        .await;
    kestrel
        .report_activity(&second, 3, "more activity after settling")
        .await;
    let queued = kestrel
        .enqueue_session(a_queued_workspace_in(&kestrel, "acme", "kestrel").await)
        .await;
    let queue = queue_read(&kestrel).await;
    assert_eq!(queue["active_work"]["occupied"], 2);
    assert_eq!(queue["active_work"]["limit"], 1);
    assert_eq!(queue["waiting"], json!([]));
    assert_eq!(
        queue["active_work"]["occupants"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| row["phase"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["trailing", "trailing"]
    );
    assert_eq!(numbered(&queue)[0]["position"], 1);
    assert!(kestrel.occupy_up_to(1).await.is_none());
    let output = client(&kestrel, &["queue", "--organization", "acme"]).await;
    assert!(output.status.success(), "{}", failed(&output));
    let shown = output.out.join("\n");
    assert!(shown.contains("2 occupied"), "{shown}");
    assert!(shown.contains("trailing"), "{shown}");
    assert!(
        shown.find(&first.name).unwrap() < shown.find(&queued.name).unwrap(),
        "{shown}"
    );
    kestrel
        .post_while_busy(first.workspace, "alice", "a new Turn while Trailing")
        .await;
    assert_eq!(kestrel.session(first.id).await.state, SessionState::Working);
    assert_eq!(kestrel.turns(first.id).await.len(), 2);
    assert_eq!(queue_read(&kestrel).await["active_work"]["occupied"], 2);
    assert!(kestrel.occupy_up_to(1).await.is_none());
    kestrel.teardown().await;
}
