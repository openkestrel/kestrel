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
        "ready": {"kind": "ready"},
        "started": {"kind": "started", "seq": 1},
        "model": {
            "kind": "model",
            "seq": 1,
            "model": "scripted-mini",
        },
        "said": {"kind": "said", "completion": {"started_at": "2026-09-29T12:00:00Z", "finished_at": "2026-09-29T12:00:00Z", "turn_outcome": null}, "seq": 1, "message": "what the agent said"},
        "usage": {
            "kind": "usage",
            "usage": {
                "context_used": 1_200,
                "context_size": 200_000,
                "cost": {"amount": 0.42, "currency": "USD"},
            },
        },
        "session_info": {
            "kind": "session_info",
            "title": "the scripted conversation",
            "options": [
                {
                    "id": "model",
                    "name": "Model",
                    "description": null,
                    "category": "model",
                    "kind": "select",
                    "current": "scripted-mini",
                    "values": [
                        {"value": "scripted-mini", "name": "scripted-mini", "description": null},
                    ],
                    "groups": [],
                },
            ],
            "commands": [
                {"name": "compact", "description": "Compact the conversation", "input_hint": "/compact"},
            ],
        },
        "answered": {"kind": "answered", "seq": 1, "usage": {"context_used": 1_200, "context_size": 200_000, "cost": {"amount": 0.42, "currency": "USD"}}},
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
        "finished": {"kind": "finished", "seq": 1, "exit": {"status": "succeeded"}, "usage": {"context_used": 1_200, "context_size": 200_000, "cost": {"amount": 0.42, "currency": "USD"}}},        "thought": {"kind": "thought", "seq": 1, "text": "thinking", "completion": {"started_at": "2026-09-29T12:00:00Z", "finished_at": "2026-09-29T12:00:00Z", "turn_outcome": null}},
        "plan": {"kind": "plan", "seq": 1, "entries": [], "completion": {"started_at": "2026-09-29T12:00:00Z", "finished_at": "2026-09-29T12:00:00Z", "turn_outcome": null}},
        "tool_call": {"kind":"tool_call", "seq":1, "call_id":"call", "title":"read", "tool_kind":"read", "status":"completed", "input":{}, "result":[], "closing_reason":null, "completion":{"started_at":"2026-09-30T12:00:00Z", "finished_at":"2026-09-30T12:00:00Z", "turn_outcome":null}},
        "session_state": {"kind":"session_state", "tools":[], "message_buffering":false, "thought_buffering":false, "usage": {"context_used": 1_200, "context_size": 200_000, "cost": {"amount": 0.42, "currency": "USD"}}},


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

/// The next event with this name, skipping whatever else a follow sends first: a follow registers
/// and says its presence once it has caught up, and entries and activities may come first.
async fn until_named(
    stream: &mut support::link_client::Events,
    name: &str,
) -> support::link_client::Event {
    let deadline = tokio::time::Instant::now() + PATIENCE;

    loop {
        let patience = deadline.saturating_duration_since(tokio::time::Instant::now());
        match stream.next_within(patience).await {
            Next::Event(event) if event.name.as_deref() == Some(name) => return event,
            Next::Event(_) => {}
            Next::Quiet => panic!("the stream never said {name}"),
            Next::Closed => panic!("the stream closed before it said {name}"),
        }
    }
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
        Entry::Expired {
            expired_at: "2026-09-30T00:00:00Z".parse().unwrap(),
        },
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
            result: Box::new(serde_json::json!([])),
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
            usage: None,
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
            result: Box::new(serde_json::json!(["read"])),
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
    assert!(stream.contains("event: activity"));
    assert!(stream.contains(&format!("id: {}:6", session.workspace)));
    assert!(!stream.contains("reasoning"));
    kestrel.teardown().await;
}

#[tokio::test]
async fn activity_summaries_survive_pages_resume_and_expansion() {
    let kestrel = Kestrel::boot().await;
    let (session, on) = a_session(&kestrel).await;
    let link = Link::to(&kestrel.link());
    let completion = kestrel::log::Completion::at("2026-09-29T12:00:00Z".parse().unwrap());
    for seq in 1..=105 {
        let report = Reported {
            session: Some(session.id),
            seq: Some(seq),
            report: Report::Thought {
                text: "private reasoning".into(),
                completion: completion.clone(),
            },
        };
        assert_eq!(
            link.report(&on.instance, Some(&on.credential), &report)
                .await
                .status(),
            StatusCode::ACCEPTED
        );
    }
    let client = reqwest::Client::new();
    let base = format!(
        "{}/link/instances/{}/entries",
        kestrel.link(),
        on.instance.replace('/', "%2F")
    );
    let first: serde_json::Value = client
        .get(&base)
        .bearer_auth(on.credential.as_str())
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(first["activities"][0]["first_seq"], 3);
    assert_eq!(first["activities"][0]["last_seq"], 100);
    assert_eq!(first["activities"][0]["counts"]["thoughts"], 98);
    assert_eq!(first["activities"][0]["closed"], false);
    let resumed: serde_json::Value = client
        .get(format!(
            "{base}?cursor={}",
            first["cursor"].as_str().unwrap()
        ))
        .bearer_auth(on.credential.as_str())
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(resumed["activities"][0]["first_seq"], 3);
    assert_eq!(resumed["activities"][0]["last_seq"], 107);
    assert_eq!(resumed["activities"][0]["counts"]["thoughts"], 105);
    let close = Reported {
        session: Some(session.id),
        seq: Some(106),
        report: Report::Said {
            message: "done".into(),
            completion,
        },
    };
    assert_eq!(
        link.report(&on.instance, Some(&on.credential), &close)
            .await
            .status(),
        StatusCode::ACCEPTED
    );
    let closed: serde_json::Value = client
        .get(format!(
            "{base}?cursor={}",
            resumed["cursor"].as_str().unwrap()
        ))
        .bearer_auth(on.credential.as_str())
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(closed["activities"][0]["first_seq"], 3);
    assert_eq!(closed["activities"][0]["counts"]["thoughts"], 105);
    assert_eq!(closed["activities"][0]["closed"], true);
    let expanded: serde_json::Value = client
        .get(format!(
            "{base}?first_seq=3&last_seq=107&kinds=shared_state,narration,detail&window=200"
        ))
        .bearer_auth(on.credential.as_str())
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(expanded["entries"].as_array().unwrap().len(), 105);
    assert_eq!(expanded["activities"], json!([]));
    let bare: serde_json::Value = client
        .get(format!("{base}?summaries=false&window=200"))
        .bearer_auth(on.credential.as_str())
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(bare["activities"], json!([]));
    assert_eq!(bare["entries"].as_array().unwrap().len(), 3);
    kestrel.teardown().await;
}

#[tokio::test]
async fn a_transcript_follow_replaces_activity_and_snapshots_session_state_without_ids() {
    let kestrel = Kestrel::boot().await;
    let (session, on) = a_session(&kestrel).await;
    let link = Link::to(&kestrel.link());
    let _instructions = link.open(&on.instance, &on.credential, None).await;
    let client = reqwest::Client::new();
    let base = format!(
        "{}/operator/organizations/acme/workspaces/{}/transcript",
        kestrel.operator(),
        session.workspace
    );
    let mut stream = support::link_client::Events::over(client.get(&base).send().await.unwrap());
    let snapshot = until_named(&mut stream, "session_state").await;
    assert_eq!(snapshot.id, None);
    let state: serde_json::Value = serde_json::from_str(&snapshot.data).unwrap();
    assert_eq!(state["tools"], json!([]));
    assert_eq!(state["message_buffering"], false);
    // The follow registers once it has caught up, so its entries and presence come first.
    let _ = until_named(&mut stream, "follower").await;
    let transient = Reported {
        session: Some(session.id),
        seq: None,
        report: Report::SessionState {
            tools: vec![kestrel::live_work::RunningTool {
                call_id: "running".into(),
                title: "read source".into(),
                status: "in_progress".into(),
                started_at: "2026-09-29T12:00:00Z".parse().unwrap(),
            }],
            message_buffering: true,
            thought_buffering: false,
            usage: None,
        },
    };
    assert_eq!(
        link.report(&on.instance, Some(&on.credential), &transient)
            .await
            .status(),
        StatusCode::ACCEPTED
    );
    let change = until_named(&mut stream, "session_state").await;
    assert_eq!(change.id, None);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&change.data).unwrap()["message_buffering"],
        true
    );
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&change.data).unwrap()["tools"][0]["title"],
        "read source"
    );
    let completion = kestrel::log::Completion::at("2026-09-29T12:00:00Z".parse().unwrap());
    let mut cursor = String::new();
    for seq in 1..=2 {
        let thought = Reported {
            session: Some(session.id),
            seq: Some(seq),
            report: Report::Thought {
                text: "reasoning".into(),
                completion: completion.clone(),
            },
        };
        assert_eq!(
            link.report(&on.instance, Some(&on.credential), &thought)
                .await
                .status(),
            StatusCode::ACCEPTED
        );
        let Next::Event(update) = stream.next_within(PATIENCE).await else {
            panic!("no summary")
        };
        assert_eq!(update.name.as_deref(), Some("activity"));
        let activity: serde_json::Value = serde_json::from_str(&update.data).unwrap();
        assert_eq!(activity["first_seq"], 3);
        assert_eq!(activity["counts"]["thoughts"], seq);
        assert_eq!(activity["closed"], false);
        cursor = update.id.unwrap();
    }
    let said = Reported {
        session: Some(session.id),
        seq: Some(3),
        report: Report::Said {
            message: "done".into(),
            completion,
        },
    };
    assert_eq!(
        link.report(&on.instance, Some(&on.credential), &said)
            .await
            .status(),
        StatusCode::ACCEPTED
    );
    for reconnect in [false, true] {
        if reconnect {
            stream = support::link_client::Events::over(
                client
                    .get(&base)
                    .header("Last-Event-ID", &cursor)
                    .send()
                    .await
                    .unwrap(),
            );
            let Next::Event(snapshot) = stream.next_within(PATIENCE).await else {
                panic!("no reconnect snapshot")
            };
            assert_eq!(snapshot.name.as_deref(), Some("session_state"));
            assert_eq!(snapshot.id, None);
        }
        let Next::Event(closed) = stream.next_within(PATIENCE).await else {
            panic!("no final summary")
        };
        assert_eq!(closed.name.as_deref(), Some("activity"));
        assert_eq!(closed.id.as_deref(), Some(cursor.as_str()));
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&closed.data).unwrap()["closed"],
            true
        );
        let Next::Event(entry) = stream.next_within(PATIENCE).await else {
            panic!("no message")
        };
        assert_eq!(entry.name.as_deref(), Some("entry"));
    }
    let bare = client
        .get(format!("{base}?follow=false&summaries=false"))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(!bare.contains("event: activity"));
    let partial = client
        .get(format!("{base}?follow=false&kinds=detail"))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert_eq!(partial.matches("event: activity").count(), 1);
    assert!(!partial.contains("event: entry"));
    let ranged = client
        .get(format!(
            "{base}?follow=false&first_seq=3&last_seq=4&kinds=shared_state,narration,detail"
        ))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert_eq!(ranged.matches("event: entry").count(), 2);
    assert!(!ranged.contains("event: activity"));
    kestrel.complete_session(&session).await;
    let mut empty = support::link_client::Events::over(client.get(&base).send().await.unwrap());
    let Next::Event(snapshot) = empty.next_within(PATIENCE).await else {
        panic!("no empty snapshot")
    };
    assert_eq!(snapshot.name.as_deref(), Some("session_state"));
    assert_eq!(snapshot.id, None);
    let state: serde_json::Value = serde_json::from_str(&snapshot.data).unwrap();
    assert_eq!(state["session_id"], serde_json::Value::Null);
    assert_eq!(state["tools"], json!([]));
    assert_eq!(state["message_buffering"], false);
    kestrel.teardown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn the_cli_prints_one_closed_activity_across_reconnects_and_can_disable_summaries() {
    let kestrel = Kestrel::boot().await;
    let (session, on) = a_session(&kestrel).await;
    let link = Link::to(&kestrel.link());
    let workspace = session.workspace.to_string();
    let mut cli = support::client::Client::spawn(
        &kestrel.operator(),
        &["workspace", "transcript", &workspace, "--follow"],
    );
    for _ in 0..2 {
        tokio::task::block_in_place(|| cli.line());
    }
    let completion = kestrel::log::Completion::at("2026-09-29T12:00:00Z".parse().unwrap());
    for seq in 1..=2 {
        let report = Reported {
            session: Some(session.id),
            seq: Some(seq),
            report: Report::Thought {
                text: "private reasoning".into(),
                completion: completion.clone(),
            },
        };
        assert_eq!(
            link.report(&on.instance, Some(&on.credential), &report)
                .await
                .status(),
            StatusCode::ACCEPTED
        );
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
    let kestrel = kestrel.kill().await.restart().await;
    let link = Link::to(&kestrel.link());
    let close = Reported {
        session: Some(session.id),
        seq: Some(3),
        report: Report::Said {
            message: "done".into(),
            completion,
        },
    };
    assert_eq!(
        link.report(&on.instance, Some(&on.credential), &close)
            .await
            .status(),
        StatusCode::ACCEPTED
    );
    let summary = tokio::task::block_in_place(|| cli.line());
    assert_eq!(
        summary,
        "activity  3..4  0 tools, 0 failed, 2 thoughts, 0 plans, 0 tombstones"
    );
    let said = tokio::task::block_in_place(|| cli.line());
    assert!(said.contains("done"));
    kestrel.complete_session(&session).await;
    kestrel.release_instance(session.workspace).await;
    kestrel.seal_workspace(session.workspace).await;
    let finished = tokio::task::block_in_place(|| cli.finish());
    assert!(finished.status.success(), "{}", finished.err);
    assert!(!finished.out.iter().any(|line| line.starts_with("activity")));
    for (flags, expect_summary, expect_thought) in [
        (vec![], true, false),
        (vec!["--no-summaries"], false, false),
        (
            vec!["--kinds", "shared_state,narration,detail"],
            false,
            true,
        ),
    ] {
        let mut args = vec!["workspace", "transcript", &workspace];
        args.extend(flags);
        let output =
            tokio::task::block_in_place(|| support::client::ran(&kestrel.operator(), &args));
        assert!(output.status.success(), "{}", output.err);
        assert_eq!(
            output.out.iter().any(|line| line.starts_with("activity")),
            expect_summary
        );
        assert_eq!(
            output
                .out
                .iter()
                .any(|line| line.contains("private reasoning")),
            expect_thought
        );
    }
    kestrel.teardown().await;
}

#[test]
fn published_transcript_reads_describe_activities_ranges_and_snapshots() {
    for (document, path) in [
        (
            include_str!("../../../openapi/link.json"),
            "/link/instances/{instance}/entries",
        ),
        (
            include_str!("../../../openapi/operator.json"),
            "/operator/organizations/{organization}/workspaces/{workspace}/transcript",
        ),
    ] {
        let document: serde_json::Value = serde_json::from_str(document).unwrap();
        let parameters = document["paths"][path]["get"]["parameters"]
            .as_array()
            .unwrap();
        for name in ["summaries", "first_seq", "last_seq"] {
            assert!(
                parameters.iter().any(|parameter| parameter["name"] == name),
                "missing {name}"
            );
        }
        let summary = parameters
            .iter()
            .find(|parameter| parameter["name"] == "summaries")
            .unwrap();
        assert_eq!(summary["schema"]["default"], true);
        let activity = &document["components"]["schemas"]["Activity"];
        assert_eq!(activity["properties"]["closed"]["type"], "boolean");
        assert_eq!(activity["properties"]["first_seq"]["type"], "integer");
        assert!(
            activity["required"]
                .as_array()
                .unwrap()
                .contains(&json!("counts"))
        );
        if path.ends_with("transcript") {
            let snapshot = &document["components"]["schemas"]["TranscriptSessionState"];
            assert!(
                snapshot["required"]
                    .as_array()
                    .unwrap()
                    .contains(&json!("tools"))
            );
            assert!(
                snapshot["required"]
                    .as_array()
                    .unwrap()
                    .contains(&json!("usage")),
                "the session_state snapshot does not document the usage it carries"
            );
            assert_eq!(
                snapshot["properties"]["usage"]["anyOf"][0]["$ref"],
                "#/components/schemas/Usage"
            );
            assert!(
                document["components"]["schemas"]["Event"]["oneOf"]
                    .as_array()
                    .unwrap()
                    .contains(&json!({"$ref":"#/components/schemas/Activity"}))
            );
        } else {
            assert_eq!(
                document["components"]["schemas"]["Page"]["properties"]["activities"]["items"]["$ref"],
                "#/components/schemas/Activity"
            );
        }
    }
}

#[tokio::test]
async fn activity_metadata_counts_only_omitted_kinds_and_expiry_leaves_only_tombstones() {
    let kestrel = Kestrel::boot().await;
    let (session, on) = a_session(&kestrel).await;
    let link = Link::to(&kestrel.link());
    let completion = |started: &str, finished: &str| json!({"started_at": started, "finished_at": finished, "turn_outcome": null});
    let reports = [
        json!({"kind":"tool_call", "call_id":"one", "title":"read source", "tool_kind":"read", "status":"failed", "input":{}, "result":"x".repeat(70 * 1024), "closing_reason":"interrupted", "completion":completion("2026-09-29T12:00:00Z", "2026-09-29T12:02:00Z")}),
        json!({"kind":"thought", "text":"reasoning", "completion":completion("2026-09-29T12:01:00Z", "2026-09-29T12:05:00Z")}),
        json!({"kind":"plan", "entries":[], "completion":completion("2026-09-29T12:03:00Z", "2026-09-29T12:04:00Z")}),
        json!({"kind":"said", "message":"a boundary", "completion":completion("2026-09-29T12:05:00Z", "2026-09-29T12:05:00Z")}),
        json!({"kind":"thought", "text":"next Activity", "completion":completion("2026-09-29T12:06:00Z", "2026-09-29T12:07:00Z")}),
    ];
    for (seq, mut report) in reports.into_iter().enumerate() {
        report["session"] = json!(session.id);
        report["seq"] = json!(seq + 1);
        assert_eq!(
            link.report_body(&on.instance, Some(&on.credential), &report)
                .await
                .status(),
            StatusCode::ACCEPTED
        );
    }
    let base = format!(
        "{}/link/instances/{}/entries",
        kestrel.link(),
        on.instance.replace('/', "%2F")
    );
    let client = reqwest::Client::new();
    let page: serde_json::Value = client
        .get(&base)
        .bearer_auth(on.credential.as_str())
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let activities = page["activities"].as_array().unwrap();
    assert_eq!(activities.len(), 2);
    assert_eq!(
        activities[0]["counts"],
        json!({"tool_calls":1,"failed_calls":1,"thoughts":1,"plans":1,"tombstones":0})
    );
    assert_eq!(
        activities[0]["latest"],
        json!({"kind":"narration","title":"Plan","status":null})
    );
    assert_eq!(activities[0]["started_at"], "2026-09-29T12:00:00Z");
    assert_eq!(activities[0]["finished_at"], "2026-09-29T12:05:00Z");
    assert_eq!(activities[0]["anomaly"], true);
    assert_eq!(activities[0]["closed"], true);
    assert_eq!(activities[1]["first_seq"], 7);
    let partial: serde_json::Value = client
        .get(format!("{base}?kinds=narration"))
        .bearer_auth(on.credential.as_str())
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(partial["activities"].as_array().unwrap().len(), 1);
    assert_eq!(partial["activities"][0]["first_seq"], 3);
    assert_eq!(partial["activities"][0]["last_seq"], 5);
    assert_eq!(
        partial["activities"][0]["counts"],
        json!({"tool_calls":1,"failed_calls":1,"thoughts":0,"plans":0,"tombstones":0})
    );
    assert_eq!(
        partial["activities"][0]["latest"],
        json!({"kind":"detail","title":"read source","status":"failed"})
    );
    let expanded: serde_json::Value = client
        .get(format!(
            "{base}?first_seq=3&last_seq=5&kinds=shared_state,narration,detail"
        ))
        .bearer_auth(on.credential.as_str())
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        expanded["entries"][0]["entry"]["result"],
        "x".repeat(70 * 1024)
    );
    assert!(expanded["entries"][0]["entry"]["payload_fields"].is_null());
    assert_eq!(expanded["entries"][0]["entry"]["title"], "read source");
    for seq in 3..=5 {
        kestrel.expire_payload_entry(session.workspace, seq).await;
    }
    let expired: serde_json::Value = client
        .get(&base)
        .bearer_auth(on.credential.as_str())
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        expired["activities"][0]["counts"],
        json!({"tool_calls":0,"failed_calls":0,"thoughts":0,"plans":0,"tombstones":3})
    );
    assert_eq!(
        expired["activities"][0]["started_at"],
        serde_json::Value::Null
    );
    assert_eq!(
        expired["activities"][0]["finished_at"],
        serde_json::Value::Null
    );
    assert_eq!(expired["activities"][0]["anomaly"], false);
    assert_eq!(
        expired["activities"][0]["latest"]["title"],
        serde_json::Value::Null
    );
    kestrel.teardown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_cli_cut_after_the_final_activity_id_does_not_print_its_replacement_twice() {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    let connections = Arc::new(AtomicUsize::new(0));
    let requests = connections.clone();
    let app = axum::Router::new().route(
        "/operator/organizations/acme/workspaces/example/transcript",
        axum::routing::get(move |headers: axum::http::HeaderMap| {
            let request = requests.fetch_add(1, Ordering::SeqCst);
            async move {
                let snapshot = "event: session_state\ndata: {\"session_id\":null,\"tools\":[],\"message_buffering\":false,\"thought_buffering\":false}\n\n";
                let summary = "id: example:4\nevent: activity\ndata: {\"first_seq\":3,\"last_seq\":4,\"counts\":{\"tool_calls\":0,\"failed_calls\":0,\"thoughts\":2,\"plans\":0,\"tombstones\":0},\"closed\":true,\"anomaly\":false}\n\n";
                let tail = if request == 0 { "" } else {
                    assert_eq!(headers["last-event-id"], "example:4");
                    "id: example:5\nevent: entry\ndata: {\"seq\":5,\"appended_at\":\"2026-09-29T12:00:00Z\",\"entry\":{\"type\":\"said\",\"message\":\"done\"}}\n\nevent: end\ndata: {\"because\":\"sealed\"}\n\n"
                };
                ([("content-type", "text/event-stream")], format!("{snapshot}{summary}{tail}"))
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let output = tokio::task::block_in_place(|| {
        support::client::ran(
            &address,
            &[
                "--organization",
                "acme",
                "workspace",
                "transcript",
                "example",
                "--follow",
            ],
        )
    });
    server.abort();
    assert!(output.status.success(), "{}", output.err);
    assert_eq!(connections.load(Ordering::SeqCst), 2);
    assert_eq!(
        output
            .out
            .iter()
            .filter(|line| line.starts_with("activity"))
            .count(),
        1
    );
    assert!(output.out.last().unwrap().contains("done"));
    assert!(output.err.contains("cursor  example:5"));
}

#[tokio::test]
async fn a_finite_transcript_range_ends_when_caught_up_and_rejects_invalid_ranges() {
    let kestrel = Kestrel::boot().await;
    let (session, on) = a_session(&kestrel).await;
    let operator = format!(
        "{}/operator/organizations/acme/workspaces/{}/transcript",
        kestrel.operator(),
        session.workspace
    );
    let link = format!(
        "{}/link/instances/{}/entries",
        kestrel.link(),
        on.instance.replace('/', "%2F")
    );
    let client = reqwest::Client::new();
    for range in ["first_seq=0", "last_seq=-1", "first_seq=4&last_seq=3"] {
        assert_eq!(
            client
                .get(format!("{operator}?{range}"))
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            client
                .get(format!("{link}?{range}"))
                .bearer_auth(on.credential.as_str())
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::BAD_REQUEST
        );
    }
    let response = client
        .get(format!(
            "{operator}?first_seq=3&last_seq=20&kinds=shared_state,narration,detail"
        ))
        .send()
        .await
        .unwrap();
    let stream = tokio::time::timeout(Duration::from_secs(1), response.text())
        .await
        .expect("a caught-up finite range must end")
        .unwrap();
    assert!(stream.contains("event: end"));
    assert!(!stream.contains("event: entry"));
    kestrel.teardown().await;
}
