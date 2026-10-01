//! A Workspace opened without a Brief is dispatched at once, without an Active-Work Slot: its
//! Instance is provisioned, the branch checked out and the harness spawned, and the Session waits
//! unbriefed until its first message (ADR-0038).

mod support;

use std::time::Duration;

use jiff::{SignedDuration, Timestamp};
use kestrel::domain::{
    Exit, Preparing, Session, SessionId, SessionState, Workspace, WorkspaceState,
};
use kestrel::log::Entry;
use kestrel::queue::Reason;
use support::repository;
use support::scripted_agent::{self, Script};
use support::supervisor;
use support::{A_PROVIDER_KEY, HARNESS, Kestrel, PROVIDER_KEY};

const PATIENCE: Duration = Duration::from_secs(30);

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

async fn sealed_by_the_sweep(kestrel: &Kestrel, workspace: Workspace) -> Workspace {
    let deadline = tokio::time::Instant::now() + PATIENCE;

    loop {
        let shown = kestrel.show_workspace(workspace.id).await;
        if shown.state == WorkspaceState::Sealed {
            return shown;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the workspace {} was last active at {} and never sealed itself",
            shown.id,
            shown.last_active_at
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
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
        .declare_agent(
            &organization,
            "builder",
            HARNESS,
            Some(kestrel_scripted_agent::OTHER_MODEL),
        )
        .await;
    kestrel
        .hold_provider_credential(&organization, PROVIDER_KEY, A_PROVIDER_KEY)
        .await;

    kestrel.open_workspace("acme", "kestrel", "builder").await
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

#[tokio::test]
async fn with_every_slot_occupied_an_unbriefed_workspace_still_reaches_harness_ready() {
    let kestrel = Kestrel::dispatching_harnesses_up_to(
        supervisor::binary(),
        &[(HARNESS, &scripted_agent::playing(Script::ReportsThenWaits))],
        1,
    )
    .await;
    let busy = a_workspace(&kestrel).await;
    let unbriefed = a_workspace(&kestrel).await;

    let working = kestrel.enqueue_session(busy.id).await;
    let working = until(
        &kestrel,
        working.id,
        "its slot's session working",
        |session| session.state == SessionState::Working,
    )
    .await;
    assert_eq!(kestrel.queue("acme").await.active_work.occupied, 1);

    let queued = kestrel
        .enqueue_session_with_nothing_posted(unbriefed.id)
        .await;
    let ready = until(
        &kestrel,
        queued.id,
        "harness ready without a slot",
        |session| session.preparing == Some(Preparing::HarnessReady),
    )
    .await;

    assert_eq!(ready.state, SessionState::Unbriefed);
    assert!(ready.instance.is_some(), "it has an Instance of its own");
    assert!(
        kestrel.turns(ready.id).await.is_empty(),
        "no Turn may be prompted before the Brief"
    );
    assert_eq!(
        kestrel.session(working.id).await.state,
        SessionState::Working,
        "the slot is still the working session's"
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn an_unbriefed_workspace_is_refused_at_the_live_instance_limit() {
    let kestrel = Kestrel::boot().await;
    let organization = kestrel.declare_limited_organization("acme", 1).await;
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
            HARNESS,
            Some(kestrel_scripted_agent::OTHER_MODEL),
        )
        .await;
    kestrel
        .hold_provider_credential(&organization, PROVIDER_KEY, A_PROVIDER_KEY)
        .await;
    let active = kestrel.open_workspace("acme", "kestrel", "builder").await;
    let waiting = kestrel.open_workspace("acme", "kestrel", "builder").await;

    let first = kestrel.enqueue_session(active.id).await;
    let claimed = kestrel
        .occupy_session()
        .await
        .expect("the first session should claim");
    assert_eq!(claimed.id, first.id);
    kestrel.executes_on(&claimed, "active").await;

    let second = kestrel
        .enqueue_session_with_nothing_posted(waiting.id)
        .await;
    assert!(
        kestrel.occupy_session().await.is_none(),
        "an unbriefed Session still cannot be admitted past the live Instance limit"
    );
    let second = kestrel.session(second.id).await;
    assert_eq!(second.state, SessionState::Queued);
    assert_eq!(
        reasons(&kestrel, &second).await,
        [Reason::LiveInstanceLimit(1)]
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn an_unbriefed_workspace_dispatches_past_a_serialized_profile() {
    let kestrel = Kestrel::boot().await;
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
        .declare_agent(&organization, "builder", support::SERIALIZED, None)
        .await;
    kestrel
        .declare_profile("acme", "jack", "Jack")
        .await
        .expect("the profile should declare");
    let holding = kestrel
        .open_workspace_with("acme", "kestrel", "builder", "jack")
        .await;
    let unbriefed = kestrel
        .open_workspace_with("acme", "kestrel", "builder", "jack")
        .await;

    let first = kestrel.enqueue_session(holding.id).await;
    let claimed = kestrel
        .occupy_session()
        .await
        .expect("the first session should claim");
    assert_eq!(claimed.id, first.id);
    kestrel.executes_on(&claimed, "profile-holder").await;

    let second = kestrel
        .enqueue_session_with_nothing_posted(unbriefed.id)
        .await;
    let claimed = kestrel
        .occupy_session()
        .await
        .expect("an unbriefed session takes no turn, so the profile does not hold it");
    assert_eq!(claimed.id, second.id);
    assert_eq!(claimed.state, SessionState::Unbriefed);

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_spawn_failure_ends_an_unbriefed_session_failed() {
    let kestrel = Kestrel::dispatching_harnesses_up_to(
        supervisor::binary(),
        &[(HARNESS, "kestrel-no-such-harness-binary")],
        1,
    )
    .await;
    let workspace = a_workspace(&kestrel).await;

    let queued = kestrel
        .enqueue_session_with_nothing_posted(workspace.id)
        .await;
    let ended = until(&kestrel, queued.id, "it ended", |session| {
        session.state == SessionState::Ended
    })
    .await;

    assert!(
        matches!(ended.exit, Some(Exit::Failed { .. })),
        "a harness that cannot spawn fails the session: {ended:?}"
    );

    kestrel.teardown().await;
}

/// The supervisor's side of the seam: it checks out, opens the ACP conversation, reports ready,
/// and prompts nothing, so no Turn and no agent message exists.
#[tokio::test]
async fn an_unbriefed_start_opens_the_conversation_reports_ready_and_prompts_nothing() {
    let kestrel = Kestrel::dispatching(supervisor::binary()).await;
    let workspace = a_workspace(&kestrel).await;

    let queued = kestrel
        .enqueue_session_with_nothing_posted(workspace.id)
        .await;
    let ready = until(&kestrel, queued.id, "harness ready", |session| {
        session.preparing == Some(Preparing::HarnessReady)
    })
    .await;

    assert!(kestrel.turns(ready.id).await.is_empty());
    let transcript = kestrel.transcript(workspace.id).await;
    assert!(
        transcript
            .iter()
            .all(|recorded| !matches!(recorded.entry, Entry::SessionStarted { .. })),
        "an unbriefed Session never starts a turn: {transcript:?}"
    );
    assert!(
        transcript
            .iter()
            .all(|recorded| !matches!(&recorded.entry, Entry::Said { participant, .. } if participant == "builder")),
        "the scripted agent says nothing until it is prompted: {transcript:?}"
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn an_idle_unbriefed_workspace_seals_after_the_window() {
    let kestrel = Kestrel::dispatching(supervisor::binary()).await;
    let workspace = a_workspace(&kestrel).await;

    let queued = kestrel
        .enqueue_session_with_nothing_posted(workspace.id)
        .await;
    let ready = until(&kestrel, queued.id, "harness ready", |session| {
        session.preparing == Some(Preparing::HarnessReady)
    })
    .await;
    assert!(ready.instance.is_some());

    kestrel
        .last_active(
            &workspace,
            Timestamp::now() - SignedDuration::from_hours(25),
        )
        .await;
    let sealed = sealed_by_the_sweep(&kestrel, workspace).await;

    assert_eq!(sealed.state, WorkspaceState::Sealed);
    assert_eq!(
        kestrel.session(ready.id).await.state,
        SessionState::Ended,
        "sealing let the unbriefed Session go with its Instance"
    );

    kestrel.teardown().await;
}
