//! Fixtures the queue suites share: Workspaces queued, waiting or unbriefed, and the rows a queue
//! read answers.

use super::fixture::Fixture;
use super::operator_requests::got;
use super::{Kestrel, SERIALIZED};
use kestrel::domain::WorkspaceId;
use kestrel::operator;
use reqwest::StatusCode;
use serde_json::{Value, json};

pub fn queue_of(organization: &str) -> String {
    operator::QUEUE.replace("{organization}", organization)
}

/// A Workspace whose enqueued Session is queued; a Workspace has one Session at a time, so a
/// queue of many needs as many of these.
pub async fn a_queued_workspace_in(
    kestrel: &Kestrel,
    organization: &str,
    project: &str,
) -> WorkspaceId {
    kestrel
        .open_workspace(organization, project, "builder")
        .await
        .id
}

/// The queue rows as the snapshot served them, in the order read.
pub fn numbered(said: &Value) -> Vec<Value> {
    said["queued"]
        .as_array()
        .expect("the snapshot's queued Sessions")
        .clone()
}

pub fn profile_reason(holding: &kestrel::domain::Session) -> Value {
    json!({
        "kind": "subscription_profile", "profile": "jack", "session": holding.name,
        "diagnostic": {
            "kind": "state_conflict", "field": null,
            "message": format!("the {} sign-in in the subscription profile jack is in use by the session {}", holding.agent.harness, holding.name),
            "context": { "operation": "use_sign_in", "resource": "subscription_profile",
                "reference": "jack", "organization": "acme", "state": "credential_in_use", "holding_session": holding.name },
            "next_steps": [
                { "action": "inspect_resource", "resource": "session", "reference": holding.name, "organization": "acme" },
                { "action": "stop_session", "organization": "acme", "session": holding.id.to_string(),
                    "consequence": "Stopping the session now records it failed.", "effect": "fails_session", "requires_choice": true }
            ]
        }
    })
}

/// An Organization whose `builder` runs a serialized harness, with the Subscription Profile
/// `jack` for its Workspaces to share, and a work role recorded dispatching `slots`.
pub async fn sharing_a_serialized_profile(kestrel: &Kestrel, maximum: Option<usize>, slots: usize) {
    let fixture = Fixture::acme().harness(SERIALIZED);
    match maximum {
        Some(maximum) => fixture.limited_to(maximum),
        None => fixture,
    }
    .declare(kestrel)
    .await;
    kestrel
        .declare_profile("acme", "jack", "Jack")
        .await
        .expect("the profile should declare");
    kestrel.record_dispatch(slots, "local-exec").await;
}

pub async fn jacks_workspace(kestrel: &Kestrel) -> WorkspaceId {
    kestrel
        .open_workspace_with("acme", "kestrel", "builder", "jack")
        .await
        .id
}

pub async fn queue_read(kestrel: &Kestrel) -> Value {
    let (status, queue) = got(kestrel, &queue_of("acme")).await;
    assert_eq!(status, StatusCode::OK, "{queue}");
    queue
}

pub async fn a_waiting_session(
    kestrel: &Kestrel,
    workspace: WorkspaceId,
) -> kestrel::domain::Session {
    let session = kestrel.dispatch_session(workspace).await;
    kestrel.waits_after_its_first_turn(&session).await
}

pub fn occupants(queue: &Value) -> Vec<(Value, Value)> {
    queue["active_work"]["occupants"]
        .as_array()
        .unwrap_or_else(|| panic!("the snapshot's occupants: {queue}"))
        .iter()
        .map(|row| (row["name"].clone(), row["phase"].clone()))
        .collect()
}

pub fn waiting_row<'a>(queue: &'a Value, name: &str) -> &'a Value {
    queue["waiting"]
        .as_array()
        .unwrap_or_else(|| panic!("the snapshot's waiting Sessions: {queue}"))
        .iter()
        .find(|row| row["name"] == name)
        .unwrap_or_else(|| panic!("no waiting Session {name}: {queue}"))
}

pub fn unbriefed_row<'a>(queue: &'a Value, name: &str) -> &'a Value {
    queue["unbriefed"]
        .as_array()
        .unwrap_or_else(|| panic!("the snapshot's unbriefed Sessions: {queue}"))
        .iter()
        .find(|row| row["name"] == name)
        .unwrap_or_else(|| panic!("no unbriefed Session {name}: {queue}"))
}

/// Its harness is not up yet, so a message posted to it is held.
pub async fn an_unbriefed_session(
    kestrel: &Kestrel,
    organization: &str,
    project: &str,
    instance: &str,
) -> (WorkspaceId, kestrel::domain::Session) {
    let workspace = a_queued_workspace_in(kestrel, organization, project).await;
    let queued = kestrel.enqueue_session_with_nothing_posted(workspace).await;
    let session = kestrel
        .claim_session()
        .await
        .expect("the unbriefed session should claim");
    assert_eq!(session.id, queued.id);
    kestrel.executes_on(&session, instance).await;

    (workspace, session)
}
