//! Naming the install's Operator: the initial Organization, and Subscription Profile ownership
//! across a rename.

use crate::support;

use kestrel::operator;
use reqwest::StatusCode;
use serde_json::{Value, json};
use support::Kestrel;
use support::operator_requests::{
    declared, got, listed, profile_variable_of, profiles_of, requested,
};

#[tokio::test]
async fn operator_naming_is_stable_and_declares_only_the_initial_organization() {
    let kestrel = Kestrel::boot().await;
    let (status, gap) = got(&kestrel, "/operator/operator").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(gap["kind"], "setup_gap");
    assert_eq!(gap["context"]["prerequisite"], "operator");
    assert_eq!(gap["next_steps"][0]["action"], "name_operator");
    let name = json!({"name": "Jack"});
    let (first, second) = tokio::join!(
        requested(
            &kestrel,
            reqwest::Method::PUT,
            "/operator/operator",
            Some(&name)
        ),
        requested(
            &kestrel,
            reqwest::Method::PUT,
            "/operator/operator",
            Some(&name)
        )
    );
    assert!(first.0.is_success());
    assert!(second.0.is_success());
    assert_eq!(first.1, second.1);
    assert_eq!(first.1["name"], "Jack");
    assert!(
        first.1["id"]
            .as_str()
            .unwrap()
            .parse::<uuid::Uuid>()
            .is_ok()
    );
    let organizations = listed(&kestrel, operator::ORGANIZATIONS).await;
    assert_eq!(organizations.len(), 1);
    assert_eq!(organizations[0]["name"], "Jack");
    let (status, renamed) = requested(
        &kestrel,
        reqwest::Method::PUT,
        "/operator/operator",
        Some(&json!({"name": "Alex"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(renamed["id"], first.1["id"]);
    assert_eq!(renamed["name"], "Alex");
    assert_eq!(got(&kestrel, "/operator/operator").await.1, renamed);
    assert_eq!(
        listed(&kestrel, operator::ORGANIZATIONS).await,
        organizations
    );
    kestrel.teardown().await;
}

#[tokio::test]
async fn operator_naming_preserves_existing_organizations() {
    let kestrel = Kestrel::boot().await;
    declared(
        &kestrel,
        operator::ORGANIZATIONS,
        &json!({"name":"acme", "max_live_instances": 3}),
    )
    .await;
    declared(&kestrel, operator::ORGANIZATIONS, &json!({"name":"other"})).await;
    let organizations = listed(&kestrel, operator::ORGANIZATIONS).await;
    let (status, named) = requested(
        &kestrel,
        reqwest::Method::PUT,
        "/operator/operator",
        Some(&json!({"name":"Jack"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(named["name"], "Jack");
    assert_eq!(
        listed(&kestrel, operator::ORGANIZATIONS).await,
        organizations
    );
    kestrel.teardown().await;
}

#[tokio::test]
async fn operator_profile_ownership_survives_rename_and_keeps_free_text_distinct() {
    let kestrel = Kestrel::boot().await;
    declared(&kestrel, operator::ORGANIZATIONS, &json!({"name":"acme"})).await;
    let (_, generic) = declared(
        &kestrel,
        &profiles_of("acme"),
        &json!({"name":"generic", "owner":"Jack"}),
    )
    .await;
    let (status, gap) = declared(&kestrel, &profiles_of("acme"), &json!({"name":"personal"})).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(gap["kind"], "setup_gap");
    assert_eq!(gap["context"]["prerequisite"], "operator");
    assert_eq!(gap["next_steps"][0]["action"], "name_operator");
    assert_eq!(listed(&kestrel, &profiles_of("acme")).await.len(), 1);
    let (_, operator) = requested(
        &kestrel,
        reqwest::Method::PUT,
        "/operator/operator",
        Some(&json!({"name":"Jack"})),
    )
    .await;
    let (status, personal) =
        declared(&kestrel, &profiles_of("acme"), &json!({"name":"personal"})).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(personal["owner_operator"], operator["id"]);
    assert_eq!(personal["owner"], "Jack");
    let (status, explicit) = declared(
        &kestrel,
        &profiles_of("acme"),
        &json!({"name":"explicit", "owner":"Jack"}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(explicit["owner_operator"], Value::Null);
    assert_eq!(generic["owner_operator"], Value::Null);
    let (status, refusal) = declared(
        &kestrel,
        &profiles_of("acme"),
        &json!({"name":"personal", "owner":"Jack"}),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{refusal}");
    let (status, refusal) =
        declared(&kestrel, &profiles_of("acme"), &json!({"name":"generic"})).await;
    assert_eq!(status, StatusCode::CONFLICT, "{refusal}");
    let (status, held) = requested(
        &kestrel,
        reqwest::Method::PUT,
        &profile_variable_of("acme", "personal", "CLAUDE_CODE_OAUTH_TOKEN"),
        Some(&json!({"secret":"subscription-material"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    requested(
        &kestrel,
        reqwest::Method::PUT,
        "/operator/operator",
        Some(&json!({"name":"Alex"})),
    )
    .await;
    let (status, same) =
        declared(&kestrel, &profiles_of("acme"), &json!({"name":"personal"})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(same["id"], personal["id"]);
    assert_eq!(same["owner_operator"], operator["id"]);
    assert_eq!(same["owner"], "Alex");
    let (status, refusal) = declared(
        &kestrel,
        &profiles_of("acme"),
        &json!({"name":"personal", "owner":"Alex"}),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{refusal}");
    let (status, refusal) =
        declared(&kestrel, &profiles_of("acme"), &json!({"name":"generic"})).await;
    assert_eq!(status, StatusCode::CONFLICT, "{refusal}");
    let kestrel = kestrel.teardown().await.restart().await;
    assert_eq!(
        got(&kestrel, "/operator/operator").await.1["id"],
        operator["id"]
    );
    let profiles = listed(&kestrel, &profiles_of("acme")).await;
    let owned = profiles
        .iter()
        .find(|profile| profile["name"] == "personal")
        .unwrap();
    assert_eq!(owned["id"], personal["id"]);
    assert_eq!(owned["owner_operator"], operator["id"]);
    assert_eq!(owned["holds"], json!([held]));
    for name in ["generic", "explicit"] {
        let profile = profiles
            .iter()
            .find(|profile| profile["name"] == name)
            .unwrap();
        assert_eq!(profile["owner"], "Jack");
        assert_eq!(profile["owner_operator"], Value::Null);
    }
    kestrel.teardown().await;
}
