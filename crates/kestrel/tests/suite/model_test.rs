//! An explicit test of saved material: what its one request is handed, what it records against
//! which revision, harness and model, and what is left behind when it fails, lapses or is cut off.

use std::path::{Path, PathBuf};
use std::time::Duration;

use kestrel::profile::Entry;
use reqwest::StatusCode;
use serde_json::{Value, json};
use tempfile::TempDir;

use crate::support::Kestrel;
use crate::support::scripted_agent::{self, Script};
use crate::support::supervisor;

const OPERATOR: &str = "ada";
const SIGN_INS: &str = "/operator/organizations/ada/sign-ins";
const CLAUDE_TOKEN: &str = "sk-ant-oat01-subscription-secret";
const ANTHROPIC_KEY: &str = "sk-ant-api03-organization-secret";
const CODEX_LOGIN: &str = r#"{"OPENAI_API_KEY":null,"tokens":{"id_token":"id-secret","access_token":"access-secret","refresh_token":"refresh-secret"}}"#;
const PATIENCE: Duration = Duration::from_secs(60);

struct Answered {
    status: StatusCode,
    body: Value,
    text: String,
}

async fn sent(request: reqwest::RequestBuilder) -> Answered {
    let response = request.send().await.expect("the operator boundary answers");
    let status = response.status();
    let text = response.text().await.expect("a body");
    for secret in [
        CLAUDE_TOKEN,
        ANTHROPIC_KEY,
        "access-secret",
        "refresh-secret",
    ] {
        assert!(!text.contains(secret), "{text}");
    }

    Answered {
        status,
        body: serde_json::from_str(&text).unwrap_or(Value::Null),
        text,
    }
}

async fn save(kestrel: &Kestrel, harness: &str, method: &str, value: &str) -> Value {
    let saved = sent(
        reqwest::Client::new()
            .put(format!(
                "{}{SIGN_INS}/{harness}/{method}",
                kestrel.operator()
            ))
            .json(&json!({ "value": value })),
    )
    .await;
    assert_eq!(saved.status, StatusCode::OK, "{}", saved.text);

    saved.body
}

async fn saved(kestrel: &Kestrel, harness: &str, method: &str) -> Value {
    let listed =
        sent(reqwest::Client::new().get(format!("{}{SIGN_INS}", kestrel.operator()))).await;
    assert_eq!(listed.status, StatusCode::OK, "{}", listed.text);
    let found: Vec<&Value> = listed
        .body
        .as_array()
        .expect("saved sign-ins")
        .iter()
        .filter(|item| item["harness"] == harness && item["method"] == method)
        .collect();
    assert_eq!(found.len(), 1, "{}", listed.text);

    found[0].clone()
}

async fn start(kestrel: &Kestrel, harness: &str, method: &str, model: Option<&str>) -> Answered {
    let request = reqwest::Client::new().post(format!(
        "{}{SIGN_INS}/{harness}/{method}/tests",
        kestrel.operator()
    ));

    sent(match model {
        Some(model) => request.json(&json!({ "model": model })),
        None => request,
    })
    .await
}

async fn started(kestrel: &Kestrel, harness: &str, method: &str, model: Option<&str>) -> Value {
    let started = start(kestrel, harness, method, model).await;
    assert_eq!(started.status, StatusCode::ACCEPTED, "{}", started.text);
    assert_eq!(started.body["state"], "pending", "{}", started.text);

    started.body
}

async fn read(kestrel: &Kestrel, test: &Value) -> Value {
    let read = sent(reqwest::Client::new().get(format!(
        "{}{SIGN_INS}/{}/{}/tests/{}",
        kestrel.operator(),
        test["harness"].as_str().expect("a harness"),
        test["method"].as_str().expect("a method"),
        test["id"].as_str().expect("an id"),
    )))
    .await;
    assert_eq!(read.status, StatusCode::OK, "{}", read.text);

    read.body
}

async fn until(kestrel: &Kestrel, test: &Value, reached: impl Fn(&Value) -> bool) -> Value {
    let deadline = tokio::time::Instant::now() + PATIENCE;
    loop {
        let now = read(kestrel, test).await;
        if reached(&now) {
            return now;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the test never got there: {now:#}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

async fn ended(kestrel: &Kestrel, test: &Value) -> Value {
    until(kestrel, test, |test| {
        test["state"] != "pending" && test["state"] != "running"
    })
    .await
}

async fn tested(kestrel: &Kestrel, harness: &str, method: &str, model: Option<&str>) -> Value {
    let test = started(kestrel, harness, method, model).await;

    ended(kestrel, &test).await
}

/// Where the scripted agent writes what each prompt it was sent found around it.
struct Witness {
    _directory: TempDir,
    path: PathBuf,
}

impl Witness {
    fn new() -> Self {
        let directory = TempDir::new().expect("a temporary directory");
        let path = directory.path().join("witnessed");

        Self {
            _directory: directory,
            path,
        }
    }

    fn playing(&self, script: Script) -> String {
        format!(
            "{} --witness \"{}\"",
            scripted_agent::playing(script),
            self.path.display()
        )
    }

    fn prompts(&self) -> Vec<Value> {
        std::fs::read_to_string(&self.path)
            .unwrap_or_default()
            .lines()
            .map(|line| serde_json::from_str(line).expect("a witnessed prompt"))
            .collect()
    }
}

async fn dispatching(harnesses: &[(&str, &str)]) -> Kestrel {
    let kestrel = Kestrel::dispatching_harnesses(supervisor::binary(), harnesses).await;
    kestrel.name_operator(OPERATOR).await;

    kestrel
}

fn instance_of(test: &Value) -> PathBuf {
    std::env::temp_dir().join(format!(
        "kestrel-sign-in-test-{}",
        test["id"].as_str().expect("an id")
    ))
}

fn left_nothing_behind(test: &Value) {
    let instance = instance_of(test);
    assert!(!instance.exists(), "{} was left", instance.display());
    let home = Path::new(&format!("{}.home", instance.display())).to_owned();
    assert!(!home.exists(), "{} was left", home.display());
}

#[tokio::test]
async fn a_test_spends_one_request_on_the_selected_material_and_nothing_else() {
    let witness = Witness::new();
    let kestrel = dispatching(&[("claude", &witness.playing(Script::Speaks))]).await;
    let token = save(&kestrel, "claude", "claude-setup-token", CLAUDE_TOKEN).await;
    let organization = kestrel
        .organizations()
        .await
        .into_iter()
        .find(|organization| organization.name == OPERATOR)
        .expect("the Operator's Organization");
    kestrel
        .hold_provider_credential(&organization, "ANTHROPIC_API_KEY", ANTHROPIC_KEY)
        .await;
    kestrel
        .hold_provider_credential(&organization, "SCRIPTED_KEY", "an-unrelated-key")
        .await;
    for (entry, held) in [
        (Entry::variable("SCRIPTED_OTHER").unwrap(), "another-login"),
        (Entry::file(".other/config").unwrap(), "another-file"),
    ] {
        kestrel
            .hold_in_profile(OPERATOR, OPERATOR, &entry, held)
            .await;
    }

    let test = started(&kestrel, "claude", "claude-setup-token", None).await;
    assert_eq!(test["revision"], token["revision"]);
    assert_eq!(test["requested_model"], Value::Null);
    assert_eq!(test["outcome"], Value::Null);
    let test = ended(&kestrel, &test).await;

    assert_eq!(test["state"], "completed", "{test:#}");
    assert_eq!(test["outcome"], "worked");
    assert_eq!(test["model"], scripted_agent_model());
    assert_eq!(test["revision"], token["revision"]);

    let prompts = witness.prompts();
    assert_eq!(prompts.len(), 1, "{prompts:#?}");
    let environment = &prompts[0]["environment"];
    assert_eq!(environment["CLAUDE_CODE_OAUTH_TOKEN"], CLAUDE_TOKEN);
    assert_ne!(environment["ANTHROPIC_API_KEY"], ANTHROPIC_KEY);
    assert_eq!(environment["SCRIPTED_KEY"], Value::Null);
    assert_eq!(environment["SCRIPTED_OTHER"], Value::Null);
    assert!(
        environment
            .as_object()
            .expect("an environment")
            .keys()
            .all(|name| !name.starts_with("KESTREL_")),
        "{environment:#}"
    );
    assert_eq!(prompts[0]["home"][".other/config"], Value::Null);

    let after = saved(&kestrel, "claude", "claude-setup-token").await;
    assert_eq!(after["revision"], token["revision"]);
    assert_eq!(after["authentication"], token["authentication"]);
    let used = after["model_use"].as_array().expect("model use");
    assert_eq!(used.len(), 1, "{after:#}");
    assert_eq!(used[0]["harness"], "claude");
    assert_eq!(used[0]["model"], scripted_agent_model());
    assert_eq!(used[0]["result"], "worked");
    assert_eq!(used[0]["source"], "model_test");

    assert!(kestrel.workspaces(OPERATOR).await.is_empty());
    assert!(kestrel.database().instances().await.is_empty());
    left_nothing_behind(&test);

    kestrel.teardown().await;
}

fn scripted_agent_model() -> &'static str {
    kestrel_scripted_agent::DEFAULT_MODEL
}

#[tokio::test]
async fn each_model_keeps_its_own_result_and_one_not_offered_fails_alone() {
    let witness = Witness::new();
    let kestrel = dispatching(&[("claude", &witness.playing(Script::Speaks))]).await;
    save(&kestrel, "claude", "claude-setup-token", CLAUDE_TOKEN).await;

    let default = tested(&kestrel, "claude", "claude-setup-token", None).await;
    assert_eq!(default["outcome"], "worked", "{default:#}");
    let first = saved(&kestrel, "claude", "claude-setup-token").await["model_use"][0].clone();

    let other = kestrel_scripted_agent::OTHER_MODEL;
    let named = tested(&kestrel, "claude", "claude-setup-token", Some(other)).await;
    assert_eq!(named["state"], "completed", "{named:#}");
    assert_eq!(named["requested_model"], other);
    assert_eq!(named["model"], other);

    let missing = tested(
        &kestrel,
        "claude",
        "claude-setup-token",
        Some("scripted-none"),
    )
    .await;
    assert_eq!(missing["state"], "failed", "{missing:#}");
    assert_eq!(missing["outcome"], "not_covered");
    assert_eq!(missing["model"], "scripted-none");

    assert_eq!(
        witness.prompts().len(),
        2,
        "a model not offered is never prompted"
    );
    let after = saved(&kestrel, "claude", "claude-setup-token").await;
    assert_eq!(after["authentication"]["state"], "unchecked");
    let used: Vec<(&str, &str)> = after["model_use"]
        .as_array()
        .expect("model use")
        .iter()
        .map(|used| {
            (
                used["model"].as_str().expect("a model"),
                used["result"].as_str().expect("a result"),
            )
        })
        .collect();
    assert_eq!(
        used,
        [
            (other, "worked"),
            (scripted_agent_model(), "worked"),
            ("scripted-none", "not_covered"),
        ]
    );
    assert_eq!(
        after["model_use"][1], first,
        "the default model's result was rewritten"
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_key_two_harnesses_share_is_tested_for_the_one_named() {
    let witness = Witness::new();
    let kestrel = dispatching(&[
        ("claude", &witness.playing(Script::Speaks)),
        ("opencode", &witness.playing(Script::Speaks)),
    ])
    .await;
    let organization = kestrel
        .organizations()
        .await
        .into_iter()
        .find(|organization| organization.name == OPERATOR)
        .expect("the Operator's Organization");
    kestrel
        .hold_provider_credential(&organization, "ANTHROPIC_API_KEY", ANTHROPIC_KEY)
        .await;

    let test = tested(&kestrel, "claude", "anthropic-api-key", None).await;

    assert_eq!(test["state"], "completed", "{test:#}");
    let prompts = witness.prompts();
    assert_eq!(prompts.len(), 1, "{prompts:#?}");
    assert_eq!(
        prompts[0]["environment"]["ANTHROPIC_API_KEY"],
        ANTHROPIC_KEY
    );
    let claude = saved(&kestrel, "claude", "anthropic-api-key").await;
    let opencode = saved(&kestrel, "opencode", "anthropic-api-key").await;
    assert_eq!(claude["revision"], opencode["revision"]);
    assert_eq!(claude["model_use"].as_array().map(Vec::len), Some(1));
    assert_eq!(opencode["model_use"], json!([]));

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_method_that_routes_by_provider_names_its_model_and_enables_that_provider_alone() {
    let witness = Witness::new();
    let offering = format!(
        "{} --offers anthropic/scripted",
        witness.playing(Script::Speaks)
    );
    let kestrel = dispatching(&[("opencode", &offering)]).await;
    let organization = kestrel
        .organizations()
        .await
        .into_iter()
        .find(|organization| organization.name == OPERATOR)
        .expect("the Operator's Organization");
    kestrel
        .hold_provider_credential(&organization, "ANTHROPIC_API_KEY", ANTHROPIC_KEY)
        .await;

    let unnamed = start(&kestrel, "opencode", "anthropic-api-key", None).await;
    assert_eq!(
        unnamed.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        unnamed.text
    );
    assert_eq!(unnamed.body["kind"], "invalid_field");
    assert_eq!(unnamed.body["field"], "model");
    for incompatible in ["openai/gpt-scripted", "scripted-mini", "anthropic/"] {
        let refused = start(
            &kestrel,
            "opencode",
            "anthropic-api-key",
            Some(incompatible),
        )
        .await;
        assert_eq!(
            refused.status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{}",
            refused.text
        );
        assert_eq!(
            refused.body["context"]["allowed_values"],
            json!(["anthropic"])
        );
    }
    assert!(witness.prompts().is_empty(), "a refused test ran");

    let test = tested(
        &kestrel,
        "opencode",
        "anthropic-api-key",
        Some("anthropic/scripted"),
    )
    .await;

    assert_eq!(test["state"], "completed", "{test:#}");
    assert_eq!(test["model"], "anthropic/scripted");
    let prompts = witness.prompts();
    assert_eq!(prompts.len(), 1, "{prompts:#?}");
    let environment = &prompts[0]["environment"];
    assert_eq!(environment["ANTHROPIC_API_KEY"], ANTHROPIC_KEY);
    assert_eq!(
        serde_json::from_str::<Value>(
            environment["OPENCODE_CONFIG_CONTENT"]
                .as_str()
                .expect("the provider confinement")
        )
        .expect("a configuration"),
        json!({ "enabled_providers": ["anthropic"] })
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_refused_login_fails_the_test_marks_its_revision_and_still_reads() {
    let witness = Witness::new();
    let kestrel = dispatching(&[("claude", &witness.playing(Script::Insists))]).await;
    let token = save(&kestrel, "claude", "claude-setup-token", CLAUDE_TOKEN).await;

    let test = tested(&kestrel, "claude", "claude-setup-token", None).await;

    assert_eq!(test["state"], "failed", "{test:#}");
    assert_eq!(test["outcome"], "authentication_failed");
    assert!(test["message"].is_string(), "{test:#}");
    assert!(
        witness.prompts().is_empty(),
        "a login refused was still prompted"
    );
    let after = saved(&kestrel, "claude", "claude-setup-token").await;
    assert_eq!(
        after["revision"], token["revision"],
        "the material was not kept"
    );
    assert_eq!(after["authentication"]["state"], "authentication_failed");
    assert_eq!(after["authentication"]["source"], "model_test");
    assert_eq!(after["model_use"], json!([]));
    left_nothing_behind(&test);

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_subscription_in_use_is_refused_and_a_restart_interrupts_the_test_holding_it() {
    let kestrel = dispatching(&[("codex", &scripted_agent::playing(Script::Dawdles))]).await;
    let login = save(&kestrel, "codex", "codex-device-auth", CODEX_LOGIN).await;
    let holding = started(&kestrel, "codex", "codex-device-auth", None).await;
    until(&kestrel, &holding, |test| test["model"].is_string()).await;

    let refused = start(&kestrel, "codex", "codex-device-auth", None).await;

    assert_eq!(refused.status, StatusCode::CONFLICT, "{}", refused.text);
    assert_eq!(refused.body["kind"], "state_conflict");
    assert_eq!(refused.body["context"]["state"], "credential_in_use");
    assert_eq!(refused.body["context"]["resource"], "subscription_profile");
    assert_eq!(read(&kestrel, &holding).await["state"], "running");

    let kestrel = kestrel.kill_and_restart().await;

    let interrupted = read(&kestrel, &holding).await;
    assert_eq!(interrupted["state"], "interrupted", "{interrupted:#}");
    assert_eq!(interrupted["outcome"], Value::Null);
    left_nothing_behind(&interrupted);
    let after = saved(&kestrel, "codex", "codex-device-auth").await;
    assert_eq!(after["revision"], login["revision"]);
    assert_eq!(after["authentication"], login["authentication"]);
    assert_eq!(after["model_use"], json!([]));
    let again = start(&kestrel, "codex", "codex-device-auth", None).await;
    assert_eq!(again.status, StatusCode::ACCEPTED, "{}", again.text);
    assert_eq!(read(&kestrel, &holding).await["state"], "interrupted");

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_test_past_its_deadline_is_stopped_and_earlier_evidence_keeps_its_time() {
    let kestrel = Kestrel::dispatching_harnesses_testing_sign_ins_within(
        supervisor::binary(),
        &[("claude", &scripted_agent::playing(Script::Dawdles))],
        Duration::from_secs(2),
    )
    .await;
    kestrel.name_operator(OPERATOR).await;
    let token = save(&kestrel, "claude", "claude-setup-token", CLAUDE_TOKEN).await;
    kestrel
        .database()
        .record_model_use(
            token["revision"].as_i64().expect("a revision"),
            "claude",
            scripted_agent_model(),
            "worked",
        )
        .await;
    let before = saved(&kestrel, "claude", "claude-setup-token").await;

    let test = tested(&kestrel, "claude", "claude-setup-token", None).await;

    assert_eq!(test["state"], "failed", "{test:#}");
    assert_eq!(test["outcome"], "unavailable");
    assert_eq!(test["model"], scripted_agent_model());
    left_nothing_behind(&test);
    assert_eq!(
        saved(&kestrel, "claude", "claude-setup-token").await,
        before
    );
    let again = start(&kestrel, "claude", "claude-setup-token", None).await;
    assert_eq!(again.status, StatusCode::ACCEPTED, "{}", again.text);

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_login_the_harness_refreshed_is_kept_as_a_refresh_of_the_revision_tested() {
    let refreshing = format!(
        "{} --login .codex/auth.json",
        scripted_agent::playing(Script::Refreshes)
    );
    let kestrel = dispatching(&[("codex", &refreshing)]).await;
    let login = save(&kestrel, "codex", "codex-device-auth", CODEX_LOGIN).await;

    let test = tested(&kestrel, "codex", "codex-device-auth", None).await;

    assert_eq!(test["state"], "completed", "{test:#}");
    assert_eq!(test["revision"], login["revision"]);
    let after = saved(&kestrel, "codex", "codex-device-auth").await;
    assert_ne!(after["revision"], login["revision"]);
    assert_eq!(after["refreshed_from"], login["revision"]);
    assert_eq!(after["model_use"][0]["result"], "worked", "{after:#}");
    let (profile, _) = kestrel
        .profiles(OPERATOR)
        .await
        .into_iter()
        .next()
        .expect("the Operator's Profile");
    assert_eq!(
        kestrel.profile_contents(&profile).await.files[".codex/auth.json"],
        format!("{CODEX_LOGIN}{}", kestrel_scripted_agent::REFRESHED)
    );
    let again = start(&kestrel, "codex", "codex-device-auth", None).await;
    assert_eq!(again.status, StatusCode::ACCEPTED, "{}", again.text);

    kestrel.teardown().await;
}

#[tokio::test]
async fn an_image_that_cannot_be_inspected_refuses_the_test_and_nothing_is_started() {
    let kestrel = Kestrel::boot_inspecting("kestrel-env:not-on-this-machine").await;
    kestrel.name_operator(OPERATOR).await;
    let token = save(&kestrel, "claude", "claude-setup-token", CLAUDE_TOKEN).await;

    let refused = start(&kestrel, "claude", "claude-setup-token", None).await;

    assert_eq!(
        refused.status,
        StatusCode::SERVICE_UNAVAILABLE,
        "{}",
        refused.text
    );
    assert_eq!(refused.body["kind"], "unavailable");
    assert_eq!(refused.body["context"]["operation"], "test_sign_in");
    assert_eq!(
        saved(&kestrel, "claude", "claude-setup-token").await,
        token,
        "a refused test changed what is saved"
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn nothing_saved_is_nothing_to_test_and_nowhere_to_run_ends_the_test() {
    let kestrel = Kestrel::boot().await;
    kestrel.name_operator(OPERATOR).await;

    let unsaved = start(&kestrel, "claude", "claude-setup-token", None).await;
    assert_eq!(unsaved.status, StatusCode::CONFLICT, "{}", unsaved.text);
    assert_eq!(unsaved.body["context"]["state"], "sign_in_not_saved");
    let unknown = start(&kestrel, "claude", "no-such-method", None).await;
    assert_eq!(
        unknown.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        unknown.text
    );
    let missing = sent(reqwest::Client::new().get(format!(
        "{}{SIGN_INS}/claude/claude-setup-token/tests/no-such-test",
        kestrel.operator()
    )))
    .await;
    assert_eq!(missing.status, StatusCode::NOT_FOUND, "{}", missing.text);
    assert_eq!(missing.body["context"]["resource"], "sign_in_test");

    let token = save(&kestrel, "claude", "claude-setup-token", CLAUDE_TOKEN).await;
    let test = tested(&kestrel, "claude", "claude-setup-token", None).await;

    assert_eq!(test["state"], "failed", "{test:#}");
    assert_eq!(test["outcome"], "unavailable");
    assert_eq!(saved(&kestrel, "claude", "claude-setup-token").await, token);
    let elsewhere = sent(reqwest::Client::new().get(format!(
        "{}{SIGN_INS}/codex/codex-device-auth/tests/{}",
        kestrel.operator(),
        test["id"].as_str().expect("an id")
    )))
    .await;
    assert_eq!(
        elsewhere.status,
        StatusCode::NOT_FOUND,
        "{}",
        elsewhere.text
    );

    kestrel.teardown().await;
}
