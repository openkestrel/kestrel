//! Fixtures the operator-boundary suites share: route paths, Client runs, and requests
//! checked against the published operator document.

use std::time::Duration;

use super::Kestrel;
use super::client;
use super::fixture::Fixture;
use kestrel::domain::WorkspaceId;
use kestrel::operator;
use reqwest::StatusCode;
use serde_json::{Value, json};

pub async fn an_open_workspace(
    kestrel: &Kestrel,
    said: usize,
) -> (String, kestrel::domain::Session) {
    Fixture::acme().declare(kestrel).await;
    let workspace = kestrel.open_workspace("acme", "kestrel", "builder").await;
    let session = kestrel.dispatch_session(workspace.id).await;
    for message in 1..=said {
        kestrel.said(&session, &format!("message {message}")).await;
    }

    (workspace.id.to_string(), session)
}

pub fn records(lines: &[String]) -> Vec<Value> {
    lines
        .iter()
        .flat_map(|line| {
            match serde_json::from_str(line)
                .unwrap_or_else(|error| panic!("{line} is not a record: {error}"))
            {
                Value::Array(records) => records,
                record => vec![record],
            }
        })
        .collect()
}

pub async fn client(kestrel: &Kestrel, args: &[&str]) -> client::Finished {
    client_given(kestrel, args, None).await
}

pub async fn client_given(
    kestrel: &Kestrel,
    args: &[&str],
    input: Option<&str>,
) -> client::Finished {
    let invocation = input.map_or_else(client::Invocation::default, |input| {
        client::Invocation::default().given(input)
    });
    client::ran_by(kestrel, args, invocation).await
}

pub fn succeeded(finished: &client::Finished) -> &[String] {
    assert!(
        finished.status.success(),
        "the client failed:\n{}",
        finished.err
    );
    assert!(
        finished.left_behind.is_empty(),
        "the client wrote {:?} where it ran",
        finished.left_behind
    );
    &finished.out
}

pub fn recorded(finished: &client::Finished) -> Vec<Value> {
    records(succeeded(finished))
}

/// Every answer is checked against what the published document says the operation answers.
pub async fn requested(
    kestrel: &Kestrel,
    method: reqwest::Method,
    path: &str,
    body: Option<&Value>,
) -> (StatusCode, Value) {
    let mut request =
        reqwest::Client::new().request(method.clone(), format!("{}{path}", kestrel.operator()));
    if let Some(body) = body {
        request = request.json(body);
    }
    let response = request
        .send()
        .await
        .expect("the operator boundary should answer");
    let status = response.status();
    let text = response.text().await.expect("an answer");
    let body = if text.is_empty() {
        Value::Null
    } else {
        serde_json::from_str(&text).unwrap_or_else(|error| panic!("{text} is not JSON: {error}"))
    };

    let (path, _) = path.split_once('?').unwrap_or((path, ""));
    conforms(path, &method.as_str().to_lowercase(), status, &body);
    (status, body)
}

pub async fn declared(kestrel: &Kestrel, path: &str, declaration: &Value) -> (StatusCode, Value) {
    requested(kestrel, reqwest::Method::POST, path, Some(declaration)).await
}

pub async fn got(kestrel: &Kestrel, path: &str) -> (StatusCode, Value) {
    requested(kestrel, reqwest::Method::GET, path, None).await
}

pub async fn listed(kestrel: &Kestrel, path: &str) -> Vec<Value> {
    let (status, body) = got(kestrel, path).await;

    assert_eq!(status, StatusCode::OK, "{body}");
    body.as_array().expect("an array of records").clone()
}

pub async fn listed_nothing(kestrel: &Kestrel, path: &str) -> bool {
    listed(kestrel, path).await.is_empty()
}

pub fn projects_of(organization: &str) -> String {
    operator::PROJECTS.replace("{organization}", organization)
}

pub fn agents_of(organization: &str) -> String {
    operator::AGENTS.replace("{organization}", organization)
}

pub fn agent_model_of(organization: &str, agent: &str) -> String {
    operator::AGENT_MODEL
        .replace("{organization}", organization)
        .replace("{agent}", agent)
}

pub fn declaration_of(organization: &str) -> String {
    operator::DECLARATION.replace("{organization}", organization)
}

pub fn declaration_preview_of(organization: &str) -> String {
    operator::DECLARATION_PREVIEW.replace("{organization}", organization)
}

pub fn credentials_of(organization: &str) -> String {
    operator::CREDENTIALS.replace("{organization}", organization)
}

pub fn credential_of(organization: &str, variable: &str) -> String {
    operator::CREDENTIAL
        .replace("{organization}", organization)
        .replace("{variable}", variable)
}

pub fn profiles_of(organization: &str) -> String {
    operator::PROFILES.replace("{organization}", organization)
}

pub fn profile_variable_of(organization: &str, profile: &str, variable: &str) -> String {
    operator::PROFILE_VARIABLE
        .replace("{organization}", organization)
        .replace("{profile}", profile)
        .replace("{variable}", variable)
}

pub fn integrations_of(organization: &str) -> String {
    operator::INTEGRATIONS.replace("{organization}", organization)
}

pub fn integration_of(organization: &str, integration: &str) -> String {
    operator::INTEGRATION
        .replace("{organization}", organization)
        .replace("{integration}", integration)
}

pub fn triggers_of(organization: &str) -> String {
    operator::TRIGGERS.replace("{organization}", organization)
}

pub fn workspace_at(organization: &str, workspace: &str) -> String {
    operator::WORKSPACE
        .replace("{organization}", organization)
        .replace("{workspace}", workspace)
}

pub fn workspace_messages_at(organization: &str, workspace: &str) -> String {
    operator::WORKSPACE_MESSAGES
        .replace("{organization}", organization)
        .replace("{workspace}", workspace)
}

pub fn workspace_seal_at(organization: &str, workspace: &str) -> String {
    operator::WORKSPACE_SEAL
        .replace("{organization}", organization)
        .replace("{workspace}", workspace)
}

pub fn sessions_of(organization: &str, workspace: &str) -> String {
    operator::SESSIONS
        .replace("{organization}", organization)
        .replace("{workspace}", workspace)
}

pub fn session_at(organization: &str, session: &str) -> String {
    operator::SESSION
        .replace("{organization}", organization)
        .replace("{session}", session)
}

pub fn transcript_of(organization: &str, workspace: &str) -> String {
    operator::TRANSCRIPT
        .replace("{organization}", organization)
        .replace("{workspace}", workspace)
}

pub fn failed(finished: &client::Finished) -> &str {
    assert!(
        !finished.status.success(),
        "the client was expected to be refused, and printed {:?}",
        finished.out
    );
    &finished.err
}

/// Past the 30 days a Transcript's narration and detail are kept.
pub const PAST_RETENTION: Duration = Duration::from_secs(31 * 24 * 60 * 60);

pub async fn ready_to_open(kestrel: &Kestrel) -> kestrel::domain::Organization {
    let organization = Fixture::acme()
        .agent("reviewer", "opencode", None)
        .declare(kestrel)
        .await;
    kestrel
        .declare_profile("acme", "jack", "Jack")
        .await
        .expect("the profile should declare");

    organization
}

pub fn workspace_of(record: &Value) -> WorkspaceId {
    record["id"]
        .as_str()
        .expect("a workspace id")
        .parse()
        .expect("a workspace identifier")
}

/// Carries a key opencode signs in with, so the plan's path lacks no sign-in.
pub fn a_start(organization: &str, harness: &str, brief: &str) -> Value {
    json!({
        "organization": organization,
        "project": "kestrel",
        "repositories": ["https://github.com/jtmthf/kestrel"],
        "branch": "main",
        "agent": "builder",
        "harness": harness,
        "brief": brief,
        "credentials": [{ "variable": "ANTHROPIC_API_KEY", "secret": "sk-ant-api03-key" }],
    })
}

/// The documented answer for this status must exist, and it must agree with the boundary on
/// whether there is a JSON body at all.
pub fn conforms(path: &str, method: &str, status: StatusCode, body: &Value) {
    let document = super::published_document("operator.json");
    let (_, operations) = document["paths"]
        .as_object()
        .expect("an object of paths")
        .iter()
        .find(|(template, _)| matches_template(template, path))
        .unwrap_or_else(|| panic!("the document describes no path matching {path}"));
    let answer = &operations[method]["responses"][status.as_str()];
    assert!(
        !answer.is_null(),
        "the document says {method} {path} never answers {status}"
    );
    let answer = match answer["$ref"].as_str() {
        Some(reference) => resolve(&document, reference),
        None => answer,
    };

    let schema = &answer["content"]["application/json"]["schema"];
    if schema.is_null() {
        assert!(
            body.is_null(),
            "the document says {method} {path} answers {status} with nothing, and it served {body}"
        );
    } else {
        assert!(
            !body.is_null(),
            "the document says {method} {path} answers {status} with a body, and it served nothing"
        );
    }
}

pub fn matches_template(template: &str, path: &str) -> bool {
    let (template, path): (Vec<_>, Vec<_>) =
        (template.split('/').collect(), path.split('/').collect());
    template.len() == path.len()
        && template
            .iter()
            .zip(&path)
            .all(|(step, given)| step.starts_with('{') || step == given)
}

pub fn resolve<'a>(document: &'a Value, reference: &str) -> &'a Value {
    reference
        .trim_start_matches("#/")
        .split('/')
        .fold(document, |document, step| &document[step])
}

pub async fn payload_entries(kestrel: &Kestrel, workspace: &str) -> Vec<Value> {
    let response = reqwest::Client::new()
        .get(format!(
            "{}{}?follow=false&kinds=shared_state,narration,detail",
            kestrel.operator(),
            transcript_of("acme", workspace)
        ))
        .send()
        .await
        .expect("the transcript should answer");
    assert_eq!(response.status(), StatusCode::OK);
    response
        .text()
        .await
        .expect("the transcript should finish")
        .lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|record| record.get("entry").is_some())
        .collect()
}

pub async fn retention_stream(kestrel: &Kestrel, workspace: &str, query: &str) -> String {
    reqwest::Client::new()
        .get(format!(
            "{}{}?follow=false&{query}",
            kestrel.operator(),
            transcript_of("acme", workspace)
        ))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .text()
        .await
        .unwrap()
}
