//! Declaring Organizations, Projects, Agents and Triggers over the operator boundary, and
//! previewing or applying one Declaration document whole.

use crate::support;

use std::time::Duration;

use kestrel::operator;
use reqwest::StatusCode;
use serde_json::{Value, json};
use support::client;
use support::github_stub::{self, GithubStub};
use support::operator_requests::{
    agent_model_of, agents_of, client, declaration_of, declaration_preview_of, declared,
    listed_nothing, recorded, requested, succeeded,
};
use support::{Kestrel, PRIVATE_KEY};

const DECLARATION: &str = r#"
project:
  name: kestrel
  repositories:
    - https://github.com/jtmthf/kestrel
  branch: main
agent:
  name: builder
  harness: opencode
trigger:
  name: ready
  filter:
    exact:
      type: com.github.issues.labeled
  brief: Work on {{ event.data.issue.title }}
  project: kestrel
  agent: builder
"#;

#[tokio::test]
async fn a_client_declares_and_lists_organizations_without_opening_a_database() {
    let kestrel = Kestrel::boot().await;

    let declared = recorded(
        &client(
            &kestrel,
            &[
                "organization",
                "declare",
                "acme",
                "--max-live-instances",
                "3",
                "--json",
            ],
        )
        .await,
    );
    succeeded(&client(&kestrel, &["organization", "declare", "globex"]).await);
    let listed = recorded(&client(&kestrel, &["organization", "list", "--json"]).await);

    assert_eq!(declared.len(), 1);
    assert_eq!(declared[0]["name"], "acme");
    assert_eq!(declared[0]["max_live_instances"], 3);
    assert_eq!(
        listed
            .iter()
            .map(|organization| organization["name"].as_str().expect("a name"))
            .collect::<Vec<_>>(),
        vec!["acme", "globex"]
    );
    assert_eq!(listed[0]["id"], declared[0]["id"]);
    assert_eq!(
        kestrel.organizations().await[0].id.to_string(),
        declared[0]["id"].as_str().expect("an id")
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn an_organizations_cap_is_a_positive_count_and_null_or_absent_is_unbounded() {
    let kestrel = Kestrel::boot().await;

    for cap in [json!(0), json!(-1)] {
        let (status, refusal) = declared(
            &kestrel,
            operator::ORGANIZATIONS,
            &json!({"name": "acme", "max_live_instances": cap}),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{refusal}");
        assert_eq!(refusal["kind"], "malformed_request", "{refusal}");
    }
    assert!(listed_nothing(&kestrel, operator::ORGANIZATIONS).await);

    let (created, capped) = declared(
        &kestrel,
        operator::ORGANIZATIONS,
        &json!({"name": "acme", "max_live_instances": 2}),
    )
    .await;
    assert_eq!(created, StatusCode::CREATED, "{capped}");
    assert_eq!(capped["max_live_instances"], 2);
    let (changed, absent) =
        declared(&kestrel, operator::ORGANIZATIONS, &json!({"name": "acme"})).await;
    assert_eq!(changed, StatusCode::OK, "{absent}");
    assert_eq!(absent["id"], capped["id"]);
    assert_eq!(absent["max_live_instances"], Value::Null);
    let (_, null) = declared(
        &kestrel,
        operator::ORGANIZATIONS,
        &json!({"name": "globex", "max_live_instances": null}),
    )
    .await;
    assert_eq!(null["max_live_instances"], Value::Null);

    kestrel.teardown().await;
}

#[tokio::test]
async fn an_agents_defaults_are_null_whether_a_declaration_omits_them_or_names_none() {
    let kestrel = Kestrel::boot().await;
    declared(&kestrel, operator::ORGANIZATIONS, &json!({"name": "acme"})).await;

    let (_, omitted) = declared(
        &kestrel,
        &agents_of("acme"),
        &json!({"name": "builder", "harness": "opencode"}),
    )
    .await;
    let (_, null) = declared(
        &kestrel,
        &agents_of("acme"),
        &json!({
            "name": "reviewer", "harness": "opencode",
            "model": null, "mode": null, "thought_level": null,
        }),
    )
    .await;
    for agent in [&omitted, &null] {
        for default in ["model", "mode", "thought_level"] {
            assert_eq!(agent[default], Value::Null, "{agent}");
        }
    }

    let model = agent_model_of("acme", "builder");
    let put =
        async |body: Value| requested(&kestrel, reqwest::Method::PUT, &model, Some(&body)).await;
    let (status, set) = put(json!({"model": "claude-opus-5"})).await;
    assert_eq!(status, StatusCode::OK, "{set}");
    assert_eq!(set["model"], "claude-opus-5");
    assert_eq!(set["id"], omitted["id"]);
    assert_eq!(put(json!({"model": null})).await.1["model"], Value::Null);
    put(json!({"model": "claude-opus-5"})).await;
    assert_eq!(put(json!({})).await.1["model"], Value::Null);

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_client_declares_and_lists_projects_and_agents() {
    let kestrel = Kestrel::boot().await;
    succeeded(&client(&kestrel, &["organization", "declare", "acme"]).await);

    let project = recorded(
        &client(
            &kestrel,
            &[
                "project",
                "declare",
                "kestrel",
                "--organization",
                "acme",
                "--repository",
                "https://github.com/jtmthf/kestrel",
                "--repository",
                "https://github.com/jtmthf/skills",
                "--branch",
                "main",
                "--json",
            ],
        )
        .await,
    );
    let agent = recorded(
        &client(
            &kestrel,
            &[
                "agent",
                "declare",
                "builder",
                "--organization",
                "acme",
                "--model",
                "claude-opus-5",
                "--json",
            ],
        )
        .await,
    );
    let projects = recorded(
        &client(
            &kestrel,
            &["project", "list", "--organization", "acme", "--json"],
        )
        .await,
    );
    let agents = recorded(
        &client(
            &kestrel,
            &["agent", "list", "--organization", "acme", "--json"],
        )
        .await,
    );

    assert_eq!(projects, project);
    assert_eq!(
        projects[0]["repositories"],
        json!([
            "https://github.com/jtmthf/kestrel",
            "https://github.com/jtmthf/skills"
        ])
    );
    assert_eq!(projects[0]["branch"], "main");
    assert_eq!(agents, agent);
    assert_eq!(agents[0]["harness"], "opencode");
    assert_eq!(agents[0]["model"], "claude-opus-5");

    let opened = kestrel.open_workspace("acme", "kestrel", "builder").await;
    assert_eq!(
        opened.project.id.to_string(),
        project[0]["id"].as_str().expect("an id")
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_client_applies_one_project_agent_and_trigger_declaration() {
    let kestrel = Kestrel::boot().await;
    succeeded(&client(&kestrel, &["organization", "declare", "acme"]).await);

    let first = client::ran_by(
        &kestrel,
        &["apply", "--organization", "acme", "-f", "kestrel.yaml"],
        client::Invocation::default().file("kestrel.yaml", DECLARATION),
    )
    .await;

    assert!(
        first
            .err
            .contains("the trigger ready fires for events from people outside"),
        "{}",
        first.err
    );
    let first = succeeded(&first);
    assert!(first.contains(&"+ project kestrel".to_owned()), "{first:?}");
    assert!(first.contains(&"+ agent builder".to_owned()), "{first:?}");
    assert!(first.contains(&"+ trigger ready".to_owned()), "{first:?}");
    assert!(first.contains(&"    branch".to_owned()), "{first:?}");
    let project = recorded(
        &client(
            &kestrel,
            &["project", "list", "--organization", "acme", "--json"],
        )
        .await,
    );
    let agent = recorded(
        &client(
            &kestrel,
            &["agent", "list", "--organization", "acme", "--json"],
        )
        .await,
    );
    let trigger = recorded(
        &client(
            &kestrel,
            &["trigger", "list", "--organization", "acme", "--json"],
        )
        .await,
    );

    let again = client::ran_by(
        &kestrel,
        &["apply", "--organization", "acme", "-f", "kestrel.yaml"],
        client::Invocation::default().file("kestrel.yaml", DECLARATION),
    )
    .await;

    assert_eq!(
        succeeded(&again),
        ["= project kestrel", "= agent builder", "= trigger ready"]
    );
    assert_eq!(
        recorded(
            &client(
                &kestrel,
                &["project", "list", "--organization", "acme", "--json",],
            )
            .await,
        ),
        project
    );
    assert_eq!(
        recorded(
            &client(
                &kestrel,
                &["agent", "list", "--organization", "acme", "--json",],
            )
            .await,
        ),
        agent
    );
    assert_eq!(
        recorded(
            &client(
                &kestrel,
                &["trigger", "list", "--organization", "acme", "--json",],
            )
            .await,
        ),
        trigger
    );

    let changed = client::ran_by(
        &kestrel,
        &["apply", "--organization", "acme", "-f", "kestrel.yaml"],
        client::Invocation::default().file(
            "kestrel.yaml",
            &DECLARATION.replace("branch: main", "branch: next"),
        ),
    )
    .await;

    let changed = succeeded(&changed);
    assert!(
        changed.contains(&"~ project kestrel".to_owned()),
        "{changed:?}"
    );
    assert!(changed.contains(&"      - main".to_owned()), "{changed:?}");
    assert!(changed.contains(&"      + next".to_owned()), "{changed:?}");

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_declaration_preview_says_what_a_firing_does_to_an_open_workspace() {
    let kestrel = Kestrel::boot().await;
    succeeded(&client(&kestrel, &["organization", "declare", "acme"]).await);
    let declaration = json!({
        "project": {
            "name": "kestrel",
            "repositories": ["https://github.com/jtmthf/kestrel"],
            "branch": "main",
        },
        "agent": { "name": "fixer", "harness": "opencode" },
        "trigger": {
            "name": "ci-failed",
            "filter": { "exact": { "type": "com.github.issues.labeled" } },
            "brief": "Fix the build of {{ event.data.issue.title }}",
            "correlation": "{{ event.source }}{{ event.subject }}",
            "on_miss": "ignore",
            "on_open_workspace": "new-session",
            "project": "kestrel",
            "agent": "fixer",
        },
    });

    let (status, preview) = declared(&kestrel, &declaration_preview_of("acme"), &declaration).await;

    assert_eq!(status, StatusCode::OK, "{preview}");
    assert!(
        preview["declarations"][2]["differences"]
            .as_array()
            .expect("the trigger's differences")
            .contains(&json!({
                "field": "on open workspace",
                "was": null,
                "becomes": "new-session",
            })),
        "{preview}"
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_declaration_preview_changes_nothing() {
    let kestrel = Kestrel::boot().await;
    succeeded(&client(&kestrel, &["organization", "declare", "acme"]).await);
    let declaration = json!({
        "project": {
            "name": "kestrel",
            "repositories": ["https://github.com/jtmthf/kestrel"],
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

    let (status, preview) = declared(&kestrel, &declaration_preview_of("acme"), &declaration).await;

    assert_eq!(status, StatusCode::OK, "{preview}");
    assert_eq!(preview["declarations"][0]["action"], "add");
    assert!(
        recorded(
            &client(
                &kestrel,
                &["project", "list", "--organization", "acme", "--json",],
            )
            .await,
        )
        .is_empty()
    );

    let (status, applied) = declared(&kestrel, &declaration_of("acme"), &declaration).await;

    assert_eq!(status, StatusCode::OK, "{applied}");
    assert_eq!(applied["declarations"][0]["action"], "add");

    kestrel.teardown().await;
}

#[tokio::test]
async fn apply_under_json_answers_what_it_applied_whole() {
    let kestrel = Kestrel::boot().await;
    succeeded(&client(&kestrel, &["organization", "declare", "acme"]).await);

    let applied = client::ran_by(
        &kestrel,
        &[
            "apply",
            "--organization",
            "acme",
            "-f",
            "kestrel.yaml",
            "--json",
        ],
        client::Invocation::default().file("kestrel.yaml", DECLARATION),
    )
    .await;
    let triggers = client::ran_by(
        &kestrel,
        &[
            "trigger",
            "apply",
            "--organization",
            "acme",
            "-f",
            "triggers.yaml",
            "--dry-run",
            "--json",
        ],
        client::Invocation::default().file(
            "triggers.yaml",
            "triggers:\n  nightly:\n    filter: {exact: {type: com.example.tidy}}\n    brief: Tidy up\n    project: kestrel\n    agent: builder\n",
        ),
    )
    .await;

    assert!(
        applied
            .err
            .contains("the trigger ready fires for events from people outside"),
        "{}",
        applied.err
    );
    let applied = applied.json();
    assert_eq!(
        applied["declarations"].as_array().map(Vec::len),
        Some(3),
        "{applied}"
    );
    assert!(
        applied["declarations"]
            .as_array()
            .into_iter()
            .flatten()
            .all(|declared| declared["action"] == "add"),
        "{applied}"
    );
    assert_eq!(
        applied["admitting_outsiders"],
        json!(["ready"]),
        "{applied}"
    );
    let triggers = triggers.json();
    assert_eq!(triggers["changes"][0]["name"], "nightly", "{triggers}");
    assert_eq!(triggers["changes"][0]["action"], "add", "{triggers}");
    assert!(
        triggers["changes"][0]["differences"].is_array(),
        "{triggers}"
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_trigger_applied_after_an_event_never_fires_for_that_event() {
    let stub = GithubStub::start();
    stub.deliver(github_stub::labelled(43, "ready-for-agent"));
    let kestrel = Kestrel::boot().await;
    succeeded(&client(&kestrel, &["organization", "declare", "acme"]).await);
    succeeded(
        &client(
            &kestrel,
            &[
                "integration",
                "github",
                "register",
                "origin",
                "--organization",
                "acme",
                "--repository",
                "jtmthf/kestrel",
                "--app-id",
                "1",
                "--installation",
                "2",
                "--private-key",
                PRIVATE_KEY,
                "--api",
                &stub.base_url(),
                "--interval",
                "1ms",
            ],
        )
        .await,
    );

    let mut events = Vec::new();
    for _ in 0..100 {
        events = recorded(
            &client(
                &kestrel,
                &["event", "list", "--organization", "acme", "--json"],
            )
            .await,
        );
        if !events.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(
        !events.is_empty(),
        "the integration never recorded its event"
    );

    succeeded(
        &client::ran_by(
            &kestrel,
            &["apply", "--organization", "acme", "-f", "kestrel.yaml"],
            client::Invocation::default().file("kestrel.yaml", DECLARATION),
        )
        .await,
    );
    tokio::time::sleep(Duration::from_secs(2)).await;

    assert!(
        recorded(
            &client(
                &kestrel,
                &["workspace", "list", "--organization", "acme", "--json",],
            )
            .await,
        )
        .is_empty(),
        "a trigger fired for an event recorded before it was applied"
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn an_inconsistent_declaration_changes_nothing() {
    let kestrel = Kestrel::boot().await;
    succeeded(&client(&kestrel, &["organization", "declare", "acme"]).await);
    let inconsistent = DECLARATION.replace(
        "project: kestrel\n  agent: builder",
        "project: elsewhere\n  agent: builder",
    );

    let refused = client::ran_by(
        &kestrel,
        &["apply", "--organization", "acme", "-f", "kestrel.yaml"],
        client::Invocation::default().file("kestrel.yaml", &inconsistent),
    )
    .await;

    assert!(!refused.status.success(), "{}", refused.err);
    assert!(
        refused.err.contains("not the declared project"),
        "{}",
        refused.err
    );
    assert!(
        recorded(
            &client(
                &kestrel,
                &["project", "list", "--organization", "acme", "--json",],
            )
            .await,
        )
        .is_empty()
    );
    assert!(
        recorded(
            &client(
                &kestrel,
                &["agent", "list", "--organization", "acme", "--json",],
            )
            .await,
        )
        .is_empty()
    );
    assert!(
        recorded(
            &client(
                &kestrel,
                &["trigger", "list", "--organization", "acme", "--json",],
            )
            .await,
        )
        .is_empty()
    );

    kestrel.teardown().await;
}
