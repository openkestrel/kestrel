//! The link an Environment dials out to, driven through the primary test seam: a local-exec
//! Environment running the real supervisor binary, and a client of kestrel's own in the
//! test's hands for the answers a supervisor only ever reacts to.

mod support;

use std::fs;
use std::time::Duration;

use kestrel::domain::{Exit, Session, SessionId};
use kestrel::link::{self, Instruction};
use kestrel::log::{Entry, Message};
use kestrel::work::{Report, Reported};
use reqwest::{StatusCode, Version, header};
use serde_json::json;
use support::link_client::{Link, Next};
use support::supervisor::Supervisor;
use support::{Kestrel, OnTheLink};

const PATIENCE: Duration = Duration::from_secs(30);
const LONG_ENOUGH_TO_BE_SURE: Duration = Duration::from_millis(500);

async fn a_session(kestrel: &Kestrel) -> (Session, OnTheLink) {
    declared(kestrel).await;

    another_session(kestrel).await
}

async fn declared(kestrel: &Kestrel) {
    let organization = kestrel.declare_organization("acme").await;
    kestrel
        .declare_project(
            &organization,
            "kestrel",
            &["https://github.com/jtmthf/kestrel".to_owned()],
            "main",
        )
        .await;
    kestrel
        .declare_agent(&organization, "builder", "opencode", Some("claude-opus-5"))
        .await;
}

/// A second Workspace, because at 0.1 nothing yet stops two Sessions being live in one.
async fn another_session(kestrel: &Kestrel) -> (Session, OnTheLink) {
    let workspace = kestrel.open_workspace("acme", "kestrel", "builder").await;

    kestrel.dispatch_to_the_link(workspace.id).await
}

#[tokio::test]
async fn an_environment_dials_out_and_the_control_plane_knows_it_is_connected() {
    let kestrel = Kestrel::boot().await;
    let (session, on) = a_session(&kestrel).await;

    assert!(kestrel.session(session.id).await.connected.is_none());
    let mut supervisor = Supervisor::provision(&kestrel.link(), &on);
    supervisor.wait_until_it_says("reported connected").await;

    let connected = kestrel
        .session(session.id)
        .await
        .connected
        .expect("the control plane should know an environment is on the link");
    assert!(
        !connected.version.is_empty(),
        "the control plane learned that something connected, but not what"
    );

    supervisor.destroy();
    kestrel.teardown().await;
}

#[tokio::test]
async fn a_supervisor_told_to_stop_its_session_stays_on_the_link() {
    let kestrel = Kestrel::boot().await;
    let (session, on) = a_session(&kestrel).await;

    let mut supervisor = Supervisor::provision(&kestrel.link(), &on);
    supervisor.wait_until_it_says("link open").await;
    assert!(
        supervisor.is_still_running(LONG_ENOUGH_TO_BE_SURE).await,
        "the supervisor let go of the stream on its own. it said:\n{}",
        supervisor.everything_it_said()
    );

    kestrel.instruct(&session, Instruction::Stop).await;

    supervisor.wait_until_it_says("instruction stop").await;
    assert!(
        supervisor.is_still_running(LONG_ENOUGH_TO_BE_SURE).await,
        "the supervisor left the link with its session. it said:\n{}",
        supervisor.everything_it_said()
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_supervisor_that_loses_the_stream_comes_back_and_is_handed_what_it_missed() {
    let kestrel = Kestrel::boot().await;
    let (session, on) = a_session(&kestrel).await;

    let mut supervisor = Supervisor::provision(&kestrel.link(), &on);
    supervisor.wait_until_it_says("reported connected").await;

    let stopped = kestrel.kill().await;
    supervisor.wait_until_it_says("lost the link").await;
    stopped.instruct(&session, Instruction::Stop).await;
    let kestrel = stopped.restart().await;

    supervisor.wait_until_it_says("instruction stop").await;

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_reconnect_carrying_a_cursor_is_not_handed_what_it_already_had() {
    let kestrel = Kestrel::boot().await;
    let (session, on) = a_session(&kestrel).await;
    kestrel.instruct(&session, Instruction::Stop).await;

    let link = Link::to(&kestrel.link());
    let mut first = link.open(&on.instance, &on.credential, None).await;
    let Next::Event(delivered) = first.next_within(PATIENCE).await else {
        panic!("the stream never delivered the instruction that was waiting on it");
    };
    assert_eq!(delivered.id.as_deref(), Some("1"));
    assert_eq!(delivered.name.as_deref(), Some("stop"));

    let mut again = link.open(&on.instance, &on.credential, Some(1)).await;

    assert!(
        matches!(again.next_within(LONG_ENOUGH_TO_BE_SURE).await, Next::Quiet),
        "reconnecting with a cursor was handed an instruction it had already been given"
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn the_link_refuses_an_environment_presenting_no_credential() {
    let kestrel = Kestrel::boot().await;
    let (_, on) = a_session(&kestrel).await;
    let link = Link::to(&kestrel.link());

    assert_eq!(
        link.instructions(&on.instance, None, None).await.status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        link.report(
            &on.instance,
            None,
            &Reported {
                session: None,
                seq: None,
                report: Report::Connected {
                    version: "0.0.0".to_owned()
                }
            }
        )
        .await
        .status(),
        StatusCode::UNAUTHORIZED
    );

    kestrel.teardown().await;
}

/// A supervisor is on the link as its Instance; no route takes a Session's word for itself.
#[tokio::test]
async fn the_link_has_no_route_for_a_session() {
    let kestrel = Kestrel::boot().await;
    let (session, on) = a_session(&kestrel).await;

    let refused = reqwest::Client::new()
        .get(format!(
            "{}/link/sessions/{}/instructions",
            kestrel.link(),
            session.id
        ))
        .bearer_auth(on.credential.as_str())
        .send()
        .await
        .expect("the link should answer");

    assert_eq!(refused.status(), StatusCode::NOT_FOUND);

    kestrel.teardown().await;
}

#[tokio::test]
async fn the_link_refuses_a_credential_belonging_to_another_instance() {
    let kestrel = Kestrel::boot().await;
    let (_, on) = a_session(&kestrel).await;
    let (_, another) = another_session(&kestrel).await;

    let refused = Link::to(&kestrel.link())
        .instructions(&on.instance, Some(&another.credential), None)
        .await;

    assert_eq!(refused.status(), StatusCode::FORBIDDEN);

    kestrel.teardown().await;
}

#[tokio::test]
async fn an_instance_credential_outlives_its_session_and_stops_working_when_the_instance_is_let_go()
{
    let kestrel = Kestrel::boot().await;
    let (session, on) = a_session(&kestrel).await;
    let link = Link::to(&kestrel.link());

    kestrel.complete_session(&session).await;
    assert_eq!(
        link.instructions(&on.instance, Some(&on.credential), None)
            .await
            .status(),
        StatusCode::OK
    );

    kestrel.release_instance(session.workspace).await;
    assert_eq!(
        link.instructions(&on.instance, Some(&on.credential), None)
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_report_about_a_session_that_has_ended_is_gone_and_the_instance_stays_on_the_link() {
    let kestrel = Kestrel::boot().await;
    let (session, on) = a_session(&kestrel).await;
    let link = Link::to(&kestrel.link());
    kestrel.complete_session(&session).await;

    let gone = link
        .report(
            &on.instance,
            Some(&on.credential),
            &Reported {
                session: Some(session.id),
                seq: Some(1),
                report: Report::Started,
            },
        )
        .await;
    assert_eq!(gone.status(), StatusCode::GONE);
    let alive = link
        .report(
            &on.instance,
            Some(&on.credential),
            &Reported {
                session: None,
                seq: None,
                report: Report::Heartbeat,
            },
        )
        .await;
    assert_eq!(alive.status(), StatusCode::ACCEPTED);

    kestrel.teardown().await;
}

#[tokio::test]
async fn the_link_is_plain_http_with_no_protocol_upgrade() {
    let kestrel = Kestrel::boot().await;
    let (_, on) = a_session(&kestrel).await;

    let stream = Link::to(&kestrel.link())
        .instructions(&on.instance, Some(&on.credential), None)
        .await;

    assert_eq!(stream.status(), StatusCode::OK);
    assert_eq!(stream.version(), Version::HTTP_11);
    assert_eq!(
        stream
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|kind| kind.to_str().ok()),
        Some("text/event-stream")
    );
    assert!(stream.headers().get(header::UPGRADE).is_none());
    assert!(
        !stream
            .headers()
            .get(header::CONNECTION)
            .and_then(|connection| connection.to_str().ok())
            .is_some_and(|connection| connection.to_lowercase().contains("upgrade"))
    );

    kestrel.teardown().await;
}

#[test]
fn the_published_openapi_document_describes_the_link_the_control_plane_serves() {
    let document = published();

    assert_eq!(document["openapi"], "3.1.0");

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

    assert_eq!(
        described,
        vec![
            (link::ANSWERS.to_owned(), "post".to_owned()),
            (link::CREDENTIALS.to_owned(), "get".to_owned()),
            (link::CREDENTIALS.to_owned(), "patch".to_owned()),
            (link::ENTRIES.to_owned(), "get".to_owned()),
            (link::INSTRUCTIONS.to_owned(), "get".to_owned()),
            (link::REPORTS.to_owned(), "post".to_owned()),
        ]
    );
}

/// A report body written as the document describes it, so a field the control plane renamed
/// under the specification fails here rather than as a 422 an Environment cannot act on.
#[tokio::test]
async fn the_link_takes_every_report_the_published_openapi_document_describes() {
    let described = reports_the_document_describes();
    let bodies = json!({
        "connected": {"kind": "connected", "version": "0.0.0"},
        "heartbeat": {"kind": "heartbeat"},
        "work": {"kind": "work", "repositories": []},
        "stderr": {"kind": "stderr", "lines": ["level=INFO message=init"]},
        "started": {"kind": "started", "seq": 1},
        "model": {
            "kind": "model",
            "seq": 1,
            "model": "scripted-mini",
        },
        "said": {"kind": "said", "completion": {"started_at": "2026-09-29T12:00:00Z", "finished_at": "2026-09-29T12:00:00Z", "turn_outcome": null}, "seq": 1, "message": "what the agent said"},
        "used": {
            "kind": "used",
            "seq": 1,
            "usage": {
                "context_used": 1_200,
                "context_size": 200_000,
                "cost": {"amount": 0.42, "currency": "USD"},
            },
        },
        "answered": {"kind": "answered", "seq": 1},
        "checkout": {
            "kind": "checkout",
            "seq": 1,
            "repositories": [
                {
                    "repository": "https://github.com/acme/widgets",
                    "git": "read",
                    "branch": "kestrel/work",
                    "untracked": 1,
                    "uncommitted": 0,
                    "stashes": 0,
                    "unpushed": 2,
                },
                {
                    "repository": "https://github.com/acme/gadgets",
                    "git": "unreadable",
                    "because": "there is no checkout at gadgets",
                },
            ],
        },
        "finished": {"kind": "finished", "seq": 1, "exit": {"status": "succeeded"}},        "thought": {"kind": "thought", "seq": 1, "text": "thinking", "completion": {"started_at": "2026-09-29T12:00:00Z", "finished_at": "2026-09-29T12:00:00Z", "turn_outcome": null}},
        "plan": {"kind": "plan", "seq": 1, "entries": [], "completion": {"started_at": "2026-09-29T12:00:00Z", "finished_at": "2026-09-29T12:00:00Z", "turn_outcome": null}},
        "tool_call": {"kind":"tool_call", "seq":1, "call_id":"call", "title":"read", "tool_kind":"read", "status":"completed", "input":{}, "result":[], "closing_reason":null, "completion":{"started_at":"2026-09-30T12:00:00Z", "finished_at":"2026-09-30T12:00:00Z", "turn_outcome":null}},
        "session_state": {"kind":"session_state", "tools":[], "message_buffering":false, "thought_buffering":false},


    });
    assert_eq!(
        described,
        bodies
            .as_object()
            .expect("an object of bodies")
            .keys()
            .cloned()
            .collect::<Vec<_>>()
    );

    let kestrel = Kestrel::boot().await;
    let link = Link::to(&kestrel.link());
    declared(&kestrel).await;

    for kind in described {
        let (session, on) = another_session(&kestrel).await;
        let mut body = bodies[&kind].clone();
        body["session"] = json!(session.id);
        assert_eq!(
            link.report_body(&on.instance, Some(&on.credential), &body)
                .await
                .status(),
            StatusCode::ACCEPTED,
            "the link would not take a {kind} report as the document describes it"
        );
    }

    kestrel.teardown().await;
}

fn published() -> serde_json::Value {
    let document = support::crate_root().join("../../openapi/link.json");

    serde_json::from_str(&fs::read_to_string(document).expect("a readable openapi document"))
        .expect("valid json")
}

fn reports_the_document_describes() -> Vec<String> {
    published()["components"]["schemas"]["Report"]["discriminator"]["mapping"]
        .as_object()
        .expect("an object of report kinds")
        .keys()
        .cloned()
        .collect()
}

/// As a supervisor seeding a cold Environment walks it.
async fn paged(link: &Link, on: &OnTheLink, window: usize) -> Vec<i64> {
    let mut walked = Vec::new();
    let mut cursor: Option<String> = None;

    loop {
        let response = link
            .entries(
                &on.instance,
                Some(&on.credential),
                cursor.as_deref(),
                Some(window),
            )
            .await;
        assert_eq!(response.status(), StatusCode::OK);

        let page: serde_json::Value = response.json().await.expect("a page of the transcript");
        let entries = page["entries"].as_array().expect("an array of entries");
        assert!(entries.len() <= window, "a read overran its window: {page}");

        walked.extend(
            entries
                .iter()
                .map(|entry| entry["seq"].as_i64().expect("a seq")),
        );
        cursor = page["cursor"].as_str().map(str::to_owned);

        if !page["more"].as_bool().expect("whether more are waiting") {
            return walked;
        }
    }
}

#[tokio::test]
async fn an_environment_reads_the_transcript_of_the_workspace_its_session_belongs_to_in_windows() {
    let kestrel = Kestrel::boot().await;
    let (session, on) = a_session(&kestrel).await;
    for message in 1..=4 {
        kestrel.said(&session, &format!("message {message}")).await;
    }
    let link = Link::to(&kestrel.link());

    let first: serde_json::Value = link
        .entries(&on.instance, Some(&on.credential), None, Some(2))
        .await
        .json()
        .await
        .expect("a page of the transcript");

    assert_eq!(first["entries"].as_array().expect("entries").len(), 2);
    assert_eq!(first["entries"][0]["entry"]["type"], "participant_joined");
    assert_eq!(first["more"], true);
    assert!(first["cursor"].is_string());
    assert_eq!(paged(&link, &on, 2).await, (1..=6).collect::<Vec<_>>());

    kestrel.teardown().await;
}

#[tokio::test]
async fn the_link_refuses_a_cursor_that_names_no_position_in_the_transcript() {
    let kestrel = Kestrel::boot().await;
    let (session, on) = a_session(&kestrel).await;
    let link = Link::to(&kestrel.link());

    for cursor in ["halfway-through", &format!("{}:99", session.workspace)] {
        assert_eq!(
            link.entries(&on.instance, Some(&on.credential), Some(cursor), None)
                .await
                .status(),
            StatusCode::BAD_REQUEST,
            "the link took the cursor {cursor} and started the walk over"
        );
    }

    kestrel.teardown().await;
}

#[tokio::test]
async fn the_link_refuses_a_window_wider_than_one_read_may_return() {
    let kestrel = Kestrel::boot().await;
    let (_, on) = a_session(&kestrel).await;
    let link = Link::to(&kestrel.link());

    assert_eq!(
        link.entries(&on.instance, Some(&on.credential), None, Some(5_000))
            .await
            .status(),
        StatusCode::BAD_REQUEST
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn the_link_refuses_a_transcript_read_from_an_environment_presenting_no_credential() {
    let kestrel = Kestrel::boot().await;
    let (_, on) = a_session(&kestrel).await;
    let link = Link::to(&kestrel.link());

    assert_eq!(
        link.entries(&on.instance, None, None, None).await.status(),
        StatusCode::UNAUTHORIZED
    );

    kestrel.teardown().await;
}

/// The document is what a supervisor seeding a cold Environment reads the entries against, so
/// a kind or a field the Transcript grew and the document did not fails here.
#[test]
fn the_published_openapi_document_describes_every_transcript_entry_the_link_serves() {
    let document = published();
    let mapping = document["components"]["schemas"]["Entry"]["discriminator"]["mapping"]
        .as_object()
        .expect("an object of entry kinds");

    let served = [
        Entry::ParticipantJoined {
            participant: "builder".to_owned(),
        },
        Entry::Brief {
            trigger: Some("ready".to_owned()),
            brief: "/implement https://github.com/jtmthf/kestrel/issues/174".to_owned(),
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
            entries: Vec::new(),
            completion: kestrel::log::Completion::at("2026-09-29T12:00:00Z".parse().unwrap()),
        },
        Entry::ToolCall {
            session_id: SessionId::generate(),
            call_id: "call".to_owned(),
            title: "read".to_owned(),
            tool_kind: "read".to_owned(),
            status: "completed".to_owned(),
            input: serde_json::json!({}),
            result: serde_json::json!([]),
            closing_reason: None,
            completion: kestrel::log::Completion::at("2026-09-29T12:00:00Z".parse().unwrap()),
        },
    ];

    let mut kinds: Vec<String> = Vec::new();
    for entry in served {
        let entry = serde_json::to_value(&entry).expect("an entry");
        let kind = entry["type"].as_str().expect("a kind").to_owned();
        let schema = mapping
            .get(&kind)
            .unwrap_or_else(|| panic!("the document describes no {kind} entry"))
            .as_str()
            .expect("a reference");

        for field in resolve(&document, schema)["required"]
            .as_array()
            .expect("an array of required fields")
        {
            let field = field.as_str().expect("a named field");
            assert!(
                entry.get(field).is_some(),
                "the document requires {field} on a {kind} entry, and the link does not serve it"
            );
        }
        kinds.push(kind);
    }

    assert_eq!(kinds, mapping.keys().cloned().collect::<Vec<_>>());
}

fn resolve<'a>(document: &'a serde_json::Value, reference: &str) -> &'a serde_json::Value {
    reference
        .trim_start_matches("#/")
        .split('/')
        .fold(document, |document, step| &document[step])
}

#[tokio::test]
async fn completed_units_replay_once_and_filtered_pages_walk_the_global_cursor() {
    let kestrel = Kestrel::boot().await;
    let (session, on) = a_session(&kestrel).await;
    let link = Link::to(&kestrel.link());
    let completion = kestrel::log::Completion::at("2026-09-29T12:00:00Z".parse().unwrap());
    let thought = Reported {
        session: Some(session.id),
        seq: Some(2),
        report: Report::Thought {
            text: "reasoning".to_owned(),
            completion: completion.clone(),
        },
    };
    assert_eq!(
        link.report(&on.instance, Some(&on.credential), &thought)
            .await
            .status(),
        StatusCode::BAD_REQUEST
    );
    for report in [
        Reported {
            session: Some(session.id),
            seq: Some(1),
            report: Report::Said {
                message: "hello".to_owned(),
                completion: completion.clone(),
            },
        },
        thought,
        Reported {
            session: Some(session.id),
            seq: Some(3),
            report: Report::Plan {
                entries: Vec::new(),
                completion: completion.clone(),
            },
        },
    ] {
        for _ in 0..2 {
            assert_eq!(
                link.report(&on.instance, Some(&on.credential), &report)
                    .await
                    .status(),
                StatusCode::ACCEPTED
            );
        }
    }
    let transient = Reported {
        session: Some(session.id),
        seq: None,
        report: Report::SessionState {
            tools: vec![],
            message_buffering: true,
            thought_buffering: false,
        },
    };
    assert!(
        serde_json::to_value(&transient)
            .unwrap()
            .get("seq")
            .is_none()
    );
    assert_eq!(
        link.report(&on.instance, Some(&on.credential), &transient)
            .await
            .status(),
        StatusCode::ACCEPTED
    );
    let tool = Reported {
        session: Some(session.id),
        seq: Some(5),
        report: Report::ToolCall {
            call_id: "call".to_owned(),
            title: "read".to_owned(),
            tool_kind: "read".to_owned(),
            status: "completed".to_owned(),
            input: serde_json::json!({"path":"a"}),
            result: serde_json::json!(["read"]),
            closing_reason: None,
            completion,
        },
    };
    assert_eq!(
        link.report(&on.instance, Some(&on.credential), &tool)
            .await
            .status(),
        StatusCode::BAD_REQUEST
    );
    let tool = Reported {
        seq: Some(4),
        ..tool
    };
    for _ in 0..2 {
        assert_eq!(
            link.report(&on.instance, Some(&on.credential), &tool)
                .await
                .status(),
            StatusCode::ACCEPTED
        );
    }
    let client = reqwest::Client::new();
    for (kinds, expected) in [
        (None, vec![1, 2, 3]),
        (Some("narration"), vec![4, 5]),
        (Some("detail"), vec![6]),
        (
            Some("narration,detail,shared_state"),
            vec![1, 2, 3, 4, 5, 6],
        ),
        (Some("narration,shared_state"), vec![1, 2, 3, 4, 5]),
    ] {
        let mut cursor: Option<String> = None;
        let mut walked = Vec::new();
        for page_number in 0..7 {
            assert!(page_number < 6, "the filtered walk repeated a page");
            let mut url = reqwest::Url::parse(&format!(
                "{}/link/instances/{}/entries",
                kestrel.link(),
                on.instance.replace('/', "%2F")
            ))
            .unwrap();
            url.query_pairs_mut().append_pair("window", "1");
            if let Some(kinds) = kinds {
                url.query_pairs_mut().append_pair("kinds", kinds);
            }
            if let Some(cursor) = &cursor {
                url.query_pairs_mut().append_pair("cursor", cursor);
            }
            let response = client
                .get(url)
                .bearer_auth(on.credential.as_str())
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            let page: serde_json::Value = response.json().await.unwrap();
            walked.extend(
                page["entries"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|entry| entry["seq"].as_i64().unwrap()),
            );
            cursor = page["cursor"].as_str().map(str::to_owned);
            assert_eq!(
                cursor,
                Some(format!("{}:{}", session.workspace, page_number + 1))
            );
            if page["more"] == false {
                break;
            }
        }
        assert_eq!(walked, expected);
        assert_eq!(cursor, Some(format!("{}:6", session.workspace)));
    }
    let stream = client
        .get(format!(
            "{}/operator/organizations/acme/workspaces/{}/transcript?follow=false",
            kestrel.operator(),
            session.workspace
        ))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(stream.contains("event: cursor"));
    assert!(stream.contains(&format!("id: {}:6", session.workspace)));
    assert!(!stream.contains("reasoning"));
    kestrel.teardown().await;
}
