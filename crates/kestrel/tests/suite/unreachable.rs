//! A queued Session whose declared tolerance can no longer be met becomes unreachable: terminal,
//! never claimed, and never carrying an exit status, because nothing failed. Tolerance defaults
//! to all-must-succeed, so one blocker failing is enough, however many others there are or were
//! still waiting behind it.

use crate::support;

use std::time::Duration;

use jiff::Timestamp;
use kestrel::domain::{Session, SessionState, Workspace};
use support::Kestrel;
use support::fixture::Fixture;

async fn a_workspace(kestrel: &Kestrel) -> Workspace {
    Fixture::acme().model("claude-opus-5").open(kestrel).await
}

async fn a_dependent_blocked_on_an_active_session(kestrel: &Kestrel) -> (Session, Session) {
    let workspace = a_workspace(kestrel).await;
    let blocker = kestrel.dispatch_session(workspace.id).await;
    let waiting = kestrel.open_workspace("acme", "kestrel", "builder").await;
    let dependent = kestrel.enqueue_session(waiting.id).await;
    kestrel.block_session(&dependent, &blocker).await;

    (blocker, dependent)
}

#[tokio::test]
async fn a_session_blocked_on_a_failed_blocker_becomes_unreachable() {
    let kestrel = Kestrel::boot().await;
    let (blocker, dependent) = a_dependent_blocked_on_an_active_session(&kestrel).await;

    kestrel
        .fail_session(&blocker, "the agent could not open a pull request")
        .await;

    let dependent = kestrel.session(dependent.id).await;
    assert_eq!(dependent.state, SessionState::Unreachable);
    assert!(
        dependent.exit.is_none(),
        "an unreachable session carried an exit status, and nothing failed"
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn one_blocker_failing_is_enough_however_many_others_have_not_resolved() {
    let kestrel = Kestrel::boot().await;
    let workspace = a_workspace(&kestrel).await;
    let succeeds = kestrel.dispatch_session(workspace.id).await;
    let elsewhere = kestrel.open_workspace("acme", "kestrel", "builder").await;
    let fails = kestrel.dispatch_session(elsewhere.id).await;
    let waiting = kestrel.open_workspace("acme", "kestrel", "builder").await;
    let dependent = kestrel.enqueue_session(waiting.id).await;
    kestrel.block_session(&dependent, &succeeds).await;
    kestrel.block_session(&dependent, &fails).await;

    kestrel.complete_session(&succeeds).await;
    assert_eq!(
        kestrel.session(dependent.id).await.state,
        SessionState::Queued
    );

    kestrel
        .fail_session(&fails, "the agent could not open a pull request")
        .await;

    assert_eq!(
        kestrel.session(dependent.id).await.state,
        SessionState::Unreachable,
        "a dependent with one blocker still to resolve became unreachable once another failed"
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn an_unreachable_session_is_never_claimed_and_never_becomes_claimable_again() {
    let kestrel = Kestrel::boot().await;
    let (blocker, dependent) = a_dependent_blocked_on_an_active_session(&kestrel).await;
    kestrel
        .fail_session(&blocker, "the agent could not open a pull request")
        .await;
    assert_eq!(
        kestrel.session(dependent.id).await.state,
        SessionState::Unreachable
    );

    for _ in 0..3 {
        assert!(
            kestrel.claim_session().await.is_none(),
            "an unreachable session was claimed"
        );
    }

    tokio::time::sleep(Duration::from_secs(1)).await;
    assert_eq!(
        kestrel.session(dependent.id).await.state,
        SessionState::Unreachable,
        "an unreachable session left its terminal state on its own"
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_blocker_failed_by_its_lease_expiring_makes_its_dependent_unreachable() {
    let kestrel = Kestrel::boot().await;
    let (blocker, dependent) = a_dependent_blocked_on_an_active_session(&kestrel).await;

    kestrel.lease_until(&blocker, Timestamp::now()).await;

    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        let dependent = kestrel.session(dependent.id).await;
        if dependent.state == SessionState::Unreachable {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the dependent is {:?}, and its blocker's lease expiring never made it unreachable",
            dependent.state
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_session_blocked_on_a_session_that_just_turned_unreachable_is_unreachable_too() {
    let kestrel = Kestrel::boot().await;
    let (blocker, first) = a_dependent_blocked_on_an_active_session(&kestrel).await;
    let waiting = kestrel.open_workspace("acme", "kestrel", "builder").await;
    let second = kestrel.enqueue_session(waiting.id).await;
    kestrel.block_session(&second, &first).await;

    kestrel
        .fail_session(&blocker, "the agent could not open a pull request")
        .await;

    assert_eq!(
        kestrel.session(first.id).await.state,
        SessionState::Unreachable
    );
    assert_eq!(
        kestrel.session(second.id).await.state,
        SessionState::Unreachable,
        "a session blocked on one that turned unreachable never turned unreachable itself"
    );

    kestrel.teardown().await;
}
