//! A Client operating Workspaces and Sessions over the operator boundary, and naming them by
//! name, identifier, prefix or `latest`.

use crate::support;

use std::collections::{HashMap, HashSet};

use kestrel::domain::WorkspaceId;
use serde_json::Value;
use support::Kestrel;
use support::client;
use support::fixture::Fixture;
use support::operator_requests::{client, failed, recorded, succeeded};

fn generated_name(record: &Value) -> &str {
    let name = record["name"].as_str().expect("a generated name");
    let mut words = name.split('-');
    let adjective = words.next().expect("an adjective");
    let noun = words.next().expect("a noun");
    let suffix = words.next().expect("a generated suffix");
    assert!(!adjective.is_empty(), "a name has an adjective");
    assert!(!noun.is_empty(), "a name has a noun");
    assert_eq!(suffix.len(), 8, "a name has an eight-letter suffix");
    assert!(words.next().is_none(), "a name has no extra words");
    name
}

/// The shortest prefix of `id` that names no other identifier, so an unambiguous reference
/// is unambiguous whatever the generated identifiers turn out to be.
fn shortest_prefix_of(id: &str, others: &[&str]) -> String {
    let mut length = 1;
    while others.iter().any(|other| other.starts_with(&id[..length])) {
        length += 1;
    }

    id[..length].to_owned()
}

#[tokio::test]
async fn a_client_operates_workspaces_and_sessions_without_opening_a_database() {
    let kestrel = Kestrel::boot().await;
    succeeded(&client(&kestrel, &["organization", "declare", "acme"]).await);
    succeeded(
        &client(
            &kestrel,
            &[
                "project",
                "declare",
                "kestrel",
                "--organization",
                "acme",
                "--repository",
                "https://github.com/jtmthf/kestrel",
                "--branch",
                "main",
            ],
        )
        .await,
    );
    succeeded(
        &client(
            &kestrel,
            &["agent", "declare", "builder", "--organization", "acme"],
        )
        .await,
    );

    let opened = recorded(
        &client(
            &kestrel,
            &[
                "workspace",
                "open",
                "--organization",
                "acme",
                "--project",
                "kestrel",
                "--agent",
                "builder",
                "--json",
            ],
        )
        .await,
    );
    let workspace = opened[0]["workspace"]["id"]
        .as_str()
        .expect("a workspace id")
        .to_owned();
    let workspace_name = opened[0]["workspace"]["name"]
        .as_str()
        .expect("a workspace name")
        .to_owned();
    let session = opened[0]["session"]["id"]
        .as_str()
        .expect("a session id")
        .to_owned();

    let listed = recorded(
        &client(
            &kestrel,
            &["workspace", "list", "--organization", "acme", "--json"],
        )
        .await,
    );
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0]["id"], workspace);
    assert_eq!(listed[0]["name"], workspace_name);
    generated_name(&listed[0]);
    let shown = recorded(&client(&kestrel, &["workspace", "show", &workspace, "--json"]).await);
    let fields = ["id", "name", "state", "continues"];
    assert_eq!(
        client::picked(&shown[0], &fields),
        client::picked(&listed[0], &fields)
    );

    let queued = recorded(
        &client(
            &kestrel,
            &[
                "session",
                "show",
                &session,
                "--organization",
                "acme",
                "--json",
            ],
        )
        .await,
    );
    assert_eq!(queued[0]["workspace"], workspace);
    assert_eq!(queued[0]["state"], "queued");
    let first_session_name = generated_name(&queued[0]).to_owned();
    let refused = client(
        &kestrel,
        &["session", "enqueue", "--workspace", &workspace, "--json"],
    )
    .await;
    assert!(
        !refused.status.success(),
        "a queued session holds the workspace"
    );

    let posted = client(
        &kestrel,
        &[
            "workspace",
            "post",
            &workspace,
            "--as-participant",
            "operator",
            "start with the operator boundary",
        ],
    )
    .await;
    succeeded(&posted);
    assert_eq!(
        posted.out,
        vec![session.clone()],
        "a post to a queued session becomes its Brief and hands back the session"
    );

    let completed = kestrel
        .claim_session()
        .await
        .expect("the queued session should wait for the worker");
    assert_eq!(completed.id.to_string(), session);
    kestrel.complete_session(&completed).await;
    let sealed = recorded(&client(&kestrel, &["workspace", "seal", &workspace, "--json"]).await);
    assert_eq!(sealed[0]["state"], "sealed");

    let continued = recorded(
        &client(
            &kestrel,
            &[
                "workspace",
                "open",
                "--organization",
                "acme",
                "--project",
                "kestrel",
                "--agent",
                "builder",
                "--continues",
                &workspace,
                "--json",
            ],
        )
        .await,
    );
    let continuing = continued[0]["workspace"]["id"]
        .as_str()
        .expect("a continuing workspace")
        .to_owned();
    assert_ne!(continued[0]["workspace"]["name"], workspace_name);
    let continuing_shown =
        recorded(&client(&kestrel, &["workspace", "show", &continuing, "--json"]).await);
    assert_eq!(continuing_shown[0]["continues"], workspace);

    let continuing_session = continued[0]["session"]["id"]
        .as_str()
        .expect("a session id")
        .to_owned();
    let completed = kestrel
        .claim_session()
        .await
        .expect("the continuation's session should claim");
    assert_eq!(completed.id.to_string(), continuing_session);
    kestrel.complete_session(&completed).await;

    let enqueued = recorded(
        &client(
            &kestrel,
            &[
                "session",
                "enqueue",
                "--workspace",
                &continuing,
                "--model",
                "claude-opus-5",
                "--json",
            ],
        )
        .await,
    );
    assert_ne!(generated_name(&enqueued[0]), first_session_name);
    assert_eq!(enqueued[0]["workspace"], continuing);
    assert_eq!(enqueued[0]["model"], "claude-opus-5");
    let listed_sessions = recorded(
        &client(
            &kestrel,
            &["session", "list", "--workspace", &continuing, "--json"],
        )
        .await,
    );
    assert!(
        listed_sessions
            .iter()
            .any(|session| session["id"] == enqueued[0]["id"]),
        "{listed_sessions:?}"
    );

    kestrel.teardown().await;
}

/// A review in the Workspace another Agent built in: a Session names its own Agent, and shows the
/// harness and model it runs on.
#[tokio::test]
async fn a_client_enqueues_a_session_naming_its_agent_and_shows_what_it_runs_on() {
    let kestrel = Kestrel::boot().await;
    Fixture::acme()
        .agent("reviewer", "claude", Some("claude-opus-5"))
        .declare(&kestrel)
        .await;
    let opened = recorded(
        &client(
            &kestrel,
            &[
                "workspace",
                "open",
                "--organization",
                "acme",
                "--project",
                "kestrel",
                "--agent",
                "builder",
                "--json",
            ],
        )
        .await,
    );
    let workspace = opened[0]["workspace"]["id"]
        .as_str()
        .expect("a workspace id")
        .to_owned();

    let first = opened[0]["session"]["id"].as_str().expect("a session id");
    let built = recorded(
        &client(
            &kestrel,
            &["session", "show", first, "--organization", "acme", "--json"],
        )
        .await,
    );
    kestrel
        .complete_session(&kestrel.session(first.parse().expect("an id")).await)
        .await;
    let reviewed = recorded(
        &client(
            &kestrel,
            &[
                "session",
                "enqueue",
                "--workspace",
                &workspace,
                "--agent",
                "reviewer",
                "--json",
            ],
        )
        .await,
    );
    let review = reviewed[0]["id"].as_str().expect("a session id");
    let shown = recorded(
        &client(
            &kestrel,
            &[
                "session",
                "show",
                review,
                "--organization",
                "acme",
                "--json",
            ],
        )
        .await,
    );

    assert_eq!(built[0]["agent"], "builder");
    assert_eq!(built[0]["harness"], "opencode");
    assert_eq!(built[0]["model"], Value::Null);
    assert_eq!(shown, reviewed);
    assert_eq!(shown[0]["agent"], "reviewer");
    assert_eq!(shown[0]["harness"], "claude");
    assert_eq!(shown[0]["model"], "claude-opus-5");

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_client_naming_an_agent_the_organization_never_declared_enqueues_nothing() {
    let kestrel = Kestrel::boot().await;
    Fixture::acme().declare(&kestrel).await;
    let opened = recorded(
        &client(
            &kestrel,
            &[
                "workspace",
                "open",
                "--organization",
                "acme",
                "--project",
                "kestrel",
                "--agent",
                "builder",
                "--json",
            ],
        )
        .await,
    );
    let workspace: WorkspaceId = opened[0]["workspace"]["id"]
        .as_str()
        .expect("a workspace id")
        .parse()
        .expect("a workspace identifier");
    kestrel
        .complete_session(
            &kestrel
                .session(
                    opened[0]["session"]["id"]
                        .as_str()
                        .expect("a session id")
                        .parse()
                        .expect("a session identifier"),
                )
                .await,
        )
        .await;

    let refused = client(
        &kestrel,
        &[
            "session",
            "enqueue",
            "--workspace",
            &workspace.to_string(),
            "--agent",
            "reviewer",
        ],
    )
    .await;

    assert!(
        failed(&refused).contains("no agent named reviewer in the organization acme"),
        "{}",
        failed(&refused)
    );
    assert_eq!(refused.status.code(), Some(3));
    assert_eq!(kestrel.sessions(workspace).await.len(), 1);

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_client_names_a_workspace_by_name_identifier_prefix_and_latest() {
    let kestrel = Kestrel::boot().await;
    Fixture::acme().declare(&kestrel).await;

    let first = kestrel.open_workspace("acme", "kestrel", "builder").await;
    let second = kestrel.open_workspace("acme", "kestrel", "builder").await;
    let (first_id, second_id) = (first.id.to_string(), second.id.to_string());

    let by_name = recorded(&client(&kestrel, &["workspace", "show", &second.name, "--json"]).await);
    assert_eq!(by_name[0]["id"], second_id);

    let by_id = recorded(&client(&kestrel, &["workspace", "show", &first_id, "--json"]).await);
    assert_eq!(by_id[0]["name"], first.name);

    let prefix = shortest_prefix_of(&first_id, &[&second_id]);
    assert!(
        prefix.len() < first_id.len(),
        "two workspaces opened into the one identifier"
    );
    let by_prefix = recorded(&client(&kestrel, &["workspace", "show", &prefix, "--json"]).await);
    assert_eq!(by_prefix[0]["id"], first_id);

    let latest = recorded(&client(&kestrel, &["workspace", "show", "latest", "--json"]).await);
    assert_eq!(latest[0]["id"], second_id);

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_workspace_reference_matching_several_is_refused_naming_them() {
    let kestrel = Kestrel::boot().await;
    Fixture::acme().declare(&kestrel).await;
    for _ in 0..17 {
        kestrel.open_workspace("acme", "kestrel", "builder").await;
    }

    let listed = recorded(&client(&kestrel, &["workspace", "list", "--json"]).await);
    let mut by_leading: HashMap<char, Vec<(String, String)>> = HashMap::new();
    for record in &listed {
        let id = record["id"].as_str().expect("an identifier").to_owned();
        let name = record["name"]
            .as_str()
            .expect("a generated name")
            .to_owned();
        by_leading
            .entry(id.chars().next().expect("an identifier"))
            .or_default()
            .push((id, name));
    }
    let (leading, matching) = by_leading
        .into_iter()
        .find(|(_, matching)| matching.len() > 1)
        .expect("seventeen identifiers over sixteen leading digits share one");

    let refused = client(&kestrel, &["workspace", "show", &leading.to_string()]).await;
    let said = failed(&refused);

    assert!(said.contains("ambiguous"), "{said}");
    assert_eq!(
        refused.status.code(),
        Some(3),
        "an ambiguous prefix is unresolved"
    );
    for (id, name) in &matching {
        assert!(said.contains(name), "{said} does not name {name}");
        assert!(said.contains(id), "{said} does not name {id}");
    }

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_workspace_reference_never_reaches_across_the_organizations_in_scope() {
    let kestrel = Kestrel::boot().await;
    for name in ["acme", "globex"] {
        Fixture::acme().organization(name).declare(&kestrel).await;
    }
    let acme = kestrel.open_workspace("acme", "kestrel", "builder").await;
    let globex = kestrel.open_workspace("globex", "kestrel", "builder").await;

    let refused = client(
        &kestrel,
        &[
            "workspace",
            "show",
            &acme.id.to_string(),
            "--organization",
            "globex",
        ],
    )
    .await;
    let said = failed(&refused);
    assert!(
        said.contains("no workspace in the organization globex matches"),
        "{said}"
    );

    let latest = recorded(
        &client(
            &kestrel,
            &[
                "workspace",
                "show",
                "latest",
                "--organization",
                "globex",
                "--json",
            ],
        )
        .await,
    );
    assert_eq!(latest[0]["id"], globex.id.to_string());

    let refused = client(
        &kestrel,
        &[
            "workspace",
            "show",
            "no-such-workspace",
            "--organization",
            "globex",
        ],
    )
    .await;
    let said = failed(&refused);
    assert!(
        said.contains("generated name") && said.contains("latest"),
        "the refusal is not corrective: {said}"
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_client_names_a_session_by_name_identifier_prefix_and_latest() {
    let kestrel = Kestrel::boot().await;
    Fixture::acme().declare(&kestrel).await;

    let workspace = kestrel.open_workspace("acme", "kestrel", "builder").await;
    let first = kestrel.enqueue_session(workspace.id).await;
    kestrel.stop_session(first.id).await;
    let second = kestrel.enqueue_session(workspace.id).await;
    let (first_id, second_id) = (first.id.to_string(), second.id.to_string());

    let by_name = recorded(&client(&kestrel, &["session", "show", &second.name, "--json"]).await);
    assert_eq!(by_name[0]["id"], second_id);

    let by_id = recorded(&client(&kestrel, &["session", "show", &first_id, "--json"]).await);
    assert_eq!(by_id[0]["name"], first.name);

    let prefix = shortest_prefix_of(&first_id, &[&second_id]);
    let by_prefix = recorded(&client(&kestrel, &["session", "show", &prefix, "--json"]).await);
    assert_eq!(by_prefix[0]["id"], first_id);

    let latest = recorded(&client(&kestrel, &["session", "show", "latest", "--json"]).await);
    assert_eq!(latest[0]["id"], second_id);

    kestrel.teardown().await;
}

#[tokio::test]
async fn workspace_and_session_names_remain_unique_when_creation_retries_collisions() {
    let kestrel = Kestrel::boot().await;
    Fixture::acme().declare(&kestrel).await;

    let mut workspace_names = HashSet::new();
    let mut session_names = HashSet::new();
    for _ in 0..100 {
        let workspace = kestrel.open_workspace("acme", "kestrel", "builder").await;
        assert!(workspace_names.insert(workspace.name));

        let session = kestrel.enqueue_session(workspace.id).await;
        assert!(session_names.insert(session.name));
    }

    kestrel.teardown().await;
}
