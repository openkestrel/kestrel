//! A firing that correlates to an open Workspace does what its Trigger declares: continues the
//! waiting Session, or starts a new Session with the Trigger's Agent once the unfinished one lets
//! go (ADR-0031).

use crate::support;

use std::time::Duration;

use jiff::SignedDuration;
use kestrel::domain::{
    Direction, Exit, OnOpenWorkspace, Session, SessionState, Workspace, WorkspaceId,
};
use kestrel::log::{BriefSource, Entry};
use kestrel::trigger::Would;
use kestrel_scripted_agent::conversed;
use support::github_stub::{self, GithubStub};
use support::scripted_agent::{self, Script};
use support::{Kestrel, labelled_on, repository, supervisor, templates};

const PATIENCE: Duration = Duration::from_secs(30);
const REPOSITORY: &str = "jtmthf/kestrel";
const ISSUE: i64 = 43;
const READY: &str = "ready-for-agent";
const CI_FAILED: &str = "ci-failed";
const CORRELATION: &str = "{{ event.source }}{{ event.subject }}";
const CI_BRIEF: &str = "Fix the build of {{ event.data.issue.title }}";
const BOTH: &[Direction] = &[Direction::Inbound, Direction::Outbound];

async fn an_organization(kestrel: &Kestrel) {
    let organization = kestrel.declare_organization("acme").await;
    kestrel
        .declare_project(
            &organization,
            "kestrel",
            &[repository::url().to_owned()],
            repository::BRANCH,
        )
        .await;
    for (agent, harness) in [
        ("builder", support::HARNESS),
        ("fixer", support::HARNESS),
        ("codex", "codex"),
    ] {
        kestrel
            .declare_agent(&organization, agent, harness, None)
            .await;
    }
    kestrel
        .hold_provider_credential(
            &organization,
            support::PROVIDER_KEY,
            support::A_PROVIDER_KEY,
        )
        .await;
}

/// `ready` opens the Workspace for the issue with `builder`; `ci` correlates to it and does what
/// `on_open_workspace` declares with `fixer`, or with `codex` when a label chooses it.
async fn correlated(kestrel: &Kestrel, on_open_workspace: OnOpenWorkspace) {
    kestrel
        .declare_correlated_trigger(
            "acme",
            "ready",
            &labelled_on(REPOSITORY, READY),
            "builder",
            &[],
            &templates(support::BRIEF, None, Some(CORRELATION)),
            OnOpenWorkspace::Continue,
        )
        .await;
    kestrel
        .declare_correlated_trigger(
            "acme",
            "ci",
            &labelled_on(REPOSITORY, CI_FAILED),
            "fixer",
            &["codex"],
            &templates(CI_BRIEF, None, Some(CORRELATION)),
            on_open_workspace,
        )
        .await;
}

async fn watching(kestrel: &Kestrel, stub: &GithubStub) {
    kestrel
        .register_integration(
            "acme",
            "github",
            REPOSITORY,
            &stub.base_url(),
            BOTH,
            SignedDuration::from_millis(1),
        )
        .await;
}

async fn opened(kestrel: &Kestrel) -> Workspace {
    let deadline = tokio::time::Instant::now() + PATIENCE;
    loop {
        if let Some(workspace) = kestrel.workspaces("acme").await.into_iter().next() {
            return workspace;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "no workspace was ever opened"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

async fn fed(kestrel: &Kestrel, count: usize) {
    let deadline = tokio::time::Instant::now() + PATIENCE;
    loop {
        let mut fed = 0;
        for event in kestrel.events("acme").await {
            fed += kestrel
                .firings(event.record_id)
                .await
                .iter()
                .filter(|firing| firing.outcome == "fed")
                .count();
        }
        if fed >= count {
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "{count} firings never fed the workspace"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

async fn sessions(kestrel: &Kestrel, workspace: WorkspaceId, count: usize) -> Vec<Session> {
    let deadline = tokio::time::Instant::now() + PATIENCE;
    loop {
        let sessions = kestrel.sessions(workspace).await;
        if sessions.len() >= count {
            return sessions;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "{count} sessions were never enqueued, only {}",
            sessions.len()
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

async fn briefs(kestrel: &Kestrel, workspace: WorkspaceId) -> Vec<(BriefSource, String)> {
    kestrel
        .transcript(workspace)
        .await
        .into_iter()
        .filter_map(|recorded| match recorded.entry {
            Entry::Brief { source, brief } => Some((source, brief)),
            _ => None,
        })
        .collect()
}

/// The CI failure on the issue the Workspace was opened for, carrying `labels` besides.
fn ci_failed(stub: &GithubStub, labels: &[&str]) {
    stub.deliver(github_stub::labelled_carrying(ISSUE, CI_FAILED, labels));
}

fn ci_brief() -> (BriefSource, String) {
    (
        BriefSource::Trigger {
            trigger: "ci".to_owned(),
        },
        format!("Fix the build of an issue numbered {ISSUE}"),
    )
}

/// The scripted agent remembers only what its own process was prompted with, so a second answer
/// naming the first prompt is the same conversation going on.
#[tokio::test]
async fn a_command_on_a_continuing_trigger_is_the_waiting_sessions_next_turn() {
    let kestrel = Kestrel::dispatching_to(
        supervisor::binary(),
        &scripted_agent::playing(Script::Converses),
    )
    .await;
    an_organization(&kestrel).await;
    kestrel
        .declare_correlated_trigger(
            "acme",
            "ready",
            &labelled_on(REPOSITORY, READY),
            "builder",
            &[],
            &templates(support::BRIEF, None, Some(CORRELATION)),
            OnOpenWorkspace::Continue,
        )
        .await;
    kestrel
        .declare_correlated_trigger(
            "acme",
            "asked",
            &serde_json::json!({"all": [
                {"exact": {"type": "com.github.issue_comment.created"}},
                {"prefix": {"data.comment.body": "@kestrel"}},
            ]})
            .to_string(),
            "fixer",
            &[],
            &templates("{{ instruction }}", None, Some(CORRELATION)),
            OnOpenWorkspace::Continue,
        )
        .await;
    let stub = GithubStub::start();
    stub.deliver(github_stub::labelled(ISSUE, READY));
    watching(&kestrel, &stub).await;
    let workspace = opened(&kestrel).await;
    let session = sessions(&kestrel, workspace.id, 1).await.remove(0);
    let waiting = kestrel.answered(session.id, 1).await;
    assert_eq!(waiting.state, SessionState::Waiting);

    stub.deliver(github_stub::issue_comment(
        11,
        ISSUE,
        "jack",
        "@kestrel also update the docs",
    ));
    let answered = kestrel.answered(session.id, 2).await;

    assert_eq!(answered.state, SessionState::Waiting);
    assert_eq!(kestrel.sessions(workspace.id).await.len(), 1);
    assert_eq!(answered.agent.name, "builder");
    let said: Vec<String> = kestrel
        .transcript(workspace.id)
        .await
        .into_iter()
        .filter_map(|recorded| match recorded.entry {
            Entry::Said {
                participant,
                message,
                ..
            } if participant == "builder" => Some(message),
            _ => None,
        })
        .collect();
    let [first, second] = said.as_slice() else {
        panic!("the agent answered other than twice: {said:?}");
    };
    assert_eq!(first, &conversed(1, &[]));
    assert!(
        second.starts_with("turn 2, after: ") && second.contains("an issue numbered 43"),
        "the second answer does not remember the first prompt: {second}"
    );

    kestrel.stop_session(session.id).await;
    kestrel.teardown().await;
}

#[tokio::test]
async fn a_new_session_trigger_starts_a_session_with_its_agent_and_brief() {
    let kestrel = Kestrel::boot().await;
    an_organization(&kestrel).await;
    correlated(&kestrel, OnOpenWorkspace::NewSession).await;
    let stub = GithubStub::start();
    stub.deliver(github_stub::labelled(ISSUE, READY));
    watching(&kestrel, &stub).await;
    let workspace = opened(&kestrel).await;
    let first = kestrel
        .claim_session()
        .await
        .expect("the opening firing enqueued a session");
    kestrel.complete_session(&first).await;

    ci_failed(&stub, &[]);
    let started = sessions(&kestrel, workspace.id, 2).await;

    assert_eq!(kestrel.workspaces("acme").await.len(), 1);
    assert_eq!(started[0].agent.name, "builder");
    assert_eq!(started[1].agent.name, "fixer");
    assert_eq!(started[1].state, SessionState::Queued);
    let transcript = kestrel.transcript(workspace.id).await;
    let ended = transcript
        .iter()
        .position(|recorded| matches!(recorded.entry, Entry::SessionEnded { .. }))
        .expect("the first session's end is recorded");
    assert_eq!(
        transcript[ended + 1].entry,
        Entry::Brief {
            source: ci_brief().0,
            brief: ci_brief().1,
        },
        "the new session's brief is not its first entry"
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_new_session_firing_waits_for_the_unfinished_session_to_let_go() {
    let kestrel = Kestrel::boot().await;
    an_organization(&kestrel).await;
    correlated(&kestrel, OnOpenWorkspace::NewSession).await;
    let stub = GithubStub::start();
    stub.deliver(github_stub::labelled(ISSUE, READY));
    watching(&kestrel, &stub).await;
    let workspace = opened(&kestrel).await;
    let first = kestrel
        .claim_session()
        .await
        .expect("the opening firing enqueued a session");

    ci_failed(&stub, &[]);
    fed(&kestrel, 1).await;
    assert_eq!(kestrel.sessions(workspace.id).await.len(), 1);
    assert!(!briefs(&kestrel, workspace.id).await.contains(&ci_brief()));
    let refused = kestrel
        .try_seal_workspace(workspace.id)
        .await
        .expect_err("a workspace with a session in flight should not seal");
    assert!(refused.to_string().contains("in flight"), "{refused}");

    kestrel.complete_session(&first).await;

    let started = kestrel.sessions(workspace.id).await;
    assert_eq!(started.len(), 2);
    assert_eq!(started[1].agent.name, "fixer");
    assert_eq!(started[1].state, SessionState::Queued);
    assert!(briefs(&kestrel, workspace.id).await.contains(&ci_brief()));

    kestrel.teardown().await;
}

/// A waiting Session has answered everything asked of it, and may wait indefinitely, so a new
/// Session ends it rather than wait on it.
#[tokio::test]
async fn a_new_session_firing_ends_the_session_waiting_between_its_turns() {
    let kestrel = Kestrel::dispatching_to(
        supervisor::binary(),
        &scripted_agent::playing(Script::Converses),
    )
    .await;
    an_organization(&kestrel).await;
    correlated(&kestrel, OnOpenWorkspace::NewSession).await;
    let stub = GithubStub::start();
    stub.deliver(github_stub::labelled(ISSUE, READY));
    watching(&kestrel, &stub).await;
    let workspace = opened(&kestrel).await;
    let first = sessions(&kestrel, workspace.id, 1).await.remove(0);
    assert_eq!(
        kestrel.answered(first.id, 1).await.state,
        SessionState::Waiting
    );

    ci_failed(&stub, &[]);
    let second = sessions(&kestrel, workspace.id, 2).await.remove(1);
    let second = kestrel.answered(second.id, 1).await;

    let first = kestrel.session(first.id).await;
    assert_eq!(first.state, SessionState::Ended);
    assert_eq!(first.exit, Some(Exit::Succeeded), "{:?}", first.exit);
    assert_eq!(second.agent.name, "fixer");
    assert_eq!(second.state, SessionState::Waiting);
    assert_eq!(second.instance, first.instance);
    assert!(briefs(&kestrel, workspace.id).await.contains(&ci_brief()));

    kestrel.stop_session(second.id).await;
    kestrel.teardown().await;
}

/// Messages that arrived before a new Session drain before it; one that arrived after waits for it.
#[tokio::test]
async fn a_workspace_drains_messages_and_new_sessions_in_the_order_they_arrived() {
    let kestrel = Kestrel::boot().await;
    an_organization(&kestrel).await;
    correlated(&kestrel, OnOpenWorkspace::NewSession).await;
    let stub = GithubStub::start();
    stub.deliver(github_stub::labelled(ISSUE, READY));
    watching(&kestrel, &stub).await;
    let workspace = opened(&kestrel).await;
    let first = kestrel
        .claim_session()
        .await
        .expect("the opening firing enqueued a session");

    kestrel
        .post_while_busy(workspace.id, "operator", "before")
        .await;
    ci_failed(&stub, &[]);
    fed(&kestrel, 1).await;
    kestrel
        .post_while_busy(workspace.id, "operator", "after")
        .await;

    let mut agents = Vec::new();
    let mut ending = first;
    for count in 2..=4 {
        kestrel.complete_session(&ending).await;
        ending = kestrel.sessions(workspace.id).await.remove(count - 1);
        agents.push(ending.agent.name.clone());
    }

    assert_eq!(agents, ["builder", "fixer", "fixer"]);
    let transcript = kestrel.transcript(workspace.id).await;
    let order: Vec<String> = transcript
        .iter()
        .filter_map(|recorded| match &recorded.entry {
            Entry::Messages { messages } => Some(
                messages
                    .iter()
                    .map(|message| message.message.clone())
                    .collect::<Vec<_>>()
                    .join(", "),
            ),
            Entry::Brief {
                source: BriefSource::Trigger { trigger },
                ..
            } => Some(trigger.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(order, ["ready", "before", "ci", "after"]);

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_label_chooses_the_agent_of_a_new_session_among_those_its_trigger_allows() {
    let kestrel = Kestrel::boot().await;
    an_organization(&kestrel).await;
    correlated(&kestrel, OnOpenWorkspace::NewSession).await;
    let stub = GithubStub::start();
    stub.deliver(github_stub::labelled(ISSUE, READY));
    watching(&kestrel, &stub).await;
    let workspace = opened(&kestrel).await;
    let first = kestrel
        .claim_session()
        .await
        .expect("the opening firing enqueued a session");
    kestrel.complete_session(&first).await;

    ci_failed(&stub, &["agent:codex"]);
    let started = sessions(&kestrel, workspace.id, 2).await;

    assert_eq!(started[1].agent.name, "codex");
    assert_eq!(started[1].agent.harness, "codex");

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_command_chooses_the_agent_of_a_new_session_among_those_its_trigger_allows() {
    let kestrel = Kestrel::boot().await;
    an_organization(&kestrel).await;
    kestrel
        .declare_correlated_trigger(
            "acme",
            "ready",
            &labelled_on(REPOSITORY, READY),
            "builder",
            &[],
            &templates(support::BRIEF, None, Some(CORRELATION)),
            OnOpenWorkspace::Continue,
        )
        .await;
    kestrel
        .declare_correlated_trigger(
            "acme",
            "asked",
            &serde_json::json!({"all": [
                {"exact": {"type": "com.github.issue_comment.created"}},
                {"prefix": {"data.comment.body": "@kestrel"}},
            ]})
            .to_string(),
            "fixer",
            &["codex"],
            &templates("{{ instruction }}", None, Some(CORRELATION)),
            OnOpenWorkspace::NewSession,
        )
        .await;
    let stub = GithubStub::start();
    stub.deliver(github_stub::labelled(ISSUE, READY));
    watching(&kestrel, &stub).await;
    let workspace = opened(&kestrel).await;
    let first = kestrel
        .claim_session()
        .await
        .expect("the opening firing enqueued a session");
    kestrel.complete_session(&first).await;

    stub.deliver(github_stub::issue_comment(
        11,
        ISSUE,
        "jack",
        "@kestrel agent=codex fix the build",
    ));
    let started = sessions(&kestrel, workspace.id, 2).await;

    assert_eq!(started[1].agent.name, "codex");
    assert!(briefs(&kestrel, workspace.id).await.contains(&(
        BriefSource::Trigger {
            trigger: "asked".to_owned()
        },
        "fix the build".to_owned()
    )));

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_trigger_that_omits_what_it_does_to_an_open_workspace_continues() {
    let kestrel = Kestrel::boot().await;
    an_organization(&kestrel).await;
    kestrel
        .apply_triggers(
            "acme",
            &format!(
                r#"
triggers:
  ready:
    filter: {ready}
    brief: "{brief}"
    correlation: "{CORRELATION}"
    on_miss: open
    project: kestrel
    agent: builder
  ci:
    filter: {ci}
    brief: "{CI_BRIEF}"
    correlation: "{CORRELATION}"
    on_miss: open
    project: kestrel
    agent: fixer
"#,
                ready = labelled_on(REPOSITORY, READY),
                brief = support::BRIEF,
                ci = labelled_on(REPOSITORY, CI_FAILED),
            ),
        )
        .await;
    assert_eq!(
        kestrel
            .show_trigger("acme", "ci")
            .await
            .templates
            .correlation
            .on_open_workspace(),
        OnOpenWorkspace::Continue
    );
    let stub = GithubStub::start();
    stub.deliver(github_stub::labelled(ISSUE, READY));
    watching(&kestrel, &stub).await;
    let workspace = opened(&kestrel).await;
    let first = kestrel
        .claim_session()
        .await
        .expect("the opening firing enqueued a session");
    kestrel.complete_session(&first).await;

    ci_failed(&stub, &[]);
    let continued = sessions(&kestrel, workspace.id, 2).await;

    assert_eq!(continued[1].agent.name, "builder");
    assert!(!briefs(&kestrel, workspace.id).await.contains(&ci_brief()));
    assert!(
        kestrel
            .transcript(workspace.id)
            .await
            .iter()
            .any(|recorded| matches!(
                &recorded.entry,
                Entry::Said { participant, message, .. }
                    if participant == "ci" && *message == ci_brief().1
            ))
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_new_session_trigger_without_a_correlation_is_refused() {
    let kestrel = Kestrel::boot().await;
    an_organization(&kestrel).await;

    let refused = kestrel
        .try_declare_correlated_trigger(
            "acme",
            "ci",
            &labelled_on(REPOSITORY, CI_FAILED),
            "fixer",
            &[],
            &templates(CI_BRIEF, None, None),
            OnOpenWorkspace::NewSession,
        )
        .await
        .expect_err("a new-session trigger needs a correlation to find an open workspace");

    assert!(refused.to_string().contains("correlation"), "{refused}");

    kestrel.teardown().await;
}

#[tokio::test]
async fn trigger_test_says_whether_a_firing_opens_continues_or_starts_a_new_session() {
    let kestrel = Kestrel::boot().await;
    an_organization(&kestrel).await;
    correlated(&kestrel, OnOpenWorkspace::NewSession).await;
    kestrel
        .declare_correlated_trigger(
            "acme",
            "also",
            &labelled_on(REPOSITORY, CI_FAILED),
            "builder",
            &[],
            &templates(CI_BRIEF, None, Some(CORRELATION)),
            OnOpenWorkspace::Continue,
        )
        .await;
    let stub = GithubStub::start();
    stub.deliver(github_stub::labelled(ISSUE, READY));
    watching(&kestrel, &stub).await;
    opened(&kestrel).await;
    let label = kestrel.events("acme").await.remove(0);

    assert_eq!(
        kestrel
            .test_trigger("acme", "ci", label.record_id)
            .await
            .would,
        Some(Would::NewSession)
    );
    assert_eq!(
        kestrel
            .test_trigger("acme", "also", label.record_id)
            .await
            .would,
        Some(Would::Continue)
    );
    let declared = format!(
        r#"
triggers:
  elsewhere:
    filter: {filter}
    brief: "{CI_BRIEF}"
    correlation: "elsewhere"
    on_miss: open
    on_open_workspace: new-session
    project: kestrel
    agent: fixer
  here:
    filter: {filter}
    brief: "{CI_BRIEF}"
    correlation: "{CORRELATION}"
    on_miss: open
    on_open_workspace: new-session
    project: kestrel
    agent: fixer
"#,
        filter = labelled_on(REPOSITORY, READY),
    );
    assert_eq!(
        kestrel
            .test_declared_trigger("acme", &declared, "elsewhere", label.record_id)
            .await
            .would,
        Some(Would::Open)
    );
    assert_eq!(
        kestrel
            .test_declared_trigger("acme", &declared, "here", label.record_id)
            .await
            .would,
        Some(Would::NewSession)
    );

    kestrel.teardown().await;
}
