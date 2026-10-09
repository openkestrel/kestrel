use crate::support;

use std::path::PathBuf;
use std::time::Duration;

use kestrel::domain::{Session, SessionState, Workspace};
use kestrel::log::Entry;
use reqwest::StatusCode;
use serde_json::{Value, json};
use support::environment::Environment;
use support::fixture::Fixture;
use support::scripted_agent::{self, Script};
use support::{Kestrel, client, operator_log, repository, supervisor};

const INLINE: usize = 1024 * 1024;

async fn workspace(kestrel: &Kestrel) -> Workspace {
    Fixture::acme()
        .project("project")
        .repositories(&[repository::url(), repository::other_url()])
        .holding_a_provider_key()
        .open(kestrel)
        .await
}

/// A Workspace whose Session has ended, leaving its Instance held and its supervisor on the link.
async fn held(script: Script) -> (Kestrel, Workspace, PathBuf) {
    let kestrel =
        Kestrel::dispatching_to(supervisor::binary(), &scripted_agent::playing(script)).await;
    let workspace = workspace(&kestrel).await;
    let session = kestrel.post(workspace.id, "operator", "begin").await;
    kestrel.answered(session.id, 1).await;
    let checkout = checkout_of(&kestrel.session(session.id).await);
    kestrel.stop_session(session.id).await;

    (kestrel, workspace, checkout)
}

fn checkout_of(session: &Session) -> PathBuf {
    Environment::root_of(session.instance.as_deref().expect("an instance")).join(repository::NAME)
}

async fn get(
    kestrel: &Kestrel,
    workspace: &Workspace,
    read: &str,
    query: &[(&str, &str)],
) -> reqwest::Response {
    let mut url = reqwest::Url::parse(&format!(
        "{}/operator/organizations/acme/workspaces/{}/{read}",
        kestrel.operator(),
        workspace.id
    ))
    .expect("a url");
    url.query_pairs_mut().extend_pairs(query);
    reqwest::Client::new()
        .get(url)
        .send()
        .await
        .expect("the operator boundary should answer")
}

async fn listed(kestrel: &Kestrel, workspace: &Workspace, path: &str) -> Value {
    let response = get(kestrel, workspace, "files", &[("path", path)]).await;
    assert_eq!(response.status(), StatusCode::OK, "{path}");
    response.json().await.expect("a listing")
}

async fn read(kestrel: &Kestrel, workspace: &Workspace, path: &str) -> (String, Vec<u8>) {
    let response = get(kestrel, workspace, "file", &[("path", path)]).await;
    assert_eq!(response.status(), StatusCode::OK, "{path}");
    let kind = response.headers()[reqwest::header::CONTENT_TYPE]
        .to_str()
        .expect("a content type")
        .to_owned();
    (kind, response.bytes().await.expect("a body").to_vec())
}

fn marks(listing: &Value) -> Vec<(String, String)> {
    listing["entries"]
        .as_array()
        .expect("entries")
        .iter()
        .map(|entry| {
            (
                entry["name"].as_str().expect("a name").to_owned(),
                entry["git"].as_str().unwrap_or("-").to_owned(),
            )
        })
        .collect()
}

/// The installed Client's standard output as bytes, which the line-reading harness cannot keep.
async fn cli_bytes(kestrel: &Kestrel, arguments: &[&str]) -> Vec<u8> {
    let mut command = std::process::Command::new(client::binary());
    command
        .args(arguments)
        .env_clear()
        .env("KESTREL_CONTROL_PLANE", kestrel.operator())
        .env("KESTREL_ORGANIZATION", "acme");
    let ran = tokio::task::spawn_blocking(move || command.output())
        .await
        .expect("the client should run")
        .expect("the client should spawn");
    assert!(
        ran.status.success(),
        "{}",
        String::from_utf8_lossy(&ran.stderr)
    );
    ran.stdout
}

#[tokio::test]
async fn a_held_instance_is_listed_and_read_through_the_operator_boundary_and_the_client() {
    let (kestrel, workspace, checkout) = held(Script::Speaks).await;
    std::fs::write(checkout.join(".git/info/exclude"), "built/\n").unwrap();
    std::fs::create_dir(checkout.join("built")).unwrap();
    std::fs::write(checkout.join("built/output"), "ignored").unwrap();
    std::fs::write(checkout.join("new.txt"), "untracked\n").unwrap();
    let large: Vec<u8> = (0..INLINE + 10)
        .map(|index| b'a' + (index % 26) as u8)
        .collect();
    std::fs::write(checkout.join("large.txt"), &large).unwrap();
    let binary: Vec<u8> = (0..=255).collect();
    std::fs::write(checkout.join("binary.bin"), &binary).unwrap();

    assert_eq!(
        marks(&listed(&kestrel, &workspace, "").await),
        [("kestrel", "-"), ("companion", "-")].map(|(name, git)| (name.to_owned(), git.to_owned()))
    );
    let listing = listed(&kestrel, &workspace, "kestrel").await;
    for (name, git) in [
        ("README.md", "tracked"),
        ("new.txt", "untracked"),
        ("built", "ignored"),
    ] {
        assert!(
            marks(&listing).contains(&(name.to_owned(), git.to_owned())),
            "{name} is not {git}: {listing}"
        );
    }
    assert_eq!(listing["truncated"], false);

    assert_eq!(
        read(&kestrel, &workspace, "kestrel/README.md").await,
        (
            "application/json".to_owned(),
            serde_json::to_vec(
                &json!({"path": "kestrel/README.md", "text": "a project's repository\n"})
            )
            .unwrap()
        )
    );
    for (path, bytes) in [
        ("kestrel/large.txt", &large),
        ("kestrel/binary.bin", &binary),
    ] {
        assert_eq!(
            read(&kestrel, &workspace, path).await,
            ("application/octet-stream".to_owned(), bytes.clone()),
            "{path}"
        );
    }

    let workspace_id = workspace.id.to_string();
    for command in ["files", "ls"] {
        let out = cli_bytes(
            &kestrel,
            &["workspace", command, &workspace_id, "kestrel", "--json"],
        )
        .await;
        assert_eq!(serde_json::from_slice::<Value>(&out).unwrap(), listing);
    }
    let shown = String::from_utf8(
        cli_bytes(&kestrel, &["workspace", "ls", &workspace_id, "kestrel"]).await,
    )
    .unwrap();
    for text in [
        "tracked    README.md",
        "untracked  new.txt",
        "ignored    built/",
    ] {
        assert!(shown.contains(text), "missing {text}: {shown}");
    }
    for command in ["read", "cat"] {
        assert_eq!(
            cli_bytes(
                &kestrel,
                &["workspace", command, &workspace_id, "kestrel/binary.bin"]
            )
            .await,
            binary
        );
    }
    assert_eq!(
        cli_bytes(
            &kestrel,
            &["workspace", "cat", &workspace_id, "kestrel/large.txt"]
        )
        .await,
        large
    );
    assert_eq!(
        cli_bytes(
            &kestrel,
            &["workspace", "cat", &workspace_id, "kestrel/README.md"]
        )
        .await,
        b"a project's repository\n"
    );
    let text = cli_bytes(
        &kestrel,
        &[
            "workspace",
            "cat",
            &workspace_id,
            "kestrel/README.md",
            "--json",
        ],
    )
    .await;
    assert_eq!(
        serde_json::from_slice::<Value>(&text).unwrap()["text"],
        "a project's repository\n"
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_directory_past_five_thousand_entries_is_truncated_with_a_note() {
    let (kestrel, workspace, checkout) = held(Script::Speaks).await;
    std::fs::create_dir(checkout.join("many")).unwrap();
    for index in 0..5_001 {
        std::fs::write(checkout.join(format!("many/{index:05}")), "").unwrap();
    }

    let listing = listed(&kestrel, &workspace, "kestrel/many").await;

    assert_eq!(listing["entries"].as_array().unwrap().len(), 5_000);
    assert_eq!(listing["total"], 5_001);
    assert_eq!(listing["truncated"], true);
    let shown = String::from_utf8(
        cli_bytes(
            &kestrel,
            &[
                "workspace",
                "files",
                &workspace.id.to_string(),
                "kestrel/many",
            ],
        )
        .await,
    )
    .unwrap();
    assert_eq!(shown.lines().count(), 5_000);
    assert!(!shown.contains("entries shown"), "{shown}");
    let terminal = client::ran_on_a_terminal_by(
        &kestrel,
        &[
            "workspace",
            "files",
            &workspace.id.to_string(),
            "kestrel/many",
        ],
        client::Invocation::default(),
        "",
    )
    .await;
    assert!(terminal.status.success(), "{}", terminal.said);
    assert!(
        terminal.said.contains("5000 of 5001 entries shown"),
        "{}",
        terminal.said
    );
    kestrel.teardown().await;
}

#[tokio::test]
async fn a_path_or_symlink_resolving_outside_the_checkouts_is_refused() {
    let (kestrel, workspace, checkout) = held(Script::Speaks).await;
    let outside = checkout
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("kestrel-outside-a-checkout");
    std::fs::write(&outside, "not the workspace's\n").unwrap();
    std::os::unix::fs::symlink(&outside, checkout.join("escape")).unwrap();

    for path in ["kestrel/escape", "kestrel/../../kestrel-outside-a-checkout"] {
        let response = get(&kestrel, &workspace, "file", &[("path", path)]).await;
        assert_eq!(
            response.status(),
            StatusCode::UNPROCESSABLE_ENTITY,
            "{path}"
        );
        let refusal: Value = response.json().await.unwrap();
        assert!(
            refusal["message"].as_str().unwrap().contains("outside"),
            "{refusal}"
        );
    }
    let response = get(
        &kestrel,
        &workspace,
        "file",
        &[("path", "kestrel/absent.md")],
    )
    .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    std::fs::remove_file(outside).unwrap();
    kestrel.teardown().await;
}

#[tokio::test]
async fn a_turn_that_is_working_does_not_hold_up_a_read() {
    let kestrel = Kestrel::dispatching_to(
        supervisor::binary(),
        &scripted_agent::playing(Script::Dawdles),
    )
    .await;
    let workspace = workspace(&kestrel).await;
    let session = kestrel.post(workspace.id, "operator", "keep working").await;
    while !kestrel
        .transcript(workspace.id)
        .await
        .iter()
        .any(|entry| matches!(entry.entry, Entry::SessionStarted { .. }))
    {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let checkout = checkout_of(&kestrel.session(session.id).await);
    std::fs::write(checkout.join("in-progress.txt"), "mid-turn\n").unwrap();

    let listing = listed(&kestrel, &workspace, "kestrel").await;

    assert!(marks(&listing).contains(&("in-progress.txt".to_owned(), "untracked".to_owned())));
    assert_eq!(
        kestrel.session(session.id).await.state,
        SessionState::Working
    );
    kestrel.teardown().await;
}

#[tokio::test]
async fn reads_leave_no_trace_and_two_identical_ones_at_once_send_one_request_down_the_link() {
    let log = operator_log::capturing();
    let (kestrel, workspace, _) = held(Script::Speaks).await;
    let recorded = format!("{:?}", kestrel.show_workspace(workspace.id).await);
    let transcript = kestrel.transcript(workspace.id).await.len();
    let events = kestrel.events("acme").await.len();

    let (one, other) = kestrel
        .database()
        .while_locked(async {
            tokio::join!(
                listed(&kestrel, &workspace, "kestrel"),
                listed(&kestrel, &workspace, "kestrel")
            )
        })
        .await;

    assert_eq!(one, other);
    let instance = kestrel.sessions(workspace.id).await[0]
        .instance
        .clone()
        .expect("an instance");
    assert_eq!(
        log.containing("a read went down the link")
            .iter()
            .filter(|line| line.contains(&instance))
            .count(),
        1
    );
    assert_eq!(kestrel.transcript(workspace.id).await.len(), transcript);
    assert_eq!(kestrel.events("acme").await.len(), events);
    assert_eq!(
        format!("{:?}", kestrel.show_workspace(workspace.id).await),
        recorded
    );
    kestrel.teardown().await;
}

#[cfg(unix)]
#[tokio::test]
async fn a_read_of_an_instance_whose_supervisor_has_stopped_fails_saying_it_did_not_answer() {
    let kestrel = Kestrel::dispatching_to(
        supervisor::binary(),
        &scripted_agent::playing(Script::Speaks),
    )
    .await;
    let workspace = workspace(&kestrel).await;
    let session = kestrel.post(workspace.id, "operator", "begin").await;
    let waiting = kestrel.answered(session.id, 1).await;
    kestrel.stop_session(session.id).await;
    let process = Environment::named(waiting.supervisor.as_deref().unwrap());
    #[allow(unsafe_code)]
    unsafe {
        libc::kill(process.pid(), libc::SIGKILL);
    }
    process.is_gone().await;

    let asked = tokio::time::Instant::now();
    let response = get(&kestrel, &workspace, "files", &[("path", "kestrel")]).await;

    assert!(
        asked.elapsed() < Duration::from_secs(12),
        "{:?}",
        asked.elapsed()
    );
    assert_eq!(response.status(), StatusCode::GATEWAY_TIMEOUT);
    let refusal: Value = response.json().await.unwrap();
    assert_eq!(refusal["message"], "the Instance didn't answer");
    assert_eq!(refusal["kind"], "instance_timeout");
    assert_eq!(refusal["context"]["workspace"], workspace.id.to_string());
    assert_eq!(refusal["context"]["operation"], "workspace_files");
    assert_eq!(refusal["next_steps"][0]["action"], "retry_read");
    assert_eq!(refusal["next_steps"][0]["operation"], "workspace_files");
    kestrel.teardown().await;
}

#[tokio::test]
async fn a_workspace_with_no_instance_has_nothing_to_read() {
    let kestrel = Kestrel::boot().await;
    let workspace = workspace(&kestrel).await;

    let response = get(&kestrel, &workspace, "files", &[]).await;

    assert_eq!(response.status(), StatusCode::CONFLICT);
    let refusal: Value = response.json().await.unwrap();
    assert!(
        refusal["message"]
            .as_str()
            .unwrap()
            .contains(&workspace.checkout.branch),
        "{refusal}"
    );
    assert_eq!(refusal["kind"], "state_conflict");
    assert_eq!(refusal["context"]["state"], "no_instance");
    assert_eq!(refusal["next_steps"][0]["action"], "inspect_resource");
    kestrel.teardown().await;
}

fn git_in(checkout: &std::path::Path, arguments: &[&str]) -> String {
    let output = support::git::command()
        .current_dir(checkout)
        .args(arguments)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

async fn changes(kestrel: &Kestrel, workspace: &Workspace, query: &[(&str, &str)]) -> Value {
    let response = get(kestrel, workspace, "changes", query).await;
    assert_eq!(response.status(), StatusCode::OK);
    response.json().await.unwrap()
}

#[tokio::test]
async fn changes_include_every_unpublished_scope_and_the_client_filters_paths() {
    let (kestrel, workspace, checkout) = held(Script::Speaks).await;
    let declared = git_in(&checkout, &["branch", "--show-current"]);
    git_in(
        &checkout,
        &["checkout", "-b", "advanced-base", "origin/main"],
    );
    std::fs::write(
        checkout.join("base-only.txt"),
        "published on base after the branch was cut\n",
    )
    .unwrap();
    git_in(&checkout, &["add", "base-only.txt"]);
    git_in(&checkout, &["commit", "-m", "base advanced"]);
    git_in(
        &checkout,
        &["update-ref", "refs/remotes/origin/main", "HEAD"],
    );
    git_in(&checkout, &["checkout", &declared]);
    std::fs::write(checkout.join("committed.txt"), "unpublished commit\n").unwrap();
    git_in(&checkout, &["add", "committed.txt"]);
    git_in(&checkout, &["commit", "-m", "unpublished"]);
    let commit = git_in(&checkout, &["rev-parse", "HEAD"]);
    std::fs::write(checkout.join("staged.txt"), "in the index\n").unwrap();
    git_in(&checkout, &["add", "staged.txt"]);
    std::fs::write(checkout.join("README.md"), "unstaged\n").unwrap();
    std::fs::write(checkout.join("new.txt"), "untracked\n").unwrap();
    std::fs::write(
        checkout.parent().unwrap().join("companion/new.txt"),
        "second repo\n",
    )
    .unwrap();
    let recorded = format!("{:?}", kestrel.show_workspace(workspace.id).await);
    let transcript = kestrel.transcript(workspace.id).await.len();
    let events = kestrel.events("acme").await.len();
    let index = std::fs::read(checkout.join(".git/index")).unwrap();
    std::fs::write(checkout.join(".git/index.lock"), "agent holds the index").unwrap();

    let answer = changes(&kestrel, &workspace, &[]).await;
    assert_eq!(answer["repositories"].as_array().unwrap().len(), 2);
    let diff = answer["repositories"][0]["diff"].as_str().unwrap();
    for path in ["committed.txt", "staged.txt", "README.md", "new.txt"] {
        assert!(diff.contains(&format!("b/kestrel/{path}")), "{diff}");
    }
    assert!(
        answer["repositories"][1]["diff"]
            .as_str()
            .unwrap()
            .contains("b/companion/new.txt")
    );
    assert!(!diff.contains("base-only.txt"), "{diff}");
    let files = answer["repositories"][0]["files"].as_array().unwrap();
    assert!(
        files
            .iter()
            .any(|file| file["path"] == "kestrel/new.txt" && file["added"] == 1)
    );

    for (scope, present, absent) in [
        ("changed".to_owned(), "README.md", "staged.txt"),
        ("staged".to_owned(), "staged.txt", "README.md"),
        (format!("commit:{commit}"), "committed.txt", "staged.txt"),
    ] {
        let scoped = changes(&kestrel, &workspace, &[("scope", &scope)]).await;
        let diff = scoped["repositories"][0]["diff"].as_str().unwrap();
        assert!(diff.contains(present), "{scope}: {diff}");
        assert!(!diff.contains(absent), "{scope}: {diff}");
        let limited = changes(
            &kestrel,
            &workspace,
            &[("scope", &scope), ("path", "kestrel/new.txt")],
        )
        .await;
        assert_eq!(limited["repositories"][0]["diff"], "");
        assert_eq!(limited["repositories"].as_array().unwrap().len(), 1);
    }
    let id = workspace.id.to_string();
    for command in ["changes", "diff"] {
        let shown = cli_bytes(&kestrel, &["workspace", command, &id, "--json"]).await;
        assert_eq!(serde_json::from_slice::<Value>(&shown).unwrap(), answer);
        let shown = cli_bytes(
            &kestrel,
            &["workspace", command, &id, "--", "kestrel/new.txt"],
        )
        .await;
        let shown = String::from_utf8(shown).unwrap();
        assert!(shown.contains("+untracked"), "{shown}");
        assert!(!shown.contains("staged.txt"), "{shown}");
    }
    for (arguments, present) in [
        (vec!["workspace", "diff", &id, "--staged"], "+in the index"),
        (vec!["workspace", "diff", &id, "--changed"], "+unstaged"),
        (
            vec![
                "workspace",
                "diff",
                &id,
                &commit,
                "--",
                "kestrel/committed.txt",
            ],
            "+unpublished commit",
        ),
    ] {
        let shown = String::from_utf8(cli_bytes(&kestrel, &arguments).await).unwrap();
        assert!(shown.contains(present), "{shown}");
    }
    assert_eq!(std::fs::read(checkout.join(".git/index")).unwrap(), index);
    assert_eq!(kestrel.transcript(workspace.id).await.len(), transcript);
    assert_eq!(kestrel.events("acme").await.len(), events);
    assert_eq!(
        format!("{:?}", kestrel.show_workspace(workspace.id).await),
        recorded
    );
    std::fs::remove_file(checkout.join(".git/index.lock")).unwrap();
    kestrel.teardown().await;
}

#[tokio::test]
async fn changes_after_a_push_use_the_declared_remote_and_commits_include_other_branches() {
    let (kestrel, workspace, checkout) = held(Script::Speaks).await;
    let declared = git_in(&checkout, &["branch", "--show-current"]);
    std::fs::write(checkout.join("README.md"), "pushed work\n").unwrap();
    git_in(&checkout, &["commit", "-am", "published"]);
    git_in(&checkout, &["push", "origin", &declared]);
    std::fs::write(checkout.join("README.md"), "unpublished work\n").unwrap();
    git_in(&checkout, &["commit", "-am", "unpushed on declared"]);
    git_in(&checkout, &["checkout", "-b", "side", "origin/main"]);
    std::fs::write(checkout.join("README.md"), "side branch\n").unwrap();
    git_in(&checkout, &["commit", "-am", "unpushed on side"]);
    git_in(&checkout, &["checkout", &declared]);
    std::fs::write(checkout.join("README.md"), "parked\n").unwrap();
    git_in(&checkout, &["stash", "push", "-m", "parked work"]);
    git_in(&checkout, &["checkout", "--detach"]);
    std::fs::write(checkout.join("README.md"), "detached work\n").unwrap();
    git_in(&checkout, &["commit", "-am", "unpushed detached"]);
    let diff = changes(&kestrel, &workspace, &[]).await;
    let diff = diff["repositories"][0]["diff"].as_str().unwrap();
    assert!(diff.contains("-pushed work"), "{diff}");
    assert!(diff.contains("+detached work"), "{diff}");
    let id = workspace.id.to_string();
    for (read, alias, expected) in [
        (
            "commits",
            "log",
            vec![
                "unpushed on side",
                "unpushed on declared",
                "unpushed detached",
            ],
        ),
        ("stashes", "stash", vec!["stash@{0}", "parked work"]),
    ] {
        let response = get(&kestrel, &workspace, read, &[]).await;
        assert_eq!(response.status(), StatusCode::OK);
        let answer: Value = response.json().await.unwrap();
        assert_eq!(answer["repositories"].as_array().unwrap().len(), 2);
        let text = answer["repositories"][0]["text"].as_str().unwrap();
        for expected in expected {
            assert!(text.contains(expected), "{text}");
        }
        assert!(!text.contains("diff --git"), "{text}");
        if read == "commits" {
            assert!(!text.contains("    published\n"), "{text}");
        }
        for command in [read, alias] {
            let shown = cli_bytes(&kestrel, &["workspace", command, &id, "--json"]).await;
            assert_eq!(serde_json::from_slice::<Value>(&shown).unwrap(), answer);
            let shown =
                String::from_utf8(cli_bytes(&kestrel, &["workspace", command, &id]).await).unwrap();
            assert!(shown.contains(text), "{shown}");
        }
    }
    kestrel.teardown().await;
}

#[tokio::test]
async fn changes_over_two_mebibytes_keep_complete_stats_and_refuse_escaping_paths() {
    let (kestrel, workspace, checkout) = held(Script::Speaks).await;
    std::fs::write(checkout.join("large.txt"), "large line\n".repeat(220_000)).unwrap();
    std::fs::write(checkout.join("small.txt"), "one\ntwo\n").unwrap();
    std::fs::write(checkout.join("binary"), [0, 1, 2]).unwrap();
    std::fs::write(
        checkout.parent().unwrap().join("companion/large.txt"),
        "other line\n".repeat(220_000),
    )
    .unwrap();
    let answer = changes(&kestrel, &workspace, &[]).await;
    let repository = &answer["repositories"][0];
    assert_eq!(repository["truncated"], true);
    assert!(repository["diff"].as_str().unwrap().len() <= 2 * 1024 * 1024);
    let files = repository["files"].as_array().unwrap();
    assert!(
        files
            .iter()
            .any(|file| file["path"] == "kestrel/large.txt" && file["added"] == 220_000)
    );
    assert!(
        files
            .iter()
            .any(|file| file["path"] == "kestrel/small.txt" && file["added"] == 2)
    );
    assert!(
        files
            .iter()
            .any(|file| file["path"] == "kestrel/binary" && file["added"].is_null())
    );
    assert!(
        answer["repositories"]
            .as_array()
            .unwrap()
            .iter()
            .map(|repository| repository["diff"].as_str().unwrap().len())
            .sum::<usize>()
            <= 2 * 1024 * 1024
    );
    assert_eq!(answer["repositories"][1]["truncated"], true);
    assert_eq!(answer["repositories"][1]["files"][0]["added"], 220_000);
    let shown = String::from_utf8(
        cli_bytes(&kestrel, &["workspace", "diff", &workspace.id.to_string()]).await,
    )
    .unwrap();
    assert!(!shown.contains("diff truncated"));
    assert!(shown.starts_with("diff --git"));
    let terminal = client::ran_on_a_terminal_by(
        &kestrel,
        &["workspace", "diff", &workspace.id.to_string()],
        client::Invocation::default(),
        "",
    )
    .await;
    assert!(terminal.status.success(), "{}", terminal.said);
    assert!(
        terminal.said.contains("diff truncated"),
        "{}",
        terminal.said
    );
    std::os::unix::fs::symlink("/etc/hosts", checkout.join("escape")).unwrap();
    for path in [
        "kestrel/../../etc/hosts",
        "kestrel/escape",
        "kestrel/.git/config",
    ] {
        let response = get(&kestrel, &workspace, "changes", &[("path", path)]).await;
        assert_eq!(
            response.status(),
            StatusCode::UNPROCESSABLE_ENTITY,
            "{path}"
        );
    }
    assert_eq!(
        get(&kestrel, &workspace, "changes", &[("scope", "bad")])
            .await
            .status(),
        StatusCode::BAD_REQUEST
    );
    kestrel.teardown().await;
}

#[tokio::test]
async fn changes_include_files_in_an_untracked_nested_repository() {
    let (kestrel, workspace, checkout) = held(Script::Speaks).await;
    let nested = checkout.join("nested");
    std::fs::create_dir(&nested).unwrap();
    git_in(&nested, &["init", "--initial-branch", "main"]);
    std::fs::write(nested.join("tracked.txt"), "nested tracked\n").unwrap();
    git_in(&nested, &["add", "tracked.txt"]);
    git_in(&nested, &["commit", "-m", "nested work"]);
    std::fs::write(nested.join("new.txt"), "nested untracked\n").unwrap();
    let answer = changes(&kestrel, &workspace, &[]).await;
    let repository = &answer["repositories"][0];
    for path in ["tracked.txt", "new.txt"] {
        assert!(
            repository["diff"]
                .as_str()
                .unwrap()
                .contains(&format!("b/kestrel/nested/{path}")),
            "{repository}"
        );
        assert!(
            repository["files"]
                .as_array()
                .unwrap()
                .iter()
                .any(|file| file["path"] == format!("kestrel/nested/{path}") && file["added"] == 1)
        );
    }
    for path in ["kestrel/nested/new.txt", "kestrel/./nested/new.txt"] {
        let selected = changes(&kestrel, &workspace, &[("path", path)]).await;
        assert_eq!(
            selected["repositories"][0]["files"]
                .as_array()
                .unwrap()
                .len(),
            1,
            "{path}"
        );
        assert_eq!(
            selected["repositories"][0]["files"][0]["path"],
            "kestrel/nested/new.txt"
        );
    }
    let selected = changes(&kestrel, &workspace, &[("path", "kestrel/.")]).await;
    assert_eq!(
        selected["repositories"][0]["files"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    for path in [
        "kestrel/nested/new.txt/",
        "kestrel/nested/new.txt/.",
        "kestrel/nested/new.txt/./.",
    ] {
        let selected = changes(&kestrel, &workspace, &[("path", path)]).await;
        assert!(
            selected["repositories"][0]["files"]
                .as_array()
                .unwrap()
                .is_empty(),
            "{path}"
        );
    }
    std::fs::remove_file(nested.join("tracked.txt")).unwrap();
    let selected = changes(&kestrel, &workspace, &[("path", "kestrel/nested/")]).await;
    assert_eq!(
        selected["repositories"][0]["files"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert!(
        !repository["diff"]
            .as_str()
            .unwrap()
            .contains("nested/.git/")
    );
    kestrel.teardown().await;
}
