mod support;

use std::time::Duration;

use jiff::{SignedDuration, Timestamp};
use kestrel::domain::{Exit, Session, SessionState, Workspace};
use kestrel::instance::{Git, Observed};
use kestrel::scheduling::Reason;
use support::Kestrel;
use support::repository;
use support::supervisor;

const PATIENCE: Duration = Duration::from_secs(30);

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

fn clean_checkout() -> Vec<Observed> {
    vec![Observed {
        repository: repository::url().to_owned(),
        git: Git::Read {
            branch: Some(repository::BRANCH.to_owned()),
            untracked: 0,
            uncommitted: 0,
            stashes: 0,
            unpushed: 0,
        },
    }]
}

async fn workspaces(kestrel: &Kestrel, maximum: usize) -> (Workspace, Workspace, Workspace) {
    let organization = kestrel.declare_limited_organization("acme", maximum).await;
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

    (
        kestrel.open_workspace("acme", "kestrel", "builder").await,
        kestrel.open_workspace("acme", "kestrel", "builder").await,
        kestrel.open_workspace("acme", "kestrel", "builder").await,
    )
}

async fn reasons(kestrel: &Kestrel, session: &Session) -> Vec<Reason> {
    kestrel
        .queue("acme")
        .await
        .queued
        .into_iter()
        .find(|queued| queued.session.id == session.id)
        .expect("the session is queued")
        .reasons
}

async fn complete_clean_sessions(kestrel: &Kestrel, workspaces: &[(&Workspace, &str)]) {
    for (workspace, instance) in workspaces {
        let queued = kestrel.enqueue_session(workspace.id).await;
        let session = kestrel
            .occupy_session()
            .await
            .expect("the session should claim");
        assert_eq!(session.id, queued.id);
        kestrel.executes_on(&session, instance).await;
        kestrel.report_checkout(&session, clean_checkout()).await;
        kestrel.complete_session(&session).await;
    }
}

#[tokio::test]
async fn an_active_instance_counts_toward_the_organization_limit() {
    let kestrel = Kestrel::boot().await;
    let (active, waiting, _) = workspaces(&kestrel, 1).await;
    let first = kestrel.enqueue_session(active.id).await;
    let claimed = kestrel
        .occupy_session()
        .await
        .expect("the session should claim");
    assert_eq!(claimed.id, first.id);
    kestrel.executes_on(&claimed, "active").await;

    let second = kestrel.enqueue_session(waiting.id).await;
    assert!(kestrel.occupy_session().await.is_none());
    let second = kestrel.session(second.id).await;

    assert_eq!(second.state, SessionState::Queued);
    assert_eq!(
        reasons(&kestrel, &second).await,
        [Reason::LiveInstanceLimit(1)]
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn reclaiming_for_new_work_does_not_delay_a_follow_up_that_already_has_an_instance() {
    let kestrel = Kestrel::boot().await;
    let (oldest, existing, arriving) = workspaces(&kestrel, 2).await;

    complete_clean_sessions(&kestrel, &[(&oldest, "oldest"), (&existing, "existing")]).await;
    kestrel
        .last_active(&oldest, Timestamp::now() - SignedDuration::from_hours(1))
        .await;

    let new_session = kestrel.enqueue_session(arriving.id).await;
    let follow_up = kestrel.enqueue_session(existing.id).await;

    assert_eq!(
        kestrel.occupy_session().await.map(|claimed| claimed.id),
        Some(follow_up.id)
    );
    let new_session = kestrel.session(new_session.id).await;
    assert_eq!(new_session.state, SessionState::Queued);
    assert_eq!(
        reasons(&kestrel, &new_session).await,
        [Reason::InstanceArchiving("oldest".to_owned())]
    );
    assert_eq!(kestrel.instance(oldest.id).await, None);

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_held_instance_blocks_new_work_but_not_its_workspaces_follow_up() {
    let kestrel = Kestrel::boot().await;
    let (existing, new, _) = workspaces(&kestrel, 1).await;

    let first = kestrel.enqueue_session(existing.id).await;
    let claimed = kestrel
        .occupy_session()
        .await
        .expect("the session should claim");
    assert_eq!(claimed.id, first.id);
    let first = claimed;
    kestrel.executes_on(&first, "held").await;
    let mut held = clean_checkout();
    held[0].git = Git::Read {
        branch: Some(repository::BRANCH.to_owned()),
        untracked: 0,
        uncommitted: 1,
        stashes: 0,
        unpushed: 0,
    };
    kestrel.report_checkout(&first, held).await;
    kestrel.complete_session(&first).await;

    let blocked = kestrel.enqueue_session(new.id).await;
    assert!(kestrel.occupy_session().await.is_none());
    let blocked = kestrel.session(blocked.id).await;
    assert_eq!(blocked.state, SessionState::Queued);
    assert_eq!(
        reasons(&kestrel, &blocked).await,
        [Reason::LiveInstanceLimit(1)]
    );

    let follow_up = kestrel.enqueue_session(existing.id).await;
    let claimed = kestrel
        .occupy_session()
        .await
        .expect("the follow-up should claim");
    assert_eq!(claimed.id, follow_up.id);
    assert_eq!(kestrel.instance(existing.id).await.as_deref(), Some("held"));
    assert_eq!(
        kestrel.session(blocked.id).await.state,
        SessionState::Queued
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn the_longest_idle_recoverable_instance_is_archived_to_admit_new_work() {
    let kestrel = Kestrel::boot().await;
    let (oldest, newer, arriving) = workspaces(&kestrel, 2).await;

    complete_clean_sessions(&kestrel, &[(&oldest, "oldest"), (&newer, "newer")]).await;
    kestrel
        .last_active(&oldest, Timestamp::now() - SignedDuration::from_hours(2))
        .await;
    kestrel
        .last_active(&newer, Timestamp::now() - SignedDuration::from_hours(1))
        .await;

    let third = kestrel.enqueue_session(arriving.id).await;
    assert!(kestrel.occupy_session().await.is_none());

    assert_eq!(kestrel.instances_to_archive().await, ["oldest"]);
    assert_eq!(kestrel.instance(oldest.id).await, None);
    assert_eq!(kestrel.instance(newer.id).await.as_deref(), Some("newer"));

    kestrel.instance_archived("oldest").await;
    let claimed = kestrel
        .occupy_session()
        .await
        .expect("the new session should claim after archival");
    assert_eq!(claimed.id, third.id);

    kestrel.teardown().await;
}

#[cfg(unix)]
#[tokio::test]
async fn an_idle_instance_reclaimed_under_the_cap_ends_its_waiting_session_succeeded() {
    let kestrel = Kestrel::dispatching(supervisor::binary()).await;
    let (idle, arriving, _) = workspaces(&kestrel, 1).await;

    let first = kestrel.enqueue_session(idle.id).await;
    let waiting = kestrel.answered(first.id, 1).await;
    assert_eq!(waiting.state, SessionState::Waiting);
    let instance = waiting.instance.clone().expect("an instance");

    let second = kestrel.enqueue_session(arriving.id).await;

    eventually(
        "the reclaimed instance's waiting session ending succeeded",
        async || kestrel.session(first.id).await.exit == Some(Exit::Succeeded),
    )
    .await;
    eventually("the reclaimed instance being archived", async || {
        !kestrel.instances_to_archive().await.contains(&instance)
    })
    .await;
    eventually(
        "the queued session starting on a fresh instance",
        async || kestrel.session(second.id).await.started_at.is_some(),
    )
    .await;
    assert_eq!(kestrel.instance(idle.id).await, None);

    kestrel.answered(second.id, 1).await;
    let resumed = kestrel.post(idle.id, "jack", "one more thing").await;
    let resumed = kestrel.answered(resumed.id, 1).await;
    assert_ne!(resumed.id, first.id);
    assert_ne!(resumed.instance, Some(instance));
    assert_eq!(kestrel.session(first.id).await.exit, Some(Exit::Succeeded));

    kestrel.teardown().await;
}
