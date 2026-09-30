//! A signed `pull_request` delivery becomes shared Workspace state when its head repository and
//! branch name exactly one open Workspace, without starting any work.

mod support;

use std::time::Duration;

use hmac::{Hmac, KeyInit as _, Mac as _};
use kestrel::domain::{Integration, Workspace, WorkspaceId};
use kestrel::log::Entry;
use reqwest::StatusCode;
use serde_json::{Value, json};
use sha2::Sha256;
use support::github_stub::GithubStub;
use support::{Kestrel, client, templates};

const PATIENCE: Duration = Duration::from_secs(30);
const SECRET: &str = "a-signing-secret";
const BASE: &str = "jtmthf/kestrel";
const TOOLS: &str = "jtmthf/tools";
const FORK: &str = "someone/kestrel";

fn url(repository: &str) -> String {
    format!("https://github.com/{repository}")
}

fn signature(body: &[u8]) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(SECRET.as_bytes()).expect("an HMAC key");
    mac.update(body);

    format!(
        "sha256={}",
        mac.finalize()
            .into_bytes()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    )
}

struct Opened<'a> {
    base: &'a str,
    head: &'a str,
    branch: &'a str,
    number: i64,
}

impl Opened<'_> {
    fn payload(&self) -> Value {
        json!({
            "action": "opened",
            "number": self.number,
            "pull_request": {
                "number": self.number,
                "html_url": format!("{}/pull/{}", url(self.base), self.number),
                "title": format!("Pull request {}", self.number),
                "state": "open",
                "merged": false,
                "updated_at": "2026-09-30T12:00:00Z",
                "head": {
                    "ref": self.branch,
                    "sha": "0123456789abcdef0123456789abcdef01234567",
                    "repo": { "full_name": self.head, "html_url": url(self.head) }
                },
                "base": {
                    "ref": "main",
                    "repo": { "full_name": self.base, "html_url": url(self.base) }
                }
            },
            "repository": { "full_name": self.base },
            "sender": { "login": "jtmthf" }
        })
    }
}

async fn deliver(
    kestrel: &Kestrel,
    integration: &Integration,
    delivery: &str,
    opened: &Opened<'_>,
) {
    let body = opened.payload().to_string().into_bytes();
    let answered = reqwest::Client::new()
        .post(format!("{}{}", kestrel.link(), integration.webhook_path()))
        .header("content-type", "application/json")
        .header("x-github-event", "pull_request")
        .header("x-github-delivery", delivery)
        .header("x-hub-signature-256", signature(&body))
        .body(body)
        .send()
        .await
        .expect("the webhook answers");

    assert_eq!(answered.status(), StatusCode::ACCEPTED);
}

async fn pull_request_entries(kestrel: &Kestrel, workspace: WorkspaceId) -> Vec<Entry> {
    kestrel
        .transcript(workspace)
        .await
        .into_iter()
        .map(|entry| entry.entry)
        .filter(|entry| matches!(entry, Entry::PullRequest { .. }))
        .collect()
}

async fn attached(kestrel: &Kestrel, workspace: WorkspaceId, count: usize) -> Vec<Entry> {
    let deadline = tokio::time::Instant::now() + PATIENCE;
    loop {
        let entries = pull_request_entries(kestrel, workspace).await;
        if entries.len() >= count {
            return entries;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the workspace {workspace} learned {} pull requests, not {count}",
            entries.len()
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

async fn shown(kestrel: &Kestrel, organization: &str, workspace: &Workspace) -> Value {
    let answered = reqwest::Client::new()
        .get(format!(
            "{}/operator/organizations/{organization}/workspaces/{}",
            kestrel.operator(),
            workspace.id
        ))
        .send()
        .await
        .expect("the operator boundary answers");
    assert_eq!(answered.status(), StatusCode::OK);

    answered.json().await.expect("a workspace")
}

async fn watching(kestrel: &Kestrel, organization: &str, repository: &str) -> Integration {
    let stub = GithubStub::start();
    let name = repository.replace('/', "-");
    kestrel
        .register_signed_github(organization, &name, repository, &stub.base_url(), SECRET)
        .await
}

async fn declared(kestrel: &Kestrel, organization: &str, repositories: &[&str]) {
    let organization = kestrel.declare_organization(organization).await;
    kestrel
        .declare_project(
            &organization,
            "kestrel",
            &repositories
                .iter()
                .map(|repository| url(repository))
                .collect::<Vec<_>>(),
            "main",
        )
        .await;
    kestrel
        .declare_agent(&organization, "builder", "opencode", None)
        .await;
}

#[tokio::test]
async fn an_opened_pull_request_on_the_declared_branch_becomes_workspace_state_once() {
    let kestrel = Kestrel::boot().await;
    declared(&kestrel, "acme", &[BASE]).await;
    let github = watching(&kestrel, "acme", BASE).await;
    let workspace = kestrel
        .open_workspace_on("acme", "kestrel", "builder", "feature")
        .await;
    let opened = Opened {
        base: BASE,
        head: BASE,
        branch: "feature",
        number: 7,
    };

    deliver(&kestrel, &github, "d-1", &opened).await;
    deliver(&kestrel, &github, "d-1", &opened).await;
    let entries = attached(&kestrel, workspace.id, 1).await;

    let events = kestrel.events("acme").await;
    assert_eq!(events.len(), 1, "one delivery is one event: {events:?}");
    let event = &events[0];
    let Entry::PullRequest {
        event: source,
        repository,
        number,
        url: pull_request_url,
        action,
        state,
        ..
    } = &entries[0]
    else {
        unreachable!()
    };
    assert_eq!(*source, event.record_id);
    assert_eq!(repository, &url(BASE));
    assert_eq!(*number, 7);
    assert_eq!(pull_request_url, &format!("{}/pull/7", url(BASE)));
    assert_eq!(action, "opened");
    assert_eq!(state.as_str(), "open");

    let record = shown(&kestrel, "acme", &workspace).await;
    assert_eq!(
        record["pull_requests"],
        json!([{
            "repository": url(BASE),
            "availability": "available",
            "known": [{
                "repository": url(BASE),
                "number": 7,
                "url": format!("{}/pull/7", url(BASE)),
                "title": "Pull request 7",
                "state": "open",
                "head_branch": "feature",
                "head_revision": "0123456789abcdef0123456789abcdef01234567",
                "updated_at": "2026-09-30T12:00:00Z",
                "event": event.record_id.to_string(),
            }]
        }])
    );
    let cli = client::ran_by(
        &kestrel,
        &[
            "--organization",
            "acme",
            "workspace",
            "show",
            &workspace.name,
            "--json",
            "pull_requests",
        ],
        client::Invocation::default(),
    )
    .await;
    assert!(cli.status.success(), "the client failed: {}", cli.err);
    let printed: Value = serde_json::from_str(&cli.out.join("\n")).expect("json");
    assert_eq!(printed["pull_requests"], record["pull_requests"]);

    tokio::time::sleep(Duration::from_millis(600)).await;
    assert_eq!(pull_request_entries(&kestrel, workspace.id).await.len(), 1);
    assert!(kestrel.firings(event.record_id).await.is_empty());
    assert!(kestrel.sessions(workspace.id).await.is_empty());
    assert_eq!(kestrel.workspaces("acme").await.len(), 1);

    kestrel.teardown().await;
}

#[tokio::test]
async fn only_the_head_repository_and_branch_of_one_open_workspace_attach() {
    let kestrel = Kestrel::boot().await;
    declared(&kestrel, "acme", &[BASE, TOOLS]).await;
    declared(&kestrel, "beta", &[BASE]).await;
    let github = watching(&kestrel, "acme", BASE).await;
    let tooling = watching(&kestrel, "acme", TOOLS).await;
    watching(&kestrel, "beta", BASE).await;
    let tools = kestrel
        .open_workspace_on("acme", "kestrel", "builder", "tooling")
        .await;
    let first = kestrel
        .open_workspace_on("acme", "kestrel", "builder", "shared")
        .await;
    let second = kestrel
        .open_workspace_on("acme", "kestrel", "builder", "shared")
        .await;
    let forked = kestrel
        .open_workspace_on("acme", "kestrel", "builder", "from-a-fork")
        .await;
    let elsewhere = kestrel
        .open_workspace_on("beta", "kestrel", "builder", "feature")
        .await;
    let learning = kestrel
        .open_workspace_on("acme", "kestrel", "builder", "feature")
        .await;

    // The branch names the fork's own branch, and the Workspace is fixed on the base repository.
    deliver(
        &kestrel,
        &github,
        "d-fork",
        &Opened {
            base: BASE,
            head: FORK,
            branch: "from-a-fork",
            number: 1,
        },
    )
    .await;
    // Two open Workspaces declare it, so neither is the one it describes.
    deliver(
        &kestrel,
        &github,
        "d-ambiguous",
        &Opened {
            base: BASE,
            head: BASE,
            branch: "shared",
            number: 2,
        },
    )
    .await;
    // Every repository of a multi-repository Project is searched, and only the head's counts.
    deliver(
        &kestrel,
        &tooling,
        "d-tools",
        &Opened {
            base: TOOLS,
            head: TOOLS,
            branch: "tooling",
            number: 3,
        },
    )
    .await;
    deliver(
        &kestrel,
        &github,
        "d-wrong-repository",
        &Opened {
            base: BASE,
            head: "jtmthf/elsewhere",
            branch: "feature",
            number: 4,
        },
    )
    .await;
    deliver(
        &kestrel,
        &github,
        "d-learned",
        &Opened {
            base: BASE,
            head: BASE,
            branch: "feature",
            number: 5,
        },
    )
    .await;
    let learned = attached(&kestrel, learning.id, 1).await;

    assert!(
        matches!(&learned[..], [Entry::PullRequest { number: 5, .. }]),
        "{learned:?}"
    );
    let learned_by_tools = attached(&kestrel, tools.id, 1).await;
    assert!(
        matches!(&learned_by_tools[..], [Entry::PullRequest { number: 3, repository, .. }] if *repository == url(TOOLS)),
        "{learned_by_tools:?}"
    );
    for unattached in [&first, &second, &forked, &elsewhere] {
        assert!(
            pull_request_entries(&kestrel, unattached.id)
                .await
                .is_empty(),
            "the workspace on {} learned a pull request that is not its own",
            unattached.checkout.branch
        );
    }
    assert_eq!(kestrel.events("acme").await.len(), 5);
    assert!(kestrel.events("beta").await.is_empty());

    let record = shown(&kestrel, "acme", &tools).await;
    assert_eq!(record["pull_requests"][0]["repository"], url(BASE));
    assert_eq!(record["pull_requests"][0]["availability"], "available");
    assert_eq!(record["pull_requests"][0]["known"], json!([]));
    assert_eq!(record["pull_requests"][1]["repository"], url(TOOLS));
    assert_eq!(record["pull_requests"][1]["known"][0]["number"], 3);

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_repository_no_inbound_integration_watches_says_its_pull_requests_are_unavailable() {
    let kestrel = Kestrel::boot().await;
    declared(&kestrel, "acme", &[BASE, TOOLS]).await;
    watching(&kestrel, "acme", BASE).await;
    let workspace = kestrel
        .open_workspace_on("acme", "kestrel", "builder", "feature")
        .await;

    let record = shown(&kestrel, "acme", &workspace).await;

    assert_eq!(
        record["pull_requests"],
        json!([
            { "repository": url(BASE), "availability": "available", "known": [] },
            { "repository": url(TOOLS), "availability": "unavailable", "known": null },
        ])
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_fork_pull_request_delivered_through_the_watched_base_is_available_on_the_fork() {
    let kestrel = Kestrel::boot().await;
    declared(&kestrel, "acme", &[FORK]).await;
    let github = watching(&kestrel, "acme", BASE).await;
    let workspace = kestrel
        .open_workspace_on("acme", "kestrel", "builder", "feature")
        .await;

    deliver(
        &kestrel,
        &github,
        "d-upstream",
        &Opened {
            base: BASE,
            head: FORK,
            branch: "feature",
            number: 9,
        },
    )
    .await;
    attached(&kestrel, workspace.id, 1).await;

    let record = shown(&kestrel, "acme", &workspace).await;
    assert_eq!(record["pull_requests"][0]["availability"], "available");
    assert_eq!(
        record["pull_requests"][0]["known"][0]["url"],
        format!("{}/pull/9", url(BASE))
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn pull_requests_sharing_a_head_and_number_against_different_bases_are_both_kept() {
    let kestrel = Kestrel::boot().await;
    declared(&kestrel, "acme", &[FORK]).await;
    let upstream = watching(&kestrel, "acme", BASE).await;
    let fork = watching(&kestrel, "acme", FORK).await;
    let workspace = kestrel
        .open_workspace_on("acme", "kestrel", "builder", "feature")
        .await;

    for (integration, delivery, base) in [(&upstream, "d-upstream", BASE), (&fork, "d-fork", FORK)]
    {
        deliver(
            &kestrel,
            integration,
            delivery,
            &Opened {
                base,
                head: FORK,
                branch: "feature",
                number: 9,
            },
        )
        .await;
    }
    attached(&kestrel, workspace.id, 2).await;

    let record = shown(&kestrel, "acme", &workspace).await;
    let mut known: Vec<_> = record["pull_requests"][0]["known"]
        .as_array()
        .expect("available")
        .iter()
        .map(|pull_request| pull_request["url"].as_str().expect("a url").to_owned())
        .collect();
    known.sort();
    assert_eq!(
        known,
        [
            format!("{}/pull/9", url(BASE)),
            format!("{}/pull/9", url(FORK))
        ]
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_trigger_declared_for_the_event_fires_as_it_would_without_the_workspace_learning_it() {
    let kestrel = Kestrel::boot().await;
    declared(&kestrel, "acme", &[BASE]).await;
    let github = watching(&kestrel, "acme", BASE).await;
    kestrel
        .declare_trigger_rendering(
            "acme",
            "review",
            &json!({"exact": {"type": "com.github.pull_request.opened"}}).to_string(),
            "kestrel",
            "builder",
            &templates("Review {{ event.data.pull_request.title }}", None, None),
        )
        .await;
    let workspace = kestrel
        .open_workspace_on("acme", "kestrel", "builder", "feature")
        .await;

    deliver(
        &kestrel,
        &github,
        "d-1",
        &Opened {
            base: BASE,
            head: BASE,
            branch: "feature",
            number: 7,
        },
    )
    .await;
    attached(&kestrel, workspace.id, 1).await;
    let event = kestrel.events("acme").await.remove(0);
    let deadline = tokio::time::Instant::now() + PATIENCE;
    while kestrel.firings(event.record_id).await.is_empty() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "the trigger never fired"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    tokio::time::sleep(Duration::from_millis(600)).await;
    assert_eq!(kestrel.firings(event.record_id).await.len(), 1);
    assert!(kestrel.sessions(workspace.id).await.is_empty());

    kestrel.teardown().await;
}
