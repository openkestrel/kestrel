//! `kestrel login` choosing a Sign-in Method and importing material, at a terminal and without one.

use serde_json::Value;

use crate::support;

use support::Kestrel;
use support::client::{self, Finished, Invocation};
use support::provider_stub::{self, ProviderStub};
use support::sign_in::{ANTHROPIC_KEY, CHATGPT_LOGIN, CLAUDE_TOKEN, booted, saved};

async fn login(kestrel: &Kestrel, args: &[&str], invocation: Invocation) -> Finished {
    let args: Vec<&str> = ["login"].iter().chain(args).copied().collect();
    client::ran_by(kestrel, &args, invocation).await
}

fn private(finished: &Finished) {
    let said = format!("{}\n{}", finished.out.join("\n"), finished.err);
    for secret in [
        CLAUDE_TOKEN,
        ANTHROPIC_KEY,
        "access-secret",
        "refresh-secret",
    ] {
        assert!(!said.contains(secret), "a secret was shown:\n{said}");
    }
}

#[tokio::test]
async fn without_a_terminal_a_method_is_needed_unless_the_input_leaves_one() {
    let stub = ProviderStub::start();
    let kestrel = booted(&stub).await;

    for (args, says) in [
        (&["claude"][..], "--method"),
        (&["claude", "--token"], "--method"),
        (
            &["codex", "--method", "codex-device-auth", "--token"],
            "--file PATH",
        ),
        (
            &[
                "claude",
                "--method",
                "claude-setup-token",
                "--file",
                "token",
            ],
            "--token",
        ),
        (&["opencode", "--file", "token"], "login file"),
        (&["nonsense"], "claude"),
    ] {
        let refused = login(&kestrel, args, Invocation::default().given(CLAUDE_TOKEN)).await;
        assert_eq!(refused.status.code(), Some(2), "{args:?}: {}", refused.err);
        assert!(refused.err.contains(says), "{args:?}: {}", refused.err);
        private(&refused);
    }
    assert!(
        saved(&kestrel).await.is_empty(),
        "a refused login saved something"
    );

    let imported = login(
        &kestrel,
        &["codex", "--file", "auth.json", "--json"],
        Invocation::default().file("auth.json", CHATGPT_LOGIN),
    )
    .await;
    private(&imported);
    let imported = imported.json();
    assert_eq!(imported["method"], "codex-device-auth");
    assert_eq!(imported["holding"]["resource"], "subscription_profile");
    assert_eq!(imported["holding"]["reference"], "ada");
    assert_eq!(imported["authentication"]["state"], "unchecked");

    let token = login(
        &kestrel,
        &["claude", "--method", "claude-setup-token", "--token"],
        Invocation::default().given(&format!("{CLAUDE_TOKEN}\n")),
    )
    .await;
    assert!(token.status.success(), "{}", token.err);
    private(&token);
    let shown = token.out.join("\n");
    assert!(shown.contains("your own Profile ada"), "{shown}");
    assert!(shown.contains("unchecked (import)"), "{shown}");
    assert!(
        token.err.contains("kestrel sign-in test claude"),
        "{}",
        token.err
    );
    assert!(!token.err.contains('\u{1b}'), "{}", token.err);

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_key_is_the_organizations_and_a_provider_that_cannot_answer_leaves_it_saved_with_exit_5()
{
    let stub = ProviderStub::start();
    let kestrel = booted(&stub).await;
    let args = ["claude", "--method", "anthropic-api-key", "--json"];

    let accepted = login(&kestrel, &args, Invocation::default().given(ANTHROPIC_KEY)).await;
    private(&accepted);
    let accepted = accepted.json();
    assert_eq!(accepted["holding"]["resource"], "provider_credential");
    assert_eq!(accepted["holding"]["reference"], "ANTHROPIC_API_KEY");
    assert_eq!(accepted["authentication"]["state"], "credential_accepted");
    assert_eq!(accepted["model_use"], Value::Array(Vec::new()));

    stub.answer(provider_stub::overloaded());
    let partial = login(
        &kestrel,
        &args[..3],
        Invocation::default().given(ANTHROPIC_KEY),
    )
    .await;
    assert_eq!(partial.status.code(), Some(5), "{}", partial.err);
    private(&partial);
    assert!(partial.out.is_empty(), "{:?}", partial.out);
    assert!(
        partial
            .err
            .contains("saved claude anthropic-api-key as revision"),
        "{}",
        partial.err
    );
    assert!(partial.err.contains("unchecked"), "{}", partial.err);

    stub.answer(provider_stub::overloaded());
    let partial = login(&kestrel, &args, Invocation::default().given(ANTHROPIC_KEY)).await;
    assert_eq!(partial.status.code(), Some(5), "{}", partial.err);
    let diagnostic: Value = serde_json::from_str(partial.err.trim()).expect("one JSON diagnostic");
    assert_eq!(diagnostic["kind"], "unavailable");
    assert_eq!(
        diagnostic["context"]["saved"]["authentication"]["state"],
        "unchecked"
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_terminal_is_shown_every_method_and_where_it_is_held_and_asked_for_the_key_unechoed() {
    let stub = ProviderStub::start();
    let kestrel = booted(&stub).await;

    let shown = client::ran_on_a_terminal_by(
        &kestrel,
        &["login", "claude"],
        Invocation::default()
            .after_prompt("Choose 1-2: ")
            .answering("Paste the Anthropic API key", &format!("{ANTHROPIC_KEY}\n")),
        "2\n",
    )
    .await;

    assert!(shown.status.success(), "{}", shown.said);
    assert!(!shown.said.contains(ANTHROPIC_KEY), "{}", shown.said);
    assert!(!shown.said.contains('\u{1b}'), "{}", shown.said);
    for said in [
        "Claude subscription (claude-setup-token), held in your own Profile",
        "Anthropic API key (anthropic-api-key), held by the Organization as ANTHROPIC_API_KEY",
        "credential_accepted",
    ] {
        assert!(shown.said.contains(said), "{said} missing:\n{}", shown.said);
    }

    kestrel.teardown().await;
}
