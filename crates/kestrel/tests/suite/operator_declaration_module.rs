//! Every declaring route reaches the one Declaration module, which types the constraint a refusal
//! broke and names its field.

use crate::support;

use kestrel::operator;
use reqwest::StatusCode;
use serde_json::{Value, json};
use support::Kestrel;
use support::operator_requests::{
    a_start, agents_of, client, declaration_of, declaration_preview_of, declared, listed,
    listed_nothing, projects_of, triggers_of,
};

/// The one Declaration module answers every route: the refusal is typed with the constraint it
/// broke, says the one sentence, names the field under the route's prefix and stamps the
/// route's operation on the correction it offers.
async fn refused_through_the_module(kestrel: &Kestrel, path: &str, body: &Value) -> Value {
    let (status, refusal) = declared(kestrel, path, body).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{refusal}");
    assert_eq!(refusal["kind"], "invalid_field", "{refusal}");
    refusal
}

fn says_the_field(refusal: &Value, field: &str, constraint: &str, message: &str, operation: &str) {
    assert_eq!(refusal["field"], field, "{refusal}");
    assert_eq!(refusal["context"]["field"], field, "{refusal}");
    assert_eq!(refusal["context"]["constraint"], constraint, "{refusal}");
    assert_eq!(refusal["message"], message, "{refusal}");
    let next = &refusal["next_steps"][0];
    assert_eq!(next["action"], "correct_field", "{refusal}");
    assert_eq!(next["operation"], operation, "{refusal}");
    assert_eq!(next["field"], field, "{refusal}");
    assert_eq!(next["constraint"], constraint, "{refusal}");
}

#[tokio::test]
async fn declare_project_reaches_the_declaration_module() {
    let kestrel = Kestrel::boot().await;
    kestrel.declare_organization("acme").await;

    let refusal = refused_through_the_module(
        &kestrel,
        &projects_of("acme"),
        &json!({
            "name": "",
            "repositories": ["https://github.com/jtmthf/kestrel"],
            "branch": "main",
        }),
    )
    .await;

    says_the_field(
        &refusal,
        "name",
        "non_empty",
        "a declaration names each record",
        "declare_project",
    );
    assert!(listed_nothing(&kestrel, &projects_of("acme")).await);

    kestrel.teardown().await;
}

#[tokio::test]
async fn declare_agent_reaches_the_declaration_module() {
    let kestrel = Kestrel::boot().await;
    kestrel.declare_organization("acme").await;

    let refusal = refused_through_the_module(
        &kestrel,
        &agents_of("acme"),
        &json!({ "name": "builder", "harness": "" }),
    )
    .await;

    says_the_field(
        &refusal,
        "harness",
        "non_empty",
        "an agent names the harness that drives it",
        "declare_agent",
    );
    assert!(listed_nothing(&kestrel, &agents_of("acme")).await);

    kestrel.teardown().await;
}

#[tokio::test]
async fn declare_trigger_reaches_the_declaration_module() {
    let kestrel = Kestrel::boot().await;
    kestrel.declare_organization("acme").await;

    let refusal = refused_through_the_module(
        &kestrel,
        &triggers_of("acme"),
        &json!({
            "name": "ready",
            "filter": { "exact": { "type": "com.github.issues.labeled" } },
            "every": "1h",
            "brief": "Work on {{ event.type }}",
            "project": "kestrel",
            "agent": "builder",
        }),
    )
    .await;

    says_the_field(
        &refusal,
        "filter",
        "well_formed",
        "a trigger declares a filter, an interval or a cron expression, and only one",
        "declare_trigger",
    );
    assert!(listed_nothing(&kestrel, &triggers_of("acme")).await);

    kestrel.teardown().await;
}

#[tokio::test]
async fn apply_declaration_reaches_the_declaration_module() {
    let kestrel = Kestrel::boot().await;
    kestrel.declare_organization("acme").await;

    let refusal = refused_through_the_module(
        &kestrel,
        &declaration_of("acme"),
        &json!({
            "project": { "name": "", "repositories": ["https://github.com/jtmthf/kestrel"], "branch": "main" },
            "agent": { "name": "builder", "harness": "opencode" },
            "trigger": {
                "name": "ready",
                "filter": { "exact": { "type": "com.github.issues.labeled" } },
                "brief": "Work on {{ event.data.issue.title }}",
                "project": "kestrel",
                "agent": "builder",
            },
        }),
    )
    .await;

    says_the_field(
        &refusal,
        "project.name",
        "non_empty",
        "a declaration names each record",
        "apply_declaration",
    );
    assert!(listed_nothing(&kestrel, &projects_of("acme")).await);

    kestrel.teardown().await;
}

#[tokio::test]
async fn preview_declaration_reaches_the_declaration_module() {
    let kestrel = Kestrel::boot().await;
    kestrel.declare_organization("acme").await;

    let refusal = refused_through_the_module(
        &kestrel,
        &declaration_preview_of("acme"),
        &json!({
            "project": { "name": "kestrel", "repositories": ["https://github.com/jtmthf/kestrel"], "branch": "main" },
            "agent": { "name": "builder", "harness": "opencode" },
            "trigger": {
                "name": "ready",
                "filter": { "exact": { "type": "com.github.issues.labeled" } },
                "brief": "Work on {{ event.data.issue.title }}",
                "project": "elsewhere",
                "agent": "builder",
            },
        }),
    )
    .await;

    says_the_field(
        &refusal,
        "trigger.project",
        "matches_declared",
        "the trigger names project elsewhere, not the declared project kestrel",
        "preview_declaration",
    );
    assert!(listed_nothing(&kestrel, &projects_of("acme")).await);

    kestrel.teardown().await;
}

#[tokio::test]
async fn apply_triggers_reaches_the_declaration_module() {
    applied_triggers_reach_the_declaration_module(
        "apply_triggers",
        &operator::APPLIED_TRIGGERS.replace("{organization}", "acme"),
    )
    .await
}

#[tokio::test]
async fn preview_applied_triggers_reaches_the_declaration_module() {
    applied_triggers_reach_the_declaration_module(
        "preview_applied_triggers",
        &operator::APPLIED_TRIGGERS_PREVIEW.replace("{organization}", "acme"),
    )
    .await
}

async fn applied_triggers_reach_the_declaration_module(operation: &str, path: &str) {
    let kestrel = Kestrel::boot().await;
    kestrel.declare_organization("acme").await;

    let refusal = refused_through_the_module(
        &kestrel,
        path,
        &json!({ "triggers": { "nightly": {
            "filter": { "bogus": { "type": "example" } },
            "brief": "Tidy up",
            "project": "kestrel",
            "agent": "builder",
        } } }),
    )
    .await;

    says_the_field(
        &refusal,
        "triggers.nightly.filter",
        "well_formed",
        "a trigger filter: bogus is not a filter kestrel knows: use exact, prefix, suffix, all, \
         any or not",
        operation,
    );
    assert!(listed_nothing(&kestrel, &triggers_of("acme")).await);

    kestrel.teardown().await;
}

#[tokio::test]
async fn start_reaches_the_declaration_module() {
    let kestrel = Kestrel::boot().await;

    let mut start = a_start("acme", "opencode", "Fix the flaky test");
    start["project"]["branch"] = json!("");
    let refusal = refused_through_the_module(&kestrel, operator::STARTS, &start).await;

    says_the_field(
        &refusal,
        "project.branch",
        "non_empty",
        "a project names the branch its work happens on",
        "start",
    );
    assert!(listed(&kestrel, operator::ORGANIZATIONS).await.is_empty());

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_client_declaring_into_no_such_organization_is_refused() {
    let kestrel = Kestrel::boot().await;

    let refused = client(
        &kestrel,
        &[
            "project",
            "declare",
            "kestrel",
            "--organization",
            "acme",
            "--repository",
            "https://github.com/jtmthf/kestrel",
            "--branch",
            "main",
        ],
    )
    .await;
    let (status, refusal) = declared(
        &kestrel,
        &agents_of("acme"),
        &json!({ "name": "builder", "harness": "opencode" }),
    )
    .await;

    assert!(!refused.status.success());
    assert!(
        refused.err.contains("no organization named acme"),
        "{}",
        refused.err
    );
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(refusal["message"], "no organization named acme");

    kestrel.teardown().await;
}

#[tokio::test]
async fn an_organizations_declaration_that_names_nothing_is_refused() {
    let kestrel = Kestrel::boot().await;
    declared(
        &kestrel,
        operator::ORGANIZATIONS,
        &json!({ "name": "acme" }),
    )
    .await;

    // The boundary's own schema decides these, before any declaration is read.
    let (malformed, _) = declared(
        &kestrel,
        operator::ORGANIZATIONS,
        &json!({ "title": "acme" }),
    )
    .await;
    let (unnamed, _) = declared(&kestrel, operator::ORGANIZATIONS, &json!({ "name": "" })).await;

    assert_eq!(malformed, StatusCode::BAD_REQUEST);
    assert_eq!(unnamed, StatusCode::UNPROCESSABLE_ENTITY);

    kestrel.teardown().await;
}
