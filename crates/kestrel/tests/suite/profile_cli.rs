//! `kestrel profile set`: the command that creates a Subscription Profile, keeps its ownership and
//! organization scope, and takes its input without echoing it.

use crate::support;

use kestrel_scripted_agent::LOGIN;
use support::Kestrel;
use support::client::{self, Invocation};
use support::profile::SUBSCRIPTION_KEY;

#[tokio::test]
async fn cli_profile_set_creates_operator_ownership_and_preserves_input() {
    let kestrel = Kestrel::boot().await;
    kestrel.name_operator("Jack").await;
    let args = [
        "profile",
        "set",
        "personal",
        "--variable",
        SUBSCRIPTION_KEY,
        "--json",
    ];
    let saved = client::ran_by(&kestrel, &args, Invocation::default().given("  secret\n\n")).await;
    assert!(saved.status.success(), "{}", saved.err);
    assert!(!saved.out.join("\n").contains("secret"));
    assert!(!saved.err.contains("secret"));
    let profiles = kestrel.profiles("Jack").await;
    assert_eq!(profiles.len(), 1);
    let profile = &profiles[0].0;
    assert!(profile.owner_operator.is_some());
    assert_eq!(
        kestrel.profile_contents(profile).await.variables[SUBSCRIPTION_KEY],
        "  secret\n"
    );

    kestrel.name_operator("Jacqueline").await;
    let saved = client::ran_by(
        &kestrel,
        &args,
        Invocation::default().given("replacement\r\n"),
    )
    .await;
    assert!(saved.status.success(), "{}", saved.err);
    let profiles = kestrel.profiles("Jack").await;
    assert_eq!(profiles[0].0.id, profile.id);
    assert_eq!(profiles[0].0.owner_operator, profile.owner_operator);
    assert_eq!(profiles[0].0.owner, "Jacqueline");
    assert_eq!(
        kestrel.profile_contents(&profiles[0].0).await.variables[SUBSCRIPTION_KEY],
        "replacement"
    );
    kestrel.teardown().await;
}

#[tokio::test]
async fn cli_profile_set_keeps_generic_ownership_and_organization_scope() {
    let kestrel = Kestrel::boot().await;
    kestrel.name_operator("Jack").await;
    kestrel.declare_organization("other").await;
    let args = [
        "--organization",
        "other",
        "profile",
        "set",
        "generic",
        "--owner",
        "Jack",
        "--file",
        LOGIN,
        "--json",
    ];
    let saved = client::ran_by(&kestrel, &args, Invocation::default().given(" \nlogin\n\n")).await;
    assert!(saved.status.success(), "{}", saved.err);
    assert!(kestrel.profiles("Jack").await.is_empty());
    let profiles = kestrel.profiles("other").await;
    assert_eq!(profiles[0].0.owner, "Jack");
    assert!(profiles[0].0.owner_operator.is_none());
    assert_eq!(
        kestrel.profile_contents(&profiles[0].0).await.files[LOGIN],
        " \nlogin\n\n"
    );
    let revision = profiles[0].1[0].revision;

    kestrel.name_operator("Jacqueline").await;
    let args = [
        "profile",
        "set",
        "generic",
        "--owner",
        "Someone else",
        "--file",
        LOGIN,
        "--json",
    ];
    let saved = client::ran_by(
        &kestrel,
        &args,
        Invocation::default()
            .env("KESTREL_ORGANIZATION", "other")
            .given("replacement"),
    )
    .await;
    assert!(saved.status.success(), "{}", saved.err);
    let profiles = kestrel.profiles("other").await;
    assert_eq!(profiles[0].0.owner, "Jack");
    assert!(profiles[0].0.owner_operator.is_none());
    assert!(profiles[0].1[0].revision > revision);
    assert_eq!(
        kestrel.profile_contents(&profiles[0].0).await.files[LOGIN],
        "replacement"
    );
    kestrel.teardown().await;
}

#[tokio::test]
async fn cli_profile_set_reports_saved_creation_and_rejects_empty_input_without_loss() {
    let kestrel = Kestrel::boot().await;
    kestrel.name_operator("Jack").await;
    let invalid = [
        "profile",
        "set",
        "personal",
        "--file",
        "../outside",
        "--json",
    ];
    let failed = client::ran_by(
        &kestrel,
        &invalid,
        Invocation::default().given("private-material"),
    )
    .await;
    assert_eq!(failed.status.code(), Some(4));
    let diagnostic: serde_json::Value =
        serde_json::from_str(&failed.err).expect("one typed diagnostic");
    assert_eq!(diagnostic["kind"], "invalid_field");
    assert!(
        diagnostic["message"]
            .as_str()
            .unwrap()
            .contains("already declared")
    );
    assert!(!failed.err.contains("private-material"));
    let profiles = kestrel.profiles("Jack").await;
    assert_eq!(profiles.len(), 1);
    assert!(profiles[0].1.is_empty());

    let args = [
        "profile",
        "set",
        "personal",
        "--variable",
        SUBSCRIPTION_KEY,
        "--json",
    ];
    let saved = client::ran_by(&kestrel, &args, Invocation::default().given("original")).await;
    assert!(saved.status.success(), "{}", saved.err);
    let revision = kestrel.profiles("Jack").await[0].1[0].revision;
    for input in ["", "\n", "\r\n"] {
        let rejected = client::ran_by(&kestrel, &args, Invocation::default().given(input)).await;
        assert_eq!(rejected.status.code(), Some(2));
    }
    let profiles = kestrel.profiles("Jack").await;
    assert_eq!(profiles[0].1[0].revision, revision);
    assert_eq!(
        kestrel.profile_contents(&profiles[0].0).await.variables[SUBSCRIPTION_KEY],
        "original"
    );
    let rejected = client::ran_by(
        &kestrel,
        &["profile", "set", "absent", "--variable", SUBSCRIPTION_KEY],
        Invocation::default().given("\n"),
    )
    .await;
    assert_eq!(rejected.status.code(), Some(2));
    assert_eq!(kestrel.profiles("Jack").await.len(), 1);
    kestrel.teardown().await;
}

#[tokio::test]
async fn cli_profile_set_requires_the_operator_only_for_defaulted_creation() {
    let kestrel = Kestrel::boot().await;
    kestrel.declare_organization("acme").await;
    let args = [
        "profile",
        "set",
        "generic",
        "--variable",
        SUBSCRIPTION_KEY,
        "--json",
    ];
    let failed = client::ran_by(&kestrel, &args, Invocation::default().given("secret")).await;
    assert_eq!(failed.status.code(), Some(78));
    let diagnostic: serde_json::Value = serde_json::from_str(&failed.err).unwrap();
    assert_eq!(diagnostic["kind"], "setup_gap");
    assert_eq!(diagnostic["next_steps"][0]["action"], "name_operator");
    assert!(kestrel.profiles("acme").await.is_empty());
    let explicit = [
        "profile",
        "set",
        "generic",
        "--owner",
        "Alex",
        "--variable",
        SUBSCRIPTION_KEY,
    ];
    let saved = client::ran_by(&kestrel, &explicit, Invocation::default().given("first")).await;
    assert!(saved.status.success(), "{}", saved.err);
    let saved = client::ran_by(&kestrel, &args, Invocation::default().given("second")).await;
    assert!(saved.status.success(), "{}", saved.err);
    let profiles = kestrel.profiles("acme").await;
    assert_eq!(profiles[0].0.owner, "Alex");
    assert!(profiles[0].0.owner_operator.is_none());
    kestrel.teardown().await;
}

#[tokio::test]
async fn cli_profile_set_hides_terminal_input() {
    let kestrel = Kestrel::boot().await;
    kestrel.name_operator("Jack").await;
    let shown = client::ran_on_a_terminal_by(
        &kestrel,
        &["profile", "set", "personal", "--variable", SUBSCRIPTION_KEY],
        Invocation::default().after_prompt("unechoed; end it with ctrl-d"),
        "terminal-private-value\n\x04",
    )
    .await;
    assert!(shown.status.success(), "{}", shown.said);
    assert!(
        !shown.said.contains("terminal-private-value"),
        "{}",
        shown.said
    );
    let profiles = kestrel.profiles("Jack").await;
    assert_eq!(
        kestrel.profile_contents(&profiles[0].0).await.variables[SUBSCRIPTION_KEY],
        "terminal-private-value"
    );
    kestrel.teardown().await;
}
