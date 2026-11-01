//! A start declares its setup and reaches a Session; a changed declaration converges on the
//! record by that name, and a missing reference offers its repair.

use crate::support;

use kestrel::log::{BriefSource, Entry};
use kestrel::operator;
use reqwest::StatusCode;
use serde_json::{Value, json};
use support::Kestrel;
use support::operator_requests::{
    a_start, agent_model_of, agents_of, credential_of, credentials_of, declared, listed,
    listed_nothing, profile_variable_of, profiles_of, projects_of, ready_to_open, requested,
};

#[tokio::test]
async fn an_unchanged_declaration_repeated_answers_the_record_it_made() {
    let kestrel = Kestrel::boot().await;
    let project = json!({
        "name": "kestrel",
        "repositories": ["https://github.com/jtmthf/kestrel"],
        "branch": "main",
    });
    let agent = json!({ "name": "builder", "harness": "opencode", "model": "claude-opus-5" });

    let declarations = [
        (
            operator::ORGANIZATIONS.to_owned(),
            json!({ "name": "acme" }),
        ),
        (projects_of("acme"), project),
        (agents_of("acme"), agent),
    ];

    for (path, declaration) in &declarations {
        let (created, first) = declared(&kestrel, path, declaration).await;
        let (repeated, second) = declared(&kestrel, path, declaration).await;

        assert_eq!(created, StatusCode::CREATED, "{first}");
        assert_eq!(repeated, StatusCode::OK, "{second}");
        assert_eq!(first, second);
    }
    assert_eq!(listed(&kestrel, operator::ORGANIZATIONS).await.len(), 1);
    assert_eq!(listed(&kestrel, &projects_of("acme")).await.len(), 1);
    assert_eq!(listed(&kestrel, &agents_of("acme")).await.len(), 1);

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_start_declares_its_setup_and_reaches_a_session_carrying_its_brief() {
    let kestrel = Kestrel::boot().await;
    kestrel.name_operator("ada").await;

    let (status, started) = declared(
        &kestrel,
        operator::STARTS,
        &a_start("acme", "opencode", "Fix the flaky test"),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED, "{started}");
    for (kind, name) in [
        ("organization", "acme"),
        ("project", "kestrel"),
        ("agent", "builder"),
    ] {
        assert_eq!(started[kind], json!({ "name": name, "created": true }));
    }
    assert_eq!(started["session"]["workspace"], started["workspace"]["id"]);
    assert_eq!(started["session"]["state"], "queued");
    let workspace = started["workspace"]["id"]
        .as_str()
        .expect("a workspace id")
        .parse()
        .expect("a workspace identifier");
    let transcript = kestrel.transcript(workspace).await;
    assert_eq!(
        transcript[0].entry,
        Entry::ParticipantJoined {
            participant: "builder".to_owned(),
        }
    );
    assert_eq!(
        transcript[1].entry,
        Entry::ParticipantJoined {
            participant: "ada".to_owned(),
        }
    );
    assert_eq!(
        transcript[2].entry,
        Entry::Brief {
            source: BriefSource::Operator {
                participant: Some("ada".to_owned()),
            },
            brief: "Fix the flaky test".to_owned(),
        }
    );

    let (status, again) = declared(
        &kestrel,
        operator::STARTS,
        &a_start("acme", "opencode", "And another"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{again}");
    assert_eq!(again["project"]["created"], false);
    assert_ne!(again["workspace"]["id"], started["workspace"]["id"]);

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_start_that_would_change_a_declaration_leaves_nothing_behind() {
    let kestrel = Kestrel::boot().await;
    let acme = kestrel.declare_organization("acme").await;
    kestrel
        .declare_agent(&acme, "builder", "claude", None)
        .await;

    let mut start = a_start("acme", "opencode", "Fix the flaky test");
    start["credentials"] = json!([{ "variable": "ANTHROPIC_API_KEY", "secret": "sk-ant" }]);
    let (status, refused) = declared(&kestrel, operator::STARTS, &start).await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{refused}");
    assert_eq!(refused["kind"], "invalid_field", "{refused}");
    assert_eq!(refused["field"], "agent.harness", "{refused}");
    assert_eq!(refused["context"]["constraint"], "immutable", "{refused}");
    assert_eq!(
        refused["message"],
        "the agent builder is declared on the harness claude with the model its harness's \
         default, and a start changes no declaration",
        "{refused}"
    );
    let next = &refused["next_steps"][0];
    assert_eq!(next["action"], "correct_field", "{refused}");
    assert_eq!(next["operation"], "start", "{refused}");
    assert_eq!(next["field"], "agent.harness", "{refused}");
    assert_eq!(next["constraint"], "immutable", "{refused}");
    assert!(listed_nothing(&kestrel, &credentials_of("acme")).await);
    assert!(
        refused["message"]
            .as_str()
            .is_some_and(|message| message.contains("claude")),
        "{refused}"
    );
    assert!(listed_nothing(&kestrel, &projects_of("acme")).await);
    assert!(
        listed_nothing(
            &kestrel,
            &operator::WORKSPACES.replace("{organization}", "acme")
        )
        .await
    );
    assert_eq!(
        listed(&kestrel, &agents_of("acme")).await[0]["harness"],
        "claude"
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_changed_project_declaration_converges_on_the_project_by_that_name() {
    let kestrel = Kestrel::boot().await;
    declared(
        &kestrel,
        operator::ORGANIZATIONS,
        &json!({ "name": "acme" }),
    )
    .await;
    let (_, first) = declared(
        &kestrel,
        &projects_of("acme"),
        &json!({
            "name": "kestrel",
            "repositories": [
                "https://github.com/jtmthf/kestrel",
                "https://github.com/jtmthf/skills",
            ],
            "branch": "main",
        }),
    )
    .await;

    let (status, changed) = declared(
        &kestrel,
        &projects_of("acme"),
        &json!({
            "name": "kestrel",
            "repositories": ["https://github.com/jtmthf/skills"],
            "branch": "next",
        }),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{changed}");
    assert_eq!(changed["id"], first["id"]);
    assert_eq!(
        changed["repositories"],
        json!(["https://github.com/jtmthf/skills"])
    );
    assert_eq!(changed["branch"], "next");
    assert_eq!(listed(&kestrel, &projects_of("acme")).await, vec![changed]);

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_changed_agent_declaration_converges_on_the_agent_by_that_name() {
    let kestrel = Kestrel::boot().await;
    declared(
        &kestrel,
        operator::ORGANIZATIONS,
        &json!({ "name": "acme" }),
    )
    .await;
    let (_, first) = declared(
        &kestrel,
        &agents_of("acme"),
        &json!({ "name": "builder", "harness": "opencode", "model": "claude-opus-5" }),
    )
    .await;

    let (_, changed) = declared(
        &kestrel,
        &agents_of("acme"),
        &json!({ "name": "builder", "harness": "claude-code", "model": "claude-sonnet-5" }),
    )
    .await;
    let (status, unnamed) = declared(
        &kestrel,
        &agents_of("acme"),
        &json!({ "name": "builder", "harness": "claude-code" }),
    )
    .await;

    assert_eq!(changed["id"], first["id"]);
    assert_eq!(changed["harness"], "claude-code");
    assert_eq!(changed["model"], "claude-sonnet-5");
    assert_eq!(status, StatusCode::OK, "{unnamed}");
    assert_eq!(unnamed["id"], first["id"]);
    assert_eq!(unnamed["model"], Value::Null);
    assert_eq!(listed(&kestrel, &agents_of("acme")).await, vec![unnamed]);

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_missing_reference_offers_the_declaration_that_repairs_it() {
    let kestrel = Kestrel::boot().await;
    ready_to_open(&kestrel).await;

    let (status, refusal) = declared(
        &kestrel,
        &agents_of("ghost"),
        &json!({ "name": "builder", "harness": "opencode" }),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{refusal}");
    assert_eq!(refusal["kind"], "missing_reference");
    assert_eq!(refusal["context"]["resource"], "organization");
    assert_eq!(refusal["context"]["reference"], "ghost");
    assert_eq!(refusal["next_steps"][0]["action"], "declare_organization");
    assert_eq!(refusal["next_steps"][0]["name"], "ghost");

    let (status, refusal) = requested(
        &kestrel,
        reqwest::Method::PUT,
        &agent_model_of("acme", "ghost"),
        Some(&json!({ "model": "claude-opus-5" })),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{refusal}");
    assert_eq!(refusal["kind"], "missing_reference");
    assert_eq!(refusal["context"]["resource"], "agent");
    assert_eq!(refusal["context"]["organization"], "acme");
    let next = &refusal["next_steps"][0];
    assert_eq!(next["action"], "declare_agent");
    assert_eq!(next["organization"], "acme");
    assert_eq!(next["name"], "ghost");
    assert_eq!(next["missing"], json!(["harness"]));

    let (status, refusal) = requested(
        &kestrel,
        reqwest::Method::PUT,
        &profile_variable_of("acme", "ghost", "ANTHROPIC_API_KEY"),
        Some(&json!({ "secret": "sk-test" })),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{refusal}");
    assert_eq!(refusal["kind"], "missing_reference");
    assert_eq!(refusal["context"]["resource"], "subscription_profile");
    assert_eq!(
        refusal["next_steps"][0]["action"],
        "declare_subscription_profile"
    );
    assert_eq!(refusal["next_steps"][0]["missing"], json!(["owner"]));

    let (status, refusal) = requested(
        &kestrel,
        reqwest::Method::DELETE,
        &credential_of("acme", "GHOST_KEY"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{refusal}");
    assert_eq!(refusal["kind"], "missing_reference");
    assert_eq!(refusal["context"]["resource"], "provider_credential");
    let next = &refusal["next_steps"][0];
    assert_eq!(next["action"], "set_provider_credential");
    assert_eq!(next["organization"], "acme");
    assert_eq!(next["name"], "GHOST_KEY");

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_subscription_profile_with_no_owner_names_the_person_it_belongs_to() {
    let kestrel = Kestrel::boot().await;
    kestrel.declare_organization("acme").await;

    let (status, refusal) = declared(
        &kestrel,
        &profiles_of("acme"),
        &json!({ "name": "jack", "owner": "" }),
    )
    .await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{refusal}");
    assert_eq!(refusal["kind"], "invalid_field", "{refusal}");
    assert_eq!(refusal["field"], "owner", "{refusal}");
    assert_eq!(refusal["context"]["constraint"], "non_empty", "{refusal}");
    assert_eq!(
        refusal["message"], "a subscription profile belongs to a person, and none was named",
        "{refusal}"
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_subscription_profile_that_belongs_to_someone_else_is_a_typed_state_conflict() {
    let kestrel = Kestrel::boot().await;
    kestrel.declare_organization("acme").await;
    kestrel
        .declare_profile("acme", "jack", "Jack")
        .await
        .expect("the profile should declare");

    let (status, refusal) = declared(
        &kestrel,
        &profiles_of("acme"),
        &json!({ "name": "jack", "owner": "Alex" }),
    )
    .await;

    assert_eq!(status, StatusCode::CONFLICT, "{refusal}");
    assert_eq!(refusal["kind"], "state_conflict");
    assert_eq!(refusal["context"]["resource"], "subscription_profile");
    assert_eq!(refusal["context"]["reference"], "jack");
    assert_eq!(
        refusal["context"]["operation"],
        "declare_subscription_profile"
    );
    assert_eq!(refusal["context"]["state"], "owned_by_another");
    assert_eq!(refusal["next_steps"][0]["action"], "inspect_resource");

    kestrel.teardown().await;
}
