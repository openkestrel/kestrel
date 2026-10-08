mod support;

use serde_json::{Value, json};
use support::{Kestrel, repository};

async fn workspace(kestrel: &Kestrel) -> kestrel::domain::Workspace {
    let organization = kestrel.declare_organization("acme").await;
    kestrel
        .declare_project(
            &organization,
            "project",
            &[
                repository::url().to_owned(),
                repository::other_url().to_owned(),
            ],
            "main",
        )
        .await;
    kestrel
        .declare_agent(&organization, "builder", support::HARNESS, None)
        .await;
    kestrel
        .hold_provider_credential(
            &organization,
            support::PROVIDER_KEY,
            support::A_PROVIDER_KEY,
        )
        .await;
    kestrel.open_workspace("acme", "project", "builder").await
}

async fn summary(kestrel: &Kestrel, workspace: &kestrel::domain::Workspace) -> Value {
    let response = reqwest::get(format!(
        "{}/operator/organizations/acme/workspaces/{}/work",
        kestrel.operator(),
        workspace.id
    ))
    .await
    .unwrap();
    assert!(response.status().is_success(), "{}", response.status());
    response.json().await.unwrap()
}

#[tokio::test]
async fn a_workspace_with_no_instance_names_its_declared_branch_without_recording_the_read() {
    let kestrel = Kestrel::boot().await;
    let workspace = workspace(&kestrel).await;
    let before = kestrel
        .transcript(workspace.id)
        .await
        .iter()
        .map(|entry| entry.seq)
        .collect::<Vec<_>>();
    assert_eq!(
        summary(&kestrel, &workspace).await,
        json!({
            "state": "no_instance",
            "branch": workspace.checkout.branch,
            "pull_request": null,
            "last_report": {"report": "none"}
        })
    );
    let shown = support::client::ran_by(
        &kestrel,
        &["workspace", "work", &workspace.id.to_string()],
        support::client::Invocation::default(),
    )
    .await;
    assert!(shown.status.success(), "{}", shown.err);
    assert!(
        shown
            .out
            .iter()
            .any(|line| line == "No work report received."),
        "{:?}",
        shown.out
    );
    assert_eq!(
        kestrel
            .transcript(workspace.id)
            .await
            .iter()
            .map(|entry| entry.seq)
            .collect::<Vec<_>>(),
        before
    );
    kestrel.teardown().await;
}

async fn reported(
    kestrel: &Kestrel,
    workspace: &kestrel::domain::Workspace,
    untracked: u64,
) -> Value {
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        let answer = summary(kestrel, workspace).await;
        if answer["state"] == "reported" && answer["repositories"][0]["untracked"] == untracked {
            return answer;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "never reported: {answer}"
        );
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
}

fn git(directory: &std::path::Path, args: &[&str]) -> String {
    let output = support::git::command()
        .current_dir(directory)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

#[tokio::test]
async fn a_held_instance_reports_all_work_on_each_repository_and_the_cli_reads_it() {
    use support::{
        client,
        environment::Environment,
        scripted_agent::{self, Script},
        supervisor,
    };
    let kestrel = Kestrel::dispatching_to(
        supervisor::binary(),
        &scripted_agent::playing(Script::Speaks),
    )
    .await;
    let workspace = workspace(&kestrel).await;
    let session = kestrel.post(workspace.id, "operator", "begin").await;
    kestrel.answered(session.id, 1).await;
    let started = kestrel.session(session.id).await;
    assert_eq!(
        started.state,
        kestrel::domain::SessionState::Waiting,
        "{started:?}"
    );
    let root = Environment::root_of(started.instance.as_deref().unwrap());
    let checkout = root.join(repository::NAME);
    std::fs::write(checkout.join("README.md"), "pushed\n").unwrap();
    git(&checkout, &["commit", "-am", "published"]);
    git(&checkout, &["push", "origin", &workspace.checkout.branch]);
    let pushed = git(&checkout, &["rev-parse", "HEAD"]);
    git(&checkout, &["checkout", "-b", "side"]);
    std::fs::write(checkout.join("README.md"), "side\nmore\n").unwrap();
    git(&checkout, &["commit", "-am", "unpublished side branch"]);
    git(&checkout, &["checkout", &workspace.checkout.branch]);
    std::fs::write(checkout.join("README.md"), "stashed\n").unwrap();
    git(&checkout, &["stash"]);
    std::fs::write(checkout.join("README.md"), "staged\n").unwrap();
    git(&checkout, &["add", "README.md"]);
    std::fs::write(checkout.join("README.md"), "changed\nextra\n").unwrap();
    std::fs::write(checkout.join("new.txt"), "new\n").unwrap();
    kestrel
        .post(workspace.id, "operator", "finish the turn")
        .await;
    kestrel.answered(session.id, 2).await;
    let closed = reported(&kestrel, &workspace, 1).await;
    kestrel.stop_session(session.id).await;
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(30);
    let answer = loop {
        let answer = reported(&kestrel, &workspace, 1).await;
        if answer["reported_at"].as_str().unwrap() > closed["reported_at"].as_str().unwrap() {
            break answer;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the stop never reported"
        );
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    };
    assert_eq!(
        answer["repositories"][0],
        json!({
            "repository": "kestrel", "git": "read", "branch": workspace.checkout.branch,
            "changed": {"files": 1, "added": 2, "removed": 1},
            "staged": {"files": 1, "added": 1, "removed": 1},
            "committed": {"commits": 1, "added": 2, "removed": 1},
            "pushed": pushed, "untracked": 1, "stashed": 1
        })
    );
    assert_eq!(answer["repositories"][1]["repository"], "companion");
    assert_eq!(answer["repositories"][1]["committed"]["commits"], 0);
    let before = kestrel.transcript(workspace.id).await.len();
    let under_write_lock = kestrel
        .while_the_database_is_locked(summary(&kestrel, &workspace))
        .await;
    assert_eq!(under_write_lock, answer);
    for command in ["work", "status"] {
        let result = client::ran_by(
            &kestrel,
            &["workspace", command, &workspace.id.to_string(), "--json"],
            client::Invocation::default(),
        )
        .await;
        assert!(result.status.success(), "{}", result.err);
        assert_eq!(
            serde_json::from_str::<Value>(&result.out.join("\n")).unwrap(),
            answer
        );
    }
    let result = client::ran_by(
        &kestrel,
        &["workspace", "status", &workspace.id.to_string()],
        client::Invocation::default(),
    )
    .await;
    assert!(result.status.success(), "{}", result.err);
    let out = result.out.join("\n");
    for text in [
        "Changed: 1 file",
        "Staged: 1 file",
        "Committed: 1 commit",
        "Untracked: 1",
        "Stashed: 1",
        &pushed,
    ] {
        assert!(out.contains(text), "missing {text}: {out}");
    }
    assert!(!out.contains("Reported by the supervisor"), "{out}");
    assert!(
        !out.lines()
            .any(|line| line == "kestrel" || line == "companion"),
        "{out}"
    );
    let terminal = client::ran_on_a_terminal_by(
        &kestrel,
        &["workspace", "status", &workspace.id.to_string()],
        client::Invocation::default(),
        "",
    )
    .await;
    assert!(terminal.status.success(), "{}", terminal.said);
    for text in ["kestrel", "companion", "Reported by the supervisor"] {
        assert!(
            terminal.said.contains(text),
            "missing {text}: {}",
            terminal.said
        );
    }
    assert_eq!(kestrel.transcript(workspace.id).await.len(), before);
    kestrel.teardown().await;
}

#[tokio::test]
async fn a_summary_changes_during_a_scripted_turn_and_returns_after_a_restart_between_sessions() {
    use support::{
        scripted_agent::{self, Script},
        supervisor,
    };
    let kestrel = Kestrel::dispatching_to(
        supervisor::binary(),
        &scripted_agent::playing(Script::Writes),
    )
    .await;
    let workspace = workspace(&kestrel).await;
    let session = kestrel.post(workspace.id, "operator", "write a file").await;
    let initial = reported(&kestrel, &workspace, 0).await;
    let started = kestrel.session(session.id).await;
    let checkout = support::environment::Environment::root_of(started.instance.as_deref().unwrap())
        .join(repository::NAME);
    std::fs::write(checkout.join(".git/index.lock"), "held by the agent").unwrap();
    assert!(initial["repositories"][0]["pushed"].is_null());
    let changed = reported(&kestrel, &workspace, 1).await;
    assert_eq!(
        std::fs::read_to_string(checkout.join(".git/index.lock")).unwrap(),
        "held by the agent"
    );
    std::fs::remove_file(checkout.join(".git/index.lock")).unwrap();
    assert!(changed["reported_at"].as_str().unwrap() > initial["reported_at"].as_str().unwrap());
    assert_eq!(
        kestrel.session(session.id).await.state,
        kestrel::domain::SessionState::Working
    );
    kestrel.answered(session.id, 1).await;
    let closed = reported(&kestrel, &workspace, 1).await;
    assert!(closed["reported_at"].as_str().unwrap() > changed["reported_at"].as_str().unwrap());
    kestrel.stop_session(session.id).await;
    let kestrel = kestrel.kill_and_restart().await;
    let reconnected = reported(&kestrel, &workspace, 1).await;
    assert!(reconnected["reported_at"].as_str().unwrap() > closed["reported_at"].as_str().unwrap());
    kestrel.teardown().await;
}

#[cfg(unix)]
#[tokio::test]
async fn a_supervisor_off_the_link_never_presents_its_turn_close_report_as_current() {
    use support::{
        environment::Environment,
        scripted_agent::{self, Script},
        supervisor,
    };
    let kestrel = Kestrel::dispatching_to(
        supervisor::binary(),
        &scripted_agent::playing(Script::Speaks),
    )
    .await;
    let workspace = workspace(&kestrel).await;
    let session = kestrel.post(workspace.id, "operator", "work").await;
    let waiting = kestrel.answered(session.id, 1).await;
    reported(&kestrel, &workspace, 0).await;
    kestrel.stop_session(session.id).await;
    let process = Environment::named(waiting.supervisor.as_deref().unwrap());
    #[allow(unsafe_code)]
    unsafe {
        libc::kill(process.pid(), libc::SIGKILL);
    }
    process.is_gone().await;
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        let answer = summary(&kestrel, &workspace).await;
        if answer["state"] == "not_answering" {
            assert!(answer.get("repositories").is_none());
            assert_eq!(answer["message"], "the Instance isn't answering");
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "still shows stale work: {answer}"
        );
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    kestrel.teardown().await;
}

#[tokio::test]
async fn stopping_a_turn_reports_work_written_since_its_last_sampling_tick() {
    use support::{
        environment::Environment,
        scripted_agent::{self, Script},
        supervisor,
    };
    let kestrel = Kestrel::dispatching_to(
        supervisor::binary(),
        &scripted_agent::playing(Script::Dawdles),
    )
    .await;
    let workspace = workspace(&kestrel).await;
    let session = kestrel.post(workspace.id, "operator", "keep working").await;
    reported(&kestrel, &workspace, 0).await;
    let working = kestrel.session(session.id).await;
    let checkout =
        Environment::root_of(working.instance.as_deref().unwrap()).join(repository::NAME);
    std::fs::write(
        checkout.join("last-write.txt"),
        "written just before stop\n",
    )
    .unwrap();
    kestrel.stop_session(session.id).await;
    let answer = reported(&kestrel, &workspace, 1).await;
    assert_eq!(answer["repositories"][0]["untracked"], 1);
    kestrel.teardown().await;
}

#[cfg(unix)]
#[tokio::test]
async fn the_last_report_outlives_its_instance_and_is_never_attributed_to_a_replacement() {
    use support::{
        environment::Environment,
        scripted_agent::{self, Script},
        supervisor,
    };
    let kestrel = Kestrel::dispatching_to(
        supervisor::binary(),
        &scripted_agent::playing(Script::Speaks),
    )
    .await;
    let workspace = workspace(&kestrel).await;
    let session = kestrel.post(workspace.id, "operator", "work").await;
    let waiting = kestrel.answered(session.id, 1).await;
    let first = waiting.instance.clone().unwrap();
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
    let live = loop {
        let answer = reported(&kestrel, &workspace, 0).await;
        if answer["last_report"]["reported_at"] == answer["reported_at"] {
            break answer;
        }
        assert!(tokio::time::Instant::now() < deadline, "{answer}");
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    };
    assert_eq!(
        live["last_report"],
        json!({
            "report": "received",
            "instance": first,
            "instance_current": true,
            "reported_at": live["reported_at"],
            "repositories": live["repositories"],
        })
    );

    kestrel.stop_session(session.id).await;
    let process = Environment::named(waiting.supervisor.as_deref().unwrap());
    #[allow(unsafe_code)]
    unsafe {
        libc::kill(process.pid(), libc::SIGKILL);
    }
    process.is_gone().await;
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
    let unanswered = loop {
        let answer = summary(&kestrel, &workspace).await;
        if answer["state"] == "not_answering" {
            break answer;
        }
        assert!(tokio::time::Instant::now() < deadline, "{answer}");
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    };
    let last = unanswered["last_report"].clone();
    assert_eq!(last["instance"], first);
    assert_eq!(last["instance_current"], true);
    assert_eq!(last["repositories"], live["repositories"]);

    assert_eq!(kestrel.release_instance(workspace.id).await, first);
    let mut historical = last.clone();
    historical["instance_current"] = json!(false);
    let released = summary(&kestrel, &workspace).await;
    assert_eq!(released["state"], "no_instance");
    assert_eq!(released["last_report"], historical);
    let shown = support::client::ran_by(
        &kestrel,
        &["workspace", "work", &workspace.id.to_string()],
        support::client::Invocation::default(),
    )
    .await;
    assert!(shown.status.success(), "{}", shown.err);
    assert!(
        shown
            .out
            .join("\n")
            .contains(&format!("Last reported by the earlier Instance {first} at")),
        "{:?}",
        shown.out
    );

    let kestrel = kestrel.kill_and_restart().await;
    assert_eq!(
        summary(&kestrel, &workspace).await["last_report"],
        historical
    );

    let replacement = kestrel.post(workspace.id, "operator", "again").await;
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        let answer = summary(&kestrel, &workspace).await;
        let last = &answer["last_report"];
        if last["instance"] == first {
            assert_eq!(*last, historical, "{answer}");
        } else {
            assert_eq!(last["report"], "received", "{answer}");
            assert_eq!(last["instance_current"], true, "{answer}");
            assert_eq!(
                Some(last["instance"].as_str().unwrap()),
                kestrel.session(replacement.id).await.instance.as_deref()
            );
            break;
        }
        assert!(tokio::time::Instant::now() < deadline, "{answer}");
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    kestrel.teardown().await;
}
