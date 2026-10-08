//! The session-held lease and the sweep that reaps it: a Session holds one from the moment it is
//! claimed, its Environment holds it out for as long as it is alive, and a lease nothing
//! holds out ends its Session failed rather than leaving a Workspace wedged.

use crate::support;

use std::time::Duration;

use jiff::{SignedDuration, Timestamp};
use kestrel::domain::{Exit, Session, SessionId, SessionState, Workspace};
use support::environment::Environment;
use support::fixture::Fixture;
use support::scripted_agent::Script;
use support::{Kestrel, scripted_agent, supervisor};

const PATIENCE: Duration = Duration::from_secs(30);

async fn a_workspace(kestrel: &Kestrel) -> Workspace {
    Fixture::acme()
        .checked_out()
        .model(kestrel_scripted_agent::OTHER_MODEL)
        .holding_a_provider_key()
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

async fn swept(kestrel: &Kestrel, session: SessionId) -> String {
    let ended = until(kestrel, session, "ended", |session| {
        session.state == SessionState::Ended
    })
    .await;

    let Some(Exit::Failed { because }) = ended.exit else {
        panic!(
            "the session ended {:?}, and nothing was holding its lease out",
            ended.exit
        );
    };
    assert!(
        because.contains("lease"),
        "a session failed by its lease says so: {because}"
    );
    assert!(
        ended.lease_expires_at.is_none(),
        "a session that has ended still holds a lease"
    );

    because
}

fn a_moment_ago() -> Timestamp {
    Timestamp::now() - SignedDuration::from_secs(1)
}

/// A lease due sooner than a real one, and further off than an Environment that is alive lets
/// one get.
fn shortened() -> Timestamp {
    Timestamp::now() + SignedDuration::from_secs(4)
}

#[tokio::test]
async fn a_session_holds_a_lease_from_the_moment_it_is_claimed() {
    let kestrel = Kestrel::boot().await;
    let workspace = a_workspace(&kestrel).await;

    let queued = kestrel.enqueue_session(workspace.id).await;
    assert!(queued.lease_expires_at.is_none());

    let claimed = kestrel.claim_session().await.expect("a session to claim");
    assert_eq!(claimed.id, queued.id);
    assert!(
        claimed.lease_expires_at > Some(Timestamp::now()),
        "a claimed session holds no lease"
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_lease_nothing_holds_out_ends_its_session_failed() {
    let kestrel = Kestrel::boot().await;
    let workspace = a_workspace(&kestrel).await;
    let session = kestrel.dispatch_session(workspace.id).await;

    kestrel.lease_until(&session, a_moment_ago()).await;

    let because = swept(&kestrel, session.id).await;
    assert_eq!(
        kestrel
            .transcript(workspace.id)
            .await
            .last()
            .expect("a transcript entry")
            .entry
            .to_string(),
        format!("session ended  {}  failed: {because}", session.id)
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_lease_that_expires_leaves_its_workspace_no_active_session() {
    let kestrel = Kestrel::boot().await;
    let workspace = a_workspace(&kestrel).await;
    let session = kestrel.dispatch_session(workspace.id).await;

    kestrel.lease_until(&session, a_moment_ago()).await;
    swept(&kestrel, session.id).await;

    assert!(
        kestrel
            .sessions(workspace.id)
            .await
            .iter()
            .all(|session| session.state != SessionState::Working),
        "a workspace whose session's lease expired still has an active session"
    );
    let next = kestrel.enqueue_session(workspace.id).await;
    assert_eq!(
        kestrel.claim_session().await.map(|claimed| claimed.id),
        Some(next.id),
        "the session after the one that expired was not dispatched"
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn one_parallel_sessions_expired_lease_leaves_the_other_session_active() {
    let kestrel = Kestrel::dispatching_up_to(
        supervisor::binary(),
        &scripted_agent::playing(Script::Dawdles),
        2,
    )
    .await;
    let first_workspace = a_workspace(&kestrel).await;
    let second_workspace = kestrel.open_workspace("acme", "kestrel", "builder").await;
    let first = kestrel.enqueue_session(first_workspace.id).await;
    let second = kestrel.enqueue_session(second_workspace.id).await;
    let first = until(&kestrel, first.id, "started", |session| {
        session.started_at.is_some()
    })
    .await;
    until(&kestrel, second.id, "started", |session| {
        session.started_at.is_some()
    })
    .await;

    kestrel.lease_until(&first, a_moment_ago()).await;
    swept(&kestrel, first.id).await;

    assert_eq!(
        kestrel.session(second.id).await.state,
        SessionState::Working
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_session_failed_by_lease_expiry_is_never_dispatched_again() {
    let kestrel = Kestrel::boot().await;
    let workspace = a_workspace(&kestrel).await;
    let session = kestrel.dispatch_session(workspace.id).await;

    kestrel.lease_until(&session, a_moment_ago()).await;
    swept(&kestrel, session.id).await;

    assert!(
        kestrel.claim_session().await.is_none(),
        "a session failed by its lease expiring was handed out to be dispatched again"
    );
    assert_eq!(kestrel.session(session.id).await.state, SessionState::Ended);

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_due_time_survives_a_control_plane_restart_and_fires_after_it() {
    let kestrel = Kestrel::boot().await;
    let workspace = a_workspace(&kestrel).await;
    let session = kestrel.dispatch_session(workspace.id).await;

    let stopped = kestrel.kill().await;
    stopped.lease_until(&session, a_moment_ago()).await;
    let kestrel = stopped.restart().await;

    swept(&kestrel, session.id).await;

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_supervisor_holds_its_sessions_lease_out_for_the_life_of_the_session() {
    let kestrel = Kestrel::dispatching_to(
        supervisor::binary(),
        &scripted_agent::playing(Script::Dawdles),
    )
    .await;
    let workspace = a_workspace(&kestrel).await;
    let session = kestrel.enqueue_session(workspace.id).await;

    let working = until(&kestrel, session.id, "started", |session| {
        session.started_at.is_some()
    })
    .await;
    let shortened = shortened();
    kestrel.lease_until(&working, shortened).await;

    let held = until(&kestrel, session.id, "had its lease held out", |session| {
        session.lease_expires_at > Some(shortened)
    })
    .await;
    assert_eq!(held.state, SessionState::Working);

    tokio::time::sleep(Duration::from_secs(5)).await;
    assert_eq!(
        kestrel.session(session.id).await.state,
        SessionState::Working,
        "a session whose supervisor is alive was swept anyway"
    );

    kestrel.teardown().await;
}

#[cfg(unix)]
#[tokio::test]
async fn a_supervisor_that_dies_mid_session_stops_holding_the_lease_out_and_the_session_ends_failed()
 {
    // The script outlives the supervisor it started, so what ends this Session is the lease rather
    // than the work role noticing a supervisor that is gone.
    let environment = Environment::executing(&format!(
        "\"{}\" &\nsupervisor=$!\nsleep 3\nkill -9 $supervisor\nsleep 60",
        supervisor::binary().display()
    ));
    let kestrel = Kestrel::dispatching_to(
        environment.path(),
        &scripted_agent::playing(Script::Dawdles),
    )
    .await;
    let workspace = a_workspace(&kestrel).await;
    let session = kestrel.enqueue_session(workspace.id).await;

    let working = until(&kestrel, session.id, "started", |session| {
        session.started_at.is_some()
    })
    .await;
    tokio::time::sleep(Duration::from_secs(4)).await;
    // The same lease the script above outlives: a supervisor still alive holds one out well
    // inside this, so what ends this Session is the supervisor being gone.
    kestrel.lease_until(&working, shortened()).await;

    swept(&kestrel, session.id).await;

    kestrel.teardown().await;
}
