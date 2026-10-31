//! A firing that correlates to an open Workspace and declares `new-session` starts a Session with
//! its Agent and Brief once the unfinished one lets go, or ends the one waiting between its turns
//! (ADR-0031).

use kestrel::domain::{Exit, OnOpenWorkspace, SessionState};
use kestrel::log::{BriefSource, Entry};

use crate::support::correlation::{
    CORRELATION, ISSUE, READY, REPOSITORY, an_organization, briefs, ci_brief, ci_failed,
    correlated, fed, opened, sessions, watching,
};
use crate::support::github_stub::{self, GithubStub};
use crate::support::scripted_agent::{self, Script};
use crate::support::{BRIEF, Kestrel, labelled_on, supervisor, templates};

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
            &templates(BRIEF, None, Some(CORRELATION)),
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
