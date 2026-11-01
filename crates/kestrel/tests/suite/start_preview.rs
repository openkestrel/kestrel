//! The shared start plan through the operator boundary: a preview that writes nothing, and the
//! start that commits its Workspace, Session and initial setup completion together.

use crate::support;

use kestrel::operator;
use reqwest::StatusCode;
use serde_json::{Value, json};
use support::Kestrel;
use support::operator_requests::{a_start, agents_of, declared, got, listed, projects_of};

const SUBSCRIPTION: &str = "sk-ant-oat01-subscription-secret";
const KEY: &str = "sk-ant-api03-key";

async fn previewed(kestrel: &Kestrel, plan: &Value) -> Value {
    let (status, preview) = declared(kestrel, operator::START_PREVIEW, plan).await;
    assert_eq!(status, StatusCode::OK, "{preview:#}");
    let text = preview.to_string();
    for secret in [SUBSCRIPTION, KEY] {
        assert!(!text.contains(secret), "{text}");
    }
    let _: kestrel_operator_types::StartPreview =
        serde_json::from_value(preview.clone()).expect("the authored preview shape");

    preview
}

async fn complete(kestrel: &Kestrel) -> bool {
    let (status, install) = got(kestrel, operator::INSTALL).await;
    assert_eq!(status, StatusCode::OK, "{install}");
    install["initial_setup_complete"] == true
}

fn fields(diagnostics: &Value) -> Vec<&str> {
    diagnostics
        .as_array()
        .expect("diagnostics")
        .iter()
        .filter_map(|diagnostic| diagnostic["field"].as_str())
        .collect()
}

fn prerequisites(diagnostics: &Value) -> Vec<&str> {
    diagnostics
        .as_array()
        .expect("diagnostics")
        .iter()
        .filter_map(|diagnostic| diagnostic["context"]["prerequisite"].as_str())
        .collect()
}

/// The Operator's own subscription for claude, saved through the sign-in route.
async fn subscribed(kestrel: &Kestrel) {
    kestrel.name_operator("ada").await;
    let (status, saved) = support::operator_requests::requested(
        kestrel,
        reqwest::Method::PUT,
        "/operator/organizations/ada/sign-ins/claude/claude-setup-token",
        Some(&json!({ "value": SUBSCRIPTION })),
    )
    .await;
    assert!(status.is_success(), "{saved}");
}

#[tokio::test]
async fn an_empty_plan_on_a_new_install_reports_everything_missing_and_writes_nothing() {
    let kestrel = Kestrel::boot().await;

    let preview = previewed(&kestrel, &json!({})).await;

    assert_eq!(preview["outcome"], "blocked", "{preview:#}");
    assert_eq!(
        prerequisites(&preview["gaps"])[0],
        "operator",
        "{preview:#}"
    );
    assert_eq!(
        fields(&preview["missing"]),
        ["organization", "repositories", "branch", "brief"],
        "{preview:#}"
    );
    assert_eq!(preview["harness"]["value"], "opencode");
    assert_eq!(preview["harness"]["source"], "default");
    assert!(listed(&kestrel, operator::ORGANIZATIONS).await.is_empty());
    assert!(!complete(&kestrel).await);

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_clone_and_a_saved_subscription_resolve_the_plan_with_their_sources() {
    let kestrel = Kestrel::boot().await;
    subscribed(&kestrel).await;

    let preview = previewed(
        &kestrel,
        &json!({
            "brief": "Fix the flaky test",
            "clone": {
                "repository": "git@github.com:jtmthf/kestrel.git",
                "checked_out": "feature",
                "default_branch": "main",
            },
        }),
    )
    .await;

    assert_eq!(preview["outcome"], "ready", "{preview:#}");
    assert_eq!(
        preview["organization"],
        json!({ "value": "ada", "source": "only_organization", "record": null })
    );
    let repository = &preview["repositories"]["value"][0];
    assert_eq!(repository["given"], "git@github.com:jtmthf/kestrel.git");
    assert_eq!(repository["address"], "git@github.com:jtmthf/kestrel.git");
    assert_eq!(preview["repositories"]["source"], "clone_origin");
    assert_eq!(preview["project"]["value"], "kestrel");
    assert_eq!(preview["project"]["source"], "named_for_repository");
    assert_eq!(preview["branch"]["value"], "main");
    assert_eq!(preview["branch"]["source"], "clone_default_branch");
    assert_eq!(preview["harness"]["value"], "claude");
    assert_eq!(preview["harness"]["source"], "saved_sign_in");
    assert_eq!(preview["agent"]["source"], "named_for_harness");
    assert_eq!(preview["profile"]["source"], "operator_profile");
    assert_eq!(preview["sign_in_method"]["value"], "claude-setup-token");
    assert_eq!(preview["sign_in"]["method"], "claude-setup-token");
    let actions: Vec<(&str, &str)> = preview["declarations"]
        .as_array()
        .expect("declarations")
        .iter()
        .map(|step| {
            (
                step["kind"].as_str().unwrap_or_default(),
                step["action"].as_str().unwrap_or_default(),
            )
        })
        .collect();
    assert_eq!(
        actions,
        [
            ("organization", "reuse"),
            ("project", "declare"),
            ("agent", "declare"),
            ("workspace", "open"),
        ]
    );
    assert!(listed(&kestrel, &projects_of("ada")).await.is_empty());
    assert!(listed(&kestrel, &agents_of("ada")).await.is_empty());

    kestrel.teardown().await;
}

#[tokio::test]
async fn github_shorthand_is_shown_at_its_canonical_address_before_anything_is_declared() {
    let kestrel = Kestrel::boot().await;
    kestrel.name_operator("ada").await;

    let mut plan = a_start("ada", "opencode", "Fix the flaky test");
    plan["repositories"] = json!(["jtmthf/kestrel"]);
    plan["credentials"] = json!([{ "variable": "ANTHROPIC_API_KEY", "secret": KEY }]);
    let preview = previewed(&kestrel, &plan).await;

    let repository = &preview["repositories"]["value"][0];
    assert_eq!(repository["given"], "jtmthf/kestrel");
    assert_eq!(
        repository["address"],
        "https://github.com/jtmthf/kestrel.git"
    );
    assert_eq!(preview["outcome"], "ready", "{preview:#}");
    assert!(listed(&kestrel, &projects_of("ada")).await.is_empty());

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_declaration_the_plan_would_change_is_a_gap_and_the_start_preserves_it() {
    let kestrel = Kestrel::boot().await;
    kestrel.name_operator("ada").await;
    let ada = kestrel.declare_organization("ada").await;
    kestrel.declare_agent(&ada, "builder", "claude", None).await;
    let plan = a_start("ada", "opencode", "Fix the flaky test");

    let preview = previewed(&kestrel, &plan).await;
    assert_eq!(preview["outcome"], "blocked", "{preview:#}");
    assert_eq!(fields(&preview["gaps"]), ["agent.harness"], "{preview:#}");

    let (status, refused) = declared(&kestrel, operator::STARTS, &plan).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{refused}");
    assert_eq!(refused["field"], "agent.harness");
    assert_eq!(
        listed(&kestrel, &agents_of("ada")).await[0]["harness"],
        "claude"
    );
    assert!(listed(&kestrel, &projects_of("ada")).await.is_empty());
    assert!(!complete(&kestrel).await);

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_start_the_plan_does_not_complete_is_refused_with_the_input_it_needs() {
    let kestrel = Kestrel::boot().await;
    kestrel.name_operator("ada").await;
    let mut plan = a_start("ada", "opencode", "Fix the flaky test");
    plan.as_object_mut().expect("a plan").remove("brief");

    let (status, refused) = declared(&kestrel, operator::STARTS, &plan).await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{refused}");
    assert_eq!(refused["field"], "brief", "{refused}");
    assert!(listed(&kestrel, &projects_of("ada")).await.is_empty());
    assert!(!complete(&kestrel).await);

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_start_on_a_path_without_a_sign_in_is_refused_with_the_setup_gap() {
    let kestrel = Kestrel::boot().await;
    kestrel.name_operator("ada").await;
    let mut plan = a_start("ada", "opencode", "Fix the flaky test");
    plan.as_object_mut().expect("a plan").remove("credentials");

    let (status, refused) = declared(&kestrel, operator::STARTS, &plan).await;

    assert_eq!(status, StatusCode::CONFLICT, "{refused}");
    assert_eq!(refused["kind"], "setup_gap", "{refused}");
    assert_eq!(refused["context"]["prerequisite"], "sign_in", "{refused}");
    assert!(listed(&kestrel, &projects_of("ada")).await.is_empty());

    kestrel.teardown().await;
}

#[tokio::test]
async fn the_first_committed_start_completes_setup_once_and_it_survives_a_restart() {
    let kestrel = Kestrel::boot().await;
    subscribed(&kestrel).await;
    let plan = json!({
        "repositories": ["jtmthf/kestrel"],
        "branch": "main",
        "brief": "Fix the flaky test",
    });
    assert!(!complete(&kestrel).await);

    let (first, second) = tokio::join!(
        declared(&kestrel, operator::STARTS, &plan),
        declared(&kestrel, operator::STARTS, &plan),
    );
    for (status, started) in [&first, &second] {
        assert_eq!(*status, StatusCode::CREATED, "{started}");
        assert_eq!(started["session"]["state"], "queued", "{started}");
    }
    let completed: Vec<&Value> = [&first.1, &second.1]
        .into_iter()
        .filter(|started| started["completed_initial_setup"] == true)
        .collect();
    assert_eq!(completed.len(), 1, "{first:?} {second:?}");
    assert_eq!(first.1["workspace"]["profile"], "ada");
    assert!(complete(&kestrel).await);

    let kestrel = kestrel.kill().await.restart().await;
    assert!(complete(&kestrel).await);
    let preview = previewed(&kestrel, &plan).await;
    assert_eq!(preview["initial_setup_complete"], true);
    assert_eq!(preview["project"]["source"], "project");

    kestrel.teardown().await;
}

/// An Agent named for the Operator is refused only once the start's declarations are placed.
#[tokio::test]
async fn a_start_refused_inside_its_transaction_leaves_none_of_it_behind() {
    let kestrel = Kestrel::boot().await;
    kestrel.name_operator("ada").await;
    let mut plan = a_start("acme", "opencode", "Fix the flaky test");
    plan["agent"] = json!("ada");
    assert_eq!(previewed(&kestrel, &plan).await["outcome"], "ready");

    let (status, refused) = declared(&kestrel, operator::STARTS, &plan).await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{refused}");
    assert_eq!(refused["field"], "participant", "{refused}");
    let organizations: Vec<Value> = listed(&kestrel, operator::ORGANIZATIONS).await;
    assert_eq!(organizations.len(), 1, "{organizations:?}");
    assert_eq!(organizations[0]["name"], "ada");
    assert!(!complete(&kestrel).await);

    kestrel.teardown().await;
}
