//! Why a queued Session is held: the instance limit, full slots, its Subscription Profile and a
//! trailing occupant.

use crate::support;

use kestrel::domain::WorkspaceId;
use kestrel::instance::{Git, Observed};
use reqwest::StatusCode;
use serde_json::{Value, json};
use support::Kestrel;
use support::client;
use support::fixture::Fixture;
use support::operator_queue::{
    a_queued_workspace_in, a_waiting_session, jacks_workspace, numbered, occupants, profile_reason,
    queue_of, queue_read, sharing_a_serialized_profile, waiting_row,
};
use support::operator_requests::{client, got};

fn clean_checkout() -> Vec<Observed> {
    vec![Observed {
        repository: "https://github.com/jtmthf/kestrel".to_owned(),
        git: Git::Read {
            branch: Some("kestrel/work".to_owned()),
            untracked: 0,
            uncommitted: 0,
            stashes: 0,
            unpushed: 0,
        },
    }]
}

/// An Organization of `maximum` live Instances whose `idle` Workspaces each keep one, clean.
async fn at_the_instance_limit(kestrel: &Kestrel, maximum: usize, idle: usize) -> Vec<WorkspaceId> {
    Fixture::acme().limited_to(maximum).declare(kestrel).await;

    let mut kept = Vec::new();
    for place in 0..idle {
        let workspace = a_queued_workspace_in(kestrel, "acme", "kestrel").await;
        let session = kestrel.dispatch_session(workspace).await;
        kestrel
            .executes_on(&session, &format!("docker/kestrel-{place}"))
            .await;
        kestrel.report_checkout(&session, clean_checkout()).await;
        kestrel.complete_session(&session).await;
        kept.push(workspace);
    }

    kept
}

fn reasons_of<'a>(queue: &'a Value, section: &str, name: &str) -> &'a Value {
    let row = queue[section]
        .as_array()
        .unwrap_or_else(|| panic!("the snapshot's {section} Sessions: {queue}"))
        .iter()
        .find(|row| row["name"] == name)
        .unwrap_or_else(|| panic!("no {section} Session {name}: {queue}"));

    &row["reasons"]
}

#[tokio::test]
async fn reading_the_queue_at_the_instance_limit_archives_nothing_and_dispatch_still_does() {
    let kestrel = Kestrel::boot().await;
    at_the_instance_limit(&kestrel, 1, 1).await;
    let arriving = a_queued_workspace_in(&kestrel, "acme", "kestrel").await;
    let queued = kestrel.enqueue_session(arriving).await;

    let (status, queue) = got(&kestrel, &queue_of("acme")).await;
    assert_eq!(status, StatusCode::OK, "{queue}");
    assert_eq!(
        *reasons_of(&queue, "queued", &queued.name),
        json!([{ "kind": "instance_archiving", "instance": "docker/kestrel-0" }])
    );
    assert_eq!(numbered(&queue)[0]["position"], Value::Null);
    assert!(
        kestrel.instances_to_archive().await.is_empty(),
        "reading the queue archived an Instance"
    );

    assert!(kestrel.occupy_session().await.is_none());
    assert_eq!(kestrel.instances_to_archive().await, ["docker/kestrel-0"]);
    let (_, queue) = got(&kestrel, &queue_of("acme")).await;
    assert_eq!(
        *reasons_of(&queue, "queued", &queued.name),
        json!([{ "kind": "instance_archiving", "instance": "docker/kestrel-0" }])
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_queued_session_names_each_reason_that_holds_it_alone_and_together() {
    let kestrel = Kestrel::boot().await;
    sharing_a_serialized_profile(&kestrel, Some(2), 4).await;
    let holding = kestrel
        .dispatch_session(jacks_workspace(&kestrel).await)
        .await;
    kestrel.executes_on(&holding, "docker/holding").await;
    let held = kestrel
        .enqueue_session(jacks_workspace(&kestrel).await)
        .await;
    let blocked = kestrel
        .enqueue_session(a_queued_workspace_in(&kestrel, "acme", "kestrel").await)
        .await;
    kestrel.block_session(&blocked, &held).await;
    let profile = profile_reason(&holding);
    let dependencies = json!({ "kind": "dependencies", "sessions": [held.name] });

    let queue = queue_read(&kestrel).await;
    assert_eq!(*reasons_of(&queue, "queued", &held.name), json!([profile]));
    assert_eq!(
        *reasons_of(&queue, "queued", &blocked.name),
        json!([dependencies])
    );

    let filling = kestrel
        .dispatch_session(a_queued_workspace_in(&kestrel, "acme", "kestrel").await)
        .await;
    kestrel.executes_on(&filling, "docker/filling").await;
    let limited = kestrel
        .enqueue_session(a_queued_workspace_in(&kestrel, "acme", "kestrel").await)
        .await;
    let limit = json!({ "kind": "live_instance_limit", "limit": 2 });

    let queue = queue_read(&kestrel).await;
    assert_eq!(
        *reasons_of(&queue, "queued", &held.name),
        json!([profile, limit])
    );
    assert_eq!(
        *reasons_of(&queue, "queued", &blocked.name),
        json!([dependencies, limit])
    );
    assert_eq!(*reasons_of(&queue, "queued", &limited.name), json!([limit]));
    assert!(
        numbered(&queue).iter().all(|row| row["position"].is_null()),
        "a Session some reason holds was numbered: {queue}"
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_blocked_session_at_the_instance_limit_names_the_limit_rather_than_an_archival() {
    let kestrel = Kestrel::boot().await;
    at_the_instance_limit(&kestrel, 1, 1).await;
    let blocked = kestrel
        .enqueue_session(a_queued_workspace_in(&kestrel, "acme", "kestrel").await)
        .await;
    let never_ran = kestrel
        .enqueue_session(a_queued_workspace_in(&kestrel, "acme", "kestrel").await)
        .await;
    kestrel.block_session(&blocked, &never_ran).await;

    let queue = queue_read(&kestrel).await;
    assert_eq!(
        *reasons_of(&queue, "queued", &blocked.name),
        json!([
            { "kind": "dependencies", "sessions": [never_ran.name] },
            { "kind": "live_instance_limit", "limit": 1 },
        ])
    );
    assert_eq!(
        *reasons_of(&queue, "queued", &never_ran.name),
        json!([{ "kind": "instance_archiving", "instance": "docker/kestrel-0" }])
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_waiting_session_with_held_input_waits_on_full_slots_alone() {
    let kestrel = Kestrel::boot().await;
    sharing_a_serialized_profile(&kestrel, None, 1).await;
    let workspace = a_queued_workspace_in(&kestrel, "acme", "kestrel").await;
    let waiting = a_waiting_session(&kestrel, workspace).await;
    kestrel
        .dispatch_session(a_queued_workspace_in(&kestrel, "acme", "kestrel").await)
        .await;
    kestrel
        .post_while_busy(workspace, "jack", "one more thing")
        .await;

    let queue = queue_read(&kestrel).await;
    assert_eq!(
        waiting_row(&queue, &waiting.name)["reasons"],
        json!([{ "kind": "active_work_slots", "limit": 1 }])
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_ready_session_behind_full_slots_keeps_its_place() {
    let kestrel = Kestrel::boot().await;
    sharing_a_serialized_profile(&kestrel, None, 1).await;
    let working = kestrel
        .dispatch_session(a_queued_workspace_in(&kestrel, "acme", "kestrel").await)
        .await;
    let first = kestrel
        .enqueue_session(a_queued_workspace_in(&kestrel, "acme", "kestrel").await)
        .await;
    let second = kestrel
        .enqueue_session(a_queued_workspace_in(&kestrel, "acme", "kestrel").await)
        .await;

    let queue = queue_read(&kestrel).await;
    assert_eq!(queue["active_work"]["limit"], 1);
    assert_eq!(queue["active_work"]["occupied"], 1);
    assert_eq!(queue["active_work"]["elsewhere"], 0);
    assert_eq!(occupants(&queue), [(json!(working.name), json!("working"))]);
    assert_eq!(
        numbered(&queue)
            .iter()
            .map(|row| (
                row["name"].clone(),
                row["position"].clone(),
                row["reasons"].clone()
            ))
            .collect::<Vec<_>>(),
        [
            (json!(first.name), json!(1), json!([])),
            (
                json!(second.name),
                json!(2),
                json!([{ "kind": "ahead", "sessions": [first.name] }])
            ),
        ]
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_trailing_session_holds_its_slot_until_it_settles_into_waiting() {
    let kestrel = Kestrel::boot().await;
    sharing_a_serialized_profile(&kestrel, None, 2).await;
    let trailing = kestrel
        .dispatch_session(a_queued_workspace_in(&kestrel, "acme", "kestrel").await)
        .await;
    kestrel.on_the_link(&trailing).await;
    kestrel.start_on_the_link(&trailing).await;
    kestrel.report_answered(&trailing, 1).await;
    let working = kestrel
        .dispatch_session(a_queued_workspace_in(&kestrel, "acme", "kestrel").await)
        .await;

    let queue = queue_read(&kestrel).await;
    assert_eq!(
        occupants(&queue),
        [
            (json!(trailing.name), json!("trailing")),
            (json!(working.name), json!("working")),
        ]
    );
    assert_eq!(queue["active_work"]["occupied"], 2);
    assert_eq!(queue["waiting"], json!([]));

    kestrel.report_settled(&trailing, 2).await;

    let queue = queue_read(&kestrel).await;
    assert_eq!(occupants(&queue), [(json!(working.name), json!("working"))]);
    waiting_row(&queue, &trailing.name);

    kestrel.teardown().await;
}

#[tokio::test]
async fn kestrel_queue_says_each_occupants_phase_on_its_row() {
    let kestrel = Kestrel::boot().await;
    sharing_a_serialized_profile(&kestrel, None, 2).await;
    let trailing = kestrel
        .dispatch_session(a_queued_workspace_in(&kestrel, "acme", "kestrel").await)
        .await;
    kestrel.on_the_link(&trailing).await;
    kestrel.start_on_the_link(&trailing).await;
    kestrel.report_answered(&trailing, 1).await;
    let working = kestrel
        .dispatch_session(a_queued_workspace_in(&kestrel, "acme", "kestrel").await)
        .await;

    let operator = kestrel.operator();
    let shown = tokio::task::spawn_blocking(move || {
        client::ran_on_a_terminal(&operator, &["queue", "--organization", "acme"], 240, "")
    })
    .await
    .expect("the client should run");
    assert!(shown.status.success(), "{}", shown.said);
    for (session, phase) in [(&trailing, "trailing"), (&working, "working")] {
        let row = shown
            .said
            .lines()
            .find(|line| line.contains(&session.name) && line.contains(phase))
            .unwrap_or_else(|| panic!("no row says {} is {phase}: {}", session.name, shown.said));
        assert!(!row.contains("waiting"), "{row}");
    }

    let queue = client(&kestrel, &["queue", "--organization", "acme", "--json"])
        .await
        .json();
    let occupants: Vec<Value> = queue["active_work"]["occupants"]
        .as_array()
        .expect("the occupants")
        .iter()
        .map(|occupant| json!({ "name": occupant["name"], "phase": occupant["phase"] }))
        .collect();
    assert_eq!(
        occupants,
        [
            json!({ "name": trailing.name, "phase": "trailing" }),
            json!({ "name": working.name, "phase": "working" }),
        ]
    );

    kestrel.teardown().await;
}
