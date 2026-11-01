//! Fixtures for what a Session says back: the Workspace an issue opened, the Session a work role
//! claims on it, and what reached the issue once the sweeps had a chance to say it (ADR-0024).

use std::time::Duration;

use jiff::SignedDuration;
use kestrel::domain::{Direction, Session, Workspace, WorkspaceId};
use kestrel::work::{Report, Reported};

use super::fixture::Fixture;
use super::github_stub::{self, GithubStub, RecordedRequest};
use super::link_client::Link;
use super::{Kestrel, OnTheLink};

const PATIENCE: Duration = Duration::from_secs(30);
const REPOSITORY: &str = "jtmthf/kestrel";
const ISSUE: i64 = 43;
pub const COMMENTS: &str = "/issues/43/comments";

pub fn comments(stub: &GithubStub) -> Vec<RecordedRequest> {
    stub.requests()
        .into_iter()
        .filter(|request| request.method == "POST" && request.url.contains(COMMENTS))
        .collect()
}

fn said(comment: &RecordedRequest) -> String {
    serde_json::from_str::<serde_json::Value>(&comment.body)
        .expect("a comment is posted as json")
        .get("body")
        .and_then(serde_json::Value::as_str)
        .expect("a comment carries a body")
        .to_owned()
}

/// The bodies posted to the issue once at least `count` of them have been.
pub async fn replies(stub: &GithubStub, count: usize) -> Vec<String> {
    let deadline = tokio::time::Instant::now() + PATIENCE;
    loop {
        let comments = comments(stub);
        if comments.len() >= count {
            return comments.iter().map(said).collect();
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "only {} of {count} replies reached the issue",
            comments.len()
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// Nothing being said is only observable by waiting for the sweeps that would have said it.
pub async fn nothing_more_is_said(stub: &GithubStub, after: usize) {
    tokio::time::sleep(Duration::from_secs(2)).await;

    assert_eq!(
        comments(stub).len(),
        after,
        "a reply that should not have been said out loud was"
    );
}

async fn workspaces(kestrel: &Kestrel, count: usize) -> Vec<Workspace> {
    let deadline = tokio::time::Instant::now() + PATIENCE;
    loop {
        let workspaces = kestrel.workspaces("acme").await;
        if workspaces.len() == count {
            return workspaces;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "{count} workspaces never opened"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// A Workspace an Event started through an Integration that carries what it says back out.
pub async fn a_workspace_from_the_issue(kestrel: &Kestrel, stub: &GithubStub) -> Workspace {
    Fixture::acme()
        .without_repositories()
        .declare(kestrel)
        .await;
    kestrel
        .declare_trigger(
            "acme",
            "ready",
            &super::labelled_on(REPOSITORY, "ready-for-agent"),
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
    stub.deliver(github_stub::labelled(ISSUE, "ready-for-agent"));

    workspaces(kestrel, 1).await.remove(0)
}

/// A Session claimed the way a work role claims it, with its first Turn prompted.
pub async fn a_working_session(kestrel: &Kestrel, workspace: WorkspaceId) -> (Session, OnTheLink) {
    let deadline = tokio::time::Instant::now() + PATIENCE;
    loop {
        if kestrel.sessions(workspace).await.len() == 1
            && let Some(claimed) = kestrel.claim_session().await
        {
            let on = kestrel.on_the_link(&claimed).await;
            kestrel.start(&claimed, super::harness()).await;
            return (claimed, on);
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the workspace never had a session to claim"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

pub async fn report(link: &Link, session: &Session, on: &OnTheLink, seq: i64, report: Report) {
    let answered = link
        .report(
            &on.instance,
            Some(&on.credential),
            &Reported {
                session: Some(session.id),
                seq: Some(seq),
                report,
            },
        )
        .await;
    assert_eq!(
        answered.status(),
        reqwest::StatusCode::ACCEPTED,
        "the link refused report {seq}"
    );
}
