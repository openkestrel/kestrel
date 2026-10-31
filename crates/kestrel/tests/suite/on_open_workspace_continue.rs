//! A firing that correlates to an open Workspace continues the waiting Session: its Brief becomes
//! the Session's next turn, and a Trigger that says nothing else does the same (ADR-0031).

use kestrel::domain::{OnOpenWorkspace, SessionState};
use kestrel::log::Entry;
use kestrel_scripted_agent::conversed;

use crate::support::correlation::{
    CI_BRIEF, CI_FAILED, CORRELATION, ISSUE, READY, REPOSITORY, an_organization, briefs, ci_brief,
    ci_failed, opened, sessions, watching,
};
use crate::support::github_stub::{self, GithubStub};
use crate::support::scripted_agent::{self, Script};
use crate::support::{BRIEF, Kestrel, labelled_on, supervisor, templates};

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
                brief = BRIEF,
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
