//! A Workspace's Sessions, one after another, run through the one supervisor that lives with its
//! Instance (ADR-0039): it outlives each Session and a control-plane restart, and goes only with the
//! Instance.

use crate::support;

use std::time::Duration;

use kestrel::domain::{Exit, Session, SessionState, Workspace};
use support::environment::Environment;
use support::scripted_agent::{self, Script};
use support::{Kestrel, repository, supervisor};

const PATIENCE: Duration = Duration::from_secs(30);
const LONG_ENOUGH_TO_BE_SURE: Duration = Duration::from_millis(500);

async fn dispatching() -> Kestrel {
    Kestrel::dispatching_to(
        supervisor::binary(),
        &scripted_agent::playing(Script::Speaks),
    )
    .await
}

async fn a_workspace(kestrel: &Kestrel) -> Workspace {
    let organization = kestrel.declare_organization("acme").await;
    kestrel
        .declare_project(
            &organization,
            repository::NAME,
            &[repository::url().to_owned()],
            repository::BRANCH,
        )
        .await;
    kestrel
        .declare_agent(&organization, "builder", support::HARNESS, None)
        .await;
    kestrel
        .hold_provider_credential(
            &organization,
            support::PROVIDER_KEY,
            support::A_PROVIDER_KEY,
        )
        .await;

    kestrel
        .open_workspace("acme", repository::NAME, "builder")
        .await
}

/// Posted once the Workspace's last Session has let go, so the post starts a Session of its own.
async fn a_session_after(kestrel: &Kestrel, workspace: &Workspace, message: &str) -> Session {
    let deadline = tokio::time::Instant::now() + PATIENCE;
    loop {
        if kestrel
            .sessions(workspace.id)
            .await
            .iter()
            .all(|session| session.state == SessionState::Ended)
        {
            return kestrel.post(workspace.id, "operator", message).await;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the workspace's last session never ended"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// Answered and then stopped, which succeeds a Session waiting between Turns.
async fn answered_and_stopped(kestrel: &Kestrel, session: &Session) -> Session {
    kestrel.answered(session.id, 1).await;
    kestrel.stop_session(session.id).await;

    kestrel.session(session.id).await
}

fn supervisor_of(session: &Session) -> &str {
    session.supervisor.as_deref().expect("a supervisor")
}

#[tokio::test]
async fn two_sessions_in_one_workspace_run_through_the_same_supervisor_on_the_same_instance() {
    let kestrel = dispatching().await;
    let workspace = a_workspace(&kestrel).await;

    let first = kestrel
        .post(workspace.id, "operator", "the first thing")
        .await;
    let first = answered_and_stopped(&kestrel, &first).await;
    let second = a_session_after(&kestrel, &workspace, "the second thing").await;
    let second = answered_and_stopped(&kestrel, &second).await;

    assert_eq!(first.exit, Some(Exit::Succeeded));
    assert_eq!(second.exit, Some(Exit::Succeeded));
    assert!(first.instance.is_some());
    assert_eq!(first.instance, second.instance);
    assert_eq!(supervisor_of(&first), supervisor_of(&second));

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_session_stopped_mid_turn_leaves_its_supervisor_on_the_link() {
    let kestrel = Kestrel::dispatching_to(
        supervisor::binary(),
        &scripted_agent::playing(Script::Dawdles),
    )
    .await;
    let workspace = a_workspace(&kestrel).await;
    let session = kestrel
        .post(workspace.id, "operator", "take your time")
        .await;
    let deadline = tokio::time::Instant::now() + PATIENCE;
    let working = loop {
        let session = kestrel.session(session.id).await;
        if session.started_at.is_some() && session.supervisor.is_some() {
            break session;
        }
        assert!(tokio::time::Instant::now() < deadline, "never started");
        tokio::time::sleep(Duration::from_millis(20)).await;
    };

    kestrel.stop_session(session.id).await;

    assert!(matches!(
        kestrel.session(session.id).await.exit,
        Some(Exit::Failed { .. })
    ));
    assert!(
        Environment::named(supervisor_of(&working))
            .is_running(LONG_ENOUGH_TO_BE_SURE)
            .await,
        "the supervisor went with the session it was stopped mid-turn"
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn releasing_an_instance_stops_its_supervisor() {
    let kestrel = dispatching().await;
    let workspace = a_workspace(&kestrel).await;
    let session = kestrel.post(workspace.id, "operator", "do the work").await;
    let ended = answered_and_stopped(&kestrel, &session).await;
    let running = Environment::named(supervisor_of(&ended));
    assert!(running.is_running(LONG_ENOUGH_TO_BE_SURE).await);

    kestrel.release_instance(workspace.id).await;

    running.is_gone().await;
    kestrel.teardown().await;
}

#[tokio::test]
async fn archiving_an_instance_as_its_workspace_seals_stops_its_supervisor() {
    let kestrel = dispatching().await;
    let workspace = a_workspace(&kestrel).await;
    let session = kestrel.post(workspace.id, "operator", "do the work").await;
    let ended = answered_and_stopped(&kestrel, &session).await;
    let running = Environment::named(supervisor_of(&ended));
    assert!(running.is_running(LONG_ENOUGH_TO_BE_SURE).await);

    kestrel.seal_workspace(workspace.id).await;

    running.is_gone().await;
    kestrel.teardown().await;
}

#[cfg(unix)]
#[tokio::test]
async fn after_a_restart_the_supervisor_redials_and_the_next_session_starts_over_its_link() {
    let kestrel = dispatching().await;
    let workspace = a_workspace(&kestrel).await;
    let first = kestrel
        .post(workspace.id, "operator", "before the restart")
        .await;
    let first = kestrel.answered(first.id, 1).await;
    let running = Environment::named(supervisor_of(&first));
    // Hold the supervisor so the ended Session's stop is still unread when it reconnects.
    #[allow(unsafe_code)]
    unsafe {
        assert_eq!(libc::kill(running.pid(), libc::SIGSTOP), 0);
    }
    kestrel.stop_session(first.id).await;

    let kestrel = kestrel.kill_and_restart().await;
    #[allow(unsafe_code)]
    unsafe {
        assert_eq!(libc::kill(running.pid(), libc::SIGCONT), 0);
    }
    let second = a_session_after(&kestrel, &workspace, "after the restart").await;
    let second = answered_and_stopped(&kestrel, &second).await;

    assert_eq!(second.exit, Some(Exit::Succeeded));
    assert_eq!(first.instance, second.instance);
    assert_eq!(supervisor_of(&first), supervisor_of(&second));

    kestrel.teardown().await;
}

#[cfg(unix)]
#[tokio::test]
async fn a_supervisor_killed_between_sessions_is_replaced_before_the_next_one_starts() {
    let kestrel = dispatching().await;
    let workspace = a_workspace(&kestrel).await;
    let first = kestrel
        .post(workspace.id, "operator", "the first thing")
        .await;
    let first = answered_and_stopped(&kestrel, &first).await;
    let killed = Environment::named(supervisor_of(&first));
    #[allow(unsafe_code)]
    unsafe {
        libc::kill(killed.pid(), libc::SIGKILL);
    }
    killed.is_gone().await;

    let second = a_session_after(&kestrel, &workspace, "the second thing").await;
    let second = answered_and_stopped(&kestrel, &second).await;

    assert_eq!(second.exit, Some(Exit::Succeeded));
    assert_eq!(first.instance, second.instance);
    assert_ne!(supervisor_of(&first), supervisor_of(&second));

    kestrel.teardown().await;
}
