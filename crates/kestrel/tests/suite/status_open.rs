use crate::support::Kestrel;
use crate::support::client::{Invocation, ran_by, ran_on_a_terminal_by};
use serde_json::{Value, json};

#[tokio::test]
async fn status_leads_with_the_configured_client_and_counts_empty_scoped_work() {
    let kestrel = Kestrel::boot_with_the_client_at("http://localhost:7800").await;
    kestrel.declare_organization("acme").await;
    kestrel.declare_organization("globex").await;
    let args = ["status", "--organization", "acme"];
    let piped = ran_by(&kestrel, &args, Invocation::default()).await;
    assert_eq!(piped.status.code(), Some(78), "{}", piped.err);
    assert!(
        piped.out[0].contains("http://localhost:7800"),
        "{:?}",
        piped.out
    );
    assert!(piped.err.contains("kestrel operator name"), "{}", piped.err);
    let terminal = ran_on_a_terminal_by(&kestrel, &args, Invocation::default().given(""), "").await;
    assert_eq!(terminal.status.code(), Some(78));
    assert!(terminal.said.contains("http://localhost:7800"));
    for state in [
        "queued",
        "working",
        "trailing",
        "waiting",
        "unbriefed",
        "ended",
        "unreachable",
    ] {
        let label = format!("Sessions {state}");
        let line = terminal
            .said
            .lines()
            .find(|line| line.contains(&label))
            .expect("every Session state is visible");
        assert!(line.ends_with('0'), "{line}");
    }

    let read = ran_by(
        &kestrel,
        &["status", "--organization", "acme", "--json"],
        Invocation::default(),
    )
    .await;
    let status: Value = serde_json::from_str(&read.out[0]).expect("one complete status");
    assert_eq!(status["organization"], "acme");
    assert_eq!(status["readiness"]["outcome"], "blocked");
    assert_eq!(status["workspace_counts"], json!({"open": 0, "sealed": 0}));
    assert_eq!(
        status["session_counts"],
        json!({"queued": 0, "working": 0, "trailing": 0, "waiting": 0, "unbriefed": 0, "ended": 0, "unreachable": 0})
    );
    kestrel.teardown().await;
}

#[tokio::test]
async fn open_prints_the_client_root_over_ssh_without_requiring_ready_setup() {
    let kestrel = Kestrel::boot_with_the_client_at("http://localhost:7800").await;
    let remote = || {
        Invocation::default()
            .env("SSH_CONNECTION", "198.51.100.7 51000 203.0.113.10 2222")
            .env("USER", "ada")
    };
    let opened = ran_by(&kestrel, &["open"], remote()).await;
    assert_eq!(opened.status.code(), Some(0), "{}", opened.err);
    assert_eq!(opened.out, ["http://localhost:7800/"]);
    assert!(opened.err.contains("7800:localhost:7800"), "{}", opened.err);
    assert!(
        opened.err.contains("-p 2222 ada@203.0.113.10"),
        "{}",
        opened.err
    );
    let port = kestrel.operator_port();
    assert!(
        opened.err.contains(&format!("{port}:127.0.0.1:{port}")),
        "{}",
        opened.err
    );
    let terminal = ran_on_a_terminal_by(&kestrel, &["open"], remote(), "").await;
    assert_eq!(terminal.status.code(), Some(0), "{}", terminal.said);
    assert!(terminal.said.contains("http://localhost:7800/"));
    let partial = ran_by(
        &kestrel,
        &["open"],
        Invocation::default().env("SSH_CONNECTION", "198.51.100.7 51000 203.0.113.10 2222"),
    )
    .await;
    assert_eq!(partial.status.code(), Some(0));
    assert!(
        partial.err.contains("-p 2222 <ssh-user>@203.0.113.10"),
        "{}",
        partial.err
    );
    assert!(partial.err.contains("template"));
    kestrel.teardown().await;
}

#[tokio::test]
async fn open_resolves_explicit_and_latest_workspaces_without_hiding_missing_references() {
    use crate::support::fixture::Fixture;
    let kestrel = Kestrel::boot_with_the_client_at("http://localhost:7800").await;
    let fixture = Fixture::acme();
    let first = fixture.open(&kestrel).await;
    let latest = fixture.open_another(&kestrel).await;
    let other = Fixture::acme().organization("globex").open(&kestrel).await;
    for (reference, expected) in [
        (Some(first.name.as_str()), first.id),
        (Some("latest"), latest.id),
        (None, latest.id),
    ] {
        let mut args = vec!["open", "--organization", "acme"];
        args.extend(reference);
        let opened = ran_by(
            &kestrel,
            &args,
            Invocation::default().env("SSH_TTY", "pts/1"),
        )
        .await;
        assert_eq!(opened.status.code(), Some(0), "{}", opened.err);
        assert_eq!(
            opened.out,
            [format!(
                "http://localhost:7800/organizations/acme/workspaces/{expected}"
            )]
        );
        assert!(opened.err.contains("template"), "{}", opened.err);
        assert!(!opened.out[0].contains(&other.id.to_string()));
    }
    let missing = ran_by(
        &kestrel,
        &["open", "missing", "--organization", "acme", "--json"],
        Invocation::default(),
    )
    .await;
    assert_eq!(missing.status.code(), Some(3), "{}", missing.err);
    assert!(missing.out.is_empty());
    let diagnosis: Value = serde_json::from_str(missing.err.trim()).expect("typed diagnostic");
    assert_eq!(diagnosis["kind"], "missing_reference");
    let complete = ran_by(
        &kestrel,
        &["open", "--organization", "acme", "--jq", ".workspace.id"],
        Invocation::default(),
    )
    .await;
    assert_eq!(complete.out, [latest.id.to_string()]);
    kestrel.teardown().await;
}

#[tokio::test]
async fn status_and_open_share_sanitized_transport_diagnostics() {
    use crate::support::client;
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("an unused port");
    let url = format!("http://{}", listener.local_addr().expect("the port"));
    drop(listener);
    for command in ["status", "open"] {
        let failed = client::ran_as(&url, &[command, "--verbose"], Invocation::default());
        assert_eq!(failed.status.code(), Some(5), "{}", failed.err);
        assert!(failed.out.is_empty());
        assert!(failed.err.contains(&url), "{}", failed.err);
        assert!(failed.err.contains("running"), "{}", failed.err);
        assert!(!failed.err.contains("docker compose"), "{}", failed.err);
        assert!(!failed.err.contains("reqwest"), "{}", failed.err);
    }
}

#[tokio::test]
async fn status_counts_all_sessions_in_scope_and_failed_work_does_not_fail_the_read() {
    use crate::support::{
        fixture::Fixture,
        scripted_agent::{self, Script},
    };
    let kestrel = Kestrel::dispatching_to(
        crate::support::supervisor::binary(),
        &scripted_agent::playing(Script::Insists),
    )
    .await;
    kestrel.name_operator("acme").await;
    let fixture = Fixture::acme().checked_out().holding_a_provider_key();
    let workspace = fixture.open(&kestrel).await;
    let session = kestrel.enqueue_session(workspace.id).await;
    kestrel.after_one_turn(session.id).await;
    let second = kestrel.enqueue_session(workspace.id).await;
    kestrel.after_one_turn(second.id).await;
    let organization = kestrel.declare_organization("acme").await;
    kestrel
        .hold_provider_credential(
            &organization,
            "ANTHROPIC_API_KEY",
            "sk-ant-api03-status-test",
        )
        .await;
    Fixture::acme().organization("globex").open(&kestrel).await;
    let read = ran_by(
        &kestrel,
        &["status", "--organization", "acme", "--json"],
        Invocation::default(),
    )
    .await;
    assert_eq!(read.status.code(), Some(0), "{}", read.err);
    let status: Value = serde_json::from_str(&read.out[0]).expect("complete status");
    assert_eq!(status["readiness"]["outcome"], "ready");
    assert_eq!(status["workspace_counts"], json!({"open": 1, "sealed": 0}));
    assert_eq!(status["session_counts"]["ended"], 2);
    assert_eq!(status["sessions"].as_array().expect("sessions").len(), 2);
    assert_eq!(
        status["sessions"][0]["diagnostic"]["kind"],
        "authentication_failed"
    );
    let piped = ran_by(
        &kestrel,
        &["status", "--organization", "acme"],
        Invocation::default(),
    )
    .await;
    assert_eq!(piped.status.code(), Some(0), "{}", piped.err);
    assert!(piped.err.contains("sign in"), "{}", piped.err);
    assert!(piped.err.contains(session.name.as_str()), "{}", piped.err);
    kestrel.teardown().await;
}

#[tokio::test]
async fn status_counts_queued_sessions_and_sealed_workspaces_in_only_the_selected_scope() {
    use crate::support::fixture::Fixture;
    let kestrel = Kestrel::boot().await;
    let fixture = Fixture::acme();
    let open = fixture.open(&kestrel).await;
    kestrel.enqueue_session(open.id).await;
    let sealed = fixture.open_another(&kestrel).await;
    kestrel.seal_workspace(sealed.id).await;
    let other = Fixture::acme().organization("globex").open(&kestrel).await;
    kestrel.enqueue_session(other.id).await;
    let status = ran_by(
        &kestrel,
        &["status", "--json"],
        Invocation::default().env("KESTREL_ORGANIZATION", "acme"),
    )
    .await;
    let status: Value = serde_json::from_str(&status.out[0]).expect("a scoped status");
    assert_eq!(status["workspace_counts"], json!({"open": 1, "sealed": 1}));
    assert_eq!(status["session_counts"]["queued"], 1);
    assert_eq!(status["sessions"].as_array().expect("sessions").len(), 1);
    kestrel.teardown().await;
}

#[tokio::test]
async fn status_exits_unavailable_when_readiness_cannot_be_inspected() {
    use crate::support::fixture::Fixture;
    let kestrel = Kestrel::boot_inspecting("kestrel-env:no-status-image").await;
    kestrel.name_operator("acme").await;
    let organization = Fixture::acme().declare(&kestrel).await;
    kestrel
        .hold_provider_credential(
            &organization,
            "ANTHROPIC_API_KEY",
            "sk-ant-api03-status-test",
        )
        .await;
    let read = ran_by(&kestrel, &["status", "--json"], Invocation::default()).await;
    assert_eq!(read.status.code(), Some(5), "{}", read.err);
    let status: Value = serde_json::from_str(&read.out[0]).expect("complete status");
    assert_eq!(status["readiness"]["outcome"], "unavailable");
    let diagnostic: Value = serde_json::from_str(read.err.trim()).expect("typed diagnostic");
    assert_eq!(diagnostic["kind"], "unavailable");
    kestrel.teardown().await;
}

#[tokio::test]
async fn an_ambiguous_explicit_workspace_stays_a_typed_error() {
    use crate::support::fixture::Fixture;
    let kestrel = Kestrel::boot().await;
    let fixture = Fixture::acme();
    fixture.declare(&kestrel).await;
    let mut prefixes = std::collections::HashSet::new();
    let mut repeated = None;
    for _ in 0..17 {
        let workspace = fixture.open_another(&kestrel).await;
        let prefix = workspace.id.to_string()[..1].to_owned();
        if !prefixes.insert(prefix.clone()) {
            repeated = Some(prefix);
            break;
        }
    }
    let prefix = repeated.expect("17 identifiers have a repeated first hex digit");
    let opened = ran_by(
        &kestrel,
        &["open", &prefix, "--json"],
        Invocation::default(),
    )
    .await;
    assert_eq!(opened.status.code(), Some(3), "{}", opened.err);
    assert!(opened.out.is_empty());
    let diagnostic: Value = serde_json::from_str(opened.err.trim()).expect("typed diagnostic");
    assert_eq!(diagnostic["kind"], "ambiguous_reference");
    kestrel.teardown().await;
}

#[tokio::test]
async fn open_launches_a_local_browser_with_the_url_it_printed() {
    use std::os::unix::fs::PermissionsExt as _;
    let kestrel = Kestrel::boot_with_the_client_at("http://localhost:7800").await;
    let directory = tempfile::tempdir().expect("browser capture directory");
    let script = directory.path().join("browser");
    let captured = directory.path().join("url");
    std::fs::write(
        &script,
        format!("#!/bin/sh\nprintf '%s' \"$1\" > '{}'\n", captured.display()),
    )
    .expect("a browser adapter");
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))
        .expect("an executable adapter");
    let opened = ran_on_a_terminal_by(
        &kestrel,
        &["open"],
        Invocation::default()
            .env("BROWSER", script.to_str().expect("a browser path"))
            .env("DISPLAY", ":1"),
        "",
    )
    .await;
    assert_eq!(opened.status.code(), Some(0), "{}", opened.said);
    let url = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if let Ok(url) = std::fs::read_to_string(&captured) {
                if !url.is_empty() {
                    break url;
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("the browser receives the URL");
    assert_eq!(url, "http://localhost:7800/");
    assert!(opened.said.contains(&url));
    kestrel.teardown().await;
}
