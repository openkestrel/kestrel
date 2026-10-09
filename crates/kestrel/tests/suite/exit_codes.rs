use crate::support;

use std::net::TcpListener;

use kestrel::domain::{Session, Workspace};
use serde_json::{Value, json};
use support::Kestrel;
use support::client::{Finished, Invocation, ran, ran_by, ran_on_a_terminal_by};

const SUCCESS: i32 = 0;
const USAGE: i32 = 2;
const UNRESOLVED: i32 = 3;
const REJECTED: i32 = 4;
const UNAVAILABLE: i32 = 5;

fn exited(finished: &Finished, code: i32) {
    assert_eq!(
        finished.status.code(),
        Some(code),
        "stdout: {:?}\nstderr: {}",
        finished.out,
        finished.err
    );
}

/// A port something listened on a moment ago and nothing does now.
fn nowhere() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("a free port");
    let address = listener.local_addr().expect("a bound address");
    drop(listener);

    format!("http://{address}")
}

async fn an_organization() -> Kestrel {
    let kestrel = Kestrel::boot().await;
    kestrel.declare_organization("acme").await;

    kestrel
}

#[test]
fn the_catalog_is_printed_without_reaching_a_control_plane() {
    let finished = ran(&nowhere(), &["exit-codes", "--json"]);

    exited(&finished, SUCCESS);
    assert_eq!(
        finished
            .records()
            .iter()
            .map(|entry| json!({ "code": entry["code"], "name": entry["name"] }))
            .collect::<Vec<_>>(),
        [
            json!({ "code": 0, "name": "success" }),
            json!({ "code": 1, "name": "failure" }),
            json!({ "code": 2, "name": "usage" }),
            json!({ "code": 3, "name": "unresolved" }),
            json!({ "code": 4, "name": "rejected" }),
            json!({ "code": 5, "name": "unavailable" }),
            json!({ "code": 78, "name": "not_ready" }),
        ]
    );
}

#[test]
fn every_entry_says_what_it_means_and_when_to_branch_on_it() {
    let finished = ran(&nowhere(), &["exit-codes", "--json"]);

    for entry in finished.records() {
        for field in ["meaning", "branch"] {
            assert!(
                entry[field].as_str().is_some_and(|text| !text.is_empty()),
                "{entry} says nothing for {field}"
            );
        }
    }
}

#[test]
fn help_points_at_the_catalog() {
    let finished = ran(&nowhere(), &["--help"]);

    exited(&finished, SUCCESS);
    assert!(
        finished
            .out
            .iter()
            .any(|line| line.contains("kestrel exit-codes")),
        "{:?}",
        finished.out
    );
}

#[test]
fn an_invalid_invocation_is_usage() {
    for args in [
        &["no-such-command"][..],
        &["organization", "list", "--no-such-flag"],
        &["organization", "list", "--organization", "acme"],
        &["organization", "list", "--json", "id,name"],
        &["--control-plane", "not a url", "organization", "list"],
        &["apply", "-f", "no-such-file.yaml"],
    ] {
        let finished = ran(&nowhere(), args);

        exited(&finished, USAGE);
    }
}

#[test]
fn guessed_workspace_and_session_verbs_explain_the_domain_verbs_without_running_them() {
    for (args, suggested, verbs) in [
        (
            &["workspace", "create"][..],
            "workspace open",
            "open, list, changes, commits, stashes, work, files, read, show, post, message, seal, transcript",
        ),
        (
            &["workspace", "close"],
            "workspace seal",
            "open, list, changes, commits, stashes, work, files, read, show, post, message, seal, transcript",
        ),
        (
            &["session", "start"],
            "session enqueue",
            "enqueue, list, show, interrupt, stop",
        ),
        (
            &["session", "enqueu"],
            "session enqueue",
            "enqueue, list, show, interrupt, stop",
        ),
        (
            &["workspace", "opne"],
            "workspace open",
            "open, list, changes, commits, stashes, work, files, read, show, post, message, seal, transcript",
        ),
        (
            &["workspace", "sael"],
            "workspace seal",
            "open, list, changes, commits, stashes, work, files, read, show, post, message, seal, transcript",
        ),
    ] {
        let finished = ran(&nowhere(), args);
        exited(&finished, USAGE);
        assert!(finished.err.contains(suggested), "{}", finished.err);
        assert!(finished.err.contains(verbs), "{}", finished.err);
        assert!(finished.out.is_empty());
    }
}

#[tokio::test]
async fn no_organization_in_scope_is_unresolved() {
    let kestrel = Kestrel::boot().await;

    let finished = ran_by(&kestrel, &["project", "list"], Invocation::default()).await;

    exited(&finished, UNRESOLVED);
    assert!(
        finished
            .err
            .contains("kestrel organization declare default")
    );
    kestrel.teardown().await;
}

#[tokio::test]
async fn ambiguous_and_empty_organization_scope_offer_runnable_choices() {
    let kestrel = an_organization().await;
    kestrel.declare_organization("globex").await;

    let ambiguous = ran_by(&kestrel, &["project", "list"], Invocation::default()).await;
    exited(&ambiguous, UNRESOLVED);
    assert!(ambiguous.err.contains("kestrel status --organization acme"));
    assert!(
        ambiguous
            .err
            .contains("kestrel status --organization globex")
    );

    let empty = ran_by(
        &kestrel,
        &["project", "list"],
        Invocation::default().file(".kestrel/organization", ""),
    )
    .await;
    exited(&empty, UNRESOLVED);
    assert!(empty.err.contains("kestrel status --organization acme"));
    kestrel.teardown().await;
}

const OPEN_ABSENT: [&str; 6] = [
    "workspace",
    "open",
    "--project",
    "absent",
    "--agent",
    "agent",
];

#[tokio::test]
async fn a_missing_project_names_the_setup_command_and_keeps_its_category() {
    let kestrel = an_organization().await;
    let finished = ran_by(&kestrel, &OPEN_ABSENT, Invocation::default()).await;

    exited(&finished, UNRESOLVED);
    assert!(
        finished.err.contains(&format!(
            "kestrel project declare absent --repository <REPOSITORY> --branch <BRANCH> \
             --organization acme --control-plane {}",
            kestrel.operator()
        )),
        "{}",
        finished.err
    );
    assert!(
        finished.err.contains("needs --repository and --branch"),
        "{}",
        finished.err
    );
    assert!(!finished.err.contains("git "), "{}", finished.err);
    kestrel.teardown().await;
}

#[tokio::test]
async fn json_puts_the_typed_diagnostic_on_stderr_and_leaves_stdout_for_success() {
    let kestrel = an_organization().await;
    let mut args = OPEN_ABSENT.to_vec();
    args.push("--json");

    let finished = ran_by(&kestrel, &args, Invocation::default()).await;

    exited(&finished, UNRESOLVED);
    assert!(finished.out.is_empty(), "{:?}", finished.out);
    let diagnostic: Value =
        serde_json::from_str(finished.err.trim()).expect("stderr should be one diagnostic");
    assert_eq!(diagnostic["kind"], "missing_reference");
    assert_eq!(diagnostic["context"]["resource"], "project");
    assert_eq!(diagnostic["next_steps"][0]["action"], "declare_project");
    assert_eq!(
        diagnostic["next_steps"][0]["missing"],
        json!(["repositories", "branch"])
    );
    kestrel.teardown().await;
}

#[tokio::test]
async fn a_terminal_collects_the_missing_inputs_and_runs_only_the_chosen_step() {
    let kestrel = an_organization().await;

    let shown = ran_on_a_terminal_by(
        &kestrel,
        &OPEN_ABSENT,
        Invocation::default(),
        "1\nhttps://example.com/repo\nmain\n",
    )
    .await;

    assert_eq!(shown.status.code(), Some(UNRESOLVED), "{}", shown.said);
    let organization = kestrel.organizations().await.remove(0);
    let projects = kestrel.projects(&organization).await;
    assert_eq!(projects.len(), 1, "{}", shown.said);
    assert_eq!(projects[0].name, "absent");
    assert_eq!(projects[0].branch, "main");
    assert!(
        kestrel.workspaces("acme").await.is_empty(),
        "the refused open was replayed: {}",
        shown.said
    );
    kestrel.teardown().await;
}

#[tokio::test]
async fn corrective_command_names_the_unencoded_organization() {
    let kestrel = Kestrel::boot().await;
    kestrel.declare_organization("Acme East").await;
    let finished = ran_by(
        &kestrel,
        &[
            "workspace",
            "open",
            "--project",
            "absent",
            "--agent",
            "agent",
        ],
        Invocation::default(),
    )
    .await;

    exited(&finished, UNRESOLVED);
    assert!(
        finished.err.contains("--organization 'Acme East'"),
        "{}",
        finished.err
    );
    kestrel.teardown().await;
}

async fn a_session_in_flight() -> (Kestrel, Workspace, Session) {
    let kestrel = an_organization().await;
    let organization = kestrel.organizations().await.remove(0);
    let project = kestrel
        .declare_project(
            &organization,
            "work",
            &["https://example.com/repo".to_owned()],
            "main",
        )
        .await;
    let agent = kestrel
        .declare_agent(&organization, "worker", "opencode", None)
        .await;
    let workspace = kestrel
        .open_workspace("acme", &project.name, &agent.name)
        .await;
    let session = kestrel.enqueue_session(workspace.id).await;

    (kestrel, workspace, session)
}

#[tokio::test]
async fn a_session_in_the_workspace_names_the_session_to_stop_and_stays_rejected() {
    let (kestrel, workspace, session) = a_session_in_flight().await;
    let finished = ran_by(
        &kestrel,
        &[
            "session",
            "enqueue",
            "--workspace",
            &workspace.id.to_string(),
        ],
        Invocation::default(),
    )
    .await;

    exited(&finished, REJECTED);
    assert!(
        finished
            .err
            .contains(&format!("kestrel session stop {}", session.id)),
        "{}",
        finished.err
    );
    assert!(
        finished.err.contains("It happens only if you choose it."),
        "{}",
        finished.err
    );
    kestrel.teardown().await;
}

#[tokio::test]
async fn stopping_the_holding_session_takes_an_explicit_yes() {
    let (kestrel, workspace, session) = a_session_in_flight().await;
    let workspace = workspace.id.to_string();
    let enqueue = ["session", "enqueue", "--workspace", workspace.as_str()];
    let declined = ran_on_a_terminal_by(&kestrel, &enqueue, Invocation::default(), "2\n\n").await;
    assert_eq!(declined.status.code(), Some(REJECTED), "{}", declined.said);
    assert!(
        declined
            .said
            .contains(&format!("2. stop the Session {}", session.id)),
        "{}",
        declined.said
    );
    assert!(declined.said.contains("[y/N]"), "{}", declined.said);
    let id = session.id;
    assert!(
        kestrel.session(id).await.exit.is_none(),
        "{}",
        declined.said
    );

    let chosen = ran_on_a_terminal_by(&kestrel, &enqueue, Invocation::default(), "2\ny\n").await;
    assert_eq!(chosen.status.code(), Some(REJECTED), "{}", chosen.said);
    assert!(kestrel.session(id).await.exit.is_some(), "{}", chosen.said);
    assert_eq!(
        kestrel.sessions(session.workspace).await.len(),
        1,
        "the refused enqueue was replayed: {}",
        chosen.said
    );
    kestrel.teardown().await;
}

#[test]
fn credential_help_presents_set_and_forget_together() {
    let finished = ran(&nowhere(), &["credential", "--help"]);
    exited(&finished, SUCCESS);
    assert!(finished.out.join("\n").contains("credential set"));
    assert!(finished.out.join("\n").contains("credential forget"));
}

#[tokio::test]
async fn a_record_that_does_not_exist_is_unresolved() {
    let kestrel = an_organization().await;

    for args in [
        &["workspace", "show", "no-such-workspace"][..],
        &["trigger", "show", "no-such-trigger"],
        &["project", "list", "--organization", "globex"],
    ] {
        let finished = ran_by(&kestrel, args, Invocation::default()).await;

        exited(&finished, UNRESOLVED);
    }
    kestrel.teardown().await;
}

#[tokio::test]
async fn a_declined_operation_is_rejected() {
    let kestrel = an_organization().await;
    let declared = ran_by(
        &kestrel,
        &["profile", "declare", "max", "--owner", "max"],
        Invocation::default(),
    )
    .await;
    exited(&declared, SUCCESS);

    let taken = ran_by(
        &kestrel,
        &["profile", "declare", "max", "--owner", "someone-else"],
        Invocation::default(),
    )
    .await;
    let unacceptable = ran_by(
        &kestrel,
        &[
            "project",
            "declare",
            "kestrel",
            "--repository",
            "https://github.com/jtmthf/kestrel",
            "--branch",
            "",
        ],
        Invocation::default(),
    )
    .await;

    exited(&taken, REJECTED);
    exited(&unacceptable, REJECTED);
    kestrel.teardown().await;
}

#[test]
fn a_control_plane_nothing_answers_for_is_unavailable() {
    let control_plane = nowhere();
    let finished = ran(&control_plane, &["organization", "list"]);

    exited(&finished, UNAVAILABLE);
    assert!(
        finished
            .err
            .contains(&format!("check that the control plane at {control_plane}")),
        "{}",
        finished.err
    );
    assert!(!finished.err.contains("Caused by"), "{}", finished.err);
}

#[test]
fn an_unreachable_control_plane_is_a_typed_diagnostic_under_json() {
    let control_plane = nowhere();
    let finished = ran(&control_plane, &["organization", "list", "--json"]);

    exited(&finished, UNAVAILABLE);
    let diagnostic: Value =
        serde_json::from_str(finished.err.trim()).expect("stderr should be one diagnostic");
    assert_eq!(diagnostic["kind"], "connection_failed");
    assert_eq!(diagnostic["context"]["url"], control_plane);
    assert_eq!(diagnostic["next_steps"][0]["action"], "check_connection");
}
