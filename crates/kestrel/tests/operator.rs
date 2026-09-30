mod support;

use std::collections::{HashMap, HashSet};
use std::fs;
use std::time::Duration;

use kestrel::domain::{EventRecordId, Exit, SessionId, WorkspaceId};
use kestrel::instance::{Git, Observed};
use kestrel::link;
use kestrel::log::{Entry, Message};
use kestrel::operator;
use kestrel::work;
use reqwest::StatusCode;
use serde_json::{Value, json};
use support::client::{self, Client};
use support::github_stub::{self, GithubStub};
use support::supervisor;
use support::{Kestrel, SERIALIZED, TOKEN};

async fn an_open_workspace(kestrel: &Kestrel, said: usize) -> (String, kestrel::domain::Session) {
    let organization = kestrel.declare_organization("acme").await;
    kestrel
        .declare_project(
            &organization,
            "kestrel",
            &["https://github.com/jtmthf/kestrel".to_owned()],
            "main",
        )
        .await;
    kestrel
        .declare_agent(&organization, "builder", "opencode", None)
        .await;
    let workspace = kestrel.open_workspace("acme", "kestrel", "builder").await;
    let session = kestrel.dispatch_session(workspace.id).await;
    for message in 1..=said {
        kestrel.said(&session, &format!("message {message}")).await;
    }

    (workspace.id.to_string(), session)
}

async fn recorded_seqs(kestrel: &Kestrel, workspace: &str) -> Vec<i64> {
    kestrel
        .transcript(workspace.parse().expect("a workspace id"))
        .await
        .iter()
        .map(|entry| entry.seq)
        .collect()
}

fn seqs(lines: &[String]) -> Vec<i64> {
    lines
        .iter()
        .map(|line| {
            let entry: Value = serde_json::from_str(line)
                .unwrap_or_else(|error| panic!("{line} is not an entry: {error}"));
            entry["seq"].as_i64().expect("a seq")
        })
        .collect()
}

fn records(lines: &[String]) -> Vec<Value> {
    lines
        .iter()
        .map(|line| {
            serde_json::from_str(line)
                .unwrap_or_else(|error| panic!("{line} is not a record: {error}"))
        })
        .collect()
}

fn generated_name(record: &Value) -> &str {
    let name = record["name"].as_str().expect("a generated name");
    let mut words = name.split('-');
    let adjective = words.next().expect("an adjective");
    let noun = words.next().expect("a noun");
    let suffix = words.next().expect("a generated suffix");
    assert!(!adjective.is_empty(), "a name has an adjective");
    assert!(!noun.is_empty(), "a name has a noun");
    assert_eq!(suffix.len(), 8, "a name has an eight-letter suffix");
    assert!(words.next().is_none(), "a name has no extra words");
    name
}

/// The shortest prefix of `id` that names no other identifier, so an unambiguous reference
/// is unambiguous whatever the generated identifiers turn out to be.
fn shortest_prefix_of(id: &str, others: &[&str]) -> String {
    let mut length = 1;
    while others.iter().any(|other| other.starts_with(&id[..length])) {
        length += 1;
    }

    id[..length].to_owned()
}

async fn client(kestrel: &Kestrel, args: &[&str]) -> client::Finished {
    client_given(kestrel, args, None).await
}

async fn client_given(kestrel: &Kestrel, args: &[&str], input: Option<&str>) -> client::Finished {
    let invocation = input.map_or_else(client::Invocation::default, |input| {
        client::Invocation::default().given(input)
    });
    client::ran_by(kestrel, args, invocation).await
}

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

fn succeeded(finished: &client::Finished) -> &[String] {
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

fn recorded(finished: &client::Finished) -> Vec<Value> {
    records(succeeded(finished))
}

/// What each test reads, named the way a script names it: a field the boundary gains later
/// reaches none of these assertions.
const ORGANIZATION: &str = "id,name,max_live_instances";
const PROJECT: &str = "id,name,repositories,branch";
const AGENT: &str = "id,name,harness,model";
const CREDENTIAL: &str = "variable";
const INTEGRATION: &str = "id,kind,repository,carries,polled_every,webhook_path,last_event_refusal";
const EVENT: &str = "record,integration,event";
const TRIGGER: &str = "id,name,state,brief";
const WORKSPACE: &str = "id,name,state,continues";
const SESSION: &str = "id,name,workspace,state,model";
const ENTRY: &str = "seq,entry";

/// Every answer is checked against what the published document says the operation answers.
async fn requested(
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

async fn declared(kestrel: &Kestrel, path: &str, declaration: &Value) -> (StatusCode, Value) {
    requested(kestrel, reqwest::Method::POST, path, Some(declaration)).await
}

async fn got(kestrel: &Kestrel, path: &str) -> (StatusCode, Value) {
    requested(kestrel, reqwest::Method::GET, path, None).await
}

async fn listed(kestrel: &Kestrel, path: &str) -> Vec<Value> {
    let (status, body) = got(kestrel, path).await;

    assert_eq!(status, StatusCode::OK, "{body}");
    body.as_array().expect("an array of records").clone()
}

async fn listed_nothing(kestrel: &Kestrel, path: &str) -> bool {
    listed(kestrel, path).await.is_empty()
}

fn projects_of(organization: &str) -> String {
    operator::PROJECTS.replace("{organization}", organization)
}

fn agents_of(organization: &str) -> String {
    operator::AGENTS.replace("{organization}", organization)
}

fn agent_model_of(organization: &str, agent: &str) -> String {
    operator::AGENT_MODEL
        .replace("{organization}", organization)
        .replace("{agent}", agent)
}

fn declaration_of(organization: &str) -> String {
    operator::DECLARATION.replace("{organization}", organization)
}

fn declaration_preview_of(organization: &str) -> String {
    operator::DECLARATION_PREVIEW.replace("{organization}", organization)
}

fn credentials_of(organization: &str) -> String {
    operator::CREDENTIALS.replace("{organization}", organization)
}

fn credential_of(organization: &str, variable: &str) -> String {
    operator::CREDENTIAL
        .replace("{organization}", organization)
        .replace("{variable}", variable)
}

fn profiles_of(organization: &str) -> String {
    operator::PROFILES.replace("{organization}", organization)
}

fn profile_variable_of(organization: &str, profile: &str, variable: &str) -> String {
    operator::PROFILE_VARIABLE
        .replace("{organization}", organization)
        .replace("{profile}", profile)
        .replace("{variable}", variable)
}

fn integrations_of(organization: &str) -> String {
    operator::INTEGRATIONS.replace("{organization}", organization)
}

fn event_refusal_of(organization: &str, integration: &str) -> String {
    operator::EVENT_REFUSAL
        .replace("{organization}", organization)
        .replace("{integration}", integration)
}

fn events_of(organization: &str) -> String {
    operator::EVENTS.replace("{organization}", organization)
}

fn event_at(record: &str) -> String {
    operator::EVENT.replace("{record}", record)
}

fn triggers_of(organization: &str) -> String {
    operator::TRIGGERS.replace("{organization}", organization)
}

fn trigger_at(organization: &str, trigger: &str) -> String {
    operator::TRIGGER
        .replace("{organization}", organization)
        .replace("{trigger}", trigger)
}

fn workspace_at(organization: &str, workspace: &str) -> String {
    operator::WORKSPACE
        .replace("{organization}", organization)
        .replace("{workspace}", workspace)
}

fn workspace_messages_at(organization: &str, workspace: &str) -> String {
    operator::WORKSPACE_MESSAGES
        .replace("{organization}", organization)
        .replace("{workspace}", workspace)
}

fn workspace_seal_at(organization: &str, workspace: &str) -> String {
    operator::WORKSPACE_SEAL
        .replace("{organization}", organization)
        .replace("{workspace}", workspace)
}

fn sessions_of(organization: &str, workspace: &str) -> String {
    operator::SESSIONS
        .replace("{organization}", organization)
        .replace("{workspace}", workspace)
}

fn session_at(organization: &str, session: &str) -> String {
    operator::SESSION
        .replace("{organization}", organization)
        .replace("{session}", session)
}

fn transcript_of(organization: &str, workspace: &str) -> String {
    operator::TRANSCRIPT
        .replace("{organization}", organization)
        .replace("{workspace}", workspace)
}

fn failed(finished: &client::Finished) -> &str {
    assert!(
        !finished.status.success(),
        "the client was expected to be refused, and printed {:?}",
        finished.out
    );
    &finished.err
}

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
                ORGANIZATION,
            ],
        )
        .await,
    );
    succeeded(&client(&kestrel, &["organization", "declare", "globex"]).await);
    let listed =
        recorded(&client(&kestrel, &["organization", "list", "--json", ORGANIZATION]).await);

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
                PROJECT,
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
                AGENT,
            ],
        )
        .await,
    );
    let projects = recorded(
        &client(
            &kestrel,
            &[
                "project",
                "list",
                "--organization",
                "acme",
                "--json",
                PROJECT,
            ],
        )
        .await,
    );
    let agents = recorded(
        &client(
            &kestrel,
            &["agent", "list", "--organization", "acme", "--json", AGENT],
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
            &[
                "project",
                "list",
                "--organization",
                "acme",
                "--json",
                PROJECT,
            ],
        )
        .await,
    );
    let agent = recorded(
        &client(
            &kestrel,
            &["agent", "list", "--organization", "acme", "--json", AGENT],
        )
        .await,
    );
    let trigger = recorded(
        &client(
            &kestrel,
            &[
                "trigger",
                "list",
                "--organization",
                "acme",
                "--json",
                TRIGGER,
            ],
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
                &[
                    "project",
                    "list",
                    "--organization",
                    "acme",
                    "--json",
                    PROJECT,
                ],
            )
            .await,
        ),
        project
    );
    assert_eq!(
        recorded(
            &client(
                &kestrel,
                &["agent", "list", "--organization", "acme", "--json", AGENT,],
            )
            .await,
        ),
        agent
    );
    assert_eq!(
        recorded(
            &client(
                &kestrel,
                &[
                    "trigger",
                    "list",
                    "--organization",
                    "acme",
                    "--json",
                    TRIGGER,
                ],
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
                &[
                    "project",
                    "list",
                    "--organization",
                    "acme",
                    "--json",
                    PROJECT,
                ],
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
async fn a_trigger_applied_after_an_event_never_fires_for_that_event() {
    let stub = GithubStub::start();
    stub.script(github_stub::page(&[github_stub::labelled(
        7,
        43,
        "ready-for-agent",
    )]));
    let kestrel = Kestrel::boot().await;
    succeeded(&client(&kestrel, &["organization", "declare", "acme"]).await);
    succeeded(
        &client(
            &kestrel,
            &[
                "integration",
                "register",
                "github",
                "origin",
                "--organization",
                "acme",
                "--repository",
                "jtmthf/kestrel",
                "--token",
                TOKEN,
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
                &["event", "list", "--organization", "acme", "--json", EVENT],
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
                &[
                    "workspace",
                    "list",
                    "--organization",
                    "acme",
                    "--json",
                    WORKSPACE,
                ],
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
                &[
                    "project",
                    "list",
                    "--organization",
                    "acme",
                    "--json",
                    PROJECT,
                ],
            )
            .await,
        )
        .is_empty()
    );
    assert!(
        recorded(
            &client(
                &kestrel,
                &["agent", "list", "--organization", "acme", "--json", AGENT,],
            )
            .await,
        )
        .is_empty()
    );
    assert!(
        recorded(
            &client(
                &kestrel,
                &[
                    "trigger",
                    "list",
                    "--organization",
                    "acme",
                    "--json",
                    TRIGGER,
                ],
            )
            .await,
        )
        .is_empty()
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_client_operates_workspaces_and_sessions_without_opening_a_database() {
    let kestrel = Kestrel::boot().await;
    succeeded(&client(&kestrel, &["organization", "declare", "acme"]).await);
    succeeded(
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
                "--branch",
                "main",
            ],
        )
        .await,
    );
    succeeded(
        &client(
            &kestrel,
            &["agent", "declare", "builder", "--organization", "acme"],
        )
        .await,
    );

    let opened = recorded(
        &client(
            &kestrel,
            &[
                "workspace",
                "open",
                "--organization",
                "acme",
                "--project",
                "kestrel",
                "--agent",
                "builder",
                "--json",
                WORKSPACE,
            ],
        )
        .await,
    );
    let workspace = opened[0]["id"].as_str().expect("a workspace id").to_owned();
    let workspace_name = generated_name(&opened[0]).to_owned();

    let listed = recorded(
        &client(
            &kestrel,
            &[
                "workspace",
                "list",
                "--organization",
                "acme",
                "--json",
                WORKSPACE,
            ],
        )
        .await,
    );
    assert_eq!(listed, opened);
    let shown = recorded(
        &client(
            &kestrel,
            &["workspace", "show", &workspace, "--json", WORKSPACE],
        )
        .await,
    );
    assert_eq!(shown, opened);
    assert_eq!(shown[0]["name"], workspace_name);

    let posted = recorded(
        &client(
            &kestrel,
            &[
                "workspace",
                "post",
                &workspace,
                "--as-participant",
                "operator",
                "start with the operator boundary",
                "--json",
                SESSION,
            ],
        )
        .await,
    );
    let session = posted[0]["id"].as_str().expect("a session id");
    let first_session_name = generated_name(&posted[0]).to_owned();
    assert_eq!(posted[0]["workspace"], workspace);
    assert_eq!(posted[0]["state"], "queued");
    assert_eq!(
        recorded(
            &client(
                &kestrel,
                &[
                    "session",
                    "list",
                    "--workspace",
                    &workspace,
                    "--json",
                    SESSION
                ]
            )
            .await
        ),
        posted
    );

    let completed = kestrel
        .claim_session()
        .await
        .expect("the posted session should wait for the worker");
    assert_eq!(completed.id.to_string(), session);
    kestrel.complete_session(&completed).await;
    let sealed = recorded(
        &client(
            &kestrel,
            &["workspace", "seal", &workspace, "--json", WORKSPACE],
        )
        .await,
    );
    assert_eq!(sealed[0]["state"], "sealed");

    let continued = recorded(
        &client(
            &kestrel,
            &[
                "workspace",
                "open",
                "--organization",
                "acme",
                "--project",
                "kestrel",
                "--agent",
                "builder",
                "--continues",
                &workspace,
                "--json",
                WORKSPACE,
            ],
        )
        .await,
    );
    let continuing = continued[0]["id"].as_str().expect("a continuing workspace");
    assert_ne!(generated_name(&continued[0]), workspace_name);
    assert_eq!(continued[0]["continues"], workspace);
    let enqueued = recorded(
        &client(
            &kestrel,
            &[
                "session",
                "enqueue",
                "--workspace",
                continuing,
                "--model",
                "claude-opus-5",
                "--json",
                SESSION,
            ],
        )
        .await,
    );
    assert_ne!(generated_name(&enqueued[0]), first_session_name);
    assert_eq!(enqueued[0]["workspace"], continuing);
    assert_eq!(enqueued[0]["model"], "claude-opus-5");
    assert_eq!(
        recorded(
            &client(
                &kestrel,
                &[
                    "session",
                    "list",
                    "--workspace",
                    continuing,
                    "--json",
                    SESSION
                ]
            )
            .await
        ),
        enqueued
    );

    kestrel.teardown().await;
}

/// A review in the Workspace another Agent built in: a Session names its own Agent, and shows the
/// harness and model it runs on.
#[tokio::test]
async fn a_client_enqueues_a_session_naming_its_agent_and_shows_what_it_runs_on() {
    let kestrel = Kestrel::boot().await;
    let organization = kestrel.declare_organization("acme").await;
    kestrel
        .declare_project(
            &organization,
            "kestrel",
            &["https://github.com/jtmthf/kestrel".to_owned()],
            "main",
        )
        .await;
    kestrel
        .declare_agent(&organization, "builder", "opencode", None)
        .await;
    kestrel
        .declare_agent(&organization, "reviewer", "claude", Some("claude-opus-5"))
        .await;
    let workspace = kestrel
        .open_workspace("acme", "kestrel", "builder")
        .await
        .id
        .to_string();
    const RUNS_ON: &str = "id,agent,harness,model";

    let built = recorded(
        &client(
            &kestrel,
            &[
                "session",
                "enqueue",
                "--workspace",
                &workspace,
                "--json",
                RUNS_ON,
            ],
        )
        .await,
    );
    kestrel
        .complete_session(&kestrel.sessions(workspace.parse().expect("an id")).await[0])
        .await;
    let reviewed = recorded(
        &client(
            &kestrel,
            &[
                "session",
                "enqueue",
                "--workspace",
                &workspace,
                "--agent",
                "reviewer",
                "--json",
                RUNS_ON,
            ],
        )
        .await,
    );
    let review = reviewed[0]["id"].as_str().expect("a session id");
    let shown = recorded(
        &client(
            &kestrel,
            &[
                "session",
                "show",
                review,
                "--organization",
                "acme",
                "--json",
                RUNS_ON,
            ],
        )
        .await,
    );

    assert_eq!(built[0]["agent"], "builder");
    assert_eq!(built[0]["harness"], "opencode");
    assert_eq!(built[0]["model"], Value::Null);
    assert_eq!(shown, reviewed);
    assert_eq!(shown[0]["agent"], "reviewer");
    assert_eq!(shown[0]["harness"], "claude");
    assert_eq!(shown[0]["model"], "claude-opus-5");

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_client_naming_an_agent_the_organization_never_declared_enqueues_nothing() {
    let kestrel = Kestrel::boot().await;
    let organization = kestrel.declare_organization("acme").await;
    kestrel
        .declare_project(
            &organization,
            "kestrel",
            &["https://github.com/jtmthf/kestrel".to_owned()],
            "main",
        )
        .await;
    kestrel
        .declare_agent(&organization, "builder", "opencode", None)
        .await;
    let workspace = kestrel.open_workspace("acme", "kestrel", "builder").await;

    let refused = client(
        &kestrel,
        &[
            "session",
            "enqueue",
            "--workspace",
            &workspace.id.to_string(),
            "--agent",
            "reviewer",
        ],
    )
    .await;

    assert!(
        failed(&refused).contains("no agent named reviewer in the organization acme"),
        "{}",
        failed(&refused)
    );
    assert_eq!(refused.status.code(), Some(3));
    assert!(kestrel.sessions(workspace.id).await.is_empty());

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_client_names_a_workspace_by_name_identifier_prefix_and_latest() {
    let kestrel = Kestrel::boot().await;
    let organization = kestrel.declare_organization("acme").await;
    kestrel
        .declare_project(
            &organization,
            "kestrel",
            &["https://github.com/jtmthf/kestrel".to_owned()],
            "main",
        )
        .await;
    kestrel
        .declare_agent(&organization, "builder", "opencode", None)
        .await;

    let first = kestrel.open_workspace("acme", "kestrel", "builder").await;
    let second = kestrel.open_workspace("acme", "kestrel", "builder").await;
    let (first_id, second_id) = (first.id.to_string(), second.id.to_string());

    let by_name = recorded(
        &client(
            &kestrel,
            &["workspace", "show", &second.name, "--json", WORKSPACE],
        )
        .await,
    );
    assert_eq!(by_name[0]["id"], second_id);

    let by_id = recorded(
        &client(
            &kestrel,
            &["workspace", "show", &first_id, "--json", WORKSPACE],
        )
        .await,
    );
    assert_eq!(by_id[0]["name"], first.name);

    let prefix = shortest_prefix_of(&first_id, &[&second_id]);
    assert!(
        prefix.len() < first_id.len(),
        "two workspaces opened into the one identifier"
    );
    let by_prefix = recorded(
        &client(
            &kestrel,
            &["workspace", "show", &prefix, "--json", WORKSPACE],
        )
        .await,
    );
    assert_eq!(by_prefix[0]["id"], first_id);

    let latest = recorded(
        &client(
            &kestrel,
            &["workspace", "show", "latest", "--json", WORKSPACE],
        )
        .await,
    );
    assert_eq!(latest[0]["id"], second_id);

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_workspace_reference_matching_several_is_refused_naming_them() {
    let kestrel = Kestrel::boot().await;
    let organization = kestrel.declare_organization("acme").await;
    kestrel
        .declare_project(
            &organization,
            "kestrel",
            &["https://github.com/jtmthf/kestrel".to_owned()],
            "main",
        )
        .await;
    kestrel
        .declare_agent(&organization, "builder", "opencode", None)
        .await;
    for _ in 0..17 {
        kestrel.open_workspace("acme", "kestrel", "builder").await;
    }

    let listed = recorded(&client(&kestrel, &["workspace", "list", "--json", "id,name"]).await);
    let mut by_leading: HashMap<char, Vec<(String, String)>> = HashMap::new();
    for record in &listed {
        let id = record["id"].as_str().expect("an identifier").to_owned();
        let name = record["name"]
            .as_str()
            .expect("a generated name")
            .to_owned();
        by_leading
            .entry(id.chars().next().expect("an identifier"))
            .or_default()
            .push((id, name));
    }
    let (leading, matching) = by_leading
        .into_iter()
        .find(|(_, matching)| matching.len() > 1)
        .expect("seventeen identifiers over sixteen leading digits share one");

    let refused = client(&kestrel, &["workspace", "show", &leading.to_string()]).await;
    let said = failed(&refused);

    assert!(said.contains("ambiguous"), "{said}");
    assert_eq!(
        refused.status.code(),
        Some(3),
        "an ambiguous prefix is unresolved"
    );
    for (id, name) in &matching {
        assert!(said.contains(name), "{said} does not name {name}");
        assert!(said.contains(id), "{said} does not name {id}");
    }

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_workspace_reference_never_reaches_across_the_organizations_in_scope() {
    let kestrel = Kestrel::boot().await;
    for name in ["acme", "globex"] {
        let organization = kestrel.declare_organization(name).await;
        kestrel
            .declare_project(
                &organization,
                "kestrel",
                &["https://github.com/jtmthf/kestrel".to_owned()],
                "main",
            )
            .await;
        kestrel
            .declare_agent(&organization, "builder", "opencode", None)
            .await;
    }
    let acme = kestrel.open_workspace("acme", "kestrel", "builder").await;
    let globex = kestrel.open_workspace("globex", "kestrel", "builder").await;

    let refused = client(
        &kestrel,
        &[
            "workspace",
            "show",
            &acme.id.to_string(),
            "--organization",
            "globex",
        ],
    )
    .await;
    let said = failed(&refused);
    assert!(
        said.contains("no workspace in the organization globex matches"),
        "{said}"
    );

    let latest = recorded(
        &client(
            &kestrel,
            &[
                "workspace",
                "show",
                "latest",
                "--organization",
                "globex",
                "--json",
                WORKSPACE,
            ],
        )
        .await,
    );
    assert_eq!(latest[0]["id"], globex.id.to_string());

    let refused = client(
        &kestrel,
        &[
            "workspace",
            "show",
            "no-such-workspace",
            "--organization",
            "globex",
        ],
    )
    .await;
    let said = failed(&refused);
    assert!(
        said.contains("generated name") && said.contains("latest"),
        "the refusal is not corrective: {said}"
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_client_names_a_session_by_name_identifier_prefix_and_latest() {
    let kestrel = Kestrel::boot().await;
    let organization = kestrel.declare_organization("acme").await;
    kestrel
        .declare_project(
            &organization,
            "kestrel",
            &["https://github.com/jtmthf/kestrel".to_owned()],
            "main",
        )
        .await;
    kestrel
        .declare_agent(&organization, "builder", "opencode", None)
        .await;

    let workspace = kestrel.open_workspace("acme", "kestrel", "builder").await;
    let first = kestrel.enqueue_session(workspace.id).await;
    kestrel.stop_session(first.id).await;
    let second = kestrel.enqueue_session(workspace.id).await;
    let (first_id, second_id) = (first.id.to_string(), second.id.to_string());

    let by_name = recorded(
        &client(
            &kestrel,
            &["session", "show", &second.name, "--json", SESSION],
        )
        .await,
    );
    assert_eq!(by_name[0]["id"], second_id);

    let by_id =
        recorded(&client(&kestrel, &["session", "show", &first_id, "--json", SESSION]).await);
    assert_eq!(by_id[0]["name"], first.name);

    let prefix = shortest_prefix_of(&first_id, &[&second_id]);
    let by_prefix =
        recorded(&client(&kestrel, &["session", "show", &prefix, "--json", SESSION]).await);
    assert_eq!(by_prefix[0]["id"], first_id);

    let latest =
        recorded(&client(&kestrel, &["session", "show", "latest", "--json", SESSION]).await);
    assert_eq!(latest[0]["id"], second_id);

    kestrel.teardown().await;
}

#[tokio::test]
async fn workspace_and_session_names_remain_unique_when_creation_retries_collisions() {
    let kestrel = Kestrel::boot().await;
    let organization = kestrel.declare_organization("acme").await;
    kestrel
        .declare_project(
            &organization,
            "kestrel",
            &["https://github.com/jtmthf/kestrel".to_owned()],
            "main",
        )
        .await;
    kestrel
        .declare_agent(&organization, "builder", "opencode", None)
        .await;

    let mut workspace_names = HashSet::new();
    let mut session_names = HashSet::new();
    for _ in 0..100 {
        let workspace = kestrel.open_workspace("acme", "kestrel", "builder").await;
        assert!(workspace_names.insert(workspace.name));

        let session = kestrel.enqueue_session(workspace.id).await;
        assert!(session_names.insert(session.name));
    }

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_client_manages_triggers_without_opening_a_database() {
    let kestrel = Kestrel::boot().await;
    succeeded(&client(&kestrel, &["organization", "declare", "acme"]).await);
    succeeded(
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
                "--branch",
                "main",
            ],
        )
        .await,
    );
    succeeded(
        &client(
            &kestrel,
            &["agent", "declare", "builder", "--organization", "acme"],
        )
        .await,
    );
    let webhook = kestrel
        .register_webhook("acme", "events", "a-shared-secret")
        .await;
    let response = reqwest::Client::new()
        .post(format!("{}{}", kestrel.link(), webhook.webhook_path()))
        .bearer_auth("a-shared-secret")
        .header("content-type", "application/cloudevents+json")
        .body(
            json!({
                "id": "retained",
                "source": "urn:test",
                "specversion": "1.0",
                "type": "example",
                "time": "2026-09-19T00:00:00Z",
            })
            .to_string(),
        )
        .send()
        .await
        .expect("the webhook should answer");
    assert!(response.status().is_success());
    let retained = kestrel.events("acme").await[0].record_id.to_string();

    let declaration = [
        "trigger",
        "declare",
        "ready",
        "--organization",
        "acme",
        "--filter",
        r#"{"exact":{"type":"example"}}"#,
        "--brief",
        "Work {{ event.type }}",
        "--project",
        "kestrel",
        "--agent",
        "builder",
        "--json",
        TRIGGER,
    ];
    let declared = recorded(&client(&kestrel, &declaration).await);
    let trigger = declared[0]["id"].as_str().expect("a trigger id").to_owned();
    assert_eq!(declared[0]["name"], "ready");
    assert_eq!(declared[0]["state"], "enabled");

    let listed = recorded(
        &client(
            &kestrel,
            &[
                "trigger",
                "list",
                "--organization",
                "acme",
                "--json",
                TRIGGER,
            ],
        )
        .await,
    );
    assert_eq!(listed, declared);
    assert_eq!(
        recorded(
            &client(
                &kestrel,
                &[
                    "trigger",
                    "show",
                    "ready",
                    "--organization",
                    "acme",
                    "--json",
                    TRIGGER
                ]
            )
            .await
        ),
        declared
    );
    assert_eq!(
        recorded(&client(&kestrel, &declaration).await)[0]["id"],
        trigger
    );

    let changed = recorded(
        &client(
            &kestrel,
            &[
                "trigger",
                "declare",
                "ready",
                "--organization",
                "acme",
                "--filter",
                r#"{"exact":{"type":"example"}}"#,
                "--brief",
                "Triage {{ event.type }}",
                "--project",
                "kestrel",
                "--agent",
                "builder",
                "--json",
                TRIGGER,
            ],
        )
        .await,
    );
    assert_eq!(changed[0]["id"], trigger);
    assert_eq!(changed[0]["brief"], "Triage {{ event.type }}");
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    assert!(kestrel.workspaces("acme").await.is_empty());

    let test = vec![
        "trigger",
        "test",
        "ready",
        "--organization",
        "acme",
        "--event",
        &retained,
        "--json",
        "matches",
    ];
    let tested = recorded(&client(&kestrel, &test).await);
    assert_eq!(tested[0]["matches"], true);

    let disabled = recorded(
        &client(
            &kestrel,
            &[
                "trigger",
                "disable",
                "ready",
                "--organization",
                "acme",
                "--json",
                "state",
            ],
        )
        .await,
    );
    assert_eq!(disabled[0]["state"], "disabled:operator");
    let enabled = recorded(
        &client(
            &kestrel,
            &[
                "trigger",
                "enable",
                "ready",
                "--organization",
                "acme",
                "--json",
                "state",
            ],
        )
        .await,
    );
    assert_eq!(enabled[0]["state"], "enabled");

    kestrel.teardown().await;
}

#[tokio::test]
async fn the_operator_documents_trigger_answers_and_refusals() {
    let kestrel = Kestrel::boot().await;
    let organization = kestrel.declare_organization("acme").await;
    kestrel
        .declare_project(
            &organization,
            "kestrel",
            &["https://github.com/jtmthf/kestrel".to_owned()],
            "main",
        )
        .await;
    kestrel
        .declare_agent(&organization, "builder", "opencode", None)
        .await;
    let triggers = triggers_of("acme");
    let declaration = json!({
        "name": "sweep",
        "every": "1h",
        "brief": "Sweep {{ event.data.trigger }}",
        "project": "kestrel",
        "agent": "builder",
    });

    assert!(listed_nothing(&kestrel, &triggers).await);
    let (status, trigger) = declared(&kestrel, &triggers, &declaration).await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, repeated) = declared(&kestrel, &triggers, &declaration).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(repeated["id"], trigger["id"]);
    let path = trigger_at("acme", "sweep");
    let (status, _) = got(&kestrel, &path).await;
    assert_eq!(status, StatusCode::OK);
    let (status, tested) = declared(&kestrel, &format!("{path}/test"), &json!({})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(tested["matches"], true);
    for misnamed in [
        json!({ "issue": 60 }),
        json!({ "integration": "github" }),
        json!({ "issue": 60, "integration": "github", "event": EventRecordId::generate().to_string() }),
    ] {
        let (status, _) = declared(&kestrel, &format!("{path}/test"), &misnamed).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{misnamed}");
    }
    let (status, _) = declared(&kestrel, &format!("{path}/disable"), &json!({})).await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = declared(&kestrel, &format!("{path}/enable"), &json!({})).await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = declared(
        &kestrel,
        &triggers,
        &json!({
            "name": "broken",
            "filter": { "exact": { "type": "x" } },
            "every": "1h",
            "brief": "x",
            "project": "kestrel",
            "agent": "builder",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_trigger_declared_on_a_cron_prints_its_expression_and_zone() {
    let kestrel = Kestrel::boot().await;
    let organization = kestrel.declare_organization("acme").await;
    kestrel
        .declare_project(
            &organization,
            "kestrel",
            &["https://github.com/jtmthf/kestrel".to_owned()],
            "main",
        )
        .await;
    kestrel
        .declare_agent(&organization, "builder", "opencode", None)
        .await;
    let fields = "name,every,cron,zone,filter";
    let declare = |extra: &'static [&'static str]| {
        let mut args = vec![
            "trigger",
            "declare",
            "triage",
            "--organization",
            "acme",
            "--brief",
            "Triage",
            "--project",
            "kestrel",
            "--agent",
            "builder",
            "--json",
            fields,
        ];
        args.extend_from_slice(extra);
        args
    };

    let triage = recorded(
        &client(
            &kestrel,
            &declare(&["--cron", "0 9 * * 1-5", "--zone", "America/New_York"]),
        )
        .await,
    );
    assert_eq!(
        triage,
        [json!({
            "name": "triage",
            "every": null,
            "cron": "0 9 * * 1-5",
            "zone": "America/New_York",
            "filter": null,
        })]
    );
    let listed = recorded(
        &client(
            &kestrel,
            &[
                "trigger",
                "list",
                "--organization",
                "acme",
                "--json",
                fields,
            ],
        )
        .await,
    );
    assert_eq!(listed, triage);
    let shown = recorded(
        &client(
            &kestrel,
            &[
                "trigger",
                "show",
                "triage",
                "--organization",
                "acme",
                "--json",
                fields,
            ],
        )
        .await,
    );
    assert_eq!(shown, triage);

    for refused in [
        declare(&["--cron", "0 9 * * *", "--zone", "UTC", "--every", "1h"]),
        declare(&[
            "--cron",
            "0 9 * * *",
            "--zone",
            "UTC",
            "--filter",
            r#"{"exact":{"type":"x"}}"#,
        ]),
        declare(&["--cron", "0 9 * * *"]),
    ] {
        failed(&client(&kestrel, &refused).await);
    }

    let triggers = triggers_of("acme");
    for body in [
        json!({ "cron": "0 9 * * *", "every": "1h" }),
        json!({ "cron": "0 9 * * *", "zone": "UTC", "filter": { "exact": { "type": "x" } } }),
        json!({ "cron": "0 9 * * *" }),
        json!({ "every": "1h", "zone": "UTC" }),
        json!({ "cron": "0 9 * * *", "zone": "Mars/Olympus_Mons" }),
        json!({ "cron": "* * * * *", "zone": "UTC" }),
    ] {
        let mut declaration = json!({
            "name": "broken",
            "brief": "x",
            "project": "kestrel",
            "agent": "builder",
        });
        declaration
            .as_object_mut()
            .expect("a declaration is an object")
            .extend(body.as_object().expect("a body is an object").clone());
        let (status, refusal) = declared(&kestrel, &triggers, &declaration).await;
        assert_eq!(
            status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{body}: {refusal}"
        );
    }

    kestrel.teardown().await;
}

#[tokio::test]
async fn the_operator_documents_workspace_and_session_answers_and_refusals() {
    let kestrel = Kestrel::boot().await;
    let organization = kestrel.declare_organization("acme").await;
    kestrel
        .declare_project(
            &organization,
            "kestrel",
            &["https://github.com/jtmthf/kestrel".to_owned()],
            "main",
        )
        .await;
    kestrel
        .declare_agent(&organization, "builder", "opencode", None)
        .await;
    let workspaces = operator::WORKSPACES.replace("{organization}", "acme");

    let (status, _) = got(&kestrel, &workspaces).await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = declared(
        &kestrel,
        &workspaces,
        &json!({ "project": "nowhere", "agent": "builder" }),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, opened) = declared(
        &kestrel,
        &workspaces,
        &json!({ "project": "kestrel", "agent": "builder" }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let workspace = opened["id"].as_str().expect("a workspace id");
    let shown = workspace_at("acme", workspace);
    let messages = workspace_messages_at("acme", workspace);
    let sessions = sessions_of("acme", workspace);

    let (status, _) = got(&kestrel, &shown).await;
    assert_eq!(status, StatusCode::OK);
    let (status, posted) = declared(
        &kestrel,
        &messages,
        &json!({ "participant": "operator", "message": "start with the operator boundary" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(posted["workspace"], workspace);
    let (status, _) = got(&kestrel, &sessions).await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = declared(&kestrel, &sessions, &json!({})).await;
    assert_eq!(status, StatusCode::CONFLICT);

    let session_id = posted["id"].as_str().expect("a session id");
    let session_name = posted["name"].as_str().expect("a generated session name");
    let (status, shown_session) = got(&kestrel, &session_at("acme", session_id)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(shown_session["workspace"], workspace);
    let (status, named_session) = got(&kestrel, &session_at("acme", session_name)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(named_session["id"], session_id);

    let session = kestrel
        .claim_session()
        .await
        .expect("the posted session should wait for the worker");
    kestrel.complete_session(&session).await;
    let seal = workspace_seal_at("acme", workspace);
    let (status, _) = declared(&kestrel, &seal, &json!({})).await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = declared(&kestrel, &seal, &json!({})).await;
    assert_eq!(status, StatusCode::CONFLICT);
    let (status, _) = declared(
        &kestrel,
        &workspaces,
        &json!({ "project": "kestrel", "agent": "builder", "continues": workspace }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let workspace_name = opened["name"].as_str().expect("a generated workspace name");
    let (status, _) = declared(
        &kestrel,
        &workspaces,
        &json!({ "project": "kestrel", "agent": "builder", "continues": workspace_name }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let nowhere = workspace_at("acme", "01a0a2d8-baf8-7c02-99fa-7280f174c14a");
    let (status, _) = got(&kestrel, &nowhere).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_post_names_its_participant_and_refuses_an_agents_name() {
    let kestrel = Kestrel::boot().await;
    let organization = kestrel.declare_organization("acme").await;
    kestrel
        .declare_project(
            &organization,
            "kestrel",
            &["https://github.com/jtmthf/kestrel".to_owned()],
            "main",
        )
        .await;
    kestrel
        .declare_agent(&organization, "builder", "opencode", None)
        .await;
    kestrel
        .declare_agent(&organization, "reviewer", "opencode", None)
        .await;
    let workspace = kestrel.open_workspace("acme", "kestrel", "builder").await;
    let messages = workspace_messages_at("acme", &workspace.id.to_string());

    for participant in [
        Value::Null,
        json!("   "),
        json!("x".repeat(65)),
        json!("con\u{7}trol"),
        json!("builder"),
        json!("reviewer"),
    ] {
        let mut body = json!({ "message": "hello" });
        if !participant.is_null() {
            body["participant"] = participant;
        }

        let (status, refusal) = declared(&kestrel, &messages, &body).await;
        assert_eq!(
            status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{body}: {refusal}"
        );
        assert_eq!(refusal["field"], "participant", "{body}: {refusal}");
    }

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_names_first_turn_joins_once_and_a_later_turn_does_not() {
    let kestrel = Kestrel::boot().await;
    let organization = kestrel.declare_organization("acme").await;
    kestrel
        .declare_project(
            &organization,
            "kestrel",
            &["https://github.com/jtmthf/kestrel".to_owned()],
            "main",
        )
        .await;
    kestrel
        .declare_agent(&organization, "builder", "opencode", None)
        .await;
    let workspace = kestrel.open_workspace("acme", "kestrel", "builder").await;

    kestrel.post(workspace.id, "alice", "the first thing").await;
    kestrel
        .post_while_busy(workspace.id, "alice", "the second thing")
        .await;

    let entries: Vec<Entry> = kestrel
        .transcript(workspace.id)
        .await
        .into_iter()
        .map(|recorded| recorded.entry)
        .collect();
    assert_eq!(
        entries,
        vec![
            Entry::ParticipantJoined {
                participant: "builder".to_owned(),
            },
            Entry::ParticipantJoined {
                participant: "alice".to_owned(),
            },
            Entry::Said {
                participant: "alice".to_owned(),
                message: "the first thing".to_owned(),
            },
            Entry::Said {
                participant: "alice".to_owned(),
                message: "the second thing".to_owned(),
            },
        ]
    );

    kestrel.teardown().await;
}

#[test]
fn the_published_document_requires_a_participant_on_a_post() {
    let document = published();
    let required = document["components"]["schemas"]["WorkspaceMessage"]["required"]
        .as_array()
        .expect("an array of required fields");

    assert!(
        required.iter().any(|field| field == "participant"),
        "the document does not require a participant: {required:?}"
    );
}

#[tokio::test]
async fn sealing_a_workspace_whose_instance_holds_unpublished_work_is_a_conflict_not_an_outage() {
    let kestrel = Kestrel::boot().await;
    let organization = kestrel.declare_organization("acme").await;
    kestrel
        .declare_project(
            &organization,
            "kestrel",
            &["https://github.com/jtmthf/kestrel".to_owned()],
            "main",
        )
        .await;
    kestrel
        .declare_agent(&organization, "builder", "opencode", None)
        .await;
    let workspace = kestrel.open_workspace("acme", "kestrel", "builder").await;

    let queued = kestrel.enqueue_session(workspace.id).await;
    let claimed = kestrel
        .occupy_session()
        .await
        .expect("the session should claim");
    assert_eq!(claimed.id, queued.id);
    kestrel.executes_on(&claimed, "held").await;
    kestrel
        .report_checkout(
            &claimed,
            vec![Observed {
                repository: "https://github.com/jtmthf/kestrel".to_owned(),
                git: Git::Read {
                    branch: Some("main".to_owned()),
                    untracked: 0,
                    uncommitted: 1,
                    stashes: 0,
                    unpushed: 0,
                },
            }],
        )
        .await;
    kestrel.complete_session(&claimed).await;

    let workspace_id = workspace.id.to_string();
    let (status, refusal) = declared(
        &kestrel,
        &workspace_seal_at("acme", &workspace_id),
        &json!({}),
    )
    .await;

    assert_eq!(status, StatusCode::CONFLICT, "{refusal}");
    let message = refusal["message"].as_str().expect("a message");
    assert!(
        message.contains("may hold the only copy of its work"),
        "{refusal}"
    );

    let rejected = client(&kestrel, &["workspace", "seal", &workspace_id]).await;
    assert!(
        failed(&rejected).contains("may hold the only copy of its work"),
        "{}",
        rejected.err
    );
    assert_eq!(
        rejected.status.code(),
        Some(4),
        "a refusal should exit rejected, not unavailable: {}",
        rejected.err
    );

    kestrel.teardown().await;
}

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

fn a_start(organization: &str, harness: &str, brief: &str) -> Value {
    json!({
        "organization": organization,
        "project": {
            "name": "kestrel",
            "repositories": ["https://github.com/jtmthf/kestrel"],
            "branch": "main",
        },
        "agent": { "name": "builder", "harness": harness, "model": null },
        "brief": brief,
    })
}

#[tokio::test]
async fn a_start_declares_its_setup_and_reaches_a_session_carrying_its_brief() {
    let kestrel = Kestrel::boot().await;

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
    assert_eq!(
        kestrel.transcript(workspace).await[0].entry,
        Entry::Brief {
            trigger: None,
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

    assert_eq!(status, StatusCode::CONFLICT, "{refused}");
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
async fn a_declaration_that_describes_nothing_declarable_is_refused() {
    let kestrel = Kestrel::boot().await;
    declared(
        &kestrel,
        operator::ORGANIZATIONS,
        &json!({ "name": "acme" }),
    )
    .await;

    let (malformed, _) = declared(
        &kestrel,
        operator::ORGANIZATIONS,
        &json!({ "title": "acme" }),
    )
    .await;
    let (unnamed, _) = declared(&kestrel, operator::ORGANIZATIONS, &json!({ "name": "" })).await;
    let (nowhere, refusal) = declared(
        &kestrel,
        &projects_of("acme"),
        &json!({ "name": "kestrel", "repositories": [], "branch": "main" }),
    )
    .await;
    let (clashing, clash) = declared(
        &kestrel,
        &projects_of("acme"),
        &json!({
            "name": "kestrel",
            "repositories": ["https://github.com/acme/api.git", "https://github.com/team/api"],
            "branch": "main",
        }),
    )
    .await;

    assert_eq!(malformed, StatusCode::BAD_REQUEST);
    assert_eq!(unnamed, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(nowhere, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        refusal["message"]
            .as_str()
            .expect("a message")
            .contains("repository"),
        "{refusal}"
    );
    assert_eq!(clashing, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        clash["message"]
            .as_str()
            .expect("a message")
            .contains("checked out into api"),
        "{clash}"
    );
    assert!(listed(&kestrel, &projects_of("acme")).await.is_empty());

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

#[tokio::test]
async fn a_client_sets_lists_and_forgets_provider_credentials_without_saying_them() {
    let kestrel = Kestrel::boot().await;
    let organization = kestrel.declare_organization("acme").await;
    let secret = "sk-kestrel-should-never-say-this";

    let set = client_given(
        &kestrel,
        &[
            "credential",
            "set",
            "ANTHROPIC_API_KEY",
            "--organization",
            "acme",
            "--json",
            CREDENTIAL,
        ],
        Some(&format!("{secret}\n")),
    )
    .await;
    let held = recorded(&set);
    let listed = client(
        &kestrel,
        &[
            "credential",
            "list",
            "--organization",
            "acme",
            "--json",
            CREDENTIAL,
        ],
    )
    .await;

    assert_eq!(held.len(), 1);
    assert_eq!(held[0]["variable"], "ANTHROPIC_API_KEY");
    assert_eq!(recorded(&listed), held);
    for said in [&set, &listed] {
        assert!(
            !said.out.join("\n").contains(secret) && !said.err.contains(secret),
            "the client spelled the credential out"
        );
    }
    assert_eq!(
        kestrel.provider_credentials_held(&organization).await[0].variable,
        "ANTHROPIC_API_KEY"
    );

    let forgotten = client(
        &kestrel,
        &[
            "credential",
            "forget",
            "ANTHROPIC_API_KEY",
            "--organization",
            "acme",
        ],
    )
    .await;
    assert!(succeeded(&forgotten).is_empty());
    assert!(listed_nothing(&kestrel, &credentials_of("acme")).await);
    assert!(
        kestrel
            .provider_credentials_held(&organization)
            .await
            .is_empty()
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_credential_answers_back_what_it_is_read_from_and_never_its_value() {
    let kestrel = Kestrel::boot().await;
    kestrel.declare_organization("acme").await;

    let (status, held) = requested(
        &kestrel,
        reqwest::Method::PUT,
        &credential_of("acme", "OPENAI_API_KEY"),
        Some(&json!({ "secret": "the-first-key" })),
    )
    .await;
    let (replaced, again) = requested(
        &kestrel,
        reqwest::Method::PUT,
        &credential_of("acme", "OPENAI_API_KEY"),
        Some(&json!({ "secret": "the-second-key" })),
    )
    .await;
    let listed = listed(&kestrel, &credentials_of("acme")).await;

    assert_eq!(status, StatusCode::OK, "{held}");
    assert_eq!(replaced, StatusCode::OK, "{again}");
    assert_eq!(listed, vec![again.clone()]);
    for answered in [&held, &again, &Value::Array(listed)] {
        let answered = answered.to_string();
        assert!(
            !answered.contains("the-first-key") && !answered.contains("the-second-key"),
            "the boundary answered a secret: {answered}"
        );
    }

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_credential_no_process_could_carry_or_nobody_holds_is_refused() {
    let kestrel = Kestrel::boot().await;
    kestrel.declare_organization("acme").await;

    let (unnamed, refusal) = requested(
        &kestrel,
        reqwest::Method::PUT,
        &credential_of("acme", "NOT-A-VARIABLE"),
        Some(&json!({ "secret": "a-key" })),
    )
    .await;
    let (reserved, reason) = requested(
        &kestrel,
        reqwest::Method::PUT,
        &credential_of("acme", "KESTREL_PROVIDER_KEY"),
        Some(&json!({ "secret": "a-key" })),
    )
    .await;
    let (empty, _) = requested(
        &kestrel,
        reqwest::Method::PUT,
        &credential_of("acme", "A_KEY"),
        Some(&json!({ "secret": "" })),
    )
    .await;
    let (nowhere, _) = requested(
        &kestrel,
        reqwest::Method::PUT,
        &credential_of("globex", "A_KEY"),
        Some(&json!({ "secret": "a-key" })),
    )
    .await;
    let (unheld, _) = requested(
        &kestrel,
        reqwest::Method::DELETE,
        &credential_of("acme", "A_KEY"),
        None,
    )
    .await;
    let nothing_on_stdin = client_given(
        &kestrel,
        &["credential", "set", "A_KEY", "--organization", "acme"],
        Some(""),
    )
    .await;
    let forgetting = client(
        &kestrel,
        &["credential", "forget", "A_KEY", "--organization", "acme"],
    )
    .await;

    assert_eq!(unnamed, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(reserved, StatusCode::UNPROCESSABLE_ENTITY, "{reason}");
    assert!(
        reason["message"]
            .as_str()
            .expect("a message")
            .contains("reserved")
    );
    assert!(
        refusal["message"]
            .as_str()
            .expect("a message")
            .contains("NOT-A-VARIABLE"),
        "{refusal}"
    );
    assert_eq!(empty, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(nowhere, StatusCode::NOT_FOUND);
    assert_eq!(unheld, StatusCode::NOT_FOUND);
    assert!(failed(&nothing_on_stdin).contains("standard input"));
    assert!(failed(&forgetting).contains("holds no provider credential named A_KEY"));
    assert!(listed_nothing(&kestrel, &credentials_of("acme")).await);

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_client_registers_and_lists_integrations_without_saying_their_secrets() {
    let kestrel = Kestrel::boot().await;
    let stub = GithubStub::start();
    kestrel.declare_organization("acme").await;

    let github = client(
        &kestrel,
        &[
            "integration",
            "register",
            "github",
            "hub",
            "--organization",
            "acme",
            "--repository",
            "jtmthf/kestrel",
            "--token",
            TOKEN,
            "--api",
            &stub.base_url(),
            "--interval",
            "5m",
            "--json",
            INTEGRATION,
        ],
    )
    .await;
    let webhook = client(
        &kestrel,
        &[
            "integration",
            "register",
            "webhook",
            "ci",
            "--organization",
            "acme",
            "--secret",
            "a-shared-secret",
            "--json",
            INTEGRATION,
        ],
    )
    .await;
    let listed = client(
        &kestrel,
        &[
            "integration",
            "list",
            "--organization",
            "acme",
            "--json",
            INTEGRATION,
        ],
    )
    .await;

    let github = recorded(&github);
    let webhook = recorded(&webhook);
    let records = recorded(&listed);
    assert_eq!(records, [webhook.clone(), github.clone()].concat());
    assert_eq!(github[0]["kind"], "github");
    assert_eq!(github[0]["repository"], "jtmthf/kestrel");
    assert_eq!(github[0]["carries"], json!(["inbound", "outbound"]));
    assert_eq!(github[0]["polled_every"], "5m");
    assert_eq!(github[0]["webhook_path"], Value::Null);
    assert_eq!(webhook[0]["kind"], "webhook");
    assert_eq!(webhook[0]["carries"], json!(["inbound"]));
    assert_eq!(
        webhook[0]["webhook_path"],
        format!("/webhooks/{}", webhook[0]["id"].as_str().expect("an id"))
    );
    assert_eq!(webhook[0]["last_event_refusal"], Value::Null);
    let said = listed.out.join("\n");
    assert!(!said.contains(TOKEN), "the listing spelled the token out");
    assert!(
        !said.contains("a-shared-secret"),
        "the listing spelled the webhook secret out"
    );
    assert_eq!(
        kestrel.integrations("acme").await[0].id.to_string(),
        webhook[0]["id"].as_str().expect("an id")
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn an_integration_is_registered_with_what_it_is_declared_to_carry() {
    let kestrel = Kestrel::boot().await;
    kestrel.declare_organization("acme").await;

    let (status, signed) = declared(
        &kestrel,
        &integrations_of("acme"),
        &json!({
            "kind": "github",
            "name": "hub",
            "repository": "jtmthf/kestrel",
            "token": TOKEN,
            "carries": ["outbound"],
            "webhook_secret": "a-signing-secret",
        }),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED, "{signed}");
    assert_eq!(signed["carries"], json!(["outbound"]));
    assert_eq!(signed["polled_every"], Value::Null);
    assert!(signed["webhook_path"].is_string(), "{signed}");
    assert!(!signed.to_string().contains("a-signing-secret"));
    assert!(!signed.to_string().contains(TOKEN));

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_registration_that_describes_no_usable_integration_is_refused() {
    let kestrel = Kestrel::boot().await;
    kestrel.declare_organization("acme").await;
    let webhook = json!({ "kind": "webhook", "name": "ci", "secret": "a-shared-secret" });
    declared(&kestrel, &integrations_of("acme"), &webhook).await;

    let refusals = [
        (
            integrations_of("acme"),
            webhook.clone(),
            StatusCode::CONFLICT,
        ),
        (
            integrations_of("globex"),
            webhook.clone(),
            StatusCode::NOT_FOUND,
        ),
        (
            integrations_of("acme"),
            json!({ "kind": "pager", "name": "pd" }),
            StatusCode::BAD_REQUEST,
        ),
        (
            integrations_of("acme"),
            json!({ "kind": "webhook", "name": "", "secret": "a-shared-secret" }),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            integrations_of("acme"),
            json!({ "kind": "webhook", "name": "out", "secret": "s", "carries": ["outbound"] }),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            integrations_of("acme"),
            json!({ "kind": "github", "name": "hub", "repository": "kestrel", "token": TOKEN }),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            integrations_of("acme"),
            json!({
                "kind": "github",
                "name": "hub",
                "repository": "jtmthf/kestrel",
                "token": TOKEN,
                "interval": "whenever",
            }),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            integrations_of("acme"),
            json!({
                "kind": "github",
                "name": "hub",
                "repository": "jtmthf/kestrel",
                "token": TOKEN,
                "carries": [],
            }),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
    ];
    for (path, registration, expected) in &refusals {
        let (status, refusal) = declared(&kestrel, path, registration).await;
        assert_eq!(status, *expected, "{registration} was answered {refusal}");
        assert!(
            !refusal.to_string().contains(TOKEN),
            "a refusal spelled the token out: {refusal}"
        );
    }
    let taken = client(
        &kestrel,
        &[
            "integration",
            "register",
            "webhook",
            "ci",
            "--organization",
            "acme",
            "--secret",
            "another",
        ],
    )
    .await;

    assert!(failed(&taken).contains("already has an integration named ci"));
    assert_eq!(listed(&kestrel, &integrations_of("acme")).await.len(), 1);

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_client_acknowledges_the_event_an_integration_refused() {
    let kestrel = Kestrel::boot().await;
    kestrel.declare_organization("acme").await;
    let webhook = kestrel
        .register_webhook("acme", "ci", "a-shared-secret")
        .await;
    reqwest::Client::new()
        .post(format!("{}{}", kestrel.link(), webhook.webhook_path()))
        .bearer_auth("a-shared-secret")
        .header("content-type", "text/plain")
        .body("x".repeat(1024 * 1024 + 1))
        .send()
        .await
        .expect("the webhook answers");

    let refused = recorded(
        &client(
            &kestrel,
            &[
                "integration",
                "list",
                "--organization",
                "acme",
                "--json",
                "last_event_refusal",
            ],
        )
        .await,
    );
    let acknowledged = client(
        &kestrel,
        &[
            "integration",
            "acknowledge-refusal",
            "ci",
            "--organization",
            "acme",
        ],
    )
    .await;
    let (status, _) = requested(
        &kestrel,
        reqwest::Method::DELETE,
        &event_refusal_of("acme", "ci"),
        None,
    )
    .await;
    let (unknown, _) = requested(
        &kestrel,
        reqwest::Method::DELETE,
        &event_refusal_of("acme", "pager"),
        None,
    )
    .await;

    let refusal = &refused[0]["last_event_refusal"];
    assert!(
        refusal["bytes"].as_u64().expect("a size") > 1024 * 1024,
        "{refusal}"
    );
    assert!(refusal["reason"].is_string(), "{refusal}");
    assert!(succeeded(&acknowledged).is_empty());
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(unknown, StatusCode::NOT_FOUND);
    assert_eq!(
        listed(&kestrel, &integrations_of("acme")).await[0]["last_event_refusal"],
        Value::Null
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_client_lists_an_organizations_events_and_shows_one_whole() {
    let kestrel = Kestrel::boot().await;
    kestrel.declare_organization("acme").await;
    let webhook = kestrel
        .register_webhook("acme", "ci", "a-shared-secret")
        .await;
    for id in ["deploy-1", "deploy-2"] {
        let answered = reqwest::Client::new()
            .post(format!("{}{}", kestrel.link(), webhook.webhook_path()))
            .bearer_auth("a-shared-secret")
            .header("content-type", "application/cloudevents+json")
            .body(
                json!({
                    "specversion": "1.0",
                    "id": id,
                    "source": "/argo/sensors/deploy",
                    "type": "io.argoproj.deployed",
                    "subject": "kestrel",
                    "data": { "image": "kestrel:1" },
                })
                .to_string(),
            )
            .send()
            .await
            .expect("the webhook answers");
        assert_eq!(answered.status(), StatusCode::ACCEPTED);
    }

    let events = recorded(
        &client(
            &kestrel,
            &["event", "list", "--organization", "acme", "--json", EVENT],
        )
        .await,
    );
    let limited = recorded(
        &client(
            &kestrel,
            &[
                "event",
                "list",
                "--organization",
                "acme",
                "--limit",
                "1",
                "--json",
                EVENT,
            ],
        )
        .await,
    );
    let record = events[0]["record"].as_str().expect("a record id");
    let shown = recorded(&client(&kestrel, &["event", "show", record, "--json", EVENT]).await);

    assert_eq!(events.len(), 2);
    assert_eq!(limited, events[..1]);
    assert_eq!(shown, events[..1]);
    assert_eq!(shown[0]["integration"], webhook.id.to_string());
    assert_eq!(shown[0]["event"]["source"], "/argo/sensors/deploy");
    assert_eq!(shown[0]["event"]["type"], "io.argoproj.deployed");
    assert_eq!(shown[0]["event"]["specversion"], "1.0");
    assert_eq!(shown[0]["event"]["subject"], "kestrel");
    assert_eq!(shown[0]["event"]["data"], json!({ "image": "kestrel:1" }));
    assert_eq!(
        kestrel.events("acme").await[0].record_id.to_string(),
        record
    );
    let (_, over_the_boundary) = got(&kestrel, &format!("{}?limit=1", events_of("acme"))).await;
    assert_eq!(over_the_boundary.as_array().map(Vec::len), Some(1));
    assert_eq!(over_the_boundary[0]["record"], limited[0]["record"]);

    kestrel.teardown().await;
}

#[tokio::test]
async fn an_event_nobody_recorded_is_refused() {
    let kestrel = Kestrel::boot().await;

    let (unrecorded, refusal) =
        got(&kestrel, &event_at(&EventRecordId::generate().to_string())).await;
    let (malformed, _) = got(&kestrel, &event_at("yesterday")).await;
    let (nowhere, _) = got(&kestrel, &events_of("acme")).await;
    let showing = client(&kestrel, &["event", "show", "yesterday"]).await;

    assert_eq!(unrecorded, StatusCode::NOT_FOUND);
    assert!(
        refusal["message"]
            .as_str()
            .expect("a message")
            .starts_with("no event"),
        "{refusal}"
    );
    assert_eq!(malformed, StatusCode::NOT_FOUND);
    assert_eq!(nowhere, StatusCode::NOT_FOUND);
    assert!(failed(&showing).contains("no event yesterday"));

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_client_in_its_own_process_reads_a_transcript_over_the_operator_boundary() {
    let kestrel = Kestrel::boot().await;
    let (workspace, _) = an_open_workspace(&kestrel, 2).await;
    let (operator, reading) = (kestrel.operator(), workspace.clone());

    let read = tokio::task::spawn_blocking(move || {
        client::ran(
            &operator,
            &["workspace", "transcript", &reading, "--json", ENTRY],
        )
    })
    .await
    .expect("the client should run");

    assert!(read.status.success(), "the client failed:\n{}", read.err);
    assert_eq!(seqs(&read.out), recorded_seqs(&kestrel, &workspace).await);
    let said: Value = serde_json::from_str(&read.out[read.out.len() - 1]).expect("an entry");
    assert_eq!(said["entry"]["kind"], "said");
    assert_eq!(said["entry"]["message"], "message 2");
    assert!(
        read.err.contains("cursor  "),
        "the client printed no cursor to resume from:\n{}",
        read.err
    );
    assert!(
        read.left_behind.is_empty(),
        "the client wrote {:?} where it ran",
        read.left_behind
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_client_handed_a_cursor_reads_only_what_came_after_it() {
    let kestrel = Kestrel::boot().await;
    let (workspace, session) = an_open_workspace(&kestrel, 1).await;
    let operator = kestrel.operator();

    let first = {
        let (operator, workspace) = (operator.clone(), workspace.clone());
        tokio::task::spawn_blocking(move || {
            client::ran(&operator, &["workspace", "transcript", &workspace])
        })
        .await
        .expect("the client should run")
    };
    let cursor = first
        .err
        .lines()
        .find_map(|line| line.strip_prefix("cursor  "))
        .expect("a cursor")
        .to_owned();
    kestrel.said(&session, "said after the first read").await;

    let second = tokio::task::spawn_blocking(move || {
        client::ran(
            &operator,
            &["workspace", "transcript", &workspace, "--cursor", &cursor],
        )
    })
    .await
    .expect("the client should run");

    assert!(
        second.status.success(),
        "the client failed:\n{}",
        second.err
    );
    assert_eq!(second.out.len(), 1, "read {:?}", second.out);
    assert!(second.out[0].contains("said after the first read"));

    kestrel.teardown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_following_client_resumes_across_a_restart_without_repeating_an_entry() {
    let kestrel = Kestrel::boot().await;
    let (workspace, session) = an_open_workspace(&kestrel, 2).await;
    let before = recorded_seqs(&kestrel, &workspace).await.len();

    let mut client = Client::spawn(
        &kestrel.operator(),
        &[
            "workspace",
            "transcript",
            &workspace,
            "--follow",
            "--json",
            ENTRY,
        ],
    );
    let mut read = Vec::new();
    for _ in 0..before {
        read.push(tokio::task::block_in_place(|| client.line()));
    }

    kestrel.said(&session, "said while it followed").await;
    read.push(tokio::task::block_in_place(|| client.line()));
    kestrel.complete_session(&session).await;

    let kestrel = kestrel.teardown().await.restart().await;
    kestrel.said(&session, "said after the restart").await;
    kestrel
        .seal_workspace(workspace.parse().expect("a workspace id"))
        .await;

    let finished = tokio::task::block_in_place(|| client.finish());
    read.extend(finished.out);

    assert!(
        finished.status.success(),
        "the client failed:\n{}",
        finished.err
    );
    assert_eq!(seqs(&read), recorded_seqs(&kestrel, &workspace).await);
    assert!(
        read.last()
            .expect("an entry")
            .contains("said after the restart")
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_client_asking_for_no_such_workspace_is_refused() {
    let kestrel = Kestrel::boot().await;
    kestrel.declare_organization("acme").await;
    let operator = kestrel.operator();

    let read = tokio::task::spawn_blocking(move || {
        client::ran(
            &operator,
            &[
                "workspace",
                "transcript",
                "01a0a2d8-baf8-7c02-99fa-7280f174c14a",
            ],
        )
    })
    .await
    .expect("the client should run");

    assert!(!read.status.success());
    assert!(
        read.err
            .contains("no workspace in the organization acme matches"),
        "{}",
        read.err
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_cursor_from_another_transcript_is_refused_rather_than_restarting_the_walk() {
    let kestrel = Kestrel::boot().await;
    let (workspace, _) = an_open_workspace(&kestrel, 1).await;
    let elsewhere = format!("{}:1", SessionId::generate());

    let response = reqwest::Client::new()
        .get(format!(
            "{}{}",
            kestrel.operator(),
            transcript_of("acme", &workspace)
        ))
        .header("last-event-id", elsewhere)
        .send()
        .await
        .expect("the operator boundary should answer");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    kestrel.teardown().await;
}

#[tokio::test]
async fn the_operator_boundary_and_the_link_are_served_apart() {
    let kestrel = Kestrel::boot().await;
    let (workspace, session) = an_open_workspace(&kestrel, 0).await;
    let client = reqwest::Client::new();

    let link_on_the_operator_listener = client
        .get(format!(
            "{}{}",
            kestrel.operator(),
            link::ENTRIES.replace("{session}", &session.id.to_string())
        ))
        .send()
        .await
        .expect("the operator listener should answer");
    let operator_on_the_link_listener = client
        .get(format!(
            "{}{}?follow=false",
            kestrel.link(),
            transcript_of("acme", &workspace)
        ))
        .send()
        .await
        .expect("the link listener should answer");

    assert_ne!(kestrel.operator(), kestrel.link());
    assert_eq!(
        link_on_the_operator_listener.status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        operator_on_the_link_listener.status(),
        StatusCode::NOT_FOUND
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn the_operator_boundary_asks_for_no_credential() {
    let kestrel = Kestrel::boot().await;
    let (workspace, _) = an_open_workspace(&kestrel, 0).await;

    let response = reqwest::Client::new()
        .get(format!(
            "{}{}?follow=false",
            kestrel.operator(),
            transcript_of("acme", &workspace)
        ))
        .send()
        .await
        .expect("the operator boundary should answer");

    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        response
            .text()
            .await
            .expect("the stream should end")
            .contains("event: end")
    );

    kestrel.teardown().await;
}

fn queue_of(organization: &str) -> String {
    operator::QUEUE.replace("{organization}", organization)
}

/// A Workspace whose enqueued Session is queued; a Workspace has one Session at a time, so a
/// queue of many needs as many of these.
async fn a_queued_workspace_in(
    kestrel: &Kestrel,
    organization: &str,
    project: &str,
) -> WorkspaceId {
    kestrel
        .open_workspace(organization, project, "builder")
        .await
        .id
}

/// The queue rows as the snapshot served them, in the order read.
fn numbered(said: &Value) -> Vec<Value> {
    said["queued"]
        .as_array()
        .expect("the snapshot's queued Sessions")
        .clone()
}

#[tokio::test]
async fn a_client_reads_the_queue_in_enqueue_order_with_positions_its_blockers_and_limits() {
    let kestrel = Kestrel::boot().await;
    let organization = kestrel.declare_organization("acme").await;
    kestrel
        .declare_project(
            &organization,
            "kestrel",
            &["https://github.com/jtmthf/kestrel".to_owned()],
            "main",
        )
        .await;
    kestrel
        .declare_agent(&organization, "builder", "opencode", None)
        .await;

    let first = a_queued_workspace_in(&kestrel, "acme", "kestrel").await;
    let second = a_queued_workspace_in(&kestrel, "acme", "kestrel").await;
    let third = a_queued_workspace_in(&kestrel, "acme", "kestrel").await;
    let fourth = a_queued_workspace_in(&kestrel, "acme", "kestrel").await;
    let blocker = kestrel.enqueue_session(first).await;
    let ahead = kestrel.enqueue_session(second).await;
    let blocked = kestrel.enqueue_session(third).await;
    let behind = kestrel.enqueue_session(fourth).await;
    kestrel.block_session(&blocked, &blocker).await;

    let (status, queue) = got(&kestrel, &queue_of("acme")).await;
    assert_eq!(status, StatusCode::OK, "{queue}");
    let said = numbered(&queue);
    assert_eq!(
        said.iter()
            .map(|row| row["name"].as_str().expect("a generated name"))
            .collect::<Vec<_>>(),
        [
            blocker.name.as_str(),
            ahead.name.as_str(),
            blocked.name.as_str(),
            behind.name.as_str(),
        ],
        "the queue is not in enqueue order"
    );
    assert_eq!(
        said.iter()
            .map(|row| row["position"].as_u64())
            .collect::<Vec<_>>(),
        [Some(1), Some(2), None, Some(3)]
    );
    assert_eq!(
        said[2]["reasons"],
        json!([{ "kind": "dependencies", "sessions": [blocker.name] }])
    );
    assert_eq!(said[0]["reasons"], json!([]));

    // With the blocker succeeded, the Session it delayed takes the place it was enqueued into.
    kestrel.complete_session(&blocker).await;
    let (status, queue) = got(&kestrel, &queue_of("acme")).await;
    assert_eq!(status, StatusCode::OK, "{queue}");
    assert_eq!(
        numbered(&queue)
            .iter()
            .map(|row| row["position"].as_u64())
            .collect::<Vec<_>>(),
        [Some(1), Some(2), Some(3)],
    );
    assert_eq!(
        numbered(&queue)
            .iter()
            .map(|row| row["reasons"].as_array().map(Vec::len))
            .collect::<Vec<_>>(),
        [Some(0); 3]
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_session_of_another_organization_occupying_the_shared_slots_is_counted_and_never_named() {
    let kestrel = Kestrel::boot().await;
    for name in ["acme", "globex"] {
        let organization = kestrel.declare_organization(name).await;
        kestrel
            .declare_project(
                &organization,
                name,
                &["https://github.com/jtmthf/kestrel".to_owned()],
                "main",
            )
            .await;
        kestrel
            .declare_agent(&organization, "builder", "opencode", None)
            .await;
    }

    let ours = a_queued_workspace_in(&kestrel, "acme", "acme").await;
    let held = kestrel.dispatch_session(ours).await;
    let elsewhere = a_queued_workspace_in(&kestrel, "globex", "globex").await;
    let hit = kestrel.dispatch_session(elsewhere).await;

    let (status, queue) = got(&kestrel, &queue_of("acme")).await;
    assert_eq!(status, StatusCode::OK, "{queue}");
    assert_eq!(queue["active_work"]["occupied"], 2);
    assert_eq!(
        queue["active_work"]["occupants"],
        json!([held.name]),
        "the Session of another Organization is named by ours"
    );
    assert_eq!(queue["active_work"]["elsewhere"], 1);

    let (status, their_queue) = got(&kestrel, &queue_of("globex")).await;
    assert_eq!(status, StatusCode::OK, "{their_queue}");
    assert_eq!(their_queue["active_work"]["occupants"], json!([hit.name]));
    assert_eq!(their_queue["active_work"]["elsewhere"], 1);

    kestrel.teardown().await;
}

#[tokio::test]
async fn the_queue_says_when_a_live_instance_limit_is_unbounded_and_what_counts_against_it() {
    let kestrel = Kestrel::boot().await;
    let organization = kestrel.declare_organization("acme").await;
    let bounded = kestrel.declare_limited_organization("bounded", 1).await;
    for (organization, name) in [(&organization, "kestrel"), (&bounded, "held")] {
        kestrel
            .declare_project(
                organization,
                name,
                &["https://github.com/jtmthf/kestrel".to_owned()],
                "main",
            )
            .await;
        kestrel
            .declare_agent(organization, "builder", "opencode", None)
            .await;
    }

    let workspace = kestrel.open_workspace("acme", "kestrel", "builder").await;
    let held = kestrel.dispatch_session(workspace.id).await;
    kestrel.executes_on(&held, "docker/kestrel-a").await;
    kestrel.complete_session(&held).await;

    let (status, unbounded) = got(&kestrel, &queue_of("acme")).await;
    assert_eq!(status, StatusCode::OK, "{unbounded}");
    assert_eq!(
        unbounded["instances"],
        json!({
            "limit": null,
            "count": 1,
            "counted": ["docker/kestrel-a"],
        })
    );

    let (status, limited) = got(&kestrel, &queue_of("bounded")).await;
    assert_eq!(status, StatusCode::OK, "{limited}");
    assert_eq!(
        limited["instances"],
        json!({
            "limit": 1,
            "count": 0,
            "counted": [],
        })
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn the_queue_reads_the_recorded_dispatch_which_a_restart_with_new_flags_replaces() {
    let supervisor_path = supervisor::binary().to_owned();

    // The record is written as a work role that can dispatch starts, a little after the
    // boundary answers, so both reads wait for it.
    async fn recorded(kestrel: &Kestrel, slots: usize) -> Value {
        kestrel.declare_organization("acme").await;
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            let (status, said) = got(kestrel, &queue_of("acme")).await;
            assert_eq!(status, StatusCode::OK, "{said}");
            if said["active_work"]["limit"] == json!(slots) {
                return said;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "the snapshot never said the work role dispatches {slots} slots: {said}"
            );
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    }

    let kestrel = Kestrel::dispatching_up_to(&supervisor_path, "-", 3).await;
    kestrel.declare_organization("acme").await;
    let first = recorded(&kestrel, 3).await;
    assert_eq!(
        first["work_role"],
        json!({ "active_work_slots": 3, "serialized_harnesses": ["codex"] }),
    );

    let restarted = kestrel
        .teardown()
        .await
        .restart_with(&supervisor_path, "-", 5)
        .await;
    let replaced = recorded(&restarted, 5).await;
    assert_eq!(
        replaced["work_role"],
        json!({ "active_work_slots": 5, "serialized_harnesses": ["codex"] }),
    );

    // The queue is read for acme whatever workspaces it has or does not have.
    assert_eq!(replaced["queued"], json!([]));
    assert_eq!(replaced["active_work"]["occupied"], 0);

    restarted.teardown().await;
}

#[tokio::test]
async fn a_queue_without_a_recorded_dispatch_says_so_and_still_numbers_the_queue() {
    let kestrel = Kestrel::boot().await;
    let organization = kestrel.declare_organization("acme").await;
    kestrel
        .declare_project(
            &organization,
            "kestrel",
            &["https://github.com/jtmthf/kestrel".to_owned()],
            "main",
        )
        .await;
    kestrel
        .declare_agent(&organization, "builder", "opencode", None)
        .await;
    let first = a_queued_workspace_in(&kestrel, "acme", "kestrel").await;
    let second = a_queued_workspace_in(&kestrel, "acme", "kestrel").await;
    kestrel.enqueue_session(first).await;
    kestrel.enqueue_session(second).await;

    let (status, queue) = got(&kestrel, &queue_of("acme")).await;
    assert_eq!(status, StatusCode::OK, "{queue}");
    assert_eq!(queue["work_role"], Value::Null);
    assert_eq!(queue["active_work"]["limit"], Value::Null);
    assert_eq!(queue["active_work"]["occupied"], 0);
    assert_eq!(
        numbered(&queue)
            .iter()
            .map(|row| row["position"].as_u64())
            .collect::<Vec<_>>(),
        [Some(1), Some(2)]
    );

    kestrel.teardown().await;
}

const QUEUE_ROW: &str = "position,name,agent,reasons";
const QUEUE_LIMITS: &str =
    "active_work.limit,active_work.occupied,active_work.elsewhere,instances.limit,instances.count";

#[tokio::test]
async fn a_client_reads_the_queue_with_kestrel_queue_and_whatever_fields_it_names() {
    let kestrel = Kestrel::boot().await;
    let organization = kestrel.declare_organization("acme").await;
    kestrel
        .declare_project(
            &organization,
            "kestrel",
            &["https://github.com/jtmthf/kestrel".to_owned()],
            "main",
        )
        .await;
    kestrel
        .declare_agent(&organization, "builder", "opencode", None)
        .await;

    let first = a_queued_workspace_in(&kestrel, "acme", "kestrel").await;
    let second = a_queued_workspace_in(&kestrel, "acme", "kestrel").await;
    let third = a_queued_workspace_in(&kestrel, "acme", "kestrel").await;
    let blocker = kestrel.enqueue_session(first).await;
    let between = kestrel.enqueue_session(second).await;
    let blocked = kestrel.enqueue_session(third).await;
    kestrel.block_session(&blocked, &blocker).await;

    let rows = recorded(
        &client(
            &kestrel,
            &["queue", "--organization", "acme", "--json", QUEUE_ROW],
        )
        .await,
    );
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0]["position"].as_u64(), Some(1));
    assert_eq!(rows[0]["name"], blocker.name);
    assert_eq!(rows[1]["position"].as_u64(), Some(2));
    assert_eq!(rows[1]["name"], between.name);
    assert_eq!(rows[2]["position"].as_u64(), None);
    assert_eq!(rows[2]["name"], blocked.name);
    assert_eq!(
        rows[2]["reasons"],
        json!([{ "kind": "dependencies", "sessions": [blocker.name] }])
    );

    let limits = recorded(
        &client(
            &kestrel,
            &["queue", "--organization", "acme", "--json", QUEUE_LIMITS],
        )
        .await,
    );
    assert_eq!(limits.len(), 3, "one record a queued Session");
    assert_eq!(
        limits[0],
        json!({
            "active_work": { "limit": Value::Null, "occupied": 0, "elsewhere": 0 },
            "instances": { "limit": Value::Null, "count": 0 },
        })
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_terminal_reads_the_kestrel_queue_with_the_limits_said_first() {
    let kestrel = Kestrel::boot().await;
    let organization = kestrel.declare_organization("acme").await;
    kestrel
        .declare_project(
            &organization,
            "kestrel",
            &["https://github.com/jtmthf/kestrel".to_owned()],
            "main",
        )
        .await;
    kestrel
        .declare_agent(&organization, "builder", "opencode", None)
        .await;
    let first = a_queued_workspace_in(&kestrel, "acme", "kestrel").await;
    let second = a_queued_workspace_in(&kestrel, "acme", "kestrel").await;
    let blocker = kestrel.enqueue_session(first).await;
    let queued = kestrel.enqueue_session(second).await;

    let shown = client::ran_on_a_terminal_by(
        &kestrel,
        &["queue", "--organization", "acme"],
        client::Invocation::default(),
        "",
    )
    .await;
    assert!(shown.status.success(), "{}", shown.said);
    let said = shown.said;
    assert!(
        said.contains("active-work slots  no work role is dispatching; 0 working"),
        "{said}"
    );
    assert!(
        said.contains("live instances     unbounded; 0 counted"),
        "{said}"
    );
    // The table's own labels and the enqueued times are truncated on a terminal this wide.
    assert!(
        said.contains("position  name"),
        "the columns come without their labels: {said}"
    );
    assert!(
        said.contains(&format!("1         {}", blocker.name)),
        "{said}"
    );
    assert!(
        said.contains(&format!("2         {}", queued.name)),
        "{said}"
    );

    kestrel.teardown().await;
}

fn clean_checkout() -> Vec<Observed> {
    vec![Observed {
        repository: "https://github.com/jtmthf/kestrel".to_owned(),
        git: Git::Read {
            branch: Some("kestrel/work".to_owned()),
            untracked: 0,
            uncommitted: 0,
            stashes: 0,
            unpushed: 0,
        },
    }]
}

/// An Organization of `maximum` live Instances whose `idle` Workspaces each keep one, clean.
async fn at_the_instance_limit(kestrel: &Kestrel, maximum: usize, idle: usize) -> Vec<WorkspaceId> {
    let organization = kestrel.declare_limited_organization("acme", maximum).await;
    kestrel
        .declare_project(
            &organization,
            "kestrel",
            &["https://github.com/jtmthf/kestrel".to_owned()],
            "main",
        )
        .await;
    kestrel
        .declare_agent(&organization, "builder", "opencode", None)
        .await;

    let mut kept = Vec::new();
    for place in 0..idle {
        let workspace = a_queued_workspace_in(kestrel, "acme", "kestrel").await;
        let session = kestrel.dispatch_session(workspace).await;
        kestrel
            .executes_on(&session, &format!("docker/kestrel-{place}"))
            .await;
        kestrel.report_checkout(&session, clean_checkout()).await;
        kestrel.complete_session(&session).await;
        kept.push(workspace);
    }

    kept
}

fn reasons_of<'a>(queue: &'a Value, section: &str, name: &str) -> &'a Value {
    let row = queue[section]
        .as_array()
        .unwrap_or_else(|| panic!("the snapshot's {section} Sessions: {queue}"))
        .iter()
        .find(|row| row["name"] == name)
        .unwrap_or_else(|| panic!("no {section} Session {name}: {queue}"));

    &row["reasons"]
}

#[tokio::test]
async fn reading_the_queue_at_the_instance_limit_archives_nothing_and_dispatch_still_does() {
    let kestrel = Kestrel::boot().await;
    at_the_instance_limit(&kestrel, 1, 1).await;
    let arriving = a_queued_workspace_in(&kestrel, "acme", "kestrel").await;
    let queued = kestrel.enqueue_session(arriving).await;

    let (status, queue) = got(&kestrel, &queue_of("acme")).await;
    assert_eq!(status, StatusCode::OK, "{queue}");
    assert_eq!(
        *reasons_of(&queue, "queued", &queued.name),
        json!([{ "kind": "instance_archiving", "instance": "docker/kestrel-0" }])
    );
    assert_eq!(numbered(&queue)[0]["position"], Value::Null);
    assert!(
        kestrel.instances_to_archive().await.is_empty(),
        "reading the queue archived an Instance"
    );

    assert!(kestrel.occupy_session().await.is_none());
    assert_eq!(kestrel.instances_to_archive().await, ["docker/kestrel-0"]);
    let (_, queue) = got(&kestrel, &queue_of("acme")).await;
    assert_eq!(
        *reasons_of(&queue, "queued", &queued.name),
        json!([{ "kind": "instance_archiving", "instance": "docker/kestrel-0" }])
    );

    kestrel.teardown().await;
}

/// An Organization whose `builder` runs a serialized harness, with the Subscription Profile
/// `jack` for its Workspaces to share, and a work role recorded dispatching `slots`.
async fn sharing_a_serialized_profile(kestrel: &Kestrel, maximum: Option<usize>, slots: usize) {
    let organization = match maximum {
        Some(maximum) => kestrel.declare_limited_organization("acme", maximum).await,
        None => kestrel.declare_organization("acme").await,
    };
    kestrel
        .declare_project(
            &organization,
            "kestrel",
            &["https://github.com/jtmthf/kestrel".to_owned()],
            "main",
        )
        .await;
    kestrel
        .declare_agent(&organization, "builder", SERIALIZED, None)
        .await;
    kestrel
        .declare_profile("acme", "jack", "Jack")
        .await
        .expect("the profile should declare");
    kestrel.record_dispatch(slots).await;
}

async fn jacks_workspace(kestrel: &Kestrel) -> WorkspaceId {
    kestrel
        .open_workspace_with("acme", "kestrel", "builder", "jack")
        .await
        .id
}

async fn queue_read(kestrel: &Kestrel) -> Value {
    let (status, queue) = got(kestrel, &queue_of("acme")).await;
    assert_eq!(status, StatusCode::OK, "{queue}");
    queue
}

#[tokio::test]
async fn a_queued_session_names_each_reason_that_holds_it_alone_and_together() {
    let kestrel = Kestrel::boot().await;
    sharing_a_serialized_profile(&kestrel, Some(2), 4).await;
    let holding = kestrel
        .dispatch_session(jacks_workspace(&kestrel).await)
        .await;
    kestrel.executes_on(&holding, "docker/holding").await;
    let held = kestrel
        .enqueue_session(jacks_workspace(&kestrel).await)
        .await;
    let blocked = kestrel
        .enqueue_session(a_queued_workspace_in(&kestrel, "acme", "kestrel").await)
        .await;
    kestrel.block_session(&blocked, &held).await;
    let profile =
        json!({ "kind": "subscription_profile", "profile": "jack", "session": holding.name });
    let dependencies = json!({ "kind": "dependencies", "sessions": [held.name] });

    let queue = queue_read(&kestrel).await;
    assert_eq!(*reasons_of(&queue, "queued", &held.name), json!([profile]));
    assert_eq!(
        *reasons_of(&queue, "queued", &blocked.name),
        json!([dependencies])
    );

    let filling = kestrel
        .dispatch_session(a_queued_workspace_in(&kestrel, "acme", "kestrel").await)
        .await;
    kestrel.executes_on(&filling, "docker/filling").await;
    let limited = kestrel
        .enqueue_session(a_queued_workspace_in(&kestrel, "acme", "kestrel").await)
        .await;
    let limit = json!({ "kind": "live_instance_limit", "limit": 2 });

    let queue = queue_read(&kestrel).await;
    assert_eq!(
        *reasons_of(&queue, "queued", &held.name),
        json!([profile, limit])
    );
    assert_eq!(
        *reasons_of(&queue, "queued", &blocked.name),
        json!([dependencies, limit])
    );
    assert_eq!(*reasons_of(&queue, "queued", &limited.name), json!([limit]));
    assert!(
        numbered(&queue).iter().all(|row| row["position"].is_null()),
        "a Session some reason holds was numbered: {queue}"
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_blocked_session_at_the_instance_limit_names_the_limit_rather_than_an_archival() {
    let kestrel = Kestrel::boot().await;
    at_the_instance_limit(&kestrel, 1, 1).await;
    let blocked = kestrel
        .enqueue_session(a_queued_workspace_in(&kestrel, "acme", "kestrel").await)
        .await;
    let never_ran = kestrel
        .enqueue_session(a_queued_workspace_in(&kestrel, "acme", "kestrel").await)
        .await;
    kestrel.block_session(&blocked, &never_ran).await;

    let queue = queue_read(&kestrel).await;
    assert_eq!(
        *reasons_of(&queue, "queued", &blocked.name),
        json!([
            { "kind": "dependencies", "sessions": [never_ran.name] },
            { "kind": "live_instance_limit", "limit": 1 },
        ])
    );
    assert_eq!(
        *reasons_of(&queue, "queued", &never_ran.name),
        json!([{ "kind": "instance_archiving", "instance": "docker/kestrel-0" }])
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_waiting_session_with_held_input_waits_on_full_slots_alone() {
    let kestrel = Kestrel::boot().await;
    sharing_a_serialized_profile(&kestrel, None, 1).await;
    let workspace = a_queued_workspace_in(&kestrel, "acme", "kestrel").await;
    let waiting = a_waiting_session(&kestrel, workspace).await;
    kestrel
        .dispatch_session(a_queued_workspace_in(&kestrel, "acme", "kestrel").await)
        .await;
    kestrel
        .post_while_busy(workspace, "jack", "one more thing")
        .await;

    let queue = queue_read(&kestrel).await;
    assert_eq!(
        waiting_row(&queue, &waiting.name)["reasons"],
        json!([{ "kind": "active_work_slots", "limit": 1 }])
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_ready_session_behind_full_slots_keeps_its_place() {
    let kestrel = Kestrel::boot().await;
    sharing_a_serialized_profile(&kestrel, None, 1).await;
    let working = kestrel
        .dispatch_session(a_queued_workspace_in(&kestrel, "acme", "kestrel").await)
        .await;
    let first = kestrel
        .enqueue_session(a_queued_workspace_in(&kestrel, "acme", "kestrel").await)
        .await;
    let second = kestrel
        .enqueue_session(a_queued_workspace_in(&kestrel, "acme", "kestrel").await)
        .await;

    let queue = queue_read(&kestrel).await;
    assert_eq!(
        queue["active_work"],
        json!({ "limit": 1, "occupied": 1, "occupants": [working.name], "elsewhere": 0 })
    );
    assert_eq!(
        numbered(&queue)
            .iter()
            .map(|row| (
                row["name"].clone(),
                row["position"].clone(),
                row["reasons"].clone()
            ))
            .collect::<Vec<_>>(),
        [
            (json!(first.name), json!(1), json!([])),
            (json!(second.name), json!(2), json!([])),
        ]
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn the_session_dispatch_claims_next_is_the_queues_first_position() {
    let kestrel = Kestrel::boot().await;
    sharing_a_serialized_profile(&kestrel, None, 8).await;
    let holding = kestrel
        .dispatch_session(jacks_workspace(&kestrel).await)
        .await;
    let blocker = kestrel
        .enqueue_session(a_queued_workspace_in(&kestrel, "acme", "kestrel").await)
        .await;
    let held = kestrel
        .enqueue_session(jacks_workspace(&kestrel).await)
        .await;
    let blocked = kestrel
        .enqueue_session(a_queued_workspace_in(&kestrel, "acme", "kestrel").await)
        .await;
    kestrel.block_session(&blocked, &blocker).await;
    let ready = kestrel
        .enqueue_session(a_queued_workspace_in(&kestrel, "acme", "kestrel").await)
        .await;

    let mut claimed = Vec::new();
    loop {
        let queue = queue_read(&kestrel).await;
        let first = numbered(&queue)
            .into_iter()
            .find(|row| row["position"] == 1)
            .map(|row| row["name"].as_str().expect("a name").to_owned());
        let next = match kestrel.occupy_up_to(8).await {
            Some(work::Occupied::Claimed(next)) => Some(next),
            Some(work::Occupied::Resumed(_)) => panic!("nothing waits to resume"),
            None => None,
        };
        assert_eq!(
            next.as_ref().map(|session| session.name.clone()),
            first,
            "dispatch disagreed with the queue: {queue}"
        );
        let Some(next) = next else { break };
        claimed.push(next);
        if claimed.len() == 2 {
            kestrel.complete_session(&holding).await;
            kestrel.complete_session(&claimed[0]).await;
        }
    }

    assert_eq!(
        claimed.iter().map(|session| session.id).collect::<Vec<_>>(),
        [blocker.id, ready.id, held.id, blocked.id]
    );

    kestrel.teardown().await;
}

async fn a_waiting_session(kestrel: &Kestrel, workspace: WorkspaceId) -> kestrel::domain::Session {
    let session = kestrel.dispatch_session(workspace).await;
    kestrel.waits_after_its_first_turn(&session).await
}

fn waiting_row<'a>(queue: &'a Value, name: &str) -> &'a Value {
    queue["waiting"]
        .as_array()
        .unwrap_or_else(|| panic!("the snapshot's waiting Sessions: {queue}"))
        .iter()
        .find(|row| row["name"] == name)
        .unwrap_or_else(|| panic!("no waiting Session {name}: {queue}"))
}

#[tokio::test]
async fn a_waiting_session_with_held_input_says_why_its_next_turn_has_not_started() {
    let kestrel = Kestrel::boot().await;
    sharing_a_serialized_profile(&kestrel, None, 2).await;
    let early = a_queued_workspace_in(&kestrel, "acme", "kestrel").await;
    let late = a_queued_workspace_in(&kestrel, "acme", "kestrel").await;
    let idle = a_queued_workspace_in(&kestrel, "acme", "kestrel").await;
    let early_session = a_waiting_session(&kestrel, early).await;
    let late_session = a_waiting_session(&kestrel, late).await;
    let idle_session = a_waiting_session(&kestrel, idle).await;
    kestrel.post_while_busy(early, "jack", "first").await;
    let queued = kestrel
        .enqueue_session(a_queued_workspace_in(&kestrel, "acme", "kestrel").await)
        .await;
    kestrel.post_while_busy(late, "jack", "second").await;

    let queue = queue_read(&kestrel).await;
    assert_eq!(
        queue["waiting"]
            .as_array()
            .expect("the waiting Sessions")
            .iter()
            .map(|row| row["name"].as_str().expect("a name"))
            .collect::<Vec<_>>(),
        [
            early_session.name.as_str(),
            late_session.name.as_str(),
            idle_session.name.as_str()
        ],
        "held input first, oldest first, then the rest"
    );
    assert!(waiting_row(&queue, &early_session.name)["pending_since"].is_string());
    assert_eq!(
        waiting_row(&queue, &early_session.name)["reasons"],
        json!([])
    );
    assert_eq!(
        waiting_row(&queue, &late_session.name)["reasons"],
        json!([{ "kind": "ahead", "sessions": [queued.name, early_session.name] }])
    );
    assert_eq!(
        waiting_row(&queue, &idle_session.name)["pending_since"],
        Value::Null
    );
    assert_eq!(
        waiting_row(&queue, &idle_session.name)["reasons"],
        json!([])
    );
    assert_eq!(
        numbered(&queue)
            .iter()
            .map(|row| row["name"].clone())
            .collect::<Vec<_>>(),
        [json!(queued.name)],
        "a Waiting Session was listed in the queue"
    );

    let order = [
        early_session.name.clone(),
        queued.name.clone(),
        late_session.name.clone(),
    ];
    for expected in order {
        let taken = match kestrel.occupy_up_to(8).await {
            Some(work::Occupied::Claimed(claimed)) => claimed,
            Some(work::Occupied::Resumed(resumed)) => resumed,
            None => panic!("a slot is free and {expected} is ahead"),
        };
        assert_eq!(taken.name, expected);
    }

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_waiting_session_with_held_input_waits_on_full_slots_and_on_its_profile() {
    let kestrel = Kestrel::boot().await;
    sharing_a_serialized_profile(&kestrel, None, 2).await;
    let jacks = jacks_workspace(&kestrel).await;
    let waiting = a_waiting_session(&kestrel, jacks).await;
    let holding = kestrel
        .dispatch_session(jacks_workspace(&kestrel).await)
        .await;
    kestrel
        .post_while_busy(jacks, "jack", "one more thing")
        .await;
    let profile =
        json!({ "kind": "subscription_profile", "profile": "jack", "session": holding.name });

    let queue = queue_read(&kestrel).await;
    assert_eq!(
        waiting_row(&queue, &waiting.name)["reasons"],
        json!([profile])
    );

    kestrel
        .dispatch_session(a_queued_workspace_in(&kestrel, "acme", "kestrel").await)
        .await;
    let queue = queue_read(&kestrel).await;
    assert_eq!(
        waiting_row(&queue, &waiting.name)["reasons"],
        json!([{ "kind": "active_work_slots", "limit": 2 }, profile])
    );
    assert!(kestrel.occupy_up_to(2).await.is_none());

    kestrel.teardown().await;
}

#[cfg(unix)]
#[tokio::test]
async fn a_session_that_failed_to_dispatch_shows_as_ended_and_not_as_waiting() {
    let kestrel = Kestrel::dispatching(supervisor::binary()).await;
    let organization = kestrel.declare_organization("acme").await;
    kestrel
        .declare_project(
            &organization,
            "kestrel",
            &["https://github.com/jtmthf/kestrel".to_owned()],
            "main",
        )
        .await;
    kestrel
        .declare_agent(&organization, "builder", "opencode", None)
        .await;
    let workspace = a_queued_workspace_in(&kestrel, "acme", "kestrel").await;
    let session = kestrel.enqueue_session(workspace).await;

    let ended = kestrel.answered(session.id, 1).await;
    assert_eq!(ended.state.as_str(), "ended");
    let queue = queue_read(&kestrel).await;
    assert_eq!(queue["queued"], json!([]), "{queue}");
    assert_eq!(queue["waiting"], json!([]), "{queue}");
    let (status, shown) = got(
        &kestrel,
        &format!("/operator/organizations/acme/sessions/{}", session.id),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{shown}");
    assert_eq!(shown["state"], "ended");
    assert_eq!(shown["exit"]["status"], "failed", "{shown}");

    kestrel.teardown().await;
}

/// A ready Session, one blocked on the Working holder of a serialized profile, and a Waiting
/// Session the profile holds back with input beside one only waiting for a turn.
struct EveryKind {
    holding: kestrel::domain::Session,
    ready: kestrel::domain::Session,
    blocked: kestrel::domain::Session,
    prompted: kestrel::domain::Session,
    waiting: kestrel::domain::Session,
}

async fn a_queue_of_every_kind(kestrel: &Kestrel) -> EveryKind {
    sharing_a_serialized_profile(kestrel, None, 4).await;
    let waiting = a_waiting_session(
        kestrel,
        a_queued_workspace_in(kestrel, "acme", "kestrel").await,
    )
    .await;
    let jacks = jacks_workspace(kestrel).await;
    let prompted = a_waiting_session(kestrel, jacks).await;
    let holding = kestrel
        .dispatch_session(jacks_workspace(kestrel).await)
        .await;
    kestrel
        .post_while_busy(jacks, "jack", "one more thing")
        .await;
    let ready = kestrel
        .enqueue_session(a_queued_workspace_in(kestrel, "acme", "kestrel").await)
        .await;
    let blocked = kestrel
        .enqueue_session(a_queued_workspace_in(kestrel, "acme", "kestrel").await)
        .await;
    kestrel.block_session(&blocked, &holding).await;

    EveryKind {
        holding,
        ready,
        blocked,
        prompted,
        waiting,
    }
}

#[tokio::test]
async fn kestrel_queue_says_each_reason_in_words_with_the_waiting_sessions_after_the_queue() {
    let kestrel = Kestrel::boot().await;
    let every = a_queue_of_every_kind(&kestrel).await;

    // Wide enough that no reason is truncated.
    let operator = kestrel.operator();
    let shown = tokio::task::spawn_blocking(move || {
        client::ran_on_a_terminal(&operator, &["queue", "--organization", "acme"], 240, "")
    })
    .await
    .expect("the client should run");
    assert!(shown.status.success(), "{}", shown.said);
    let lines: Vec<&str> = shown.said.lines().collect();
    let line_of = |name: &str| {
        lines
            .iter()
            .position(|line| line.contains(name))
            .unwrap_or_else(|| panic!("no line names {name}: {}", shown.said))
    };
    assert!(
        line_of(&every.ready.name) < line_of(&every.blocked.name)
            && line_of(&every.blocked.name) < line_of(&every.prompted.name)
            && line_of(&every.prompted.name) < line_of(&every.waiting.name),
        "{}",
        shown.said
    );
    for (session, said) in [
        (&every.ready, "ready".to_owned()),
        (&every.blocked, format!("waits on {}", every.holding.name)),
        (
            &every.prompted,
            format!(
                "the Subscription Profile jack is held by {}",
                every.holding.name
            ),
        ),
        (&every.waiting, "waiting for a turn".to_owned()),
    ] {
        let line = lines[line_of(&session.name)];
        assert!(line.contains(&said), "{line} does not say {said}");
    }
    assert!(lines[line_of(&every.prompted.name)].contains("input since "));

    kestrel.teardown().await;
}

#[tokio::test]
async fn kestrel_queue_json_agrees_with_the_operator_read() {
    let kestrel = Kestrel::boot().await;
    a_queue_of_every_kind(&kestrel).await;

    let queue = queue_read(&kestrel).await;
    let rows = recorded(
        &client(
            &kestrel,
            &[
                "queue",
                "--organization",
                "acme",
                "--json",
                "name,state,position,reasons,pending_since",
            ],
        )
        .await,
    );

    let mut read = Vec::new();
    for row in numbered(&queue) {
        read.push(json!({
            "name": row["name"],
            "state": "queued",
            "position": row["position"],
            "reasons": row["reasons"],
            "pending_since": null,
        }));
    }
    for row in queue["waiting"].as_array().expect("the waiting Sessions") {
        read.push(json!({
            "name": row["name"],
            "state": "waiting",
            "position": null,
            "reasons": row["reasons"],
            "pending_since": row["pending_since"],
        }));
    }
    assert_eq!(rows, read);

    kestrel.teardown().await;
}

#[test]
fn the_published_operator_document_describes_the_boundary_the_control_plane_serves() {
    let document = published();

    assert_eq!(document["openapi"], "3.1.0");
    assert_eq!(document["security"], serde_json::json!([]));

    let described: Vec<(String, String)> = document["paths"]
        .as_object()
        .expect("an object of paths")
        .iter()
        .flat_map(|(path, operations)| {
            operations
                .as_object()
                .expect("an object of operations")
                .keys()
                .map(|method| (path.clone(), method.clone()))
                .collect::<Vec<_>>()
        })
        .collect();

    let served = [
        (operator::ORGANIZATIONS, "get"),
        (operator::ORGANIZATIONS, "post"),
        (operator::STARTS, "post"),
        (operator::PROJECTS, "get"),
        (operator::PROJECTS, "post"),
        (operator::AGENTS, "get"),
        (operator::AGENTS, "post"),
        (operator::AGENT_MODEL, "put"),
        (operator::DECLARATION, "post"),
        (operator::DECLARATION_PREVIEW, "post"),
        (operator::CREDENTIALS, "get"),
        (operator::CREDENTIAL, "put"),
        (operator::CREDENTIAL, "delete"),
        (operator::PROFILES, "get"),
        (operator::PROFILES, "post"),
        (operator::PROFILE_VARIABLE, "put"),
        (operator::PROFILE_VARIABLE, "delete"),
        (operator::PROFILE_FILE, "put"),
        (operator::PROFILE_FILE, "delete"),
        (operator::INTEGRATIONS, "get"),
        (operator::INTEGRATIONS, "post"),
        (operator::EVENT_REFUSAL, "delete"),
        (operator::EVENTS, "get"),
        (operator::EVENT, "get"),
        (operator::WORKSPACES, "get"),
        (operator::WORKSPACES, "post"),
        (operator::WORKSPACE, "get"),
        (operator::WORKSPACE_MESSAGES, "post"),
        (operator::WORKSPACE_SEAL, "post"),
        (operator::WORKSPACE_INSTANCE_RELEASE, "post"),
        (operator::SESSIONS, "get"),
        (operator::SESSIONS, "post"),
        (operator::SESSION, "get"),
        (operator::SESSION_STOP, "post"),
        (operator::TRIGGERS, "get"),
        (operator::TRIGGERS, "post"),
        (operator::TRIGGER, "get"),
        (operator::TRIGGER_TEST, "post"),
        (operator::TRIGGER_DISABLE, "post"),
        (operator::TRIGGER_ENABLE, "post"),
        (operator::TRIGGER_DISPATCH, "post"),
        (operator::APPLIED_TRIGGERS, "post"),
        (operator::APPLIED_TRIGGERS_PREVIEW, "post"),
        (operator::INSTANCES, "get"),
        (operator::QUEUE, "get"),
        (operator::TRANSCRIPT, "get"),
    ];
    assert_eq!(
        described,
        served
            .iter()
            .map(|(path, method)| ((*path).to_owned(), (*method).to_owned()))
            .collect::<Vec<_>>()
    );
}

#[test]
fn the_published_operator_document_describes_every_transcript_entry() {
    let document = published();
    let mapping = document["components"]["schemas"]["Entry"]["discriminator"]["mapping"]
        .as_object()
        .expect("an object of entry kinds");

    let served = [
        Entry::ParticipantJoined {
            participant: "builder".to_owned(),
        },
        Entry::Brief {
            trigger: Some("sweep".to_owned()),
            brief: "Sweep the backlog".to_owned(),
        },
        Entry::SessionStarted {
            session: SessionId::generate(),
            agent: "builder".to_owned(),
        },
        Entry::Said {
            participant: "builder".to_owned(),
            message: "what the agent said".to_owned(),
        },
        Entry::Messages {
            messages: vec![Message {
                participant: "operator".to_owned(),
                message: "what arrived while it worked".to_owned(),
            }],
        },
        Entry::SessionEnded {
            session: SessionId::generate(),
            exit: Exit::Succeeded,
        },
        Entry::InstanceReleased {
            participant: "operator".to_owned(),
            instance: "docker/kestrel-01999cf2".to_owned(),
            unpublished: Some("https://github.com/acme/widgets has 1 untracked file".to_owned()),
        },
    ];

    let mut kinds: Vec<String> = Vec::new();
    for entry in served {
        let entry = serde_json::to_value(&entry).expect("an entry");
        let kind = entry["kind"].as_str().expect("a kind").to_owned();
        let schema = mapping
            .get(&kind)
            .unwrap_or_else(|| panic!("the document describes no {kind} entry"))
            .as_str()
            .expect("a reference");

        for field in resolve(&document, schema)["required"]
            .as_array()
            .expect("an array of required fields")
        {
            let field = field.as_str().expect("a named field");
            assert!(
                entry.get(field).is_some(),
                "the document requires {field} on a {kind} entry, and the boundary does not serve it"
            );
        }
        kinds.push(kind);
    }

    assert_eq!(kinds, mapping.keys().cloned().collect::<Vec<_>>());
}

fn published() -> Value {
    let document = support::crate_root().join("../../openapi/operator.json");

    serde_json::from_str(&fs::read_to_string(document).expect("a readable openapi document"))
        .expect("valid json")
}

/// The documented answer for this status must exist, and every field it requires must be served.
fn conforms(path: &str, method: &str, status: StatusCode, body: &Value) {
    let document = published();
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
        return;
    }
    requires(&document, schema, body);
}

fn requires(document: &Value, schema: &Value, body: &Value) {
    let schema = match schema["$ref"].as_str() {
        Some(reference) => resolve(document, reference),
        None => schema,
    };
    if let Some(options) = schema["anyOf"].as_array() {
        let option = options
            .iter()
            .find(|option| (option["type"] == "null") == body.is_null())
            .expect("the documented alternatives include the answer");
        requires(document, option, body);
        return;
    }
    if schema["type"] == "array" {
        for item in body.as_array().expect("an array, as documented") {
            requires(document, &schema["items"], item);
        }
        return;
    }

    for field in schema["required"]
        .as_array()
        .expect("an array of required fields")
    {
        let field = field.as_str().expect("a named field");
        assert!(
            body.get(field).is_some(),
            "the document requires {field}, and the boundary served {body}"
        );
    }
}

fn matches_template(template: &str, path: &str) -> bool {
    let (template, path): (Vec<_>, Vec<_>) =
        (template.split('/').collect(), path.split('/').collect());
    template.len() == path.len()
        && template
            .iter()
            .zip(&path)
            .all(|(step, given)| step.starts_with('{') || step == given)
}

fn resolve<'a>(document: &'a Value, reference: &str) -> &'a Value {
    reference
        .trim_start_matches("#/")
        .split('/')
        .fold(document, |document, step| &document[step])
}
