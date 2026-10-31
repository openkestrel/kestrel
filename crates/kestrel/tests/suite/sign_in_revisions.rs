use reqwest::StatusCode;
use serde_json::{Value, json};

use crate::support::provider_stub::{Answer, ProviderStub};
use crate::support::sign_in::{
    ANTHROPIC_KEY, SIGN_INS, answered, booted, generic_credential, one, save, saved,
};

#[tokio::test]
async fn every_accepted_write_mints_a_revision_and_a_generic_one_is_never_checked() {
    let stub = ProviderStub::start();
    let kestrel = booted(&stub).await;
    let revision = |listed: &[Value]| {
        one(listed, "claude", "anthropic-api-key")["revision"]
            .as_i64()
            .expect("a revision")
    };

    let checked = save(&kestrel, "claude", "anthropic-api-key", ANTHROPIC_KEY).await;
    let first = checked.body["revision"].as_i64().expect("a revision");

    generic_credential(&kestrel, "sk-ant-api03-pasted-by-hand").await;
    let listed = saved(&kestrel).await;
    let second = revision(&listed);
    assert!(second > first, "{second} after {first}");
    let generic = &one(&listed, "claude", "anthropic-api-key")["authentication"];
    assert_eq!(generic["state"], "unchecked");
    assert_eq!(generic["source"], "generic_write");
    assert_eq!(stub.asked().len(), 1, "a generic write asks no provider");

    let forgotten = reqwest::Client::new()
        .delete(format!(
            "{}/operator/organizations/ada/credentials/ANTHROPIC_API_KEY",
            kestrel.operator()
        ))
        .send()
        .await
        .expect("the operator boundary answers");
    assert_eq!(forgotten.status(), StatusCode::NO_CONTENT);
    assert!(saved(&kestrel).await.is_empty());

    generic_credential(&kestrel, "sk-ant-api03-pasted-again").await;
    let third = revision(&saved(&kestrel).await);
    assert!(third > second, "a write after a delete is new material");

    let resaved = save(&kestrel, "claude", "anthropic-api-key", ANTHROPIC_KEY).await;
    assert!(resaved.body["revision"].as_i64().expect("a revision") > third);

    kestrel.teardown().await;
}

#[tokio::test]
async fn material_replaced_while_it_was_being_checked_stands() {
    let stub = ProviderStub::start();
    let kestrel = booted(&stub).await;

    stub.answer(Answer::Held(200, r#"{"data":[]}"#.to_owned()));
    let checking = {
        let operator = kestrel.operator();
        tokio::spawn(async move {
            answered(
                reqwest::Client::new()
                    .put(format!("{operator}{SIGN_INS}/claude/anthropic-api-key"))
                    .json(&json!({ "value": ANTHROPIC_KEY }))
                    .send()
                    .await
                    .expect("the operator boundary answers"),
            )
            .await
        })
    };
    stub.until_asked(1).await;
    generic_credential(&kestrel, "sk-ant-api03-replacement").await;
    let replacement = saved(&kestrel).await;
    stub.release();

    let late = checking.await.expect("the save finished");
    assert_eq!(late.status, StatusCode::CONFLICT, "{}", late.text);
    assert_eq!(late.body["kind"], "state_conflict");
    assert_eq!(late.body["context"]["state"], "replaced");
    assert_eq!(late.body["context"]["resource"], "provider_credential");
    assert_eq!(
        saved(&kestrel).await,
        replacement,
        "the late check verified nothing"
    );

    kestrel.teardown().await;
}
