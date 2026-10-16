//! A stepped control plane runs its work role only when the test asks (ADR-0059), so each test
//! here puts what the work role races on one side of a pass or the other on purpose.

use crate::support;

use std::time::Duration;

use jiff::{SignedDuration, Timestamp};
use kestrel::domain::{Exit, Session, SessionState, Workspace, WorkspaceState};
use kestrel::work::{Report, Reported};
use reqwest::StatusCode;
use support::fixture::Fixture;
use support::link_client::Link;
use support::scripted_agent::{self, Script};
use support::{Kestrel, supervisor};

const PATIENCE: Duration = Duration::from_secs(30);

async fn dispatching() -> Kestrel {
    Kestrel::stepped_dispatching_to(
        supervisor::binary(),
        &scripted_agent::playing(Script::ReportsThenWaits),
    )
    .await
}

async fn a_workspace(kestrel: &Kestrel) -> Workspace {
    Fixture::acme()
        .checked_out()
        .model(kestrel_scripted_agent::OTHER_MODEL)
        .holding_a_provider_key()
        .open(kestrel)
        .await
}

async fn another_workspace(kestrel: &Kestrel) -> Workspace {
    Fixture::acme()
        .checked_out()
        .model(kestrel_scripted_agent::OTHER_MODEL)
        .open_another(kestrel)
        .await
}

fn claimed(session: &Session) -> bool {
    session.state != SessionState::Queued
}

async fn until_claimed(kestrel: &Kestrel, session: &Session) {
    let deadline = tokio::time::Instant::now() + PATIENCE;
    while !claimed(&kestrel.session(session.id).await) {
        assert!(
            tokio::time::Instant::now() < deadline,
            "the session {} was never claimed",
            session.id
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test]
async fn a_pass_claims_one_queued_session_and_returns_with_it_on_its_supervisor() {
    let kestrel = dispatching().await;
    let first = a_workspace(&kestrel).await;
    let second = another_workspace(&kestrel).await;
    let first = kestrel.enqueue_session(first.id).await;
    let second = kestrel.enqueue_session(second.id).await;

    kestrel.work_pass().await;
    let after_one = [
        kestrel.session(first.id).await,
        kestrel.session(second.id).await,
    ];
    let on_a_supervisor: Vec<_> = after_one
        .iter()
        .filter(|session| claimed(session))
        .collect();
    assert_eq!(
        on_a_supervisor.len(),
        1,
        "one pass claims one session: {after_one:?}"
    );
    assert!(
        on_a_supervisor[0].supervisor.is_some(),
        "a pass returns once its session's supervisor is recorded: {:?}",
        on_a_supervisor[0]
    );

    kestrel.work_pass().await;
    assert!(claimed(&kestrel.session(first.id).await));
    assert!(claimed(&kestrel.session(second.id).await));

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_stepped_work_role_turned_live_claims_without_a_pass() {
    let kestrel = dispatching().await;
    let workspace = a_workspace(&kestrel).await;

    kestrel.go_live().await;
    let session = kestrel.enqueue_session(workspace.id).await;

    until_claimed(&kestrel, &session).await;
    kestrel.teardown().await;
}

#[tokio::test]
async fn a_session_stopped_before_the_pass_that_would_claim_it_is_never_dispatched() {
    let kestrel = dispatching().await;
    let workspace = a_workspace(&kestrel).await;
    let session = kestrel.enqueue_session(workspace.id).await;

    kestrel.stop_session(session.id).await;
    kestrel.work_pass().await;

    let after = kestrel.session(session.id).await;
    assert_eq!(after.state, SessionState::Ended);
    assert!(
        after.instance.is_none(),
        "a stopped session was dispatched: {after:?}"
    );
    kestrel.teardown().await;
}

#[tokio::test]
async fn a_session_stopped_after_the_pass_that_claimed_it_ends_on_its_instance() {
    let kestrel = dispatching().await;
    let workspace = a_workspace(&kestrel).await;
    let session = kestrel.enqueue_session(workspace.id).await;

    kestrel.work_pass().await;
    kestrel.stop_session(session.id).await;

    let after = kestrel.session(session.id).await;
    assert_eq!(after.state, SessionState::Ended);
    assert!(
        after.supervisor.is_some(),
        "the pass returned before its session reached a supervisor: {after:?}"
    );
    kestrel.teardown().await;
}

#[tokio::test]
async fn a_report_on_a_lapsed_lease_is_taken_before_the_sweep_and_refused_after_it() {
    let kestrel = Kestrel::stepped().await;
    let workspace = a_workspace(&kestrel).await;
    let (session, on) = kestrel.dispatch_to_the_link(workspace.id).await;
    kestrel
        .lease_until(&session, Timestamp::now() - SignedDuration::from_secs(1))
        .await;
    let link = Link::to(&kestrel.link());
    let started = |seq| Reported {
        session: Some(session.id),
        seq: Some(seq),
        report: Report::Started,
    };

    let before = link
        .report(&on.instance, Some(&on.credential), &started(1))
        .await;
    assert_eq!(before.status(), StatusCode::ACCEPTED);
    kestrel.work_pass().await;
    let after = link
        .report(&on.instance, Some(&on.credential), &started(2))
        .await;

    assert_eq!(after.status(), StatusCode::GONE);
    let ended = kestrel.session(session.id).await;
    assert!(
        matches!(&ended.exit, Some(Exit::Failed { because }) if because.contains("lease")),
        "{ended:?}"
    );
    kestrel.teardown().await;
}

#[tokio::test]
async fn a_session_enqueued_before_the_idle_sweep_keeps_its_workspace_open() {
    let kestrel = Kestrel::stepped().await;
    let held = a_workspace(&kestrel).await;
    let idle = another_workspace(&kestrel).await;
    let a_day_ago = Timestamp::now() - SignedDuration::from_hours(25);
    kestrel.last_active(&held, a_day_ago).await;
    kestrel.last_active(&idle, a_day_ago).await;

    kestrel.enqueue_session(held.id).await;
    kestrel.work_pass().await;

    assert_eq!(
        kestrel.show_workspace(held.id).await.state,
        WorkspaceState::Open
    );
    assert_eq!(
        kestrel.show_workspace(idle.id).await.state,
        WorkspaceState::Sealed
    );
    kestrel.teardown().await;
}
