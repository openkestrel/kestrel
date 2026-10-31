use reqwest::StatusCode;
use serde_json::{Value, json};

use crate::support::provider_stub::ProviderStub;
use crate::support::sign_in::{
    ANTHROPIC_KEY, CLAUDE_TOKEN, OPERATOR, booted, generic_credential, one, save, saved,
};

#[tokio::test]
async fn model_use_is_evidence_for_one_harness_model_and_revision_only() {
    let stub = ProviderStub::start();
    let kestrel = booted(&stub).await;
    let key = save(&kestrel, "claude", "anthropic-api-key", ANTHROPIC_KEY).await;
    let revision = key.body["revision"].as_i64().expect("a revision");
    let database = kestrel.database();
    database
        .record_model_use(revision, "claude", "claude-opus", "worked")
        .await;
    database
        .record_model_use(revision, "opencode", "anthropic/claude-opus", "not_covered")
        .await;
    database
        .record_model_use(revision, "opencode", "anthropic/claude-haiku", "worked")
        .await;

    let listed = saved(&kestrel).await;
    let claude = one(&listed, "claude", "anthropic-api-key");
    let opencode = one(&listed, "opencode", "anthropic-api-key");
    let results = |item: &Value| -> Vec<(String, String)> {
        item["model_use"]
            .as_array()
            .expect("model use")
            .iter()
            .map(|used| {
                (
                    used["model"].as_str().expect("a model").to_owned(),
                    used["result"].as_str().expect("a result").to_owned(),
                )
            })
            .collect()
    };
    assert_eq!(
        results(claude),
        [("claude-opus".to_owned(), "worked".to_owned())]
    );
    assert_eq!(
        results(opencode),
        [
            ("anthropic/claude-haiku".to_owned(), "worked".to_owned()),
            ("anthropic/claude-opus".to_owned(), "not_covered".to_owned()),
        ]
    );
    for item in [claude, opencode] {
        assert_eq!(item["authentication"]["state"], "credential_accepted");
    }

    generic_credential(&kestrel, "sk-ant-api03-replacement").await;
    database
        .record_model_use(revision, "claude", "claude-sonnet", "worked")
        .await;
    let replaced = saved(&kestrel).await;
    assert!(
        results(one(&replaced, "claude", "anthropic-api-key")).is_empty(),
        "evidence about the earlier revision is not about this one"
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn cli_generic_profile_set_replaces_catalogue_evidence_without_checking_or_model_usage() {
    use crate::support::client::{self, Invocation};

    let stub = ProviderStub::start();
    let kestrel = booted(&stub).await;
    let imported = save(&kestrel, "claude", "claude-setup-token", CLAUDE_TOKEN).await;
    assert_eq!(imported.status, StatusCode::OK, "{}", imported.text);
    let revision = imported.body["revision"].as_i64().expect("a revision");
    kestrel
        .database()
        .record_authentication(revision, "authentication_failed")
        .await;
    kestrel
        .database()
        .record_model_use(revision, "claude", "claude-sonnet", "worked")
        .await;
    let before = saved(&kestrel).await;
    let before = one(&before, "claude", "claude-setup-token");
    assert_eq!(before["authentication"]["state"], "authentication_failed");
    assert_eq!(before["model_use"].as_array().unwrap().len(), 1);

    let replaced = client::ran_by(
        &kestrel,
        &[
            "profile",
            "set",
            OPERATOR,
            "--variable",
            "CLAUDE_CODE_OAUTH_TOKEN",
            "--json",
        ],
        Invocation::default().given("generic-private-value\n"),
    )
    .await;
    assert!(replaced.status.success(), "{}", replaced.err);
    assert!(!replaced.out.join("\n").contains("generic-private-value"));
    assert!(!replaced.err.contains("generic-private-value"));
    let after = saved(&kestrel).await;
    let after = one(&after, "claude", "claude-setup-token");
    assert!(after["revision"].as_i64().unwrap() > revision);
    assert_eq!(after["authentication"]["state"], "unchecked");
    assert_eq!(after["authentication"]["source"], "generic_write");
    assert_eq!(after["model_use"], json!([]));
    assert!(
        stub.asked().is_empty(),
        "generic writes make no provider call"
    );
    kestrel.teardown().await;
}
