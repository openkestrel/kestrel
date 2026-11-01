//! `kestrel integration github connect`: the manifest flow started, shown and waited on from the
//! CLI, with GitHub's side driven over HTTP against a scripted GitHub.

use reqwest::StatusCode;
use serde_json::{Value, json};

use crate::support;

use support::client::{Client, Invocation};
use support::github_stub::{GithubStub, ScriptedResponse};
use support::{Kestrel, PRIVATE_KEY};

const CLIENT: &str = "http://localhost:7719";

async fn booted(stub: &GithubStub) -> Kestrel {
    let kestrel = Kestrel::boot_serving_at_github(&stub.base_url()).await;
    kestrel.declare_organization("acme").await;
    kestrel
}

fn connecting(kestrel: &Kestrel, args: &[&str], invocation: Invocation) -> Client {
    let args = [&["integration", "github", "connect"], args].concat();
    Client::spawn_as(&kestrel.operator(), &args, invocation)
}

/// Off the runtime, which serves the control plane the Client is talking to.
async fn heard(client: Client, text: &'static str) -> Client {
    tokio::task::spawn_blocking(move || {
        client.until_err_says(text);
        client
    })
    .await
    .unwrap()
}

/// The flow reference is the `state` of the setup URL the CLI printed on its own line.
async fn flow_from(client: Client) -> (Client, String) {
    let client = heard(client, "Ctrl-C").await;
    let said = client.err_so_far();
    let url = said
        .lines()
        .map(str::trim)
        .find(|line| line.starts_with(CLIENT) && line.contains("state="))
        .unwrap_or_else(|| panic!("no setup URL on a line of its own:\n{said}"));
    let flow = url.split("state=").nth(1).expect("a state").to_owned();
    (client, flow)
}

async fn installed(kestrel: &Kestrel, stub: &GithubStub, flow: &str) {
    stub.script_answer(
        "POST",
        "/app-manifests/code123/conversions",
        ScriptedResponse::ok(
            json!({"id": 17, "pem": PRIVATE_KEY, "webhook_secret": "manifest-webhook-secret", "slug": "kestrel-example"})
                .to_string(),
        ),
    );
    stub.script_answer(
        "GET",
        "/app/installations/23",
        ScriptedResponse::ok(r#"{"id":23,"app_id":17,"account":{"login":"acme"}}"#),
    );
    stub.script_answer(
        "GET",
        "/repos/acme/repo/installation",
        ScriptedResponse::ok(r#"{"id":23,"app_id":17}"#),
    );
    for _ in 0..4 {
        stub.script_answer(
            "GET",
            "/repos/acme/repo",
            ScriptedResponse::ok(
                r#"{"id":1,"full_name":"acme/repo","default_branch":"main","private":true}"#,
            ),
        );
    }
    let browser = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let operator = kestrel.operator();
    let called = browser
        .get(format!(
            "{operator}/operator/github-app/callback?state={flow}&code=code123"
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(called.status(), StatusCode::OK);
    let returned = browser
        .get(format!(
            "{operator}/operator/github-app/installed?state={flow}&installation_id=23"
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(returned.status(), StatusCode::SEE_OTHER);
}

async fn flow_read(kestrel: &Kestrel, flow: &str) -> Value {
    reqwest::get(format!(
        "{}/operator/github-app/flows/{flow}",
        kestrel.operator()
    ))
    .await
    .unwrap()
    .json()
    .await
    .unwrap()
}

async fn waited(client: Client) -> support::client::Finished {
    let mut client = client;
    tokio::task::spawn_blocking(move || client.finish())
        .await
        .unwrap()
}

fn kept_secret(said: &str) {
    for secret in [PRIVATE_KEY, "manifest-webhook-secret", "BEGIN"] {
        assert!(!said.contains(secret), "{said}");
    }
}

#[tokio::test]
async fn over_ssh_it_prints_the_tunnel_to_the_server_and_finishes_with_the_given_repository() {
    let stub = GithubStub::start();
    let kestrel = booted(&stub).await;

    let client = connecting(
        &kestrel,
        &["--repository", "acme/repo"],
        Invocation::default()
            .env("SSH_CONNECTION", "198.51.100.7 51000 203.0.113.10 2222")
            .env("USER", "jack"),
    );
    let (client, flow) = flow_from(client).await;
    let said = client.err_so_far();
    assert!(
        said.lines()
            .any(|line| line.trim() == "ssh -N -L 7719:localhost:7719 -p 2222 jack@203.0.113.10"),
        "{said}"
    );
    assert!(!said.contains("<ssh-destination>"), "{said}");
    assert_eq!(flow_read(&kestrel, &flow).await["phase"], "ready");

    installed(&kestrel, &stub, &flow).await;
    let finished = waited(client).await;

    assert!(finished.status.success(), "{}", finished.err);
    assert_eq!(
        finished.out,
        ["GitHub connected · Integration github · Organization acme · repository acme/repo"],
        "{}",
        finished.err
    );
    kept_secret(&finished.err);
    kept_secret(&finished.out.join("\n"));
    assert_eq!(flow_read(&kestrel, &flow).await["phase"], "completed");
    kestrel.teardown().await;
}

#[tokio::test]
async fn with_no_known_destination_the_tunnel_is_a_template_and_the_browser_chooses() {
    let stub = GithubStub::start();
    let kestrel = booted(&stub).await;

    let client = connecting(
        &kestrel,
        &["origin", "--json"],
        Invocation::default().env("SSH_TTY", "/dev/pts/3"),
    );
    let (client, flow) = flow_from(client).await;
    let said = client.err_so_far();
    assert!(
        said.lines()
            .any(|line| line.trim() == "ssh -N -L 7719:localhost:7719 <ssh-destination>"),
        "{said}"
    );
    assert!(said.contains("template"), "{said}");

    installed(&kestrel, &stub, &flow).await;
    let client = heard(client, "Choose its repository in the browser").await;
    let chosen = reqwest::Client::new()
        .post(format!(
            "{}/operator/github-app/flows/{flow}/finish",
            kestrel.operator()
        ))
        .json(&json!({"repository": "acme/repo"}))
        .send()
        .await
        .unwrap();
    assert_eq!(chosen.status(), StatusCode::OK);
    let finished = waited(client).await;

    let connected = finished.json();
    assert_eq!(connected["phase"], "completed");
    assert_eq!(connected["name"], "origin");
    assert_eq!(connected["repository"], "acme/repo");
    assert_eq!(connected["organization"], "acme");
    kept_secret(&finished.out.join("\n"));
    kestrel.teardown().await;
}

#[tokio::test]
async fn interrupted_waiting_keeps_the_setup_and_resuming_finishes_it() {
    let stub = GithubStub::start();
    let kestrel = booted(&stub).await;

    let (mut client, flow) = flow_from(connecting(&kestrel, &[], Invocation::default())).await;
    let said = client.err_so_far();
    assert!(
        said.contains(&format!(
            "kestrel integration github connect --resume {flow}"
        )),
        "{said}"
    );
    client.interrupt();
    let interrupted = tokio::task::spawn_blocking(move || client.finish())
        .await
        .unwrap();
    assert!(!interrupted.status.success());
    assert_eq!(flow_read(&kestrel, &flow).await["phase"], "ready");

    installed(&kestrel, &stub, &flow).await;
    let resumed = connecting(
        &kestrel,
        &["--resume", &flow, "--repository", "acme/repo"],
        Invocation::default(),
    );
    let finished = waited(resumed).await;

    assert!(finished.status.success(), "{}", finished.err);
    assert_eq!(
        finished.out,
        ["GitHub connected · Integration github · Organization acme · repository acme/repo"]
    );
    kestrel.teardown().await;
}

#[tokio::test]
async fn a_cancelled_setup_is_reported_as_cancelled_and_saves_nothing() {
    let stub = GithubStub::start();
    let kestrel = booted(&stub).await;

    let client = connecting(&kestrel, &[], Invocation::default());
    let (client, flow) = flow_from(client).await;
    let cancelled = reqwest::Client::new()
        .delete(format!(
            "{}/operator/github-app/flows/{flow}",
            kestrel.operator()
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(cancelled.status(), StatusCode::OK);
    let finished = waited(client).await;

    assert_eq!(finished.status.code(), Some(4), "{}", finished.err);
    assert!(finished.out.is_empty(), "{:?}", finished.out);
    assert!(finished.err.contains("cancelled"), "{}", finished.err);
    assert!(kestrel.integrations("acme").await.is_empty());
    kestrel.teardown().await;
}

#[tokio::test]
async fn an_expired_setup_says_so_and_offers_a_new_one() {
    let stub = GithubStub::start();
    let kestrel = booted(&stub).await;

    let client = connecting(&kestrel, &[], Invocation::default());
    let (client, _) = flow_from(client).await;
    kestrel.away_for(std::time::Duration::from_secs(60 * 60 + 1));
    let finished = waited(client).await;

    assert_eq!(finished.status.code(), Some(4), "{}", finished.err);
    assert!(finished.err.contains("expired"), "{}", finished.err);
    assert!(
        finished.err.contains("kestrel integration github connect"),
        "{}",
        finished.err
    );
    kestrel.teardown().await;
}
