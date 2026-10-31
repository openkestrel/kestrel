//! `kestrel sign-in test`: what it says it will spend, what it reports, and how a test that did
//! not work or never started fails.

use kestrel_scripted_agent::DEFAULT_MODEL;
use serde_json::Value;

use crate::support;

use support::Kestrel;
use support::client::{self, Finished, Invocation};
use support::scripted_agent::{self, Script};
use support::sign_in::{CLAUDE_TOKEN, save};
use support::supervisor;

const TEST: [&str; 5] = [
    "sign-in",
    "test",
    "claude",
    "--method",
    "claude-setup-token",
];

async fn playing(script: Script) -> Kestrel {
    let kestrel = Kestrel::dispatching_harnesses(
        supervisor::binary(),
        &[("claude", &scripted_agent::playing(script))],
    )
    .await;
    kestrel.name_operator("ada").await;
    kestrel
}

async fn tested(kestrel: &Kestrel, extra: &[&str]) -> Finished {
    let args: Vec<&str> = TEST.iter().chain(extra).copied().collect();
    client::ran_by(kestrel, &args, Invocation::default()).await
}

#[tokio::test]
async fn a_test_says_it_spends_model_usage_and_reports_the_model_use_it_established() {
    let kestrel = playing(Script::Speaks).await;
    let saved = save(&kestrel, "claude", "claude-setup-token", CLAUDE_TOKEN).await;

    let worked = tested(&kestrel, &["--json"]).await;
    assert!(worked.err.contains("spends model usage"), "{}", worked.err);
    assert!(!worked.err.contains(CLAUDE_TOKEN), "{}", worked.err);
    let test = worked.json();
    assert_eq!(test["outcome"], "worked");
    assert_eq!(test["model"], DEFAULT_MODEL);
    assert_eq!(test["revision"], saved.body["revision"]);

    let shown = tested(&kestrel, &[]).await;
    assert!(shown.status.success(), "{}", shown.err);
    let shown = shown.out.join("\n");
    assert!(
        shown.contains(&format!("model use worked for claude {DEFAULT_MODEL}")),
        "{shown}"
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_refused_login_fails_the_command_with_a_typed_diagnostic_and_nothing_on_stdout() {
    let kestrel = playing(Script::Insists).await;
    save(&kestrel, "claude", "claude-setup-token", CLAUDE_TOKEN).await;

    let failed = tested(&kestrel, &["--json"]).await;

    assert_eq!(failed.status.code(), Some(4), "{}", failed.err);
    assert!(failed.out.is_empty(), "{:?}", failed.out);
    let diagnostic: Value =
        serde_json::from_str(failed.err.lines().last().expect("a diagnostic")).expect("JSON");
    assert_eq!(diagnostic["context"]["resource"], "sign_in_test");
    assert_eq!(diagnostic["context"]["state"], "authentication_failed");
    assert_eq!(diagnostic["next_steps"][0]["action"], "sign_in");

    kestrel.teardown().await;
}

#[tokio::test]
async fn nothing_is_sent_without_a_model_a_method_needs_and_an_unsaved_sign_in_is_refused() {
    let kestrel = playing(Script::Speaks).await;

    let unnamed = client::ran_by(
        &kestrel,
        &["sign-in", "test", "opencode", "--method", "opencode-go-zen"],
        Invocation::default(),
    )
    .await;
    assert_eq!(unnamed.status.code(), Some(2), "{}", unnamed.err);
    assert!(
        unnamed.err.contains("--model PROVIDER/MODEL"),
        "{}",
        unnamed.err
    );
    assert!(
        !unnamed.err.contains("spends model usage"),
        "{}",
        unnamed.err
    );

    let unsaved = tested(&kestrel, &[]).await;
    assert_eq!(unsaved.status.code(), Some(4), "{}", unsaved.err);
    assert!(unsaved.err.contains("error: "), "{}", unsaved.err);

    kestrel.teardown().await;
}
