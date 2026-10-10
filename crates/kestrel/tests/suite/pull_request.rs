use crate::support;

use std::time::Duration;

use hmac::{Hmac, KeyInit as _, Mac as _};
use kestrel::domain::{Event, EventRecordId, Integration, Workspace, WorkspaceId};
use kestrel::log::Entry;
use reqwest::StatusCode;
use serde_json::{Value, json};
use sha2::Sha256;
use support::Consideration;
use support::fixture::Fixture;
use support::github_stub::{self, GithubStub, ScriptedResponse};
use support::{Kestrel, client, templates};

const PATIENCE: Duration = Duration::from_secs(30);
const SECRET: &str = "a-signing-secret";
const BASE: &str = "jtmthf/kestrel";
const TOOLS: &str = "jtmthf/tools";
const FORK: &str = "someone/kestrel";
const REVISION: &str = "0123456789abcdef0123456789abcdef01234567";
const MOVED: &str = "89abcdef0123456789abcdef0123456789abcdef";
const OPENED_AT: &str = "2026-09-30T12:00:00Z";
const MOVED_AT: &str = "2026-09-30T12:01:00Z";
const CLOSED_AT: &str = "2026-09-30T12:02:00Z";
const REOPENED_AT: &str = "2026-09-30T12:03:00Z";

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

#[derive(Clone)]
struct Delivery<'a> {
    action: &'a str,
    state: &'a str,
    merged: bool,
    base: &'a str,
    head: &'a str,
    branch: &'a str,
    number: i64,
    revision: &'a str,
    updated_at: &'a str,
}

impl<'a> Delivery<'a> {
    fn new(
        action: &'a str,
        state: &'a str,
        merged: bool,
        number: i64,
        revision: &'a str,
        updated_at: &'a str,
    ) -> Self {
        Self {
            action,
            state,
            merged,
            base: BASE,
            head: BASE,
            branch: "feature",
            number,
            revision,
            updated_at,
        }
    }

    fn logged(&self) -> github_stub::Delivery {
        github_stub::Delivery {
            event: "pull_request".to_owned(),
            payload: self.payload(),
        }
    }

    fn payload(&self) -> Value {
        json!({
            "action": self.action,
            "number": self.number,
            "pull_request": {
                "number": self.number,
                "html_url": format!("{}/pull/{}", url(self.base), self.number),
                "title": format!("Pull request {}", self.number),
                "state": self.state,
                "merged": self.merged,
                "updated_at": self.updated_at,
                "head": {
                    "ref": self.branch,
                    "sha": self.revision,
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

fn opened<'a>(number: i64, revision: &'a str, updated_at: &'a str) -> Delivery<'a> {
    Delivery::new("opened", "open", false, number, revision, updated_at)
}

fn synchronize<'a>(number: i64, revision: &'a str, updated_at: &'a str) -> Delivery<'a> {
    Delivery::new("synchronize", "open", false, number, revision, updated_at)
}

fn closed<'a>(number: i64, revision: &'a str, updated_at: &'a str) -> Delivery<'a> {
    Delivery::new("closed", "closed", false, number, revision, updated_at)
}

fn merged<'a>(number: i64, revision: &'a str, updated_at: &'a str) -> Delivery<'a> {
    Delivery::new("closed", "closed", true, number, revision, updated_at)
}

fn reopened<'a>(number: i64, revision: &'a str, updated_at: &'a str) -> Delivery<'a> {
    Delivery::new("reopened", "open", false, number, revision, updated_at)
}

fn edited<'a>(number: i64, revision: &'a str, updated_at: &'a str) -> Delivery<'a> {
    Delivery::new("edited", "open", false, number, revision, updated_at)
}

async fn deliver(
    kestrel: &Kestrel,
    integration: &Integration,
    delivery: &str,
    sent: &Delivery<'_>,
) {
    let body = sent.payload().to_string().into_bytes();
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

fn event_named<'a>(events: &'a [Event], delivery: &str) -> &'a Event {
    events
        .iter()
        .find(|event| event.occurrence.id == delivery)
        .unwrap_or_else(|| panic!("no event for the delivery {delivery}: {events:?}"))
}

async fn consideration(kestrel: &Kestrel, event: EventRecordId) -> Consideration {
    let deadline = tokio::time::Instant::now() + PATIENCE;
    loop {
        if let Some(considered) = kestrel.consideration(event).await {
            return considered;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the event {event} was never considered"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
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

fn observations(entries: &[Entry]) -> Vec<(&str, &str)> {
    entries
        .iter()
        .map(|entry| match entry {
            Entry::PullRequest { action, state, .. } => (action.as_str(), state.as_str()),
            other => panic!("{other:?} is not a pull request entry"),
        })
        .collect()
}

fn known(record: &Value) -> &Vec<Value> {
    record["pull_requests"][0]["known"]
        .as_array()
        .expect("the repository is available with current values")
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

async fn watching(
    kestrel: &Kestrel,
    organization: &str,
    repository: &str,
) -> (Integration, GithubStub) {
    watching_as(
        kestrel,
        organization,
        repository,
        &repository.replace('/', "-"),
    )
    .await
}

async fn watching_as(
    kestrel: &Kestrel,
    organization: &str,
    repository: &str,
    name: &str,
) -> (Integration, GithubStub) {
    let stub = GithubStub::start();
    let integration = kestrel
        .register_signed_github(organization, name, repository, &stub.base_url(), SECRET)
        .await;

    (integration, stub)
}

async fn declared(kestrel: &Kestrel, organization: &str, repositories: &[&str]) {
    Fixture::acme()
        .organization(organization)
        .repositories(
            &repositories
                .iter()
                .map(|repository| url(repository))
                .collect::<Vec<_>>(),
        )
        .declare(kestrel)
        .await;
}

#[tokio::test]
async fn an_opened_pull_request_on_the_declared_branch_becomes_workspace_state_once() {
    let kestrel = Kestrel::boot().await;
    declared(&kestrel, "acme", &[BASE]).await;
    let (github, _stub) = watching(&kestrel, "acme", BASE).await;
    let workspace = kestrel
        .open_workspace_on("acme", "kestrel", "builder", "feature")
        .await;

    deliver(&kestrel, &github, "d-1", &opened(7, REVISION, OPENED_AT)).await;
    deliver(&kestrel, &github, "d-1", &opened(7, REVISION, OPENED_AT)).await;
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
    let considered = consideration(&kestrel, event.record_id).await;
    assert_eq!(considered.outcome, "attached");
    assert_eq!(considered.candidates, [(workspace.id, "open".to_owned())]);

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
                "head_revision": REVISION,
                "updated_at": OPENED_AT,
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
    let (github, _github_stub) = watching(&kestrel, "acme", BASE).await;
    let (tooling, _tooling_stub) = watching(&kestrel, "acme", TOOLS).await;
    let (_beta, _beta_stub) = watching(&kestrel, "beta", BASE).await;
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
        &Delivery {
            head: FORK,
            branch: "from-a-fork",
            number: 1,
            ..opened(1, REVISION, OPENED_AT)
        },
    )
    .await;
    // Two open Workspaces declare it, so neither is the one it describes.
    deliver(
        &kestrel,
        &github,
        "d-ambiguous",
        &Delivery {
            branch: "shared",
            ..opened(2, REVISION, MOVED_AT)
        },
    )
    .await;
    // Every repository of a multi-repository Project is searched, and only the head's counts.
    deliver(
        &kestrel,
        &tooling,
        "d-tools",
        &Delivery {
            base: TOOLS,
            head: TOOLS,
            branch: "tooling",
            ..opened(3, REVISION, CLOSED_AT)
        },
    )
    .await;
    deliver(
        &kestrel,
        &github,
        "d-wrong-repository",
        &Delivery {
            head: "jtmthf/elsewhere",
            ..opened(4, REVISION, REOPENED_AT)
        },
    )
    .await;
    deliver(
        &kestrel,
        &github,
        "d-learned",
        &opened(5, REVISION, MOVED_AT),
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
    let events = kestrel.events("acme").await;
    assert_eq!(events.len(), 5);
    assert!(kestrel.events("beta").await.is_empty());

    let ambiguous = consideration(&kestrel, event_named(&events, "d-ambiguous").record_id).await;
    assert_eq!(ambiguous.outcome, "ambiguous");
    let mut candidates = ambiguous.candidates;
    candidates.sort_by_key(|(workspace, _)| workspace.to_string());
    assert_eq!(
        candidates,
        [
            (first.id, "open".to_owned()),
            (second.id, "open".to_owned())
        ]
    );
    let a_fork = consideration(&kestrel, event_named(&events, "d-fork").record_id).await;
    assert_eq!(a_fork.outcome, "unmatched");
    assert!(a_fork.candidates.is_empty());
    let wrong = consideration(
        &kestrel,
        event_named(&events, "d-wrong-repository").record_id,
    )
    .await;
    assert_eq!(wrong.outcome, "unmatched");
    assert!(wrong.candidates.is_empty());

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
    let (_github, _stub) = watching(&kestrel, "acme", BASE).await;
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
    let (github, _stub) = watching(&kestrel, "acme", BASE).await;
    let workspace = kestrel
        .open_workspace_on("acme", "kestrel", "builder", "feature")
        .await;

    deliver(
        &kestrel,
        &github,
        "d-upstream",
        &Delivery {
            head: FORK,
            number: 9,
            ..opened(9, REVISION, OPENED_AT)
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
    let (upstream, _upstream_stub) = watching(&kestrel, "acme", BASE).await;
    let (fork, _fork_stub) = watching(&kestrel, "acme", FORK).await;
    let workspace = kestrel
        .open_workspace_on("acme", "kestrel", "builder", "feature")
        .await;

    for (integration, delivery, base) in [(&upstream, "d-upstream", BASE), (&fork, "d-fork", FORK)]
    {
        deliver(
            &kestrel,
            integration,
            delivery,
            &Delivery {
                base,
                head: FORK,
                number: 9,
                ..opened(9, REVISION, OPENED_AT)
            },
        )
        .await;
    }
    attached(&kestrel, workspace.id, 2).await;

    let record = shown(&kestrel, "acme", &workspace).await;
    let mut known: Vec<_> = known(&record)
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
async fn a_pull_requests_lifecycle_appends_each_observation_and_reads_the_latest_value() {
    let kestrel = Kestrel::boot().await;
    declared(&kestrel, "acme", &[BASE]).await;
    let (github, _stub) = watching(&kestrel, "acme", BASE).await;
    let workspace = kestrel
        .open_workspace_on("acme", "kestrel", "builder", "feature")
        .await;

    deliver(
        &kestrel,
        &github,
        "d-opened",
        &opened(7, REVISION, OPENED_AT),
    )
    .await;
    attached(&kestrel, workspace.id, 1).await;
    deliver(
        &kestrel,
        &github,
        "d-moved",
        &synchronize(7, MOVED, MOVED_AT),
    )
    .await;
    attached(&kestrel, workspace.id, 2).await;
    assert_eq!(
        known(&shown(&kestrel, "acme", &workspace).await)[0]["head_revision"],
        MOVED
    );
    deliver(&kestrel, &github, "d-closed", &merged(7, MOVED, CLOSED_AT)).await;
    attached(&kestrel, workspace.id, 3).await;
    assert_eq!(
        known(&shown(&kestrel, "acme", &workspace).await)[0]["state"],
        "merged"
    );
    deliver(
        &kestrel,
        &github,
        "d-reopened",
        &reopened(7, MOVED, REOPENED_AT),
    )
    .await;
    let entries = attached(&kestrel, workspace.id, 4).await;

    assert_eq!(
        observations(&entries),
        [
            ("opened", "open"),
            ("synchronize", "open"),
            ("closed", "merged"),
            ("reopened", "open"),
        ]
    );

    let record = shown(&kestrel, "acme", &workspace).await;
    let latest = &known(&record)[0];
    assert_eq!(latest["state"], "open");
    assert_eq!(latest["head_revision"], MOVED);
    assert_eq!(latest["updated_at"], REOPENED_AT);

    let events = kestrel.events("acme").await;
    assert_eq!(events.len(), 4);
    assert!(kestrel.sessions(workspace.id).await.is_empty());
    for event in &events {
        assert!(kestrel.firings(event.record_id).await.is_empty());
    }

    let cli = client::ran_by(
        &kestrel,
        &[
            "--organization",
            "acme",
            "workspace",
            "show",
            &workspace.name,
            "--json",
        ],
        client::Invocation::default(),
    )
    .await;
    assert!(cli.status.success(), "the client failed: {}", cli.err);
    let printed: Value = serde_json::from_str(&cli.out.join("\n")).expect("json");
    assert_eq!(printed["pull_requests"], record["pull_requests"]);

    let cli = client::ran_by(
        &kestrel,
        &[
            "--organization",
            "acme",
            "workspace",
            "transcript",
            &workspace.name,
            "--json",
        ],
        client::Invocation::default(),
    )
    .await;
    assert!(cli.status.success(), "the client failed: {}", cli.err);
    let actions: Vec<String> = cli
        .out
        .iter()
        .filter_map(|line| {
            let entry: Value = serde_json::from_str(line).expect("a transcript entry");
            (entry["entry"]["type"] == "pull_request").then(|| {
                entry["entry"]["action"]
                    .as_str()
                    .expect("an action")
                    .to_owned()
            })
        })
        .collect();
    assert_eq!(actions, ["opened", "synchronize", "closed", "reopened"]);

    kestrel.teardown().await;
}

#[tokio::test]
async fn two_pull_requests_on_one_branch_keep_their_own_values() {
    let kestrel = Kestrel::boot().await;
    declared(&kestrel, "acme", &[BASE]).await;
    let (github, _stub) = watching(&kestrel, "acme", BASE).await;
    let workspace = kestrel
        .open_workspace_on("acme", "kestrel", "builder", "feature")
        .await;

    deliver(
        &kestrel,
        &github,
        "d-seven",
        &opened(7, REVISION, OPENED_AT),
    )
    .await;
    deliver(&kestrel, &github, "d-nine", &opened(9, MOVED, MOVED_AT)).await;
    deliver(
        &kestrel,
        &github,
        "d-seven-closed",
        &closed(7, REVISION, CLOSED_AT),
    )
    .await;
    deliver(
        &kestrel,
        &github,
        "d-nine-merged",
        &merged(9, MOVED, REOPENED_AT),
    )
    .await;
    attached(&kestrel, workspace.id, 4).await;

    let record = shown(&kestrel, "acme", &workspace).await;
    let known = known(&record);
    assert_eq!(known.len(), 2);
    assert_eq!(known[0]["number"], 7);
    assert_eq!(known[0]["state"], "closed");
    assert_eq!(known[1]["number"], 9);
    assert_eq!(known[1]["state"], "merged");
    assert_eq!(known[1]["head_revision"], MOVED);

    kestrel.teardown().await;
}

#[tokio::test]
async fn an_unsupported_pull_request_action_makes_no_workspace_entry() {
    let kestrel = Kestrel::boot().await;
    declared(&kestrel, "acme", &[BASE]).await;
    let (github, _stub) = watching(&kestrel, "acme", BASE).await;
    let workspace = kestrel
        .open_workspace_on("acme", "kestrel", "builder", "feature")
        .await;

    deliver(
        &kestrel,
        &github,
        "d-edited",
        &edited(7, REVISION, OPENED_AT),
    )
    .await;

    let events = kestrel.events("acme").await;
    assert_eq!(events.len(), 1, "the delivery stays an Organization Event");
    assert_eq!(
        events[0].occurrence.r#type,
        "com.github.pull_request.edited"
    );
    tokio::time::sleep(Duration::from_millis(600)).await;
    assert!(
        pull_request_entries(&kestrel, workspace.id)
            .await
            .is_empty()
    );
    assert!(kestrel.consideration(events[0].record_id).await.is_none());
    let record = shown(&kestrel, "acme", &workspace).await;
    assert!(known(&record).is_empty());
    assert!(kestrel.sessions(workspace.id).await.is_empty());

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_duplicate_observation_appends_no_second_entry_however_it_arrives() {
    let kestrel = Kestrel::boot().await;
    declared(&kestrel, "acme", &[BASE]).await;
    let (one, _one_stub) = watching_as(&kestrel, "acme", BASE, "kestrel-one").await;
    let (two, _two_stub) = watching_as(&kestrel, "acme", BASE, "kestrel-two").await;
    let workspace = kestrel
        .open_workspace_on("acme", "kestrel", "builder", "feature")
        .await;

    deliver(&kestrel, &one, "d-one", &opened(7, REVISION, OPENED_AT)).await;
    deliver(&kestrel, &two, "d-two", &opened(7, REVISION, OPENED_AT)).await;
    deliver(&kestrel, &one, "d-one", &opened(7, REVISION, OPENED_AT)).await;
    attached(&kestrel, workspace.id, 1).await;

    tokio::time::sleep(Duration::from_millis(600)).await;
    assert_eq!(
        pull_request_entries(&kestrel, workspace.id).await.len(),
        1,
        "a repeated observation manufactured history"
    );
    let events = kestrel.events("acme").await;
    assert_eq!(
        events.len(),
        2,
        "each delivery is still an Event: {events:?}"
    );
    let record = shown(&kestrel, "acme", &workspace).await;
    assert_eq!(known(&record).len(), 1);
    assert_eq!(
        consideration(&kestrel, event_named(&events, "d-two").record_id)
            .await
            .outcome,
        "attached"
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_delayed_older_delivery_appears_in_history_without_regressing_the_value() {
    let kestrel = Kestrel::boot().await;
    declared(&kestrel, "acme", &[BASE]).await;
    let (github, _stub) = watching(&kestrel, "acme", BASE).await;
    let workspace = kestrel
        .open_workspace_on("acme", "kestrel", "builder", "feature")
        .await;

    deliver(
        &kestrel,
        &github,
        "d-moved",
        &synchronize(7, MOVED, MOVED_AT),
    )
    .await;
    attached(&kestrel, workspace.id, 1).await;
    deliver(
        &kestrel,
        &github,
        "d-delayed",
        &opened(7, REVISION, OPENED_AT),
    )
    .await;
    let entries = attached(&kestrel, workspace.id, 2).await;
    assert_eq!(
        observations(&entries),
        [("synchronize", "open"), ("opened", "open")],
        "the delayed observation is kept in the order it was learned"
    );

    let record = shown(&kestrel, "acme", &workspace).await;
    let latest = &known(&record)[0];
    assert_eq!(latest["head_revision"], MOVED);
    assert_eq!(latest["updated_at"], MOVED_AT);

    deliver(
        &kestrel,
        &github,
        "d-delayed-again",
        &opened(7, REVISION, OPENED_AT),
    )
    .await;
    tokio::time::sleep(Duration::from_millis(600)).await;
    assert_eq!(pull_request_entries(&kestrel, workspace.id).await.len(), 2);

    kestrel.teardown().await;
}

#[tokio::test]
async fn conflicting_ties_at_the_same_source_time_are_settled_by_the_repositorys_read() {
    let kestrel = Kestrel::boot().await;
    declared(&kestrel, "acme", &[BASE]).await;
    let (github, stub) = watching(&kestrel, "acme", BASE).await;
    let workspace = kestrel
        .open_workspace_on("acme", "kestrel", "builder", "feature")
        .await;
    stub.script_answer(
        "GET",
        "/repos/jtmthf/kestrel/pulls/7",
        ScriptedResponse::ok(
            json!({
                "number": 7,
                "html_url": format!("{}/pull/7", url(BASE)),
                "title": "Pull request 7",
                "state": "closed",
                "merged": true,
                "updated_at": OPENED_AT,
                "head": {
                    "ref": "feature",
                    "sha": REVISION,
                    "repo": { "full_name": BASE, "html_url": url(BASE) }
                },
                "base": { "ref": "main", "repo": { "full_name": BASE } }
            })
            .to_string(),
        ),
    );

    deliver(
        &kestrel,
        &github,
        "d-opened",
        &opened(7, REVISION, OPENED_AT),
    )
    .await;
    attached(&kestrel, workspace.id, 1).await;
    // The same source moment, the opposite state: arrival order may not decide it.
    deliver(
        &kestrel,
        &github,
        "d-closed",
        &closed(7, REVISION, OPENED_AT),
    )
    .await;
    let entries = attached(&kestrel, workspace.id, 2).await;
    assert_eq!(
        observations(&entries),
        [("opened", "open"), ("closed", "closed")]
    );

    let record = shown(&kestrel, "acme", &workspace).await;
    let latest = &known(&record)[0];
    assert_eq!(
        latest["state"], "merged",
        "the tied value is the repository's current answer"
    );
    assert_eq!(latest["updated_at"], OPENED_AT);
    assert!(
        stub.requests()
            .iter()
            .any(|request| request.url.contains("/pulls/7")),
        "the tie was never read back"
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_tie_the_repository_cannot_settle_keeps_the_value_it_held() {
    let kestrel = Kestrel::boot().await;
    declared(&kestrel, "acme", &[BASE]).await;
    let (github, _stub) = watching(&kestrel, "acme", BASE).await;
    let workspace = kestrel
        .open_workspace_on("acme", "kestrel", "builder", "feature")
        .await;

    deliver(
        &kestrel,
        &github,
        "d-opened",
        &opened(7, REVISION, OPENED_AT),
    )
    .await;
    attached(&kestrel, workspace.id, 1).await;
    // The stub answers nothing for the read back, so the tie stays as it was.
    deliver(
        &kestrel,
        &github,
        "d-closed",
        &closed(7, REVISION, OPENED_AT),
    )
    .await;
    let entries = attached(&kestrel, workspace.id, 2).await;
    assert_eq!(
        observations(&entries),
        [("opened", "open"), ("closed", "closed")]
    );

    let record = shown(&kestrel, "acme", &workspace).await;
    assert_eq!(known(&record)[0]["state"], "open");
    assert_eq!(known(&record)[0]["updated_at"], OPENED_AT);

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_sealed_workspaces_transcript_and_values_stay_fixed_while_the_event_remains() {
    let kestrel = Kestrel::boot().await;
    declared(&kestrel, "acme", &[BASE]).await;
    let (github, _stub) = watching(&kestrel, "acme", BASE).await;
    let workspace = kestrel
        .open_workspace_on("acme", "kestrel", "builder", "feature")
        .await;

    deliver(
        &kestrel,
        &github,
        "d-opened",
        &opened(7, REVISION, OPENED_AT),
    )
    .await;
    attached(&kestrel, workspace.id, 1).await;
    kestrel.seal_workspace(workspace.id).await;

    deliver(
        &kestrel,
        &github,
        "d-moved",
        &synchronize(7, MOVED, MOVED_AT),
    )
    .await;
    deliver(&kestrel, &github, "d-closed", &merged(7, MOVED, CLOSED_AT)).await;

    let events = kestrel.events("acme").await;
    assert_eq!(
        events.len(),
        3,
        "every Event remains in Organization history"
    );
    for delivery in ["d-moved", "d-closed"] {
        let considered = consideration(&kestrel, event_named(&events, delivery).record_id).await;
        assert_eq!(considered.outcome, "sealed");
        assert_eq!(considered.candidates, [(workspace.id, "sealed".to_owned())]);
    }
    assert_eq!(pull_request_entries(&kestrel, workspace.id).await.len(), 1);
    let record = shown(&kestrel, "acme", &workspace).await;
    let latest = &known(&record)[0];
    assert_eq!(latest["state"], "open");
    assert_eq!(latest["head_revision"], REVISION);
    assert_eq!(latest["updated_at"], OPENED_AT);
    assert!(kestrel.sessions(workspace.id).await.is_empty());

    kestrel.teardown().await;
}

#[tokio::test]
async fn learning_resumes_across_a_restart_without_a_second_entry() {
    let kestrel = Kestrel::boot().await;
    declared(&kestrel, "acme", &[BASE]).await;
    let (github, _stub) = watching(&kestrel, "acme", BASE).await;
    let workspace = kestrel
        .open_workspace_on("acme", "kestrel", "builder", "feature")
        .await;

    deliver(
        &kestrel,
        &github,
        "d-opened",
        &opened(7, REVISION, OPENED_AT),
    )
    .await;
    attached(&kestrel, workspace.id, 1).await;

    let kestrel = kestrel.kill_and_restart().await;
    deliver(
        &kestrel,
        &github,
        "d-moved",
        &synchronize(7, MOVED, MOVED_AT),
    )
    .await;
    let entries = attached(&kestrel, workspace.id, 2).await;

    assert_eq!(
        observations(&entries),
        [("opened", "open"), ("synchronize", "open")],
        "the restarted control plane learned the moved head, and the opening once"
    );
    let record = shown(&kestrel, "acme", &workspace).await;
    assert_eq!(known(&record)[0]["head_revision"], MOVED);
    assert_eq!(kestrel.events("acme").await.len(), 2);
    assert!(kestrel.sessions(workspace.id).await.is_empty());

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_trigger_declared_for_the_event_fires_as_it_would_without_the_workspace_learning_it() {
    let kestrel = Kestrel::boot().await;
    declared(&kestrel, "acme", &[BASE]).await;
    let (github, _stub) = watching(&kestrel, "acme", BASE).await;
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

    deliver(&kestrel, &github, "d-1", &opened(7, REVISION, OPENED_AT)).await;
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

/// Polled only: no signing secret, and nothing ever reaches its webhook.
async fn polling(kestrel: &Kestrel, organization: &str, repository: &str) -> GithubStub {
    let stub = GithubStub::start();
    kestrel
        .register_integration(
            organization,
            &repository.replace('/', "-"),
            repository,
            &stub.base_url(),
            &[kestrel::domain::Direction::Inbound],
            jiff::SignedDuration::from_millis(1),
        )
        .await;

    stub
}

#[tokio::test]
async fn a_polled_integration_learns_a_pull_request_opened_pushed_to_and_merged() {
    let kestrel = Kestrel::boot().await;
    declared(&kestrel, "acme", &[BASE]).await;
    let stub = polling(&kestrel, "acme", BASE).await;
    let workspace = kestrel
        .open_workspace_on("acme", "kestrel", "builder", "feature")
        .await;
    assert_eq!(
        shown(&kestrel, "acme", &workspace).await["pull_requests"][0]["availability"],
        "available",
        "a polled integration can learn this repository's pull requests"
    );

    stub.deliver(opened(7, REVISION, OPENED_AT).logged());
    stub.deliver(synchronize(7, MOVED, MOVED_AT).logged());
    stub.deliver(merged(7, MOVED, CLOSED_AT).logged());
    let entries = attached(&kestrel, workspace.id, 3).await;

    assert_eq!(
        observations(&entries),
        [
            ("opened", "open"),
            ("synchronize", "open"),
            ("closed", "merged")
        ]
    );
    let record = shown(&kestrel, "acme", &workspace).await;
    assert_eq!(record["pull_requests"][0]["availability"], "available");
    assert_eq!(known(&record)[0]["state"], "merged");
    assert_eq!(known(&record)[0]["head_revision"], MOVED);

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_trigger_on_a_closed_pull_request_fires_for_a_polled_integration() {
    let kestrel = Kestrel::boot().await;
    declared(&kestrel, "acme", &[BASE]).await;
    kestrel
        .declare_trigger_rendering(
            "acme",
            "landed",
            &json!({"exact": {"type": "com.github.pull_request.closed"}}).to_string(),
            "kestrel",
            "builder",
            &templates("Follow up {{ event.data.pull_request.title }}", None, None),
        )
        .await;
    let stub = polling(&kestrel, "acme", BASE).await;

    stub.deliver(merged(7, MOVED, CLOSED_AT).logged());

    let deadline = tokio::time::Instant::now() + PATIENCE;
    loop {
        if let Some(event) = kestrel.events("acme").await.first()
            && !kestrel.firings(event.record_id).await.is_empty()
        {
            assert_eq!(event.occurrence.r#type, "com.github.pull_request.closed");
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the trigger never fired"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    kestrel.teardown().await;
}

/// A Delivery GitHub could reach kestrel with is recorded on arrival and recognised when the poll
/// lists it, or the other way round: either way it is one Event, and one firing.
#[tokio::test]
async fn a_delivery_by_webhook_and_by_poll_is_one_event_in_either_order() {
    for webhook_first in [true, false] {
        let kestrel = Kestrel::boot().await;
        declared(&kestrel, "acme", &[BASE]).await;
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
        let (github, stub) = watching(&kestrel, "acme", BASE).await;
        let sent = opened(7, REVISION, OPENED_AT);
        let guid = "6f9b2a40-c131-11f1-9cac-dea11dd30571";

        if webhook_first {
            deliver(&kestrel, &github, guid, &sent).await;
            stub.deliver_as(sent.logged(), guid);
        } else {
            stub.deliver_as(sent.logged(), guid);
            while kestrel.events("acme").await.is_empty() {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            deliver(&kestrel, &github, guid, &sent).await;
        }
        let polls = |stub: &GithubStub| {
            stub.requests()
                .iter()
                .filter(|request| request.url.starts_with("/app/hook/deliveries?"))
                .count()
        };
        let listed = polls(&stub);
        while polls(&stub) < listed + 3 {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        let events = kestrel.events("acme").await;
        assert_eq!(events.len(), 1, "webhook first: {webhook_first}");
        assert_eq!(events[0].occurrence.id, guid);
        let deadline = tokio::time::Instant::now() + PATIENCE;
        while kestrel.firings(events[0].record_id).await.is_empty() {
            assert!(
                tokio::time::Instant::now() < deadline,
                "the trigger never fired"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        tokio::time::sleep(Duration::from_millis(300)).await;

        assert_eq!(kestrel.firings(events[0].record_id).await.len(), 1);
        assert_eq!(
            stub.requests()
                .iter()
                .filter(|request| request.url.starts_with("/app/hook/deliveries/"))
                .count(),
            usize::from(!webhook_first),
            "a payload already recorded was fetched again"
        );

        kestrel.teardown().await;
    }
}
