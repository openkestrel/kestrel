//! What a Session's harness is actually handed, which revision of it, and what a failure or a
//! refresh is then allowed to say about that revision and no other.

use std::collections::BTreeSet;

use kestrel::domain::{Session, SessionId};
use kestrel::profile::Entry;
use reqwest::StatusCode;
use serde_json::{Value, json};

use crate::support::fixture::Fixture;
use crate::support::link_client::Link;
use crate::support::provider_stub::ProviderStub;
use crate::support::{Kestrel, OnTheLink, SERIALIZED, repository};

const OPERATOR: &str = "ada";
const ORGANIZATION: &str = "/operator/organizations/acme";
const CLAUDE_TOKEN: &str = "sk-ant-oat01-first-subscription";
const NEWER_CLAUDE_TOKEN: &str = "sk-ant-oat01-newer-subscription";
const ANTHROPIC_KEY: &str = "sk-ant-api03-organization-key";
const CODEX_LOGIN: &str = ".codex/auth.json";

fn chatgpt_login(generation: &str) -> String {
    json!({
        "OPENAI_API_KEY": null,
        "tokens": {
            "id_token": format!("id-{generation}"),
            "access_token": format!("access-{generation}"),
            "refresh_token": format!("refresh-{generation}"),
        },
    })
    .to_string()
}

struct Answered {
    status: StatusCode,
    body: Value,
}

async fn sent(request: reqwest::RequestBuilder) -> Answered {
    let response = request.send().await.expect("the operator boundary answers");
    let status = response.status();
    let text = response.text().await.expect("a body");

    Answered {
        status,
        body: serde_json::from_str(&text).unwrap_or(Value::String(text)),
    }
}

async fn put(kestrel: &Kestrel, path: &str, body: Value) -> Answered {
    sent(
        reqwest::Client::new()
            .put(format!("{}{path}", kestrel.operator()))
            .json(&body),
    )
    .await
}

async fn post(kestrel: &Kestrel, path: &str, body: Value) -> Answered {
    sent(
        reqwest::Client::new()
            .post(format!("{}{path}", kestrel.operator()))
            .json(&body),
    )
    .await
}

async fn get(kestrel: &Kestrel, path: &str) -> Value {
    let read = sent(reqwest::Client::new().get(format!("{}{path}", kestrel.operator()))).await;
    assert_eq!(read.status, StatusCode::OK, "{:#}", read.body);

    read.body
}

/// An install with its Operator named and one Agent, `builder`, on `harness`.
async fn booted(stub: &ProviderStub, harness: &str) -> Kestrel {
    let kestrel = Kestrel::boot_checking_sign_ins_at(&stub.base_url()).await;
    let named = put(&kestrel, "/operator/operator", json!({ "name": OPERATOR })).await;
    assert_eq!(named.status, StatusCode::CREATED, "{:#}", named.body);
    Fixture::acme()
        .checked_out()
        .harness(harness)
        .declare(&kestrel)
        .await;

    kestrel
}

/// The revision the save minted.
async fn save(kestrel: &Kestrel, harness: &str, method: &str, value: &str) -> i64 {
    let saved = put(
        kestrel,
        &format!("{ORGANIZATION}/sign-ins/{harness}/{method}"),
        json!({ "value": value }),
    )
    .await;
    assert_eq!(saved.status, StatusCode::OK, "{:#}", saved.body);

    saved.body["revision"].as_i64().expect("a revision")
}

async fn sign_in(kestrel: &Kestrel, harness: &str, method: &str) -> Value {
    let listed = get(kestrel, &format!("{ORGANIZATION}/sign-ins")).await;
    let found: Vec<&Value> = listed
        .as_array()
        .expect("saved sign-ins")
        .iter()
        .filter(|item| item["harness"] == harness && item["method"] == method)
        .collect();
    assert_eq!(found.len(), 1, "{listed:#}");

    found[0].clone()
}

async fn open(kestrel: &Kestrel, selecting: Value) -> Answered {
    let mut declaration = json!({
        "project": repository::NAME,
        "agent": "builder",
        "brief": "do the work",
    });
    declaration
        .as_object_mut()
        .expect("an object")
        .extend(selecting.as_object().expect("an object").clone());

    post(kestrel, &format!("{ORGANIZATION}/workspaces"), declaration).await
}

/// Opened, claimed and on an Instance's link, with its first Turn begun.
async fn carried(kestrel: &Kestrel, selecting: Value) -> (Session, OnTheLink) {
    let opened = open(kestrel, selecting).await;
    assert_eq!(opened.status, StatusCode::CREATED, "{:#}", opened.body);

    on_the_link(kestrel).await
}

async fn on_the_link(kestrel: &Kestrel) -> (Session, OnTheLink) {
    let session = kestrel.claim_session().await.expect("a session to claim");
    let on = kestrel.on_the_link(&session).await;
    kestrel.start_on_the_link(&session).await;

    (kestrel.session(session.id).await, on)
}

async fn handed(kestrel: &Kestrel, session: &Session, on: &OnTheLink) -> Value {
    let response = Link::to(&kestrel.link())
        .credentials(&on.instance, session.id, Some(&on.credential))
        .await;
    assert_eq!(response.status(), StatusCode::OK);

    response.json().await.expect("credentials")
}

fn names(handed: &Value, of: &str) -> BTreeSet<String> {
    handed[of]
        .as_object()
        .expect("named values")
        .keys()
        .cloned()
        .collect()
}

async fn report(kestrel: &Kestrel, on: &OnTheLink, body: Value) {
    let taken = Link::to(&kestrel.link())
        .report_body(&on.instance, Some(&on.credential), &body)
        .await;
    assert_eq!(taken.status(), StatusCode::ACCEPTED, "{body}");
}

async fn refused_authentication(kestrel: &Kestrel, session: &Session, on: &OnTheLink, seq: i64) {
    report(
        kestrel,
        on,
        json!({
            "kind": "finished",
            "session": session.id,
            "seq": seq,
            "exit": {"status": "failed", "because": "the agent would not open a session"},
            "evidence": {
                "kind": "authentication_required",
                "code": -32000,
                "methods": [],
                "method": null,
            },
        }),
    )
    .await;
}

async fn read(kestrel: &Kestrel, session: SessionId) -> Value {
    get(kestrel, &format!("{ORGANIZATION}/sessions/{session}")).await
}

fn supplied<'a>(session: &'a Value, name: &str) -> &'a Value {
    session["supplied"]
        .as_array()
        .expect("supplied material")
        .iter()
        .find(|supplied| supplied["name"] == name)
        .unwrap_or_else(|| panic!("nothing named {name} was supplied: {session:#}"))
}

#[tokio::test]
async fn a_method_is_checked_against_its_harness_and_what_holds_it_before_work_is_accepted() {
    let stub = ProviderStub::start();
    let kestrel = booted(&stub, "claude").await;
    save(&kestrel, "claude", "claude-setup-token", CLAUDE_TOKEN).await;

    let another_harnesses = open(
        &kestrel,
        json!({ "profile": OPERATOR, "sign_in_method": "codex-device-auth" }),
    )
    .await;
    assert!(another_harnesses.status.is_client_error());
    assert_eq!(another_harnesses.body["kind"], "invalid_field");
    assert_eq!(another_harnesses.body["field"], "sign_in_method");

    let no_profile = open(&kestrel, json!({ "sign_in_method": "claude-setup-token" })).await;
    assert!(no_profile.status.is_client_error());
    assert_eq!(no_profile.body["kind"], "invalid_field");
    assert_eq!(no_profile.body["field"], "sign_in_method");

    let never_saved = open(
        &kestrel,
        json!({ "profile": OPERATOR, "sign_in_method": "anthropic-api-key" }),
    )
    .await;
    assert_eq!(never_saved.status, StatusCode::CONFLICT);
    assert_eq!(never_saved.body["kind"], "state_conflict");
    assert_eq!(never_saved.body["context"]["state"], "sign_in_not_saved");
    assert_eq!(
        get(&kestrel, &format!("{ORGANIZATION}/workspaces")).await,
        json!([]),
        "a refused selection opened a Workspace"
    );

    let opened = open(
        &kestrel,
        json!({ "profile": OPERATOR, "sign_in_method": "claude-setup-token" }),
    )
    .await;
    assert_eq!(opened.status, StatusCode::CREATED, "{:#}", opened.body);
    assert_eq!(
        opened.body["session"]["sign_in_method"],
        "claude-setup-token"
    );
    let first: SessionId = opened.body["session"]["id"]
        .as_str()
        .expect("an id")
        .parse()
        .expect("a session id");
    assert_eq!(
        read(&kestrel, first).await["sign_in_method"],
        "claude-setup-token"
    );

    kestrel.stop_session(first).await;
    let sessions = format!(
        "{ORGANIZATION}/workspaces/{}/sessions",
        opened.body["workspace"]["id"].as_str().expect("an id")
    );
    let unsaved = post(
        &kestrel,
        &sessions,
        json!({ "sign_in_method": "anthropic-api-key" }),
    )
    .await;
    assert_eq!(unsaved.status, StatusCode::CONFLICT, "{:#}", unsaved.body);

    let next = post(&kestrel, &sessions, json!({})).await;
    assert_eq!(next.status, StatusCode::CREATED, "{:#}", next.body);
    assert_eq!(
        next.body["sign_in_method"], "claude-setup-token",
        "only an explicit method switches credentials"
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_selected_sign_in_is_handed_over_without_the_ones_it_competes_with() {
    let stub = ProviderStub::start();
    let kestrel = booted(&stub, "claude").await;
    let token = save(&kestrel, "claude", "claude-setup-token", CLAUDE_TOKEN).await;
    save(&kestrel, "claude", "anthropic-api-key", ANTHROPIC_KEY).await;
    save(&kestrel, "opencode", "openai-api-key", "sk-proj-unrelated").await;
    kestrel
        .hold_in_profile(
            "acme",
            OPERATOR,
            &Entry::variable("GH_TOKEN").expect("a variable"),
            "a-github-token",
        )
        .await;

    let (session, on) = carried(
        &kestrel,
        json!({ "profile": OPERATOR, "sign_in_method": "claude-setup-token" }),
    )
    .await;
    let handed = handed(&kestrel, &session, &on).await;
    assert_eq!(
        names(&handed, "variables"),
        ["CLAUDE_CODE_OAUTH_TOKEN", "GH_TOKEN", "OPENAI_API_KEY"]
            .map(str::to_owned)
            .into(),
        "the Organization's key would have let a refused subscription fall back"
    );
    assert_eq!(handed["variables"]["CLAUDE_CODE_OAUTH_TOKEN"], CLAUDE_TOKEN);

    refused_authentication(&kestrel, &session, &on, 1).await;

    let used = sign_in(&kestrel, "claude", "claude-setup-token").await;
    assert_eq!(used["authentication"]["state"], "authentication_failed");
    assert_eq!(used["authentication"]["source"], "session");
    assert_eq!(
        sign_in(&kestrel, "claude", "anthropic-api-key").await["authentication"]["state"],
        "credential_accepted",
        "a sign-in the Session was never handed was blamed"
    );

    let read = read(&kestrel, session.id).await;
    let diagnostic = &read["diagnostic"];
    assert_eq!(diagnostic["kind"], "authentication_failed", "{read:#}");
    let context = &diagnostic["context"];
    assert_eq!(context["attribution"], "established");
    assert_eq!(context["sign_in"], "claude-setup-token");
    assert_eq!(context["revision"], token);
    assert_eq!(context["current"], true);
    assert_eq!(context["holding"]["resource"], "subscription_profile");
    assert_eq!(context["holding"]["reference"], OPERATOR);
    assert_eq!(context["expired"], Value::Null);
    assert_eq!(context["covered"], Value::Null);
    assert_eq!(diagnostic["next_steps"][0]["action"], "sign_in");
    assert_eq!(diagnostic["next_steps"][0]["method"], "claude-setup-token");
    assert_eq!(diagnostic["next_steps"][0]["sign_in"], OPERATOR);
    assert!(
        !read.to_string().contains(CLAUDE_TOKEN),
        "a Session read carried the material it was handed"
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn material_replaced_before_the_handoff_is_the_revision_the_session_used() {
    let stub = ProviderStub::start();
    let kestrel = booted(&stub, "claude").await;
    let enqueued_with = save(&kestrel, "claude", "claude-setup-token", CLAUDE_TOKEN).await;
    let opened = open(
        &kestrel,
        json!({ "profile": OPERATOR, "sign_in_method": "claude-setup-token" }),
    )
    .await;
    assert_eq!(opened.status, StatusCode::CREATED, "{:#}", opened.body);
    let replacement = save(&kestrel, "claude", "claude-setup-token", NEWER_CLAUDE_TOKEN).await;
    assert_ne!(replacement, enqueued_with);

    let (session, on) = on_the_link(&kestrel).await;
    let handed = handed(&kestrel, &session, &on).await;
    assert_eq!(
        handed["variables"]["CLAUDE_CODE_OAUTH_TOKEN"],
        NEWER_CLAUDE_TOKEN
    );
    let read_back = read(&kestrel, session.id).await;
    let token = supplied(&read_back, "CLAUDE_CODE_OAUTH_TOKEN");
    assert_eq!(token["revision"], replacement);
    assert_eq!(token["method"], "claude-setup-token");
    assert_eq!(token["resource"], "subscription_profile");
    assert_eq!(token["reference"], OPERATOR);

    refused_authentication(&kestrel, &session, &on, 1).await;
    let used = sign_in(&kestrel, "claude", "claude-setup-token").await;
    assert_eq!(used["revision"], replacement);
    assert_eq!(used["authentication"]["state"], "authentication_failed");

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_failure_that_arrives_after_its_material_was_replaced_marks_nothing() {
    let stub = ProviderStub::start();
    let kestrel = booted(&stub, "claude").await;
    let used = save(&kestrel, "claude", "claude-setup-token", CLAUDE_TOKEN).await;
    let (session, on) = carried(
        &kestrel,
        json!({ "profile": OPERATOR, "sign_in_method": "claude-setup-token" }),
    )
    .await;
    handed(&kestrel, &session, &on).await;
    let replacement = save(&kestrel, "claude", "claude-setup-token", NEWER_CLAUDE_TOKEN).await;

    refused_authentication(&kestrel, &session, &on, 1).await;

    let current = sign_in(&kestrel, "claude", "claude-setup-token").await;
    assert_eq!(current["revision"], replacement);
    assert_eq!(
        current["authentication"]["state"], "unchecked",
        "a failure of the revision it replaced was read as this one's"
    );
    let read = read(&kestrel, session.id).await;
    let context = &read["diagnostic"]["context"];
    assert_eq!(context["attribution"], "established", "{read:#}");
    assert_eq!(context["sign_in"], "claude-setup-token");
    assert_eq!(context["revision"], used);
    assert_eq!(context["current"], false);

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_failure_several_logins_could_explain_is_blamed_on_none_of_them() {
    let stub = ProviderStub::start();
    let kestrel = booted(&stub, "opencode").await;
    save(&kestrel, "opencode", "opencode-go-zen", "an-opencode-key").await;
    save(&kestrel, "opencode", "anthropic-api-key", ANTHROPIC_KEY).await;

    let (session, on) = carried(&kestrel, json!({ "profile": OPERATOR })).await;
    assert_eq!(
        names(&handed(&kestrel, &session, &on).await, "variables"),
        ["ANTHROPIC_API_KEY", "OPENCODE_API_KEY"]
            .map(str::to_owned)
            .into()
    );
    refused_authentication(&kestrel, &session, &on, 1).await;

    assert_eq!(
        sign_in(&kestrel, "opencode", "opencode-go-zen").await["authentication"]["state"],
        "unchecked"
    );
    assert_eq!(
        sign_in(&kestrel, "opencode", "anthropic-api-key").await["authentication"]["state"],
        "credential_accepted"
    );
    let read = read(&kestrel, session.id).await;
    let diagnostic = &read["diagnostic"];
    let context = &diagnostic["context"];
    assert_eq!(context["attribution"], "uncertain", "{read:#}");
    assert_eq!(context["sign_in"], Value::Null);
    assert_eq!(
        context["candidates"],
        json!(["anthropic-api-key", "opencode-go-zen"])
    );
    assert_eq!(diagnostic["next_steps"][0]["action"], "inspect_resource");

    let forgotten = sent(reqwest::Client::new().delete(format!(
        "{}{ORGANIZATION}/credentials/ANTHROPIC_API_KEY",
        kestrel.operator()
    )))
    .await;
    assert!(forgotten.status.is_success(), "{:#}", forgotten.body);
    let (alone, on) = carried(&kestrel, json!({ "profile": OPERATOR })).await;
    handed(&kestrel, &alone, &on).await;
    refused_authentication(&kestrel, &alone, &on, 1).await;
    assert_eq!(
        sign_in(&kestrel, "opencode", "opencode-go-zen").await["authentication"]["state"],
        "authentication_failed",
        "the one login the harness was handed was not named"
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_refresh_advances_only_the_revision_its_session_was_handed() {
    let stub = ProviderStub::start();
    let kestrel = booted(&stub, "codex").await;
    let imported = save(
        &kestrel,
        "codex",
        "codex-device-auth",
        &chatgpt_login("first"),
    )
    .await;
    let (session, on) = carried(
        &kestrel,
        json!({ "profile": OPERATOR, "sign_in_method": "codex-device-auth" }),
    )
    .await;
    assert_eq!(
        names(&handed(&kestrel, &session, &on).await, "files"),
        [CODEX_LOGIN.to_owned()].into()
    );
    let link = Link::to(&kestrel.link());
    let refresh = async |contents: String| {
        let answered = link
            .refresh(
                &on.instance,
                session.id,
                &on.credential,
                &[(CODEX_LOGIN, &contents)],
            )
            .await;
        assert_eq!(answered.status(), StatusCode::NO_CONTENT);
        sign_in(&kestrel, "codex", "codex-device-auth").await
    };

    let once = refresh(chatgpt_login("second")).await;
    assert_eq!(once["authentication"]["source"], "refresh");
    assert_eq!(once["refreshed_from"], imported);
    let twice = refresh(chatgpt_login("third")).await;
    assert_eq!(
        twice["refreshed_from"], once["revision"],
        "a Session's second refresh was read as stale"
    );

    let newer = save(
        &kestrel,
        "codex",
        "codex-device-auth",
        &chatgpt_login("imported"),
    )
    .await;
    let stale = refresh(chatgpt_login("fourth")).await;
    assert_eq!(stale["revision"], newer);
    assert_eq!(stale["authentication"]["source"], "import");
    assert_eq!(stale["refreshed_from"], Value::Null);
    let (profile, _) = kestrel.profiles("acme").await.remove(0);
    assert_eq!(
        kestrel.profile_contents(&profile).await.files[CODEX_LOGIN],
        chatgpt_login("imported"),
        "a stale refresh overwrote the login imported after its handoff"
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_login_a_session_holds_is_refused_to_a_test_and_a_test_holds_sessions_back() {
    let kestrel = Kestrel::boot().await;
    kestrel.record_serialized_harnesses().await;
    Fixture::acme()
        .checked_out()
        .harness(SERIALIZED)
        .declare(&kestrel)
        .await;
    let login = Entry::file(CODEX_LOGIN).expect("a file");
    for (name, owner) in [("jack", "Jack"), ("alex", "Alex")] {
        kestrel
            .declare_profile("acme", name, owner)
            .await
            .expect("the profile should declare");
        kestrel
            .hold_in_profile("acme", name, &login, "a-login")
            .await;
    }
    let jacks = kestrel
        .open_workspace_with("acme", repository::NAME, "builder", "jack")
        .await;
    kestrel.enqueue_session(jacks.id).await;
    let working = kestrel.claim_session().await.expect("the session claims");

    let busy = kestrel
        .acquire_credential_use("jack", "a-test")
        .await
        .expect_err("a login a Session is using was lent to a test");
    let conflict = busy
        .downcast_ref::<kestrel::declined::Reason>()
        .expect("a typed refusal");
    assert!(
        matches!(
            conflict,
            kestrel::declined::Reason::StateConflict {
                state: "credential_in_use",
                holding_session: Some(holding),
                ..
            } if *holding == working.id.to_string()
        ),
        "{conflict:?}"
    );
    let alexs = kestrel
        .acquire_credential_use("alex", "a-test")
        .await
        .expect("another person's Profile is independent");

    kestrel.complete_session(&working).await;
    let held = kestrel
        .acquire_credential_use("jack", "a-test")
        .await
        .expect("the login is free once its Session ended");
    assert!(
        kestrel
            .acquire_credential_use("jack", "another-test")
            .await
            .is_err(),
        "two tests held one login"
    );

    let again = kestrel
        .open_workspace_with("acme", repository::NAME, "builder", "jack")
        .await;
    kestrel.enqueue_session(again.id).await;
    assert!(
        kestrel.claim_session().await.is_none(),
        "a Session was dispatched onto a login a test still holds"
    );
    kestrel.release_credential_use(&held).await;
    assert_eq!(
        kestrel.claim_session().await.map(|next| next.workspace),
        Some(again.id)
    );
    kestrel.release_credential_use(&alexs).await;

    kestrel.teardown().await;
}

#[tokio::test]
async fn what_a_session_learned_of_one_model_says_nothing_of_another() {
    let stub = ProviderStub::start();
    let kestrel = booted(&stub, "claude").await;
    save(&kestrel, "claude", "claude-setup-token", CLAUDE_TOKEN).await;
    let opened = open(
        &kestrel,
        json!({ "profile": OPERATOR, "sign_in_method": "claude-setup-token" }),
    )
    .await;
    assert_eq!(opened.status, StatusCode::CREATED, "{:#}", opened.body);
    let (worked, on) = on_the_link(&kestrel).await;
    handed(&kestrel, &worked, &on).await;
    report(
        &kestrel,
        &on,
        json!({"kind": "model", "session": worked.id, "seq": 1, "model": "claude-opus"}),
    )
    .await;
    report(
        &kestrel,
        &on,
        json!({"kind": "answered", "session": worked.id, "seq": 2}),
    )
    .await;

    let uses = |saved: &Value| -> Vec<(String, String, String)> {
        saved["model_use"]
            .as_array()
            .expect("model use")
            .iter()
            .map(|used| {
                let text = |field: &str| used[field].as_str().expect(field).to_owned();
                (text("model"), text("result"), text("source"))
            })
            .collect()
    };
    let opus = (
        "claude-opus".to_owned(),
        "worked".to_owned(),
        "session".to_owned(),
    );
    assert_eq!(
        uses(&sign_in(&kestrel, "claude", "claude-setup-token").await),
        std::slice::from_ref(&opus)
    );

    kestrel.stop_session(worked.id).await;
    let next = post(
        &kestrel,
        &format!(
            "{ORGANIZATION}/workspaces/{}/sessions",
            opened.body["workspace"]["id"].as_str().expect("an id")
        ),
        json!({ "model": "claude-sonnet" }),
    )
    .await;
    assert_eq!(next.status, StatusCode::CREATED, "{:#}", next.body);
    let failed = kestrel.claim_session().await.expect("a session to claim");
    let on = kestrel.on_the_link(&failed).await;
    handed(&kestrel, &failed, &on).await;
    refused_authentication(&kestrel, &failed, &on, 1).await;

    assert_eq!(
        uses(&sign_in(&kestrel, "claude", "claude-setup-token").await),
        [
            opus,
            (
                "claude-sonnet".to_owned(),
                "authentication_failed".to_owned(),
                "session".to_owned()
            ),
        ],
        "one model's failure rewrote what another was known to do"
    );

    kestrel.teardown().await;
}
