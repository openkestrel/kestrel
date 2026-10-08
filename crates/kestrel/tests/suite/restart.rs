//! ADR-0002's definition of done for rung 0.1: the control plane dies under a Session in flight,
//! comes back, and the Session completes with a gap-free Transcript. The Environment outlives it,
//! because the supervisor holds a cursor and nothing else.

use crate::support;

use std::time::Duration;

use jiff::{SignedDuration, Timestamp};
use kestrel::domain::{Exit, Session, SessionId, SessionState, Workspace};
use kestrel::work::{Report, Reported};
use reqwest::StatusCode;
use serde_json::json;
use support::fixture::Fixture;
use support::link_client::Link;
use support::scripted_agent::Script;
use support::supervisor::Supervisor;
use support::{Kestrel, OnTheLink};

const PATIENCE: Duration = Duration::from_secs(45);
const LONG_ENOUGH_TO_BE_SURE: Duration = Duration::from_secs(1);

async fn a_workspace(kestrel: &Kestrel) -> Workspace {
    Fixture::acme()
        .without_repositories()
        .model("claude-opus-5")
        .open(kestrel)
        .await
}

async fn until(
    kestrel: &Kestrel,
    session: SessionId,
    what: &str,
    ready: impl Fn(&Session) -> bool,
) -> Session {
    let deadline = tokio::time::Instant::now() + PATIENCE;

    loop {
        let session = kestrel.session(session).await;
        if ready(&session) {
            return session;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the session {} is {} with the exit status {:?}, and never {what}",
            session.id,
            session.state,
            session.exit
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

async fn transcript(kestrel: &Kestrel, workspace: &Workspace) -> Vec<String> {
    kestrel
        .transcript(workspace.id)
        .await
        .iter()
        .map(|entry| entry.entry.to_string())
        .collect()
}

async fn working(
    kestrel: &Kestrel,
    workspace: &Workspace,
    script: Script,
) -> (Session, Supervisor) {
    let (session, on) = kestrel.dispatch_to_the_link(workspace.id).await;
    let mut supervisor = Supervisor::provision_playing(&kestrel.link(), &on, script);

    supervisor.wait_until_it_says("reported connected").await;
    kestrel.start(&session, supervisor.harness()).await;
    supervisor.wait_until_it_says("reported started").await;

    (session, supervisor)
}

/// Killed while the agent is still working at its turn, and left down until the Environment
/// has something to say and finds nothing there to say it to.
async fn killed_mid_session() -> (Kestrel, Workspace, Session, Supervisor) {
    let kestrel = Kestrel::boot().await;
    let workspace = a_workspace(&kestrel).await;
    let (session, mut supervisor) = working(&kestrel, &workspace, Script::Lingers).await;

    let stopped = kestrel.kill().await;
    supervisor.wait_until_it_says("lost the link").await;

    (stopped.restart().await, workspace, session, supervisor)
}

#[tokio::test]
async fn a_turn_in_flight_when_the_control_plane_is_killed_is_answered_after_it_restarts() {
    let (kestrel, _, session, mut supervisor) = killed_mid_session().await;

    let ended = kestrel.after_one_turn(session.id).await;

    assert_eq!(ended.exit, Some(Exit::Succeeded));
    supervisor.lets_go_of(session.id).await;
    kestrel.teardown().await;
}

#[tokio::test]
async fn the_transcript_of_a_session_that_outlived_a_restart_has_no_gap_and_no_duplicate() {
    let (kestrel, workspace, session, mut supervisor) = killed_mid_session().await;
    kestrel.after_one_turn(session.id).await;

    assert_eq!(
        transcript(&kestrel, &workspace).await,
        vec![
            "participant joined  builder".to_owned(),
            "said  operator  do the work this environment was provisioned for".to_owned(),
            format!("session started  {}  builder", session.id),
            "said  builder  half of one message, and the other half".to_owned(),
            "said  builder  a second message".to_owned(),
            format!("session ended  {}  succeeded", session.id),
        ]
    );
    let all = reqwest::Client::new()
        .get(format!("{}/operator/organizations/acme/workspaces/{}/transcript?follow=false&kinds=shared_state,narration,detail", kestrel.operator(), workspace.id))
        .send().await.unwrap().text().await.unwrap();
    let entries: Vec<serde_json::Value> = all
        .split("\n\n")
        .filter_map(|frame| {
            if !frame.contains("event: entry") {
                return None;
            }
            let data = frame
                .lines()
                .find_map(|line| line.strip_prefix("data: "))
                .unwrap();
            Some(serde_json::from_str(data).unwrap())
        })
        .collect();
    let sequences: Vec<i64> = entries
        .iter()
        .map(|entry| entry["seq"].as_i64().unwrap())
        .collect();
    assert_eq!(sequences, (1..=9).collect::<Vec<_>>());
    let tools: Vec<_> = entries
        .iter()
        .filter(|entry| entry["entry"]["type"] == "tool_call")
        .collect();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0]["kind"], "detail");
    assert_eq!(tools[0]["session_id"], session.id.to_string());

    supervisor.lets_go_of(session.id).await;
    kestrel.teardown().await;
}

#[tokio::test]
async fn the_environment_comes_back_on_its_own_carrying_the_cursor_it_held() {
    let (kestrel, _, session, mut supervisor) = killed_mid_session().await;

    supervisor.wait_until_it_says("link open after").await;

    assert!(
        kestrel.session(session.id).await.connected.is_some(),
        "the control plane that came back does not know an environment is on the link"
    );
    kestrel.after_one_turn(session.id).await;
    supervisor.lets_go_of(session.id).await;
    kestrel.teardown().await;
}

/// A lease due sooner than a real one, and further off than the Environment's next heartbeat.
fn shortened() -> Timestamp {
    Timestamp::now() + SignedDuration::from_secs(6)
}

/// A second Session whose lease is already up when the control plane comes back, so the sweep
/// that reaps it is observably the same sweep that left the live one alone.
#[tokio::test]
async fn a_lease_is_not_swept_while_the_environment_that_holds_it_out_is_reconnecting() {
    let kestrel = Kestrel::boot().await;
    let workspace = a_workspace(&kestrel).await;
    let (session, supervisor) = working(&kestrel, &workspace, Script::Dawdles).await;
    let abandoned = kestrel
        .dispatch_session(
            kestrel
                .open_workspace("acme", "kestrel", "builder")
                .await
                .id,
        )
        .await;

    let stopped = kestrel.kill().await;
    let shortened = shortened();
    stopped.lease_until(&session, shortened).await;
    stopped
        .lease_until(&abandoned, Timestamp::now() - SignedDuration::from_secs(1))
        .await;
    let kestrel = stopped.restart().await;

    let expired = until(&kestrel, abandoned.id, "was swept", |session| {
        session.state == SessionState::Ended
    })
    .await;
    assert_eq!(
        expired.exit,
        Some(Exit::Failed {
            because: "the supervisor stopped holding the session's lease out, and it expired"
                .to_owned()
        })
    );
    assert_eq!(
        kestrel.session(session.id).await.state,
        SessionState::Working,
        "the sweep that reaped the expired lease took the live one with it"
    );

    let held = until(
        &kestrel,
        session.id,
        "had its lease held out again",
        |session| session.lease_expires_at > Some(shortened),
    )
    .await;
    assert_eq!(held.state, SessionState::Working);

    supervisor.destroy();
    kestrel.teardown().await;
}

#[tokio::test]
async fn restarting_with_no_session_in_flight_changes_nothing() {
    let kestrel = Kestrel::boot().await;
    let workspace = a_workspace(&kestrel).await;
    let session = kestrel.dispatch_session(workspace.id).await;
    kestrel.complete_session(&session).await;
    let before = transcript(&kestrel, &workspace).await;

    let kestrel = kestrel.kill_and_restart().await;
    tokio::time::sleep(LONG_ENOUGH_TO_BE_SURE).await;

    assert_eq!(transcript(&kestrel, &workspace).await, before);
    assert_eq!(
        kestrel.session(session.id).await.exit,
        Some(Exit::Succeeded)
    );
    assert_eq!(kestrel.sessions(workspace.id).await.len(), 1);

    kestrel.teardown().await;
}

async fn a_session(kestrel: &Kestrel) -> (Session, OnTheLink) {
    let workspace = a_workspace(kestrel).await;

    kestrel.dispatch_to_the_link(workspace.id).await
}

#[tokio::test]
async fn a_report_whose_answer_never_arrived_is_taken_once_when_it_is_sent_again() {
    let kestrel = Kestrel::boot().await;
    let (session, on) = a_session(&kestrel).await;
    let link = Link::to(&kestrel.link());
    let said = Reported {
        session: Some(session.id),
        seq: Some(1),
        report: Report::Said {
            message: "said once, and reported twice".to_owned(),
            completion: kestrel::log::Completion::at("2026-09-29T12:00:00Z".parse().unwrap()),
        },
    };

    for _ in 0..2 {
        assert_eq!(
            link.report(&on.instance, Some(&on.credential), &said)
                .await
                .status(),
            StatusCode::ACCEPTED
        );
    }

    assert_eq!(
        kestrel
            .transcript(session.workspace)
            .await
            .iter()
            .filter(|entry| entry
                .entry
                .to_string()
                .contains("said once, and reported twice"))
            .count(),
        1
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_report_that_skips_one_the_environment_has_yet_to_send_is_refused() {
    let kestrel = Kestrel::boot().await;
    let (session, on) = a_session(&kestrel).await;
    let link = Link::to(&kestrel.link());

    let refused = link
        .report_body(
            &on.instance,
            Some(&on.credential),
            &json!({"session": session.id, "kind": "said", "completion": {"started_at": "2026-09-29T12:00:00Z", "finished_at": "2026-09-29T12:00:00Z", "turn_outcome": null}, "seq": 2, "message": "the one before this is missing"}),
        )
        .await;

    assert_eq!(refused.status(), StatusCode::BAD_REQUEST);
    assert_eq!(kestrel.transcript(session.workspace).await.len(), 2);

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_report_that_changes_the_sessions_record_and_is_not_numbered_is_refused() {
    let kestrel = Kestrel::boot().await;
    let (session, on) = a_session(&kestrel).await;
    let link = Link::to(&kestrel.link());

    let refused = link
        .report_body(
            &on.instance,
            Some(&on.credential),
            &json!({"session": session.id, "kind": "said", "completion": {"started_at": "2026-09-29T12:00:00Z", "finished_at": "2026-09-29T12:00:00Z", "turn_outcome": null}, "message": "unnumbered"}),
        )
        .await;

    assert_eq!(refused.status(), StatusCode::BAD_REQUEST);
    assert_eq!(kestrel.transcript(session.workspace).await.len(), 2);

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_gap_gives_a_healthy_waiting_session_grace_and_attributes_a_lost_supervisor() {
    let kestrel = Kestrel::boot().await;
    let workspace = a_workspace(&kestrel).await;
    let (healthy, supervisor) = working(&kestrel, &workspace, Script::Speaks).await;
    until(&kestrel, healthy.id, "answered and settled", |session| {
        session.state == SessionState::Waiting
    })
    .await;
    let workspace = kestrel.open_workspace("acme", "kestrel", "builder").await;
    let (lost, lost_supervisor) = working(&kestrel, &workspace, Script::Speaks).await;
    until(&kestrel, lost.id, "answered and settled", |session| {
        session.state == SessionState::Waiting
    })
    .await;
    let workspace = kestrel.open_workspace("acme", "kestrel", "builder").await;
    let (forgotten, on) = kestrel.dispatch_to_the_link(workspace.id).await;
    let command = support::scripted_agent::playing(Script::Speaks);
    let mut forgetful = Supervisor::provision_running(
        &kestrel.link(),
        &on,
        &command,
        "",
        Some(Duration::from_secs(1)),
    );
    forgetful.wait_until_it_says("reported connected").await;
    kestrel.start(&forgotten, forgetful.harness()).await;
    until(&kestrel, forgotten.id, "answered and settled", |session| {
        session.state == SessionState::Waiting
    })
    .await;
    let stopped = kestrel.kill().await;
    lost_supervisor.destroy();
    forgetful.wait_until_it_says("let the session").await;
    let start = Timestamp::now() - SignedDuration::from_mins(9);
    stopped.database().last_lease_sweep(start).await;
    let lapsed = Timestamp::now() - SignedDuration::from_secs(1);
    stopped.lease_until(&healthy, lapsed).await;
    stopped.lease_until(&lost, lapsed).await;
    stopped.lease_until(&forgotten, lapsed).await;
    let kestrel = stopped.restart().await;
    let held = until(&kestrel, healthy.id, "renewed its lease", |session| {
        session.lease_expires_at > Some(Timestamp::now() + SignedDuration::from_secs(60))
    })
    .await;
    assert_eq!(held.state, SessionState::Waiting);
    let graced = kestrel.session(lost.id).await;
    assert_eq!(graced.state, SessionState::Waiting);
    let deadline = graced.lease_expires_at.unwrap();
    assert!(deadline > Timestamp::now());
    let ended = until(&kestrel, lost.id, "failed after grace", |session| {
        session.state == SessionState::Ended
    })
    .await;
    assert!(Timestamp::now() >= deadline);
    let end = deadline - SignedDuration::from_secs(30);
    assert_eq!(
        ended.exit,
        Some(Exit::Failed {
            because: format!(
                "the control plane was not running from {start} to {end}, and the supervisor did not reach it within 30 s after"
            )
        })
    );
    let forgotten = until(
        &kestrel,
        forgotten.id,
        "failed without renewing the session it let go",
        |session| session.state == SessionState::Ended,
    )
    .await;
    assert_eq!(
        forgotten.exit,
        Some(Exit::Failed {
            because: format!(
                "the supervisor came back after the control plane was not running from {start} to {end}, no longer carrying the session"
            )
        })
    );
    assert_eq!(
        kestrel.session(healthy.id).await.state,
        SessionState::Waiting
    );
    assert_eq!(kestrel.sessions(lost.workspace).await.len(), 1);
    supervisor.destroy();
    forgetful.destroy();
    kestrel.teardown().await;
}
