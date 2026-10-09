//! Whether a Session's open work is currently observed, read through the operator boundary while
//! a client of kestrel's own plays the supervisor on the link.

use crate::support;

use std::time::Duration;

use kestrel::domain::{Session, SessionId};
use kestrel::live_work::{RunningTool, RunningUnit, UnitKind};
use kestrel::log::ToolStatus;
use kestrel::operator;
use kestrel::work::{Report, Reported};
use reqwest::StatusCode;
use serde_json::{Value, json};
use support::link_client::{Events, Link, Next};
use support::{Kestrel, OnTheLink};

const PATIENCE: Duration = Duration::from_secs(30);

async fn a_session(kestrel: &Kestrel) -> (Session, OnTheLink) {
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
        .declare_agent(&organization, "builder", "opencode", None)
        .await;
    let workspace = kestrel.open_workspace("acme", "kestrel", "builder").await;

    kestrel.dispatch_to_the_link(workspace.id).await
}

fn a_tool() -> RunningTool {
    RunningTool {
        call_id: "call".to_owned(),
        title: "read source".to_owned(),
        tool_kind: "read".to_owned(),
        status: ToolStatus::InProgress,
        started_at: "2026-10-08T12:00:00Z".parse().unwrap(),
    }
}

fn a_unit(id: &str, kind: UnitKind, title: &str) -> RunningUnit {
    RunningUnit {
        id: id.to_owned(),
        kind,
        title: title.to_owned(),
        started_at: "2026-10-08T12:00:00Z".parse().unwrap(),
    }
}

fn open_work() -> Report {
    Report::SessionState {
        tools: vec![a_tool()],
        units: vec![
            a_unit("task", UnitKind::BackgroundTask, "background tests"),
            a_unit("child", UnitKind::Subagent, "reviewer"),
        ],
        message_buffering: false,
        thought_buffering: false,
        usage: None,
        last_activity_at: None,
    }
}

fn no_open_work() -> Report {
    Report::SessionState {
        tools: vec![],
        units: vec![],
        message_buffering: false,
        thought_buffering: false,
        usage: None,
        last_activity_at: None,
    }
}

async fn reported(link: &Link, on: &OnTheLink, session: SessionId, report: Report) {
    let status = link
        .report(
            &on.instance,
            Some(&on.credential),
            &Reported {
                session: Some(session),
                seq: None,
                report,
            },
        )
        .await
        .status();
    assert_eq!(status, StatusCode::ACCEPTED, "the link refused the report");
}

async fn session_read(kestrel: &Kestrel, session: &Session) -> Value {
    let path = operator::SESSION
        .replace("{organization}", "acme")
        .replace("{session}", &session.name);

    reqwest::get(format!("{}{path}", kestrel.operator()))
        .await
        .expect("the operator boundary should answer")
        .json()
        .await
        .expect("the session as JSON")
}

async fn read_until(kestrel: &Kestrel, session: &Session, done: impl Fn(&Value) -> bool) -> Value {
    let deadline = tokio::time::Instant::now() + PATIENCE;
    loop {
        let read = session_read(kestrel, session).await;
        if done(&read) {
            return read;
        }
        assert!(tokio::time::Instant::now() < deadline, "never came: {read}");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

async fn follow(kestrel: &Kestrel, session: &Session) -> Events {
    let path = format!(
        "{}/operator/organizations/acme/workspaces/{}/transcript",
        kestrel.operator(),
        session.workspace
    );

    Events::over(reqwest::Client::new().get(path).send().await.unwrap())
}

async fn next_state(stream: &mut Events, done: impl Fn(&Value) -> bool) -> Value {
    let deadline = tokio::time::Instant::now() + PATIENCE;
    loop {
        let patience = deadline.saturating_duration_since(tokio::time::Instant::now());
        match stream.next_within(patience).await {
            Next::Event(event) if event.name.as_deref() == Some("session_state") => {
                let state: Value = serde_json::from_str(&event.data).unwrap();
                if done(&state) {
                    return state;
                }
            }
            Next::Event(_) => {}
            Next::Quiet | Next::Closed => panic!("the follow never delivered the session state"),
        }
    }
}

fn availability(read: &Value) -> &Value {
    &read["observation"]["availability"]
}

fn observed_work(read: &Value) -> Value {
    json!({ "tools": read["tools"], "units": read["units"] })
}

#[tokio::test]
async fn a_current_observation_lists_open_tools_and_adapter_units_with_when_it_was_observed() {
    let kestrel = Kestrel::boot().await;
    let (session, on) = a_session(&kestrel).await;
    let link = Link::to(&kestrel.link());
    let _instructions = link.open(&on.instance, &on.credential, None).await;
    let mut transcript = follow(&kestrel, &session).await;

    let before = jiff::Timestamp::now();
    reported(&link, &on, session.id, open_work()).await;

    let read = session_read(&kestrel, &session).await;
    assert_eq!(availability(&read), "current", "{read}");
    let observed_at: jiff::Timestamp = read["observation"]["observed_at"]
        .as_str()
        .expect("a current observation says when it was observed")
        .parse()
        .unwrap();
    assert!(observed_at >= before, "{read}");
    assert_eq!(read["tools"][0]["title"], "read source");
    assert_eq!(
        read["units"]
            .as_array()
            .unwrap()
            .iter()
            .map(|unit| (unit["kind"].clone(), unit["title"].clone()))
            .collect::<Vec<_>>(),
        vec![
            (json!("background_task"), json!("background tests")),
            (json!("subagent"), json!("reviewer")),
        ]
    );

    let state = next_state(&mut transcript, |state| {
        state["observation"]["availability"] == "current" && state["units"] != json!([])
    })
    .await;
    assert_eq!(state["observation"], read["observation"]);
    assert_eq!(observed_work(&state), observed_work(&read));

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_current_observation_of_no_open_work_is_current_and_empty() {
    let kestrel = Kestrel::boot().await;
    let (session, on) = a_session(&kestrel).await;
    let link = Link::to(&kestrel.link());
    let _instructions = link.open(&on.instance, &on.credential, None).await;

    reported(&link, &on, session.id, no_open_work()).await;

    let read = session_read(&kestrel, &session).await;
    assert_eq!(availability(&read), "current", "{read}");
    assert!(read["observation"]["observed_at"].is_string(), "{read}");
    assert_eq!(observed_work(&read), json!({ "tools": [], "units": [] }));

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_connected_heartbeating_supervisor_that_sent_no_snapshot_is_not_an_empty_observation() {
    let kestrel = Kestrel::boot().await;
    let (session, on) = a_session(&kestrel).await;
    let link = Link::to(&kestrel.link());
    let _instructions = link.open(&on.instance, &on.credential, None).await;
    for report in [
        Report::Connected {
            version: "test".to_owned(),
        },
        Report::Heartbeat,
    ] {
        reported(&link, &on, session.id, report).await;
    }
    let mut transcript = follow(&kestrel, &session).await;

    let read = session_read(&kestrel, &session).await;
    assert_eq!(
        read["observation"],
        json!({ "availability": "unavailable", "last": null }),
        "{read}"
    );
    assert_eq!(observed_work(&read), json!({ "tools": [], "units": [] }));
    let state = next_state(&mut transcript, |_| true).await;
    assert_eq!(state["observation"], read["observation"]);

    kestrel.teardown().await;
}

#[tokio::test]
async fn link_loss_leaves_the_last_observation_historical_and_a_fresh_snapshot_restores_it() {
    let kestrel = Kestrel::boot().await;
    let (session, on) = a_session(&kestrel).await;
    let link = Link::to(&kestrel.link());
    let instructions = link.open(&on.instance, &on.credential, None).await;
    let mut transcript = follow(&kestrel, &session).await;
    reported(&link, &on, session.id, open_work()).await;
    let current = session_read(&kestrel, &session).await;
    assert_eq!(availability(&current), "current", "{current}");
    next_state(&mut transcript, |state| {
        state["observation"]["availability"] == "current"
    })
    .await;

    drop(instructions);
    let lost = read_until(&kestrel, &session, |read| {
        availability(read) == "unavailable"
    })
    .await;
    assert_eq!(
        lost["observation"]["last"],
        json!({
            "observed_at": current["observation"]["observed_at"],
            "tools": current["tools"],
            "units": current["units"],
        }),
        "{lost}"
    );
    assert_eq!(
        observed_work(&lost),
        json!({ "tools": [], "units": [] }),
        "a historical observation is never listed as current work"
    );
    assert_eq!(lost["state"], current["state"]);
    assert_eq!(lost["lease_expires_at"], current["lease_expires_at"]);
    let state = next_state(&mut transcript, |state| {
        state["observation"]["availability"] == "unavailable"
    })
    .await;
    assert_eq!(state["observation"], lost["observation"]);

    let _instructions = link.open(&on.instance, &on.credential, None).await;
    let reconnected = session_read(&kestrel, &session).await;
    assert_eq!(
        reconnected["observation"], lost["observation"],
        "a reconnect alone is not a fresh observation"
    );

    reported(&link, &on, session.id, no_open_work()).await;
    let restored = session_read(&kestrel, &session).await;
    assert_eq!(availability(&restored), "current", "{restored}");
    assert_eq!(
        observed_work(&restored),
        json!({ "tools": [], "units": [] })
    );
    assert_eq!(restored["state"], current["state"]);
    assert_eq!(restored["lease_expires_at"], current["lease_expires_at"]);
    next_state(&mut transcript, |state| {
        state["observation"]["availability"] == "current"
    })
    .await;

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_supervisor_gone_quiet_on_an_open_stream_leaves_its_observation_historical() {
    let kestrel = Kestrel::boot().await;
    let (session, on) = a_session(&kestrel).await;
    let link = Link::to(&kestrel.link());
    let _instructions = link.open(&on.instance, &on.credential, None).await;
    reported(&link, &on, session.id, no_open_work()).await;
    let current = session_read(&kestrel, &session).await;
    assert_eq!(availability(&current), "current", "{current}");

    let stale = read_until(&kestrel, &session, |read| {
        availability(read) == "unavailable"
    })
    .await;
    assert_eq!(
        stale["observation"]["last"],
        json!({
            "observed_at": current["observation"]["observed_at"],
            "tools": [],
            "units": [],
        }),
        "{stale}"
    );
    assert_eq!(stale["state"], current["state"]);

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_restarted_control_plane_holds_no_observation_until_a_fresh_snapshot() {
    let kestrel = Kestrel::boot().await;
    let (session, on) = a_session(&kestrel).await;
    let link = Link::to(&kestrel.link());
    let instructions = link.open(&on.instance, &on.credential, None).await;
    reported(&link, &on, session.id, open_work()).await;
    assert_eq!(
        availability(&session_read(&kestrel, &session).await),
        "current"
    );
    drop(instructions);

    let kestrel = kestrel.kill_and_restart().await;
    let restarted = session_read(&kestrel, &session).await;
    assert_eq!(
        restarted["observation"],
        json!({ "availability": "unavailable", "last": null }),
        "{restarted}"
    );
    assert_eq!(
        observed_work(&restarted),
        json!({ "tools": [], "units": [] })
    );

    let link = Link::to(&kestrel.link());
    let _instructions = link.open(&on.instance, &on.credential, None).await;
    reported(&link, &on, session.id, open_work()).await;
    let restored = session_read(&kestrel, &session).await;
    assert_eq!(availability(&restored), "current", "{restored}");
    assert_eq!(restored["units"].as_array().unwrap().len(), 2);

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_stopped_session_holds_no_observation() {
    let kestrel = Kestrel::boot().await;
    let (session, on) = a_session(&kestrel).await;
    let link = Link::to(&kestrel.link());
    let _instructions = link.open(&on.instance, &on.credential, None).await;
    reported(&link, &on, session.id, open_work()).await;

    let path = operator::SESSION_STOP
        .replace("{organization}", "acme")
        .replace("{session}", &session.name);
    let stopped: Value = reqwest::Client::new()
        .post(format!("{}{path}", kestrel.operator()))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        stopped["observation"],
        json!({ "availability": "unavailable", "last": null }),
        "{stopped}"
    );
    assert_eq!(
        session_read(&kestrel, &session).await["observation"],
        stopped["observation"]
    );

    kestrel.teardown().await;
}

#[test]
fn the_published_operator_document_requires_the_observation_on_session_reads_and_state() {
    let document: Value = serde_json::from_str(
        &std::fs::read_to_string(support::crate_root().join("../../openapi/operator.json"))
            .unwrap(),
    )
    .unwrap();
    for schema in ["Session", "TranscriptSessionState"] {
        let schema = &document["components"]["schemas"][schema];
        assert!(
            schema["required"]
                .as_array()
                .unwrap()
                .contains(&json!("observation")),
            "{schema}"
        );
        assert_eq!(
            schema["properties"]["observation"]["$ref"],
            "#/components/schemas/SessionObservation"
        );
    }
}
