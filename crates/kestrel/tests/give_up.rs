//! A supervisor carries an Environment through a control plane that comes back. One that is gone
//! for good has let the Session's lease go, so past that the supervisor stops rather than
//! reconnecting forever, and the control plane refuses a link held for a lease it has already
//! let lapse.

mod support;

use std::time::Duration;

use jiff::{SignedDuration, Timestamp};
use kestrel::domain::{Exit, SessionId, SessionState, Workspace};
use kestrel::work::{Report, Reported};
use reqwest::StatusCode;
use support::environment::Environment;
use support::link_client::Link;
use support::scripted_agent::Script;
use support::supervisor::Supervisor;
use support::{Kestrel, repository};

const PATIENCE: Duration = Duration::from_secs(30);

/// A lease short enough to watch lapse without waiting a real two minutes out.
const LEASE: Duration = Duration::from_secs(1);

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

    kestrel.open_workspace("acme", "kestrel", "builder").await
}

async fn until(
    kestrel: &Kestrel,
    session: SessionId,
    what: &str,
    ready: impl Fn(&kestrel::domain::Session) -> bool,
) -> kestrel::domain::Session {
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

/// The Harness writes its own pid before it starts, so what the supervisor stops can be watched
/// from outside it.
#[cfg(unix)]
fn dawdling_harness() -> Environment {
    Environment::executing(&format!(
        "echo $$ > \"$(dirname \"$0\")/harness.pid\"\nexec {}",
        support::scripted_agent::playing(Script::Dawdles)
    ))
}

#[cfg(unix)]
#[tokio::test]
async fn a_supervisor_whose_control_plane_is_gone_for_good_gives_up_and_stops_its_harness() {
    let kestrel = Kestrel::boot().await;
    let workspace = a_workspace(&kestrel).await;
    let (session, credential) = kestrel.dispatch_session(workspace.id).await;

    let harness = dawdling_harness();
    let mut supervisor = Supervisor::provision_running(
        &kestrel.link(),
        session.id,
        &credential,
        harness.path().to_str().expect("a utf-8 path"),
        "",
        Some(LEASE),
    );
    supervisor.wait_until_it_says("reported connected").await;
    kestrel.start(&session).await;

    // Past a start that only says the Session began, once the Harness has actually started.
    let deadline = tokio::time::Instant::now() + PATIENCE;
    while !harness.has_written("harness.pid") {
        assert!(
            tokio::time::Instant::now() < deadline,
            "the harness never started; the supervisor said:\n{}",
            supervisor.everything_it_said()
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    // The process goes away and stays away: nothing answers the link again.
    kestrel.kill().await;

    let exited = supervisor.exits().await;
    assert!(
        !exited.success(),
        "the supervisor exited {exited} instead of giving up; it said:\n{}",
        supervisor.everything_it_said()
    );
    assert!(
        supervisor.everything_it_said().contains("gave up"),
        "the supervisor did not say it gave up; it said:\n{}",
        supervisor.everything_it_said()
    );
    assert!(
        supervisor.everything_it_said().contains("lease"),
        "the supervisor gave up for a reason other than the lapsed lease; it said:\n{}",
        supervisor.everything_it_said()
    );

    Environment::process(&harness.wrote("harness.pid"))
        .is_gone()
        .await;
}

#[tokio::test]
async fn a_link_held_for_a_lease_that_has_passed_is_refused() {
    let kestrel = Kestrel::boot_serving_alone().await;
    let workspace = a_workspace(&kestrel).await;
    let (session, credential) = kestrel.dispatch_session(workspace.id).await;

    kestrel
        .lease_until(&session, Timestamp::now() - SignedDuration::from_secs(1))
        .await;

    let refused = Link::to(&kestrel.link())
        .report(
            session.id,
            Some(&credential),
            &Reported {
                seq: None,
                report: Report::Heartbeat,
            },
        )
        .await;

    assert_eq!(
        refused.status(),
        StatusCode::FORBIDDEN,
        "the link did not refuse a report for a lapsed lease"
    );
    let said = refused.text().await.expect("a refusal says why");
    assert!(
        said.contains("lease"),
        "the refusal is not about the lease: {said}"
    );

    let kestrel = kestrel.kill_and_restart().await;
    let ended = until(&kestrel, session.id, "was swept", |session| {
        session.state == SessionState::Ended
    })
    .await;
    assert!(
        matches!(ended.exit, Some(Exit::Failed { .. })),
        "a session the lease sweep ended did not fail: {:?}",
        ended.exit
    );

    kestrel.teardown().await;
}
