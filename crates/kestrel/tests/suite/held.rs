//! An Instance is archived when its Workspace seals only if a real checkout says everything in it
//! can be recovered from the remote; one that may hold the only copy of some work is held until a
//! person releases it.

use crate::support;

use std::time::Duration;

use jiff::{SignedDuration, Timestamp};
use kestrel::domain::{Exit, Session, SessionState, Workspace, WorkspaceState};
use support::Kestrel;
use support::environment::Environment;
use support::repository;
use support::scripted_agent::{self, Script};
use support::supervisor;

const PATIENCE: Duration = Duration::from_secs(30);

const COMMIT: &str = "git -C kestrel commit --quiet";

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
        .declare_agent(
            &organization,
            "builder",
            "opencode",
            Some(kestrel_scripted_agent::OTHER_MODEL),
        )
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

/// Stands in for the Harness: does something to the checkout, then hands over to the agent.
#[cfg(unix)]
fn working(shell: &str) -> Environment {
    Environment::executing(&format!(
        "{{ {shell}; }} >&2\nexec {}",
        scripted_agent::playing(Script::Speaks)
    ))
}

#[cfg(unix)]
async fn dispatching_to(harness: &Environment) -> Kestrel {
    Kestrel::dispatching_to(
        supervisor::binary(),
        &format!("\"{}\"", harness.path().display()),
    )
    .await
}

/// Ended, so nothing but what its checkout holds keeps its Workspace. A Session that answers rather than failing waits between turns until something stops it
/// (ADR-0024), so this stops it itself once it has answered, the way a person or a seal would.
async fn over(kestrel: &Kestrel, workspace: &Workspace) -> Session {
    let session = kestrel.enqueue_session(workspace.id).await;
    let answered = kestrel.answered(session.id, 1).await;
    if answered.state != SessionState::Ended {
        kestrel.stop_session(session.id).await;
    }

    let deadline = tokio::time::Instant::now() + PATIENCE;
    loop {
        let ended = kestrel.session(session.id).await;
        if ended.state == SessionState::Ended {
            return ended;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the session {} is {} and never finished",
            session.id,
            ended.state
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn a_day_ago() -> Timestamp {
    Timestamp::now() - SignedDuration::from_hours(25)
}

async fn eventually(what: &str, done: impl AsyncFn() -> bool) {
    let deadline = tokio::time::Instant::now() + PATIENCE;

    while !done().await {
        assert!(
            tokio::time::Instant::now() < deadline,
            "{what} never happened"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

async fn archived(instance: &str) {
    let root = Environment::root_of(instance);
    eventually(&format!("archiving {instance}"), async || !root.exists()).await;
}

/// Long enough that a sweep that was going to seal this Workspace has run several times over.
async fn stays_open(kestrel: &Kestrel, workspace: &Workspace) {
    tokio::time::sleep(Duration::from_secs(2)).await;

    assert_eq!(
        kestrel.show_workspace(workspace.id).await.state,
        WorkspaceState::Open,
        "the workspace {} sealed itself with its instance holding work",
        workspace.id
    );
}

#[cfg(unix)]
#[tokio::test]
async fn clean_research_work_seals_when_idle_and_its_instance_is_archived() {
    let harness = working(
        "echo target/ >> kestrel/.git/info/exclude; mkdir -p kestrel/target; \
         echo built > kestrel/target/output",
    );
    let kestrel = dispatching_to(&harness).await;
    let workspace = a_workspace(&kestrel).await;
    let session = over(&kestrel, &workspace).await;
    assert_eq!(session.exit, Some(Exit::Succeeded));
    let instance = session.instance.expect("an instance");
    assert!(kestrel.held_instances("acme").await.is_empty());

    kestrel.last_active(&workspace, a_day_ago()).await;

    let workspace_id = workspace.id;
    eventually("the idle sweep sealing the workspace", async || {
        kestrel.show_workspace(workspace_id).await.state == WorkspaceState::Sealed
    })
    .await;
    archived(&instance).await;
    assert_eq!(kestrel.instance(workspace.id).await, None);

    kestrel.teardown().await;
}

#[cfg(unix)]
#[tokio::test]
async fn a_pushed_checkout_is_archived_when_its_workspace_seals() {
    let harness = working(&format!(
        "echo committed > kestrel/committed; git -C kestrel add committed; \
         {COMMIT} --message 'pushed work'; git -C kestrel push --quiet origin HEAD"
    ));
    let kestrel = dispatching_to(&harness).await;
    let workspace = a_workspace(&kestrel).await;
    let session = over(&kestrel, &workspace).await;
    assert_eq!(session.exit, Some(Exit::Succeeded));

    kestrel.seal_workspace(workspace.id).await;

    archived(&session.instance.expect("an instance")).await;

    kestrel.teardown().await;
}

#[cfg(unix)]
#[tokio::test]
async fn unpublished_work_outlasts_the_idle_window_held_with_a_reason_until_released() {
    let harness = working(&format!(
        "echo committed > kestrel/committed; git -C kestrel add committed; \
         {COMMIT} --message 'work only this instance has'; \
         echo uncommitted >> kestrel/README.md; echo untracked > kestrel/untracked"
    ));
    let kestrel = dispatching_to(&harness).await;
    let workspace = a_workspace(&kestrel).await;
    let session = over(&kestrel, &workspace).await;
    assert_eq!(session.exit, Some(Exit::Succeeded));
    let instance = session.instance.expect("an instance");

    kestrel.last_active(&workspace, a_day_ago()).await;
    stays_open(&kestrel, &workspace).await;

    let held = kestrel.held_instances("acme").await;
    assert_eq!(held.len(), 1);
    assert_eq!(held[0].workspace, workspace.id);
    assert_eq!(held[0].instance, instance);
    assert_eq!(
        held[0].because,
        format!(
            "{} on {} has 1 unpushed commit, 1 uncommitted change, 1 untracked file",
            repository::url(),
            workspace.checkout.branch
        )
    );
    let refused = kestrel
        .try_seal_workspace(workspace.id)
        .await
        .expect_err("a workspace whose instance holds the only copy of its work sealed");
    assert!(
        refused.to_string().contains(&held[0].because),
        "the refusal does not say what is held: {refused}"
    );
    assert!(Environment::root_of(&instance).is_dir());

    assert_eq!(kestrel.release_instance(workspace.id).await, instance);

    archived(&instance).await;
    assert!(kestrel.held_instances("acme").await.is_empty());
    assert_eq!(
        kestrel
            .transcript(workspace.id)
            .await
            .last()
            .expect("a transcript entry")
            .entry
            .to_string(),
        format!(
            "instance released  operator  {instance}  discarding {}",
            held[0].because
        )
    );
    let workspace_id = workspace.id;
    eventually(
        "the idle sweep sealing the released workspace",
        async || kestrel.show_workspace(workspace_id).await.state == WorkspaceState::Sealed,
    )
    .await;

    kestrel.teardown().await;
}

#[cfg(unix)]
#[tokio::test]
async fn a_session_that_fails_without_reporting_its_checkout_holds_its_instance() {
    let environment = Environment::executing("exit 3");
    let kestrel = Kestrel::dispatching(environment.path()).await;
    let workspace = a_workspace(&kestrel).await;
    let session = over(&kestrel, &workspace).await;
    assert!(matches!(session.exit, Some(Exit::Failed { .. })));
    let instance = session.instance.expect("an instance");

    kestrel.last_active(&workspace, a_day_ago()).await;
    stays_open(&kestrel, &workspace).await;

    let held = kestrel.held_instances("acme").await;
    assert_eq!(held.len(), 1);
    assert_eq!(held[0].instance, instance);
    assert_eq!(
        held[0].because,
        "no session reported what its checkout holds"
    );
    kestrel
        .try_seal_workspace(workspace.id)
        .await
        .expect_err("a workspace whose instance nobody reported on sealed");
    assert!(Environment::root_of(&instance).is_dir());

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_workspace_with_no_instance_has_nothing_to_release() {
    let kestrel = Kestrel::boot().await;
    let workspace = a_workspace(&kestrel).await;

    let refused = kestrel
        .try_release_instance(workspace.id)
        .await
        .expect_err("a workspace that never ran released an instance");

    assert!(refused.to_string().contains("no instance"), "{refused}");

    kestrel.teardown().await;
}

#[cfg(unix)]
#[tokio::test]
async fn a_waiting_session_ends_when_its_clean_workspace_seals_idle() {
    let harness = working("true");
    let kestrel = dispatching_to(&harness).await;
    let workspace = a_workspace(&kestrel).await;
    let session = kestrel.enqueue_session(workspace.id).await;
    let waiting = kestrel.answered(session.id, 1).await;
    assert_eq!(waiting.state, SessionState::Waiting);

    kestrel.last_active(&workspace, a_day_ago()).await;

    let workspace_id = workspace.id;
    eventually("the idle sweep sealing the workspace", async || {
        kestrel.show_workspace(workspace_id).await.state == WorkspaceState::Sealed
    })
    .await;
    assert_eq!(
        kestrel.session(session.id).await.exit,
        Some(Exit::Succeeded)
    );
    archived(&waiting.instance.expect("an instance")).await;

    kestrel.teardown().await;
}

#[cfg(unix)]
#[tokio::test]
async fn a_waiting_session_over_unpublished_work_outlasts_the_idle_window() {
    let harness = working("echo untracked > kestrel/untracked");
    let kestrel = dispatching_to(&harness).await;
    let workspace = a_workspace(&kestrel).await;
    let session = kestrel.enqueue_session(workspace.id).await;
    let waiting = kestrel.answered(session.id, 1).await;

    kestrel.last_active(&workspace, a_day_ago()).await;
    stays_open(&kestrel, &workspace).await;

    assert_eq!(
        kestrel.session(session.id).await.state,
        SessionState::Waiting
    );
    let held = kestrel.held_instances("acme").await;
    assert_eq!(held.len(), 1);
    assert_eq!(held[0].instance, waiting.instance.expect("an instance"));
    assert!(
        held[0].because.contains("1 untracked file"),
        "{}",
        held[0].because
    );

    kestrel.stop_session(session.id).await;
    kestrel.teardown().await;
}

#[cfg(unix)]
#[tokio::test]
async fn releasing_an_idle_workspace_ends_its_waiting_session_succeeded() {
    let harness = working("true");
    let kestrel = dispatching_to(&harness).await;
    let workspace = a_workspace(&kestrel).await;
    let session = kestrel.enqueue_session(workspace.id).await;
    let waiting = kestrel.answered(session.id, 1).await;
    assert_eq!(waiting.state, SessionState::Waiting);
    let instance = waiting.instance.expect("an instance");

    assert_eq!(kestrel.release_instance(workspace.id).await, instance);

    eventually(
        "the released workspace's waiting session ending succeeded",
        async || kestrel.session(session.id).await.exit == Some(Exit::Succeeded),
    )
    .await;
    assert_eq!(kestrel.instance(workspace.id).await, None);
    archived(&instance).await;

    kestrel.teardown().await;
}
