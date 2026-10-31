use reqwest::StatusCode;
use serde_json::json;

use crate::support::provider_stub::ProviderStub;
use crate::support::sign_in::{CLAUDE_TOKEN, OPERATOR, answered, booted, put, save, saved};

#[tokio::test]
async fn several_operator_profiles_are_refused_rather_than_one_chosen() {
    let stub = ProviderStub::start();
    let kestrel = booted(&stub).await;
    for name in ["work", "play"] {
        let declared = answered(
            reqwest::Client::new()
                .post(format!(
                    "{}/operator/organizations/ada/profiles",
                    kestrel.operator()
                ))
                .json(&json!({ "name": name }))
                .send()
                .await
                .expect("the operator boundary answers"),
        )
        .await;
        assert_eq!(declared.status, StatusCode::CREATED, "{}", declared.text);
    }

    let ambiguous = save(&kestrel, "claude", "claude-setup-token", CLAUDE_TOKEN).await;

    assert_eq!(ambiguous.status, StatusCode::CONFLICT, "{}", ambiguous.text);
    assert_eq!(ambiguous.body["kind"], "state_conflict");
    assert_eq!(
        ambiguous.body["context"]["state"],
        "several_operator_profiles"
    );
    assert!(saved(&kestrel).await.is_empty());

    let held = put(
        &kestrel,
        "/operator/organizations/ada/profiles/play/variables/CLAUDE_CODE_OAUTH_TOKEN",
        json!({ "secret": "sk-ant-oat01-earlier" }),
    )
    .await;
    assert_eq!(held.status, StatusCode::OK, "{}", held.text);
    let replaced = save(&kestrel, "claude", "claude-setup-token", CLAUDE_TOKEN).await;
    assert_eq!(replaced.status, StatusCode::OK, "{}", replaced.text);
    assert_eq!(
        replaced.body["holding"]["reference"], "play",
        "the one already holding a Claude subscription"
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_profile_someone_else_owns_is_never_chosen() {
    let stub = ProviderStub::start();
    let kestrel = booted(&stub).await;
    kestrel
        .declare_profile(OPERATOR, OPERATOR, OPERATOR)
        .await
        .expect("a free-text owner, even one matching the Operator's name");

    let token = save(&kestrel, "claude", "claude-setup-token", CLAUDE_TOKEN).await;

    assert_eq!(token.status, StatusCode::CONFLICT, "{}", token.text);
    assert_eq!(token.body["context"]["state"], "owned_by_another");
    let profiles = kestrel.profiles(OPERATOR).await;
    assert_eq!(profiles.len(), 1);
    assert!(profiles[0].1.is_empty(), "nothing was held in theirs");

    kestrel.teardown().await;
}
