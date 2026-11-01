//! Provider credentials set, listed and forgotten without ever being said back.

use crate::support;

use reqwest::StatusCode;
use serde_json::{Value, json};
use support::Kestrel;
use support::operator_requests::{
    client, client_given, credential_of, credentials_of, failed, listed, listed_nothing, recorded,
    requested, succeeded,
};

#[tokio::test]
async fn a_client_sets_lists_and_forgets_provider_credentials_without_saying_them() {
    let kestrel = Kestrel::boot().await;
    let organization = kestrel.declare_organization("acme").await;
    let secret = "sk-kestrel-should-never-say-this";

    let set = client_given(
        &kestrel,
        &[
            "credential",
            "set",
            "ANTHROPIC_API_KEY",
            "--organization",
            "acme",
            "--json",
        ],
        Some(&format!("{secret}\n")),
    )
    .await;
    let held = recorded(&set);
    let listed = client(
        &kestrel,
        &["credential", "list", "--organization", "acme", "--json"],
    )
    .await;

    assert_eq!(held.len(), 1);
    assert_eq!(held[0]["variable"], "ANTHROPIC_API_KEY");
    assert_eq!(recorded(&listed), held);
    for said in [&set, &listed] {
        assert!(
            !said.out.join("\n").contains(secret) && !said.err.contains(secret),
            "the client spelled the credential out"
        );
    }
    assert_eq!(
        kestrel.provider_credentials_held(&organization).await[0].variable,
        "ANTHROPIC_API_KEY"
    );

    let forgotten = client(
        &kestrel,
        &[
            "credential",
            "forget",
            "ANTHROPIC_API_KEY",
            "--organization",
            "acme",
        ],
    )
    .await;
    assert!(succeeded(&forgotten).is_empty());
    assert!(listed_nothing(&kestrel, &credentials_of("acme")).await);
    assert!(
        kestrel
            .provider_credentials_held(&organization)
            .await
            .is_empty()
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_credential_answers_back_what_it_is_read_from_and_never_its_value() {
    let kestrel = Kestrel::boot().await;
    kestrel.declare_organization("acme").await;

    let (status, held) = requested(
        &kestrel,
        reqwest::Method::PUT,
        &credential_of("acme", "OPENAI_API_KEY"),
        Some(&json!({ "secret": "the-first-key" })),
    )
    .await;
    let (replaced, again) = requested(
        &kestrel,
        reqwest::Method::PUT,
        &credential_of("acme", "OPENAI_API_KEY"),
        Some(&json!({ "secret": "the-second-key" })),
    )
    .await;
    let listed = listed(&kestrel, &credentials_of("acme")).await;

    assert_eq!(status, StatusCode::OK, "{held}");
    assert_eq!(replaced, StatusCode::OK, "{again}");
    assert_eq!(listed, vec![again.clone()]);
    for answered in [&held, &again, &Value::Array(listed)] {
        let answered = answered.to_string();
        assert!(
            !answered.contains("the-first-key") && !answered.contains("the-second-key"),
            "the boundary answered a secret: {answered}"
        );
    }

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_credential_no_process_could_carry_or_nobody_holds_is_refused() {
    let kestrel = Kestrel::boot().await;
    kestrel.declare_organization("acme").await;

    let (unnamed, refusal) = requested(
        &kestrel,
        reqwest::Method::PUT,
        &credential_of("acme", "NOT-A-VARIABLE"),
        Some(&json!({ "secret": "a-key" })),
    )
    .await;
    let (reserved, reason) = requested(
        &kestrel,
        reqwest::Method::PUT,
        &credential_of("acme", "KESTREL_PROVIDER_KEY"),
        Some(&json!({ "secret": "a-key" })),
    )
    .await;
    let (empty, _) = requested(
        &kestrel,
        reqwest::Method::PUT,
        &credential_of("acme", "A_KEY"),
        Some(&json!({ "secret": "" })),
    )
    .await;
    let (nowhere, _) = requested(
        &kestrel,
        reqwest::Method::PUT,
        &credential_of("globex", "A_KEY"),
        Some(&json!({ "secret": "a-key" })),
    )
    .await;
    let (unheld, _) = requested(
        &kestrel,
        reqwest::Method::DELETE,
        &credential_of("acme", "A_KEY"),
        None,
    )
    .await;
    let nothing_on_stdin = client_given(
        &kestrel,
        &["credential", "set", "A_KEY", "--organization", "acme"],
        Some(""),
    )
    .await;
    let forgetting = client(
        &kestrel,
        &["credential", "forget", "A_KEY", "--organization", "acme"],
    )
    .await;

    assert_eq!(unnamed, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(reserved, StatusCode::UNPROCESSABLE_ENTITY, "{reason}");
    assert!(
        reason["message"]
            .as_str()
            .expect("a message")
            .contains("reserved")
    );
    assert!(
        refusal["message"]
            .as_str()
            .expect("a message")
            .contains("NOT-A-VARIABLE"),
        "{refusal}"
    );
    assert_eq!(empty, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(nowhere, StatusCode::NOT_FOUND);
    assert_eq!(unheld, StatusCode::NOT_FOUND);
    assert!(failed(&nothing_on_stdin).contains("standard input"));
    assert!(failed(&forgetting).contains("holds no provider credential named A_KEY"));
    assert!(listed_nothing(&kestrel, &credentials_of("acme")).await);

    kestrel.teardown().await;
}
