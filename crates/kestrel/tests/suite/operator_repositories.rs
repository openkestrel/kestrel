//! Resolving the repositories a Project names, before and while it is declared.

use crate::support;

use kestrel::operator;
use reqwest::StatusCode;
use serde_json::json;
use support::Kestrel;
use support::operator_requests::{
    a_start, agent_model_of, agents_of, declaration_of, declaration_preview_of, declared, listed,
    listed_nothing, profile_variable_of, profiles_of, projects_of, requested,
};

#[tokio::test]
async fn a_project_holds_github_shorthand_as_its_https_address() {
    let kestrel = Kestrel::boot().await;
    kestrel.declare_organization("acme").await;

    let (status, declared_project) = declared(
        &kestrel,
        &projects_of("acme"),
        &json!({
            "name": "kestrel",
            "repositories": ["acme/api", "github.com/acme/web.git", "./acme/tools"],
            "branch": "main",
        }),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED, "{declared_project}");
    assert_eq!(
        listed(&kestrel, &projects_of("acme")).await[0]["repositories"],
        json!([
            "https://github.com/acme/api.git",
            "https://github.com/acme/web.git",
            "./acme/tools",
        ])
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn an_unreadable_repository_is_refused_on_its_field_and_saves_nothing() {
    let kestrel = Kestrel::boot().await;
    kestrel.declare_organization("acme").await;

    for repositories in [
        json!(["widgets"]),
        json!(["acme/api", ""]),
        json!(["svn://example.com/api"]),
    ] {
        let (status, refusal) = declared(
            &kestrel,
            &projects_of("acme"),
            &json!({ "name": "kestrel", "repositories": repositories, "branch": "main" }),
        )
        .await;

        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{refusal}");
        assert_eq!(refusal["kind"], "invalid_field");
        assert_eq!(refusal["field"], "repositories");
        assert_eq!(refusal["context"]["constraint"], "git_repository");
    }
    assert!(listed_nothing(&kestrel, &projects_of("acme")).await);

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_declaration_document_previews_and_applies_the_resolved_repositories() {
    let kestrel = Kestrel::boot().await;
    kestrel.declare_organization("acme").await;
    let declaration = json!({
        "project": { "name": "kestrel", "repositories": ["jtmthf/kestrel"], "branch": "main" },
        "agent": { "name": "builder", "harness": "opencode" },
        "trigger": {
            "name": "ready",
            "filter": { "exact": { "type": "com.github.issues.labeled" } },
            "brief": "Work on {{ event.data.issue.title }}",
            "project": "kestrel",
            "agent": "builder",
        },
    });
    let resolved = json!({
        "field": "repositories",
        "was": null,
        "becomes": "https://github.com/jtmthf/kestrel.git",
    });

    let (status, preview) = declared(&kestrel, &declaration_preview_of("acme"), &declaration).await;
    assert_eq!(status, StatusCode::OK, "{preview}");
    assert!(
        preview["declarations"][0]["differences"]
            .as_array()
            .expect("the project's differences")
            .contains(&resolved),
        "{preview}"
    );

    let (status, applied) = declared(&kestrel, &declaration_of("acme"), &declaration).await;
    assert_eq!(status, StatusCode::OK, "{applied}");
    assert_eq!(
        listed(&kestrel, &projects_of("acme")).await[0]["repositories"],
        json!(["https://github.com/jtmthf/kestrel.git"])
    );

    let (status, again) = declared(&kestrel, &declaration_preview_of("acme"), &declaration).await;
    assert_eq!(status, StatusCode::OK, "{again}");
    assert_eq!(again["declarations"][0]["action"], "unchanged", "{again}");

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_declaration_document_whose_repositories_collide_after_resolution_saves_nothing() {
    let kestrel = Kestrel::boot().await;
    kestrel.declare_organization("acme").await;
    let declaration = json!({
        "project": {
            "name": "kestrel",
            "repositories": ["acme/api", "https://git.example.com/team/api"],
            "branch": "main",
        },
        "agent": { "name": "builder", "harness": "opencode" },
        "trigger": {
            "name": "ready",
            "filter": { "exact": { "type": "com.github.issues.labeled" } },
            "brief": "Work on {{ event.data.issue.title }}",
            "project": "kestrel",
            "agent": "builder",
        },
    });

    for path in [declaration_preview_of("acme"), declaration_of("acme")] {
        let (status, refusal) = declared(&kestrel, &path, &declaration).await;

        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{refusal}");
        assert_eq!(refusal["field"], "project.repositories");
        assert_eq!(
            refusal["context"]["constraint"],
            "distinct_checkout_directories"
        );
    }
    assert!(listed_nothing(&kestrel, &projects_of("acme")).await);
    assert!(listed_nothing(&kestrel, &agents_of("acme")).await);

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_start_declares_its_project_against_the_resolved_repositories() {
    let kestrel = Kestrel::boot().await;
    kestrel.name_operator("ada").await;
    let mut start = a_start("acme", "opencode", "Fix the flaky test");
    start["repositories"] = json!(["jtmthf/kestrel"]);

    let (status, started) = declared(&kestrel, operator::STARTS, &start).await;
    assert_eq!(status, StatusCode::CREATED, "{started}");
    assert_eq!(
        listed(&kestrel, &projects_of("acme")).await[0]["repositories"],
        json!(["https://github.com/jtmthf/kestrel.git"])
    );

    start["repositories"] = json!(["https://github.com/jtmthf/kestrel.git"]);
    let (status, again) = declared(&kestrel, operator::STARTS, &start).await;
    assert_eq!(status, StatusCode::CREATED, "{again}");
    assert_eq!(again["project"]["created"], false);

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_start_naming_an_unreadable_repository_leaves_nothing_behind() {
    let kestrel = Kestrel::boot().await;
    let mut start = a_start("acme", "opencode", "Fix the flaky test");
    start["repositories"] = json!(["kestrel"]);

    let (status, refusal) = declared(&kestrel, operator::STARTS, &start).await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{refusal}");
    assert_eq!(refusal["field"], "repositories");
    assert_eq!(refusal["context"]["constraint"], "git_repository");
    assert!(listed(&kestrel, operator::ORGANIZATIONS).await.is_empty());

    kestrel.teardown().await;
}

#[tokio::test]
async fn repositories_resolve_without_anything_being_declared() {
    let kestrel = Kestrel::boot().await;

    let (status, resolution) = declared(
        &kestrel,
        operator::REPOSITORY_RESOLUTIONS,
        &json!({ "repositories": ["acme/api", "git@github.com:acme/web.git", "/srv/git/tools"] }),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{resolution}");
    assert_eq!(
        resolution,
        json!({
            "repositories": [
                {
                    "given": "acme/api",
                    "address": "https://github.com/acme/api.git",
                    "checkout_directory": "api",
                    "integration": null,
                },
                {
                    "given": "git@github.com:acme/web.git",
                    "address": "git@github.com:acme/web.git",
                    "checkout_directory": "web",
                    "integration": null,
                },
                {
                    "given": "/srv/git/tools",
                    "address": "/srv/git/tools",
                    "checkout_directory": "tools",
                    "integration": null,
                },
            ],
            "warnings": [],
        })
    );
    assert!(listed(&kestrel, operator::ORGANIZATIONS).await.is_empty());

    kestrel.teardown().await;
}

#[tokio::test]
async fn public_setup_resolves_only_https_and_warns_nothing_app_backed_can_push() {
    let kestrel = Kestrel::boot().await;

    let (status, resolution) = declared(
        &kestrel,
        operator::REPOSITORY_RESOLUTIONS,
        &json!({ "repositories": ["acme/api"], "purpose": "public_setup" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{resolution}");
    assert_eq!(
        resolution["repositories"][0]["address"],
        "https://github.com/acme/api.git"
    );
    assert_eq!(resolution["warnings"][0]["code"], "no_app_push_authority");

    let (status, refusal) = declared(
        &kestrel,
        operator::REPOSITORY_RESOLUTIONS,
        &json!({ "repositories": ["git@github.com:acme/api.git"], "purpose": "public_setup" }),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{refusal}");
    assert_eq!(refusal["kind"], "invalid_field");
    assert_eq!(refusal["context"]["constraint"], "https_repository");
    assert_eq!(
        refusal["next_steps"][0]["operation"],
        "resolve_repositories"
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn an_agent_names_a_model_a_newly_added_profile_could_offer() {
    let kestrel = Kestrel::boot().await;
    kestrel.declare_organization("acme").await;
    let (built, _) = declared(
        &kestrel,
        &agents_of("acme"),
        &json!({ "name": "builder", "harness": "opencode" }),
    )
    .await;
    assert_eq!(built, StatusCode::CREATED);

    let (subscribed, _) = declared(
        &kestrel,
        &profiles_of("acme"),
        &json!({ "name": "jack", "owner": "Jack" }),
    )
    .await;
    assert_eq!(subscribed, StatusCode::CREATED);
    let (keyed, _) = requested(
        &kestrel,
        reqwest::Method::PUT,
        &profile_variable_of("acme", "jack", "OPENCODE_API_KEY"),
        Some(&json!({ "secret": "jacks-subscription-key" })),
    )
    .await;
    assert_eq!(keyed, StatusCode::OK);

    let (changed, model) = requested(
        &kestrel,
        reqwest::Method::PUT,
        &agent_model_of("acme", "builder"),
        Some(&json!({ "model": "opencode-go/glm-5.3" })),
    )
    .await;
    let (named, fresh) = declared(
        &kestrel,
        &agents_of("acme"),
        &json!({
            "name": "reviewer",
            "harness": "opencode",
            "model": "opencode-go/glm-5.3"
        }),
    )
    .await;

    assert_eq!(changed, StatusCode::OK, "{model}");
    assert_eq!(model["model"], "opencode-go/glm-5.3");
    assert_eq!(named, StatusCode::CREATED, "{fresh}");
    assert_eq!(fresh["model"], "opencode-go/glm-5.3");

    kestrel.teardown().await;
}
