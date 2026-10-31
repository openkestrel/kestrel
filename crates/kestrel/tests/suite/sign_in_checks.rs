use reqwest::StatusCode;
use serde_json::{Value, json};

use crate::support::provider_stub::{self, Answer, ProviderStub};
use crate::support::sign_in::{ANTHROPIC_KEY, OPERATOR, booted, one, save, saved};

#[tokio::test]
async fn a_key_the_provider_rejects_keeps_what_was_saved_before() {
    let stub = ProviderStub::start();
    let kestrel = booted(&stub).await;
    let first = save(&kestrel, "claude", "anthropic-api-key", ANTHROPIC_KEY).await;
    assert_eq!(first.status, StatusCode::OK, "{}", first.text);

    stub.answer(provider_stub::rejected());
    let rejected = save(
        &kestrel,
        "claude",
        "anthropic-api-key",
        "sk-ant-api03-mistyped",
    )
    .await;

    assert_eq!(
        rejected.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        rejected.text
    );
    assert!(!rejected.text.contains("mistyped"), "{}", rejected.text);
    assert!(
        !rejected.text.contains("invalid x-api-key"),
        "{}",
        rejected.text
    );
    assert_eq!(rejected.body["kind"], "credential_rejected");
    assert_eq!(rejected.body["field"], "value");
    assert_eq!(
        rejected.body["context"],
        json!({
            "organization": OPERATOR,
            "harness": "claude",
            "method": "anthropic-api-key",
            "provider": "anthropic",
            "status": 401,
            "provider_error": "authentication_error",
        })
    );
    assert_eq!(rejected.body["next_steps"][0]["action"], "sign_in");
    assert_eq!(
        one(&saved(&kestrel).await, "claude", "anthropic-api-key"),
        &first.body
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_refusal_about_permission_saves_the_key_unchecked() {
    let stub = ProviderStub::start();
    let kestrel = booted(&stub).await;

    stub.answer(provider_stub::forbidden());
    let key = save(&kestrel, "claude", "anthropic-api-key", ANTHROPIC_KEY).await;

    assert_eq!(key.status, StatusCode::OK, "{}", key.text);
    assert_eq!(key.body["authentication"]["state"], "unchecked");
    assert_eq!(key.body["authentication"]["source"], "provider_check");
    assert_eq!(
        key.body["authentication"]["provider_check"],
        json!({
            "provider": "anthropic",
            "outcome": "inconclusive",
            "status": 403,
            "provider_error": "permission_error",
        })
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_provider_that_cannot_answer_leaves_the_key_saved_unchecked_and_says_so() {
    let stub = ProviderStub::start();
    let kestrel = booted(&stub).await;

    for (answer, status) in [
        (provider_stub::overloaded(), json!(529)),
        (Answer::Silent, Value::Null),
    ] {
        stub.answer(answer);
        let partial = save(&kestrel, "claude", "anthropic-api-key", ANTHROPIC_KEY).await;

        assert_eq!(
            partial.status,
            StatusCode::SERVICE_UNAVAILABLE,
            "{}",
            partial.text
        );
        assert!(!partial.text.contains(ANTHROPIC_KEY), "{}", partial.text);
        assert_eq!(partial.body["kind"], "unavailable");
        assert_eq!(partial.body["context"]["service"], "anthropic");
        let partially = &partial.body["context"]["saved"];
        assert_eq!(partially["authentication"]["state"], "unchecked");
        assert_eq!(
            partially["authentication"]["provider_check"]["outcome"],
            "unavailable"
        );
        assert_eq!(
            partially["authentication"]["provider_check"]["status"],
            status
        );
        assert_eq!(
            one(&saved(&kestrel).await, "claude", "anthropic-api-key"),
            partially,
            "the partial save is what both Clients read"
        );
    }

    kestrel.teardown().await;
}
