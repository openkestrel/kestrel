mod support;

use std::path::PathBuf;
use std::time::Duration;

use kestrel::domain::{Session, SessionState, Workspace};
use kestrel::log::Entry;
use reqwest::StatusCode;
use serde_json::{Value, json};
use support::environment::Environment;
use support::scripted_agent::{self, Script};
use support::{Kestrel, client, operator_log, repository, supervisor};

const INLINE: usize = 1024 * 1024;

async fn workspace(kestrel: &Kestrel) -> Workspace {
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
    assert!(shown.contains("5000 of 5001 entries shown"), "{shown}");
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
async fn a_turn_that_is_working_does_not_hold_up_a_read_nor_does_a_read_take_the_index_lock() {
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
    std::fs::write(checkout.join(".git/index.lock"), "held by the agent").unwrap();

    let listing = listed(&kestrel, &workspace, "kestrel").await;

    assert!(marks(&listing).contains(&("in-progress.txt".to_owned(), "untracked".to_owned())));
    assert_eq!(
        std::fs::read_to_string(checkout.join(".git/index.lock")).unwrap(),
        "held by the agent"
    );
    assert_eq!(
        kestrel.session(session.id).await.state,
        SessionState::Working
    );
    std::fs::remove_file(checkout.join(".git/index.lock")).unwrap();
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
        .while_the_database_is_locked(async {
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
    kestrel.teardown().await;
}

#[tokio::test]
async fn a_workspace_with_no_instance_has_nothing_to_read() {
    let kestrel = Kestrel::boot().await;
    let workspace = workspace(&kestrel).await;

    let response = get(&kestrel, &workspace, "files", &[]).await;

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let refusal: Value = response.json().await.unwrap();
    assert!(
        refusal["message"]
            .as_str()
            .unwrap()
            .contains(&workspace.checkout.branch),
        "{refusal}"
    );
    kestrel.teardown().await;
}
