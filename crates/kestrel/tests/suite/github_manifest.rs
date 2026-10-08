use crate::support;

use reqwest::{Client, StatusCode, Url};
use serde_json::{Value, json};
use sqlx::Row;
use support::{
    Kestrel, PRIVATE_KEY,
    github_stub::{GithubStub, ScriptedResponse},
};

async fn start(kestrel: &Kestrel, stub: &GithubStub, webhook: bool) -> (Client, String, String) {
    kestrel.declare_organization("acme").await;
    let client = Client::new();
    let response = client.post(format!("{}/operator/organizations/acme/github-app", kestrel.operator()))
        .json(&json!({"name": "github", "repository": "acme/repo", "callback_base": kestrel.operator(), "api": stub.base_url(), "webhook_base": webhook.then_some("https://hooks.example.com")}))
        .send().await.unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let answer: Value = response.json().await.unwrap();
    let url = answer["url"].as_str().unwrap().to_owned();
    let state = Url::parse(&url)
        .unwrap()
        .query_pairs()
        .find(|(key, _)| key == "state")
        .unwrap()
        .1
        .into_owned();
    (client, url, state)
}

fn conversion(stub: &GithubStub) {
    stub.script_answer("POST", "/app-manifests/code123/conversions", ScriptedResponse::ok(json!({
        "id": 17, "pem": PRIVATE_KEY, "webhook_secret": "manifest-webhook-secret", "slug": "kestrel-example"
    }).to_string()));
}

#[tokio::test]
async fn manifest_setup_registers_a_usable_app_without_returning_secrets() {
    for webhook in [false, true] {
        let kestrel = Kestrel::boot_serving_alone().await;
        let stub = GithubStub::start();
        let (client, url, state) = start(&kestrel, &stub, webhook).await;
        let response = client.get(url).send().await.unwrap();
        assert_eq!(response.headers()["cache-control"], "no-store");
        let html = response.text().await.unwrap();
        let manifest = html
            .split("name=\"manifest\" value=\"")
            .nth(1)
            .unwrap()
            .split('"')
            .next()
            .unwrap()
            .replace("&quot;", "\"")
            .replace("&#39;", "'")
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&amp;", "&");
        let manifest: Value = serde_json::from_str(&manifest).unwrap();
        let app_name = manifest["name"].as_str().unwrap();
        assert!(app_name.starts_with("kestrel-"));
        assert_eq!(app_name.len(), 32);
        assert!(!app_name.contains(&state[..24]));
        assert_eq!(
            manifest["default_permissions"],
            json!({"contents": "write", "issues": "write", "pull_requests": "write", "metadata": "read"})
        );
        assert_eq!(
            manifest["default_events"],
            json!(["issues", "issue_comment", "pull_request"])
        );
        assert_eq!(manifest["hook_attributes"]["active"], true);
        assert!(
            manifest["redirect_url"]
                .as_str()
                .unwrap()
                .starts_with(&kestrel.operator())
        );
        conversion(&stub);
        let callback = format!(
            "{}/operator/github-app/callback?state={state}&code=code123",
            kestrel.operator()
        );
        let response = client.get(&callback).send().await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let html = response.text().await.unwrap();
        assert!(html.contains("https://github.com/apps/kestrel-example/installations/new"));
        assert!(!html.contains(PRIVATE_KEY));
        assert!(!html.contains("manifest-webhook-secret"));
        assert_eq!(
            client.get(&callback).send().await.unwrap().status(),
            StatusCode::UNPROCESSABLE_ENTITY
        );
        let pool = support::database(kestrel.data_dir()).await;
        let sealed: String = sqlx::query_scalar("SELECT configuration_sealed FROM github_app_flow")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert!(!sealed.contains(PRIVATE_KEY));
        assert!(!sealed.contains("manifest-webhook-secret"));
        stub.script_answer(
            "GET",
            "/repos/acme/repo/installation",
            ScriptedResponse::ok(r#"{"id":23,"app_id":17}"#),
        );
        let finish = format!(
            "{}/operator/github-app/installed?state={state}&installation_id=999",
            kestrel.operator()
        );
        assert_eq!(
            client.get(&finish).send().await.unwrap().status(),
            StatusCode::OK
        );
        assert_eq!(
            client.get(&finish).send().await.unwrap().status(),
            StatusCode::UNPROCESSABLE_ENTITY
        );
        let integration = sqlx::query("SELECT id, app_id, installation_id, private_key_sealed, signing_secret, poll_due_at FROM integration").fetch_one(&pool).await.unwrap();
        assert_eq!(integration.get::<i64, _>("app_id"), 17);
        assert_eq!(integration.get::<i64, _>("installation_id"), 23);
        assert!(
            integration
                .get::<Option<String>, _>("poll_due_at")
                .is_some(),
            "every inbound GitHub Integration is polled"
        );
        assert!(
            !integration
                .get::<String, _>("private_key_sealed")
                .contains("BEGIN")
        );
        assert_ne!(
            integration.get::<String, _>("signing_secret"),
            "manifest-webhook-secret"
        );
        assert_eq!(
            manifest["hook_attributes"]["url"],
            format!(
                "{}/webhooks/{}",
                if webhook {
                    "https://hooks.example.com"
                } else {
                    "https://unreachable.invalid"
                },
                integration.get::<String, _>("id")
            )
        );
        let records: Value = client
            .get(format!(
                "{}/operator/organizations/acme/integrations",
                kestrel.operator()
            ))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(records[0]["bot_login"], "kestrel-example[bot]");
        assert!(!records.to_string().contains("manifest-webhook-secret"));
        assert!(!records.to_string().contains("PRIVATE KEY"));
        let integrations = kestrel.integrations("acme").await;
        kestrel::integration::github::Github::dialling_out()
            .unwrap()
            .issue(&integrations[0], 1)
            .await
            .unwrap();
        let requests = stub.requests();
        assert!(
            requests
                .iter()
                .any(|r| r.url == "/app/installations/23/access_tokens")
        );
        assert!(requests.iter().any(|r| {
            r.url == "/repos/acme/repo/issues/1"
                && r.headers.iter().any(|(name, value)| {
                    name.eq_ignore_ascii_case("authorization")
                        && value == &format!("Bearer {}", support::github_stub::INSTALLATION_TOKEN)
                })
        }));
        let verified = requests
            .iter()
            .find(|r| r.url == "/repos/acme/repo/installation")
            .unwrap();
        assert!(
            verified
                .headers
                .iter()
                .any(|(name, value)| name.eq_ignore_ascii_case("authorization")
                    && value.starts_with("Bearer ey"))
        );
    }
}

#[tokio::test]
async fn unknown_expired_and_concurrent_callbacks_cannot_exchange_twice() {
    let kestrel = Kestrel::boot_serving_alone().await;
    let stub = GithubStub::start();
    let (client, _, state) = start(&kestrel, &stub, false).await;
    let callback = format!("{}/operator/github-app/callback", kestrel.operator());
    assert_eq!(
        client
            .get(format!("{callback}?state=unknown&code=code123"))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert!(stub.requests().is_empty());
    conversion(&stub);
    let request = || {
        client
            .get(format!("{callback}?state={state}&code=code123"))
            .send()
    };
    let (first, second) = tokio::join!(request(), request());
    let mut statuses = [
        first.unwrap().status().as_u16(),
        second.unwrap().status().as_u16(),
    ];
    statuses.sort();
    assert_eq!(statuses, [200, 422]);
    assert_eq!(
        stub.requests()
            .iter()
            .filter(|r| r.url.contains("/conversions"))
            .count(),
        1
    );
    let pool = support::database(kestrel.data_dir()).await;
    sqlx::query("UPDATE github_app_flow SET phase = 'ready', expires_at = '2000-01-01'")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        request().await.unwrap().status(),
        StatusCode::UNPROCESSABLE_ENTITY
    );
}

#[tokio::test]
async fn installation_must_belong_to_the_created_app_and_requested_repository() {
    let kestrel = Kestrel::boot_serving_alone().await;
    let stub = GithubStub::start();
    let (client, _, state) = start(&kestrel, &stub, false).await;
    conversion(&stub);
    client
        .get(format!(
            "{}/operator/github-app/callback?state={state}&code=code123",
            kestrel.operator()
        ))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    let finish = format!(
        "{}/operator/github-app/installed?state={state}",
        kestrel.operator()
    );
    for response in [
        ScriptedResponse::answering(404),
        ScriptedResponse::ok(r#"{"id":23,"app_id":99}"#),
    ] {
        stub.script_answer("GET", "/repos/acme/repo/installation", response);
        assert_eq!(
            client.get(&finish).send().await.unwrap().status(),
            StatusCode::UNPROCESSABLE_ENTITY
        );
    }
    let pool = support::database(kestrel.data_dir()).await;
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM integration")
            .fetch_one(&pool)
            .await
            .unwrap(),
        0
    );
    stub.script_answer(
        "GET",
        "/repos/acme/repo/installation",
        ScriptedResponse::ok(r#"{"id":23,"app_id":17}"#),
    );
    assert_eq!(
        client.get(finish).send().await.unwrap().status(),
        StatusCode::OK
    );
}

#[tokio::test]
async fn a_converted_app_survives_a_control_plane_restart() {
    let kestrel = Kestrel::boot_serving_alone().await;
    let stub = GithubStub::start();
    let (client, _, state) = start(&kestrel, &stub, true).await;
    conversion(&stub);
    client
        .get(format!(
            "{}/operator/github-app/callback?state={state}&code=code123",
            kestrel.operator()
        ))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    let kestrel = kestrel.kill_and_restart().await;
    stub.script_answer(
        "GET",
        "/repos/acme/repo/installation",
        ScriptedResponse::ok(r#"{"id":23,"app_id":17}"#),
    );
    assert_eq!(
        client
            .get(format!(
                "{}/operator/github-app/installed?state={state}",
                kestrel.operator()
            ))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    assert_eq!(kestrel.integrations("acme").await.len(), 1);
}

#[tokio::test]
async fn an_organization_app_uses_the_organization_manifest_form() {
    let kestrel = Kestrel::boot_serving_alone().await;
    kestrel.declare_organization("acme").await;
    let client = Client::new();
    let endpoint = format!(
        "{}/operator/organizations/acme/github-app",
        kestrel.operator()
    );
    let mut registration = json!({"name":"github", "repository":"acme/repo", "callback_base": kestrel.operator(), "app_organization":"acme"});
    let started: Value = client
        .post(&endpoint)
        .json(&registration)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let page = client
        .get(started["url"].as_str().unwrap())
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(page.contains("https://github.com/organizations/acme/settings/apps/new?state="));
    registration["callback_base"] = "https://attacker.example.com".into();
    assert_eq!(
        client
            .post(&endpoint)
            .json(&registration)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::UNPROCESSABLE_ENTITY
    );
}
