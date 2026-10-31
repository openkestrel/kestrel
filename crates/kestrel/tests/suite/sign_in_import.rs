use reqwest::StatusCode;
use serde_json::{Value, json};

use crate::support::provider_stub::ProviderStub;
use crate::support::sign_in::{
    ANTHROPIC_KEY, CHATGPT_LOGIN, CLAUDE_TOKEN, OPERATOR, booted, one, save, saved,
};

#[tokio::test]
async fn a_subscription_token_is_held_in_the_operators_own_profile_without_asking_anyone() {
    let stub = ProviderStub::start();
    let kestrel = booted(&stub).await;

    let token = save(
        &kestrel,
        "claude",
        "claude-setup-token",
        &format!("{CLAUDE_TOKEN}\n"),
    )
    .await;

    assert_eq!(token.status, StatusCode::OK, "{}", token.text);
    assert!(!token.text.contains(CLAUDE_TOKEN), "{}", token.text);
    assert_eq!(token.body["kind"], "subscription");
    assert_eq!(
        token.body["holding"],
        json!({
            "resource": "subscription_profile",
            "reference": OPERATOR,
            "fills": { "kind": "variable", "variable": "CLAUDE_CODE_OAUTH_TOKEN" },
        })
    );
    assert_eq!(token.body["authentication"]["state"], "unchecked");
    assert_eq!(token.body["authentication"]["source"], "import");
    assert_eq!(token.body["authentication"]["provider_check"], Value::Null);
    assert!(
        stub.asked().is_empty(),
        "a subscription has no check to ask"
    );
    assert_eq!(saved(&kestrel).await, vec![token.body]);

    let credentials = reqwest::get(format!(
        "{}/operator/organizations/ada/credentials",
        kestrel.operator()
    ))
    .await
    .expect("the operator boundary answers");
    assert_eq!(
        credentials.json::<Value>().await.expect("credentials"),
        json!([]),
        "a Claude plan is never the Organization's to share"
    );
    let profiles = kestrel.profiles(OPERATOR).await;
    assert_eq!(profiles.len(), 1);
    assert!(
        profiles[0].0.owner_operator.is_some(),
        "declared as the Operator's"
    );
    assert_eq!(
        kestrel
            .profile_contents(&profiles[0].0)
            .await
            .variables
            .get("CLAUDE_CODE_OAUTH_TOKEN"),
        Some(&CLAUDE_TOKEN.to_owned()),
        "held without the line break a terminal adds"
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_key_is_the_organizations_and_one_metadata_read_records_its_acceptance() {
    let stub = ProviderStub::start();
    let kestrel = booted(&stub).await;

    let key = save(&kestrel, "claude", "anthropic-api-key", ANTHROPIC_KEY).await;

    assert_eq!(key.status, StatusCode::OK, "{}", key.text);
    assert!(!key.text.contains(ANTHROPIC_KEY), "{}", key.text);
    assert_eq!(key.body["kind"], "key");
    assert_eq!(key.body["holding"]["resource"], "provider_credential");
    assert_eq!(key.body["holding"]["reference"], "ANTHROPIC_API_KEY");
    assert_eq!(key.body["authentication"]["state"], "credential_accepted");
    assert_eq!(
        key.body["authentication"]["provider_check"],
        json!({ "provider": "anthropic", "outcome": "accepted", "status": 200, "provider_error": null })
    );
    let asked = stub.asked();
    assert_eq!(asked.len(), 1, "one request, and no inference");
    assert_eq!(asked[0].method, "GET");
    assert_eq!(asked[0].url, "/v1/models?limit=1");
    assert_eq!(asked[0].header("x-api-key"), Some(ANTHROPIC_KEY));
    assert_eq!(asked[0].header("anthropic-version"), Some("2023-06-01"));

    let listed = saved(&kestrel).await;
    assert_eq!(listed.len(), 2, "{listed:#?}");
    let claude = one(&listed, "claude", "anthropic-api-key");
    let opencode = one(&listed, "opencode", "anthropic-api-key");
    assert_eq!(claude, &key.body);
    assert_eq!(opencode["revision"], claude["revision"]);
    assert_eq!(opencode["authentication"], claude["authentication"]);
    assert!(kestrel.profiles(OPERATOR).await.is_empty());

    let openai = save(&kestrel, "codex", "openai-api-key", "sk-proj-openai-secret").await;
    assert_eq!(openai.status, StatusCode::OK, "{}", openai.text);
    assert_eq!(
        openai.body["authentication"]["state"],
        "credential_accepted"
    );
    let asked = stub.asked();
    assert_eq!(asked[1].url, "/v1/models");
    assert_eq!(
        asked[1].header("authorization"),
        Some("Bearer sk-proj-openai-secret")
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn empty_or_incompatible_input_changes_nothing() {
    let stub = ProviderStub::start();
    let kestrel = booted(&stub).await;
    let token = save(&kestrel, "claude", "claude-setup-token", CLAUDE_TOKEN).await;
    assert_eq!(token.status, StatusCode::OK, "{}", token.text);

    for (harness, method, value, constraint) in [
        ("claude", "claude-setup-token", " \n", "non_empty"),
        ("claude", "anthropic-api-key", "", "non_empty"),
        (
            "claude",
            "claude-setup-token",
            "two sk-ant-halves",
            "single_token",
        ),
        ("codex", "codex-device-auth", "", "non_empty"),
        (
            "codex",
            "codex-device-auth",
            r#"{"OPENAI_API_KEY":"sk-proj-leaked"}"#,
            "chatgpt_login_file",
        ),
        (
            "codex",
            "codex-device-auth",
            r#"{"tokens": "sk-proj-leaked""#,
            "chatgpt_login_file",
        ),
    ] {
        let refused = save(&kestrel, harness, method, value).await;
        assert_eq!(
            refused.status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{}",
            refused.text
        );
        assert_eq!(refused.body["kind"], "invalid_field", "{}", refused.text);
        assert_eq!(refused.body["context"]["constraint"], constraint);
        assert!(!refused.text.contains("leaked"), "{}", refused.text);
        assert!(!refused.text.contains("halves"), "{}", refused.text);
    }
    assert!(stub.asked().is_empty());
    assert_eq!(saved(&kestrel).await, vec![token.body]);

    let login = save(&kestrel, "codex", "codex-device-auth", CHATGPT_LOGIN).await;
    assert_eq!(login.status, StatusCode::OK, "{}", login.text);
    assert_eq!(login.body["holding"]["reference"], OPERATOR);
    assert_eq!(
        login.body["holding"]["fills"],
        json!({ "kind": "file", "path": ".codex/auth.json" })
    );
    assert_eq!(login.body["authentication"]["state"], "unchecked");
    assert!(
        stub.asked().is_empty(),
        "a ChatGPT login has no check to ask"
    );

    let unknown = save(&kestrel, "claude", "codex-device-auth", CHATGPT_LOGIN).await;
    assert_eq!(unknown.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(unknown.body["context"]["constraint"], "offered");

    kestrel.teardown().await;
}
