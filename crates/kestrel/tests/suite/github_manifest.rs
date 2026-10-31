use crate::support;

use reqwest::{Client, StatusCode, Url};
use serde_json::{Value, json};
use support::{
    Kestrel, PRIVATE_KEY,
    github_stub::{GithubStub, ScriptedResponse},
};

async fn start(kestrel: &Kestrel, webhook: bool) -> (Client, String, String) {
    kestrel.declare_organization("acme").await;
    let client = Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let response = client.post(format!("{}/operator/organizations/acme/github-app", kestrel.operator()))
        .json(&json!({"name": "github", "callback_base": kestrel.operator(), "webhook_base": webhook.then_some("https://hooks.example.com")}))
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

fn repository(stub: &GithubStub) {
    for _ in 0..4 {
        stub.script_answer(
            "GET",
            "/repos/acme/repo",
            ScriptedResponse::ok(
                r#"{"id":1,"full_name":"acme/repo","default_branch":"main","private":true}"#,
            ),
        );
    }
}

async fn select(kestrel: &Kestrel, stub: &GithubStub, client: &Client, state: &str) {
    stub.script_answer(
        "GET",
        "/app/installations/23",
        ScriptedResponse::ok(r#"{"id":23,"app_id":17,"account":{"login":"acme"}}"#),
    );
    let response = client
        .get(format!(
            "{}/operator/github-app/installed?state={state}&installation_id=23",
            kestrel.operator()
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    let location = response.headers()["location"].to_str().unwrap();
    assert!(location.contains("/setup?organization=acme&github_app_flow="));
    assert!(!location.contains(PRIVATE_KEY));
    assert!(!location.contains("manifest-webhook-secret"));
    repository(stub);
}

const FLOW: std::time::Duration = std::time::Duration::from_secs(60 * 60);

#[tokio::test]
async fn manifest_setup_registers_a_usable_app_without_returning_secrets() {
    for webhook in [false, true] {
        let stub = GithubStub::start();
        let kestrel = Kestrel::boot_serving_at_github(&stub.base_url()).await;
        let (client, url, state) = start(&kestrel, webhook).await;
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
        let sealed = kestrel.database().sealed_github_app_configuration().await;
        assert!(!sealed.contains(PRIVATE_KEY));
        assert!(!sealed.contains("manifest-webhook-secret"));
        stub.script_answer(
            "GET",
            "/repos/acme/repo/installation",
            ScriptedResponse::ok(r#"{"id":23,"app_id":17}"#),
        );
        select(&kestrel, &stub, &client, &state).await;
        let finish = format!(
            "{}/operator/github-app/flows/{state}/finish",
            kestrel.operator()
        );
        assert_eq!(
            client
                .post(&finish)
                .json(&json!({"repository":"acme/repo"}))
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::OK
        );
        assert_eq!(
            client
                .post(&finish)
                .json(&json!({"repository":"acme/repo"}))
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::OK
        );
        assert!(
            kestrel
                .database()
                .sealed_github_app_configuration()
                .await
                .is_empty()
        );
        let integration = kestrel.database().github_integration_at_rest().await;
        assert_eq!(integration.app_id, 17);
        assert_eq!(integration.installation_id, 23);
        assert!(
            integration.polled,
            "every inbound GitHub Integration is polled"
        );
        assert!(!integration.private_key_sealed.contains("BEGIN"));
        assert_ne!(integration.signing_secret, "manifest-webhook-secret");
        assert_eq!(
            manifest["hook_attributes"]["url"],
            format!(
                "{}/webhooks/{}",
                if webhook {
                    "https://hooks.example.com"
                } else {
                    "https://unreachable.invalid"
                },
                kestrel.integrations("acme").await[0].id
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
        kestrel::integration::github::Github::unfenced(&kestrel::timer::Clock::System)
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
    let stub = GithubStub::start();
    let kestrel = Kestrel::boot_serving_at_github(&stub.base_url()).await;
    let (client, _, state) = start(&kestrel, false).await;
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
    kestrel.away_for(FLOW);
    assert_eq!(
        request().await.unwrap().status(),
        StatusCode::UNPROCESSABLE_ENTITY
    );
}

#[tokio::test]
async fn installation_must_belong_to_the_created_app_and_requested_repository() {
    let stub = GithubStub::start();
    let kestrel = Kestrel::boot_serving_at_github(&stub.base_url()).await;
    let (client, _, state) = start(&kestrel, false).await;
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
    let installed = format!(
        "{}/operator/github-app/installed?state={state}&installation_id=999",
        kestrel.operator()
    );
    for response in [
        ScriptedResponse::answering(404),
        ScriptedResponse::ok(r#"{"id":999,"app_id":99,"account":{"login":"acme"}}"#),
    ] {
        stub.script_answer("GET", "/app/installations/999", response);
        assert_eq!(
            client.get(&installed).send().await.unwrap().status(),
            StatusCode::UNPROCESSABLE_ENTITY
        );
    }
    assert!(kestrel.integrations("acme").await.is_empty());
    select(&kestrel, &stub, &client, &state).await;
    stub.script_answer(
        "GET",
        "/repos/acme/repo/installation",
        ScriptedResponse::ok(r#"{"id":24,"app_id":17}"#),
    );
    let finish = format!(
        "{}/operator/github-app/flows/{state}/finish",
        kestrel.operator()
    );
    assert_eq!(
        client
            .post(&finish)
            .json(&json!({"repository":"acme/repo"}))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert!(kestrel.integrations("acme").await.is_empty());
    stub.script_answer(
        "GET",
        "/repos/acme/repo/installation",
        ScriptedResponse::ok(r#"{"id":23,"app_id":17}"#),
    );
    assert_eq!(
        client
            .post(&finish)
            .json(&json!({"repository":"acme/repo"}))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
}

#[tokio::test]
async fn a_converted_app_survives_a_control_plane_restart() {
    let stub = GithubStub::start();
    let kestrel = Kestrel::boot_serving_at_github(&stub.base_url()).await;
    let (client, _, state) = start(&kestrel, true).await;
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
    select(&kestrel, &stub, &client, &state).await;
    let kestrel = kestrel.kill_and_restart().await;
    stub.script_answer(
        "GET",
        "/repos/acme/repo/installation",
        ScriptedResponse::ok(r#"{"id":23,"app_id":17}"#),
    );
    assert_eq!(
        client
            .post(format!(
                "{}/operator/github-app/flows/{state}/finish",
                kestrel.operator()
            ))
            .json(&json!({"repository":"acme/repo"}))
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
    let client = Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let endpoint = format!(
        "{}/operator/organizations/acme/github-app",
        kestrel.operator()
    );
    let mut registration =
        json!({"name":"github", "callback_base": kestrel.operator(), "app_organization":"acme"});
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

#[tokio::test]
async fn setup_starts_without_a_repository_and_waits_for_explicit_selection() {
    let stub = GithubStub::start();
    let kestrel = Kestrel::boot_serving_at_github(&stub.base_url()).await;
    kestrel.declare_organization("acme").await;
    let client = Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let response = client
        .post(format!(
            "{}/operator/organizations/acme/github-app",
            kestrel.operator()
        ))
        .json(&json!({"name":"github", "callback_base":kestrel.operator()}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let started: Value = response.json().await.unwrap();
    let state = started["flow"].as_str().unwrap();
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
    stub.script_answer(
        "GET",
        "/app/installations/23",
        ScriptedResponse::ok(r#"{"id":23,"app_id":17,"account":{"login":"acme"}}"#),
    );
    assert_eq!(
        client
            .get(format!(
                "{}/operator/github-app/installed?state={state}&installation_id=23",
                kestrel.operator()
            ))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::SEE_OTHER
    );
    assert!(kestrel.integrations("acme").await.is_empty());
    let endpoint = format!("{}/operator/github-app/flows/{state}", kestrel.operator());
    let status: Value = client
        .get(&endpoint)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(status["phase"], "select_repository");
    stub.script_answer(
        "GET",
        "/app/installations/23",
        ScriptedResponse::ok(r#"{"id":23,"app_id":17,"account":{"login":"acme"}}"#),
    );
    stub.script_answer("GET", "/installation/repositories", ScriptedResponse::ok(r#"{"total_count":1,"repositories":[{"id":41,"full_name":"acme/repo","default_branch":"main","private":true}]}"#));
    let repositories: Value = client
        .get(format!("{endpoint}/repositories"))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(repositories["repositories"][0]["default_branch"], "main");
    stub.script_answer(
        "GET",
        "/repos/acme/repo/installation",
        ScriptedResponse::ok(r#"{"id":23,"app_id":17}"#),
    );
    repository(&stub);
    stub.script_answer(
        "GET",
        "/repos/acme/repo/installation",
        ScriptedResponse::ok(r#"{"id":23,"app_id":17}"#),
    );
    let finish = || {
        client
            .post(format!("{endpoint}/finish"))
            .json(&json!({"repository":"acme/repo"}))
            .send()
    };
    let (first, second) = tokio::join!(finish(), finish());
    let first: Value = first
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let second: Value = second
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(first, second);
    assert_eq!(first["phase"], "completed");
    assert_eq!(kestrel.integrations("acme").await.len(), 1);
    assert_eq!(
        client
            .get(&endpoint)
            .send()
            .await
            .unwrap()
            .json::<Value>()
            .await
            .unwrap(),
        first
    );
}

#[tokio::test]
async fn repository_pages_expose_empty_results_pagination_and_safe_retries() {
    let stub = GithubStub::start();
    let kestrel = Kestrel::boot_serving_at_github(&stub.base_url()).await;
    let (client, _, state) = start(&kestrel, false).await;
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
    select(&kestrel, &stub, &client, &state).await;
    let endpoint = format!(
        "{}/operator/github-app/flows/{state}/repositories",
        kestrel.operator()
    );
    for response in [
        ScriptedResponse::ok(r#"{"total_count":0,"repositories":[]}"#),
        ScriptedResponse::ok(
            r#"{"total_count":2,"repositories":[{"id":1,"full_name":"acme/repo","default_branch":"main","private":true}]}"#,
        ),
        ScriptedResponse::ok(
            r#"{"total_count":2,"repositories":[{"id":2,"full_name":"acme/other","default_branch":"trunk","private":false}]}"#,
        ),
        ScriptedResponse::answering(404),
        ScriptedResponse::answering(503),
        support::github_stub::rate_limited(),
        ScriptedResponse::ok(r#"{"total_count":0,"repositories":[]}"#),
    ] {
        stub.script_answer("GET", "/installation/repositories", response);
    }
    for _ in 0..8 {
        stub.script_answer(
            "GET",
            "/app/installations/23",
            ScriptedResponse::ok(r#"{"id":23,"app_id":17,"account":{"login":"acme"}}"#),
        );
    }
    let empty: Value = client
        .get(&endpoint)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(empty["repositories"], json!([]));
    assert_eq!(empty["next_page"], Value::Null);
    let first: Value = client
        .get(format!("{endpoint}?per_page=1"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(first["next_page"], 2);
    assert_eq!(first["repositories"][0]["default_branch"], "main");
    let last: Value = client
        .get(format!("{endpoint}?per_page=1&page=2"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(last["next_page"], Value::Null);
    assert_eq!(last["repositories"][0]["default_branch"], "trunk");
    for expected in [
        StatusCode::UNPROCESSABLE_ENTITY,
        StatusCode::SERVICE_UNAVAILABLE,
        StatusCode::SERVICE_UNAVAILABLE,
    ] {
        let response = client.get(&endpoint).send().await.unwrap();
        assert_eq!(response.status(), expected);
        let diagnostic = response.text().await.unwrap();
        assert!(!diagnostic.contains(PRIVATE_KEY));
        assert!(!diagnostic.contains(support::github_stub::INSTALLATION_TOKEN));
    }
    assert_eq!(
        client.get(&endpoint).send().await.unwrap().status(),
        StatusCode::OK
    );
    assert_eq!(
        client
            .get(format!("{endpoint}?page=0"))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert!(kestrel.integrations("acme").await.is_empty());
}

#[tokio::test]
async fn cancellation_and_expiry_erase_temporary_material_and_never_register() {
    for cancelled in [false, true] {
        let stub = GithubStub::start();
        let kestrel = Kestrel::boot_serving_at_github(&stub.base_url()).await;
        let (client, _, state) = start(&kestrel, false).await;
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
        let endpoint = format!("{}/operator/github-app/flows/{state}", kestrel.operator());
        let status: Value = if cancelled {
            client
                .delete(&endpoint)
                .send()
                .await
                .unwrap()
                .error_for_status()
                .unwrap()
                .json()
                .await
                .unwrap()
        } else {
            kestrel.away_for(FLOW);
            client
                .get(&endpoint)
                .send()
                .await
                .unwrap()
                .error_for_status()
                .unwrap()
                .json()
                .await
                .unwrap()
        };
        assert_eq!(
            status["phase"],
            if cancelled { "cancelled" } else { "expired" }
        );
        assert!(status["guidance"].as_str().unwrap().contains("manually"));
        assert!(
            kestrel
                .database()
                .sealed_github_app_configuration()
                .await
                .is_empty()
        );
        assert_eq!(
            client
                .post(format!("{endpoint}/finish"))
                .json(&json!({"repository":"acme/repo"}))
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::UNPROCESSABLE_ENTITY
        );
        assert!(kestrel.integrations("acme").await.is_empty());
        assert_eq!(
            stub.requests().len(),
            1,
            "cancellation and expiry never contact GitHub"
        );
    }
}

#[tokio::test]
async fn an_uncertain_conversion_explains_restart_without_reusing_the_code() {
    let stub = GithubStub::start();
    let kestrel = Kestrel::boot_serving_at_github(&stub.base_url()).await;
    let (client, _, state) = start(&kestrel, false).await;
    stub.script_answer(
        "POST",
        "/app-manifests/code123/conversions",
        ScriptedResponse::answering(503),
    );
    let callback = format!(
        "{}/operator/github-app/callback?state={state}&code=code123",
        kestrel.operator()
    );
    assert_eq!(
        client.get(&callback).send().await.unwrap().status(),
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let status: Value = client
        .get(format!(
            "{}/operator/github-app/flows/{state}",
            kestrel.operator()
        ))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(status["phase"], "conversion_failed");
    assert!(status["guidance"].as_str().unwrap().contains("restart"));
    assert!(status["guidance"].as_str().unwrap().contains("manually"));
    assert!(
        kestrel
            .database()
            .sealed_github_app_configuration()
            .await
            .is_empty()
    );
    assert_eq!(
        client.get(callback).send().await.unwrap().status(),
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(stub.requests().len(), 1);
}

#[tokio::test]
async fn server_setup_pages_discover_installations_and_finish_an_explicit_choice() {
    let stub = GithubStub::start();
    let kestrel = Kestrel::boot_serving_at_github(&stub.base_url()).await;
    let (client, url, state) = start(&kestrel, false).await;
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
    stub.script_answer(
        "GET",
        "/app/installations",
        ScriptedResponse::ok(r#"[{"id":23,"app_id":17,"account":{"login":"acme"}}]"#),
    );
    let discover = client
        .get(format!(
            "{}/operator/github-app/installed?state={state}",
            kestrel.operator()
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(discover.status(), StatusCode::OK);
    let discovery = discover.text().await.unwrap();
    let continuation = discovery
        .split("href=\"")
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap()
        .replace("&amp;", "&");
    assert!(continuation.starts_with("/operator/github-app/setup?"));
    assert!(continuation.contains("installation_id=23"));
    for _ in 0..2 {
        stub.script_answer(
            "GET",
            "/app/installations/23",
            ScriptedResponse::ok(r#"{"id":23,"app_id":17,"account":{"login":"acme"}}"#),
        );
    }
    stub.script_answer("GET", "/installation/repositories", ScriptedResponse::ok(r#"{"total_count":1,"repositories":[{"id":1,"full_name":"acme/repo","default_branch":"main","private":true}]}"#));
    repository(&stub);
    let response = client
        .get(format!("{}{}", kestrel.operator(), continuation))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        response.headers()["content-security-policy"]
            .to_str()
            .unwrap()
            .contains("form-action 'self'")
    );
    let html = response.text().await.unwrap();
    assert!(html.contains("name=\"repository\""));
    assert!(html.contains("default branch: main"));
    assert!(!html.contains("checked"), "selection must be explicit");
    stub.script_answer(
        "GET",
        "/repos/acme/repo/installation",
        ScriptedResponse::ok(r#"{"id":23,"app_id":17}"#),
    );
    let completed = client
        .post(&url)
        .header("content-type", "application/x-www-form-urlencoded")
        .body("repository=acme%2Frepo")
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(completed.contains("Integration is ready"));
    assert_eq!(kestrel.integrations("acme").await.len(), 1);
}

#[tokio::test]
async fn finish_retries_temporary_installation_failure_without_saving() {
    let stub = GithubStub::start();
    let kestrel = Kestrel::boot_serving_at_github(&stub.base_url()).await;
    let (client, _, state) = start(&kestrel, false).await;
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
    select(&kestrel, &stub, &client, &state).await;
    let finish = format!(
        "{}/operator/github-app/flows/{state}/finish",
        kestrel.operator()
    );
    stub.script_answer(
        "GET",
        "/repos/acme/repo/installation",
        ScriptedResponse::answering(503),
    );
    let response = client
        .post(&finish)
        .json(&json!({"repository":"acme/repo"}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let diagnostic: Value = response.json().await.unwrap();
    assert_eq!(diagnostic["next_steps"][0]["action"], "retry_request");
    assert!(kestrel.integrations("acme").await.is_empty());
    stub.script_answer(
        "GET",
        "/repos/acme/repo/installation",
        ScriptedResponse::ok(r#"{"id":23,"app_id":17}"#),
    );
    assert_eq!(
        client
            .post(&finish)
            .json(&json!({"repository":"acme/repo"}))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
}

#[tokio::test]
async fn manifest_requests_cannot_choose_an_arbitrary_upstream() {
    let kestrel = Kestrel::boot_serving_alone().await;
    kestrel.declare_organization("acme").await;
    for api in [
        "http://169.254.169.254",
        "https://api.github.com.attacker.example",
        "null",
    ] {
        let response = Client::new()
            .post(format!(
                "{}/operator/organizations/acme/github-app",
                kestrel.operator()
            ))
            .json(&json!({"name":"github", "callback_base":kestrel.operator(), "api":api}))
            .send()
            .await
            .unwrap();
        assert!(
            response.status().is_client_error(),
            "an operator request must not choose the App's upstream host"
        );
    }
    assert!(kestrel.integrations("acme").await.is_empty());
}

#[test]
fn a_hostile_flow_state_cannot_inject_markup_into_the_installed_page() {
    let html = kestrel::integration::manifest::installed_page("\"><script>alert(1)</script>");
    assert!(!html.contains("<script>"));
    assert!(html.contains("?state=&quot;&gt;&lt;script&gt;alert(1)&lt;/script&gt;"));
}

#[tokio::test]
async fn the_deployment_base_applies_to_new_apps_and_existing_hook_updates() {
    let stub = GithubStub::start();
    let kestrel = Kestrel::boot_serving_with_webhook_base(
        &stub.base_url(),
        Some("https://public.example.com".into()),
    )
    .await;
    kestrel.declare_organization("acme").await;
    let (client, url, _) = start(&kestrel, false).await;
    let html = client.get(url).send().await.unwrap().text().await.unwrap();
    assert!(
        html.contains("https://public.example.com/webhooks/"),
        "{html}"
    );
    assert!(!html.contains("unreachable.invalid"));
    let integration = kestrel
        .register_signed_github(
            "acme",
            "existing",
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
    let response = client
        .put(format!(
            "{}/operator/organizations/acme/integrations/existing/webhook",
            kestrel.operator()
        ))
        .json(&json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let patch = stub
        .requests()
        .into_iter()
        .find(|r| r.method == "PATCH")
        .unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&patch.body).unwrap(),
        json!({"url": format!("https://public.example.com{}", integration.webhook_path())})
    );
    kestrel.teardown().await;
}
