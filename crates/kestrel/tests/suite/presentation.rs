//! What the Client prints is decided by what is reading it: a person at a terminal, or a
//! script holding the other end of a pipe.

use crate::support;

use serde_json::Value;
use support::client::{self, Invocation, ran_on_a_terminal};
use support::fixture::Fixture;
use support::github_stub::GithubStub;
use support::{Kestrel, PRIVATE_KEY};

const REPOSITORY: &str = "https://github.com/jtmthf/kestrel";

async fn an_organization_holding_two_agents() -> Kestrel {
    let kestrel = Kestrel::boot().await;
    Fixture::acme()
        .repositories(&[REPOSITORY])
        .model("claude-opus-5")
        .agent("reviewer", "opencode", None)
        .declare(&kestrel)
        .await;

    kestrel
}

async fn piped(kestrel: &Kestrel, args: &[&str]) -> client::Finished {
    client::ran_by(kestrel, args, Invocation::default()).await
}

async fn on_a_terminal(kestrel: &Kestrel, args: &[&str], columns: u16) -> client::Shown {
    let operator = kestrel.operator();
    let args: Vec<String> = args.iter().map(|&arg| arg.to_owned()).collect();

    tokio::task::spawn_blocking(move || {
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        ran_on_a_terminal(&operator, &args, columns, "")
    })
    .await
    .expect("the client should run")
}

#[tokio::test]
async fn a_pipe_gets_the_commands_own_fields_tab_delimited_without_a_flag() {
    let kestrel = an_organization_holding_two_agents().await;

    let listed = piped(&kestrel, &["agent", "list"]).await;

    let agents: Vec<Vec<&str>> = listed
        .out
        .iter()
        .map(|line| line.split('\t').collect())
        .collect();
    assert_eq!(agents.len(), 2, "{:?}", listed.out);
    assert_eq!(agents[0][1..], ["builder", "opencode", "claude-opus-5"]);
    assert_eq!(agents[1][1..], ["reviewer", "opencode", "-"]);
    kestrel.teardown().await;
}

#[tokio::test]
async fn a_pipe_gets_no_header_to_strip_and_no_styling_to_strip_it_from() {
    let kestrel = an_organization_holding_two_agents().await;

    let listed = piped(&kestrel, &["agent", "list"]).await;

    let said = listed.out.join("\n");
    assert!(
        !said.contains("name\tharness"),
        "a pipe was given a header:\n{said}"
    );
    assert!(
        !said.contains('\u{1b}'),
        "a pipe was given styling:\n{said}"
    );
    kestrel.teardown().await;
}

#[tokio::test]
async fn a_terminal_gets_the_records_under_the_fields_they_fill() {
    let kestrel = an_organization_holding_two_agents().await;

    let listed = on_a_terminal(&kestrel, &["agent", "list"], 120).await;

    assert!(listed.status.success(), "{}", listed.said);
    let [header, builder, reviewer] = listed.lines()[..] else {
        panic!("a terminal was shown {:?}", listed.lines());
    };
    assert!(header.starts_with("id "), "{header:?}");
    assert!(header.contains("name") && header.contains("harness") && header.contains("model"));
    assert_eq!(
        builder.find("opencode"),
        reviewer.find("opencode"),
        "the columns do not line up:\n{builder}\n{reviewer}"
    );
    kestrel.teardown().await;
}

#[tokio::test]
async fn a_terminal_gets_a_detail_read_down_the_page_and_a_pipe_gets_it_across_one_line() {
    let kestrel = an_organization_holding_two_agents().await;
    let workspace = kestrel.open_workspace("acme", "kestrel", "builder").await;
    let workspace = workspace.id.to_string();

    let watched = on_a_terminal(&kestrel, &["workspace", "show", &workspace], 120).await;
    let scripted = piped(&kestrel, &["workspace", "show", &workspace]).await;

    assert!(
        watched
            .lines()
            .iter()
            .any(|line| line.starts_with("id ") && line.ends_with(&workspace)),
        "a terminal was shown:\n{}",
        watched.said
    );
    assert!(
        watched
            .lines()
            .iter()
            .any(|line| line.starts_with("state ")),
        "a terminal was shown:\n{}",
        watched.said
    );
    assert_eq!(scripted.out.len(), 1, "{:?}", scripted.out);
    let fields: Vec<&str> = scripted.out[0].split('\t').collect();
    assert_eq!(fields[0], workspace);
    assert!(fields.contains(&"open"), "{fields:?}");
    kestrel.teardown().await;
}

#[tokio::test]
async fn a_terminal_gets_lines_it_can_hold_and_a_pipe_gets_them_whole() {
    let kestrel = Kestrel::boot().await;
    let acme = kestrel.declare_organization("acme").await;
    let repositories = ["alpha", "beta", "gamma", "delta", "epsilon", "zeta"]
        .map(|name| format!("https://github.com/jtmthf/{name}-a-rather-long-repository-name"));
    kestrel
        .declare_project(&acme, "kestrel", &repositories, "main")
        .await;

    let watched = on_a_terminal(&kestrel, &["project", "list"], 60).await;
    let scripted = piped(&kestrel, &["project", "list"]).await;

    for line in watched.lines() {
        assert!(
            line.chars().count() <= 60,
            "{line:?} is wider than the terminal"
        );
    }
    for repository in &repositories {
        assert!(
            scripted.out[0].contains(repository.as_str()),
            "a pipe was given a truncated record:\n{}",
            scripted.out[0]
        );
    }
    kestrel.teardown().await;
}

async fn answered(kestrel: &Kestrel, path: &str) -> Value {
    reqwest::get(format!("{}/operator/{path}", kestrel.operator()))
        .await
        .expect("the operator boundary should answer")
        .json()
        .await
        .expect("a JSON answer")
}

#[tokio::test]
async fn json_is_the_whole_collection_as_one_document_jq_reads_whole() {
    let kestrel = an_organization_holding_two_agents().await;

    let listed = piped(&kestrel, &["agent", "list", "--json"]).await;

    assert_eq!(listed.out.len(), 1, "{:?}", listed.out);
    assert_eq!(
        listed.json(),
        answered(&kestrel, "organizations/acme/agents").await
    );
    assert_eq!(listed.jq("length"), "2");
    assert_eq!(
        listed.jq("map(.name)"),
        r#"["builder","reviewer"]"#,
        "jq did not see the whole collection"
    );
    kestrel.teardown().await;
}

#[tokio::test]
async fn json_answers_an_empty_collection_with_an_empty_array() {
    let kestrel = an_organization_holding_two_agents().await;

    let listed = piped(&kestrel, &["workspace", "list", "--json"]).await;

    assert_eq!(listed.out, ["[]"]);
    assert!(listed.err.is_empty(), "{}", listed.err);
    assert_eq!(listed.jq("length"), "0");
    kestrel.teardown().await;
}

#[tokio::test]
async fn a_catalogue_is_one_read_shown_three_ways() {
    let kestrel = Kestrel::boot().await;
    let unbounded = piped(&kestrel, &["organization", "declare", "acme"]).await;
    let capped = piped(
        &kestrel,
        &[
            "organization",
            "declare",
            "globex",
            "--max-live-instances",
            "3",
        ],
    )
    .await;
    let acme = kestrel.organizations().await.remove(0);
    kestrel
        .declare_project(
            &acme,
            "kestrel",
            &[REPOSITORY.to_owned(), format!("{REPOSITORY}-docs")],
            "main",
        )
        .await;
    let scoped = ["--organization", "acme"];

    let organizations = piped(&kestrel, &["organization", "list"]).await;
    let watched = on_a_terminal(&kestrel, &["organization", "list"], 120).await;
    let projects = piped(&kestrel, &[&scoped[..], &["project", "list"]].concat()).await;
    let whole = piped(&kestrel, &["organization", "list", "--json"]).await;

    assert_eq!(
        organizations.out,
        [
            format!("{}\tacme\t-", unbounded.out[0]),
            format!("{}\tglobex\t3", capped.out[0]),
        ]
    );
    let header: Vec<&str> = watched.lines()[0]
        .split("  ")
        .map(str::trim)
        .filter(|label| !label.is_empty())
        .collect();
    assert_eq!(
        header,
        ["id", "name", "max live instances"],
        "{}",
        watched.said
    );
    let project: Vec<&str> = projects.out[0].split('\t').collect();
    assert_eq!(
        project[1..],
        [
            "kestrel",
            "main",
            &format!("{REPOSITORY},{REPOSITORY}-docs")
        ]
    );
    assert_eq!(whole.json(), answered(&kestrel, "organizations").await);
    assert_eq!(
        whole.jq("map(.max_live_instances)"),
        "[null,3]",
        "an unbounded Organization lost its null"
    );
    kestrel.teardown().await;
}

#[tokio::test]
async fn an_empty_catalogue_is_an_empty_array_nothing_on_a_pipe_and_guidance_at_a_terminal() {
    let kestrel = Kestrel::boot().await;
    kestrel.declare_organization("acme").await;

    for records in ["project", "agent"] {
        let whole = piped(&kestrel, &[records, "list", "--json"]).await;
        let scripted = piped(&kestrel, &[records, "list"]).await;
        let watched = on_a_terminal(&kestrel, &[records, "list"], 120).await;

        assert_eq!(whole.out, ["[]"], "{records}");
        assert!(scripted.status.success(), "{}", scripted.err);
        assert!(scripted.out.is_empty(), "{:?}", scripted.out);
        assert!(
            watched.said.contains(&format!("{records} declare --help")),
            "{}",
            watched.said
        );
    }
    kestrel.teardown().await;
}

#[tokio::test]
async fn a_declaration_answers_its_identifier_and_json_answers_the_record() {
    let kestrel = Kestrel::boot().await;
    kestrel.declare_organization("acme").await;

    let project = piped(
        &kestrel,
        &[
            "project",
            "declare",
            "kestrel",
            "--repository",
            REPOSITORY,
            "--branch",
            "main",
        ],
    )
    .await;
    let agent = piped(&kestrel, &["agent", "declare", "builder"]).await;
    let again = piped(&kestrel, &["agent", "declare", "builder", "--json"]).await;

    let projects = answered(&kestrel, "organizations/acme/projects").await;
    let agents = answered(&kestrel, "organizations/acme/agents").await;
    assert_eq!(project.out, [projects[0]["id"].as_str().expect("an id")]);
    assert_eq!(agent.out, [agents[0]["id"].as_str().expect("an id")]);
    assert_eq!(again.json(), agents[0]);
    assert_eq!(
        again.jq("[.model, .mode, .thought_level]"),
        "[null,null,null]",
        "a default the Agent leaves to its Harness lost its null"
    );
    kestrel.teardown().await;
}

#[tokio::test]
async fn an_agents_model_is_the_value_a_pipe_captures_and_a_dash_when_it_names_none() {
    let kestrel = an_organization_holding_two_agents().await;

    let set = piped(
        &kestrel,
        &["agent", "model", "reviewer", "--model", "claude-sonnet-5"],
    )
    .await;
    let whole = piped(
        &kestrel,
        &[
            "agent",
            "model",
            "reviewer",
            "--model",
            "claude-sonnet-5",
            "--json",
        ],
    )
    .await;
    let cleared = piped(&kestrel, &["agent", "model", "reviewer"]).await;

    assert_eq!(set.out, ["claude-sonnet-5"]);
    assert_eq!(
        whole.jq("[.name, .harness, .model]"),
        r#"["reviewer","opencode","claude-sonnet-5"]"#
    );
    assert_eq!(cleared.out, ["-"]);
    let agents = answered(&kestrel, "organizations/acme/agents").await;
    assert_eq!(agents[1]["model"], Value::Null);
    kestrel.teardown().await;
}

#[tokio::test]
async fn json_is_the_whole_record_however_little_a_terminal_is_shown() {
    let kestrel = an_organization_holding_two_agents().await;
    let workspace = kestrel.open_workspace("acme", "kestrel", "builder").await;
    let id = workspace.id.to_string();

    let shown = piped(&kestrel, &["workspace", "show", &id, "--json"]).await;

    assert_eq!(
        shown.json(),
        answered(&kestrel, &format!("organizations/acme/workspaces/{id}")).await
    );
    assert_eq!(shown.jq(".id"), format!("\"{id}\""));
    kestrel.teardown().await;
}

#[tokio::test]
async fn json_answers_a_compound_write_whole() {
    let kestrel = an_organization_holding_two_agents().await;

    let opened = piped(
        &kestrel,
        &[
            "workspace",
            "open",
            "--project",
            "kestrel",
            "--agent",
            "builder",
            "--json",
        ],
    )
    .await;
    assert_eq!(
        opened.jq("[.workspace.id == .session.workspace, .workspace.state]"),
        r#"[true,"open"]"#
    );
    let opened = opened.json();
    let typed: kestrel_operator_types::Opened = serde_json::from_value(opened.clone()).unwrap();
    assert_eq!(typed.workspace.id, typed.session.workspace);
    assert_eq!(serde_json::to_value(&typed).unwrap(), opened);
    let workspace = opened["workspace"]["id"].as_str().expect("the Workspace");
    assert_eq!(opened["session"]["workspace"], workspace, "{opened}");
    assert_eq!(opened["workspace"]["state"], "open", "{opened}");

    let posted = piped(
        &kestrel,
        &[
            "workspace",
            "post",
            workspace,
            "--as-participant",
            "operator",
            "begin",
            "--json",
        ],
    )
    .await;
    assert_eq!(posted.jq("has(\"held_message\")"), "true");
    let posted = posted.json();
    assert_eq!(posted["session"]["id"], opened["session"]["id"], "{posted}");
    assert!(
        posted
            .as_object()
            .is_some_and(|posted| posted.contains_key("held_message")),
        "{posted}"
    );
    let typed: kestrel_operator_types::Posted = serde_json::from_value(posted.clone()).unwrap();
    assert_eq!(serde_json::to_value(&typed).unwrap(), posted);
    let listed = piped(&kestrel, &["workspace", "list", "--json"])
        .await
        .json();
    let typed: kestrel_operator_types::ListWorkspacesResponse =
        serde_json::from_value(listed.clone()).unwrap();
    assert_eq!(typed[0].id.to_string(), workspace);
    assert_eq!(serde_json::to_value(&typed).unwrap(), listed);
    let sessions = piped(
        &kestrel,
        &["session", "list", "--workspace", workspace, "--json"],
    )
    .await
    .json();
    let typed: kestrel_operator_types::ListSessionsResponse =
        serde_json::from_value(sessions.clone()).unwrap();
    assert_eq!(
        typed[0].id.to_string(),
        opened["session"]["id"].as_str().unwrap()
    );
    assert_eq!(serde_json::to_value(&typed).unwrap(), sessions);
    kestrel.teardown().await;
}

#[tokio::test]
async fn json_streams_a_transcript_one_whole_record_a_line() {
    let kestrel = an_organization_holding_two_agents().await;
    let workspace = kestrel.open_workspace("acme", "kestrel", "builder").await;
    let session = kestrel.dispatch_session(workspace.id).await;
    kestrel.said(&session, "one line\nand another").await;

    let read = piped(
        &kestrel,
        &[
            "workspace",
            "transcript",
            &workspace.id.to_string(),
            "--json",
        ],
    )
    .await;

    assert!(read.out.len() > 1, "{:?}", read.out);
    for line in &read.out {
        let record: Value = serde_json::from_str(line)
            .unwrap_or_else(|error| panic!("{line} is not one record: {error}"));
        assert!(
            record.get("seq").is_some() && record.get("entry").is_some(),
            "{line}"
        );
    }
    assert_eq!(
        read.jq(".seq").lines().count(),
        read.out.len(),
        "jq did not read one record a line"
    );
    assert!(
        read.out
            .iter()
            .any(|line| line.contains(r"one line\nand another")),
        "{:?}",
        read.out
    );
    assert!(read.err.contains("cursor  "), "{}", read.err);
    kestrel.teardown().await;
}

#[tokio::test]
async fn json_answers_a_held_secret_without_the_secret() {
    let kestrel = an_organization_holding_two_agents().await;

    let held = client::ran_by(
        &kestrel,
        &["credential", "set", "ANTHROPIC_API_KEY", "--json"],
        Invocation::default().given("sk-a-secret-key\n"),
    )
    .await;

    assert_eq!(held.json()["variable"], "ANTHROPIC_API_KEY");
    assert!(
        !held.out.join("\n").contains("sk-a-secret-key") && !held.err.contains("sk-a-secret-key"),
        "the secret was written back: {:?} {}",
        held.out,
        held.err
    );
    kestrel.teardown().await;
}

#[tokio::test]
async fn a_field_list_after_json_is_refused_before_anything_is_written() {
    let kestrel = an_organization_holding_two_agents().await;

    let listed = piped(
        &kestrel,
        &["organization", "declare", "globex", "--json", "id,name"],
    )
    .await;
    let assigned = piped(
        &kestrel,
        &["organization", "declare", "globex", "--json=id"],
    )
    .await;
    let named = piped(
        &kestrel,
        &["organization", "declare", "globex", "--json", "id"],
    )
    .await;

    for refused in [&listed, &assigned, &named] {
        assert_eq!(refused.status.code(), Some(2), "{}", refused.err);
        assert!(refused.out.is_empty(), "{:?}", refused.out);
        let diagnostic: Value = serde_json::from_str(refused.err.trim()).expect("one diagnostic");
        assert_eq!(diagnostic["kind"], "client_failure", "{diagnostic}");
        assert_eq!(diagnostic["context"]["operation"], "organization declare");
        assert!(
            diagnostic["message"].as_str().unwrap().contains("jq"),
            "{diagnostic}"
        );
    }
    assert_eq!(
        kestrel.organizations().await.len(),
        1,
        "a refused invocation declared an Organization"
    );
    kestrel.teardown().await;
}

#[tokio::test]
async fn json_reports_a_write_whose_answer_was_lost_as_one_diagnostic_on_stderr() {
    let server = tiny_http::Server::http("127.0.0.1:0").expect("a stub operator");
    let operator = format!("http://{}", server.server_addr());
    std::thread::spawn(move || {
        if let Ok(request) = server.recv() {
            let _ = request.respond(tiny_http::Response::from_string("not json"));
        }
    });

    let finished = tokio::task::spawn_blocking(move || {
        client::ran(&operator, &["organization", "declare", "acme", "--json"])
    })
    .await
    .expect("the client should run");

    assert_eq!(finished.status.code(), Some(5), "{}", finished.err);
    assert!(finished.out.is_empty(), "{:?}", finished.out);
    let diagnostic: Value = serde_json::from_str(finished.err.trim_end())
        .unwrap_or_else(|error| panic!("{} is not one diagnostic: {error}", finished.err));
    assert_eq!(diagnostic["kind"], "unknown_response", "{diagnostic}");
    assert_eq!(
        diagnostic["next_steps"][0]["action"], "inspect_operation",
        "{diagnostic}"
    );
    assert_eq!(
        diagnostic["next_steps"][0]["uncertain"], true,
        "{diagnostic}"
    );
}

#[tokio::test]
async fn standard_output_carries_the_value_and_standard_error_carries_the_rest() {
    let kestrel = an_organization_holding_two_agents().await;
    let workspace = kestrel.open_workspace("acme", "kestrel", "builder").await;
    let stub = GithubStub::start();

    let read = piped(
        &kestrel,
        &["workspace", "transcript", &workspace.id.to_string()],
    )
    .await;
    let registered = piped(
        &kestrel,
        &[
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
            PRIVATE_KEY,
            "--api",
            &stub.base_url(),
        ],
    )
    .await;

    assert!(
        read.out.iter().all(|line| !line.contains("cursor")),
        "the cursor was printed among the entries:\n{:?}",
        read.out
    );
    assert!(read.err.contains("cursor  "), "{}", read.err);
    assert_eq!(registered.out.len(), 1, "{:?}", registered.out);
    assert!(
        !registered.out[0].contains('\t'),
        "a creation answered more than the identifier it minted: {}",
        registered.out[0]
    );
    assert!(registered.err.is_empty(), "{}", registered.err);
    kestrel.teardown().await;
}

#[tokio::test]
async fn a_pipe_on_standard_input_is_read_without_asking_for_anything() {
    let kestrel = an_organization_holding_two_agents().await;

    let held = client::ran_by(
        &kestrel,
        &["credential", "set", "ANTHROPIC_API_KEY"],
        Invocation::default().given("sk-a-key\n"),
    )
    .await;
    let nothing = client::ran_by(
        &kestrel,
        &["credential", "set", "OPENAI_API_KEY"],
        Invocation::default(),
    )
    .await;

    assert!(held.status.success(), "{}", held.err);
    assert!(
        held.err.is_empty(),
        "the client asked a pipe a question: {}",
        held.err
    );
    assert!(!nothing.status.success());
    assert!(
        !nothing.err.contains("ctrl-d"),
        "the client prompted a closed standard input: {}",
        nothing.err
    );
    kestrel.teardown().await;
}

#[tokio::test]
async fn a_terminal_on_standard_input_is_told_what_is_being_waited_for() {
    let kestrel = an_organization_holding_two_agents().await;
    let operator = kestrel.operator();

    let held = tokio::task::spawn_blocking(move || {
        ran_on_a_terminal(
            &operator,
            &["credential", "set", "ANTHROPIC_API_KEY"],
            80,
            "sk-a-key\n\u{4}",
        )
    })
    .await
    .expect("the client should run");

    assert!(held.status.success(), "{}", held.said);
    assert!(
        held.said.contains("a provider credential") && held.said.contains("standard input"),
        "a terminal was left waiting in silence:\n{}",
        held.said
    );
    kestrel.teardown().await;
}

/// A Transcript is the one listing whose records are prose, so the terminal presentation that
/// makes a table readable would make this command pointless.
#[tokio::test]
async fn a_transcript_reaches_a_terminal_whole_however_narrow_it_is() {
    let kestrel = an_organization_holding_two_agents().await;
    let workspace = kestrel.open_workspace("acme", "kestrel", "builder").await;
    let session = kestrel.dispatch_session(workspace.id).await;
    let said = "a line of what the agent had to say, and then\na second line after it";
    kestrel.said(&session, said).await;

    let read = on_a_terminal(
        &kestrel,
        &["workspace", "transcript", &workspace.id.to_string()],
        40,
    )
    .await;

    for line in said.lines() {
        assert!(
            read.said.contains(line),
            "the terminal was shown a clipped transcript:\n{}",
            read.said
        );
    }
    kestrel.teardown().await;
}

#[tokio::test]
async fn jq_sees_a_collection_whole_prints_a_string_raw_and_yields_nothing_successfully() {
    let kestrel = an_organization_holding_two_agents().await;

    let mapped = piped(&kestrel, &["agent", "list", "--jq", "map(.name)"]).await;
    assert!(mapped.status.success(), "{}", mapped.err);
    assert_eq!(mapped.out, [r#"["builder","reviewer"]"#]);

    let each = piped(&kestrel, &["agent", "list", "--jq", ".[] | .name"]).await;
    assert_eq!(
        each.out,
        ["builder", "reviewer"],
        "a string result was quoted"
    );

    let length = piped(&kestrel, &["agent", "list", "--jq", "length"]).await;
    assert_eq!(length.out, ["2"]);

    let none = piped(
        &kestrel,
        &[
            "agent",
            "list",
            "--jq",
            r#".[] | select(.name == "nobody")"#,
        ],
    )
    .await;
    assert!(none.status.success(), "{}", none.err);
    assert!(none.out.is_empty(), "{:?}", none.out);
    kestrel.teardown().await;
}

#[tokio::test]
async fn a_malformed_filter_is_refused_before_anything_is_written() {
    let kestrel = an_organization_holding_two_agents().await;

    let refused = piped(
        &kestrel,
        &["organization", "declare", "globex", "--jq", ".["],
    )
    .await;

    assert_eq!(refused.status.code(), Some(2), "{}", refused.err);
    assert!(refused.out.is_empty(), "{:?}", refused.out);
    let diagnostic: Value = serde_json::from_str(refused.err.trim()).expect("one diagnostic");
    assert_eq!(diagnostic["kind"], "client_failure", "{diagnostic}");
    assert_eq!(
        diagnostic["context"]["operation"], "organization declare",
        "{diagnostic}"
    );
    assert!(
        diagnostic["message"].as_str().unwrap().contains("filter"),
        "{diagnostic}"
    );
    assert_eq!(
        kestrel.organizations().await.len(),
        1,
        "a refused filter declared an Organization"
    );
    kestrel.teardown().await;
}

/// A filter that cannot run the answer fails after the write has already happened, so the exit
/// says the operation may have taken effect and nothing is sent again.
#[tokio::test]
async fn a_filter_that_fails_after_a_write_keeps_the_writes_truthful_outcome() {
    let kestrel = an_organization_holding_two_agents().await;

    let failed = piped(
        &kestrel,
        &["organization", "declare", "globex", "--jq", r#""x" + 1"#],
    )
    .await;

    assert_eq!(failed.status.code(), Some(1), "{}", failed.err);
    assert!(failed.out.is_empty(), "{:?}", failed.out);
    let diagnostic: Value = serde_json::from_str(failed.err.trim()).expect("one diagnostic");
    assert_eq!(diagnostic["kind"], "client_failure", "{diagnostic}");
    assert_eq!(
        diagnostic["next_steps"][0]["action"], "inspect_operation",
        "{diagnostic}"
    );
    assert_eq!(
        diagnostic["next_steps"][0]["uncertain"], true,
        "{diagnostic}"
    );
    assert_eq!(
        kestrel.organizations().await.len(),
        2,
        "the write the filter could not show was lost"
    );
    kestrel.teardown().await;
}

#[tokio::test]
async fn a_filter_does_not_swallow_a_refusal() {
    let kestrel = an_organization_holding_two_agents().await;

    let refused = piped(
        &kestrel,
        &["workspace", "show", "no-such-workspace", "--jq", ".id"],
    )
    .await;

    assert_eq!(refused.status.code(), Some(3), "{}", refused.err);
    assert!(refused.out.is_empty(), "{:?}", refused.out);
    let diagnostic: Value = serde_json::from_str(refused.err.trim()).expect("one diagnostic");
    assert_eq!(diagnostic["kind"], "missing_reference", "{diagnostic}");
    kestrel.teardown().await;
}

#[tokio::test]
async fn jq_filters_a_followed_stream_one_record_at_a_time() {
    let kestrel = an_organization_holding_two_agents().await;
    let workspace = kestrel.open_workspace("acme", "kestrel", "builder").await;
    let session = kestrel.dispatch_session(workspace.id).await;
    kestrel.said(&session, "one line\nand another").await;
    let workspace = workspace.id.to_string();

    let read = piped(
        &kestrel,
        &["workspace", "transcript", &workspace, "--jq", ".seq"],
    )
    .await;

    assert!(read.status.success(), "{}", read.err);
    assert!(read.out.len() > 1, "{:?}", read.out);
    for line in &read.out {
        assert!(
            line.parse::<i64>().is_ok(),
            "{line} is not one record's seq"
        );
    }
    assert!(read.err.contains("cursor  "), "{}", read.err);

    let quiet = piped(
        &kestrel,
        &[
            "workspace",
            "transcript",
            &workspace,
            "--jq",
            "select(.seq > 100000)",
        ],
    )
    .await;
    assert!(quiet.status.success(), "{}", quiet.err);
    assert!(
        quiet.out.is_empty(),
        "a record the filter yielded nothing for wrote a line: {:?}",
        quiet.out
    );
    kestrel.teardown().await;
}
