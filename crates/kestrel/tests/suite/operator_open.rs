//! Opening a Workspace over the operator boundary: its brief, its author and the Session it
//! queues.

use crate::support;

use kestrel::log::{BriefSource, Entry};
use kestrel::operator;
use reqwest::StatusCode;
use serde_json::json;
use support::Kestrel;
use support::client;
use support::fixture::Fixture;
use support::operator_requests::{
    a_start, agents_of, client, client_given, declared, failed, listed, listed_nothing,
    projects_of, ready_to_open, recorded, workspace_messages_at, workspace_of,
};

#[tokio::test]
async fn an_open_carries_its_brief_and_its_author_and_answers_with_both_records() {
    let kestrel = Kestrel::boot().await;
    ready_to_open(&kestrel).await;
    let workspaces = operator::WORKSPACES.replace("{organization}", "acme");

    let (status, opened) = declared(
        &kestrel,
        &workspaces,
        &json!({
            "project": "kestrel",
            "agent": "builder",
            "model": "claude-opus-5",
            "brief": "Fix the flaky test",
            "participant": "alice",
        }),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED, "{opened}");
    assert_eq!(opened["session"]["state"], "queued");
    assert_eq!(opened["session"]["model"], "claude-opus-5");
    assert_eq!(opened["workspace"]["started_by"], "alice");
    let workspace = workspace_of(&opened["workspace"]);
    let transcript = kestrel.transcript(workspace).await;
    assert_eq!(
        transcript[0].entry,
        Entry::ParticipantJoined {
            participant: "builder".to_owned(),
        }
    );
    assert_eq!(
        transcript[1].entry,
        Entry::ParticipantJoined {
            participant: "alice".to_owned(),
        }
    );
    assert_eq!(
        transcript[2].entry,
        Entry::Brief {
            source: BriefSource::Operator {
                participant: Some("alice".to_owned()),
            },
            brief: "Fix the flaky test".to_owned(),
        }
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn an_open_without_a_brief_queues_a_session_a_message_becomes_its_brief() {
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
    assert_eq!(opened["session"]["state"], "queued");
    assert!(opened["workspace"]["started_by"].is_null(), "{opened}");
    let workspace = workspace_of(&opened["workspace"]);
    let transcript = kestrel.transcript(workspace).await;
    assert_eq!(transcript.len(), 1);
    assert_eq!(
        transcript[0].entry,
        Entry::ParticipantJoined {
            participant: "builder".to_owned(),
        }
    );

    let (status, posted) = declared(
        &kestrel,
        &workspace_messages_at("acme", &workspace.to_string()),
        &json!({ "participant": "alice", "message": "what I want" }),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(posted["session"]["id"], opened["session"]["id"], "{posted}");
    assert!(posted["held_message"].is_null(), "{posted}");
    let transcript = kestrel.transcript(workspace).await;
    assert_eq!(
        transcript[1].entry,
        Entry::ParticipantJoined {
            participant: "alice".to_owned(),
        }
    );
    assert_eq!(
        transcript[2].entry,
        Entry::Brief {
            source: BriefSource::Operator {
                participant: Some("alice".to_owned()),
            },
            brief: "what I want".to_owned(),
        }
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_brief_without_a_participant_records_the_operator_and_joins_no_one() {
    let kestrel = Kestrel::boot().await;
    ready_to_open(&kestrel).await;
    let workspaces = operator::WORKSPACES.replace("{organization}", "acme");

    let (status, opened) = declared(
        &kestrel,
        &workspaces,
        &json!({
            "project": "kestrel",
            "agent": "builder",
            "brief": "Do the work",
        }),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED, "{opened}");
    assert!(opened["workspace"]["started_by"].is_null(), "{opened}");
    let transcript = kestrel.transcript(workspace_of(&opened["workspace"])).await;
    assert_eq!(
        transcript
            .iter()
            .map(|entry| &entry.entry)
            .collect::<Vec<_>>(),
        [
            &Entry::ParticipantJoined {
                participant: "builder".to_owned(),
            },
            &Entry::Brief {
                source: BriefSource::Operator { participant: None },
                brief: "Do the work".to_owned(),
            },
        ]
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_refused_open_names_its_field_and_leaves_the_organization_empty() {
    let kestrel = Kestrel::boot().await;
    ready_to_open(&kestrel).await;
    let workspaces = operator::WORKSPACES.replace("{organization}", "acme");

    for (body, status, field) in [
        (
            json!({ "project": "nowhere", "agent": "builder" }),
            StatusCode::NOT_FOUND,
            "project",
        ),
        (
            json!({ "project": "kestrel", "agent": "nowhere" }),
            StatusCode::NOT_FOUND,
            "agent",
        ),
        (
            json!({ "project": "kestrel", "agent": "builder", "profile": "nowhere" }),
            StatusCode::NOT_FOUND,
            "profile",
        ),
        (
            json!({ "project": "kestrel", "agent": "builder", "continues": "nowhere" }),
            StatusCode::NOT_FOUND,
            "continues",
        ),
        (
            json!({ "project": "kestrel", "agent": "builder", "branch": "  " }),
            StatusCode::UNPROCESSABLE_ENTITY,
            "branch",
        ),
        (
            json!({ "project": "kestrel", "agent": "builder", "model": "" }),
            StatusCode::UNPROCESSABLE_ENTITY,
            "model",
        ),
        (
            json!({ "project": "kestrel", "agent": "builder", "brief": "  " }),
            StatusCode::UNPROCESSABLE_ENTITY,
            "brief",
        ),
        (
            json!({ "project": "kestrel", "agent": "builder", "participant": "alice" }),
            StatusCode::UNPROCESSABLE_ENTITY,
            "participant",
        ),
    ] {
        let (answered, refusal) = declared(&kestrel, &workspaces, &body).await;

        assert_eq!(answered, status, "{body}: {refusal}");
        assert_eq!(refusal["field"], field, "{body}: {refusal}");
        assert!(
            listed_nothing(&kestrel, &workspaces).await,
            "{body} left a workspace behind"
        );
        assert_eq!(listed(&kestrel, &projects_of("acme")).await.len(), 1);
        assert_eq!(listed(&kestrel, &agents_of("acme")).await.len(), 2);
    }

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_brief_written_under_an_agents_name_is_refused_on_participant() {
    let kestrel = Kestrel::boot().await;
    ready_to_open(&kestrel).await;
    let workspaces = operator::WORKSPACES.replace("{organization}", "acme");

    let (status, refusal) = declared(
        &kestrel,
        &workspaces,
        &json!({
            "project": "kestrel",
            "agent": "builder",
            "brief": "Fix the flaky test",
            "participant": "reviewer",
        }),
    )
    .await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{refusal}");
    assert_eq!(refusal["field"], "participant", "{refusal}");
    assert!(listed_nothing(&kestrel, &workspaces).await);

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_continuation_that_is_open_or_names_a_branch_is_refused_and_leaves_nothing_behind() {
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
    let open = workspace_of(&opened["workspace"]);
    let (status, refusal) = declared(
        &kestrel,
        &workspaces,
        &json!({
            "project": "kestrel",
            "agent": "builder",
            "continues": open.to_string(),
        }),
    )
    .await;

    assert_eq!(status, StatusCode::CONFLICT, "{refusal}");
    assert_eq!(refusal["field"], "continues", "{refusal}");

    let opened = kestrel
        .claim_session()
        .await
        .expect("the open's session should claim");
    assert_eq!(opened.workspace, open);
    kestrel.complete_session(&opened).await;
    let sealed = {
        let workspace = kestrel.open_workspace("acme", "kestrel", "builder").await;
        let session = kestrel.dispatch_session(workspace.id).await;
        kestrel.complete_session(&session).await;
        kestrel.seal_workspace(workspace.id).await
    };
    let held = listed(&kestrel, &workspaces).await.len();
    let (status, refusal) = declared(
        &kestrel,
        &workspaces,
        &json!({
            "project": "kestrel",
            "agent": "builder",
            "continues": sealed.id.to_string(),
            "branch": "topic",
        }),
    )
    .await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{refusal}");
    assert_eq!(refusal["field"], "branch", "{refusal}");
    assert_eq!(listed(&kestrel, &workspaces).await.len(), held);

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_start_and_an_operator_open_reach_the_same_transcript() {
    let kestrel = Kestrel::boot().await;

    let (status, started) = declared(
        &kestrel,
        operator::STARTS,
        &a_start("acme", "opencode", "Fix the flaky test"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{started}");
    let started = kestrel
        .transcript(workspace_of(&started["workspace"]))
        .await;

    let workspaces = operator::WORKSPACES.replace("{organization}", "acme");
    let (status, opened) = declared(
        &kestrel,
        &workspaces,
        &json!({
            "project": "kestrel",
            "agent": "builder",
            "brief": "Fix the flaky test",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{opened}");
    let opened = kestrel.transcript(workspace_of(&opened["workspace"])).await;

    assert_eq!(
        started.iter().map(|entry| &entry.entry).collect::<Vec<_>>(),
        opened.iter().map(|entry| &entry.entry).collect::<Vec<_>>(),
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_client_opens_a_workspace_with_a_brief_a_model_and_a_participant() {
    let kestrel = Kestrel::boot().await;
    ready_to_open(&kestrel).await;

    let opened = recorded(
        &client::ran_by(
            &kestrel,
            &[
                "workspace",
                "open",
                "--organization",
                "acme",
                "--project",
                "kestrel",
                "--agent",
                "builder",
                "--brief",
                "@notes.md",
                "--model",
                "claude-opus-5",
                "--as-participant",
                "alice",
                "--json",
            ],
            client::Invocation::default().file("notes.md", "Fix the flaky test"),
        )
        .await,
    );
    let workspace = workspace_of(&json!({ "id": opened[0]["workspace"]["id"] }));
    let shown = recorded(
        &client(
            &kestrel,
            &["workspace", "show", &workspace.to_string(), "--json"],
        )
        .await,
    );
    assert_eq!(shown[0]["started_by"], "alice");
    let transcript = kestrel.transcript(workspace).await;
    assert_eq!(
        transcript[2].entry,
        Entry::Brief {
            source: BriefSource::Operator {
                participant: Some("alice".to_owned()),
            },
            brief: "Fix the flaky test".to_owned(),
        }
    );

    let piped = recorded(
        &client_given(
            &kestrel,
            &[
                "workspace",
                "open",
                "--organization",
                "acme",
                "--project",
                "kestrel",
                "--agent",
                "builder",
                "--brief",
                "-",
                "--json",
            ],
            Some("From standard input"),
        )
        .await,
    );
    let piped = workspace_of(&json!({ "id": piped[0]["workspace"]["id"] }));
    assert_eq!(
        kestrel.transcript(piped).await[1].entry,
        Entry::Brief {
            source: BriefSource::Operator { participant: None },
            brief: "From standard input".to_owned(),
        }
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_later_session_enqueue_refuses_a_workspace_that_never_had_a_session() {
    let kestrel = Kestrel::boot().await;
    Fixture::acme().declare(&kestrel).await;
    let workspace = kestrel.open_workspace("acme", "kestrel", "builder").await;

    let refused = client(
        &kestrel,
        &[
            "session",
            "enqueue",
            "--workspace",
            &workspace.id.to_string(),
        ],
    )
    .await;

    assert!(
        failed(&refused).contains("has never had a session"),
        "{}",
        failed(&refused)
    );
    assert_eq!(refused.status.code(), Some(4));
    assert!(kestrel.sessions(workspace.id).await.is_empty());

    kestrel.teardown().await;
}
