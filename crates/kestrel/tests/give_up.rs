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
async fn a_lease_that_passes_while_the_control_plane_is_away_ends_the_harness_and_the_supervisor_redials()
 {
    let kestrel = Kestrel::boot().await;
    let workspace = a_workspace(&kestrel).await;
    let (session, on) = kestrel.dispatch_to_the_link(workspace.id).await;

    let harness = dawdling_harness();
    let mut supervisor = Supervisor::provision_running(
        &kestrel.link(),
        &on,
        harness.path().to_str().expect("a utf-8 path"),
        "",
        Some(LEASE),
    );
    supervisor.wait_until_it_says("reported connected").await;
    kestrel.start(&session, supervisor.harness()).await;

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

    let stopped = kestrel.kill().await;
    supervisor.lets_go_of(session.id).await;
    assert!(
        supervisor.said("gave up") && supervisor.said("lease"),
        "the supervisor let the session go for a reason other than the lapsed lease; it said:\n{}",
        supervisor.everything_it_said()
    );
    Environment::process(&harness.wrote("harness.pid"))
        .is_gone()
        .await;
    assert!(
        supervisor.is_still_running(Duration::from_secs(1)).await,
        "the supervisor left with its session; it said:\n{}",
        supervisor.everything_it_said()
    );

    // Where the real lease, which the supervisor's bound outlasts, would be by now.
    stopped
        .lease_until(&session, Timestamp::now() - SignedDuration::from_secs(1))
        .await;
    let kestrel = stopped.restart().await;
    supervisor.wait_until_it_says("link open after").await;
    let ended = until(&kestrel, session.id, "was swept", |session| {
        session.state == SessionState::Ended
    })
    .await;
    assert!(matches!(ended.exit, Some(Exit::Failed { .. })));

    kestrel.teardown().await;
}

#[tokio::test]
async fn an_overdue_lease_accepts_reports_until_the_sweep_ends_its_session() {
    let kestrel = Kestrel::boot_serving_alone().await;
    let workspace = a_workspace(&kestrel).await;
    let (session, on) = kestrel.dispatch_to_the_link(workspace.id).await;
    let lapsed = Timestamp::now() - SignedDuration::from_secs(1);
    kestrel.lease_until(&session, lapsed).await;
    let link = Link::to(&kestrel.link());

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
    assert!(kestrel.session(session.id).await.lease_expires_at <= Some(Timestamp::now()));

    let accepted = link
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
    assert_eq!(accepted.status(), StatusCode::ACCEPTED);
    assert!(kestrel.session(session.id).await.started_at.is_some());

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

    let refused = Link::to(&kestrel.link())
        .report(
            &on.instance,
            Some(&on.credential),
            &Reported {
                session: Some(session.id),
                seq: Some(2),
                report: Report::Started,
            },
        )
        .await;
    assert_eq!(refused.status(), StatusCode::GONE);

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_live_session_with_an_overdue_lease_is_renewed_without_dropping_the_link() {
    let kestrel = Kestrel::boot_serving_alone().await;
    let workspace = a_workspace(&kestrel).await;
    let (session, on) = kestrel.dispatch_to_the_link(workspace.id).await;

    let mut supervisor =
        Supervisor::provision_playing(&kestrel.link(), &on, Script::ReportsThenWaits);
    supervisor.wait_until_it_says("reported connected").await;
    kestrel.start(&session, supervisor.harness()).await;
    supervisor
        .wait_until_it_says(&format!("reported usage for {}", session.id))
        .await;

    kestrel
        .lease_until(&session, Timestamp::now() - SignedDuration::from_secs(1))
        .await;
    let renewed = until(
        &kestrel,
        session.id,
        "renewed its overdue lease",
        |session| session.lease_expires_at > Some(Timestamp::now() + SignedDuration::from_secs(60)),
    )
    .await;
    assert_eq!(renewed.state, SessionState::Working);
    assert!(supervisor.is_still_running(Duration::from_secs(1)).await);
    assert!(
        !supervisor.said("lost the link"),
        "renewing an overdue lease took the link down; the supervisor said:\n{}",
        supervisor.everything_it_said()
    );

    supervisor.destroy();
    kestrel.teardown().await;
}
