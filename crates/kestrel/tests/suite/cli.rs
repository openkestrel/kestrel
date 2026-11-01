//! CLI assertions use the operator boundary while work advances only at explicit passes.

mod fixture;
mod process;

use crate::support;
use fixture::{Booted, dispatched, refusal, succeeded};
use serde_json::{Value, json};
use std::time::Duration;
use support::client::{self, Finished, Invocation};
use support::github_stub::{self, GithubStub, ScriptedResponse};
use support::scripted_agent::Script;

async fn declared(kestrel: &Booted) {
    kestrel.run(&["organization", "declare", "acme"]).await;
    kestrel
        .run(&[
            "project",
            "declare",
            support::repository::NAME,
            "--repository",
            support::repository::url(),
            "--branch",
            support::repository::BRANCH,
        ])
        .await;
    kestrel
        .run(&[
            "agent",
            "declare",
            "builder",
            "--model",
            kestrel_scripted_agent::OTHER_MODEL,
        ])
        .await;
    kestrel
        .run_as(
            &["credential", "set", support::PROVIDER_KEY],
            Invocation::default().given(support::A_PROVIDER_KEY),
        )
        .await;
}

async fn opened(kestrel: &Booted) -> String {
    let opened = kestrel
        .record(&[
            "workspace",
            "open",
            "--project",
            support::repository::NAME,
            "--agent",
            "builder",
            "--json",
        ])
        .await;

    opened["workspace"]["name"]
        .as_str()
        .expect("the opened workspace's generated name")
        .to_owned()
}

#[tokio::test]
async fn a_session_show_says_the_title_its_options_and_its_commands() {
    let booted = Booted::playing(Script::Announces).await;
    declared(&booted).await;
    let workspace = opened(&booted).await;
    booted
        .run(&[
            "workspace",
            "post",
            &workspace,
            "--as-participant",
            "operator",
            "go",
        ])
        .await;

    booted.plane.work_pass().await;
    let listed = booted
        .until(
            &["session", "list", "--workspace", &workspace, "--json"],
            |listed| listed.iter().any(|session| !session["title"].is_null()),
            "show the harness's title",
        )
        .await;
    let session = listed
        .iter()
        .find(|session| !session["title"].is_null())
        .and_then(|session| session["id"].as_str())
        .expect("the session's identifier")
        .to_owned();

    let shown = booted.run(&["session", "show", &session]).await;

    assert!(
        shown.contains(kestrel_scripted_agent::TITLE),
        "the title is not shown:\n{shown}"
    );
    assert!(
        shown.contains("model: scripted-max"),
        "the model option's current value is not shown as `category: current`:\n{shown}"
    );
    assert!(
        shown.contains("mode: plan"),
        "the mode option's current value is not shown:\n{shown}"
    );
    assert!(
        shown.contains("compact"),
        "the commands are not shown:\n{shown}"
    );

    booted.plane.teardown().await;
}

#[tokio::test]
async fn a_session_option_set_changes_an_option_and_warns_about_the_cache() {
    let booted = Booted::playing(Script::Speaks).await;
    declared(&booted).await;
    let workspace = opened(&booted).await;
    booted
        .run(&[
            "workspace",
            "post",
            &workspace,
            "--as-participant",
            "operator",
            "go",
        ])
        .await;

    booted.plane.work_pass().await;
    let waiting = booted
        .until(
            &["session", "list", "--workspace", &workspace, "--json"],
            |listed| listed.iter().any(|session| session["state"] == "waiting"),
            "reach a waiting session",
        )
        .await;
    let session = waiting
        .iter()
        .find(|session| session["state"] == "waiting")
        .and_then(|session| session["id"].as_str())
        .expect("the waiting session's identifier")
        .to_owned();
    // The bookkeeping report is debounced, so the option is waited for before it is changed.
    booted
        .until(
            &["session", "show", &session, "--json"],
            |shown| {
                shown[0]["options"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .any(|option| option["category"] == "model")
            },
            "report its options",
        )
        .await;

    let changed = booted
        .client(&[
            "session",
            "option",
            "set",
            &session,
            "model",
            kestrel_scripted_agent::OTHER_MODEL,
            "--as-participant",
            "operator",
        ])
        .await;
    assert!(
        changed.status.success(),
        "`session option set` failed:\n{}",
        changed.err
    );
    assert!(
        changed.err.contains("1200"),
        "the cache warning did not name the context it re-reads:\n{}",
        changed.err
    );

    let settled = booted
        .until(
            &["session", "show", &session, "--json"],
            |shown| {
                let shown = &shown[0];
                shown["changing_options"]
                    .as_array()
                    .is_some_and(Vec::is_empty)
                    && shown["options"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .any(|option| {
                            option["category"] == "model"
                                && option["current"] == kestrel_scripted_agent::OTHER_MODEL
                        })
            },
            "apply the changed model",
        )
        .await;
    assert_eq!(
        settled[0]["changing_options"].as_array().map(Vec::len),
        Some(0)
    );

    booted.plane.teardown().await;
}

#[tokio::test]
async fn workspace_open_declares_the_mode_its_first_session_runs_in() {
    let booted = Booted::playing(Script::Speaks).await;
    declared(&booted).await;
    let workspace = booted
        .record(&[
            "workspace",
            "open",
            "--project",
            support::repository::NAME,
            "--agent",
            "builder",
            "--mode",
            kestrel_scripted_agent::SWITCHED_MODE,
            "--brief",
            "go",
            "--as-participant",
            "operator",
            "--json",
        ])
        .await["workspace"]["name"]
        .as_str()
        .expect("the opened workspace's name")
        .to_owned();

    booted.plane.work_pass().await;
    let listed = booted
        .until(
            &["session", "list", "--workspace", &workspace, "--json"],
            |listed| listed.iter().any(|session| session["state"] == "waiting"),
            "answer its first turn",
        )
        .await;
    let session = listed[0]["id"].as_str().expect("the session's identifier");
    let shown: Value =
        serde_json::from_str(&booted.run(&["session", "show", session, "--json"]).await)
            .expect("the session's fields as JSON");

    assert_eq!(
        shown["mode"],
        kestrel_scripted_agent::SWITCHED_MODE,
        "the mode the open declared was not set on the harness: {shown}"
    );

    booted.plane.teardown().await;
}

#[tokio::test]
async fn a_role_boots_on_an_empty_data_directory_and_makes_its_database() {
    let booted = Booted::playing(Script::Speaks).await;

    assert!(booted.plane.data_dir().join("kestrel.db").exists());
    assert_eq!(booted.run(&["organization", "list"]).await, "");
}

#[tokio::test]
async fn a_disabled_trigger_shows_its_reason_and_budget() {
    let booted = Booted::playing(Script::Speaks).await;
    declared(&booted).await;
    booted
        .run(&[
            "trigger",
            "declare",
            "ready",
            "--filter",
            r#"{"exact": {"type": "com.github.issues.labeled"}}"#,
            "--brief",
            "Work on {{ event.data.issue.title }}",
            "--project",
            "kestrel",
            "--agent",
            "builder",
        ])
        .await;
    booted.run(&["trigger", "disable", "ready"]).await;

    let trigger = booted.record(&["trigger", "show", "ready", "--json"]).await;

    assert_eq!(trigger["state"], "disabled:operator");
    assert_eq!(trigger["disabled_because"], "disabled by an operator");
    assert_eq!(trigger["firing_budget"]["limit"], 10);
}

#[tokio::test]
async fn an_agents_model_changes_without_declaring_the_agent_again() {
    let booted = Booted::playing(Script::Speaks).await;
    booted.run(&["organization", "declare", "acme"]).await;
    booted
        .run(&[
            "agent",
            "declare",
            "builder",
            "--harness",
            "codex",
            "--model",
            "claude-opus-5",
        ])
        .await;

    assert_eq!(
        booted
            .run(&["agent", "model", "builder", "--model", "claude-sonnet-5"])
            .await,
        "claude-sonnet-5"
    );
    let agents = booted.records(&["agent", "list", "--json"]).await;
    assert_eq!(agents.len(), 1, "{agents:?}");
    assert_eq!(agents[0]["name"], "builder");
    assert_eq!(agents[0]["harness"], "codex");
    assert_eq!(agents[0]["model"], "claude-sonnet-5");
    assert_eq!(
        booted
            .record(&["agent", "model", "builder", "--json"])
            .await["model"],
        Value::Null
    );
    assert!(
        booted
            .refused(&["agent", "model", "reviewer", "--model", "claude-sonnet-5"])
            .await
            .contains("no agent named reviewer"),
    );
}

#[tokio::test]
async fn an_instance_is_shown_on_its_workspace_and_released_on_the_record() {
    let booted = Booted::playing(Script::Speaks).await;
    declared(&booted).await;
    let workspace = opened(&booted).await;
    booted
        .run(&[
            "workspace",
            "post",
            &workspace,
            "--as-participant",
            "operator",
            "go",
        ])
        .await;
    dispatched(&booted, &workspace).await;

    let shown = booted
        .record(&["workspace", "show", &workspace, "--json"])
        .await;
    let instance = shown["instance"]
        .as_str()
        .expect("the workspace keeps its instance")
        .to_owned();
    assert_eq!(
        shown["held"],
        Value::Null,
        "a checkout the remote can restore was held"
    );
    assert_eq!(booted.run(&["instance", "list"]).await, "");

    assert_eq!(
        booted
            .run(&[
                "instance",
                "release",
                &workspace,
                "--as-participant",
                "operator"
            ])
            .await,
        instance
    );

    assert_eq!(
        booted
            .record(&["workspace", "show", &workspace, "--json"])
            .await["instance"],
        Value::Null,
        "a released instance is still the workspace's"
    );
    let transcript = booted
        .records(&[
            "workspace",
            "transcript",
            &workspace,
            "--no-summaries",
            "--json",
        ])
        .await;
    let released = transcript.last().expect("the release is on the record");
    assert_eq!(released["seq"], 11);
    assert_eq!(released["entry"]["type"], "instance_released");
    assert_eq!(released["entry"]["participant"], "operator");
    assert_eq!(released["entry"]["instance"], instance);
    assert!(
        booted
            .refused(&[
                "instance",
                "release",
                &workspace,
                "--as-participant",
                "operator"
            ])
            .await
            .contains("no instance"),
    );
}

#[tokio::test]
async fn a_workspace_post_without_a_participant_or_an_operator_says_how_to_name_one() {
    let booted = Booted::playing(Script::Speaks).await;
    declared(&booted).await;
    let workspace = opened(&booted).await;

    let refused = booted
        .refused(&["workspace", "post", &workspace, "go"])
        .await;

    assert!(
        refused.contains("kestrel operator name <NAME>"),
        "a post nobody signed does not say how to name the Operator: {refused}"
    );
}

#[tokio::test]
async fn a_held_message_is_printed_listed_edited_and_withdrawn() {
    let booted = Booted::playing(Script::WorksUntilCancelled).await;
    declared(&booted).await;
    let workspace = opened(&booted).await;
    booted
        .run(&[
            "workspace",
            "post",
            &workspace,
            "--as-participant",
            "operator",
            "go",
        ])
        .await;
    booted.plane.work_pass().await;
    booted
        .until(
            &["session", "list", "--workspace", &workspace, "--json"],
            |listed| listed.iter().any(|session| session["state"] == "working"),
            "reach a working session",
        )
        .await;

    let held = booted
        .run(&[
            "workspace",
            "post",
            &workspace,
            "--as-participant",
            "alice",
            "one more change",
        ])
        .await;
    let id: i64 = held.parse().expect("the post prints the held id");
    let listed = booted
        .record(&["workspace", "show", &workspace, "--json"])
        .await;
    assert_eq!(listed["held_messages"][0]["id"], id);
    assert_eq!(listed["held_messages"][0]["participant"], "alice");
    assert_eq!(listed["held_messages"][0]["message"], "one more change");

    booted.plane.advance(Duration::from_millis(1)).await;
    let edited = booted
        .record(&[
            "workspace",
            "message",
            "edit",
            &workspace,
            &held,
            "--as-participant",
            "alice",
            "the edited change",
            "--json",
        ])
        .await;
    assert_eq!(edited["id"], id);
    assert_eq!(edited["message"], "the edited change");
    assert!(!edited["edited_at"].is_null(), "{edited}");

    let refused = booted
        .refused(&[
            "workspace",
            "message",
            "edit",
            &workspace,
            &held,
            "--as-participant",
            "bob",
            "mine now",
        ])
        .await;
    assert!(
        refused.contains("only its author may change it"),
        "{refused}"
    );

    booted
        .run(&[
            "workspace",
            "message",
            "withdraw",
            &workspace,
            &held,
            "--as-participant",
            "alice",
        ])
        .await;
    assert_eq!(
        booted
            .record(&["workspace", "show", &workspace, "--json"])
            .await["held_messages"],
        serde_json::json!([])
    );
    let refused = booted
        .refused(&[
            "workspace",
            "message",
            "withdraw",
            &workspace,
            &held,
            "--as-participant",
            "alice",
        ])
        .await;
    assert!(refused.contains("already withdrawn"), "{refused}");
}

#[tokio::test]
async fn a_session_interrupt_names_who_asked_and_a_waiting_one_is_refused() {
    let booted = Booted::playing(Script::WorksUntilCancelled).await;
    declared(&booted).await;
    // Opened with a Brief, so the session's first turn is its instruction and nothing races it.
    let workspace = booted
        .record(&[
            "workspace",
            "open",
            "--project",
            support::repository::NAME,
            "--agent",
            "builder",
            "--brief",
            "go",
            "--json",
        ])
        .await["workspace"]["name"]
        .as_str()
        .expect("the opened workspace's name")
        .to_owned();
    booted.plane.work_pass().await;
    let listed = booted
        .until(
            &["session", "list", "--workspace", &workspace, "--json"],
            |listed| listed.iter().any(|session| session["state"] == "working"),
            "reach a working session",
        )
        .await;
    let session = listed[0]["id"]
        .as_str()
        .expect("the working session's id")
        .to_owned();

    let interrupted = booted
        .record(&[
            "session",
            "interrupt",
            &session,
            "--as-participant",
            "alice",
            "--json",
        ])
        .await;
    assert_eq!(interrupted["id"], session);
    assert_eq!(interrupted["state"], "working");
    assert_eq!(interrupted["interrupting"]["participant"], "alice");

    let waiting = booted
        .until(
            &["session", "show", &session, "--json"],
            |shown| shown.iter().any(|session| session["state"] == "waiting"),
            "settle waiting",
        )
        .await;
    assert_eq!(waiting[0]["state"], "waiting");
    assert!(
        booted
            .refused(&[
                "session",
                "interrupt",
                &session,
                "--as-participant",
                "alice"
            ])
            .await
            .contains("waiting"),
    );

    let entries = booted
        .records(&[
            "workspace",
            "transcript",
            &workspace,
            "--kinds",
            "shared_state,narration,detail",
            "--json",
        ])
        .await;
    assert!(
        entries.iter().any(|recorded| {
            recorded["entry"]["type"] == "turn_interrupted"
                && recorded["entry"]["participant"] == "alice"
        }),
        "{entries:?}"
    );
}

#[tokio::test]
async fn a_session_ends_succeeded_while_waiting_and_is_not_stopped_twice() {
    let booted = Booted::playing(Script::Speaks).await;
    declared(&booted).await;
    let workspace = opened(&booted).await;
    booted
        .run(&[
            "workspace",
            "post",
            &workspace,
            "--as-participant",
            "operator",
            "go",
        ])
        .await;

    let listed = dispatched(&booted, &workspace).await;
    let session = listed[0]["id"]
        .as_str()
        .expect("the session's id")
        .to_owned();

    assert_eq!(listed[0]["exit"]["status"], "succeeded");
    assert!(
        listed[0]["instance"]
            .as_str()
            .is_some_and(|instance| instance.starts_with("local-exec/")),
        "the session does not list the instance it executed on: {listed:?}"
    );
    assert_eq!(
        listed[0]["worked_model"],
        kestrel_scripted_agent::OTHER_MODEL
    );
    assert!(
        booted
            .refused(&["session", "stop", &session])
            .await
            .contains("has already ended"),
    );
}

/// A Client hands the control plane every secret it holds over the operator boundary, and
/// the control plane says none of them back while it takes them.
#[tokio::test]
async fn secrets_set_through_the_client_appear_in_no_log_line() {
    let stub = GithubStub::start();
    let booted = Booted::traced().await;
    let provider_key = "sk-kestrel-should-never-say-this-either";
    let signing_secret = "whsec-kestrel-should-never-say-this";

    let mut printed = booted.run(&["organization", "declare", "acme"]).await;
    printed += &booted
        .run_as(
            &["credential", "set", "ANTHROPIC_API_KEY"],
            Invocation::default().given(provider_key),
        )
        .await;
    let registering = [
        "integration",
        "github",
        "register",
        "hub",
        "--repository",
        "jtmthf/kestrel",
        "--app-id",
        "1",
        "--installation",
        "2",
        "--private-key",
        support::PRIVATE_KEY,
        "--api",
        &stub.base_url(),
        "--webhook-secret",
        signing_secret,
    ];
    printed += &booted.run(&registering).await;
    printed += &booted.refused(&registering).await;
    printed += &booted.run(&["credential", "list"]).await;
    printed += &booted.run(&["integration", "list"]).await;
    let said = booted.said();
    booted.plane.teardown().await;

    assert!(
        said.contains("INSERT INTO provider_credential")
            && said.contains("INSERT INTO integration"),
        "the control plane logged nothing of what it was asked to hold:\n{said}"
    );
    for (name, secret) in [
        ("the provider key", provider_key),
        ("the private key", support::PRIVATE_KEY),
        ("the signing secret", signing_secret),
    ] {
        assert!(!printed.contains(secret), "the client printed {name}");
        assert!(!said.contains(secret), "a log line spelled {name} out");
    }
}

async fn watching(kestrel: &Booted, stub: &GithubStub, interval: &str) {
    kestrel.run(&["organization", "declare", "acme"]).await;
    kestrel
        .run(&[
            "integration",
            "github",
            "register",
            "hub",
            "--repository",
            "jtmthf/kestrel",
            "--app-id",
            "1",
            "--installation",
            "2",
            "--private-key",
            support::PRIVATE_KEY,
            "--api",
            &stub.base_url(),
            "--interval",
            interval,
        ])
        .await;
}

#[tokio::test]
async fn the_client_selects_an_integration_for_a_projects_repository() {
    const DECLARE: [&str; 9] = [
        "project",
        "declare",
        "web",
        "--repository",
        "git@github.com:jtmthf/kestrel.git",
        "--integration",
        "hub",
        "--branch",
        "main",
    ];
    let stub = GithubStub::start();
    let booted = Booted::playing(Script::Speaks).await;
    watching(&booted, &stub, "1h").await;

    let refused = booted.refused(&DECLARE).await;
    assert!(refused.contains("has no branch main"), "{refused}");
    assert!(
        booted
            .records(&["project", "list", "--json"])
            .await
            .is_empty()
    );

    stub.script_answer(
        "GET",
        "/branches/main",
        ScriptedResponse::ok(r#"{"name":"main"}"#),
    );
    let declared = booted.record(&[&DECLARE[..], &["--json"]].concat()).await;
    assert_eq!(
        declared["repositories"],
        serde_json::json!(["https://github.com/jtmthf/kestrel.git"])
    );
    assert_eq!(declared["integrations"][0]["integration"], "hub");

    let unpaired = booted
        .refused(&[
            "project",
            "declare",
            "two",
            "--repository",
            "jtmthf/kestrel",
            "--repository",
            "jtmthf/other",
            "--integration",
            "hub",
            "--branch",
            "main",
        ])
        .await;
    assert!(unpaired.contains("REPOSITORY=hub"), "{unpaired}");
}

#[tokio::test]
async fn the_client_reads_repository_metadata_and_pagination_through_its_selected_integration() {
    let stub = GithubStub::start();
    let booted = Booted::playing(Script::Speaks).await;
    watching(&booted, &stub, "1h").await;
    let page = serde_json::json!({
        "total_count": 3,
        "repositories": [{
            "id": 1, "full_name": "jtmthf/kestrel", "default_branch": "trunk", "private": true,
        }],
    });
    stub.script_answer(
        "GET",
        "/installation/repositories",
        ScriptedResponse::ok(page.to_string()),
    );
    let args = [
        "integration",
        "repositories",
        "hub",
        "--page",
        "2",
        "--per-page",
        "1",
    ];
    let finished = booted.client(&[&args[..], &["--json"]].concat()).await;
    assert!(finished.status.success(), "{}", finished.err);
    let listed = finished.json();
    assert_eq!(listed["repositories"][0]["default_branch"], "trunk");
    assert_eq!(listed["repositories"][0]["watched"], true);
    assert_eq!(listed["page"], 2);
    assert_eq!(listed["total"], 3);
    assert_eq!(listed["next_page"], 3);
    assert!(
        stub.requests()
            .iter()
            .any(|request| { request.url == "/installation/repositories?per_page=1&page=2" })
    );
    stub.script_answer(
        "GET",
        "/installation/repositories",
        ScriptedResponse::ok(page.to_string()),
    );
    let shown = booted.run(&args).await;
    assert!(
        shown.contains("trunk") && shown.contains("page 3"),
        "{shown}"
    );
    stub.script_answer(
        "GET",
        "/installation/repositories",
        github_stub::rate_limited(),
    );
    let refused = booted.refused(&args).await;
    assert!(
        refused.contains("retry") || refused.contains("again"),
        "{refused}"
    );
}

#[tokio::test]
async fn the_client_retires_an_integration_only_on_an_explicit_choice() {
    let stub = GithubStub::start();
    let booted = Booted::playing(Script::Speaks).await;
    watching(&booted, &stub, "1h").await;

    let unchosen = booted.refused(&["integration", "retire", "hub"]).await;
    assert!(unchosen.contains("--yes"), "{unchosen}");
    assert_eq!(
        booted
            .record(&["integration", "show", "hub", "--json"])
            .await["state"],
        "enabled"
    );

    let retired = booted
        .record(&["integration", "retire", "hub", "--yes", "--json"])
        .await;
    assert_eq!(retired["state"], "retired");
    assert_eq!(retired["repository"], "jtmthf/kestrel");
    assert!(!retired.to_string().contains(support::PRIVATE_KEY));

    let shown = booted.run(&["integration", "show", "hub"]).await;
    assert!(shown.contains("retired"), "{shown}");
    assert!(
        !shown.contains("integration enable"),
        "a retired integration offered a way back:\n{shown}"
    );
    let refused = booted.refused(&["integration", "enable", "hub"]).await;
    assert!(refused.contains("retired"), "{refused}");
}

#[tokio::test]
async fn the_client_disables_changes_and_enables_an_integration() {
    let stub = GithubStub::start();
    let booted = Booted::playing(Script::Speaks).await;
    watching(&booted, &stub, "1h").await;

    let disabled = booted.run(&["integration", "disable", "hub"]).await;
    assert!(disabled.contains("disabled"), "{disabled}");
    assert!(
        disabled.contains("kestrel integration enable hub --organization acme"),
        "the disable named no way to resume:\n{disabled}"
    );

    let changed = booted
        .record(&[
            "integration",
            "change",
            "hub",
            "--rename",
            "github",
            "--carries",
            "outbound",
            "--json",
        ])
        .await;
    assert_eq!(changed["name"], "github");
    assert_eq!(changed["carries"], serde_json::json!(["outbound"]));
    assert_eq!(changed["state"], "disabled");
    assert_eq!(changed["repository"], "jtmthf/kestrel");
    let fixed = booted
        .refused(&[
            "integration",
            "change",
            "github",
            "--repository",
            "jtmthf/other",
        ])
        .await;
    assert!(fixed.contains("--repository"), "{fixed}");

    let enabled = booted
        .record(&["integration", "enable", "github", "--json"])
        .await;
    booted.plane.teardown().await;

    assert_eq!(enabled["state"], "enabled");
    assert_eq!(enabled["diagnostic"], Value::Null);
}

#[tokio::test]
async fn the_client_replaces_a_private_key_from_a_file_or_standard_input_and_never_says_it() {
    let stub = GithubStub::start();
    let booted = Booted::traced().await;
    watching(&booted, &stub, "1h").await;
    let before = booted
        .record(&["integration", "show", "hub", "--json"])
        .await;

    stub.script_answer("POST", "/access_tokens", ScriptedResponse::answering(401));
    let mut printed = booted
        .refused_as(
            &["integration", "github", "replace-key", "hub"],
            Invocation::default().given(support::REPLACEMENT_PRIVATE_KEY),
        )
        .await;
    assert!(printed.contains("private_key"), "{printed}");
    stub.script_answer("POST", "/access_tokens", ScriptedResponse::answering(502));
    let unanswered = booted
        .refused_as(
            &["integration", "github", "replace-key", "hub"],
            Invocation::default().given(support::REPLACEMENT_PRIVATE_KEY),
        )
        .await;
    assert!(
        unanswered.contains("kestrel integration github replace-key hub --organization acme"),
        "an outage named no way to try again:\n{unanswered}"
    );
    printed += &unanswered;
    assert_eq!(
        booted
            .record(&["integration", "show", "hub", "--json"])
            .await["revision"],
        before["revision"]
    );

    let from_input = booted
        .client_as(
            &["integration", "github", "replace-key", "hub", "--json"],
            Invocation::default().given(support::REPLACEMENT_PRIVATE_KEY),
        )
        .await
        .json();
    let from_file = booted
        .client_as(
            &[
                "integration",
                "github",
                "replace-key",
                "hub",
                "--private-key-file",
                "app.pem",
                "--json",
            ],
            Invocation::default().file("app.pem", support::PRIVATE_KEY),
        )
        .await
        .json();
    printed += &format!("{from_input}{from_file}");
    printed += &booted.run(&["integration", "show", "hub"]).await;
    let said = booted.said();
    booted.plane.teardown().await;

    assert!(from_input["revision"].as_i64() > before["revision"].as_i64());
    assert!(from_file["revision"].as_i64() > from_input["revision"].as_i64());
    assert_eq!(from_file["id"], before["id"]);
    for key in [support::PRIVATE_KEY, support::REPLACEMENT_PRIVATE_KEY] {
        assert!(!printed.contains(key), "the client printed a private key");
        assert!(!said.contains(key), "a log line spelled a private key out");
    }
    for said in [&printed, &said] {
        assert!(!said.contains("PRIVATE KEY"), "{said}");
        assert!(!said.contains(github_stub::INSTALLATION_TOKEN), "{said}");
    }
}

#[tokio::test]
async fn the_client_starts_reads_and_resumes_a_webhook_secret_rotation_and_never_says_a_secret() {
    const FIRST: &str = "the-first-webhook-secret";
    let stub = GithubStub::start();
    let booted = Booted::traced().await;
    booted.run(&["organization", "declare", "acme"]).await;
    booted
        .run(&[
            "integration",
            "github",
            "register",
            "hub",
            "--repository",
            "jtmthf/kestrel",
            "--app-id",
            "1",
            "--installation",
            "2",
            "--private-key",
            support::PRIVATE_KEY,
            "--webhook-secret",
            FIRST,
            "--api",
            &stub.base_url(),
            "--interval",
            "1h",
        ])
        .await;

    stub.script_answer(
        "PATCH",
        "/app/hook/config",
        ScriptedResponse::answering(502),
    );
    let mut printed = booted
        .refused_as(
            &["integration", "github", "rotate-secret", "start", "hub"],
            Invocation::default(),
        )
        .await;
    assert!(
        printed.contains("kestrel integration github rotate-secret resume hub --organization acme"),
        "an unfinished rotation named no way to finish it:\n{printed}"
    );
    let status = booted
        .run(&["integration", "github", "rotate-secret", "status", "hub"])
        .await;
    assert!(status.contains("unfinished since"), "{status}");
    assert!(
        status.contains("kestrel integration github rotate-secret resume hub"),
        "{status}"
    );
    printed += &status;
    let shown = booted.run(&["integration", "show", "hub"]).await;
    assert!(shown.contains("unfinished since"), "{shown}");
    printed += &shown;
    let unfinished = booted
        .record(&[
            "integration",
            "github",
            "rotate-secret",
            "status",
            "hub",
            "--json",
        ])
        .await;
    assert!(
        unfinished["webhook_secret_rotation"]["started_at"].is_string(),
        "{unfinished}"
    );
    printed += &unfinished.to_string();

    let resumed = booted
        .record(&[
            "integration",
            "github",
            "rotate-secret",
            "resume",
            "hub",
            "--json",
        ])
        .await;
    assert_eq!(resumed["webhook_secret_rotation"], Value::Null);
    let finished = booted
        .run(&["integration", "github", "rotate-secret", "status", "hub"])
        .await;
    assert!(finished.contains("none under way"), "{finished}");
    printed += &format!("{resumed}{finished}");
    let said = booted.said();
    booted.plane.teardown().await;

    let secrets: Vec<String> = stub
        .requests()
        .iter()
        .filter(|request| request.method == "PATCH")
        .map(|request| {
            serde_json::from_str::<Value>(&request.body).expect("JSON")["secret"]
                .as_str()
                .expect("a secret")
                .to_owned()
        })
        .collect();
    assert_eq!(secrets.len(), 2);
    assert_eq!(secrets[0], secrets[1]);
    for secret in [FIRST, secrets[0].as_str()] {
        assert!(
            !printed.contains(secret),
            "the client printed a webhook secret"
        );
        assert!(
            !said.contains(secret),
            "a log line spelled a webhook secret out"
        );
    }
}

/// The one command that has a credential in it, and the whole of what kestrel says while it
/// uses it: neither the listing an operator reads nor the log they debug from has the token.
#[tokio::test]
async fn the_client_lists_what_a_poll_recorded_and_the_credential_appears_in_neither_it_nor_a_log()
{
    let stub = GithubStub::start();
    let delivery = stub.deliver(github_stub::labelled(43, "ready-for-agent"));
    let booted = Booted::traced().await;
    watching(&booted, &stub, "1ms").await;

    booted.plane.work_pass().await;
    let listed = booted.records(&["event", "list", "--json"]).await;
    assert_eq!(
        listed.len(),
        1,
        "the pass should record the delivered Event"
    );
    let said = booted.said();
    let record = listed[0]["record"].as_str().expect("an event record");
    let shown = booted.record(&["event", "show", record, "--json"]).await;
    booted.plane.teardown().await;

    assert_eq!(shown["record"], record);
    assert_eq!(shown["event"]["id"], delivery);
    assert_eq!(shown["event"]["specversion"], "1.0");
    assert_eq!(shown["event"]["subject"], "#43");
    let listed = serde_json::to_string(&listed).expect("the listing serializes");
    assert!(
        listed.contains("ready-for-agent"),
        "an event listing that does not say what happened:\n{listed}"
    );
    assert!(
        !listed.contains(support::PRIVATE_KEY),
        "the listing spelled the credential out"
    );
    assert!(
        !said.contains(support::PRIVATE_KEY),
        "a log line spelled the credential out"
    );
    assert!(
        said.contains("a poll recorded events"),
        "the control plane never said it polled:\n{said}"
    );
}

#[tokio::test]
async fn a_dispatch_starts_a_triggers_work_on_the_issue_it_names() {
    let stub = GithubStub::start();
    stub.script_answer("GET", "/issues/60", github_stub::issue(60, &[]));
    let booted = Booted::playing(Script::Speaks).await;
    declared(&booted).await;
    booted
        .run(&[
            "integration",
            "github",
            "register",
            "hub",
            "--repository",
            "jtmthf/kestrel",
            "--app-id",
            "1",
            "--installation",
            "2",
            "--private-key",
            support::PRIVATE_KEY,
            "--api",
            &stub.base_url(),
            "--carries",
            "outbound",
        ])
        .await;
    booted
        .run(&[
            "trigger",
            "declare",
            "delegated",
            "--filter",
            r#"{"exact": {"type": "com.github.issue_comment.created"}}"#,
            "--brief",
            "{{ instruction }} {{ event.data.issue.number }}",
            "--branch",
            "kestrel/issue-{{ event.data.issue.number }}",
            "--project",
            "kestrel",
            "--agent",
            "builder",
        ])
        .await;

    let tested = booted
        .record(&[
            "trigger",
            "test",
            "delegated",
            "--integration",
            "hub",
            "--issue",
            "60",
            "--instruction",
            "/tdd the parser",
            "--json",
        ])
        .await;
    assert!(
        booted
            .records(&["event", "list", "--json"])
            .await
            .is_empty()
    );

    let fired = booted
        .record(&[
            "trigger",
            "dispatch",
            "delegated",
            "--integration",
            "hub",
            "--issue",
            "60",
            "--instruction",
            "/tdd the parser",
            "--json",
        ])
        .await;

    assert_eq!(tested["matches"], true, "{tested}");
    assert_eq!(tested["brief"], "/tdd the parser 60", "{tested}");
    assert_eq!(tested["branch"], "kestrel/issue-60", "{tested}");
    assert_eq!(tested["agent"], "builder", "{tested}");
    assert_eq!(fired["outcome"], "opened");
    let workspace = fired["workspace"]
        .as_str()
        .expect("the workspace it opened");
    let shown = booted
        .record(&["workspace", "show", workspace, "--json"])
        .await;
    assert_eq!(shown["checkout"]["branch"], "kestrel/issue-60");
    for misused in [
        &["trigger", "test", "delegated", "--issue", "60"][..],
        &[
            "trigger",
            "test",
            "delegated",
            "--integration",
            "hub",
            "--issue",
            "60",
            "--event",
            "x",
        ],
    ] {
        assert_eq!(
            booted.client(misused).await.status.code(),
            Some(2),
            "`kestrel {}` is a usage error",
            misused.join(" ")
        );
    }
    assert!(
        booted
            .refused(&[
                "trigger",
                "dispatch",
                "delegated",
                "--integration",
                "nowhere",
                "--issue",
                "60",
            ])
            .await
            .contains("no integration named nowhere"),
    );
}

const APPLIED: &str = r#"
triggers:
  ready:
    filter: {exact: {type: com.github.issues.labeled}}
    brief: Work on {{ event.data.issue.title }}
    project: kestrel
    agent: builder
  triage:
    filter:
      all:
        - exact: {type: com.github.issues.opened}
        - exact: {data.issue.author_association: MEMBER}
    brief: Triage {{ event.data.issue.title }}
    project: kestrel
    agent: builder
"#;

async fn applying(kestrel: &Booted, declarations: &str, flags: &[&str]) -> Finished {
    let mut args = vec!["trigger", "apply", "-f", "triggers.yaml"];
    args.extend_from_slice(flags);
    kestrel
        .client_as(
            &args,
            Invocation::default().file("triggers.yaml", declarations),
        )
        .await
}

/// What it printed, and what it warned of.
async fn applied(kestrel: &Booted, declarations: &str, flags: &[&str]) -> (String, String) {
    let finished = applying(kestrel, declarations, flags).await;
    let diff = succeeded(&["trigger", "apply"], &finished);
    (diff, finished.err)
}

async fn trigger_names(kestrel: &Booted) -> Vec<String> {
    kestrel
        .records(&["trigger", "list", "--json"])
        .await
        .iter()
        .map(|trigger| trigger["name"].as_str().expect("a name").to_owned())
        .collect()
}

#[tokio::test]
async fn apply_prints_the_diff_it_makes_and_nothing_once_it_is_made() {
    let booted = Booted::playing(Script::Speaks).await;
    declared(&booted).await;

    let (diff, _) = applied(&booted, APPLIED, &[]).await;

    assert!(diff.contains("+ ready\n"), "{diff}");
    assert!(diff.contains("+ triage\n"), "{diff}");
    assert!(
        diff.contains("    brief\n      + Work on {{ event.data.issue.title }}\n"),
        "{diff}"
    );
    assert_eq!(trigger_names(&booted).await, ["ready", "triage"]);
    assert_eq!(
        booted.record(&["trigger", "show", "ready", "--json"]).await["applied"],
        true
    );

    assert_eq!(applied(&booted, APPLIED, &[]).await.0, "no changes");
}

#[tokio::test]
async fn reapplying_changes_and_removes_only_what_a_file_applied() {
    let booted = Booted::playing(Script::Speaks).await;
    declared(&booted).await;
    booted
        .run(&[
            "trigger",
            "declare",
            "one-off",
            "--filter",
            r#"{"exact": {"type": "com.example.build.failed"}}"#,
            "--brief",
            "Fix the build",
            "--project",
            "kestrel",
            "--agent",
            "builder",
        ])
        .await;
    applied(&booted, APPLIED, &[]).await;

    let changed = r#"
triggers:
  ready:
    filter: {exact: {type: com.github.issues.labeled}}
    brief: Work {{ event.data.issue.html_url }}
    project: kestrel
    agent: builder
"#;
    let (diff, _) = applied(&booted, changed, &[]).await;

    assert_eq!(
        diff,
        "~ ready\n    brief\n      - Work on {{ event.data.issue.title }}\n      + Work {{ event.data.issue.html_url }}\n- triage"
    );
    assert_eq!(trigger_names(&booted).await, ["one-off", "ready"]);
}

#[tokio::test]
async fn a_one_off_a_file_declares_becomes_the_files() {
    let booted = Booted::playing(Script::Speaks).await;
    declared(&booted).await;
    booted
        .run(&[
            "trigger",
            "declare",
            "ready",
            "--filter",
            r#"{"exact": {"type": "com.github.issues.labeled"}}"#,
            "--brief",
            "Work on {{ event.data.issue.title }}",
            "--project",
            "kestrel",
            "--agent",
            "builder",
        ])
        .await;

    let (diff, _) = applied(&booted, APPLIED, &[]).await;

    assert!(
        diff.contains("~ ready\n    declared by\n      - flags\n      + a file\n"),
        "{diff}"
    );
    applied(&booted, "triggers: {}", &[]).await;
    assert!(trigger_names(&booted).await.is_empty());
}

#[tokio::test]
async fn a_dry_run_prints_the_diff_and_changes_nothing() {
    let booted = Booted::playing(Script::Speaks).await;
    declared(&booted).await;

    let (diff, warned) = applied(&booted, APPLIED, &["--dry-run"]).await;

    assert!(diff.contains("+ ready\n"), "{diff}");
    assert!(trigger_names(&booted).await.is_empty());
    assert!(warned.contains("the trigger ready fires"), "{warned}");
    assert!(warned.contains("0.4"), "{warned}");
    assert!(!warned.contains("the trigger triage"), "{warned}");
}

#[tokio::test]
async fn an_apply_that_cannot_be_made_whole_changes_nothing() {
    let booted = Booted::playing(Script::Speaks).await;
    declared(&booted).await;
    let naming_a_stranger = format!(
        "{APPLIED}  stranger:\n    filter: {{exact: {{type: x}}}}\n    brief: x\n    project: kestrel\n    agent: nobody\n"
    );

    let refusal = refusal(
        &["trigger", "apply"],
        &applying(&booted, &naming_a_stranger, &[]).await,
    );

    assert!(refusal.contains("nobody"), "{refusal}");
    assert!(trigger_names(&booted).await.is_empty());
}

#[tokio::test]
async fn a_declaration_file_that_is_not_one_is_refused_saying_where() {
    let booted = Booted::playing(Script::Speaks).await;
    declared(&booted).await;

    for (declarations, because) in [
        ("", "triggers"),
        (
            "triggers:\n  ready:\n    brief: x\n    project: kestrel\n    agent: builder\n",
            "triggers.ready: missing field `filter`",
        ),
        (
            "triggers:\n  ready:\n    filter: {sql: x}\n    brief: x\n    project: kestrel\n    agent: builder\n",
            "triggers.ready.filter",
        ),
        (
            "triggers:\n  ready:\n    filter: {exact: {type: x}}\n    brief: x\n    correlation: x\n    project: kestrel\n    agent: builder\n",
            "misses",
        ),
        (
            "triggers:\n  ready:\n    filter: {exact: {type: x}}\n    brief: x\n    agnet: builder\n    project: kestrel\n",
            "triggers.ready: unknown field `agnet`",
        ),
    ] {
        let said = refusal(
            &["trigger", "apply"],
            &applying(&booted, declarations, &[]).await,
        );
        assert!(
            said.contains(because),
            "{declarations:?} was refused unhelpfully: {said}"
        );
    }
}

#[tokio::test]
async fn apply_reads_a_declaration_file_from_standard_input() {
    let booted = Booted::playing(Script::Speaks).await;
    declared(&booted).await;

    booted
        .run_as(
            &["trigger", "apply", "-f", "-"],
            Invocation::default().given(APPLIED),
        )
        .await;

    assert_eq!(trigger_names(&booted).await, ["ready", "triage"]);
}

#[tokio::test]
async fn a_trigger_is_tested_as_a_file_declares_it_rather_than_as_it_was_applied() {
    let stub = GithubStub::start();
    stub.deliver(github_stub::labelled(43, "ready-for-agent"));
    let booted = Booted::playing(Script::Speaks).await;
    declared(&booted).await;
    watching(&booted, &stub, "1ms").await;
    booted
        .run(&[
            "trigger",
            "declare",
            "ready",
            "--filter",
            r#"{"exact": {"type": "com.github.issues.opened"}}"#,
            "--brief",
            "as applied",
            "--project",
            "kestrel",
            "--agent",
            "builder",
        ])
        .await;
    booted.plane.work_pass().await;
    let events = booted.records(&["event", "list", "--json"]).await;
    assert_eq!(
        events.len(),
        1,
        "the pass should record the delivered Event"
    );
    let event = events[0]["record"]
        .as_str()
        .expect("an event record")
        .to_owned();
    let declaring = "triggers:\n  ready:\n    filter: {exact: {type: com.github.issues.labeled}}\n    brief: '{{ instruction }} {{ event.subject }}'\n    project: kestrel\n    agent: builder\n";

    let tested = booted
        .client_as(
            &[
                "trigger",
                "test",
                "ready",
                "--event",
                &event,
                "-f",
                "triggers.yaml",
                "--instruction",
                "@instruction.md",
                "--json",
            ],
            Invocation::default()
                .file("triggers.yaml", declaring)
                .file("instruction.md", "/triage"),
        )
        .await
        .json();

    assert_eq!(tested["matches"], true, "{tested}");
    assert_eq!(tested["brief"], "/triage #43", "{tested}");
    assert_eq!(
        booted
            .record(&["trigger", "test", "ready", "--event", &event, "--json"])
            .await["matches"],
        false
    );
    assert!(
        booted
            .refused_as(
                &[
                    "trigger",
                    "test",
                    "elsewhere",
                    "--event",
                    &event,
                    "-f",
                    "triggers.yaml"
                ],
                Invocation::default().file("triggers.yaml", declaring),
            )
            .await
            .contains("declares no trigger elsewhere"),
    );
}

#[tokio::test]
async fn a_brief_and_a_filter_are_read_from_a_file_or_standard_input() {
    let booted = Booted::playing(Script::Speaks).await;
    declared(&booted).await;

    let finished = booted
        .client_as(
            &[
                "trigger",
                "declare",
                "ready",
                "--filter",
                "-",
                "--brief",
                "@brief.md",
                "--project",
                "kestrel",
                "--agent",
                "builder",
            ],
            Invocation::default()
                .given(r#"{"exact": {"type": "com.github.issues.labeled"}}"#)
                .file(
                    "brief.md",
                    "Work on {{ event.data.issue.title }}\n\nand say so.\n",
                ),
        )
        .await;
    succeeded(&["trigger", "declare"], &finished);

    let shown = booted.record(&["trigger", "show", "ready", "--json"]).await;
    assert_eq!(
        shown["filter"],
        serde_json::json!({ "exact": { "type": "com.github.issues.labeled" } })
    );
    assert!(
        shown["brief"]
            .as_str()
            .is_some_and(|brief| brief.ends_with("and say so.\n")),
        "{shown}"
    );
    assert_eq!(shown["applied"], false);
    assert!(
        finished.err.contains("the trigger ready fires"),
        "a one-off admitting outsiders was declared without a warning"
    );
}

#[tokio::test]
async fn only_one_of_a_filter_and_a_brief_is_read_from_standard_input() {
    let booted = Booted::playing(Script::Speaks).await;
    declared(&booted).await;

    let refusal = booted
        .refused(&[
            "trigger",
            "declare",
            "ready",
            "--filter",
            "-",
            "--brief",
            "-",
            "--project",
            "kestrel",
            "--agent",
            "builder",
        ])
        .await;

    assert!(refusal.contains("standard input"), "{refusal}");
}

#[tokio::test]
async fn transcript_kinds_select_the_entries_the_client_streams() {
    let booted = Booted::playing(Script::Speaks).await;
    declared(&booted).await;
    let workspace = opened(&booted).await;
    booted
        .run(&[
            "workspace",
            "post",
            &workspace,
            "--as-participant",
            "operator",
            "go",
        ])
        .await;
    dispatched(&booted, &workspace).await;
    let shared = booted
        .records(&[
            "workspace",
            "transcript",
            &workspace,
            "--no-summaries",
            "--json",
        ])
        .await;
    assert!(shared.iter().all(|record| record["kind"] == "shared_state"));
    let narration = booted
        .records(&[
            "workspace",
            "transcript",
            &workspace,
            "--kinds",
            "narration",
            "--no-summaries",
            "--json",
        ])
        .await;
    assert_eq!(narration.len(), 2);
    assert_eq!(narration[0]["entry"]["type"], "plan");
    assert_eq!(narration[1]["entry"]["type"], "thought");
    assert!(
        narration
            .iter()
            .all(|record| record["kind"] == "narration" && record["session_id"].is_string())
    );
    let all = booted
        .records(&[
            "workspace",
            "transcript",
            &workspace,
            "--kinds",
            "narration,shared_state",
            "--no-summaries",
            "--json",
        ])
        .await;
    assert_eq!(all.len(), shared.len() + narration.len());
    assert!(
        all.windows(2)
            .all(|pair| pair[0]["seq"].as_i64() < pair[1]["seq"].as_i64())
    );
}

#[tokio::test]
async fn empty_cli_lists_guide_a_terminal_and_leave_pipes_empty() {
    let booted = Booted::playing(Script::Speaks).await;
    let args = ["organization", "list"];
    let shown = booted.terminal(&args).await;
    assert!(shown.status.success(), "{}", shown.said);
    assert!(shown.said.contains("No Organizations."), "{}", shown.said);
    assert!(
        shown.said.contains("kestrel organization declare --help"),
        "{}",
        shown.said
    );
    assert!(booted.client(&args).await.out.is_empty());
    let json = booted.terminal(&["organization", "list", "--json"]).await;
    assert!(json.status.success(), "{}", json.said);
    assert!(!json.said.contains("No Organizations"), "{}", json.said);
    booted.run(&["organization", "declare", "acme"]).await;
    for (command, absent, action) in [
        ("project", "Projects", "project declare --help"),
        ("agent", "Agents", "agent declare --help"),
        (
            "credential",
            "Provider Credentials",
            "credential set --help",
        ),
        ("profile", "Subscription Profiles", "profile declare --help"),
        (
            "integration",
            "Integrations",
            "integration github connect --help",
        ),
        ("event", "Events", "integration list"),
        ("trigger", "Triggers", "trigger declare --help"),
        ("workspace", "Workspaces", "workspace open --help"),
        ("instance", "Instances", "workspace list"),
    ] {
        let args = ["--organization", "acme", command, "list"];
        let shown = booted.terminal(&args).await;
        assert!(shown.status.success(), "{}", shown.said);
        assert!(
            shown
                .said
                .contains(&format!("No {absent} in Organization acme.")),
            "{}",
            shown.said
        );
        assert!(
            shown
                .said
                .contains(&format!("kestrel --organization acme {action}")),
            "{}",
            shown.said
        );
        let piped = booted.client(&args).await;
        assert!(piped.status.success(), "{}", piped.err);
        assert!(piped.out.is_empty(), "{:?}", piped.out);
        assert!(piped.err.is_empty(), "{}", piped.err);
        assert!(!shown.said.contains("Compose"));
        assert!(!shown.said.contains("Participant"));
    }
}

#[test]
fn work_inspection_help_explains_comparisons_scopes_and_paths() {
    for (command, description) in [
        ("work", "reported snapshot"),
        ("changes", "Before the first push"),
        ("commits", "no remote-tracking branch reaches"),
        ("stashes", "without diffing them"),
    ] {
        let finished = client::ran("http://127.0.0.1:1", &["workspace", command, "--help"]);
        assert!(finished.status.success(), "{}", finished.err);
        let shown = finished.out.join("\n");
        for required in [
            description,
            "checkout base",
            "work branch",
            "<repo>/<path>",
            "kestrel workspace",
            "latest",
        ] {
            assert!(
                shown.contains(required),
                "{command} help lacks {required}: {shown}"
            );
        }
        assert!(!shown.contains('\u{1b}'), "{shown}");
    }
}

#[test]
fn empty_cli_inspection_lists_leave_pipes_empty_and_json_unmodified() {
    let server = tiny_http::Server::http("127.0.0.1:0").expect("a stub operator");
    let operator = format!("http://{}", server.server_addr());
    let serving = std::thread::spawn(move || {
        for index in 0..19 {
            let request = server.recv().expect("an inspection request");
            let answer = if request.url().contains("/work") && index >= 17 {
                json!({"state":"reported", "reported_at":"2026-10-07T12:00:00Z", "repositories":[{"repository":"repo", "git":"unreadable", "because":"checkout unavailable"}], "last_report":{"report":"none"},"earlier_reports":[]})
            } else if index >= 15 {
                json!({"repositories":[{"repository":"repo", "text":"abcd1234 a local commit\n"}]})
            } else if request.url().contains("/sessions") {
                json!([])
            } else if request.url().contains("/files") {
                json!({"path":"repo/empty", "entries":[], "total":0, "truncated":false})
            } else if request.url().contains("/changes") {
                json!({"repositories":[{"repository":"repo", "diff":"", "files":[], "truncated":false}]})
            } else {
                json!({"repositories":[{"repository":"repo", "text":""}]})
            };
            request
                .respond(tiny_http::Response::from_string(answer.to_string()))
                .expect("an operator answer");
        }
    });
    for (args, absent) in [
        (
            vec!["workspace", "changes", "latest"],
            "changes in this comparison",
        ),
        (vec!["workspace", "commits", "latest"], "commits"),
        (vec!["workspace", "stashes", "latest"], "stashes"),
        (
            vec!["workspace", "files", "latest", "repo/empty"],
            "directory entries",
        ),
        (vec!["session", "list", "--workspace", "latest"], "Sessions"),
    ] {
        let mut args = args;
        args.extend(["--organization", "selected"]);
        let shown = client::ran_on_a_terminal(&operator, &args, 160, "");
        assert!(shown.status.success(), "{}", shown.said);
        assert!(
            shown
                .said
                .contains(&format!("No {absent} in Organization selected.")),
            "{}",
            shown.said
        );
        assert!(shown.said.contains("latest"), "{}", shown.said);
        assert!(shown.said.contains(&operator), "{}", shown.said);
        let piped = client::ran(&operator, &args);
        assert!(piped.status.success(), "{}", piped.err);
        assert!(piped.out.is_empty(), "{:?}", piped.out);
        assert!(piped.err.is_empty(), "{}", piped.err);
        args.push("--json");
        let json = client::ran(&operator, &args);
        assert!(json.status.success(), "{}", json.err);
        assert!(!json.out.join("\n").contains("No "));
        if args[0] == "workspace" {
            assert!(json.json().is_object(), "{:?}", json.out);
        } else {
            assert_eq!(json.json(), serde_json::json!([]));
        }
    }
    let args = [
        "workspace",
        "commits",
        "latest",
        "--organization",
        "selected",
    ];
    let shown = client::ran_on_a_terminal(&operator, &args, 160, "");
    assert!(shown.status.success(), "{}", shown.said);
    assert!(shown.said.contains("repo:"), "{}", shown.said);
    let piped = client::ran(&operator, &args);
    assert!(piped.status.success(), "{}", piped.err);
    assert_eq!(piped.out, ["abcd1234 a local commit"]);
    let args = ["workspace", "work", "latest", "--organization", "selected"];
    let shown = client::ran_on_a_terminal(&operator, &args, 160, "");
    assert!(shown.status.success(), "{}", shown.said);
    assert!(
        shown.said.contains("Reported by the supervisor"),
        "{}",
        shown.said
    );
    let piped = client::ran(&operator, &args);
    assert!(piped.status.success(), "{}", piped.err);
    assert_eq!(piped.out, ["  checkout unavailable"]);
    serving.join().expect("the stub served every read");
}

#[tokio::test]
async fn the_client_sets_a_hook_url_and_reads_safe_delivery_results() {
    let stub = GithubStub::start();
    let booted = Booted::traced().await;
    booted.run(&["organization", "declare", "acme"]).await;
    booted
        .run(&[
            "integration",
            "github",
            "register",
            "hub",
            "--repository",
            "jtmthf/kestrel",
            "--app-id",
            "1",
            "--installation",
            "2",
            "--private-key",
            support::PRIVATE_KEY,
            "--webhook-secret",
            "kept-secret",
            "--api",
            &stub.base_url(),
            "--interval",
            "1h",
        ])
        .await;
    stub.script_answer(
        "PATCH",
        "/app/hook/config",
        ScriptedResponse::answering(200),
    );
    let changed = booted
        .record(&[
            "integration",
            "github",
            "webhook",
            "set-url",
            "hub",
            "--base",
            "https://hooks.example.com",
            "--json",
        ])
        .await;
    assert_eq!(changed["revision"], 2);
    stub.script_answer(
        "GET",
        "/app/hook/config",
        ScriptedResponse::ok(
            r#"{"url":"https://hooks.example.com/webhooks/id","secret":"never-print-this"}"#,
        ),
    );
    let status = booted
        .record(&[
            "integration",
            "github",
            "webhook",
            "status",
            "hub",
            "--json",
        ])
        .await;
    assert_eq!(
        status,
        json!({"url":"https://hooks.example.com/webhooks/id","deliveries":[],"pending_url":null})
    );
    let refused = booted
        .refused_as(
            &[
                "integration",
                "github",
                "webhook",
                "set-url",
                "hub",
                "--base",
                "https://localhost",
            ],
            Invocation::default(),
        )
        .await;
    assert!(refused.contains("public HTTPS origin"), "{refused}");
    let printed = format!("{changed}{status}{refused}{}", booted.said());
    assert!(!printed.contains("kept-secret"));
    assert!(!printed.contains("never-print-this"));
    booted.plane.teardown().await;
}
