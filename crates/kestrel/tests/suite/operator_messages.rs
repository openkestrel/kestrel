//! Posting to a Workspace: unbriefed Sessions, held messages, a name's first turn, and sealing
//! what holds unpublished work.

use crate::support;

use kestrel::domain::{Preparing, SessionState};
use kestrel::instance::{Git, Observed};
use kestrel::log::Entry;
use kestrel::operator;
use reqwest::StatusCode;
use serde_json::json;
use support::Kestrel;
use support::client;
use support::fixture::Fixture;
use support::operator_requests::{
    client, declared, failed, got, payload_entries, ready_to_open, recorded, requested, session_at,
    workspace_at, workspace_messages_at, workspace_of, workspace_seal_at,
};

fn workspace_message_at(organization: &str, workspace: &str, message: i64) -> String {
    operator::WORKSPACE_MESSAGE
        .replace("{organization}", organization)
        .replace("{workspace}", workspace)
        .replace("{id}", &message.to_string())
}

#[tokio::test]
async fn an_unbriefed_session_shows_its_state_and_preparing_step_everywhere() {
    let kestrel = Kestrel::boot().await;
    ready_to_open(&kestrel).await;
    let workspaces = operator::WORKSPACES.replace("{organization}", "acme");

    let (status, opened) = declared(
        &kestrel,
        &workspaces,
        &json!({ "project": "kestrel", "agent": "builder" }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{opened}");
    let workspace = workspace_of(&opened["workspace"]).to_string();
    let session = opened["session"]["id"]
        .as_str()
        .expect("a session id")
        .to_owned();

    let queued = recorded(
        &client(
            &kestrel,
            &[
                "session",
                "show",
                &session,
                "--organization",
                "acme",
                "--json",
            ],
        )
        .await,
    );
    assert_eq!(
        client::picked(&queued[0], &["state", "preparing"]),
        json!({ "state": "queued", "preparing": null })
    );

    let claimed = kestrel
        .claim_session()
        .await
        .expect("the queued session should claim");
    assert_eq!(claimed.state, SessionState::Unbriefed);
    assert_eq!(claimed.preparing, Some(Preparing::Provisioning));

    let provisioning = recorded(
        &client(
            &kestrel,
            &[
                "session",
                "show",
                &session,
                "--organization",
                "acme",
                "--json",
            ],
        )
        .await,
    );
    assert_eq!(
        client::picked(&provisioning[0], &["state", "preparing"]),
        json!({ "state": "unbriefed", "preparing": "provisioning" })
    );

    kestrel.report_ready(&claimed).await;
    let ready = recorded(
        &client(
            &kestrel,
            &[
                "session",
                "show",
                &session,
                "--organization",
                "acme",
                "--json",
            ],
        )
        .await,
    );
    assert_eq!(
        client::picked(&ready[0], &["state", "preparing"]),
        json!({ "state": "unbriefed", "preparing": "harness_ready" })
    );

    let listed = recorded(
        &client(
            &kestrel,
            &["session", "list", "--workspace", &workspace, "--json"],
        )
        .await,
    );
    assert_eq!(listed.len(), 1, "{listed:?}");
    assert_eq!(
        client::picked(&listed[0], &["state", "preparing"]),
        json!({ "state": "unbriefed", "preparing": "harness_ready" })
    );

    let shown = recorded(&client(&kestrel, &["workspace", "show", &workspace, "--json"]).await);
    assert_eq!(shown[0]["unfinished_session"]["state"], "unbriefed");
    assert_eq!(shown[0]["unfinished_session"]["preparing"], "harness_ready");

    let (status, read) = got(&kestrel, &session_at("acme", &session)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(read["state"], "unbriefed");
    assert_eq!(read["preparing"], "harness_ready");
    let (status, read) = got(&kestrel, &workspace_at("acme", &workspace)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(read["unfinished_session"]["state"], "unbriefed");
    assert_eq!(read["unfinished_session"]["preparing"], "harness_ready");

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_post_names_its_participant_and_refuses_an_agents_name() {
    let kestrel = Kestrel::boot().await;
    Fixture::acme()
        .agent("reviewer", "opencode", None)
        .declare(&kestrel)
        .await;
    let workspace = kestrel.open_workspace("acme", "kestrel", "builder").await;
    let messages = workspace_messages_at("acme", &workspace.id.to_string());

    for participant in [
        json!("   "),
        json!("x".repeat(65)),
        json!("con\u{7}trol"),
        json!("builder"),
        json!("reviewer"),
    ] {
        let body = json!({ "participant": participant, "message": "hello" });

        let (status, refusal) = declared(&kestrel, &messages, &body).await;
        assert_eq!(
            status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{body}: {refusal}"
        );
        assert_eq!(refusal["field"], "participant", "{body}: {refusal}");
    }

    kestrel.teardown().await;
}

#[tokio::test]
async fn held_messages_are_listed_edited_and_withdrawn_over_the_boundary() {
    let kestrel = Kestrel::boot().await;
    Fixture::acme().declare(&kestrel).await;
    let workspace = kestrel.open_workspace("acme", "kestrel", "builder").await;
    let active = kestrel.dispatch_session(workspace.id).await;
    let messages = workspace_messages_at("acme", &workspace.id.to_string());
    let shown = workspace_at("acme", &workspace.id.to_string());

    let (status, posted) = declared(
        &kestrel,
        &messages,
        &json!({ "participant": "alice", "message": "one more change" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let typed: kestrel_operator_types::Posted = serde_json::from_value(posted.clone()).unwrap();
    assert_eq!(serde_json::to_value(typed).unwrap(), posted);
    assert!(posted["session"].is_null(), "{posted}");
    let id = posted["held_message"]["id"]
        .as_i64()
        .unwrap_or_else(|| panic!("the post answers with the held id: {posted}"));
    assert_eq!(posted["held_message"]["participant"], "alice");
    assert_eq!(posted["held_message"]["message"], "one more change");
    assert!(posted["held_message"]["edited_at"].is_null());
    let at = workspace_message_at("acme", &workspace.id.to_string(), id);

    let (status, shown_workspace) = got(&kestrel, &shown).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(shown_workspace["held_messages"][0]["id"], id);
    assert_eq!(
        shown_workspace["held_messages"][0]["message"],
        "one more change"
    );

    let before = payload_entries(&kestrel, &workspace.id.to_string()).await;
    let (status, edited) = requested(
        &kestrel,
        reqwest::Method::PUT,
        &at,
        Some(&json!({ "participant": "alice", "message": "the edited change" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{edited}");
    let typed: kestrel_operator_types::HeldMessage =
        serde_json::from_value(edited.clone()).unwrap();
    assert_eq!(serde_json::to_value(typed).unwrap(), edited);
    assert_eq!(edited["id"], id);
    assert_eq!(edited["message"], "the edited change");
    assert!(!edited["edited_at"].is_null(), "an edit is stamped");
    assert_eq!(
        payload_entries(&kestrel, &workspace.id.to_string()).await,
        before,
        "an edit writes no Transcript entry"
    );

    let (status, refusal) = requested(
        &kestrel,
        reqwest::Method::PUT,
        &at,
        Some(&json!({ "participant": "bob", "message": "mine now" })),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{refusal}");
    assert_eq!(refusal["kind"], "forbidden_action");
    assert_eq!(refusal["context"]["operation"], "edit_workspace_message");
    assert_eq!(refusal["context"]["resource"], "held_message");
    assert_eq!(refusal["context"]["reference"], id.to_string());
    assert_eq!(refusal["context"]["constraint"], "author_only");
    assert_eq!(
        refusal["next_steps"],
        json!([{
            "action": "inspect_resource",
            "resource": "workspace",
            "reference": workspace.id.to_string(),
            "organization": "acme",
        }])
    );
    let (status, refusal) = requested(
        &kestrel,
        reqwest::Method::DELETE,
        &at,
        Some(&json!({ "participant": "bob" })),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{refusal}");
    let (status, refusal) = requested(
        &kestrel,
        reqwest::Method::PUT,
        &at,
        Some(&json!({ "participant": "builder", "message": "mine now" })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{refusal}");
    assert_eq!(refusal["field"], "participant", "{refusal}");

    let (status, withdrawal) = requested(
        &kestrel,
        reqwest::Method::DELETE,
        &at,
        Some(&json!({ "participant": "alice" })),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{withdrawal}");
    assert!(withdrawal.is_null(), "{withdrawal}");
    assert_eq!(
        payload_entries(&kestrel, &workspace.id.to_string()).await,
        before,
        "a withdrawal writes no Transcript entry"
    );

    let (status, refusal) = requested(
        &kestrel,
        reqwest::Method::PUT,
        &at,
        Some(&json!({ "participant": "alice", "message": "changed my mind" })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{refusal}");
    assert!(
        refusal["message"]
            .as_str()
            .is_some_and(|why| why.contains("already withdrawn")),
        "{refusal}"
    );
    assert_eq!(refusal["context"]["resource"], "held_message");
    assert_eq!(refusal["context"]["state"], "withdrawn");
    assert_eq!(refusal["next_steps"][0]["resource"], "workspace");

    let (status, posted) = declared(
        &kestrel,
        &messages,
        &json!({ "participant": "alice", "message": "a later thing" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let later = posted["held_message"]["id"]
        .as_i64()
        .expect("the later message's id");
    assert_ne!(
        later, id,
        "a message posted after one drained gets a new id"
    );
    kestrel.complete_session(&active).await;
    kestrel
        .claim_session()
        .await
        .expect("the later message starts a Session");
    let (status, refusal) = requested(
        &kestrel,
        reqwest::Method::DELETE,
        &workspace_message_at("acme", &workspace.id.to_string(), later),
        Some(&json!({ "participant": "alice" })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{refusal}");
    assert!(
        refusal["message"]
            .as_str()
            .is_some_and(|why| why.contains("already sent to the agent")),
        "{refusal}"
    );

    for id in ["424242", "not-a-number"] {
        let (status, refusal) = requested(
            &kestrel,
            reqwest::Method::PUT,
            &operator::WORKSPACE_MESSAGE
                .replace("{organization}", "acme")
                .replace("{workspace}", &workspace.id.to_string())
                .replace("{id}", id),
            Some(&json!({ "participant": "alice", "message": "nowhere" })),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{refusal}");
        assert_eq!(refusal["kind"], "missing_reference");
        assert_eq!(refusal["context"]["resource"], "held_message");
        assert_eq!(refusal["context"]["reference"], id);
        assert_eq!(refusal["next_steps"][0]["action"], "inspect_resource");
        assert_eq!(refusal["next_steps"][0]["resource"], "workspace");
    }

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_names_first_turn_joins_once_and_a_later_turn_does_not() {
    let kestrel = Kestrel::boot().await;
    Fixture::acme().declare(&kestrel).await;
    let workspace = kestrel.open_workspace("acme", "kestrel", "builder").await;

    kestrel.post(workspace.id, "alice", "the first thing").await;
    kestrel
        .post_while_busy(workspace.id, "alice", "the second thing")
        .await;

    let entries: Vec<Entry> = kestrel
        .transcript(workspace.id)
        .await
        .into_iter()
        .map(|recorded| recorded.entry)
        .collect();
    assert_eq!(
        entries,
        vec![
            Entry::ParticipantJoined {
                participant: "builder".to_owned(),
            },
            Entry::ParticipantJoined {
                participant: "alice".to_owned(),
            },
            Entry::Said {
                participant: "alice".to_owned(),
                message: "the first thing".to_owned(),
                session_id: None,
                completion: None,
            },
            Entry::Said {
                participant: "alice".to_owned(),
                message: "the second thing".to_owned(),
                session_id: None,
                completion: None,
            },
        ]
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn sealing_a_workspace_whose_instance_holds_unpublished_work_is_a_conflict_not_an_outage() {
    let kestrel = Kestrel::boot().await;
    Fixture::acme().declare(&kestrel).await;
    let workspace = kestrel.open_workspace("acme", "kestrel", "builder").await;

    let queued = kestrel.enqueue_session(workspace.id).await;
    let claimed = kestrel
        .occupy_session()
        .await
        .expect("the session should claim");
    assert_eq!(claimed.id, queued.id);
    kestrel.executes_on(&claimed, "held").await;
    kestrel
        .report_checkout(
            &claimed,
            vec![Observed {
                repository: "https://github.com/jtmthf/kestrel".to_owned(),
                git: Git::Read {
                    branch: Some("main".to_owned()),
                    untracked: 0,
                    uncommitted: 1,
                    stashes: 0,
                    unpushed: 0,
                },
            }],
        )
        .await;
    kestrel.complete_session(&claimed).await;

    let workspace_id = workspace.id.to_string();
    let (status, refusal) = declared(
        &kestrel,
        &workspace_seal_at("acme", &workspace_id),
        &json!({}),
    )
    .await;

    assert_eq!(status, StatusCode::CONFLICT, "{refusal}");
    let message = refusal["message"].as_str().expect("a message");
    assert!(
        message.contains("may hold the only copy of its work"),
        "{refusal}"
    );

    let rejected = client(&kestrel, &["workspace", "seal", &workspace_id]).await;
    assert!(
        failed(&rejected).contains("may hold the only copy of its work"),
        "{}",
        rejected.err
    );
    assert_eq!(
        rejected.status.code(),
        Some(4),
        "a refusal should exit rejected, not unavailable: {}",
        rejected.err
    );

    kestrel.teardown().await;
}
