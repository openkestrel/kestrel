//! Rotating an Integration's webhook secret and replacing its private key, and the registrations
//! that describe no usable Integration.

use crate::support;

use jiff::SignedDuration;
use kestrel::domain::Direction;
use kestrel::operator;
use reqwest::StatusCode;
use serde_json::{Value, json};
use support::github_stub::{self, GithubStub, ScriptedResponse};
use support::operator_requests::{
    client, declared, failed, got, integration_of, integrations_of, listed, requested,
};
use support::{Kestrel, PRIVATE_KEY, REPLACEMENT_PRIVATE_KEY};

#[tokio::test]
async fn a_client_starts_reads_and_resumes_a_webhook_secret_rotation_through_one_interface() {
    let kestrel = Kestrel::boot().await;
    let stub = GithubStub::start();
    kestrel.declare_organization("acme").await;
    kestrel
        .register_signed_github(
            "acme",
            "hub",
            "jtmthf/kestrel",
            &stub.base_url(),
            "the-first-secret",
        )
        .await;
    let rotation = operator::INTEGRATION_WEBHOOK_SECRET_ROTATION
        .replace("{organization}", "acme")
        .replace("{integration}", "hub");
    let resume = operator::INTEGRATION_WEBHOOK_SECRET_ROTATION_RESUME
        .replace("{organization}", "acme")
        .replace("{integration}", "hub");
    let resuming = json!({
        "action": "resume_integration_webhook_secret_rotation",
        "organization": "acme",
        "integration": "hub",
        "retry_after_seconds": null,
    });
    let mut answers = Vec::new();

    stub.script_answer(
        "PATCH",
        "/app/hook/config",
        ScriptedResponse::answering(502),
    );
    let (status, unanswered) =
        requested(&kestrel, reqwest::Method::POST, &rotation, Some(&json!({}))).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{unanswered}");
    assert_eq!(unanswered["kind"], "unavailable");
    assert_eq!(unanswered["context"]["service"], "github");
    assert_eq!(
        unanswered["context"]["operation"],
        "rotate_integration_webhook_secret"
    );
    assert_eq!(unanswered["next_steps"], json!([resuming]));
    answers.push(unanswered);

    let (_, shown) = got(&kestrel, &integration_of("acme", "hub")).await;
    let unfinished = &shown["webhook_secret_rotation"];
    assert!(unfinished["started_at"].is_string(), "{shown}");
    assert!(unfinished["attempted_at"].is_string(), "{shown}");
    assert_eq!(unfinished["diagnostic"]["kind"], "unavailable");
    assert_eq!(unfinished["diagnostic"]["next_steps"], json!([resuming]));
    answers.push(shown.clone());

    let (status, again) =
        requested(&kestrel, reqwest::Method::POST, &rotation, Some(&json!({}))).await;
    assert_eq!(status, StatusCode::CONFLICT, "{again}");
    assert_eq!(again["context"]["state"], "rotating_webhook_secret");
    assert_eq!(again["next_steps"][1], resuming);
    answers.push(again);

    let (status, resumed) = requested(&kestrel, reqwest::Method::POST, &resume, None).await;
    assert_eq!(status, StatusCode::OK, "{resumed}");
    assert_eq!(resumed["webhook_secret_rotation"], Value::Null);
    assert_eq!(resumed["revision"], shown["revision"]);
    answers.push(resumed);

    kestrel.declare_organization("elsewhere").await;
    kestrel
        .register_signed_github(
            "elsewhere",
            "their-connection",
            "elsewhere/plans",
            &stub.base_url(),
            "their-secret",
        )
        .await;
    let (status, shared) =
        requested(&kestrel, reqwest::Method::POST, &rotation, Some(&json!({}))).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{shared}");
    assert_eq!(shared["kind"], "forbidden_action");
    assert_eq!(shared["context"]["constraint"], "shared_app");
    assert!(!shared.to_string().contains("elsewhere"), "{shared}");
    assert!(!shared.to_string().contains("their-connection"), "{shared}");
    answers.push(shared);

    let secrets: Vec<String> = stub
        .requests()
        .iter()
        .filter(|request| request.method == "PATCH")
        .map(|request| {
            serde_json::from_str::<Value>(&request.body).expect("JSON")["secret"]
                .as_str()
                .expect("a secret")
                .to_owned()
        })
        .collect();
    assert_eq!(secrets.len(), 2);
    for answer in answers {
        let answer = answer.to_string();
        for secret in &secrets {
            assert!(!answer.contains(secret.as_str()), "{answer}");
        }
        assert!(!answer.contains("the-first-secret"), "{answer}");
    }

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_client_replaces_a_private_key_and_is_told_how_to_repair_one_github_refuses() {
    let kestrel = Kestrel::boot().await;
    let stub = GithubStub::start();
    kestrel.declare_organization("acme").await;
    let registered = kestrel
        .register_integration(
            "acme",
            "hub",
            "jtmthf/kestrel",
            &stub.base_url(),
            &[Direction::Inbound, Direction::Outbound],
            SignedDuration::from_hours(1),
        )
        .await;
    let private_key = operator::INTEGRATION_PRIVATE_KEY
        .replace("{organization}", "acme")
        .replace("{integration}", "hub");
    let replacing = json!({ "private_key": REPLACEMENT_PRIVATE_KEY });
    let mut answers = Vec::new();

    stub.script_answer("POST", "/access_tokens", ScriptedResponse::answering(401));
    let (status, rejected) = requested(
        &kestrel,
        reqwest::Method::PUT,
        &private_key,
        Some(&replacing),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{rejected}");
    assert_eq!(rejected["kind"], "invalid_field");
    assert_eq!(rejected["field"], "private_key");
    assert_eq!(rejected["context"]["constraint"], "accepted_by_app");
    assert_eq!(rejected["next_steps"][0]["action"], "correct_field");
    assert_eq!(
        rejected["next_steps"][0]["operation"],
        "replace_integration_private_key"
    );
    answers.push(rejected);

    stub.script_answer("POST", "/access_tokens", github_stub::rate_limited());
    let (status, limited) = requested(
        &kestrel,
        reqwest::Method::PUT,
        &private_key,
        Some(&replacing),
    )
    .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{limited}");
    assert_eq!(limited["kind"], "unavailable");
    assert_eq!(limited["context"]["service"], "github");
    assert_eq!(limited["context"]["saved"], Value::Null);
    assert!(
        limited["context"]["retry_after_seconds"].is_i64(),
        "{limited}"
    );
    assert_eq!(
        limited["next_steps"],
        json!([{
            "action": "replace_integration_private_key",
            "organization": "acme",
            "integration": "hub",
            "retry_after_seconds": limited["context"]["retry_after_seconds"],
        }])
    );
    answers.push(limited);

    stub.script_answer(
        "GET",
        "/repos/jtmthf/kestrel",
        ScriptedResponse::answering(404),
    );
    let (status, unreachable) = requested(
        &kestrel,
        reqwest::Method::PUT,
        &private_key,
        Some(&replacing),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{unreachable}");
    assert_eq!(unreachable["context"]["state"], "repository_unreachable");
    answers.push(unreachable);

    let (status, stale) = requested(
        &kestrel,
        reqwest::Method::PUT,
        &private_key,
        Some(&json!({
            "private_key": REPLACEMENT_PRIVATE_KEY,
            "revision": registered.revision + 1,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{stale}");
    assert_eq!(stale["context"]["state"], "revised");
    assert_eq!(stale["next_steps"][0]["action"], "inspect_resource");
    assert_eq!(
        stale["next_steps"][1]["action"],
        "replace_integration_private_key"
    );
    answers.push(stale);

    let (_, shown) = got(&kestrel, &integration_of("acme", "hub")).await;
    assert_eq!(shown["revision"], registered.revision);

    let (status, replaced) = requested(
        &kestrel,
        reqwest::Method::PUT,
        &private_key,
        Some(&json!({
            "private_key": REPLACEMENT_PRIVATE_KEY,
            "revision": registered.revision,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{replaced}");
    assert_eq!(replaced["id"], shown["id"]);
    assert_eq!(replaced["repository"], "jtmthf/kestrel");
    assert!(replaced["revision"].as_i64() > shown["revision"].as_i64());
    answers.push(replaced);
    answers.push(got(&kestrel, &integration_of("acme", "hub")).await.1);

    for answer in answers {
        let answer = answer.to_string();
        assert!(!answer.contains("PRIVATE KEY"), "{answer}");
        assert!(
            !answer.contains(github_stub::INSTALLATION_TOKEN),
            "{answer}"
        );
    }

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_registration_that_describes_no_usable_integration_is_refused() {
    let kestrel = Kestrel::boot().await;
    kestrel.declare_organization("acme").await;
    let webhook = json!({ "kind": "webhook", "name": "ci", "secret": "a-shared-secret" });
    declared(&kestrel, &integrations_of("acme"), &webhook).await;

    let refusals = [
        (
            integrations_of("acme"),
            webhook.clone(),
            StatusCode::CONFLICT,
        ),
        (
            integrations_of("globex"),
            webhook.clone(),
            StatusCode::NOT_FOUND,
        ),
        (
            integrations_of("acme"),
            json!({ "kind": "pager", "name": "pd" }),
            StatusCode::BAD_REQUEST,
        ),
        (
            integrations_of("acme"),
            json!({ "kind": "webhook", "name": "", "secret": "a-shared-secret" }),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            integrations_of("acme"),
            json!({ "kind": "webhook", "name": "out", "secret": "s", "carries": ["outbound"] }),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            integrations_of("acme"),
            json!({
                "kind": "github",
                "name": "hub",
                "repository": "kestrel",
                "app_id": 1,
                "installation": 2,
                "private_key": PRIVATE_KEY,
            }),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            integrations_of("acme"),
            json!({
                "kind": "github",
                "name": "hub",
                "repository": "jtmthf/kestrel",
                "app_id": 1,
                "installation": 2,
                "private_key": PRIVATE_KEY,
                "interval": "whenever",
            }),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            integrations_of("acme"),
            json!({
                "kind": "github",
                "name": "hub",
                "repository": "jtmthf/kestrel",
                "app_id": 1,
                "installation": 2,
                "private_key": PRIVATE_KEY,
                "carries": [],
            }),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
    ];
    for (path, registration, expected) in &refusals {
        let (status, refusal) = declared(&kestrel, path, registration).await;
        assert_eq!(status, *expected, "{registration} was answered {refusal}");
        assert!(
            !refusal.to_string().contains(PRIVATE_KEY),
            "a refusal spelled the private key out: {refusal}"
        );
    }
    let taken = client(
        &kestrel,
        &[
            "integration",
            "register",
            "webhook",
            "ci",
            "--organization",
            "acme",
            "--secret",
            "another",
        ],
    )
    .await;

    assert!(failed(&taken).contains("already has an integration named ci"));
    assert_eq!(listed(&kestrel, &integrations_of("acme")).await.len(), 1);

    kestrel.teardown().await;
}
