//! A stepped control plane runs its work role only when the test asks (ADR-0059), so each test
//! here puts what the work role races on one side of a pass or the other on purpose.

use crate::support;

use std::time::Duration;

use jiff::{SignedDuration, Timestamp};
use kestrel::domain::{Direction, Exit, Session, SessionState, Workspace, WorkspaceState};
use kestrel::log::Entry;
use kestrel::work::{Report, Reported};
use reqwest::StatusCode;
use support::environment::Environment;
use support::fixture::Fixture;
use support::github_stub::{self, GithubStub};
use support::link_client::Link;
use support::scripted_agent::{self, Script};
use support::{Kestrel, supervisor};

const PATIENCE: Duration = Duration::from_secs(30);
const REPOSITORY: &str = "jtmthf/kestrel";
const ISSUE: i64 = 43;
const READY: &str = "ready-for-agent";
const COMMENTS: &str = "/issues/43/comments";

async fn dispatching() -> Kestrel {
    dispatching_playing(Script::ReportsThenWaits).await
}

async fn dispatching_playing(script: Script) -> Kestrel {
    Kestrel::stepped_dispatching_to(supervisor::binary(), &scripted_agent::playing(script)).await
}

/// Declared before the Integration, since an Event recorded before its Trigger fires nothing.
async fn watching(kestrel: &Kestrel, stub: &GithubStub) {
    Fixture::acme()
        .without_repositories()
        .declare(kestrel)
        .await;
    kestrel
        .declare_trigger(
            "acme",
            "ready",
            &support::labelled_on(REPOSITORY, READY),
            "kestrel",
            "builder",
        )
        .await;
    kestrel
        .register_integration(
            "acme",
            "github",
            REPOSITORY,
            &stub.base_url(),
            &[Direction::Inbound, Direction::Outbound],
            SignedDuration::from_millis(1),
        )
        .await;
}

/// The poll round a pass would run, so a test can act between it and the rounds after it.
async fn polled(kestrel: &Kestrel) {
    kestrel
        .poll_as(&kestrel.integration("acme", "github").await)
        .await;
}

fn posted(stub: &GithubStub) -> usize {
    stub.requests()
        .iter()
        .filter(|request| request.method == "POST" && request.url.contains(COMMENTS))
        .count()
}

async fn until_state(kestrel: &Kestrel, session: &Session, state: SessionState) -> Session {
    let deadline = tokio::time::Instant::now() + PATIENCE;
    loop {
        let shown = kestrel.session(session.id).await;
        if shown.state == state {
            return shown;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the session {} is {}, never {state}",
            shown.id,
            shown.state
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
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
    kestrel.lapse_unswept().await;
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
    kestrel.away_for(Duration::from_secs(25 * 60 * 60));

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

#[tokio::test]
async fn an_integration_disabled_before_the_pass_is_not_polled_and_is_polled_once_enabled() {
    let stub = GithubStub::start();
    stub.deliver(github_stub::labelled(ISSUE, READY));
    let kestrel = Kestrel::stepped().await;
    watching(&kestrel, &stub).await;

    kestrel.disable_integration("acme", "github").await;
    kestrel.work_pass().await;
    assert!(kestrel.events("acme").await.is_empty());

    kestrel.enable_integration("acme", "github").await;
    kestrel.work_pass().await;
    assert_eq!(kestrel.events("acme").await.len(), 1);
    assert_eq!(
        kestrel.workspaces("acme").await.len(),
        1,
        "a pass fires for what its own poll recorded"
    );
    kestrel.teardown().await;
}

#[tokio::test]
async fn a_trigger_disabled_between_the_poll_and_the_firing_fires_for_nothing() {
    let stub = GithubStub::start();
    stub.deliver(github_stub::labelled(ISSUE, READY));
    let kestrel = Kestrel::stepped().await;
    watching(&kestrel, &stub).await;
    kestrel.work_pass().await;
    assert_eq!(kestrel.workspaces("acme").await.len(), 1);

    stub.deliver(github_stub::labelled(ISSUE + 1, READY));
    polled(&kestrel).await;
    kestrel.disable_trigger("acme", "ready").await;
    kestrel.work_pass().await;

    assert_eq!(kestrel.events("acme").await.len(), 2);
    assert_eq!(
        kestrel.workspaces("acme").await.len(),
        1,
        "a disabled trigger fired for an event recorded while it was enabled"
    );
    kestrel.teardown().await;
}

#[tokio::test]
async fn a_comment_feeds_its_workspace_unless_the_workspace_seals_before_the_pass_that_receives_it()
{
    let stub = GithubStub::start();
    stub.deliver(github_stub::labelled(ISSUE, READY));
    let kestrel = Kestrel::stepped().await;
    watching(&kestrel, &stub).await;
    kestrel.work_pass().await;
    let workspace = kestrel.workspaces("acme").await.remove(0);
    let first = kestrel.sessions(workspace.id).await.remove(0);
    kestrel.stop_session(first.id).await;

    stub.deliver(github_stub::issue_comment(
        11,
        ISSUE,
        "jack",
        "please add the missing test",
    ));
    polled(&kestrel).await;
    kestrel.work_pass().await;
    let fed = kestrel.sessions(workspace.id).await;
    assert_eq!(
        fed.len(),
        2,
        "the comment was not fed to its open workspace"
    );
    for session in &fed {
        if session.state != SessionState::Ended {
            kestrel.stop_session(session.id).await;
        }
    }

    stub.deliver(github_stub::issue_comment(
        12,
        ISSUE,
        "jack",
        "and the docs",
    ));
    polled(&kestrel).await;
    kestrel.seal_workspace(workspace.id).await;
    kestrel.work_pass().await;
    assert_eq!(
        kestrel.sessions(workspace.id).await.len(),
        2,
        "a comment fed a workspace sealed before the pass that received it"
    );
    kestrel.teardown().await;
}

#[tokio::test]
async fn what_a_session_said_waits_out_a_disabled_integration_and_posts_once_it_is_enabled() {
    let stub = GithubStub::start();
    stub.script_answer("POST", COMMENTS, github_stub::created(1, "posted"));
    stub.deliver(github_stub::labelled(ISSUE, READY));
    let kestrel = Kestrel::stepped().await;
    watching(&kestrel, &stub).await;
    kestrel.work_pass().await;
    let session = kestrel.claim_session().await.expect("the fired session");
    kestrel.complete_session(&session).await;

    kestrel.disable_integration("acme", "github").await;
    kestrel.work_pass().await;
    assert_eq!(posted(&stub), 0);

    kestrel.enable_integration("acme", "github").await;
    kestrel.work_pass().await;
    assert_eq!(posted(&stub), 1);
    kestrel.teardown().await;
}

#[tokio::test]
async fn a_read_before_the_retention_pass_sees_an_old_thought_and_one_after_sees_it_expired() {
    let kestrel = Kestrel::stepped().await;
    let workspace = a_workspace(&kestrel).await;
    let session = kestrel.dispatch_session(workspace.id).await;
    kestrel.on_the_link(&session).await;
    kestrel.start(&session, support::harness()).await;
    kestrel
        .report(
            &session,
            1,
            Report::Thought {
                text: "an old thought".to_owned(),
                completion: kestrel::log::Completion::at(Timestamp::now()),
            },
        )
        .await;
    kestrel.stop_session(session.id).await;
    kestrel.away_for(Duration::from_secs(31 * 24 * 60 * 60));
    let thought = |entries: &[Entry]| {
        entries
            .iter()
            .any(|entry| matches!(entry, Entry::Thought { text, .. } if text == "an old thought"))
    };

    assert!(thought(&kestrel.every_entry(workspace.id).await));
    kestrel.work_pass().await;

    let after = kestrel.every_entry(workspace.id).await;
    assert!(!thought(&after), "{after:?}");
    assert!(
        after
            .iter()
            .any(|entry| matches!(entry, Entry::Expired { .. })),
        "{after:?}"
    );
    kestrel.teardown().await;
}

#[tokio::test]
async fn a_session_claimed_in_the_pass_that_archives_its_workspaces_instance_starts_on_a_fresh_one()
{
    let kestrel = dispatching().await;
    let workspace = a_workspace(&kestrel).await;
    let first = kestrel.enqueue_session(workspace.id).await;
    kestrel.work_pass().await;
    let first = until_state(&kestrel, &first, SessionState::Working).await;
    let released = first.instance.clone().expect("an instance");
    kestrel.stop_session(first.id).await;
    kestrel.release_instance(workspace.id).await;

    let second = kestrel.enqueue_session(workspace.id).await;
    kestrel.work_pass().await;

    let second = kestrel.session(second.id).await;
    assert!(second.instance.is_some());
    assert_ne!(second.instance.as_deref(), Some(released.as_str()));
    assert!(
        !Environment::root_of(&released).exists(),
        "the released instance {released} was not archived"
    );
    kestrel.teardown().await;
}

#[cfg(unix)]
#[tokio::test]
async fn a_session_stopped_before_the_pass_that_finds_its_supervisor_gone_stays_stopped() {
    let kestrel = dispatching().await;
    let stopped = a_workspace(&kestrel).await;
    let failed = another_workspace(&kestrel).await;
    let stopped = kestrel.enqueue_session(stopped.id).await;
    let failed = kestrel.enqueue_session(failed.id).await;
    kestrel.work_pass().await;
    kestrel.work_pass().await;
    for session in [&stopped, &failed] {
        let working = until_state(&kestrel, session, SessionState::Working).await;
        let killed = Environment::named(working.supervisor.as_deref().expect("a supervisor"));
        #[allow(unsafe_code)]
        unsafe {
            libc::kill(killed.pid(), libc::SIGKILL);
        }
        killed.is_gone().await;
    }

    kestrel.stop_session(stopped.id).await;
    kestrel.work_pass().await;

    let lost = |session: &Session| matches!(&session.exit, Some(Exit::Failed { because }) if because.contains("supervisor exited"));
    let stopped = kestrel.session(stopped.id).await;
    assert_eq!(stopped.state, SessionState::Ended);
    assert!(!lost(&stopped), "{stopped:?}");
    let failed = kestrel.session(failed.id).await;
    assert!(lost(&failed), "{failed:?}");
    kestrel.teardown().await;
}

#[tokio::test]
async fn a_held_message_withdrawn_before_the_pass_prompts_nothing_and_one_kept_prompts_the_next_turn()
 {
    let kestrel = dispatching_playing(Script::Speaks).await;
    let workspace = a_workspace(&kestrel).await;
    let session = kestrel.enqueue_session(workspace.id).await;
    kestrel.work_pass().await;
    kestrel.answered(session.id, 1).await;
    until_state(&kestrel, &session, SessionState::Waiting).await;

    let withdrawn = kestrel
        .posted_while_busy(workspace.id, "alice", "never mind")
        .await
        .held_message
        .expect("a message to a waiting session is held for its next turn");
    kestrel
        .withdraw_message(workspace.id, withdrawn.id, "alice")
        .await
        .expect("its author should withdraw it");
    kestrel.work_pass().await;
    assert_eq!(kestrel.turns(session.id).await.len(), 1);

    let kept = kestrel
        .posted_while_busy(workspace.id, "alice", "the next thing")
        .await
        .held_message
        .expect("a message to a waiting session is held for its next turn");
    kestrel.work_pass().await;
    assert_eq!(kestrel.turns(session.id).await.len(), 2);
    assert!(
        kestrel
            .withdraw_message(workspace.id, kept.id, "alice")
            .await
            .is_err(),
        "a message the pass prompted was still withdrawable"
    );
    kestrel.teardown().await;
}
