use reqwest::StatusCode;
use serde_json::{Value, json};

use crate::support::Kestrel;

const METHOD: &str = "/operator/organizations/ada/sign-ins/claude/claude-setup-token";

async fn named(kestrel: &Kestrel) {
    let response = reqwest::Client::new()
        .put(format!("{}/operator/operator", kestrel.operator()))
        .json(&json!({"name": "ada"}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
}

#[tokio::test]
async fn a_relay_starts_with_a_safe_resumable_identity_and_a_fixed_deadline() {
    let kestrel = Kestrel::boot().await;
    named(&kestrel).await;
    let response = reqwest::Client::new()
        .post(format!("{}{METHOD}/relay", kestrel.operator()))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let relay: Value = response.json().await.unwrap();
    assert!(relay["id"].as_str().is_some());
    assert_eq!(relay["progress"]["state"], "starting");
    assert_eq!(relay["destination"]["reference"], "ada");
    let created: jiff::Timestamp = relay["created_at"].as_str().unwrap().parse().unwrap();
    let deadline: jiff::Timestamp = relay["deadline"].as_str().unwrap().parse().unwrap();
    assert_eq!(deadline.as_second() - created.as_second(), 600);
}

const TOKEN: &str = "sk-ant-oat01-scripted-subscription-secret";
const CODE: &str = "private-authorization-code";
const URL: &str = "https://claude.com/cai/oauth/authorize?client_id=client&redirect_uri=https%3A%2F%2Fplatform.claude.com%2Foauth%2Fcode%2Fcallback&response_type=code&scope=user%3Ainference&code_challenge=abcdefghijklmnopqrstuvwxyz0123456789ABCDEFGHI&state=public-state";

fn vendor() -> tempfile::TempDir {
    vendor_waiting("")
}

fn vendor_waiting(after_code: &str) -> tempfile::TempDir {
    use std::os::unix::fs::PermissionsExt as _;
    let scripts = tempfile::tempdir().unwrap();
    let script = scripts.path().join("claude");
    let (first, second) = URL.split_at(90);
    std::fs::write(&script, format!(r#"#!/bin/sh
[ "$1" = setup-token ] || exit 2
stty -echo
printf '\033['
sleep 0.02
printf '2J\033[HSign in\r\n%s\r\n%s\r\n\r\nPaste\033[1Ccode here if prompted > ' '{first}' '{second}'
IFS= read -r code
[ "$code" = '{CODE}' ] || {{ printf '\r\nOAuth error: Invalid code. Press Enter to retry.\r\n'; sleep 60; exit 1; }}
{after_code}
printf '\033[2J\033[HYour token:\r\nsk-ant-oat01-scripted-'
sleep 0.02
printf 'subscription-secret\r\n\r\nPress Enter to continue\r\n'
IFS= read -r continuation
exit 0
"#)).unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    scripts
}

async fn sent(request: reqwest::RequestBuilder) -> (StatusCode, Value) {
    let response = request.send().await.unwrap();
    let status = response.status();
    let text = response.text().await.unwrap();
    assert!(!text.contains(TOKEN), "a token leaked");
    assert!(!text.contains(CODE), "an authorization code leaked");
    (status, serde_json::from_str(&text).unwrap_or(Value::Null))
}

async fn started(kestrel: &Kestrel) -> Value {
    let (status, body) =
        sent(reqwest::Client::new().post(format!("{}{METHOD}/relay", kestrel.operator()))).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    body
}

fn address(kestrel: &Kestrel, relay: &Value) -> String {
    format!(
        "{}{METHOD}/relay/{}",
        kestrel.operator(),
        relay["id"].as_str().unwrap()
    )
}

async fn until(kestrel: &Kestrel, relay: &Value, expected: &str) -> Value {
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let (status, body) = sent(reqwest::Client::new().get(address(kestrel, relay))).await;
            assert_eq!(status, StatusCode::OK, "{body}");
            if body["progress"]["state"] == expected {
                return body;
            }
            assert!(
                !matches!(
                    body["progress"]["state"].as_str(),
                    Some("failed" | "expired" | "cancelled" | "completed")
                ),
                "{body}"
            );
            tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        }
    })
    .await
    .expect("the relay reaches its state")
}

#[tokio::test]
async fn terminal_redraws_wrapped_urls_and_continuation_produce_login_evidence_without_model_usage()
{
    let scripts = vendor();
    let kestrel = Kestrel::dispatching_with_programs(scripts.path()).await;
    named(&kestrel).await;
    let relay = started(&kestrel).await;
    let waiting = until(&kestrel, &relay, "awaiting_code").await;
    assert_eq!(waiting["progress"]["url"], URL);
    assert_eq!(waiting["deadline"], relay["deadline"]);
    let (status, saving) = sent(
        reqwest::Client::new()
            .post(format!("{}/code", address(&kestrel, &relay)))
            .json(&json!({"code": CODE})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{saving}");
    assert_eq!(saving["progress"]["state"], "saving");
    let completed = until(&kestrel, &relay, "completed").await;
    let saved = &completed["progress"]["sign_in"];
    assert_eq!(saved["authentication"]["state"], "login_completed");
    assert_eq!(saved["authentication"]["source"], "relay");
    assert_eq!(saved["model_use"], json!([]));
    let (status, listed) = sent(reqwest::Client::new().get(format!(
        "{}/operator/organizations/ada/sign-ins",
        kestrel.operator()
    )))
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listed[0], *saved);
    let (status, reread) = sent(reqwest::Client::new().get(address(&kestrel, &relay))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(reread, completed);
}

#[tokio::test]
async fn concurrent_starts_share_one_destination_and_other_profiles_and_organizations_proceed() {
    let scripts = vendor();
    let kestrel = Kestrel::dispatching_with_programs(scripts.path()).await;
    named(&kestrel).await;
    let endpoint = format!("{}{METHOD}/relay", kestrel.operator());
    let (one, two) = tokio::join!(
        sent(reqwest::Client::new().post(&endpoint)),
        sent(reqwest::Client::new().post(&endpoint)),
    );
    let (accepted, refused) = if one.0 == StatusCode::ACCEPTED {
        (one, two)
    } else {
        (two, one)
    };
    assert_eq!(accepted.0, StatusCode::ACCEPTED, "{accepted:?}");
    assert_eq!(refused.0, StatusCode::CONFLICT, "{refused:?}");
    assert_eq!(refused.1["context"]["reference"], accepted.1["id"]);
    let steps = refused.1["next_steps"].as_array().unwrap();
    assert!(
        steps
            .iter()
            .any(|step| step["action"] == "continue_sign_in_relay")
    );
    assert!(
        steps
            .iter()
            .any(|step| step["action"] == "cancel_sign_in_relay")
    );
    let (status, profile) = sent(
        reqwest::Client::new()
            .post(format!(
                "{}/operator/organizations/ada/profiles",
                kestrel.operator()
            ))
            .json(&json!({"name":"second"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{profile}");
    let (status, other) = sent(
        reqwest::Client::new()
            .post(&endpoint)
            .json(&json!({"profile":"second"})),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{other}");
    assert_ne!(other["id"], accepted.1["id"]);
    assert_eq!(other["destination"]["reference"], "second");
    let (status, organization) = sent(
        reqwest::Client::new()
            .post(format!("{}/operator/organizations", kestrel.operator()))
            .json(&json!({"name":"elsewhere"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{organization}");
    let (status, elsewhere) =
        sent(reqwest::Client::new().post(endpoint.replace("/ada/", "/elsewhere/"))).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{elsewhere}");
    for relay in [&accepted.1, &other] {
        until(&kestrel, relay, "awaiting_code").await;
    }
    let (status, mismatch) = sent(
        reqwest::Client::new().get(address(&kestrel, &accepted.1).replace("/ada/", "/elsewhere/")),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{mismatch}");
    let (status, _) = sent(
        reqwest::Client::new().get(
            address(&kestrel, &accepted.1)
                .replace("/claude/claude-setup-token/", "/codex/codex-device-auth/"),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

async fn cleaned(kestrel: &Kestrel, relay: &Value) {
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while !kestrel
            .database()
            .relay_cleaned(relay["id"].as_str().unwrap())
            .await
        {
            tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        }
    })
    .await
    .expect("temporary relay material is cleaned up");
    let metadata = kestrel
        .database()
        .relay_metadata(relay["id"].as_str().unwrap())
        .await;
    assert!(!metadata.contains(TOKEN));
    assert!(!metadata.contains(CODE));
}

#[tokio::test]
async fn an_accepted_import_cancels_the_same_slot_and_a_late_vendor_result_cannot_replace_it() {
    let scripts = vendor_waiting("sleep 0.5");
    let kestrel = Kestrel::dispatching_with_programs(scripts.path()).await;
    named(&kestrel).await;
    let relay = started(&kestrel).await;
    until(&kestrel, &relay, "awaiting_code").await;
    let (status, body) = sent(
        reqwest::Client::new()
            .post(format!("{}/code", address(&kestrel, &relay)))
            .json(&json!({"code":CODE})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, replacement) = sent(
        reqwest::Client::new()
            .put(format!("{}{METHOD}", kestrel.operator()))
            .json(&json!({"value":"replacement-subscription"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{replacement}");
    let cancelled = until(&kestrel, &relay, "cancelled").await;
    assert_eq!(cancelled["progress"]["state"], "cancelled");
    cleaned(&kestrel, &relay).await;
    let (status, saved) = sent(reqwest::Client::new().get(format!(
        "{}/operator/organizations/ada/sign-ins",
        kestrel.operator()
    )))
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(saved[0]["revision"], replacement["revision"]);
    assert_eq!(saved[0]["authentication"]["state"], "unchecked");
    let (status, _) = sent(
        reqwest::Client::new()
            .post(format!("{}/code", address(&kestrel, &relay)))
            .json(&json!({"code":CODE})),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
}

#[tokio::test]
async fn a_vendor_code_error_fails_without_waiting_for_exit_and_late_or_malformed_codes_stay_private()
 {
    let scripts = vendor();
    let kestrel = Kestrel::dispatching_with_programs(scripts.path()).await;
    named(&kestrel).await;
    let relay = started(&kestrel).await;
    until(&kestrel, &relay, "awaiting_code").await;
    let endpoint = format!("{}/code", address(&kestrel, &relay));
    let (status, _) = sent(
        reqwest::Client::new()
            .post(&endpoint)
            .json(&json!({"code": {"secret":CODE}})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, state) = sent(reqwest::Client::new().get(address(&kestrel, &relay))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(state["progress"]["state"], "awaiting_code");
    let (status, _) = sent(
        reqwest::Client::new()
            .post(&endpoint)
            .json(&json!({"code":"bad-code"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let failed = until(&kestrel, &relay, "failed").await;
    assert!(failed["progress"]["diagnostic"].is_object());
    cleaned(&kestrel, &relay).await;
    let (status, conflict) = sent(
        reqwest::Client::new()
            .post(&endpoint)
            .json(&json!({"code":CODE})),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(conflict["context"]["state"], "failed");
    let (status, saved) = sent(reqwest::Client::new().get(format!(
        "{}/operator/organizations/ada/sign-ins",
        kestrel.operator()
    )))
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(saved, json!([]));
    let (status, unchanged) = sent(reqwest::Client::new().delete(address(&kestrel, &relay))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(unchanged, failed);
}

#[tokio::test]
async fn disconnect_keeps_the_relay_running_but_its_absolute_deadline_expires_and_retention_ends_with_gone()
 {
    let scripts = vendor();
    let kestrel = Kestrel::dispatching_with_programs(scripts.path()).await;
    named(&kestrel).await;
    let relay = started(&kestrel).await;
    let waiting = until(&kestrel, &relay, "awaiting_code").await;
    let connected = reqwest::get(format!("{}/events", address(&kestrel, &relay)))
        .await
        .unwrap();
    assert_eq!(connected.status(), StatusCode::OK);
    drop(connected);
    let (status, reread) = sent(reqwest::Client::new().get(address(&kestrel, &relay))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(reread, waiting);
    kestrel
        .database()
        .expire_relay(relay["id"].as_str().unwrap())
        .await;
    let expired = until(&kestrel, &relay, "expired").await;
    cleaned(&kestrel, &relay).await;
    let (status, refused) = sent(
        reqwest::Client::new()
            .post(format!("{}/code", address(&kestrel, &relay)))
            .json(&json!({"code":CODE})),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(refused["context"]["state"], "expired");
    let events = reqwest::get(format!("{}/events", address(&kestrel, &relay)))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(events.contains("event: relay\n"), "{events}");
    assert!(events.contains("event: end\n"), "{events}");
    assert!(events.contains("\"state\":\"expired\""), "{events}");
    assert!(!events.contains(TOKEN));
    assert!(!events.contains(CODE));
    let (status, unchanged) = sent(reqwest::Client::new().delete(address(&kestrel, &relay))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(unchanged, expired);
    kestrel
        .database()
        .age_relay_terminal_state(relay["id"].as_str().unwrap())
        .await;
    let (status, gone) = sent(reqwest::Client::new().get(address(&kestrel, &relay))).await;
    assert_eq!(status, StatusCode::GONE, "{gone}");
    let (status, _) = sent(
        reqwest::Client::new().get(
            address(&kestrel, &relay)
                .replace("/claude/claude-setup-token/", "/codex/codex-device-auth/"),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let next = started(&kestrel).await;
    assert_ne!(next["id"], relay["id"]);
}

#[tokio::test]
async fn restart_retains_a_terminal_failure_cleans_up_and_never_replays_vendor_login() {
    let scripts = vendor();
    let kestrel = Kestrel::dispatching_with_programs(scripts.path()).await;
    named(&kestrel).await;
    let relay = started(&kestrel).await;
    until(&kestrel, &relay, "awaiting_code").await;
    let kestrel = kestrel.kill_and_restart().await;
    let failed = until(&kestrel, &relay, "failed").await;
    cleaned(&kestrel, &relay).await;
    assert!(failed["progress"]["diagnostic"].is_object());
    let (status, conflict) = sent(
        reqwest::Client::new()
            .post(format!("{}/code", address(&kestrel, &relay)))
            .json(&json!({"code":CODE})),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(conflict["context"]["state"], "failed");
    let (status, saved) = sent(reqwest::Client::new().get(format!(
        "{}/operator/organizations/ada/sign-ins",
        kestrel.operator()
    )))
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(saved, json!([]));
    let next = started(&kestrel).await;
    assert_ne!(next["id"], relay["id"]);
    until(&kestrel, &next, "awaiting_code").await;
    let (status, cancelled) = sent(reqwest::Client::new().delete(address(&kestrel, &next))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(cancelled["progress"]["state"], "cancelled");
    cleaned(&kestrel, &next).await;
}

#[tokio::test]
async fn cancel_and_completion_have_one_terminal_outcome_that_late_requests_cannot_change() {
    let scripts = vendor();
    let kestrel = Kestrel::dispatching_with_programs(scripts.path()).await;
    named(&kestrel).await;
    let relay = started(&kestrel).await;
    until(&kestrel, &relay, "awaiting_code").await;
    let endpoint = address(&kestrel, &relay);
    let (code, cancelled) = tokio::join!(
        sent(
            reqwest::Client::new()
                .post(format!("{endpoint}/code"))
                .json(&json!({"code":CODE}))
        ),
        sent(reqwest::Client::new().delete(&endpoint)),
    );
    assert!(
        matches!(code.0, StatusCode::OK | StatusCode::CONFLICT),
        "{code:?}"
    );
    assert_eq!(cancelled.0, StatusCode::OK);
    let state = cancelled.1["progress"]["state"].as_str().unwrap();
    assert!(matches!(state, "completed" | "cancelled"), "{cancelled:?}");
    cleaned(&kestrel, &relay).await;
    let (status, final_state) = sent(reqwest::Client::new().get(&endpoint)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(final_state, cancelled.1);
    let (status, repeated) = sent(reqwest::Client::new().delete(&endpoint)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(repeated, final_state);
    let (status, _) = sent(
        reqwest::Client::new()
            .post(format!("{endpoint}/code"))
            .json(&json!({"code":CODE})),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
}

#[tokio::test]
async fn generic_profile_imports_cancel_only_the_relay_whose_entry_they_replace() {
    let scripts = vendor();
    let kestrel = Kestrel::dispatching_with_programs(scripts.path()).await;
    named(&kestrel).await;
    let relay = started(&kestrel).await;
    let waiting = until(&kestrel, &relay, "awaiting_code").await;
    let profile = format!(
        "{}/operator/organizations/ada/profiles/ada/variables",
        kestrel.operator()
    );
    let (status, _) = sent(
        reqwest::Client::new()
            .put(format!("{profile}/UNRELATED"))
            .json(&json!({"secret":"other-material"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, unchanged) = sent(reqwest::Client::new().get(address(&kestrel, &relay))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(unchanged, waiting);
    let (status, invalid) = sent(
        reqwest::Client::new()
            .put(format!("{}{METHOD}", kestrel.operator()))
            .json(&json!({"value":"not one token"})),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{invalid}");
    let (status, unchanged) = sent(reqwest::Client::new().get(address(&kestrel, &relay))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(unchanged, waiting);
    let (status, imported) = sent(
        reqwest::Client::new()
            .put(format!("{profile}/CLAUDE_CODE_OAUTH_TOKEN"))
            .json(&json!({"secret":"generic-subscription"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{imported}");
    until(&kestrel, &relay, "cancelled").await;
    cleaned(&kestrel, &relay).await;
}

#[tokio::test]
async fn closed_relay_requests_refuse_unknown_fields_without_changing_the_destination_or_echoing_code()
 {
    let scripts = vendor();
    let kestrel = Kestrel::dispatching_with_programs(scripts.path()).await;
    named(&kestrel).await;
    let (status, refused) = sent(
        reqwest::Client::new()
            .post(format!("{}{METHOD}/relay", kestrel.operator()))
            .json(&json!({"profil":"second"})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{refused}");
    let relay = started(&kestrel).await;
    let waiting = until(&kestrel, &relay, "awaiting_code").await;
    let (status, refused) = sent(
        reqwest::Client::new()
            .post(format!("{}/code", address(&kestrel, &relay)))
            .json(&json!({"code":CODE,"unknown":CODE})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{refused}");
    let (status, reread) = sent(reqwest::Client::new().get(address(&kestrel, &relay))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(reread, waiting);
}

#[tokio::test]
async fn expiry_interrupts_stalled_docker_provisioning_and_failed_cleanup_remains_retryable() {
    use std::os::unix::fs::PermissionsExt as _;
    use std::time::Duration;
    let directory = tempfile::tempdir().unwrap();
    let stub = directory.path().join("docker");
    std::fs::write(
        &stub,
        r#"#!/bin/sh
case "$1" in
  image) printf 'sha256:relay {"dev.kestrel.harnesses":"claude"}\n';;
  create) touch "$RELAY_SCRIPT_ROOT/provisioning"; sleep 3 & exit 0;;
  rm) [ ! -f "$RELAY_SCRIPT_ROOT/refuse-cleanup" ] || exit 1; touch "$RELAY_SCRIPT_ROOT/cleaned";;
  *) exit 0;;
esac
"#,
    )
    .unwrap();
    std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(directory.path().join("refuse-cleanup"), "").unwrap();
    let mut paths = vec![directory.path().to_path_buf()];
    paths.extend(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    ));
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let bind = listener.local_addr().unwrap();
    drop(listener);
    let operator = format!("http://{bind}");
    let mut process = tokio::process::Command::new(env!("CARGO_BIN_EXE_kestrel-control-plane"))
        .env("PATH", std::env::join_paths(paths).unwrap())
        .env("RELAY_SCRIPT_ROOT", directory.path())
        .env("KESTREL_COMPUTE", "docker")
        .env("KESTREL_IMAGE", "relay-stub")
        .env("KESTREL_DATA_DIR", directory.path())
        .env("KESTREL_LISTEN", "127.0.0.1:0")
        .env("KESTREL_OPERATOR_LISTEN", bind.to_string())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if reqwest::get(format!("{operator}/operator/organizations"))
                .await
                .is_ok()
            {
                break;
            }
            assert!(
                process.try_wait().unwrap().is_none(),
                "the control plane exited"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    let (status, _) = sent(
        reqwest::Client::new()
            .put(format!("{operator}/operator/operator"))
            .json(&json!({"name":"ada"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, relay) =
        sent(reqwest::Client::new().post(format!("{operator}{METHOD}/relay"))).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{relay}");
    tokio::time::timeout(Duration::from_secs(2), async {
        while !directory.path().join("provisioning").exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let database = crate::support::Database::at(directory.path());
    let id = relay["id"].as_str().unwrap();
    database.expire_relay(id).await;
    let endpoint = format!("{operator}{METHOD}/relay/{id}");
    let (status, expired) = sent(reqwest::Client::new().get(&endpoint)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(expired["progress"]["state"], "expired");
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(!database.relay_cleaned(id).await);
    std::fs::remove_file(directory.path().join("refuse-cleanup")).unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        while !database.relay_cleaned(id).await {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("expiry interrupts provisioning before its stalled command finishes");
    assert!(directory.path().join("cleaned").exists());
    let (status, retained) = sent(reqwest::Client::new().get(&endpoint)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(retained, expired);
    process.kill().await.unwrap();
    process.wait().await.unwrap();
}

const CODEX: &str = "/operator/organizations/ada/sign-ins/codex/codex-device-auth";
const DEVICE_URL: &str = "https://auth.openai.com/codex/device";
const DEVICE_CODE: &str = "0K88-JZ4OG";
const ACCESS: &str = "scripted-chatgpt-access-secret";
const LOGIN: &str = r#"{"auth_mode":"chatgpt","OPENAI_API_KEY":null,"tokens":{"id_token":"scripted-id","access_token":"scripted-chatgpt-access-secret","refresh_token":"scripted-refresh","account_id":"account"},"last_refresh":"2026-10-16T00:00:00Z"}"#;
const VENDOR_NOISE: &str = "raw-vendor-diagnostic-output";

/// A pinned-format `codex login --device-auth` that waits for `approved` in its directory, then
/// writes `login` as the vendor file and exits with `status`.
fn device_vendor(login: Option<&str>, status: u8) -> tempfile::TempDir {
    use std::os::unix::fs::PermissionsExt as _;
    let scripts = tempfile::tempdir().unwrap();
    let directory = scripts.path().display();
    let write = login.map_or(String::new(), |login| {
        format!("mkdir -p \"$HOME/.codex\" && printf '%s' '{login}' > \"$HOME/.codex/auth.json\"")
    });
    let script = scripts.path().join("codex");
    std::fs::write(&script, format!(r#"#!/bin/sh
[ "$*" = 'login -c cli_auth_credentials_store=file --device-auth' ] || exit 2
printf '\r\nWelcome to Codex [v\033[90m0.156.1\033[0m]\r\n\033[90mOpenAI'"'"'s command-line coding agent\033[0m\r\n\r\n'
printf 'Follow these steps to sign in with ChatGPT using device code authorization:\r\n\r\n'
printf '1. Open this link in your browser and sign in to your account\r\n   \033[94m{DEVICE_URL}\033[0m\r\n\r\n'
sleep 0.02
printf '2. Enter this one-time code \033[90m(expires in 15 minutes)\033[0m\r\n   \033[94m{DEVICE_CODE}\033[0m\r\n\r\n'
while [ ! -f '{directory}/approved' ]; do sleep 0.02; done
{write}
printf '{VENDOR_NOISE}\r\n'
exit {status}
"#)).unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    scripts
}

fn approve(scripts: &tempfile::TempDir) {
    std::fs::write(scripts.path().join("approved"), "").unwrap();
}

async fn private(request: reqwest::RequestBuilder) -> (StatusCode, Value) {
    let (status, body) = sent(request).await;
    let text = body.to_string();
    for secret in [ACCESS, "scripted-refresh", "scripted-id", VENDOR_NOISE] {
        assert!(!text.contains(secret), "vendor material leaked: {text}");
    }
    (status, body)
}

async fn device_started(kestrel: &Kestrel) -> Value {
    let (status, body) =
        private(reqwest::Client::new().post(format!("{}{CODEX}/relay", kestrel.operator()))).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    body
}

fn device_address(kestrel: &Kestrel, relay: &Value) -> String {
    format!(
        "{}{CODEX}/relay/{}",
        kestrel.operator(),
        relay["id"].as_str().unwrap()
    )
}

async fn device_until(kestrel: &Kestrel, relay: &Value, expected: &str) -> Value {
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let (status, body) =
                private(reqwest::Client::new().get(device_address(kestrel, relay))).await;
            assert_eq!(status, StatusCode::OK, "{body}");
            if body["progress"]["state"] == expected {
                return body;
            }
            assert!(
                !matches!(
                    body["progress"]["state"].as_str(),
                    Some("failed" | "expired" | "cancelled" | "completed")
                ),
                "{body}"
            );
            tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        }
    })
    .await
    .expect("the relay reaches its state")
}

async fn saved_sign_ins(kestrel: &Kestrel) -> Value {
    let (status, saved) = private(reqwest::Client::new().get(format!(
        "{}/operator/organizations/ada/sign-ins",
        kestrel.operator()
    )))
    .await;
    assert_eq!(status, StatusCode::OK);
    saved
}

#[tokio::test]
async fn device_authorization_shows_its_url_and_code_and_saves_the_approved_vendor_file() {
    let scripts = device_vendor(Some(LOGIN), 0);
    let kestrel = Kestrel::dispatching_with_programs(scripts.path()).await;
    named(&kestrel).await;
    let relay = device_started(&kestrel).await;
    assert_eq!(relay["destination"]["fills"]["path"], ".codex/auth.json");
    let waiting = device_until(&kestrel, &relay, "awaiting_browser").await;
    assert_eq!(waiting["progress"]["url"], DEVICE_URL);
    assert_eq!(waiting["progress"]["code"], DEVICE_CODE);
    let (status, refused) = private(
        reqwest::Client::new()
            .post(format!("{}/code", device_address(&kestrel, &relay)))
            .json(&json!({"code": DEVICE_CODE})),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{refused}");
    assert_eq!(refused["context"]["state"], "awaiting_browser");
    approve(&scripts);
    let completed = device_until(&kestrel, &relay, "completed").await;
    let saved = &completed["progress"]["sign_in"];
    assert_eq!(saved["authentication"]["state"], "login_completed");
    assert_eq!(saved["authentication"]["source"], "relay");
    assert_eq!(saved["model_use"], json!([]));
    assert_eq!(saved_sign_ins(&kestrel).await[0], *saved);
    cleaned(&kestrel, &relay).await;
    let metadata = kestrel
        .database()
        .relay_metadata(relay["id"].as_str().unwrap())
        .await;
    assert!(!metadata.contains(ACCESS));
}

#[tokio::test]
async fn a_missing_or_incompatible_vendor_file_fails_with_a_sanitized_import_path() {
    for login in [
        None,
        Some(r#"{"auth_mode":"apikey","OPENAI_API_KEY":"scripted-chatgpt-access-secret"}"#),
    ] {
        let scripts = device_vendor(login, 0);
        let kestrel = Kestrel::dispatching_with_programs(scripts.path()).await;
        named(&kestrel).await;
        let relay = device_started(&kestrel).await;
        device_until(&kestrel, &relay, "awaiting_browser").await;
        approve(&scripts);
        let failed = device_until(&kestrel, &relay, "failed").await;
        let diagnostic = &failed["progress"]["diagnostic"];
        assert!(
            diagnostic["message"]
                .as_str()
                .unwrap()
                .contains("auth.json"),
            "{diagnostic}"
        );
        assert!(
            diagnostic["next_steps"]
                .as_array()
                .unwrap()
                .iter()
                .any(|step| step["action"] == "sign_in" && step["method"] == "codex-device-auth"),
            "{diagnostic}"
        );
        cleaned(&kestrel, &relay).await;
        assert_eq!(saved_sign_ins(&kestrel).await, json!([]));
    }
}

#[tokio::test]
async fn a_failed_vendor_process_fails_without_its_output_and_a_retry_starts_fresh() {
    let scripts = device_vendor(Some(LOGIN), 1);
    let kestrel = Kestrel::dispatching_with_programs(scripts.path()).await;
    named(&kestrel).await;
    let relay = device_started(&kestrel).await;
    device_until(&kestrel, &relay, "awaiting_browser").await;
    approve(&scripts);
    let failed = device_until(&kestrel, &relay, "failed").await;
    assert!(failed["progress"]["diagnostic"].is_object());
    cleaned(&kestrel, &relay).await;
    assert_eq!(saved_sign_ins(&kestrel).await, json!([]));
    let next = device_started(&kestrel).await;
    assert_ne!(next["id"], relay["id"]);
}

#[tokio::test]
async fn device_authorization_shares_the_slot_cancellation_import_and_restart_lifecycle() {
    let scripts = device_vendor(Some(LOGIN), 0);
    let kestrel = Kestrel::dispatching_with_programs(scripts.path()).await;
    named(&kestrel).await;
    let relay = device_started(&kestrel).await;
    let (status, conflict) =
        private(reqwest::Client::new().post(format!("{}{CODEX}/relay", kestrel.operator()))).await;
    assert_eq!(status, StatusCode::CONFLICT, "{conflict}");
    assert_eq!(conflict["context"]["reference"], relay["id"]);
    device_until(&kestrel, &relay, "awaiting_browser").await;
    let (status, cancelled) =
        private(reqwest::Client::new().delete(device_address(&kestrel, &relay))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(cancelled["progress"]["state"], "cancelled");
    cleaned(&kestrel, &relay).await;

    let imported = device_started(&kestrel).await;
    device_until(&kestrel, &imported, "awaiting_browser").await;
    let (status, replacement) = private(
        reqwest::Client::new()
            .put(format!("{}{CODEX}", kestrel.operator()))
            .json(&json!({"value": LOGIN})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{replacement}");
    device_until(&kestrel, &imported, "cancelled").await;
    cleaned(&kestrel, &imported).await;
    approve(&scripts);
    let saved = saved_sign_ins(&kestrel).await;
    assert_eq!(saved[0]["revision"], replacement["revision"]);
    assert_eq!(saved[0]["authentication"]["state"], "unchecked");

    std::fs::remove_file(scripts.path().join("approved")).unwrap();
    let interrupted = device_started(&kestrel).await;
    device_until(&kestrel, &interrupted, "awaiting_browser").await;
    let kestrel = kestrel.kill_and_restart().await;
    device_until(&kestrel, &interrupted, "failed").await;
    cleaned(&kestrel, &interrupted).await;
    assert_eq!(
        saved_sign_ins(&kestrel).await[0]["revision"],
        replacement["revision"]
    );
}
