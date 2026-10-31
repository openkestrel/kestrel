//! Declaring a Trigger that correlates to an open Workspace, and asking what a firing would do to
//! one, without delivering the event (ADR-0031).

use kestrel::domain::OnOpenWorkspace;
use kestrel::trigger::Would;

use crate::support::correlation::{
    CI_BRIEF, CI_FAILED, CORRELATION, ISSUE, READY, REPOSITORY, an_organization, correlated,
    opened, watching,
};
use crate::support::github_stub::{self, GithubStub};
use crate::support::{Kestrel, labelled_on, templates};

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
