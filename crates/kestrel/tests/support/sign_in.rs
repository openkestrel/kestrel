use reqwest::StatusCode;
use serde_json::{Value, json};

use crate::support::Kestrel;
use crate::support::provider_stub::ProviderStub;

pub const OPERATOR: &str = "ada";
pub const SIGN_INS: &str = "/operator/organizations/ada/sign-ins";
pub const CLAUDE_TOKEN: &str = "sk-ant-oat01-subscription-secret";
pub const ANTHROPIC_KEY: &str = "sk-ant-api03-first-secret";
pub const CHATGPT_LOGIN: &str = r#"{"OPENAI_API_KEY":null,"tokens":{"id_token":"id-secret","access_token":"access-secret","refresh_token":"refresh-secret","account_id":"acct"}}"#;

pub struct Answered {
    pub status: StatusCode,
    pub body: Value,
    pub text: String,
}

pub async fn answered(response: reqwest::Response) -> Answered {
    let status = response.status();
    let text = response.text().await.expect("a body");
    Answered {
        status,
        body: serde_json::from_str(&text).unwrap_or(Value::Null),
        text,
    }
}

pub async fn put(kestrel: &Kestrel, path: &str, body: Value) -> Answered {
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

pub async fn save(kestrel: &Kestrel, harness: &str, method: &str, value: &str) -> Answered {
    put(
        kestrel,
        &format!("{SIGN_INS}/{harness}/{method}"),
        json!({ "value": value }),
    )
    .await
}

pub async fn saved(kestrel: &Kestrel) -> Vec<Value> {
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

pub fn one<'a>(saved: &'a [Value], harness: &str, method: &str) -> &'a Value {
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

pub async fn booted(stub: &ProviderStub) -> Kestrel {
    let kestrel = Kestrel::boot_checking_sign_ins_at(&stub.base_url()).await;
    named(&kestrel).await;
    kestrel
}

pub async fn generic_credential(kestrel: &Kestrel, secret: &str) {
    let held = put(
        kestrel,
        "/operator/organizations/ada/credentials/ANTHROPIC_API_KEY",
        json!({ "secret": secret }),
    )
    .await;
    assert_eq!(held.status, StatusCode::OK, "{}", held.text);
}
