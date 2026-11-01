//! Waiting and unbriefed Sessions beside the queue, and why a next turn has not started.

use crate::support;

use kestrel::scheduling;
use reqwest::StatusCode;
use serde_json::{Value, json};
use support::Kestrel;
use support::fixture::Fixture;
use support::operator_queue::{
    a_queued_workspace_in, a_waiting_session, an_unbriefed_session, jacks_workspace, numbered,
    profile_reason, queue_of, queue_read, sharing_a_serialized_profile, unbriefed_row, waiting_row,
};
use support::operator_requests::got;
use support::supervisor;

#[tokio::test]
async fn an_unbriefed_session_stands_beside_the_waiting_ones_without_a_slot_of_its_own() {
    let kestrel = Kestrel::boot().await;
    Fixture::acme().declare(&kestrel).await;

    let waiting = a_waiting_session(
        &kestrel,
        a_queued_workspace_in(&kestrel, "acme", "kestrel").await,
    )
    .await;
    let holding = kestrel
        .dispatch_session(a_queued_workspace_in(&kestrel, "acme", "kestrel").await)
        .await;
    kestrel.executes_on(&holding, "docker/holding").await;
    let (getting_ready, unbriefed) =
        an_unbriefed_session(&kestrel, "acme", "kestrel", "docker/getting-ready").await;
    let also_queued = kestrel
        .enqueue_session(a_queued_workspace_in(&kestrel, "acme", "kestrel").await)
        .await;

    let queue = queue_read(&kestrel).await;
    assert_eq!(
        waiting_row(&queue, &waiting.name)["pending_since"],
        Value::Null
    );
    let row = unbriefed_row(&queue, &unbriefed.name);
    assert_eq!(row["preparing"], "provisioning");
    assert_eq!(row["pending_since"], Value::Null);
    assert_eq!(
        numbered(&queue)
            .iter()
            .map(|row| row["name"].clone())
            .collect::<Vec<_>>(),
        [json!(also_queued.name)],
        "an unbriefed Session was numbered in the queue"
    );
    assert_eq!(
        queue["active_work"]["occupied"], 1,
        "only the Working Session holds an Active-Work Slot"
    );
    assert_eq!(
        queue["instances"]["count"], 3,
        "the unbriefed Session's Instance counts against the live limit"
    );
    assert!(
        queue["instances"]["counted"]
            .as_array()
            .expect("the counted Instances")
            .contains(&json!("docker/getting-ready"))
    );

    kestrel
        .post_while_busy(getting_ready, "alice", "what I want")
        .await;
    assert!(kestrel.database().holds_messages(getting_ready).await);
    let queue = queue_read(&kestrel).await;
    assert!(unbriefed_row(&queue, &unbriefed.name)["pending_since"].is_string());

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_queued_session_with_no_brief_waits_only_on_its_instance_and_is_never_numbered() {
    let kestrel = Kestrel::boot().await;
    Fixture::acme().limited_to(1).declare(&kestrel).await;
    let holding = kestrel
        .dispatch_session(a_queued_workspace_in(&kestrel, "acme", "kestrel").await)
        .await;
    kestrel.executes_on(&holding, "docker/holding").await;

    let queued = kestrel
        .enqueue_session_with_nothing_posted(
            a_queued_workspace_in(&kestrel, "acme", "kestrel").await,
        )
        .await;

    let queue = queue_read(&kestrel).await;
    let row = numbered(&queue)
        .into_iter()
        .find(|row| row["name"] == queued.name)
        .expect("the queued Session");
    assert_eq!(row["position"], Value::Null);
    assert_eq!(
        row["reasons"],
        json!([{ "kind": "live_instance_limit", "limit": 1 }]),
        "a queued Session with no Brief waits only on its Instance"
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn the_queue_names_the_compute_driver_the_work_role_recorded() {
    let kestrel = Kestrel::boot().await;
    kestrel.declare_organization("acme").await;

    let (status, queue) = got(&kestrel, &queue_of("acme")).await;
    assert_eq!(status, StatusCode::OK, "{queue}");
    assert_eq!(
        queue["work_role"],
        Value::Null,
        "no work role is dispatching"
    );

    kestrel.record_dispatch(2, "docker").await;
    let (status, queue) = got(&kestrel, &queue_of("acme")).await;
    assert_eq!(status, StatusCode::OK, "{queue}");
    assert_eq!(
        queue["work_role"],
        json!({
            "active_work_slots": 2,
            "serialized_harnesses": ["codex"],
            "driver": "docker",
        }),
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_waiting_session_with_held_input_says_why_its_next_turn_has_not_started() {
    let kestrel = Kestrel::boot().await;
    sharing_a_serialized_profile(&kestrel, None, 2).await;
    let early = a_queued_workspace_in(&kestrel, "acme", "kestrel").await;
    let late = a_queued_workspace_in(&kestrel, "acme", "kestrel").await;
    let idle = a_queued_workspace_in(&kestrel, "acme", "kestrel").await;
    let early_session = a_waiting_session(&kestrel, early).await;
    let late_session = a_waiting_session(&kestrel, late).await;
    let idle_session = a_waiting_session(&kestrel, idle).await;
    kestrel.post_while_busy(early, "jack", "first").await;
    let queued = kestrel
        .enqueue_session(a_queued_workspace_in(&kestrel, "acme", "kestrel").await)
        .await;
    kestrel.post_while_busy(late, "jack", "second").await;

    let queue = queue_read(&kestrel).await;
    assert_eq!(
        queue["waiting"]
            .as_array()
            .expect("the waiting Sessions")
            .iter()
            .map(|row| row["name"].as_str().expect("a name"))
            .collect::<Vec<_>>(),
        [
            early_session.name.as_str(),
            late_session.name.as_str(),
            idle_session.name.as_str()
        ],
        "held input first, oldest first, then the rest"
    );
    assert!(waiting_row(&queue, &early_session.name)["pending_since"].is_string());
    assert_eq!(
        waiting_row(&queue, &early_session.name)["reasons"],
        json!([])
    );
    assert_eq!(
        waiting_row(&queue, &late_session.name)["reasons"],
        json!([{ "kind": "ahead", "sessions": [early_session.name, queued.name] }])
    );
    assert_eq!(
        waiting_row(&queue, &idle_session.name)["pending_since"],
        Value::Null
    );
    assert_eq!(
        waiting_row(&queue, &idle_session.name)["reasons"],
        json!([])
    );
    assert_eq!(
        numbered(&queue)
            .iter()
            .map(|row| row["name"].clone())
            .collect::<Vec<_>>(),
        [json!(queued.name)],
        "a Waiting Session was listed in the queue"
    );

    let order = [
        early_session.name.clone(),
        queued.name.clone(),
        late_session.name.clone(),
    ];
    for expected in order {
        let taken = match kestrel.occupy_up_to(8).await {
            Some(scheduling::Occupied::Claimed(claimed)) => claimed,
            Some(scheduling::Occupied::Resumed(resumed)) => resumed,
            None => panic!("a slot is free and {expected} is ahead"),
        };
        assert_eq!(taken.name, expected);
    }

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_waiting_session_with_held_input_waits_on_full_slots_and_on_its_profile() {
    let kestrel = Kestrel::boot().await;
    sharing_a_serialized_profile(&kestrel, None, 2).await;
    let jacks = jacks_workspace(&kestrel).await;
    let waiting = a_waiting_session(&kestrel, jacks).await;
    let holding = kestrel
        .dispatch_session(jacks_workspace(&kestrel).await)
        .await;
    kestrel
        .post_while_busy(jacks, "jack", "one more thing")
        .await;
    let profile = profile_reason(&holding);

    let queue = queue_read(&kestrel).await;
    assert_eq!(
        waiting_row(&queue, &waiting.name)["reasons"],
        json!([profile])
    );

    kestrel
        .dispatch_session(a_queued_workspace_in(&kestrel, "acme", "kestrel").await)
        .await;
    let queue = queue_read(&kestrel).await;
    assert_eq!(
        waiting_row(&queue, &waiting.name)["reasons"],
        json!([{ "kind": "active_work_slots", "limit": 2 }, profile])
    );
    assert!(kestrel.occupy_up_to(2).await.is_none());

    kestrel.teardown().await;
}

#[cfg(unix)]
#[tokio::test]
async fn a_session_that_failed_to_dispatch_shows_as_ended_and_not_as_waiting() {
    let kestrel = Kestrel::dispatching(supervisor::binary()).await;
    Fixture::acme().declare(&kestrel).await;
    let workspace = a_queued_workspace_in(&kestrel, "acme", "kestrel").await;
    let session = kestrel.enqueue_session(workspace).await;

    let ended = kestrel.answered(session.id, 1).await;
    assert_eq!(ended.state.as_str(), "ended");
    let queue = queue_read(&kestrel).await;
    assert_eq!(queue["queued"], json!([]), "{queue}");
    assert_eq!(queue["waiting"], json!([]), "{queue}");
    let (status, shown) = got(
        &kestrel,
        &format!("/operator/organizations/acme/sessions/{}", session.id),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{shown}");
    assert_eq!(shown["state"], "ended");
    assert_eq!(shown["exit"]["status"], "failed", "{shown}");

    kestrel.teardown().await;
}
