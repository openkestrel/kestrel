//! The queue a Client reads: enqueue order, positions, blockers and the recorded dispatch limits.

use crate::support;

use reqwest::StatusCode;
use serde_json::{Value, json};
use support::Kestrel;
use support::fixture::Fixture;
use support::operator_queue::{a_queued_workspace_in, numbered, occupants, queue_of};
use support::operator_requests::got;
use support::supervisor;

#[tokio::test]
async fn a_client_reads_the_queue_in_enqueue_order_with_positions_its_blockers_and_limits() {
    let kestrel = Kestrel::boot().await;
    kestrel.record_dispatch(8, "local-exec").await;
    Fixture::acme().declare(&kestrel).await;

    let first = a_queued_workspace_in(&kestrel, "acme", "kestrel").await;
    let second = a_queued_workspace_in(&kestrel, "acme", "kestrel").await;
    let third = a_queued_workspace_in(&kestrel, "acme", "kestrel").await;
    let fourth = a_queued_workspace_in(&kestrel, "acme", "kestrel").await;
    let blocker = kestrel.enqueue_session(first).await;
    let ahead = kestrel.enqueue_session(second).await;
    let blocked = kestrel.enqueue_session(third).await;
    let behind = kestrel.enqueue_session(fourth).await;
    kestrel.block_session(&blocked, &blocker).await;

    let (status, queue) = got(&kestrel, &queue_of("acme")).await;
    assert_eq!(status, StatusCode::OK, "{queue}");
    let said = numbered(&queue);
    assert_eq!(
        said.iter()
            .map(|row| row["name"].as_str().expect("a generated name"))
            .collect::<Vec<_>>(),
        [
            blocker.name.as_str(),
            ahead.name.as_str(),
            blocked.name.as_str(),
            behind.name.as_str(),
        ],
        "the queue is not in enqueue order"
    );
    assert_eq!(
        said.iter()
            .map(|row| row["position"].as_u64())
            .collect::<Vec<_>>(),
        [Some(1), Some(2), None, Some(3)]
    );
    assert_eq!(
        said[2]["reasons"],
        json!([{ "kind": "dependencies", "sessions": [blocker.name] }])
    );
    assert_eq!(said[0]["reasons"], json!([]));

    // With the blocker succeeded, the Session it delayed takes the place it was enqueued into.
    kestrel.complete_session(&blocker).await;
    let (status, queue) = got(&kestrel, &queue_of("acme")).await;
    assert_eq!(status, StatusCode::OK, "{queue}");
    assert_eq!(
        numbered(&queue)
            .iter()
            .map(|row| row["position"].as_u64())
            .collect::<Vec<_>>(),
        [Some(1), Some(2), Some(3)],
    );
    assert_eq!(
        numbered(&queue)
            .iter()
            .map(|row| row["reasons"].as_array().map(Vec::len))
            .collect::<Vec<_>>(),
        [Some(0), Some(1), Some(1)]
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_session_of_another_organization_occupying_the_shared_slots_is_counted_and_never_named() {
    let kestrel = Kestrel::boot().await;
    for name in ["acme", "globex"] {
        Fixture::acme()
            .organization(name)
            .project(name)
            .declare(&kestrel)
            .await;
    }

    let ours = a_queued_workspace_in(&kestrel, "acme", "acme").await;
    let held = kestrel.dispatch_session(ours).await;
    let elsewhere = a_queued_workspace_in(&kestrel, "globex", "globex").await;
    let hit = kestrel.dispatch_session(elsewhere).await;

    let (status, queue) = got(&kestrel, &queue_of("acme")).await;
    assert_eq!(status, StatusCode::OK, "{queue}");
    assert_eq!(queue["active_work"]["occupied"], 2);
    assert_eq!(
        occupants(&queue),
        [(json!(held.name), json!("working"))],
        "the Session of another Organization is named by ours"
    );
    assert_eq!(queue["active_work"]["elsewhere"], 1);

    let (status, their_queue) = got(&kestrel, &queue_of("globex")).await;
    assert_eq!(status, StatusCode::OK, "{their_queue}");
    assert_eq!(
        occupants(&their_queue),
        [(json!(hit.name), json!("working"))]
    );
    assert_eq!(their_queue["active_work"]["elsewhere"], 1);

    kestrel.teardown().await;
}

#[tokio::test]
async fn the_queue_says_when_a_live_instance_limit_is_unbounded_and_what_counts_against_it() {
    let kestrel = Kestrel::boot().await;
    Fixture::acme().declare(&kestrel).await;
    Fixture::acme()
        .organization("bounded")
        .limited_to(1)
        .project("held")
        .declare(&kestrel)
        .await;

    let workspace = kestrel.open_workspace("acme", "kestrel", "builder").await;
    let held = kestrel.dispatch_session(workspace.id).await;
    kestrel.executes_on(&held, "docker/kestrel-a").await;
    kestrel.complete_session(&held).await;

    let (status, unbounded) = got(&kestrel, &queue_of("acme")).await;
    assert_eq!(status, StatusCode::OK, "{unbounded}");
    assert_eq!(
        unbounded["instances"],
        json!({
            "limit": null,
            "count": 1,
            "counted": ["docker/kestrel-a"],
        })
    );

    let (status, limited) = got(&kestrel, &queue_of("bounded")).await;
    assert_eq!(status, StatusCode::OK, "{limited}");
    assert_eq!(
        limited["instances"],
        json!({
            "limit": 1,
            "count": 0,
            "counted": [],
        })
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn the_queue_reads_the_recorded_dispatch_which_a_restart_with_new_flags_replaces() {
    let supervisor_path = supervisor::binary().to_owned();

    // The record is written as a work role that can dispatch starts, a little after the
    // boundary answers, so both reads wait for it.
    async fn recorded(kestrel: &Kestrel, slots: usize) -> Value {
        kestrel.declare_organization("acme").await;
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            let (status, said) = got(kestrel, &queue_of("acme")).await;
            assert_eq!(status, StatusCode::OK, "{said}");
            if said["active_work"]["limit"] == json!(slots) {
                return said;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "the snapshot never said the work role dispatches {slots} slots: {said}"
            );
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    }

    let kestrel = Kestrel::dispatching_up_to(&supervisor_path, "-", 3).await;
    kestrel.declare_organization("acme").await;
    let first = recorded(&kestrel, 3).await;
    assert_eq!(
        first["work_role"],
        json!({
            "active_work_slots": 3,
            "serialized_harnesses": ["codex"],
            "driver": "local-exec",
        }),
    );

    let restarted = kestrel
        .teardown()
        .await
        .restart_with(&supervisor_path, "-", 5)
        .await;
    let replaced = recorded(&restarted, 5).await;
    assert_eq!(
        replaced["work_role"],
        json!({
            "active_work_slots": 5,
            "serialized_harnesses": ["codex"],
            "driver": "local-exec",
        }),
    );

    // The queue is read for acme whatever workspaces it has or does not have.
    assert_eq!(replaced["queued"], json!([]));
    assert_eq!(replaced["active_work"]["occupied"], 0);

    restarted.teardown().await;
}

#[tokio::test]
async fn a_queue_without_a_recorded_dispatch_leaves_positions_unknown() {
    let kestrel = Kestrel::boot().await;
    Fixture::acme().declare(&kestrel).await;
    let first = a_queued_workspace_in(&kestrel, "acme", "kestrel").await;
    let second = a_queued_workspace_in(&kestrel, "acme", "kestrel").await;
    kestrel.enqueue_session(first).await;
    kestrel.enqueue_session(second).await;

    let (status, queue) = got(&kestrel, &queue_of("acme")).await;
    assert_eq!(status, StatusCode::OK, "{queue}");
    assert_eq!(queue["work_role"], Value::Null);
    assert_eq!(queue["active_work"]["limit"], Value::Null);
    assert_eq!(queue["active_work"]["occupied"], 0);
    assert_eq!(
        numbered(&queue)
            .iter()
            .map(|row| row["position"].as_u64())
            .collect::<Vec<_>>(),
        [None, None]
    );

    kestrel.teardown().await;
}
