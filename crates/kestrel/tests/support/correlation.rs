//! Fixtures for a Trigger that correlates to an open Workspace (ADR-0031): declare the Trigger
//! that opens one and the Trigger that correlates to it, then deliver what each fires on.

use std::time::Duration;

use jiff::SignedDuration;
use kestrel::domain::{Direction, OnOpenWorkspace, Session, Workspace, WorkspaceId};
use kestrel::log::{BriefSource, Entry};

use super::fixture::Fixture;
use super::github_stub::{self, GithubStub};
use super::{BRIEF, HARNESS, Kestrel, labelled_on, templates};

pub const PATIENCE: Duration = Duration::from_secs(30);
pub const REPOSITORY: &str = "jtmthf/kestrel";
pub const ISSUE: i64 = 43;
pub const READY: &str = "ready-for-agent";
pub const CI_FAILED: &str = "ci-failed";
pub const CORRELATION: &str = "{{ event.source }}{{ event.subject }}";
pub const CI_BRIEF: &str = "Fix the build of {{ event.data.issue.title }}";
const BOTH: &[Direction] = &[Direction::Inbound, Direction::Outbound];

pub async fn an_organization(kestrel: &Kestrel) {
    Fixture::acme()
        .checked_out()
        .agent("fixer", HARNESS, None)
        .agent("codex", "codex", None)
        .holding_a_provider_key()
        .declare(kestrel)
        .await;
}

/// `ready` opens the Workspace for the issue with `builder`; `ci` correlates to it and does what
/// `on_open_workspace` declares with `fixer`, or with `codex` when a label chooses it.
pub async fn correlated(kestrel: &Kestrel, on_open_workspace: OnOpenWorkspace) {
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
            "ci",
            &labelled_on(REPOSITORY, CI_FAILED),
            "fixer",
            &["codex"],
            &templates(CI_BRIEF, None, Some(CORRELATION)),
            on_open_workspace,
        )
        .await;
}

pub async fn watching(kestrel: &Kestrel, stub: &GithubStub) {
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

pub async fn opened(kestrel: &Kestrel) -> Workspace {
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

pub async fn fed(kestrel: &Kestrel, count: usize) {
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

pub async fn sessions(kestrel: &Kestrel, workspace: WorkspaceId, count: usize) -> Vec<Session> {
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

pub async fn briefs(kestrel: &Kestrel, workspace: WorkspaceId) -> Vec<(BriefSource, String)> {
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
pub fn ci_failed(stub: &GithubStub, labels: &[&str]) {
    stub.deliver(github_stub::labelled_carrying(ISSUE, CI_FAILED, labels));
}

pub fn ci_brief() -> (BriefSource, String) {
    (
        BriefSource::Trigger {
            trigger: "ci".to_owned(),
        },
        format!("Fix the build of an issue numbered {ISSUE}"),
    )
}
