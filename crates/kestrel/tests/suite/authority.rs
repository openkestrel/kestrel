use crate::support;

use std::time::Duration;

use jiff::SignedDuration;
use kestrel::domain::Direction;
use kestrel::integration::Change;
use kestrel::operator;
use reqwest::StatusCode;
use serde_json::{Value, json};
use support::Kestrel;
use support::github_stub::{self, GithubStub, ScriptedResponse};

const REPOSITORY: &str = "jtmthf/kestrel";
const HTTPS: &str = "https://github.com/jtmthf/kestrel.git";
const SSH: &str = "git@github.com:jtmthf/kestrel.git";
const OUTBOUND: &[Direction] = &[Direction::Outbound];
const MINTED: &str = "/access_tokens";
const NAMED: &str = "/repos/jtmthf/kestrel";

async fn requested(
    kestrel: &Kestrel,
    method: reqwest::Method,
    path: &str,
    body: Option<&Value>,
) -> (StatusCode, Value) {
    let mut request =
        reqwest::Client::new().request(method, format!("{}{path}", kestrel.operator()));
    if let Some(body) = body {
        request = request.json(body);
    }
    let response = request
        .send()
        .await
        .expect("the operator boundary should answer");
    let status = response.status();
    let text = response.text().await.expect("a body");

    (status, serde_json::from_str(&text).unwrap_or(Value::Null))
}

async fn posted(kestrel: &Kestrel, path: &str, body: &Value) -> (StatusCode, Value) {
    requested(kestrel, reqwest::Method::POST, path, Some(body)).await
}

async fn got(kestrel: &Kestrel, path: &str) -> (StatusCode, Value) {
    requested(kestrel, reqwest::Method::GET, path, None).await
}

fn of(path: &str) -> String {
    path.replace("{organization}", "acme")
}

async fn connected(kestrel: &Kestrel, stub: &GithubStub) {
    kestrel.declare_organization("acme").await;
    connect(kestrel, stub, "github", REPOSITORY).await;
}

async fn connect(kestrel: &Kestrel, stub: &GithubStub, name: &str, repository: &str) {
    kestrel
        .register_integration(
            "acme",
            name,
            repository,
            &stub.base_url(),
            OUTBOUND,
            SignedDuration::from_hours(1),
        )
        .await;
}

fn has_branch(stub: &GithubStub, branch: &str) {
    stub.script_answer(
        "GET",
        &format!("/branches/{branch}"),
        ScriptedResponse::ok(json!({ "name": branch }).to_string()),
    );
}

fn selecting(repository: &str, branch: &str, integration: &str) -> Value {
    json!({
        "name": "web",
        "repositories": [repository],
        "integrations": [{ "repository": repository, "integration": integration }],
        "branch": branch,
    })
}

async fn declare(kestrel: &Kestrel, declaration: &Value) -> (StatusCode, Value) {
    posted(kestrel, &of(operator::PROJECTS), declaration).await
}

async fn projects(kestrel: &Kestrel) -> Value {
    let (status, projects) = got(kestrel, &of(operator::PROJECTS)).await;
    assert_eq!(status, StatusCode::OK, "{projects}");
    projects
}

fn asked(stub: &GithubStub) -> usize {
    stub.requests().len()
}

fn document(project: &Value) -> Value {
    json!({
        "project": project,
        "agent": { "name": "builder", "harness": "opencode" },
        "trigger": {
            "name": "ready",
            "filter": { "exact": { "type": "com.github.issues.labeled" } },
            "brief": "Work on {{ event.data.issue.title }}",
            "project": "web",
            "agent": "builder",
        },
    })
}

async fn nothing_declared(kestrel: &Kestrel) {
    assert_eq!(projects(kestrel).await, json!([]));
    let (_, agents) = got(kestrel, &of(operator::AGENTS)).await;
    assert_eq!(agents, json!([]));
    let (_, workspaces) = got(kestrel, &of(operator::WORKSPACES)).await;
    assert_eq!(workspaces, json!([]));
}

#[tokio::test]
async fn a_selected_integration_is_asked_for_the_repository_and_branch_before_the_project_is_saved()
{
    let stub = GithubStub::start();
    let kestrel = Kestrel::boot().await;
    connected(&kestrel, &stub).await;
    has_branch(&stub, "release/1.x");

    let (status, project) = declare(&kestrel, &selecting(SSH, "release/1.x", "github")).await;

    assert_eq!(status, StatusCode::CREATED, "{project}");
    assert_eq!(project["repositories"], json!([HTTPS]));
    assert_eq!(project["integrations"][0]["repository"], HTTPS);
    assert_eq!(project["integrations"][0]["integration"], "github");
    assert_eq!(
        project["integrations"][0]["integration_id"],
        kestrel.integration("acme", "github").await.id.to_string()
    );
    assert!(
        stub.requests()
            .iter()
            .any(|request| request.url == "/repos/jtmthf/kestrel/branches/release/1.x"),
        "the branch was never asked for"
    );
    assert_eq!(projects(&kestrel).await[0], project);

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_github_address_alone_selects_no_integration_and_asks_github_nothing() {
    let stub = GithubStub::start();
    let kestrel = Kestrel::boot().await;
    connected(&kestrel, &stub).await;
    let before = asked(&stub);

    let (status, project) = declare(
        &kestrel,
        &json!({ "name": "web", "repositories": [REPOSITORY], "branch": "main" }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{project}");
    assert_eq!(project["repositories"], json!([HTTPS]));
    assert_eq!(project["integrations"], json!([]));

    let (status, project) = declare(
        &kestrel,
        &json!({ "name": "tools", "repositories": [SSH], "branch": "main" }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{project}");
    assert_eq!(project["repositories"], json!([SSH]));
    assert_eq!(project["integrations"], json!([]));
    assert_eq!(asked(&stub), before);

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_branch_the_app_cannot_see_is_refused_and_the_previous_declaration_stands() {
    let stub = GithubStub::start();
    let kestrel = Kestrel::boot().await;
    connected(&kestrel, &stub).await;

    let (status, refusal) = declare(&kestrel, &selecting(REPOSITORY, "main", "github")).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{refusal}");
    assert_eq!(refusal["kind"], "invalid_field");
    assert_eq!(refusal["context"]["field"], "branch");
    assert_eq!(refusal["context"]["constraint"], "branch_in_repository");
    assert_eq!(projects(&kestrel).await, json!([]));

    stub.script_answer(
        "GET",
        "/branches/master",
        ScriptedResponse::ok(json!({ "name": "main" }).to_string()),
    );
    for absent in ["master", "..", "."] {
        let (status, refusal) = declare(&kestrel, &selecting(REPOSITORY, absent, "github")).await;
        assert_eq!(
            status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{absent}: {refusal}"
        );
        assert_eq!(refusal["context"]["constraint"], "branch_in_repository");
    }

    has_branch(&stub, "main");
    let (status, saved) = declare(&kestrel, &selecting(REPOSITORY, "main", "github")).await;
    assert_eq!(status, StatusCode::CREATED, "{saved}");

    let (status, refusal) = declare(&kestrel, &selecting(REPOSITORY, "gone", "github")).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{refusal}");
    assert_eq!(projects(&kestrel).await, json!([saved]));

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_repository_the_app_cannot_read_is_refused_without_claiming_it_does_not_exist() {
    let stub = GithubStub::start();
    let kestrel = Kestrel::boot().await;
    connected(&kestrel, &stub).await;
    stub.script_answer("GET", NAMED, ScriptedResponse::answering(404));

    let (status, refusal) = declare(&kestrel, &selecting(REPOSITORY, "main", "github")).await;

    assert_eq!(status, StatusCode::CONFLICT, "{refusal}");
    assert_eq!(refusal["kind"], "state_conflict");
    assert_eq!(refusal["context"]["resource"], "integration");
    assert_eq!(refusal["context"]["state"], "repository_unreachable");
    assert_eq!(refusal["next_steps"][0]["action"], "inspect_resource");
    let message = refusal["message"].as_str().expect("a message");
    assert!(
        message.contains("does not establish whether the repository exists"),
        "{message}"
    );
    assert_eq!(projects(&kestrel).await, json!([]));

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_key_github_rejects_is_refused_with_its_replacement_as_the_repair() {
    let stub = GithubStub::start();
    let kestrel = Kestrel::boot().await;
    connected(&kestrel, &stub).await;
    stub.script_answer("POST", MINTED, ScriptedResponse::answering(401));

    let (status, refusal) = declare(&kestrel, &selecting(REPOSITORY, "main", "github")).await;

    assert_eq!(status, StatusCode::CONFLICT, "{refusal}");
    assert_eq!(refusal["context"]["state"], "credentials_rejected");
    assert_eq!(
        refusal["next_steps"][1]["action"],
        "replace_integration_private_key"
    );
    assert_eq!(refusal["next_steps"][1]["integration"], "github");
    assert_eq!(projects(&kestrel).await, json!([]));

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_rate_limit_or_an_outage_is_refused_as_safe_to_send_again() {
    let stub = GithubStub::start();
    let kestrel = Kestrel::boot().await;
    connected(&kestrel, &stub).await;

    for unanswered in [
        github_stub::rate_limited(),
        ScriptedResponse::answering(502),
        ScriptedResponse::answering(429),
    ] {
        let limited = unanswered.status == 403;
        stub.script_answer("GET", "/branches/main", unanswered);

        let (status, refusal) = declare(&kestrel, &selecting(REPOSITORY, "main", "github")).await;

        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{refusal}");
        assert_eq!(refusal["kind"], "unavailable");
        assert_eq!(refusal["context"]["service"], "github");
        assert_eq!(refusal["context"]["operation"], "declare_project");
        assert_eq!(refusal["next_steps"][0]["action"], "retry_request");
        assert_eq!(
            refusal["context"]["retry_after_seconds"].is_number(),
            limited,
            "{refusal}"
        );
        assert_eq!(projects(&kestrel).await, json!([]));
    }

    kestrel.teardown().await;
}

#[tokio::test]
async fn an_integration_out_of_use_or_of_another_repository_cannot_be_selected() {
    let stub = GithubStub::start();
    let kestrel = Kestrel::boot().await;
    connected(&kestrel, &stub).await;
    connect(&kestrel, &stub, "elsewhere", "jtmthf/other").await;
    kestrel
        .register_webhook("acme", "hook", "a-shared-secret")
        .await;
    let before = asked(&stub);

    for (declaration, status, expected) in [
        (
            selecting(REPOSITORY, "main", "absent"),
            StatusCode::NOT_FOUND,
            json!({ "kind": "missing_reference" }),
        ),
        (
            selecting(REPOSITORY, "main", "elsewhere"),
            StatusCode::UNPROCESSABLE_ENTITY,
            json!({ "kind": "invalid_field", "constraint": "watched_by_integration" }),
        ),
        (
            selecting(REPOSITORY, "main", "hook"),
            StatusCode::FORBIDDEN,
            json!({ "kind": "forbidden_action" }),
        ),
        (
            json!({
                "name": "web",
                "repositories": [REPOSITORY],
                "integrations": [{ "repository": "jtmthf/other", "integration": "elsewhere" }],
                "branch": "main",
            }),
            StatusCode::UNPROCESSABLE_ENTITY,
            json!({ "kind": "invalid_field", "constraint": "declared_repository" }),
        ),
    ] {
        let (answered, refusal) = declare(&kestrel, &declaration).await;
        assert_eq!(answered, status, "{refusal}");
        assert_eq!(refusal["kind"], expected["kind"], "{refusal}");
        if let Some(constraint) = expected.get("constraint") {
            assert_eq!(&refusal["context"]["constraint"], constraint, "{refusal}");
            assert_eq!(refusal["context"]["field"], "integrations", "{refusal}");
        }
    }

    kestrel
        .change_integration(
            "acme",
            "github",
            Change {
                carries: Some(&[Direction::Inbound]),
                ..Change::default()
            },
        )
        .await
        .expect("the directions should change");
    let (status, refusal) = declare(&kestrel, &selecting(REPOSITORY, "main", "github")).await;
    assert_eq!(status, StatusCode::CONFLICT, "{refusal}");
    assert_eq!(refusal["context"]["state"], "outbound_not_carried");

    kestrel.disable_integration("acme", "github").await;
    let (status, refusal) = declare(&kestrel, &selecting(REPOSITORY, "main", "github")).await;
    assert_eq!(status, StatusCode::CONFLICT, "{refusal}");
    assert_eq!(refusal["context"]["state"], "disabled");
    assert_eq!(refusal["next_steps"][1]["action"], "enable_integration");

    kestrel.retire_integration("acme", "github").await;
    let (status, refusal) = declare(&kestrel, &selecting(REPOSITORY, "main", "github")).await;
    assert_eq!(status, StatusCode::CONFLICT, "{refusal}");
    assert_eq!(refusal["context"]["state"], "retired");

    let selections = stub.requests()[before..]
        .iter()
        .filter(|request| request.url.starts_with("/repos/"))
        .count();
    assert_eq!(selections, 0, "github was asked about a refused selection");
    assert_eq!(projects(&kestrel).await, json!([]));

    kestrel.teardown().await;
}

#[tokio::test]
async fn maintenance_that_lands_while_github_is_asked_refuses_the_declaration() {
    let stub = GithubStub::start();
    let kestrel = Kestrel::boot().await;
    connected(&kestrel, &stub).await;
    stub.script_answer(
        "GET",
        "/branches/main",
        ScriptedResponse::ok(json!({ "name": "main" }).to_string()).after(Duration::from_secs(1)),
    );

    let declaration = selecting(REPOSITORY, "main", "github");

    let ((status, refusal), _) = tokio::join!(declare(&kestrel, &declaration), async {
        while !stub
            .requests()
            .iter()
            .any(|request| request.url.ends_with("/branches/main"))
        {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        kestrel.disable_integration("acme", "github").await
    });

    assert_eq!(status, StatusCode::CONFLICT, "{refusal}");
    assert_eq!(refusal["context"]["resource"], "integration");
    assert_eq!(refusal["context"]["state"], "revised");
    assert_eq!(projects(&kestrel).await, json!([]));

    kestrel.teardown().await;
}

#[tokio::test]
async fn an_unchanged_binding_is_fenced_while_another_repository_is_checked() {
    let stub = GithubStub::start();
    let kestrel = Kestrel::boot().await;
    connected(&kestrel, &stub).await;
    connect(&kestrel, &stub, "tools", "jtmthf/tools").await;
    has_branch(&stub, "main");
    let (_, saved) = declare(&kestrel, &selecting(REPOSITORY, "main", "github")).await;
    stub.script_answer(
        "GET",
        "/repos/jtmthf/tools/branches/main",
        ScriptedResponse::ok(json!({ "name": "main" }).to_string()).after(Duration::from_secs(1)),
    );
    let declaration = json!({
        "name": "web",
        "repositories": [REPOSITORY, "jtmthf/tools"],
        "integrations": [
            { "repository": REPOSITORY, "integration": "github" },
            { "repository": "jtmthf/tools", "integration": "tools" },
        ],
        "branch": "main",
    });
    let ((status, refusal), _) = tokio::join!(declare(&kestrel, &declaration), async {
        while !stub
            .requests()
            .iter()
            .any(|request| request.url.ends_with("/repos/jtmthf/tools/branches/main"))
        {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        kestrel.disable_integration("acme", "github").await
    });

    assert_eq!(status, StatusCode::CONFLICT, "{refusal}");
    assert_eq!(refusal["context"]["state"], "revised");
    assert_eq!(refusal["context"]["reference"], "github");
    assert_eq!(projects(&kestrel).await, json!([saved]));
    kestrel.teardown().await;
}

#[tokio::test]
async fn a_declaration_that_changes_nothing_under_a_selection_asks_github_nothing() {
    let stub = GithubStub::start();
    let kestrel = Kestrel::boot().await;
    connected(&kestrel, &stub).await;
    has_branch(&stub, "main");
    let (_, saved) = declare(&kestrel, &selecting(REPOSITORY, "main", "github")).await;
    let before = asked(&stub);

    let (status, again) = declare(&kestrel, &selecting(SSH, "main", "github")).await;

    assert_eq!(status, StatusCode::OK, "{again}");
    assert_eq!(again, saved);
    assert_eq!(asked(&stub), before);

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_preview_asks_github_and_an_apply_asks_again_and_a_refused_document_declares_nothing() {
    let stub = GithubStub::start();
    let kestrel = Kestrel::boot().await;
    connected(&kestrel, &stub).await;
    let declaration = document(&selecting(SSH, "main", "github"));
    has_branch(&stub, "main");

    let (status, preview) =
        posted(&kestrel, &of(operator::DECLARATION_PREVIEW), &declaration).await;
    assert_eq!(status, StatusCode::OK, "{preview}");
    let differences = preview["declarations"][0]["differences"]
        .as_array()
        .expect("the project's differences");
    assert!(
        differences.contains(&json!({
            "field": "repositories", "was": null, "becomes": HTTPS,
        })),
        "{preview}"
    );
    assert!(
        differences.contains(&json!({
            "field": "integrations", "was": null, "becomes": format!("{HTTPS} as github"),
        })),
        "{preview}"
    );
    nothing_declared(&kestrel).await;

    let (status, refusal) = posted(&kestrel, &of(operator::DECLARATION), &declaration).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{refusal}");
    assert_eq!(refusal["context"]["field"], "project.branch");
    assert_eq!(refusal["context"]["constraint"], "branch_in_repository");
    nothing_declared(&kestrel).await;

    has_branch(&stub, "main");
    let (status, applied) = posted(&kestrel, &of(operator::DECLARATION), &declaration).await;
    assert_eq!(status, StatusCode::OK, "{applied}");
    assert_eq!(
        projects(&kestrel).await[0]["integrations"][0]["integration"],
        "github"
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_start_whose_selection_is_refused_writes_nothing_and_one_that_passes_fixes_it() {
    let stub = GithubStub::start();
    let kestrel = Kestrel::boot().await;
    connected(&kestrel, &stub).await;
    kestrel.name_operator("ada").await;
    let plan = json!({
        "organization": "acme",
        "project": "web",
        "repositories": [SSH],
        "integrations": [{ "repository": SSH, "integration": "github" }],
        "branch": "main",
        "agent": "builder",
        "harness": "opencode",
        "brief": "Fix the flaky test",
        "credentials": [{ "variable": "ANTHROPIC_API_KEY", "secret": "sk-ant-api03-key" }],
    });
    stub.script_answer("GET", NAMED, ScriptedResponse::answering(403));

    let (status, refusal) = posted(&kestrel, operator::STARTS, &plan).await;
    assert_eq!(status, StatusCode::CONFLICT, "{refusal}");
    assert_eq!(refusal["context"]["state"], "repository_unreachable");
    nothing_declared(&kestrel).await;

    has_branch(&stub, "main");
    let (status, started) = posted(&kestrel, operator::STARTS, &plan).await;
    assert_eq!(status, StatusCode::CREATED, "{started}");
    assert_eq!(
        started["workspace"]["checkout"]["repositories"],
        json!([HTTPS])
    );
    assert_eq!(
        started["workspace"]["integrations"][0]["integration"],
        "github"
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_workspace_keeps_the_integration_it_opened_with_through_rebinding_and_retirement() {
    let stub = GithubStub::start();
    let kestrel = Kestrel::boot().await;
    connected(&kestrel, &stub).await;
    connect(&kestrel, &stub, "replacement", REPOSITORY).await;
    let (status, agent) = posted(
        &kestrel,
        &of(operator::AGENTS),
        &json!({ "name": "builder", "harness": "opencode" }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{agent}");
    has_branch(&stub, "main");
    declare(&kestrel, &selecting(REPOSITORY, "main", "github")).await;
    let opened = kestrel.open_workspace("acme", "web", "builder").await;
    let workspace = of(operator::WORKSPACE).replace("{workspace}", &opened.id.to_string());

    has_branch(&stub, "main");
    let (status, rebound) = declare(&kestrel, &selecting(REPOSITORY, "main", "replacement")).await;
    assert_eq!(status, StatusCode::OK, "{rebound}");
    assert_eq!(rebound["integrations"][0]["integration"], "replacement");
    let retired = kestrel.retire_integration("acme", "github").await;

    let (status, read) = got(&kestrel, &workspace).await;
    assert_eq!(status, StatusCode::OK, "{read}");
    assert_eq!(
        read["integrations"],
        json!([{
            "repository": HTTPS,
            "integration": "github",
            "integration_id": retired.id.to_string(),
        }])
    );
    let later = kestrel.open_workspace("acme", "web", "builder").await;
    let (_, later) = got(
        &kestrel,
        &of(operator::WORKSPACE).replace("{workspace}", &later.id.to_string()),
    )
    .await;
    assert_eq!(later["integrations"][0]["integration"], "replacement");

    let (status, work) = got(
        &kestrel,
        &of(operator::WORKSPACE_WORK).replace("{workspace}", &opened.id.to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{work}");
    assert_eq!(work["state"], "no_instance");
    assert_eq!(work["last_report"]["report"], "none");

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_resolution_shows_a_selected_ssh_address_as_https_and_leaves_an_unbound_one_alone() {
    let stub = GithubStub::start();
    let kestrel = Kestrel::boot().await;
    connected(&kestrel, &stub).await;
    let before = asked(&stub);

    let (status, resolution) = posted(
        &kestrel,
        operator::REPOSITORY_RESOLUTIONS,
        &json!({
            "organization": "acme",
            "repositories": [SSH, "git@github.com:jtmthf/tools.git"],
            "integrations": [{ "repository": SSH, "integration": "github" }],
        }),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{resolution}");
    assert_eq!(
        resolution["repositories"],
        json!([
            {
                "given": SSH,
                "address": HTTPS,
                "checkout_directory": "kestrel",
                "integration": "github",
            },
            {
                "given": "git@github.com:jtmthf/tools.git",
                "address": "git@github.com:jtmthf/tools.git",
                "checkout_directory": "tools",
                "integration": null,
            },
        ])
    );
    assert_eq!(asked(&stub), before);

    let (status, refusal) = posted(
        &kestrel,
        operator::REPOSITORY_RESOLUTIONS,
        &json!({
            "organization": "acme",
            "repositories": [REPOSITORY],
            "integrations": [{ "repository": REPOSITORY, "integration": "github" }],
            "purpose": "public_setup",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{refusal}");
    assert_eq!(refusal["context"]["constraint"], "declaration_purpose");

    kestrel.teardown().await;
}

fn installed(total: usize, repositories: &[(&str, &str, bool)]) -> ScriptedResponse {
    ScriptedResponse::ok(
        json!({
            "total_count": total,
            "repositories": repositories
                .iter()
                .enumerate()
                .map(|(id, (full_name, default_branch, private))| json!({
                    "id": id,
                    "full_name": full_name,
                    "default_branch": default_branch,
                    "private": private,
                }))
                .collect::<Vec<_>>(),
        })
        .to_string(),
    )
}

#[tokio::test]
async fn a_selected_integration_resolves_supported_github_ssh_addresses() {
    let stub = GithubStub::start();
    let kestrel = Kestrel::boot().await;
    connected(&kestrel, &stub).await;
    let before = asked(&stub);
    for address in [
        "ssh://git@github.com:22/jtmthf/kestrel.git",
        "ssh://github.com/jtmthf/kestrel.git",
        "github.com:jtmthf/kestrel.git",
    ] {
        let (status, resolution) = posted(
            &kestrel,
            operator::REPOSITORY_RESOLUTIONS,
            &json!({
                "organization": "acme",
                "repositories": [address],
                "integrations": [{ "repository": address, "integration": "github" }],
            }),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{address}: {resolution}");
        assert_eq!(resolution["repositories"][0]["address"], HTTPS);
        assert_eq!(resolution["repositories"][0]["integration"], "github");
    }
    assert_eq!(asked(&stub), before);
    kestrel.teardown().await;
}

#[tokio::test]
async fn an_integrations_repositories_are_read_from_github_a_page_at_a_time() {
    const LISTED: &str = "/installation/repositories";
    let stub = GithubStub::start();
    let kestrel = Kestrel::boot().await;
    connected(&kestrel, &stub).await;
    let repositories = of(operator::INTEGRATION_REPOSITORIES).replace("{integration}", "github");
    stub.script_answer(
        "GET",
        LISTED,
        installed(
            3,
            &[
                ("jtmthf/kestrel", "trunk", true),
                ("jtmthf/other", "main", false),
            ],
        ),
    );

    let (status, page) = got(&kestrel, &format!("{repositories}?per_page=2")).await;

    assert_eq!(status, StatusCode::OK, "{page}");
    assert_eq!(
        page,
        json!({
            "repositories": [
                {
                    "repository": "jtmthf/kestrel",
                    "address": HTTPS,
                    "default_branch": "trunk",
                    "private": true,
                    "watched": true,
                },
                {
                    "repository": "jtmthf/other",
                    "address": "https://github.com/jtmthf/other.git",
                    "default_branch": "main",
                    "private": false,
                    "watched": false,
                },
            ],
            "total": 3,
            "page": 1,
            "next_page": 2,
        })
    );
    assert!(
        stub.requests()
            .iter()
            .any(|request| request.url == format!("{LISTED}?per_page=2&page=1")),
        "the page asked for was not the page read"
    );

    stub.script_answer("GET", LISTED, installed(0, &[]));
    let (status, empty) = got(&kestrel, &repositories).await;
    assert_eq!(status, StatusCode::OK, "{empty}");
    assert_eq!(empty["repositories"], json!([]));
    assert_eq!(empty["next_page"], Value::Null);

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_repository_read_github_does_not_answer_is_a_refusal_never_an_empty_list_or_a_guess() {
    const LISTED: &str = "/installation/repositories";
    let stub = GithubStub::start();
    let kestrel = Kestrel::boot().await;
    connected(&kestrel, &stub).await;
    let repositories = of(operator::INTEGRATION_REPOSITORIES).replace("{integration}", "github");

    stub.script_answer("GET", LISTED, github_stub::rate_limited());
    let (status, refusal) = got(&kestrel, &repositories).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{refusal}");
    assert_eq!(refusal["context"]["service"], "github");
    assert_eq!(refusal["next_steps"][0]["action"], "retry_read");

    stub.script_answer(
        "GET",
        LISTED,
        ScriptedResponse::ok(
            json!({
                "total_count": 1,
                "repositories": [{ "id": 1, "full_name": "jtmthf/kestrel", "private": false }],
            })
            .to_string(),
        ),
    );
    let (status, refusal) = got(&kestrel, &repositories).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{refusal}");

    stub.script_answer("GET", LISTED, ScriptedResponse::answering(404));
    let (status, refusal) = got(&kestrel, &repositories).await;
    assert_eq!(status, StatusCode::CONFLICT, "{refusal}");
    assert_eq!(refusal["context"]["state"], "installation_unreachable");

    let (status, refusal) = got(&kestrel, &format!("{repositories}?page=0")).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{refusal}");
    assert_eq!(refusal["context"]["field"], "page");

    kestrel.disable_integration("acme", "github").await;
    let (status, refusal) = got(&kestrel, &repositories).await;
    assert_eq!(status, StatusCode::CONFLICT, "{refusal}");
    assert_eq!(refusal["context"]["state"], "disabled");

    kestrel.teardown().await;
}

#[cfg(unix)]
#[tokio::test]
async fn a_successful_app_check_does_not_hide_an_instances_checkout_failure() {
    let stub = GithubStub::start();
    let environment = support::environment::Environment::executing(&format!(
        "export GIT_CONFIG_COUNT=1\n\
         export GIT_CONFIG_KEY_0='url.{}.insteadOf'\n\
         export GIT_CONFIG_VALUE_0='{HTTPS}'\n\
         exec '{}'",
        support::repository::url(),
        support::supervisor::binary().display(),
    ));
    let kestrel = Kestrel::dispatching(environment.path()).await;
    connected(&kestrel, &stub).await;
    let (status, credential) = requested(
        &kestrel,
        reqwest::Method::PUT,
        &of(operator::CREDENTIAL).replace("{variable}", support::PROVIDER_KEY),
        Some(&json!({ "secret": support::A_PROVIDER_KEY })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{credential}");
    let branch = "a-branch-nobody-cut";
    has_branch(&stub, branch);
    let (status, saved) = declare(&kestrel, &selecting(REPOSITORY, branch, "github")).await;
    assert_eq!(status, StatusCode::CREATED, "{saved}");
    let (status, agent) = posted(
        &kestrel,
        &of(operator::AGENTS),
        &json!({
            "name": "builder", "harness": "opencode",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{agent}");
    let (status, opened) = posted(
        &kestrel,
        &of(operator::WORKSPACES),
        &json!({
            "project": "web", "agent": "builder", "brief": "go",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{opened}");
    let session = opened["session"]["id"].as_str().expect("a session");
    let path = of(operator::SESSION).replace("{session}", session);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    let ended = loop {
        let (status, read) = got(&kestrel, &path).await;
        assert_eq!(status, StatusCode::OK, "{read}");
        if read["state"] == "ended" {
            break read;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "checkout never ended: {read}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    };
    assert_eq!(ended["exit"]["status"], "failed", "{ended}");
    let failure = ended["exit"]["because"].as_str().expect("a failure");
    assert!(
        failure.contains(HTTPS) && failure.contains("could not be checked out"),
        "{failure}"
    );
    assert!(failure.contains(branch), "{failure}");
    assert_eq!(ended["started_at"], Value::Null);
    assert_eq!(projects(&kestrel).await, json!([saved]));
    kestrel.teardown().await;
}
