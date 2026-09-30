//! A Session from enqueued to ended, driven through the primary test seam: the work role claims
//! it, a local-exec Environment executes it, and it ends with an exit status.

mod support;

use std::path::{Path, PathBuf};
use std::time::Duration;

use kestrel::domain::{Exit, Session, SessionId, SessionState, Workspace};
use kestrel::log::Entry;
use kestrel::work::{Report, Reported};
use support::Kestrel;
use support::environment::Environment;
use support::link_client::Link;
use support::repository;
use support::scripted_agent::{self, Script};
use support::supervisor;

const PATIENCE: Duration = Duration::from_secs(30);

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

/// Answering a turn never ends a Session, so one that answered is stopped, the way a person would.
async fn ended(kestrel: &Kestrel, session: SessionId) -> Session {
    kestrel.after_one_turn(session).await
}

#[cfg(unix)]
#[tokio::test]
async fn a_panicking_test_stops_its_local_supervisor_and_removes_its_instance() {
    let (started, received) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(async move {
        let environment = Environment::executing("sleep 300");
        let kestrel = Kestrel::dispatching_to(environment.path(), "unused").await;
        let workspace = a_workspace(&kestrel).await;
        let session = kestrel.enqueue_session(workspace.id).await;
        let session = until(&kestrel, session.id, "reached a supervisor", |session| {
            session.supervisor.is_some()
        })
        .await;
        started
            .send((session.supervisor.unwrap(), session.instance.unwrap()))
            .unwrap();
        panic!("the test fails after starting a Session");
    });

    let (supervisor, instance) = received.await.unwrap();
    assert!(task.await.unwrap_err().is_panic());
    Environment::named(&supervisor).is_gone().await;
    assert!(!Environment::root_of(&instance).exists());
}

#[cfg(unix)]
#[tokio::test]
async fn a_killed_control_plane_without_a_restart_stops_its_local_supervisor() {
    let environment = Environment::executing("sleep 300");
    let kestrel = Kestrel::dispatching_to(environment.path(), "unused").await;
    let workspace = a_workspace(&kestrel).await;
    let session = kestrel.enqueue_session(workspace.id).await;
    let session = until(&kestrel, session.id, "reached a supervisor", |session| {
        session.supervisor.is_some()
    })
    .await;
    let supervisor = session.supervisor.unwrap();
    let instance = session.instance.unwrap();

    drop(kestrel.kill().await);

    Environment::named(&supervisor).is_gone().await;
    assert!(!Environment::root_of(&instance).exists());
}

#[tokio::test]
async fn sessions_in_distinct_workspaces_start_at_the_same_time() {
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

    let second = until(&kestrel, second.id, "started", |session| {
        session.started_at.is_some()
    })
    .await;

    assert_eq!(kestrel.session(first.id).await.state, SessionState::Working);
    assert_eq!(second.state, SessionState::Working);

    kestrel.teardown().await;
}

#[tokio::test]
async fn the_active_session_limit_queues_excess_work_and_releases_it_as_sessions_end() {
    let kestrel = Kestrel::dispatching_up_to(
        supervisor::binary(),
        &scripted_agent::playing(Script::Dawdles),
        1,
    )
    .await;
    let first_workspace = a_workspace(&kestrel).await;
    let second_workspace = kestrel.open_workspace("acme", "kestrel", "builder").await;
    let third_workspace = kestrel.open_workspace("acme", "kestrel", "builder").await;
    let first = kestrel.enqueue_session(first_workspace.id).await;
    let second = kestrel.enqueue_session(second_workspace.id).await;
    let third = kestrel.enqueue_session(third_workspace.id).await;

    let first = until(&kestrel, first.id, "started", |session| {
        session.started_at.is_some()
    })
    .await;
    assert_eq!(kestrel.session(second.id).await.state, SessionState::Queued);
    assert_eq!(kestrel.session(third.id).await.state, SessionState::Queued);

    kestrel.complete_session(&first).await;
    let second = until(&kestrel, second.id, "started", |session| {
        session.started_at.is_some()
    })
    .await;
    assert_eq!(kestrel.session(third.id).await.state, SessionState::Queued);

    kestrel.complete_session(&second).await;
    let third = until(&kestrel, third.id, "started", |session| {
        session.started_at.is_some()
    })
    .await;
    assert_eq!(third.state, SessionState::Working);

    kestrel.teardown().await;
}

/// The supervisor outlives a control plane that goes away (ADR-0002), and a Session in flight is
/// left to it and its lease rather than failed on the way out.
#[cfg(unix)]
#[tokio::test]
async fn a_control_plane_that_stops_leaves_each_session_in_flight_to_its_supervisor() {
    let environment = Environment::executing("sleep 300");
    let kestrel = Kestrel::dispatching_up_to(environment.path(), "unused", 2).await;
    let first_workspace = a_workspace(&kestrel).await;
    let second_workspace = kestrel.open_workspace("acme", "kestrel", "builder").await;
    let first = kestrel.enqueue_session(first_workspace.id).await;
    let second = kestrel.enqueue_session(second_workspace.id).await;
    let first = until(&kestrel, first.id, "reached a supervisor", |session| {
        session.supervisor.is_some()
    })
    .await;
    let second = until(&kestrel, second.id, "reached a supervisor", |session| {
        session.supervisor.is_some()
    })
    .await;

    let stopped = kestrel.kill().await;

    for session in [first, second] {
        assert_eq!(
            stopped.session(session.id).await.state,
            SessionState::Working
        );
        assert!(
            Environment::named(session.supervisor.as_deref().expect("a supervisor"))
                .is_running(Duration::from_millis(200))
                .await,
            "the supervisor went with the control plane"
        );
    }
}

#[tokio::test]
async fn a_session_enqueued_is_claimed_dispatched_and_reaches_an_instance() {
    let kestrel = Kestrel::dispatching(supervisor::binary()).await;
    let workspace = a_workspace(&kestrel).await;

    let session = kestrel.enqueue_session(workspace.id).await;
    assert_eq!(session.state, SessionState::Queued);
    let ended = ended(&kestrel, session.id).await;

    assert!(
        ended.connected.is_some(),
        "the session ended without a supervisor ever reaching the link"
    );
    assert!(ended.instance.is_some());
    assert_eq!(ended.exit, Some(Exit::Succeeded));

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_session_that_reaches_an_instance_starts_and_ends_in_the_transcript() {
    let kestrel = Kestrel::dispatching(supervisor::binary()).await;
    let workspace = a_workspace(&kestrel).await;

    let session = kestrel.enqueue_session(workspace.id).await;
    ended(&kestrel, session.id).await;

    let said: Vec<String> = kestrel
        .transcript(workspace.id)
        .await
        .iter()
        .map(|entry| entry.entry.to_string())
        .collect();

    assert_eq!(
        said,
        vec![
            "participant joined  builder".to_owned(),
            "said  operator  do the work this environment was provisioned for".to_owned(),
            format!("session started  {}  builder", session.id),
            "said  builder  half of one message, and the other half".to_owned(),
            "said  builder  a second message".to_owned(),
            format!("session ended  {}  succeeded", session.id),
        ]
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_finished_sessions_supervisor_stays_with_the_instance_kept_for_the_workspace() {
    let kestrel = Kestrel::dispatching(supervisor::binary()).await;
    let workspace = a_workspace(&kestrel).await;

    let session = kestrel.enqueue_session(workspace.id).await;
    let ended = ended(&kestrel, session.id).await;

    assert!(
        Environment::named(ended.supervisor.as_deref().expect("a supervisor"))
            .is_running(Duration::from_millis(500))
            .await,
        "the supervisor went with the session"
    );
    let instance = ended.instance.expect("an instance");
    assert_eq!(
        kestrel.instance(workspace.id).await.as_ref(),
        Some(&instance)
    );
    assert!(
        Environment::root_of(&instance).is_dir(),
        "the instance went with the session"
    );

    kestrel.teardown().await;
}

/// Stands in for the Harness, so what it finds is what the supervisor checked out
/// before spawning one, and then hands over to the agent it stands in for.
#[cfg(unix)]
fn noting_the_checkout() -> Environment {
    Environment::executing(&format!(
        "echo \"$(git -C kestrel branch --show-current) $(cat kestrel/README.md)\" \
           >> \"$(dirname \"$0\")/found\"\n\
         exec {}",
        scripted_agent::playing(Script::Speaks)
    ))
}

#[cfg(unix)]
async fn noted(harness: &Environment, maximum: usize) -> Kestrel {
    Kestrel::dispatching_up_to(
        supervisor::binary(),
        &format!("\"{}\"", harness.path().display()),
        maximum,
    )
    .await
}

#[cfg(unix)]
#[tokio::test]
async fn a_workspace_declares_a_branch_of_its_own_and_the_session_starts_on_it() {
    let harness = noting_the_checkout();
    let kestrel = noted(&harness, 1).await;
    let workspace = a_workspace(&kestrel).await;

    let session = kestrel.enqueue_session(workspace.id).await;
    let ended = ended(&kestrel, session.id).await;

    assert_eq!(ended.exit, Some(Exit::Succeeded));
    assert_eq!(
        workspace.checkout.branch,
        format!("kestrel/{}", workspace.id)
    );
    assert_eq!(workspace.checkout.base, repository::BRANCH);
    assert_eq!(
        harness.wrote("found"),
        format!("{} a project's repository", workspace.checkout.branch),
        "the agent did not start on its workspace's branch"
    );

    kestrel.teardown().await;
}

#[cfg(unix)]
#[tokio::test]
async fn parallel_workspaces_work_on_distinct_branches() {
    let harness = noting_the_checkout();
    let kestrel = noted(&harness, 2).await;
    let first = a_workspace(&kestrel).await;
    let second = kestrel.open_workspace("acme", "kestrel", "builder").await;

    let sessions = [
        kestrel.enqueue_session(first.id).await,
        kestrel.enqueue_session(second.id).await,
    ];
    for session in sessions {
        ended(&kestrel, session.id).await;
    }

    assert_ne!(first.checkout.branch, second.checkout.branch);
    let mut found: Vec<String> = harness.wrote("found").lines().map(str::to_owned).collect();
    found.sort();
    let mut expected = vec![
        format!("{} a project's repository", first.checkout.branch),
        format!("{} a project's repository", second.checkout.branch),
    ];
    expected.sort();
    assert_eq!(found, expected);

    kestrel.teardown().await;
}

#[cfg(unix)]
#[tokio::test]
async fn a_workspace_on_a_branch_its_operator_named_starts_on_that_branchs_work() {
    let harness = noting_the_checkout();
    let kestrel = noted(&harness, 1).await;
    a_workspace(&kestrel).await;
    let workspace = kestrel
        .open_workspace_on("acme", "kestrel", "builder", repository::EXISTING_BRANCH)
        .await;

    let session = kestrel.enqueue_session(workspace.id).await;
    ended(&kestrel, session.id).await;

    assert_eq!(
        harness.wrote("found"),
        format!("{} an existing branch's work", repository::EXISTING_BRANCH)
    );

    kestrel.teardown().await;
}

#[cfg(unix)]
#[tokio::test]
async fn a_branch_the_remote_does_not_have_is_cut_from_the_projects() {
    let harness = noting_the_checkout();
    let kestrel = noted(&harness, 1).await;
    a_workspace(&kestrel).await;
    let workspace = kestrel
        .open_workspace_on("acme", "kestrel", "builder", "kestrel/issue-43")
        .await;

    let session = kestrel.enqueue_session(workspace.id).await;
    ended(&kestrel, session.id).await;

    assert_eq!(
        harness.wrote("found"),
        "kestrel/issue-43 a project's repository"
    );

    kestrel.teardown().await;
}

async fn located(kestrel: &Kestrel, workspace: &Workspace) -> Vec<PathBuf> {
    kestrel
        .transcript(workspace.id)
        .await
        .into_iter()
        .filter_map(|recorded| match recorded.entry {
            Entry::Said {
                participant,
                message,
                ..
            } if participant == "builder" => Some(PathBuf::from(message)),
            _ => None,
        })
        .collect()
}

fn checkout_of(instance: &str, repository: &str) -> PathBuf {
    Environment::root_of(instance)
        .canonicalize()
        .expect("the instance's directory should exist")
        .join(repository)
}

#[tokio::test]
async fn a_sessions_agent_is_rooted_in_the_checkout_of_its_workspaces_repository() {
    let kestrel = Kestrel::dispatching_to(
        supervisor::binary(),
        &scripted_agent::playing(Script::Locates),
    )
    .await;
    let workspace = a_workspace(&kestrel).await;

    let session = kestrel.enqueue_session(workspace.id).await;
    let ended = ended(&kestrel, session.id).await;

    let instance = ended.instance.expect("an instance");
    assert_eq!(
        located(&kestrel, &workspace).await,
        [checkout_of(&instance, repository::NAME)]
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_sessions_agent_is_rooted_in_the_first_repository_its_workspace_declares() {
    let kestrel = Kestrel::dispatching_to(
        supervisor::binary(),
        &scripted_agent::playing(Script::Locates),
    )
    .await;
    a_workspace(&kestrel).await;
    let organization = kestrel.declare_organization("acme").await;
    kestrel
        .declare_project(
            &organization,
            "both",
            &[
                repository::other_url().to_owned(),
                repository::url().to_owned(),
            ],
            repository::BRANCH,
        )
        .await;
    let workspace = kestrel.open_workspace("acme", "both", "builder").await;

    let session = kestrel.enqueue_session(workspace.id).await;
    let ended = ended(&kestrel, session.id).await;

    let instance = ended.instance.expect("an instance");
    assert_eq!(
        located(&kestrel, &workspace).await,
        [checkout_of(&instance, repository::OTHER)]
    );
    assert!(
        checkout_of(&instance, repository::NAME)
            .join(".git")
            .is_dir(),
        "the workspace's other repository is not checked out beside the first"
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn redeclaring_the_project_does_not_move_where_an_open_workspaces_agent_is_rooted() {
    let kestrel = Kestrel::dispatching_to(
        supervisor::binary(),
        &scripted_agent::playing(Script::Locates),
    )
    .await;
    let workspace = a_workspace(&kestrel).await;
    let organization = kestrel.declare_organization("acme").await;
    kestrel
        .declare_project(
            &organization,
            repository::NAME,
            &[
                repository::other_url().to_owned(),
                repository::url().to_owned(),
            ],
            repository::BRANCH,
        )
        .await;

    let session = kestrel.enqueue_session(workspace.id).await;
    let ended = ended(&kestrel, session.id).await;

    let instance = ended.instance.expect("an instance");
    assert_eq!(
        located(&kestrel, &workspace).await,
        [checkout_of(&instance, repository::NAME)]
    );

    kestrel.teardown().await;
}

/// Stands in for the Harness. On a checkout it has not been on before it leaves work only
/// this Instance has, and a process behind it; on one it has, it notes what it finds.
#[cfg(unix)]
fn leaving_work_behind() -> Environment {
    Environment::executing(&format!(
        "here=\"$(dirname \"$0\")\"\n\
         if [ -f kestrel/untracked ]; then\n\
           {{ git -C kestrel log -1 --format=%s; git -C kestrel status --porcelain; \
              cat kestrel/untracked; }} >> \"$here/found\"\n\
         else\n\
           echo fresh >> \"$here/found\"\n\
           {{ echo committed > kestrel/committed\n\
             git -C kestrel add committed\n\
             git -C kestrel commit --message 'work only this instance has'\n\
             echo uncommitted >> kestrel/README.md\n\
             echo untracked > kestrel/untracked; }} >&2\n\
           sleep 300 </dev/null >/dev/null 2>&1 &\n\
           echo $! > \"$here/lingering\"\n\
         fi\n\
         exec {}",
        scripted_agent::playing(Script::Speaks)
    ))
}

async fn enqueued_once_free(kestrel: &Kestrel, workspace: &Workspace) -> Session {
    let deadline = tokio::time::Instant::now() + PATIENCE;

    loop {
        match kestrel.try_enqueue_session(workspace.id).await {
            Ok(session) => return session,
            Err(error) => assert!(
                tokio::time::Instant::now() < deadline,
                "the workspace never took another session: {error}"
            ),
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[cfg(unix)]
#[tokio::test]
async fn a_later_session_finds_the_checkout_exactly_as_the_session_before_it_left_it() {
    let harness = leaving_work_behind();
    let kestrel = noted(&harness, 1).await;
    let workspace = a_workspace(&kestrel).await;

    let first = kestrel.enqueue_session(workspace.id).await;
    let first = ended(&kestrel, first.id).await;
    let second = enqueued_once_free(&kestrel, &workspace).await;
    let second = ended(&kestrel, second.id).await;

    assert_eq!(first.exit, Some(Exit::Succeeded));
    assert_eq!(second.exit, Some(Exit::Succeeded));
    assert_eq!(first.instance, second.instance);
    assert_eq!(
        harness.wrote("found"),
        "fresh\n\
         work only this instance has\n \
         M README.md\n\
         ?? untracked\n\
         untracked"
    );

    kestrel.teardown().await;
}

#[cfg(unix)]
#[tokio::test]
async fn a_workspaces_sessions_share_one_supervisor_and_leave_no_harness_process_to_the_next() {
    let harness = leaving_work_behind();
    let kestrel = noted(&harness, 1).await;
    let workspace = a_workspace(&kestrel).await;

    let first = kestrel.enqueue_session(workspace.id).await;
    let first = ended(&kestrel, first.id).await;
    Environment::process(&harness.wrote("lingering"))
        .is_gone()
        .await;

    let second = enqueued_once_free(&kestrel, &workspace).await;
    let second = ended(&kestrel, second.id).await;

    assert_eq!(first.instance, second.instance);
    assert!(first.supervisor.is_some());
    assert_eq!(first.supervisor, second.supervisor);

    kestrel.teardown().await;
}

#[cfg(unix)]
#[tokio::test]
async fn a_session_whose_instance_is_gone_fails_saying_so_and_the_next_starts_from_the_remote() {
    let harness = leaving_work_behind();
    let kestrel = noted(&harness, 1).await;
    let workspace = a_workspace(&kestrel).await;

    let first = kestrel.enqueue_session(workspace.id).await;
    let first = ended(&kestrel, first.id).await;
    let lost = first.instance.clone().expect("an instance");
    std::fs::remove_dir_all(Environment::root_of(&lost)).expect("the instance should go");

    let second = enqueued_once_free(&kestrel, &workspace).await;
    let second = ended(&kestrel, second.id).await;
    let Some(Exit::Failed { because }) = &second.exit else {
        panic!(
            "the session ended {:?}, and its instance was gone",
            second.exit
        );
    };
    assert!(
        because.contains(&lost)
            && because.contains("never pushed")
            && because.contains(&workspace.checkout.branch),
        "the failure does not say what was lost: {because}"
    );
    assert_eq!(second.started_at, None);
    assert_eq!(kestrel.instance(workspace.id).await, None);
    // Forgotten with its Instance, so the link refuses it and it leaves.
    Environment::named(first.supervisor.as_deref().expect("a supervisor"))
        .is_gone()
        .await;

    let third = enqueued_once_free(&kestrel, &workspace).await;
    let third = ended(&kestrel, third.id).await;
    assert_eq!(third.exit, Some(Exit::Succeeded));
    assert_ne!(third.instance.as_ref(), Some(&lost));
    assert_eq!(
        harness.wrote("found"),
        "fresh\nfresh",
        "the session after a lost instance found work that was lost with it"
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_checkout_that_fails_names_the_repository_and_branch_and_the_session_never_starts() {
    let kestrel = Kestrel::dispatching(supervisor::binary()).await;
    let organization = kestrel.declare_organization("acme").await;
    kestrel
        .declare_project(
            &organization,
            repository::NAME,
            &[repository::url().to_owned()],
            "a-branch-nobody-cut",
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

    let workspace = kestrel.open_workspace("acme", "kestrel", "builder").await;

    let session = kestrel.enqueue_session(workspace.id).await;
    let ended = ended(&kestrel, session.id).await;

    let Some(Exit::Failed { because }) = &ended.exit else {
        panic!(
            "the session ended {:?}, and its project names a branch that is not there",
            ended.exit
        );
    };
    assert!(
        because.contains(repository::url()) && because.contains(&workspace.checkout.branch),
        "the failure names neither the repository nor the branch: {because}"
    );
    assert_eq!(
        ended.started_at, None,
        "a session that was never checked out started"
    );

    kestrel.teardown().await;
}

#[cfg(unix)]
#[tokio::test]
async fn a_supervisor_that_ends_without_saying_how_the_session_went_leaves_it_failed() {
    let environment = Environment::executing("exit 3");
    let kestrel = Kestrel::dispatching(environment.path()).await;
    let workspace = a_workspace(&kestrel).await;

    let session = kestrel.enqueue_session(workspace.id).await;
    let ended = ended(&kestrel, session.id).await;

    let Some(Exit::Failed { because }) = &ended.exit else {
        panic!(
            "the session ended {:?}, and its supervisor reported nothing",
            ended.exit
        );
    };
    assert!(
        because.contains("without reporting how the session went"),
        "unhelpful exit status: {because}"
    );
    Environment::named(ended.supervisor.as_deref().expect("a supervisor"))
        .is_gone()
        .await;

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_supervisor_that_reports_its_session_failed_ends_it_failed() {
    let kestrel = Kestrel::boot().await;
    let workspace = a_workspace(&kestrel).await;
    let (session, on) = kestrel.dispatch_to_the_link(workspace.id).await;

    Link::to(&kestrel.link())
        .report(
            &on.instance,
            Some(&on.credential),
            &Reported {
                session: Some(session.id),
                seq: Some(1),
                report: Report::Finished {
                    exit: Exit::Failed {
                        because: "the agent could not open a pull request".to_owned(),
                    },
                },
            },
        )
        .await;

    let ended = ended(&kestrel, session.id).await;
    assert_eq!(
        ended.exit,
        Some(Exit::Failed {
            because: "the agent could not open a pull request".to_owned()
        })
    );
    assert_eq!(
        kestrel
            .transcript(workspace.id)
            .await
            .last()
            .expect("a transcript entry")
            .entry
            .to_string(),
        format!(
            "session ended  {}  failed: the agent could not open a pull request",
            session.id
        )
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_session_whose_supervisor_cannot_be_started_ends_rather_than_staying_queued() {
    let kestrel = Kestrel::dispatching(Path::new("/nowhere/kestrel-supervisor")).await;
    let workspace = a_workspace(&kestrel).await;

    let session = kestrel.enqueue_session(workspace.id).await;
    let ended = ended(&kestrel, session.id).await;

    let Some(Exit::Failed { because }) = &ended.exit else {
        panic!(
            "the session ended {:?}, and nothing provisioned it",
            ended.exit
        );
    };
    assert!(
        because.contains("could not be started"),
        "unhelpful exit status: {because}"
    );
    assert!(ended.supervisor.is_none());

    kestrel.teardown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_queued_session_is_claimed_once_however_many_claimants_ask_at_once() {
    let kestrel = Kestrel::boot().await;
    let workspace = a_workspace(&kestrel).await;
    let session = kestrel.enqueue_session(workspace.id).await;

    let (first, second) = tokio::join!(kestrel.claim_session(), kestrel.claim_session());

    let claimed: Vec<SessionId> = [first, second]
        .into_iter()
        .flatten()
        .map(|claimed| claimed.id)
        .collect();
    assert_eq!(claimed, vec![session.id]);

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_session_being_executed_is_never_claimed_again() {
    let kestrel = Kestrel::boot().await;
    let workspace = a_workspace(&kestrel).await;
    let session = kestrel.dispatch_session(workspace.id).await;

    assert_eq!(
        kestrel.session(session.id).await.state,
        SessionState::Working
    );
    assert!(
        kestrel.claim_session().await.is_none(),
        "a session already being executed was handed out to be dispatched again"
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_session_that_ended_is_never_claimed_again() {
    let kestrel = Kestrel::boot().await;
    let workspace = a_workspace(&kestrel).await;
    let session = kestrel.dispatch_session(workspace.id).await;

    kestrel.complete_session(&session).await;

    assert!(
        kestrel.claim_session().await.is_none(),
        "a session that already ended was handed out to be dispatched again"
    );

    kestrel.teardown().await;
}
