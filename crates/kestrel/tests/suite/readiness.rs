//! Whether work can start, as the operator boundary reads it and bare `kestrel` renders it.

use jiff::SignedDuration;
use kestrel::domain::Direction;
use kestrel::profile::Entry;
use reqwest::StatusCode;
use serde_json::{Value, json};

use crate::support::Kestrel;
use crate::support::client::{Finished, Invocation, ran_by, ran_on_a_terminal_by};
use crate::support::github_stub::GithubStub;

const ADA: &str = "ada";
const PUBLIC: &str = "https://github.com/jtmthf/kestrel.git";
const CLAUDE_TOKEN: &str = "sk-ant-oat01-subscription-secret";
const CHATGPT_LOGIN: &str = r#"{"OPENAI_API_KEY":null,"tokens":{"id_token":"id-secret","access_token":"access-secret","refresh_token":"refresh-secret","account_id":"acct"}}"#;

async fn asked(kestrel: &Kestrel, query: &str) -> (StatusCode, Value) {
    let response = reqwest::get(format!("{}/operator/readiness{query}", kestrel.operator()))
        .await
        .expect("the operator boundary answers");
    let status = response.status();
    let text = response.text().await.expect("a body");
    for secret in [CLAUDE_TOKEN, "id-secret", "refresh-secret"] {
        assert!(!text.contains(secret), "{text}");
    }

    (status, serde_json::from_str(&text).unwrap_or(Value::Null))
}

async fn read(kestrel: &Kestrel, query: &str) -> Value {
    let (status, body) = asked(kestrel, query).await;
    assert_eq!(status, StatusCode::OK, "{body:#}");
    let _: kestrel_operator_types::Readiness =
        serde_json::from_value(body.clone()).expect("the authored readiness shape");

    body
}

async fn sent(request: reqwest::RequestBuilder) -> Value {
    let response = request.send().await.expect("the operator boundary answers");
    let status = response.status();
    let text = response.text().await.expect("a body");
    assert!(status.is_success(), "{status}: {text}");

    serde_json::from_str(&text).unwrap_or(Value::Null)
}

async fn name_the_operator(kestrel: &Kestrel) {
    sent(
        reqwest::Client::new()
            .put(format!("{}/operator/operator", kestrel.operator()))
            .json(&json!({ "name": ADA })),
    )
    .await;
}

/// A subscription, which no provider is asked about, so it is saved unchecked.
async fn sign_in(kestrel: &Kestrel, harness: &str, method: &str, value: &str) -> i64 {
    sent(
        reqwest::Client::new()
            .put(format!(
                "{}/operator/organizations/{ADA}/sign-ins/{harness}/{method}",
                kestrel.operator()
            ))
            .json(&json!({ "value": value })),
    )
    .await["revision"]
        .as_i64()
        .expect("a revision")
}

async fn declare_a_project(kestrel: &Kestrel) {
    sent(
        reqwest::Client::new()
            .post(format!(
                "{}/operator/organizations/{ADA}/projects",
                kestrel.operator()
            ))
            .json(&json!({ "name": "kestrel", "repositories": [PUBLIC], "branch": "main" })),
    )
    .await;
}

/// Named, signed in to Claude with a subscription a login completed, and with a Project.
async fn ready(kestrel: &Kestrel) -> i64 {
    name_the_operator(kestrel).await;
    let revision = sign_in(kestrel, "claude", "claude-setup-token", CLAUDE_TOKEN).await;
    kestrel
        .database()
        .record_authentication(revision, "login_completed")
        .await;
    declare_a_project(kestrel).await;

    revision
}

fn prerequisites(diagnostics: &Value) -> Vec<&str> {
    diagnostics
        .as_array()
        .expect("diagnostics")
        .iter()
        .map(|diagnostic| {
            diagnostic["context"]["prerequisite"]
                .as_str()
                .unwrap_or_else(|| diagnostic["kind"].as_str().expect("a kind"))
        })
        .collect()
}

fn actions(diagnostic: &Value) -> Vec<&str> {
    diagnostic["next_steps"]
        .as_array()
        .expect("next steps")
        .iter()
        .map(|step| step["action"].as_str().expect("an action"))
        .collect()
}

#[tokio::test]
async fn a_new_install_reports_every_gap_it_can_establish_in_repair_order() {
    let kestrel = Kestrel::boot().await;

    let read = read(&kestrel, "").await;

    assert_eq!(read["outcome"], "blocked");
    assert_eq!(read["organization"], Value::Null);
    assert_eq!(read["initial_setup_complete"], false);
    assert_eq!(read["client_url"], "http://localhost:7719");
    assert_eq!(
        prerequisites(&read["gaps"]),
        ["operator", "sign_in", "repository"]
    );
    assert_eq!(actions(&read["gaps"][0]), ["name_operator"]);
    assert_eq!(actions(&read["gaps"][2]), ["connect_github"]);
    assert_eq!(read["unavailable"], json!([]));
    kestrel.teardown().await;
}

#[tokio::test]
async fn naming_the_operator_saves_one_prerequisite_and_leaves_setup_unfinished() {
    let kestrel = Kestrel::boot().await;
    name_the_operator(&kestrel).await;

    let read = read(&kestrel, "").await;

    assert_eq!(read["outcome"], "blocked");
    assert_eq!(read["organization"], ADA);
    assert_eq!(read["saved"]["operator"]["name"], ADA);
    assert_eq!(read["initial_setup_complete"], false);
    assert_eq!(prerequisites(&read["gaps"]), ["sign_in", "repository"]);
    assert_eq!(
        read["gaps"][0]["next_steps"],
        json!([{ "action": "sign_in", "harness": null, "method": null, "sign_in": null }])
    );
    assert_eq!(
        actions(&read["gaps"][1]),
        ["connect_github", "declare_project"]
    );
    assert_eq!(read["gaps"][1]["next_steps"][0]["organization"], ADA);
    assert_eq!(
        read["gaps"][1]["next_steps"][1]["missing"],
        json!(["name", "repositories", "branch"])
    );
    kestrel.teardown().await;
}

#[tokio::test]
async fn a_lending_github_integration_leaves_only_the_project_to_choose() {
    let stub = GithubStub::start();
    let kestrel = Kestrel::boot().await;
    name_the_operator(&kestrel).await;
    sign_in(&kestrel, "claude", "claude-setup-token", CLAUDE_TOKEN).await;
    kestrel
        .register_integration(
            ADA,
            "github",
            "jtmthf/kestrel",
            &stub.base_url(),
            &[Direction::Outbound],
            SignedDuration::from_hours(1),
        )
        .await;

    let connected = read(&kestrel, "").await;
    assert_eq!(prerequisites(&connected["gaps"]), ["project"]);
    assert_eq!(actions(&connected["gaps"][0]), ["declare_project"]);
    assert_eq!(connected["saved"]["github_integrations"], json!(["github"]));

    kestrel.disable_integration(ADA, "github").await;
    let disabled = read(&kestrel, "").await;
    assert_eq!(prerequisites(&disabled["gaps"]), ["repository"]);
    assert_eq!(disabled["saved"]["github_integrations"], json!([]));
    let paused = disabled["warnings"]
        .as_array()
        .expect("warnings")
        .iter()
        .find(|warning| warning["context"]["resource"] == "integration")
        .expect("the disabled Integration's warning");
    assert_eq!(actions(paused), ["inspect_resource", "enable_integration"]);
    kestrel.teardown().await;
}

#[tokio::test]
async fn a_public_repository_is_ready_without_github_and_unchecked_material_warns() {
    let kestrel = Kestrel::boot().await;
    name_the_operator(&kestrel).await;
    let revision = sign_in(&kestrel, "claude", "claude-setup-token", CLAUDE_TOKEN).await;
    declare_a_project(&kestrel).await;

    let unchecked = read(&kestrel, "").await;
    assert_eq!(unchecked["outcome"], "ready", "{unchecked:#}");
    assert_eq!(unchecked["gaps"], json!([]));
    assert_eq!(
        unchecked["selected"],
        json!({
            "harness": "claude",
            "model": null,
            "profile": ADA,
            "sign_in_method": "claude-setup-token",
            "project": "kestrel",
        })
    );
    assert_eq!(unchecked["sign_in"]["revision"], revision);
    assert_eq!(unchecked["saved"]["projects"], json!(["kestrel"]));
    assert_eq!(unchecked["saved"]["github_integrations"], json!([]));
    assert_eq!(prerequisites(&unchecked["warnings"]), ["sign_in_unchecked"]);
    assert_eq!(
        read(&kestrel, "").await,
        unchecked,
        "a read changed something"
    );

    kestrel
        .database()
        .record_authentication(revision, "login_completed")
        .await;
    let checked = read(&kestrel, "").await;
    assert_eq!(checked["outcome"], "ready");
    assert_eq!(checked["warnings"], json!([]));
    assert_eq!(checked["sign_in"]["revision"], revision);
    kestrel.teardown().await;
}

#[tokio::test]
async fn a_broken_sign_in_blocks_only_the_path_that_selects_it() {
    let kestrel = Kestrel::boot().await;
    let claude = ready(&kestrel).await;
    let codex = sign_in(&kestrel, "codex", "codex-device-auth", CHATGPT_LOGIN).await;
    let database = kestrel.database();
    database
        .record_authentication(codex, "login_completed")
        .await;
    database
        .record_authentication(claude, "authentication_failed")
        .await;

    let any = read(&kestrel, "").await;
    assert_eq!(any["outcome"], "ready", "{any:#}");
    assert_eq!(any["selected"]["harness"], "codex");
    assert_eq!(any["sign_in"]["revision"], codex);
    assert_eq!(prerequisites(&any["warnings"]), ["sign_in_attention"]);
    assert_eq!(any["warnings"][0]["context"]["harness"], "claude");

    let selected = read(&kestrel, "?harness=claude").await;
    assert_eq!(selected["outcome"], "blocked");
    assert_eq!(prerequisites(&selected["gaps"]), ["sign_in_attention"]);
    let gap = &selected["gaps"][0];
    assert_eq!(gap["context"]["method"], "claude-setup-token");
    assert_eq!(gap["context"]["resource"], "subscription_profile");
    assert_eq!(gap["context"]["sign_in"], ADA);
    assert_eq!(gap["context"]["organization"], ADA);
    assert_eq!(actions(gap), ["sign_in", "inspect_resource"]);
    assert_eq!(selected["warnings"], json!([]));

    assert_eq!(read(&kestrel, "?harness=codex").await["outcome"], "ready");

    database.record_authentication(codex, "expired").await;
    let none = read(&kestrel, "").await;
    assert_eq!(none["outcome"], "blocked");
    assert_eq!(
        prerequisites(&none["gaps"]),
        ["sign_in_attention", "sign_in_attention"]
    );
    kestrel.teardown().await;
}

#[tokio::test]
async fn a_models_evidence_blocks_that_model_and_an_untested_one_is_no_prerequisite() {
    let kestrel = Kestrel::boot().await;
    let revision = ready(&kestrel).await;
    kestrel
        .database()
        .record_model_use(revision, "claude", "opus", "not_covered")
        .await;

    assert_eq!(read(&kestrel, "").await["outcome"], "ready");
    assert_eq!(
        read(&kestrel, "?harness=claude&model=sonnet").await["outcome"],
        "ready"
    );
    let uncovered = read(&kestrel, "?harness=claude&model=opus").await;
    assert_eq!(uncovered["outcome"], "blocked");
    assert_eq!(uncovered["selected"]["model"], "opus");
    // Nothing says signing in again buys coverage.
    assert_eq!(actions(&uncovered["gaps"][0]), ["inspect_resource"]);
    kestrel.teardown().await;
}

#[tokio::test]
async fn setup_once_complete_stays_complete_through_a_later_gap() {
    let kestrel = Kestrel::boot().await;
    let revision = ready(&kestrel).await;
    assert_eq!(read(&kestrel, "").await["initial_setup_complete"], false);

    kestrel.database().complete_initial_setup().await;
    kestrel
        .database()
        .record_authentication(revision, "authentication_failed")
        .await;

    let later = read(&kestrel, "").await;
    assert_eq!(later["outcome"], "blocked");
    assert_eq!(later["initial_setup_complete"], true);
    assert_eq!(prerequisites(&later["gaps"]), ["sign_in_attention"]);
    kestrel.teardown().await;
}

#[tokio::test]
async fn an_image_that_cannot_be_inspected_is_unavailable_and_never_an_absent_harness() {
    let kestrel = Kestrel::boot_inspecting("kestrel-env:no-such-readiness-image").await;

    let unnamed = read(&kestrel, "").await;
    assert_eq!(unnamed["outcome"], "blocked");
    assert_eq!(
        prerequisites(&unnamed["gaps"]),
        ["operator", "sign_in", "repository"]
    );
    assert_eq!(prerequisites(&unnamed["unavailable"]), ["unavailable"]);

    ready(&kestrel).await;
    for query in ["", "?harness=claude"] {
        let read = read(&kestrel, query).await;
        assert_eq!(read["outcome"], "unavailable", "{read:#}");
        assert_eq!(read["gaps"], json!([]));
        let failed = &read["unavailable"][0];
        assert_eq!(failed["kind"], "unavailable");
        assert_eq!(failed["context"]["service"], "image_inspection");
        assert_eq!(
            failed["context"]["resource"],
            "kestrel-env:no-such-readiness-image"
        );
        assert_eq!(actions(failed), ["inspect_harness_image", "retry_read"]);
    }
    kestrel.teardown().await;
}

#[tokio::test]
async fn the_client_url_is_the_one_configured() {
    let kestrel = Kestrel::boot_with_the_client_at("http://localhost:7800").await;

    assert_eq!(
        read(&kestrel, "").await["client_url"],
        "http://localhost:7800"
    );
    kestrel.teardown().await;
}

#[tokio::test]
async fn a_scope_or_selection_is_never_chosen_for_the_reader() {
    let kestrel = Kestrel::boot().await;
    ready(&kestrel).await;
    kestrel.declare_organization("globex").await;

    let (status, several) = asked(&kestrel, "").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(several["kind"], "ambiguous_reference");
    let candidates: Vec<&str> = several["context"]["candidates"]
        .as_array()
        .expect("candidates")
        .iter()
        .map(|candidate| candidate["name"].as_str().expect("a name"))
        .collect();
    assert_eq!(candidates, [ADA, "globex"]);

    assert_eq!(
        read(&kestrel, "?organization=ada").await["outcome"],
        "ready"
    );
    assert_eq!(
        prerequisites(&read(&kestrel, "?organization=globex").await["gaps"]),
        ["sign_in", "repository"]
    );

    let (status, absent) = asked(&kestrel, "?organization=absent").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(absent["kind"], "missing_reference");

    for query in [
        "?organization=ada&sign_in_method=unknown",
        "?organization=ada&harness=codex&sign_in_method=claude-setup-token",
    ] {
        let (status, refused) = asked(&kestrel, query).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{query}");
        assert_eq!(refused["field"], "sign_in_method", "{query}");
    }

    for (query, resource) in [
        ("?organization=ada&project=absent", "project"),
        ("?organization=ada&profile=absent", "subscription_profile"),
    ] {
        let named = read(&kestrel, query).await;
        assert_eq!(named["outcome"], "blocked", "{query}");
        assert_eq!(prerequisites(&named["gaps"]), ["missing_reference"]);
        assert_eq!(named["gaps"][0]["context"]["resource"], resource);
    }
    kestrel.teardown().await;
}

#[tokio::test]
async fn a_named_profile_and_method_test_that_path_and_no_other() {
    let kestrel = Kestrel::boot().await;
    name_the_operator(&kestrel).await;
    declare_a_project(&kestrel).await;
    kestrel
        .declare_profile(ADA, "shared", "grace")
        .await
        .expect("a profile someone else owns");
    kestrel
        .hold_in_profile(
            ADA,
            "shared",
            &Entry::variable("CLAUDE_CODE_OAUTH_TOKEN").expect("a variable"),
            CLAUDE_TOKEN,
        )
        .await;

    // Someone else's Profile is never taken unnamed.
    let unnamed = read(&kestrel, "?harness=claude").await;
    assert_eq!(prerequisites(&unnamed["gaps"]), ["sign_in"]);
    assert_eq!(unnamed["gaps"][0]["context"]["harness"], "claude");

    let named = read(&kestrel, "?harness=claude&profile=shared").await;
    assert_eq!(named["outcome"], "ready", "{named:#}");
    assert_eq!(named["selected"]["profile"], "shared");
    assert_eq!(named["sign_in"]["holding"]["reference"], "shared");

    let key = read(
        &kestrel,
        "?harness=claude&profile=shared&sign_in_method=anthropic-api-key",
    )
    .await;
    assert_eq!(prerequisites(&key["gaps"]), ["sign_in"]);
    assert_eq!(key["gaps"][0]["context"]["method"], "anthropic-api-key");
    kestrel.teardown().await;
}

fn exited(finished: &Finished, code: i32) {
    assert_eq!(
        finished.status.code(),
        Some(code),
        "stdout: {:?}\nstderr: {}",
        finished.out,
        finished.err
    );
}

#[tokio::test]
async fn bare_kestrel_says_ready_on_stdout_and_exits_zero() {
    let kestrel = Kestrel::boot().await;
    ready(&kestrel).await;

    let finished = ran_by(&kestrel, &[], Invocation::default()).await;

    exited(&finished, 0);
    assert_eq!(
        finished.out,
        [
            "Ready · Organization ada · claude · Profile ada".to_owned(),
            "Client: http://localhost:7719".to_owned(),
            format!(
                "Next: kestrel start --brief <BRIEF> --control-plane {}",
                kestrel.operator()
            ),
        ]
    );
    assert_eq!(finished.err, "");
    kestrel.teardown().await;
}

#[tokio::test]
async fn bare_kestrel_shows_the_first_gap_on_stdout_and_its_step_on_stderr_and_exits_78() {
    let kestrel = Kestrel::boot().await;

    let unnamed = ran_by(&kestrel, &[], Invocation::default()).await;
    exited(&unnamed, 78);
    assert!(
        unnamed.out[0].starts_with("Not ready: this install has no Operator yet"),
        "{:?}",
        unnamed.out
    );
    assert_eq!(unnamed.out[1], "Client: http://localhost:7719");
    assert_eq!(unnamed.out.len(), 2, "{:?}", unnamed.out);
    assert!(
        unnamed.err.contains("kestrel operator name <NAME>"),
        "{}",
        unnamed.err
    );

    name_the_operator(&kestrel).await;
    let unsigned = ran_by(&kestrel, &[], Invocation::default()).await;
    exited(&unsigned, 78);
    assert_eq!(
        unsigned.out[0],
        "Not ready: no harness is signed in in the organization ada"
    );
    assert!(unsigned.err.contains("sign in"), "{}", unsigned.err);
    assert!(
        !unsigned.err.contains("repository"),
        "only the first gap's repair is shown: {}",
        unsigned.err
    );
    kestrel.teardown().await;
}

#[tokio::test]
async fn a_correction_keeps_the_connection_and_the_organization() {
    let stub = GithubStub::start();
    let kestrel = Kestrel::boot().await;
    name_the_operator(&kestrel).await;
    sign_in(&kestrel, "claude", "claude-setup-token", CLAUDE_TOKEN).await;
    kestrel
        .register_integration(
            ADA,
            "github",
            "jtmthf/kestrel",
            &stub.base_url(),
            &[Direction::Outbound],
            SignedDuration::from_hours(1),
        )
        .await;
    kestrel.declare_organization("globex").await;

    let finished = ran_by(
        &kestrel,
        &["--organization", ADA],
        Invocation::default().env("NO_COLOR", "1"),
    )
    .await;

    exited(&finished, 78);
    assert!(
        finished.out[0].starts_with("Not ready: no project is declared in the organization ada"),
        "{:?}",
        finished.out
    );
    assert!(
        finished.err.contains(&format!(
            "kestrel project declare <NAME> --repository <REPOSITORY> --branch <BRANCH> \
             --organization ada --control-plane {}",
            kestrel.operator()
        )),
        "{}",
        finished.err
    );

    let unscoped = ran_by(&kestrel, &[], Invocation::default()).await;
    exited(&unscoped, 3);
    assert!(unscoped.out.is_empty(), "{:?}", unscoped.out);
    kestrel.teardown().await;
}

#[tokio::test]
async fn a_usable_alternative_is_ready_with_the_broken_sign_in_as_a_warning() {
    let kestrel = Kestrel::boot().await;
    let claude = ready(&kestrel).await;
    let codex = sign_in(&kestrel, "codex", "codex-device-auth", CHATGPT_LOGIN).await;
    kestrel
        .database()
        .record_authentication(codex, "login_completed")
        .await;
    kestrel
        .database()
        .record_authentication(claude, "authentication_failed")
        .await;

    let finished = ran_by(&kestrel, &[], Invocation::default()).await;

    exited(&finished, 0);
    assert_eq!(
        finished.out[0],
        "Ready · Organization ada · codex · Profile ada"
    );
    assert!(
        finished
            .err
            .starts_with("warning: Claude subscription, the claude sign-in"),
        "{}",
        finished.err
    );
    kestrel.teardown().await;
}

#[tokio::test]
async fn an_inspection_outage_is_unavailable_and_exits_5() {
    let kestrel = Kestrel::boot_inspecting("kestrel-env:no-such-readiness-image").await;
    ready(&kestrel).await;

    let finished = ran_by(&kestrel, &[], Invocation::default()).await;

    exited(&finished, 5);
    assert!(
        finished.out[0].starts_with("Readiness unavailable: the image"),
        "{:?}",
        finished.out
    );
    assert_eq!(finished.out[1], "Client: http://localhost:7719");
    assert!(!finished.err.contains("does not carry"), "{}", finished.err);
    kestrel.teardown().await;
}

#[tokio::test]
async fn an_inspection_outage_behind_a_gap_is_still_said() {
    let kestrel = Kestrel::boot_inspecting("kestrel-env:no-such-readiness-image").await;

    let finished = ran_by(&kestrel, &[], Invocation::default()).await;

    exited(&finished, 78);
    assert!(
        finished
            .err
            .contains("warning: the image kestrel-env:no-such-readiness-image"),
        "{}",
        finished.err
    );
    kestrel.teardown().await;
}

#[tokio::test]
async fn json_is_the_whole_read_on_stdout_and_the_blocking_diagnostic_on_stderr() {
    let kestrel = Kestrel::boot_with_the_client_at("http://localhost:7800").await;
    name_the_operator(&kestrel).await;

    let finished = ran_by(&kestrel, &["--json"], Invocation::default()).await;

    exited(&finished, 78);
    assert_eq!(finished.out.len(), 1, "{:?}", finished.out);
    let printed: Value = serde_json::from_str(&finished.out[0]).expect("one JSON document");
    assert_eq!(printed, read(&kestrel, "?organization=ada").await);
    assert_eq!(printed["client_url"], "http://localhost:7800");
    let diagnostic: Value = serde_json::from_str(finished.err.trim()).expect("one JSON line");
    assert_eq!(diagnostic, printed["gaps"][0]);
    kestrel.teardown().await;
}

#[tokio::test]
async fn only_a_terminal_that_did_not_ask_otherwise_is_decorated() {
    let kestrel = Kestrel::boot().await;
    ready(&kestrel).await;

    let piped = ran_by(&kestrel, &[], Invocation::default()).await;
    assert!(!piped.out.join("\n").contains('\u{1b}'), "{:?}", piped.out);

    let terminal = ran_on_a_terminal_by(&kestrel, &[], Invocation::default(), "").await;
    assert!(
        terminal
            .said
            .contains("\u{1b}[1;32mReady\u{1b}[0m · Organization ada"),
        "{:?}",
        terminal.said
    );

    let asked = ran_on_a_terminal_by(
        &kestrel,
        &[],
        Invocation::default().env("NO_COLOR", "1"),
        "",
    )
    .await;
    assert!(!asked.said.contains('\u{1b}'), "{:?}", asked.said);
    assert!(asked.said.contains("Ready · Organization ada"));
    kestrel.teardown().await;
}
