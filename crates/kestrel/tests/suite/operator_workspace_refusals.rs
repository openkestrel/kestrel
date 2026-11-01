//! The answers and refusals the operator document gives Workspace and Session operations, and the
//! state a conflict carries.

use crate::support;

use kestrel::operator;
use reqwest::StatusCode;
use serde_json::{Value, json};
use support::Kestrel;
use support::fixture::Fixture;
use support::operator_requests::{
    declared, got, ready_to_open, session_at, sessions_of, workspace_at, workspace_messages_at,
    workspace_seal_at,
};

#[tokio::test]
async fn the_operator_documents_workspace_and_session_answers_and_refusals() {
    let kestrel = Kestrel::boot().await;
    Fixture::acme().declare(&kestrel).await;
    let workspaces = operator::WORKSPACES.replace("{organization}", "acme");

    let (status, _) = got(&kestrel, &workspaces).await;
    assert_eq!(status, StatusCode::OK);
    let (status, refused) = declared(
        &kestrel,
        &workspaces,
        &json!({ "project": "nowhere", "agent": "builder" }),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(refused["field"], "project");
    let (status, opened) = declared(
        &kestrel,
        &workspaces,
        &json!({ "project": "kestrel", "agent": "builder" }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let workspace = opened["workspace"]["id"].as_str().expect("a workspace id");
    let session_id = opened["session"]["id"].as_str().expect("a session id");
    let session_name = opened["session"]["name"]
        .as_str()
        .expect("a generated session name");
    assert_eq!(opened["session"]["state"], "queued");
    let (status, listed) = got(&kestrel, &workspaces).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listed[0]["session"]["id"], session_id, "{listed}");
    assert!(listed[0]["queue"]["reasons"].is_array(), "{listed}");
    let shown = workspace_at("acme", workspace);
    let messages = workspace_messages_at("acme", workspace);
    let sessions = sessions_of("acme", workspace);

    let (status, _) = got(&kestrel, &shown).await;
    assert_eq!(status, StatusCode::OK);
    let (status, listed_sessions) = got(&kestrel, &sessions).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listed_sessions.as_array().expect("sessions").len(), 1);
    let (status, refused) = declared(&kestrel, &sessions, &json!({})).await;
    assert_eq!(status, StatusCode::CONFLICT, "{refused}");

    let (status, posted) = declared(
        &kestrel,
        &messages,
        &json!({ "participant": "operator", "message": "start with the operator boundary" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(posted["session"]["id"], session_id, "{posted}");
    assert!(posted["held_message"].is_null(), "{posted}");

    let (status, shown_session) = got(&kestrel, &session_at("acme", session_id)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(shown_session["workspace"], workspace);
    let (status, named_session) = got(&kestrel, &session_at("acme", session_name)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(named_session["id"], session_id);

    let session = kestrel
        .claim_session()
        .await
        .expect("the queued session should wait for the worker");
    kestrel.complete_session(&session).await;
    let seal = workspace_seal_at("acme", workspace);
    let (status, _) = declared(&kestrel, &seal, &json!({})).await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = declared(&kestrel, &seal, &json!({})).await;
    assert_eq!(status, StatusCode::CONFLICT);
    let (status, _) = declared(
        &kestrel,
        &workspaces,
        &json!({ "project": "kestrel", "agent": "builder", "continues": workspace }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let workspace_name = opened["workspace"]["name"]
        .as_str()
        .expect("a generated workspace name");
    let (status, _) = declared(
        &kestrel,
        &workspaces,
        &json!({ "project": "kestrel", "agent": "builder", "continues": workspace_name }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let nowhere = workspace_at("acme", "01a0a2d8-baf8-7c02-99fa-7280f174c14a");
    let (status, _) = got(&kestrel, &nowhere).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    kestrel.teardown().await;
}

/// A state conflict names the state, inspects before it offers anything that changes work, and
/// types the consequence and the choice a stopping step needs.
#[tokio::test]
async fn workspace_and_session_refusals_carry_their_state_and_inspect_first() {
    let kestrel = Kestrel::boot().await;
    ready_to_open(&kestrel).await;
    let workspaces = operator::WORKSPACES.replace("{organization}", "acme");
    let (_, opened) = declared(
        &kestrel,
        &workspaces,
        &json!({ "project": "kestrel", "agent": "builder" }),
    )
    .await;
    let workspace = opened["workspace"]["id"].as_str().expect("a workspace id");
    let session = opened["session"]["id"].as_str().expect("a session id");

    let (status, refusal) = declared(&kestrel, &sessions_of("acme", workspace), &json!({})).await;
    assert_eq!(status, StatusCode::CONFLICT, "{refusal}");
    assert_eq!(refusal["kind"], "state_conflict");
    assert_eq!(refusal["context"]["operation"], "enqueue_session");
    assert_eq!(refusal["context"]["resource"], "workspace");
    assert_eq!(refusal["context"]["reference"], workspace);
    assert_eq!(refusal["context"]["organization"], "acme");
    assert_eq!(refusal["context"]["state"], "session_unfinished");
    assert_eq!(refusal["context"]["holding_session"], session);
    assert_eq!(
        refusal["next_steps"],
        json!([
            {
                "action": "inspect_resource",
                "resource": "session",
                "reference": session,
                "organization": "acme",
            },
            {
                "action": "stop_session",
                "organization": "acme",
                "session": session,
                "consequence": "Stopping the session now records it failed.",
                "effect": "fails_session",
                "requires_choice": true,
            },
        ]),
        "{refusal}"
    );

    let (status, refusal) =
        declared(&kestrel, &workspace_seal_at("acme", workspace), &json!({})).await;
    assert_eq!(status, StatusCode::CONFLICT, "{refusal}");
    assert_eq!(refusal["context"]["state"], "session_in_flight");
    assert_eq!(refusal["context"]["holding_session"], session);
    assert_eq!(refusal["next_steps"][0]["action"], "inspect_resource");
    assert_eq!(refusal["next_steps"][1]["action"], "stop_session");

    let release = operator::WORKSPACE_INSTANCE_RELEASE
        .replace("{organization}", "acme")
        .replace("{workspace}", workspace);
    let (status, refusal) = declared(&kestrel, &release, &json!({ "participant": "alice" })).await;
    assert_eq!(status, StatusCode::CONFLICT, "{refusal}");
    assert_eq!(refusal["context"]["operation"], "release_instance");
    assert_eq!(refusal["context"]["state"], "no_instance");
    assert_eq!(
        refusal["next_steps"],
        json!([{
            "action": "inspect_resource",
            "resource": "workspace",
            "reference": workspace,
            "organization": "acme",
        }])
    );

    let interrupt = operator::SESSION_INTERRUPT
        .replace("{organization}", "acme")
        .replace("{session}", session);
    let (status, refusal) =
        declared(&kestrel, &interrupt, &json!({ "participant": "alice" })).await;
    assert_eq!(status, StatusCode::CONFLICT, "{refusal}");
    assert_eq!(refusal["context"]["resource"], "session");
    assert_eq!(refusal["context"]["reference"], session);
    assert_eq!(refusal["context"]["state"], "queued");
    assert_eq!(refusal["next_steps"].as_array().map(Vec::len), Some(1));

    let options = operator::SESSION_OPTIONS
        .replace("{organization}", "acme")
        .replace("{session}", session);
    let (status, refusal) = declared(
        &kestrel,
        &options,
        &json!({ "participant": "alice", "category": "colour", "value": "blue" }),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{refusal}");
    assert_eq!(refusal["kind"], "invalid_field");
    assert_eq!(refusal["field"], "category");
    assert_eq!(refusal["context"]["constraint"], "offered");
    assert_eq!(
        refusal["context"]["allowed_values"],
        json!(["model", "mode", "thought_level"])
    );
    assert_eq!(refusal["next_steps"][0]["action"], "correct_field");
    assert_eq!(refusal["next_steps"][0]["operation"], "set_session_option");
    let (status, refusal) = declared(
        &kestrel,
        &options,
        &json!({ "participant": "alice", "option": "model", "category": "model", "value": "x" }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{refusal}");
    assert_eq!(refusal["kind"], "malformed_request");
    assert_eq!(refusal["context"]["operation"], "set_session_option");
    assert_eq!(refusal["context"]["field"], "option");

    let unreadable = reqwest::Client::new()
        .post(format!(
            "{}{}",
            kestrel.operator(),
            workspace_messages_at("acme", workspace)
        ))
        .header("content-type", "application/json")
        .body("{")
        .send()
        .await
        .expect("the operator boundary should answer");
    assert_eq!(unreadable.status(), StatusCode::BAD_REQUEST);
    let refusal: Value = unreadable.json().await.expect("a JSON refusal");
    assert_eq!(refusal["kind"], "malformed_request");
    assert_eq!(refusal["context"]["operation"], "post_to_workspace");
    assert_eq!(
        refusal["next_steps"],
        json!([{
            "action": "inspect_operation",
            "operation": "post_to_workspace",
            "resource": null,
            "uncertain": false,
        }])
    );

    let (_, other) = declared(
        &kestrel,
        &workspaces,
        &json!({ "project": "kestrel", "agent": "builder" }),
    )
    .await;
    let other = other["workspace"]["id"].as_str().expect("a workspace id");
    let (status, refusal) = declared(
        &kestrel,
        &workspaces,
        &json!({ "project": "kestrel", "agent": "builder", "continues": other }),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{refusal}");
    assert_eq!(refusal["field"], "continues");
    assert_eq!(refusal["context"]["state"], "open");
    assert_eq!(
        refusal["next_steps"],
        json!([
            {
                "action": "inspect_resource",
                "resource": "workspace",
                "reference": other,
                "organization": "acme",
            },
            {
                "action": "enqueue_session",
                "organization": "acme",
                "workspace": other,
                "missing": [],
            },
        ])
    );

    let claimed = kestrel.claim_session().await.expect("the queued session");
    kestrel.complete_session(&claimed).await;
    let (status, _) = declared(&kestrel, &workspace_seal_at("acme", workspace), &json!({})).await;
    assert_eq!(status, StatusCode::OK);
    let (status, refusal) =
        declared(&kestrel, &workspace_seal_at("acme", workspace), &json!({})).await;
    assert_eq!(status, StatusCode::CONFLICT, "{refusal}");
    assert_eq!(refusal["context"]["state"], "sealed");
    let (status, refusal) = declared(
        &kestrel,
        &workspace_messages_at("acme", workspace),
        &json!({ "participant": "alice", "message": "one more thing" }),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{refusal}");
    assert_eq!(refusal["context"]["operation"], "post_to_workspace");
    assert_eq!(refusal["context"]["state"], "sealed");
    let stop = operator::SESSION_STOP
        .replace("{organization}", "acme")
        .replace("{session}", session);
    let (status, refusal) = declared(&kestrel, &stop, &json!({})).await;
    assert_eq!(status, StatusCode::CONFLICT, "{refusal}");
    assert_eq!(refusal["context"]["resource"], "session");
    assert_eq!(refusal["context"]["state"], "ended");
    assert_eq!(
        refusal["next_steps"].as_array().map(Vec::len),
        Some(1),
        "an ended session offers no stop: {refusal}"
    );

    kestrel.teardown().await;
}
