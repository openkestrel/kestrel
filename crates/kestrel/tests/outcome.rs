//! The Integration's outbound direction (0.1/22). A Session that ends leaves one comment on the
//! issue that started it, carrying the exit status and what the Agent said last; exactly one,
//! however the control plane was interrupted, and a delivery that cannot be made changes
//! nothing about how the Session went.

mod support;

use std::time::Duration;

use jiff::SignedDuration;
use kestrel::domain::{Direction, Exit, Session, Workspace};
use support::Kestrel;
use support::github_stub::{
    self, GithubStub, INSTALLATION_TOKEN, RecordedRequest, ScriptedResponse,
};

const PATIENCE: Duration = Duration::from_secs(30);
const REPOSITORY: &str = "jtmthf/kestrel";
const READY: &str = "ready-for-agent";
const ISSUE: i64 = 43;
const COMMENTS: &str = "/issues/43/comments";
const BOTH: &[Direction] = &[Direction::Inbound, Direction::Outbound];
const INBOUND: &[Direction] = &[Direction::Inbound];

/// Sooner than the wheel's own sweep, so what paces these tests is the sweep rather than a
/// wait written into them.
fn eagerly() -> SignedDuration {
    SignedDuration::from_millis(1)
}

/// The Trigger comes before the poll: an Event recorded before the Trigger was declared fires
/// nothing.
async fn watching(kestrel: &Kestrel, stub: &GithubStub, carries: &[Direction]) {
    let organization = kestrel.declare_organization("acme").await;
    kestrel
        .declare_project(
            &organization,
            "kestrel",
            &["https://github.com/jtmthf/kestrel".to_owned()],
            "main",
        )
        .await;
    kestrel
        .declare_agent(&organization, "builder", "opencode", None)
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
            carries,
            eagerly(),
        )
        .await;
}

/// The Workspace a label opened, with its queued Session claimed the way a work role would claim
/// it.
async fn working(kestrel: &Kestrel) -> (Workspace, Session) {
    let deadline = tokio::time::Instant::now() + PATIENCE;

    loop {
        if let Some(workspace) = kestrel.workspaces("acme").await.into_iter().next()
            && let Some(claimed) = kestrel.claim_session().await
        {
            return (workspace, claimed);
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "no workspace was ever opened with a session to claim in it"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn comments_on_the_issue(stub: &GithubStub) -> Vec<RecordedRequest> {
    stub.requests()
        .into_iter()
        .filter(|request| request.method == "POST" && request.url.contains(COMMENTS))
        .collect()
}

async fn commented(stub: &GithubStub) -> RecordedRequest {
    let deadline = tokio::time::Instant::now() + PATIENCE;

    loop {
        if let Some(comment) = comments_on_the_issue(stub).into_iter().next() {
            return comment;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "nothing was ever said back on the issue"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// Nothing being said is only observable by waiting for the sweeps that would have said it.
async fn nothing_is_said(stub: &GithubStub) {
    tokio::time::sleep(Duration::from_secs(2)).await;

    assert!(
        comments_on_the_issue(stub).is_empty(),
        "a comment reached an issue nothing should have been said on"
    );
}

fn said(comment: &RecordedRequest) -> String {
    serde_json::from_str::<serde_json::Value>(&comment.body)
        .expect("a comment is posted as json")
        .get("body")
        .and_then(serde_json::Value::as_str)
        .expect("a comment carries a body")
        .to_owned()
}

fn labelled(stub: &GithubStub) {
    stub.script(github_stub::page(&[github_stub::labelled(7, ISSUE, READY)]));
}

#[tokio::test]
async fn a_session_that_completes_says_so_on_the_issue_that_started_it() {
    let stub = GithubStub::start();
    labelled(&stub);
    stub.script_answer("POST", COMMENTS, github_stub::created(1, "posted"));
    let kestrel = Kestrel::boot().await;
    watching(&kestrel, &stub, BOTH).await;

    let (workspace, session) = working(&kestrel).await;
    kestrel
        .said(
            &session,
            "Opened https://github.com/jtmthf/kestrel/pull/92.",
        )
        .await;
    kestrel.complete_session(&session).await;

    let comment = commented(&stub).await;
    let body = said(&comment);

    assert!(body.contains("session succeeded"), "{body}");
    assert!(
        body.contains("> Opened https://github.com/jtmthf/kestrel/pull/92."),
        "the agent's last message is where a link to its pull request lives: {body}"
    );
    assert!(body.contains(&workspace.id.to_string()), "{body}");
    assert!(body.contains(&session.id.to_string()), "{body}");

    kestrel.teardown().await;
}

/// The Organization's credential, presented by the Integration that carried the Event in.
#[tokio::test]
async fn the_comment_is_posted_with_the_integrations_credential() {
    let stub = GithubStub::start();
    labelled(&stub);
    stub.script_answer("POST", COMMENTS, github_stub::created(1, "posted"));
    let kestrel = Kestrel::boot().await;
    watching(&kestrel, &stub, BOTH).await;

    let (_, session) = working(&kestrel).await;
    kestrel.complete_session(&session).await;

    let comment = commented(&stub).await;

    assert!(
        comment
            .headers
            .iter()
            .any(|(name, value)| name == "authorization"
                && value == &format!("Bearer {INSTALLATION_TOKEN}")),
        "the comment went out without the integration's credential: {:?}",
        comment.headers
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_session_that_fails_says_that_it_failed_and_why() {
    let stub = GithubStub::start();
    labelled(&stub);
    stub.script_answer("POST", COMMENTS, github_stub::created(1, "posted"));
    let kestrel = Kestrel::boot().await;
    watching(&kestrel, &stub, BOTH).await;

    let (_, session) = working(&kestrel).await;
    kestrel
        .fail_session(&session, "the environment could not be provisioned")
        .await;

    let body = said(&commented(&stub).await);

    assert!(body.contains("session failed"), "{body}");
    assert!(
        body.contains("the environment could not be provisioned"),
        "{body}"
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn one_session_leaves_exactly_one_comment() {
    let stub = GithubStub::start();
    labelled(&stub);
    stub.script_answer("POST", COMMENTS, github_stub::created(1, "posted"));
    let kestrel = Kestrel::boot().await;
    watching(&kestrel, &stub, BOTH).await;

    let (_, session) = working(&kestrel).await;
    kestrel.complete_session(&session).await;
    commented(&stub).await;
    tokio::time::sleep(Duration::from_secs(2)).await;

    assert_eq!(
        comments_on_the_issue(&stub).len(),
        1,
        "one session said itself out loud more than once"
    );

    kestrel.teardown().await;
}

/// A control plane that dies between sending the comment and learning what became of it comes
/// back to a delivery it cannot tell apart from one that never went out. It reads the issue
/// back, recognises its own comment, and says nothing a second time.
#[tokio::test]
async fn a_comment_that_landed_while_the_control_plane_died_is_not_posted_twice() {
    let stub = GithubStub::start();
    labelled(&stub);
    stub.script_answer("POST", COMMENTS, ScriptedResponse::answering(502));
    let kestrel = Kestrel::boot().await;
    watching(&kestrel, &stub, BOTH).await;

    let (_, session) = working(&kestrel).await;
    kestrel.complete_session(&session).await;
    let landed = said(&commented(&stub).await);

    let kestrel = kestrel.kill_and_restart().await;
    stub.script_answer(
        "GET",
        COMMENTS,
        github_stub::page(&[github_stub::comment(1, &landed)]),
    );
    tokio::time::sleep(Duration::from_secs(2)).await;

    assert_eq!(
        comments_on_the_issue(&stub).len(),
        1,
        "the comment already on the issue was posted again after the restart"
    );

    kestrel.teardown().await;
}

/// A refusal is not a failure of the work: the Session's exit status was decided before anything
/// was said, and the delivery is tried again rather than given up on.
#[tokio::test]
async fn a_comment_that_is_refused_is_tried_again_and_leaves_the_session_as_it_was() {
    let stub = GithubStub::start();
    labelled(&stub);
    stub.script_answer("POST", COMMENTS, ScriptedResponse::answering(500));
    stub.script_answer("GET", COMMENTS, github_stub::page(&[]));
    stub.script_answer("POST", COMMENTS, github_stub::created(1, "posted"));
    let kestrel = Kestrel::boot().await;
    watching(&kestrel, &stub, BOTH).await;

    let (_, session) = working(&kestrel).await;
    kestrel.complete_session(&session).await;

    let deadline = tokio::time::Instant::now() + PATIENCE;
    while comments_on_the_issue(&stub).len() < 2 {
        assert!(
            tokio::time::Instant::now() < deadline,
            "a refused comment was never tried again"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    assert_eq!(
        kestrel.session(session.id).await.exit,
        Some(Exit::Succeeded),
        "a comment that could not be posted changed how the session ended"
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_workspace_no_event_started_says_nothing_and_that_is_not_an_error() {
    let stub = GithubStub::start();
    let kestrel = Kestrel::boot().await;
    watching(&kestrel, &stub, BOTH).await;

    let workspace = kestrel.open_workspace("acme", "kestrel", "builder").await;
    let session = kestrel.dispatch_session(workspace.id).await;
    kestrel.complete_session(&session).await;

    nothing_is_said(&stub).await;

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_later_sessions_outcome_does_not_reuse_an_earlier_sessions_message() {
    let stub = GithubStub::start();
    let kestrel = Kestrel::boot().await;
    watching(&kestrel, &stub, BOTH).await;

    let workspace = kestrel.open_workspace("acme", "kestrel", "builder").await;
    let first = kestrel.dispatch_session(workspace.id).await;
    kestrel
        .said(&first, "The first investigation finished.")
        .await;
    kestrel.complete_session(&first).await;
    assert_eq!(
        kestrel.session(first.id).await.outcome_message.as_deref(),
        Some("The first investigation finished.")
    );

    let second = kestrel.enqueue_session(workspace.id).await;
    kestrel
        .fail_session(&second, "the environment stopped")
        .await;
    let ended = kestrel.session(second.id).await;
    assert_eq!(
        ended.exit,
        Some(Exit::Failed {
            because: "the environment stopped".to_owned()
        })
    );
    assert_eq!(ended.outcome_message, None);

    kestrel.teardown().await;
}

/// The direction an Integration declares is what it does, rather than a label beside it.
#[tokio::test]
async fn an_integration_that_carries_only_inbound_says_nothing() {
    let stub = GithubStub::start();
    labelled(&stub);
    let kestrel = Kestrel::boot().await;
    watching(&kestrel, &stub, INBOUND).await;

    let (_, session) = working(&kestrel).await;
    kestrel.complete_session(&session).await;

    nothing_is_said(&stub).await;

    kestrel.teardown().await;
}
