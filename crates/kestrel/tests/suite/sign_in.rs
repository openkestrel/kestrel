use reqwest::StatusCode;
use serde_json::{Value, json};

use crate::support::Kestrel;
use crate::support::provider_stub::{self, Answer, ProviderStub};

const OPERATOR: &str = "ada";
const SIGN_INS: &str = "/operator/organizations/ada/sign-ins";
const CLAUDE_TOKEN: &str = "sk-ant-oat01-subscription-secret";
const ANTHROPIC_KEY: &str = "sk-ant-api03-first-secret";
const CHATGPT_LOGIN: &str = r#"{"OPENAI_API_KEY":null,"tokens":{"id_token":"id-secret","access_token":"access-secret","refresh_token":"refresh-secret","account_id":"acct"}}"#;

struct Answered {
    status: StatusCode,
    body: Value,
    text: String,
}

async fn answered(response: reqwest::Response) -> Answered {
    let status = response.status();
    let text = response.text().await.expect("a body");
    Answered {
        status,
        body: serde_json::from_str(&text).unwrap_or(Value::Null),
        text,
    }
}

async fn put(kestrel: &Kestrel, path: &str, body: Value) -> Answered {
    answered(
        reqwest::Client::new()
            .put(format!("{}{path}", kestrel.operator()))
            .json(&body)
            .send()
            .await
            .expect("the operator boundary answers"),
    )
    .await
}

async fn save(kestrel: &Kestrel, harness: &str, method: &str, value: &str) -> Answered {
    put(
        kestrel,
        &format!("{SIGN_INS}/{harness}/{method}"),
        json!({ "value": value }),
    )
    .await
}

async fn saved(kestrel: &Kestrel) -> Vec<Value> {
    let listed = answered(
        reqwest::get(format!("{}{SIGN_INS}", kestrel.operator()))
            .await
            .expect("the operator boundary answers"),
    )
    .await;
    assert_eq!(listed.status, StatusCode::OK, "{}", listed.text);
    for secret in [CLAUDE_TOKEN, ANTHROPIC_KEY, "id-secret", "refresh-secret"] {
        assert!(!listed.text.contains(secret), "{}", listed.text);
    }
    listed.body.as_array().expect("saved sign-ins").clone()
}

fn one<'a>(saved: &'a [Value], harness: &str, method: &str) -> &'a Value {
    let found: Vec<&Value> = saved
        .iter()
        .filter(|item| item["harness"] == harness && item["method"] == method)
        .collect();
    assert_eq!(found.len(), 1, "{saved:#?}");
    found[0]
}

async fn named(kestrel: &Kestrel) {
    let named = put(kestrel, "/operator/operator", json!({ "name": OPERATOR })).await;
    assert_eq!(named.status, StatusCode::CREATED, "{}", named.text);
}

async fn booted(stub: &ProviderStub) -> Kestrel {
    let kestrel = Kestrel::boot_checking_sign_ins_at(&stub.base_url()).await;
    named(&kestrel).await;
    kestrel
}

async fn generic_credential(kestrel: &Kestrel, secret: &str) {
    let held = put(
        kestrel,
        "/operator/organizations/ada/credentials/ANTHROPIC_API_KEY",
        json!({ "secret": secret }),
    )
    .await;
    assert_eq!(held.status, StatusCode::OK, "{}", held.text);
}

#[tokio::test]
async fn a_subscription_token_is_held_in_the_operators_own_profile_without_asking_anyone() {
    let stub = ProviderStub::start();
    let kestrel = booted(&stub).await;

    let token = save(
        &kestrel,
        "claude",
        "claude-setup-token",
        &format!("{CLAUDE_TOKEN}\n"),
    )
    .await;

    assert_eq!(token.status, StatusCode::OK, "{}", token.text);
    assert!(!token.text.contains(CLAUDE_TOKEN), "{}", token.text);
    assert_eq!(token.body["kind"], "subscription");
    assert_eq!(
        token.body["holding"],
        json!({
            "resource": "subscription_profile",
            "reference": OPERATOR,
            "fills": { "kind": "variable", "variable": "CLAUDE_CODE_OAUTH_TOKEN" },
        })
    );
    assert_eq!(token.body["authentication"]["state"], "unchecked");
    assert_eq!(token.body["authentication"]["source"], "import");
    assert_eq!(token.body["authentication"]["provider_check"], Value::Null);
    assert!(
        stub.asked().is_empty(),
        "a subscription has no check to ask"
    );
    assert_eq!(saved(&kestrel).await, vec![token.body]);

    let credentials = reqwest::get(format!(
        "{}/operator/organizations/ada/credentials",
        kestrel.operator()
    ))
    .await
    .expect("the operator boundary answers");
    assert_eq!(
        credentials.json::<Value>().await.expect("credentials"),
        json!([]),
        "a Claude plan is never the Organization's to share"
    );
    let profiles = kestrel.profiles(OPERATOR).await;
    assert_eq!(profiles.len(), 1);
    assert!(
        profiles[0].0.owner_operator.is_some(),
        "declared as the Operator's"
    );
    assert_eq!(
        kestrel
            .profile_contents(&profiles[0].0)
            .await
            .variables
            .get("CLAUDE_CODE_OAUTH_TOKEN"),
        Some(&CLAUDE_TOKEN.to_owned()),
        "held without the line break a terminal adds"
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_key_is_the_organizations_and_one_metadata_read_records_its_acceptance() {
    let stub = ProviderStub::start();
    let kestrel = booted(&stub).await;

    let key = save(&kestrel, "claude", "anthropic-api-key", ANTHROPIC_KEY).await;

    assert_eq!(key.status, StatusCode::OK, "{}", key.text);
    assert!(!key.text.contains(ANTHROPIC_KEY), "{}", key.text);
    assert_eq!(key.body["kind"], "key");
    assert_eq!(key.body["holding"]["resource"], "provider_credential");
    assert_eq!(key.body["holding"]["reference"], "ANTHROPIC_API_KEY");
    assert_eq!(key.body["authentication"]["state"], "credential_accepted");
    assert_eq!(
        key.body["authentication"]["provider_check"],
        json!({ "provider": "anthropic", "outcome": "accepted", "status": 200, "provider_error": null })
    );
    let asked = stub.asked();
    assert_eq!(asked.len(), 1, "one request, and no inference");
    assert_eq!(asked[0].method, "GET");
    assert_eq!(asked[0].url, "/v1/models?limit=1");
    assert_eq!(asked[0].header("x-api-key"), Some(ANTHROPIC_KEY));
    assert_eq!(asked[0].header("anthropic-version"), Some("2023-06-01"));

    let listed = saved(&kestrel).await;
    assert_eq!(listed.len(), 2, "{listed:#?}");
    let claude = one(&listed, "claude", "anthropic-api-key");
    let opencode = one(&listed, "opencode", "anthropic-api-key");
    assert_eq!(claude, &key.body);
    assert_eq!(opencode["revision"], claude["revision"]);
    assert_eq!(opencode["authentication"], claude["authentication"]);
    assert!(kestrel.profiles(OPERATOR).await.is_empty());

    let openai = save(&kestrel, "codex", "openai-api-key", "sk-proj-openai-secret").await;
    assert_eq!(openai.status, StatusCode::OK, "{}", openai.text);
    assert_eq!(
        openai.body["authentication"]["state"],
        "credential_accepted"
    );
    let asked = stub.asked();
    assert_eq!(asked[1].url, "/v1/models");
    assert_eq!(
        asked[1].header("authorization"),
        Some("Bearer sk-proj-openai-secret")
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_key_the_provider_rejects_keeps_what_was_saved_before() {
    let stub = ProviderStub::start();
    let kestrel = booted(&stub).await;
    let first = save(&kestrel, "claude", "anthropic-api-key", ANTHROPIC_KEY).await;
    assert_eq!(first.status, StatusCode::OK, "{}", first.text);

    stub.answer(provider_stub::rejected());
    let rejected = save(
        &kestrel,
        "claude",
        "anthropic-api-key",
        "sk-ant-api03-mistyped",
    )
    .await;

    assert_eq!(
        rejected.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        rejected.text
    );
    assert!(!rejected.text.contains("mistyped"), "{}", rejected.text);
    assert!(
        !rejected.text.contains("invalid x-api-key"),
        "{}",
        rejected.text
    );
    assert_eq!(rejected.body["kind"], "credential_rejected");
    assert_eq!(rejected.body["field"], "value");
    assert_eq!(
        rejected.body["context"],
        json!({
            "organization": OPERATOR,
            "harness": "claude",
            "method": "anthropic-api-key",
            "provider": "anthropic",
            "status": 401,
            "provider_error": "authentication_error",
        })
    );
    assert_eq!(rejected.body["next_steps"][0]["action"], "sign_in");
    assert_eq!(
        one(&saved(&kestrel).await, "claude", "anthropic-api-key"),
        &first.body
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_refusal_about_permission_saves_the_key_unchecked() {
    let stub = ProviderStub::start();
    let kestrel = booted(&stub).await;

    stub.answer(provider_stub::forbidden());
    let key = save(&kestrel, "claude", "anthropic-api-key", ANTHROPIC_KEY).await;

    assert_eq!(key.status, StatusCode::OK, "{}", key.text);
    assert_eq!(key.body["authentication"]["state"], "unchecked");
    assert_eq!(key.body["authentication"]["source"], "provider_check");
    assert_eq!(
        key.body["authentication"]["provider_check"],
        json!({
            "provider": "anthropic",
            "outcome": "inconclusive",
            "status": 403,
            "provider_error": "permission_error",
        })
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_provider_that_cannot_answer_leaves_the_key_saved_unchecked_and_says_so() {
    let stub = ProviderStub::start();
    let kestrel = booted(&stub).await;

    for (answer, status) in [
        (provider_stub::overloaded(), json!(529)),
        (Answer::Silent, Value::Null),
    ] {
        stub.answer(answer);
        let partial = save(&kestrel, "claude", "anthropic-api-key", ANTHROPIC_KEY).await;

        assert_eq!(
            partial.status,
            StatusCode::SERVICE_UNAVAILABLE,
            "{}",
            partial.text
        );
        assert!(!partial.text.contains(ANTHROPIC_KEY), "{}", partial.text);
        assert_eq!(partial.body["kind"], "unavailable");
        assert_eq!(partial.body["context"]["service"], "anthropic");
        let partially = &partial.body["context"]["saved"];
        assert_eq!(partially["authentication"]["state"], "unchecked");
        assert_eq!(
            partially["authentication"]["provider_check"]["outcome"],
            "unavailable"
        );
        assert_eq!(
            partially["authentication"]["provider_check"]["status"],
            status
        );
        assert_eq!(
            one(&saved(&kestrel).await, "claude", "anthropic-api-key"),
            partially,
            "the partial save is what both Clients read"
        );
    }

    kestrel.teardown().await;
}

#[tokio::test]
async fn empty_or_incompatible_input_changes_nothing() {
    let stub = ProviderStub::start();
    let kestrel = booted(&stub).await;
    let token = save(&kestrel, "claude", "claude-setup-token", CLAUDE_TOKEN).await;
    assert_eq!(token.status, StatusCode::OK, "{}", token.text);

    for (harness, method, value, constraint) in [
        ("claude", "claude-setup-token", " \n", "non_empty"),
        ("claude", "anthropic-api-key", "", "non_empty"),
        (
            "claude",
            "claude-setup-token",
            "two sk-ant-halves",
            "single_token",
        ),
        ("codex", "codex-device-auth", "", "non_empty"),
        (
            "codex",
            "codex-device-auth",
            r#"{"OPENAI_API_KEY":"sk-proj-leaked"}"#,
            "chatgpt_login_file",
        ),
        (
            "codex",
            "codex-device-auth",
            r#"{"tokens": "sk-proj-leaked""#,
            "chatgpt_login_file",
        ),
    ] {
        let refused = save(&kestrel, harness, method, value).await;
        assert_eq!(
            refused.status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{}",
            refused.text
        );
        assert_eq!(refused.body["kind"], "invalid_field", "{}", refused.text);
        assert_eq!(refused.body["context"]["constraint"], constraint);
        assert!(!refused.text.contains("leaked"), "{}", refused.text);
        assert!(!refused.text.contains("halves"), "{}", refused.text);
    }
    assert!(stub.asked().is_empty());
    assert_eq!(saved(&kestrel).await, vec![token.body]);

    let login = save(&kestrel, "codex", "codex-device-auth", CHATGPT_LOGIN).await;
    assert_eq!(login.status, StatusCode::OK, "{}", login.text);
    assert_eq!(login.body["holding"]["reference"], OPERATOR);
    assert_eq!(
        login.body["holding"]["fills"],
        json!({ "kind": "file", "path": ".codex/auth.json" })
    );
    assert_eq!(login.body["authentication"]["state"], "unchecked");
    assert!(
        stub.asked().is_empty(),
        "a ChatGPT login has no check to ask"
    );

    let unknown = save(&kestrel, "claude", "codex-device-auth", CHATGPT_LOGIN).await;
    assert_eq!(unknown.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(unknown.body["context"]["constraint"], "offered");

    kestrel.teardown().await;
}

#[tokio::test]
async fn several_operator_profiles_are_refused_rather_than_one_chosen() {
    let stub = ProviderStub::start();
    let kestrel = booted(&stub).await;
    for name in ["work", "play"] {
        let declared = answered(
            reqwest::Client::new()
                .post(format!(
                    "{}/operator/organizations/ada/profiles",
                    kestrel.operator()
                ))
                .json(&json!({ "name": name }))
                .send()
                .await
                .expect("the operator boundary answers"),
        )
        .await;
        assert_eq!(declared.status, StatusCode::CREATED, "{}", declared.text);
    }

    let ambiguous = save(&kestrel, "claude", "claude-setup-token", CLAUDE_TOKEN).await;

    assert_eq!(ambiguous.status, StatusCode::CONFLICT, "{}", ambiguous.text);
    assert_eq!(ambiguous.body["kind"], "state_conflict");
    assert_eq!(
        ambiguous.body["context"]["state"],
        "several_operator_profiles"
    );
    assert!(saved(&kestrel).await.is_empty());

    let held = put(
        &kestrel,
        "/operator/organizations/ada/profiles/play/variables/CLAUDE_CODE_OAUTH_TOKEN",
        json!({ "secret": "sk-ant-oat01-earlier" }),
    )
    .await;
    assert_eq!(held.status, StatusCode::OK, "{}", held.text);
    let replaced = save(&kestrel, "claude", "claude-setup-token", CLAUDE_TOKEN).await;
    assert_eq!(replaced.status, StatusCode::OK, "{}", replaced.text);
    assert_eq!(
        replaced.body["holding"]["reference"], "play",
        "the one already holding a Claude subscription"
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_profile_someone_else_owns_is_never_chosen() {
    let stub = ProviderStub::start();
    let kestrel = booted(&stub).await;
    kestrel
        .declare_profile(OPERATOR, OPERATOR, OPERATOR)
        .await
        .expect("a free-text owner, even one matching the Operator's name");

    let token = save(&kestrel, "claude", "claude-setup-token", CLAUDE_TOKEN).await;

    assert_eq!(token.status, StatusCode::CONFLICT, "{}", token.text);
    assert_eq!(token.body["context"]["state"], "owned_by_another");
    let profiles = kestrel.profiles(OPERATOR).await;
    assert_eq!(profiles.len(), 1);
    assert!(profiles[0].1.is_empty(), "nothing was held in theirs");

    kestrel.teardown().await;
}

#[tokio::test]
async fn every_accepted_write_mints_a_revision_and_a_generic_one_is_never_checked() {
    let stub = ProviderStub::start();
    let kestrel = booted(&stub).await;
    let revision = |listed: &[Value]| {
        one(listed, "claude", "anthropic-api-key")["revision"]
            .as_i64()
            .expect("a revision")
    };

    let checked = save(&kestrel, "claude", "anthropic-api-key", ANTHROPIC_KEY).await;
    let first = checked.body["revision"].as_i64().expect("a revision");

    generic_credential(&kestrel, "sk-ant-api03-pasted-by-hand").await;
    let listed = saved(&kestrel).await;
    let second = revision(&listed);
    assert!(second > first, "{second} after {first}");
    let generic = &one(&listed, "claude", "anthropic-api-key")["authentication"];
    assert_eq!(generic["state"], "unchecked");
    assert_eq!(generic["source"], "generic_write");
    assert_eq!(stub.asked().len(), 1, "a generic write asks no provider");

    let forgotten = reqwest::Client::new()
        .delete(format!(
            "{}/operator/organizations/ada/credentials/ANTHROPIC_API_KEY",
            kestrel.operator()
        ))
        .send()
        .await
        .expect("the operator boundary answers");
    assert_eq!(forgotten.status(), StatusCode::NO_CONTENT);
    assert!(saved(&kestrel).await.is_empty());

    generic_credential(&kestrel, "sk-ant-api03-pasted-again").await;
    let third = revision(&saved(&kestrel).await);
    assert!(third > second, "a write after a delete is new material");

    let resaved = save(&kestrel, "claude", "anthropic-api-key", ANTHROPIC_KEY).await;
    assert!(resaved.body["revision"].as_i64().expect("a revision") > third);

    kestrel.teardown().await;
}

#[tokio::test]
async fn material_replaced_while_it_was_being_checked_stands() {
    let stub = ProviderStub::start();
    let kestrel = booted(&stub).await;

    stub.answer(Answer::Held(200, r#"{"data":[]}"#.to_owned()));
    let checking = {
        let operator = kestrel.operator();
        tokio::spawn(async move {
            answered(
                reqwest::Client::new()
                    .put(format!("{operator}{SIGN_INS}/claude/anthropic-api-key"))
                    .json(&json!({ "value": ANTHROPIC_KEY }))
                    .send()
                    .await
                    .expect("the operator boundary answers"),
            )
            .await
        })
    };
    stub.until_asked(1).await;
    generic_credential(&kestrel, "sk-ant-api03-replacement").await;
    let replacement = saved(&kestrel).await;
    stub.release();

    let late = checking.await.expect("the save finished");
    assert_eq!(late.status, StatusCode::CONFLICT, "{}", late.text);
    assert_eq!(late.body["kind"], "state_conflict");
    assert_eq!(late.body["context"]["state"], "replaced");
    assert_eq!(late.body["context"]["resource"], "provider_credential");
    assert_eq!(
        saved(&kestrel).await,
        replacement,
        "the late check verified nothing"
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn model_use_is_evidence_for_one_harness_model_and_revision_only() {
    let stub = ProviderStub::start();
    let kestrel = booted(&stub).await;
    let key = save(&kestrel, "claude", "anthropic-api-key", ANTHROPIC_KEY).await;
    let revision = key.body["revision"].as_i64().expect("a revision");
    let database = kestrel.database();
    database
        .record_model_use(revision, "claude", "claude-opus", "worked")
        .await;
    database
        .record_model_use(revision, "opencode", "anthropic/claude-opus", "not_covered")
        .await;
    database
        .record_model_use(revision, "opencode", "anthropic/claude-haiku", "worked")
        .await;

    let listed = saved(&kestrel).await;
    let claude = one(&listed, "claude", "anthropic-api-key");
    let opencode = one(&listed, "opencode", "anthropic-api-key");
    let results = |item: &Value| -> Vec<(String, String)> {
        item["model_use"]
            .as_array()
            .expect("model use")
            .iter()
            .map(|used| {
                (
                    used["model"].as_str().expect("a model").to_owned(),
                    used["result"].as_str().expect("a result").to_owned(),
                )
            })
            .collect()
    };
    assert_eq!(
        results(claude),
        [("claude-opus".to_owned(), "worked".to_owned())]
    );
    assert_eq!(
        results(opencode),
        [
            ("anthropic/claude-haiku".to_owned(), "worked".to_owned()),
            ("anthropic/claude-opus".to_owned(), "not_covered".to_owned()),
        ]
    );
    for item in [claude, opencode] {
        assert_eq!(item["authentication"]["state"], "credential_accepted");
    }

    generic_credential(&kestrel, "sk-ant-api03-replacement").await;
    database
        .record_model_use(revision, "claude", "claude-sonnet", "worked")
        .await;
    let replaced = saved(&kestrel).await;
    assert!(
        results(one(&replaced, "claude", "anthropic-api-key")).is_empty(),
        "evidence about the earlier revision is not about this one"
    );

    kestrel.teardown().await;
}
