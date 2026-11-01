//! The operator boundary itself: its root, its separation from the link, its lack of credentials,
//! and the published document that describes it.

use crate::support;

use kestrel::domain::{Exit, SessionId};
use kestrel::link;
use kestrel::log::{BriefSource, Entry, Message, ToolStatus};
use kestrel::operator;
use reqwest::StatusCode;
use support::Kestrel;
use support::operator_requests::{an_open_workspace, transcript_of};

/// Someone who opens the operator port in a browser learns it is the API, and where the Client is.
#[tokio::test]
async fn the_operator_root_names_the_api_and_the_browser_client() {
    let kestrel = Kestrel::boot().await;

    let answered = reqwest::get(format!("{}/", kestrel.operator()))
        .await
        .expect("the operator boundary should answer");
    assert_eq!(answered.status(), StatusCode::OK);
    assert_eq!(
        answered.headers()[reqwest::header::CONTENT_TYPE],
        "text/plain; charset=utf-8"
    );
    let said = answered.text().await.expect("an answer");
    assert!(said.contains("kestrel operator API"), "{said}");
    assert!(said.contains("http://localhost:7719"), "{said}");

    let rebound = reqwest::Client::new()
        .get(format!("{}/", kestrel.operator()))
        .header(reqwest::header::HOST, "attacker.example")
        .send()
        .await
        .expect("the operator boundary should answer");
    assert_eq!(rebound.status(), StatusCode::FORBIDDEN);

    kestrel.teardown().await;
}

#[tokio::test]
async fn the_operator_boundary_and_the_link_are_served_apart() {
    let kestrel = Kestrel::boot().await;
    let (workspace, session) = an_open_workspace(&kestrel, 0).await;
    let client = reqwest::Client::new();

    let link_on_the_operator_listener = client
        .get(format!(
            "{}{}",
            kestrel.operator(),
            link::ENTRIES.replace("{session}", &session.id.to_string())
        ))
        .send()
        .await
        .expect("the operator listener should answer");
    let operator_on_the_link_listener = client
        .get(format!(
            "{}{}?follow=false",
            kestrel.link(),
            transcript_of("acme", &workspace)
        ))
        .send()
        .await
        .expect("the link listener should answer");

    assert_ne!(kestrel.operator(), kestrel.link());
    assert_eq!(
        link_on_the_operator_listener.status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        operator_on_the_link_listener.status(),
        StatusCode::NOT_FOUND
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn the_operator_boundary_asks_for_no_credential() {
    let kestrel = Kestrel::boot().await;
    let (workspace, _) = an_open_workspace(&kestrel, 0).await;

    let response = reqwest::Client::new()
        .get(format!(
            "{}{}?follow=false",
            kestrel.operator(),
            transcript_of("acme", &workspace)
        ))
        .send()
        .await
        .expect("the operator boundary should answer");

    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        response
            .text()
            .await
            .expect("the stream should end")
            .contains("event: end")
    );

    kestrel.teardown().await;
}

#[test]
fn the_published_operator_document_describes_the_boundary_the_control_plane_serves() {
    let document = support::published_document("operator.json");

    assert_eq!(document["openapi"], "3.1.0");
    assert_eq!(document["security"], serde_json::json!([]));

    let described: Vec<(String, String)> = document["paths"]
        .as_object()
        .expect("an object of paths")
        .iter()
        .flat_map(|(path, operations)| {
            operations
                .as_object()
                .expect("an object of operations")
                .keys()
                .map(|method| (path.clone(), method.clone()))
                .collect::<Vec<_>>()
        })
        .collect();

    let served = [
        (operator::ROOT, "get"),
        (operator::HARNESSES, "get"),
        (operator::SIGN_IN_METHOD, "get"),
        (operator::OPERATOR, "get"),
        (operator::OPERATOR, "put"),
        (operator::INSTALL, "get"),
        (operator::READINESS, "get"),
        (operator::ORGANIZATIONS, "get"),
        (operator::ORGANIZATIONS, "post"),
        (operator::STARTS, "post"),
        (operator::START_PREVIEW, "post"),
        (operator::REPOSITORY_RESOLUTIONS, "post"),
        (operator::PROJECTS, "get"),
        (operator::PROJECTS, "post"),
        (operator::AGENTS, "get"),
        (operator::AGENTS, "post"),
        (operator::AGENT_MODEL, "put"),
        (operator::DECLARATION, "post"),
        (operator::DECLARATION_PREVIEW, "post"),
        (operator::CREDENTIALS, "get"),
        (operator::CREDENTIAL, "put"),
        (operator::CREDENTIAL, "delete"),
        (operator::PROFILES, "get"),
        (operator::PROFILES, "post"),
        (operator::PROFILE_VARIABLE, "put"),
        (operator::PROFILE_VARIABLE, "delete"),
        (operator::PROFILE_FILE, "put"),
        (operator::PROFILE_FILE, "delete"),
        (operator::SIGN_INS, "get"),
        (operator::SIGN_IN, "put"),
        (operator::SIGN_IN_TESTS, "post"),
        (operator::SIGN_IN_TEST, "get"),
        (operator::SIGN_IN_RELAYS, "post"),
        (operator::SIGN_IN_RELAY, "get"),
        (operator::SIGN_IN_RELAY, "delete"),
        (operator::SIGN_IN_RELAY_CODE, "post"),
        (operator::SIGN_IN_RELAY_EVENTS, "get"),
        (operator::INTEGRATIONS, "get"),
        (operator::INTEGRATIONS, "post"),
        (operator::GITHUB_APP, "post"),
        (kestrel::integration::manifest::PAGE, "get"),
        (kestrel::integration::manifest::PAGE, "post"),
        (kestrel::integration::manifest::CALLBACK, "get"),
        (kestrel::integration::manifest::INSTALLED, "get"),
        (operator::INTEGRATION, "get"),
        (operator::INTEGRATION, "patch"),
        (operator::INTEGRATION_DISABLE, "post"),
        (operator::INTEGRATION_ENABLE, "post"),
        (operator::INTEGRATION_RETIRE, "post"),
        (operator::INTEGRATION_PRIVATE_KEY, "put"),
        (operator::INTEGRATION_REPOSITORIES, "get"),
        (operator::INTEGRATION_WEBHOOK, "get"),
        (operator::INTEGRATION_WEBHOOK, "put"),
        (operator::INTEGRATION_WEBHOOK_SECRET_ROTATION, "post"),
        (operator::INTEGRATION_WEBHOOK_SECRET_ROTATION_RESUME, "post"),
        (operator::EVENT_REFUSAL, "delete"),
        (operator::EVENTS, "get"),
        (operator::EVENT, "get"),
        (operator::WORKSPACES, "get"),
        (operator::WORKSPACES, "post"),
        (operator::WORKSPACE, "get"),
        (operator::WORKSPACE_WORK, "get"),
        (operator::WORKSPACE_CHANGES, "get"),
        (operator::WORKSPACE_COMMITS, "get"),
        (operator::WORKSPACE_STASHES, "get"),
        (operator::WORKSPACE_FILES, "get"),
        (operator::WORKSPACE_FILE, "get"),
        (operator::WORKSPACE_MESSAGES, "post"),
        (operator::WORKSPACE_MESSAGE, "put"),
        (operator::WORKSPACE_MESSAGE, "delete"),
        (operator::WORKSPACE_SEAL, "post"),
        (operator::WORKSPACE_INSTANCE_RELEASE, "post"),
        (operator::SESSIONS, "get"),
        (operator::SESSIONS, "post"),
        (operator::SESSION, "get"),
        (operator::SESSION_STOP, "post"),
        (operator::SESSION_INTERRUPT, "post"),
        (operator::SESSION_OPTIONS, "post"),
        (operator::TRIGGERS, "get"),
        (operator::TRIGGERS, "post"),
        (operator::TRIGGER, "get"),
        (operator::TRIGGER_TEST, "post"),
        (operator::TRIGGER_DISABLE, "post"),
        (operator::TRIGGER_ENABLE, "post"),
        (operator::TRIGGER_DISPATCH, "post"),
        (operator::APPLIED_TRIGGERS, "post"),
        (operator::APPLIED_TRIGGERS_PREVIEW, "post"),
        (operator::INSTANCES, "get"),
        (operator::QUEUE, "get"),
        (operator::TRANSCRIPT, "get"),
        (operator::FOLLOWER_LEASE, "post"),
        (operator::TRANSCRIPT_PAYLOAD, "get"),
        (operator::CHANGES, "get"),
        (operator::STREAMS, "put"),
        (operator::STREAM, "get"),
        (operator::STREAM_SUBSCRIPTION, "put"),
        (operator::STREAM_SUBSCRIPTION, "delete"),
        (kestrel::integration::manifest::FLOW, "get"),
        (kestrel::integration::manifest::FLOW, "delete"),
        (kestrel::integration::manifest::INSTALLATIONS, "get"),
        (kestrel::integration::manifest::REPOSITORIES, "get"),
        (kestrel::integration::manifest::FINISH, "post"),
    ];
    assert_eq!(
        described,
        served
            .iter()
            .map(|(path, method)| ((*path).to_owned(), (*method).to_owned()))
            .collect::<Vec<_>>()
    );
}

#[test]
fn the_published_operator_document_describes_every_transcript_entry() {
    let document = support::published_document("operator.json");
    let mapping = document["components"]["schemas"]["Entry"]["discriminator"]["mapping"]
        .as_object()
        .expect("an object of entry kinds");

    let served = [
        Entry::Expired {
            expired_at: "2026-09-30T00:00:00Z".parse().unwrap(),
        },
        Entry::ParticipantJoined {
            participant: "builder".to_owned(),
        },
        Entry::Brief {
            source: BriefSource::Trigger {
                trigger: "sweep".to_owned(),
            },
            brief: "Sweep the backlog".to_owned(),
        },
        Entry::SessionStarted {
            session: SessionId::generate(),
            agent: "builder".to_owned(),
        },
        Entry::Said {
            participant: "builder".to_owned(),
            message: "what the agent said".to_owned(),
            session_id: None,
            completion: None,
        },
        Entry::Messages {
            messages: vec![Message {
                participant: "operator".to_owned(),
                message: "what arrived while it worked".to_owned(),
            }],
        },
        Entry::SessionEnded {
            session: SessionId::generate(),
            exit: Exit::Succeeded,
        },
        Entry::TurnInterrupted {
            session: SessionId::generate(),
            participant: "operator".to_owned(),
        },
        Entry::InstanceReleased {
            participant: "operator".to_owned(),
            instance: "docker/kestrel-01999cf2".to_owned(),
            unpublished: Some("https://github.com/acme/widgets has 1 untracked file".to_owned()),
        },
        Entry::Thought {
            session_id: SessionId::generate(),
            text: "thinking".to_owned(),
            completion: kestrel::log::Completion::at("2026-09-29T12:00:00Z".parse().unwrap()),
        },
        Entry::Plan {
            session_id: SessionId::generate(),
            entries: vec![kestrel::log::PlanEntry {
                content: "read the backlog".to_owned(),
                priority: "medium".to_owned(),
                status: "pending".to_owned(),
            }],
            completion: kestrel::log::Completion::at("2026-09-29T12:00:00Z".parse().unwrap()),
        },
        Entry::PullRequest {
            event: kestrel::domain::EventRecordId::generate(),
            repository: "https://github.com/jtmthf/kestrel".to_owned(),
            number: 7,
            url: "https://github.com/jtmthf/kestrel/pull/7".to_owned(),
            title: "Learn pull requests".to_owned(),
            action: "opened".to_owned(),
            state: kestrel::domain::PullRequestState::Open,
        },
        Entry::OptionChanged {
            session: SessionId::generate(),
            participant: "operator".to_owned(),
            option: "model".to_owned(),
            category: "model".to_owned(),
            from: Some("scripted-mini".to_owned()),
            to: Some("scripted-max".to_owned()),
            refused: None,
        },
        Entry::ToolCall {
            session_id: SessionId::generate(),
            call_id: "call".to_owned(),
            title: "read".to_owned(),
            tool_kind: "read".to_owned(),
            status: ToolStatus::Completed,
            input: serde_json::json!({}),
            result: Box::new(serde_json::json!([])),
            closing_reason: None,
            completion: kestrel::log::Completion::at("2026-09-29T12:00:00Z".parse().unwrap()),
        },
    ];

    let mut kinds: Vec<String> = Vec::new();
    for entry in served {
        let entry = serde_json::to_value(&entry).expect("an entry");
        let kind = entry["type"].as_str().expect("a kind").to_owned();
        assert!(
            mapping.contains_key(&kind),
            "the document describes no {kind} entry"
        );
        let typed: kestrel_operator_types::Entry = serde_json::from_value(entry.clone())
            .unwrap_or_else(|error| panic!("a {kind} entry is not the documented one: {error}"));
        assert_eq!(
            serde_json::to_value(&typed).expect("an entry"),
            entry,
            "the generated {kind} entry drops what the boundary serves"
        );
        kinds.push(kind);
    }

    assert_eq!(kinds, mapping.keys().cloned().collect::<Vec<_>>());
}
