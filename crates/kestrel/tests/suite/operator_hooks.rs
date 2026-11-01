//! Changing and reading an Integration's webhook URL.

use crate::support;

use reqwest::StatusCode;
use serde_json::{Value, json};
use support::Kestrel;
use support::github_stub::{GithubStub, ScriptedResponse};
use support::operator_requests::requested;

#[tokio::test]
async fn a_webhook_url_change_sets_only_the_url_and_keeps_polling() {
    let kestrel = Kestrel::boot_serving_alone().await;
    let stub = GithubStub::start();
    kestrel.declare_organization("acme").await;
    let integration = kestrel
        .register_signed_github(
            "acme",
            "hub",
            "jtmthf/kestrel",
            &stub.base_url(),
            "kept-secret",
        )
        .await;
    stub.script_answer(
        "PATCH",
        "/app/hook/config",
        ScriptedResponse::answering(200),
    );
    let path = "/operator/organizations/acme/integrations/hub/webhook";
    let (status, changed) = requested(
        &kestrel,
        reqwest::Method::PUT,
        path,
        Some(&json!({"base": "https://hooks.example.com", "revision": integration.revision})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{changed}");
    assert_eq!(changed["revision"], integration.revision + 1);
    let patches: Vec<_> = stub
        .requests()
        .into_iter()
        .filter(|r| r.method == "PATCH" && r.url == "/app/hook/config")
        .map(|r| serde_json::from_str::<Value>(&r.body).unwrap())
        .collect();
    assert_eq!(
        patches,
        vec![json!({"url": format!("https://hooks.example.com{}", integration.webhook_path())})]
    );
    let current = kestrel.integration("acme", "hub").await;
    assert_eq!(
        current.deliveries_read_from,
        integration.deliveries_read_from
    );
    assert!(current.poll_due_at.is_some());
    let (status, refused) = requested(
        &kestrel,
        reqwest::Method::PUT,
        path,
        Some(&json!({"base": "https://other.example.com", "revision": integration.revision})),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{refused}");
    kestrel.teardown().await;
}

#[tokio::test]
async fn hook_url_maintenance_refuses_shared_retired_and_rotating_apps() {
    let kestrel = Kestrel::boot_serving_alone().await;
    let stub = GithubStub::start();
    kestrel.declare_organization("acme").await;
    kestrel.declare_organization("other").await;
    kestrel
        .register_signed_github(
            "acme",
            "hub",
            "jtmthf/kestrel",
            &stub.base_url(),
            "kept-secret",
        )
        .await;
    kestrel
        .register_signed_github(
            "other",
            "private",
            "jtmthf/kestrel",
            &stub.base_url(),
            "other-secret",
        )
        .await;
    let other = "/operator/organizations/other/integrations/private";
    let path = "/operator/organizations/acme/integrations/hub/webhook";
    let body = json!({"base": "https://hooks.example.com"});
    requested(
        &kestrel,
        reqwest::Method::POST,
        &format!("{other}/disable"),
        None,
    )
    .await;
    let (status, refused) = requested(&kestrel, reqwest::Method::PUT, path, Some(&body)).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{refused}");
    assert_eq!(refused["context"]["constraint"], "shared_app");
    assert!(!refused.to_string().contains("private"));
    assert!(!stub.requests().iter().any(|r| r.method == "PATCH"));
    requested(
        &kestrel,
        reqwest::Method::POST,
        &format!("{other}/retire"),
        Some(&json!({})),
    )
    .await;
    stub.script_answer(
        "PATCH",
        "/app/hook/config",
        ScriptedResponse::answering(502),
    );
    requested(
        &kestrel,
        reqwest::Method::POST,
        "/operator/organizations/acme/integrations/hub/webhook-secret-rotation",
        Some(&json!({})),
    )
    .await;
    let (status, refused) = requested(&kestrel, reqwest::Method::PUT, path, Some(&body)).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{refused}");
    assert_eq!(
        refused["next_steps"][0]["action"],
        "resume_integration_webhook_secret_rotation"
    );
    requested(
        &kestrel,
        reqwest::Method::POST,
        "/operator/organizations/acme/integrations/hub/retire",
        Some(&json!({})),
    )
    .await;
    let (status, refused) = requested(&kestrel, reqwest::Method::PUT, path, Some(&body)).await;
    assert_eq!(status, StatusCode::CONFLICT, "{refused}");
    assert_eq!(refused["context"]["state"], "retired");
    assert_eq!(
        stub.requests()
            .iter()
            .filter(|r| r.method == "PATCH")
            .count(),
        1
    );
    kestrel.teardown().await;
}

#[tokio::test]
async fn hook_reads_show_delivery_results_without_claiming_configuration_proves_reachability() {
    let kestrel = Kestrel::boot_serving_alone().await;
    let stub = GithubStub::start();
    kestrel.declare_organization("acme").await;
    kestrel
        .register_signed_github(
            "acme",
            "hub",
            "jtmthf/kestrel",
            &stub.base_url(),
            "kept-secret",
        )
        .await;
    let path = "/operator/organizations/acme/integrations/hub/webhook";
    stub.script_answer(
        "GET",
        "/app/hook/config",
        ScriptedResponse::ok(
            r#"{"url":"https://hooks.example.com/webhooks/id","secret":"never-return-this"}"#,
        ),
    );
    stub.script_answer("GET", "/app/hook/deliveries?per_page=100", ScriptedResponse::ok(
        r#"[{"guid":"failed","delivered_at":"2026-10-16T00:00:00Z","status_code":502,"installation_id":2,"repository_id":1},{"guid":"other-repository","delivered_at":"2026-10-16T00:00:00Z","status_code":200,"installation_id":2,"repository_id":99}]"#));
    let (status, shown) = requested(&kestrel, reqwest::Method::GET, path, None).await;
    assert_eq!(status, StatusCode::OK, "{shown}");
    assert_eq!(
        shown["deliveries"],
        json!([{"guid":"failed","delivered_at":"2026-10-16T00:00:00Z","status_code":502}])
    );
    assert!(!shown.to_string().contains("never-return-this"));
    assert!(!shown.to_string().contains("other-repository"));
    for base in [
        None,
        Some("https://localhost"),
        Some("http://example.com"),
        Some("https://example.com/?secret=x"),
    ] {
        let body = base.map_or_else(|| json!({}), |base| json!({"base":base}));
        let (status, refused) = requested(&kestrel, reqwest::Method::PUT, path, Some(&body)).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{refused}");
        assert_eq!(refused["kind"], "invalid_field");
    }
    assert!(!stub.requests().iter().any(|r| r.method == "PATCH"));
    kestrel.teardown().await;
}

#[tokio::test]
async fn an_unconfirmed_hook_url_survives_restart_and_serializes_secret_rotation() {
    let kestrel = Kestrel::boot_serving_alone().await;
    let stub = GithubStub::start();
    kestrel.declare_organization("acme").await;
    let integration = kestrel
        .register_signed_github(
            "acme",
            "hub",
            "jtmthf/kestrel",
            &stub.base_url(),
            "kept-secret",
        )
        .await;
    let path = "/operator/organizations/acme/integrations/hub/webhook";
    let base = json!({"base":"https://hooks.example.com"});
    stub.script_answer(
        "PATCH",
        "/app/hook/config",
        ScriptedResponse::answering(502),
    );
    let (status, refused) = requested(&kestrel, reqwest::Method::PUT, path, Some(&base)).await;
    assert_eq!(status, StatusCode::CONFLICT, "{refused}");
    assert_eq!(refused["context"]["state"], "hook_unconfirmed");
    let kestrel = kestrel.kill_and_restart().await;
    let rotation = "/operator/organizations/acme/integrations/hub/webhook-secret-rotation";
    let (status, refused) =
        requested(&kestrel, reqwest::Method::POST, rotation, Some(&json!({}))).await;
    assert_eq!(status, StatusCode::CONFLICT, "{refused}");
    assert_eq!(refused["context"]["state"], "hook_url_update_pending");
    let (status, refused) = requested(
        &kestrel,
        reqwest::Method::PUT,
        path,
        Some(&json!({"base":"https://different.example.com"})),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{refused}");
    stub.script_answer(
        "GET",
        "/app/hook/config",
        ScriptedResponse::ok(r#"{"url":"https://unreachable.invalid/webhooks/id"}"#),
    );
    let (status, shown) = requested(&kestrel, reqwest::Method::GET, path, None).await;
    assert_eq!(status, StatusCode::OK, "{shown}");
    assert_eq!(
        shown["pending_url"],
        format!("https://hooks.example.com{}", integration.webhook_path())
    );
    stub.script_answer(
        "PATCH",
        "/app/hook/config",
        ScriptedResponse::answering(200),
    );
    let (status, changed) = requested(&kestrel, reqwest::Method::PUT, path, Some(&base)).await;
    assert_eq!(status, StatusCode::OK, "{changed}");
    assert_eq!(changed["revision"], integration.revision + 1);
    stub.script_answer(
        "PATCH",
        "/app/hook/config",
        ScriptedResponse::answering(200),
    );
    let (status, rotated) =
        requested(&kestrel, reqwest::Method::POST, rotation, Some(&json!({}))).await;
    assert_eq!(status, StatusCode::OK, "{rotated}");
    let urls: Vec<Value> = stub
        .requests()
        .iter()
        .filter(|r| r.method == "PATCH")
        .map(|r| serde_json::from_str::<Value>(&r.body).unwrap())
        .filter(|r| r.get("url").is_some())
        .collect();
    assert_eq!(urls.len(), 2);
    assert_eq!(urls[0], urls[1]);
    kestrel.teardown().await;
}
