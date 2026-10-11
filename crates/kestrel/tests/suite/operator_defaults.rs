//! What a write from a Client takes from the install's Operator when it names nothing itself:
//! the Participant, the Organization and the Subscription Profile (ADR-0049).

use std::time::Duration;

use jiff::SignedDuration;
use kestrel::domain::{Schedule, Session};
use kestrel::log::Entry;
use reqwest::StatusCode;
use serde_json::{Value, json};

use crate::support::client::{Finished, Invocation, ran_by};
use crate::support::fixture::Fixture;
use crate::support::link_client::Link;
use crate::support::{Kestrel, OnTheLink, repository, templates};

const ORGANIZATION: &str = "/operator/organizations/acme";

struct Answered {
    status: StatusCode,
    body: Value,
}

async fn sent(request: reqwest::RequestBuilder) -> Answered {
    let response = request.send().await.expect("the operator boundary answers");
    let status = response.status();
    let text = response.text().await.expect("a body");

    Answered {
        status,
        body: serde_json::from_str(&text).unwrap_or(Value::String(text)),
    }
}

async fn put(kestrel: &Kestrel, path: &str, body: Value) -> Answered {
    sent(
        reqwest::Client::new()
            .put(format!("{}{path}", kestrel.operator()))
            .json(&body),
    )
    .await
}

async fn post(kestrel: &Kestrel, path: &str, body: Value) -> Answered {
    sent(
        reqwest::Client::new()
            .post(format!("{}{path}", kestrel.operator()))
            .json(&body),
    )
    .await
}

async fn get(kestrel: &Kestrel, path: &str) -> Value {
    let read = sent(reqwest::Client::new().get(format!("{}{path}", kestrel.operator()))).await;
    assert_eq!(read.status, StatusCode::OK, "{:#}", read.body);

    read.body
}

async fn name_the_operator(kestrel: &Kestrel, name: &str) -> Value {
    let named = put(kestrel, "/operator/operator", json!({ "name": name })).await;
    assert!(named.status.is_success(), "{:#}", named.body);

    named.body
}

/// `acme` with one Agent, `builder`, on `harness`, declared before any Operator is named so it
/// stays the only Organization.
async fn acme(harness: &str) -> Kestrel {
    let kestrel = Kestrel::boot().await;
    Fixture::acme()
        .checked_out()
        .harness(harness)
        .declare(&kestrel)
        .await;

    kestrel
}

async fn open(kestrel: &Kestrel, naming: Value) -> Answered {
    let mut declaration = json!({
        "project": repository::NAME,
        "agent": "builder",
        "brief": "do the work",
    });
    declaration
        .as_object_mut()
        .expect("an object")
        .extend(naming.as_object().expect("an object").clone());

    post(kestrel, &format!("{ORGANIZATION}/workspaces"), declaration).await
}

fn is_the_naming_gap(refused: &Answered) {
    assert_eq!(refused.status, StatusCode::CONFLICT, "{:#}", refused.body);
    assert_eq!(refused.body["kind"], "setup_gap");
    assert_eq!(refused.body["field"], "participant");
    assert_eq!(refused.body["context"]["prerequisite"], "operator");
    assert_eq!(refused.body["next_steps"][0]["action"], "name_operator");
}

#[tokio::test]
async fn an_omitted_participant_is_the_operator_and_a_rename_changes_only_what_is_written_next() {
    let kestrel = acme("scripted").await;
    name_the_operator(&kestrel, "ada").await;

    let opened = open(&kestrel, json!({})).await;
    assert_eq!(opened.status, StatusCode::CREATED, "{:#}", opened.body);
    assert_eq!(opened.body["workspace"]["started_by"], "ada");
    let workspace = format!(
        "{ORGANIZATION}/workspaces/{}",
        opened.body["workspace"]["id"].as_str().expect("an id")
    );

    for message in [
        json!({ "message": "and add a test" }),
        json!({ "participant": "bob", "message": "and format it" }),
    ] {
        let posted = post(&kestrel, &format!("{workspace}/messages"), message).await;
        assert_eq!(posted.status, StatusCode::OK, "{:#}", posted.body);
    }
    name_the_operator(&kestrel, "grace").await;
    let posted = post(
        &kestrel,
        &format!("{workspace}/messages"),
        json!({ "message": "and update the docs" }),
    )
    .await;
    assert_eq!(posted.status, StatusCode::OK, "{:#}", posted.body);

    let id = opened.body["workspace"]["id"]
        .as_str()
        .expect("an id")
        .parse()
        .expect("a workspace id");
    let authors: Vec<String> = kestrel
        .transcript(id)
        .await
        .into_iter()
        .filter_map(|recorded| match recorded.entry {
            Entry::Said { participant, .. } => Some(participant),
            _ => None,
        })
        .collect();
    assert_eq!(
        authors,
        ["ada", "bob", "grace"],
        "a name given was replaced, or a rename rewrote what was already written"
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_write_that_needs_an_author_names_the_gap_when_no_operator_is_named() {
    let kestrel = acme("scripted").await;

    let opened = open(&kestrel, json!({ "participant": "ada" })).await;
    assert_eq!(opened.status, StatusCode::CREATED, "{:#}", opened.body);
    let workspace = format!(
        "{ORGANIZATION}/workspaces/{}",
        opened.body["workspace"]["id"].as_str().expect("an id")
    );

    let anonymous = post(
        &kestrel,
        &format!("{workspace}/messages"),
        json!({ "message": "and add a test" }),
    )
    .await;
    is_the_naming_gap(&anonymous);
    let interrupted = post(
        &kestrel,
        &format!(
            "{ORGANIZATION}/sessions/{}/interrupt",
            opened.body["session"]["id"].as_str().expect("an id")
        ),
        json!({}),
    )
    .await;
    is_the_naming_gap(&interrupted);

    kestrel.teardown().await;
}

const CLAUDE_TOKEN: &str = "sk-ant-oat01-the-operators-subscription";
const SUBSCRIPTION: &str = "CLAUDE_CODE_OAUTH_TOKEN";
const ORGANIZATION_KEY: &str = "ANTHROPIC_API_KEY";

/// Saved where the catalogue says a subscription is held: the Operator's own Profile, declared
/// under their name when they have none.
async fn save_the_subscription(kestrel: &Kestrel) {
    let saved = put(
        kestrel,
        &format!("{ORGANIZATION}/sign-ins/claude/claude-setup-token"),
        json!({ "value": CLAUDE_TOKEN }),
    )
    .await;
    assert_eq!(saved.status, StatusCode::OK, "{:#}", saved.body);
}

async fn hold_the_organizations_key(kestrel: &Kestrel) {
    let held = put(
        kestrel,
        &format!("{ORGANIZATION}/credentials/{ORGANIZATION_KEY}"),
        json!({ "secret": "sk-ant-api03-organization-key" }),
    )
    .await;
    assert!(held.status.is_success(), "{:#}", held.body);
}

async fn declare_a_profile(kestrel: &Kestrel, declaration: Value) {
    let name = declaration["name"].as_str().expect("a name").to_owned();
    let declared = post(kestrel, &format!("{ORGANIZATION}/profiles"), declaration).await;
    assert_eq!(declared.status, StatusCode::CREATED, "{:#}", declared.body);
    let held = put(
        kestrel,
        &format!("{ORGANIZATION}/profiles/{name}/variables/{SUBSCRIPTION}"),
        json!({ "secret": CLAUDE_TOKEN }),
    )
    .await;
    assert_eq!(held.status, StatusCode::OK, "{:#}", held.body);
}

/// The queued Session on an Instance's link, and the variables its harness is handed.
async fn handed(kestrel: &Kestrel) -> (Session, OnTheLink, Vec<String>) {
    let session = kestrel.claim_session().await.expect("a session to claim");
    let on = kestrel.on_the_link(&session).await;
    kestrel.start_on_the_link(&session).await;
    let response = Link::to(&kestrel.link())
        .credentials(&on.instance, session.id, Some(&on.credential))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let handed: Value = response.json().await.expect("credentials");
    let variables = handed["variables"]
        .as_object()
        .expect("named values")
        .keys()
        .cloned()
        .collect();

    (session, on, variables)
}

#[tokio::test]
async fn work_the_operator_opens_takes_their_own_subscription_and_keeps_it_through_a_rename() {
    let kestrel = acme("claude").await;
    name_the_operator(&kestrel, "ada").await;
    save_the_subscription(&kestrel).await;
    hold_the_organizations_key(&kestrel).await;

    let opened = open(&kestrel, json!({})).await;
    assert_eq!(opened.status, StatusCode::CREATED, "{:#}", opened.body);
    assert_eq!(opened.body["workspace"]["profile"], "ada");
    assert_eq!(
        opened.body["session"]["sign_in_method"],
        "claude-setup-token"
    );
    let (_, _, variables) = handed(&kestrel).await;
    assert_eq!(
        variables,
        [SUBSCRIPTION],
        "the Organization's key would let a refused subscription fall back"
    );

    name_the_operator(&kestrel, "grace").await;
    let after_the_rename = open(&kestrel, json!({})).await;
    assert_eq!(after_the_rename.body["workspace"]["profile"], "ada");
    assert_eq!(
        after_the_rename.body["session"]["sign_in_method"],
        "claude-setup-token"
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_profile_that_was_named_or_is_not_the_operators_own_is_never_replaced_or_chosen() {
    let kestrel = acme("claude").await;
    name_the_operator(&kestrel, "ada").await;
    declare_a_profile(&kestrel, json!({ "name": "shared", "owner": "ada" })).await;
    hold_the_organizations_key(&kestrel).await;

    let unowned = open(&kestrel, json!({})).await;
    assert_eq!(unowned.status, StatusCode::CREATED, "{:#}", unowned.body);
    assert_eq!(
        unowned.body["workspace"]["profile"],
        Value::Null,
        "a Profile whose free-text owner reads like the Operator was taken for theirs"
    );
    assert_eq!(unowned.body["session"]["sign_in_method"], Value::Null);

    save_the_subscription(&kestrel).await;
    let named = open(&kestrel, json!({ "profile": "shared" })).await;
    assert_eq!(named.status, StatusCode::CREATED, "{:#}", named.body);
    assert_eq!(named.body["workspace"]["profile"], "shared");
    assert_eq!(named.body["session"]["sign_in_method"], Value::Null);

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_harness_the_operator_holds_no_subscription_for_uses_the_organizations_credentials() {
    let kestrel = acme("codex").await;
    name_the_operator(&kestrel, "ada").await;
    save_the_subscription(&kestrel).await;

    let opened = open(&kestrel, json!({})).await;
    assert_eq!(opened.status, StatusCode::CREATED, "{:#}", opened.body);
    assert_eq!(opened.body["workspace"]["profile"], Value::Null);
    assert_eq!(opened.body["session"]["sign_in_method"], Value::Null);

    kestrel.teardown().await;
}

#[tokio::test]
async fn several_of_the_operators_profiles_holding_the_subscription_are_a_conflict_not_a_choice() {
    let kestrel = acme("claude").await;
    name_the_operator(&kestrel, "ada").await;
    declare_a_profile(&kestrel, json!({ "name": "work" })).await;
    declare_a_profile(&kestrel, json!({ "name": "personal" })).await;

    let refused = open(&kestrel, json!({})).await;
    assert_eq!(refused.status, StatusCode::CONFLICT, "{:#}", refused.body);
    assert_eq!(refused.body["kind"], "state_conflict");
    assert_eq!(refused.body["field"], "profile");
    assert_eq!(
        refused.body["context"]["state"],
        "several_operator_profiles"
    );
    assert_eq!(
        get(&kestrel, &format!("{ORGANIZATION}/workspaces")).await,
        json!([]),
        "a refused open left a Workspace behind"
    );

    let named = open(&kestrel, json!({ "profile": "personal" })).await;
    assert_eq!(named.status, StatusCode::CREATED, "{:#}", named.body);
    assert_eq!(named.body["workspace"]["profile"], "personal");

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_session_enqueued_in_work_a_person_opened_takes_the_subscription_saved_since() {
    let kestrel = acme("claude").await;
    name_the_operator(&kestrel, "ada").await;
    let opened = open(&kestrel, json!({})).await;
    assert_eq!(opened.body["workspace"]["profile"], Value::Null);
    let workspace = format!(
        "{ORGANIZATION}/workspaces/{}",
        opened.body["workspace"]["id"].as_str().expect("an id")
    );
    kestrel
        .stop_session(
            opened.body["session"]["id"]
                .as_str()
                .expect("an id")
                .parse()
                .expect("a session id"),
        )
        .await;

    save_the_subscription(&kestrel).await;
    let enqueued = post(&kestrel, &format!("{workspace}/sessions"), json!({})).await;
    assert_eq!(enqueued.status, StatusCode::CREATED, "{:#}", enqueued.body);
    assert_eq!(enqueued.body["sign_in_method"], "claude-setup-token");
    assert_eq!(get(&kestrel, &workspace).await["profile"], "ada");

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_triggers_work_never_takes_the_operators_subscription() {
    let kestrel = acme("claude").await;
    name_the_operator(&kestrel, "ada").await;
    save_the_subscription(&kestrel).await;
    let trigger = kestrel
        .try_declare_scheduled_trigger(
            "acme",
            "sweep",
            Schedule::Every(SignedDuration::from_hours(1)),
            &templates("Sweep the backlog", None, None),
        )
        .await
        .expect("an hourly schedule should declare");

    kestrel
        .elapse(trigger.declared_at + SignedDuration::from_hours(1))
        .await;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    let opened = loop {
        let opened = get(&kestrel, &format!("{ORGANIZATION}/workspaces")).await;
        if opened != json!([]) {
            break opened;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the schedule never opened a Workspace"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    };
    let fired = &opened[0];
    assert_eq!(
        fired["started_by"],
        kestrel.events("acme").await[0].record_id.to_string()
    );
    assert_eq!(fired["profile"], Value::Null);
    assert_eq!(fired["session"]["sign_in_method"], Value::Null);

    let id = fired["session"]["id"]
        .as_str()
        .expect("an id")
        .parse()
        .expect("a session id");
    kestrel.stop_session(id).await;
    let workspace = format!(
        "{ORGANIZATION}/workspaces/{}",
        fired["id"].as_str().expect("an id")
    );
    let enqueued = post(&kestrel, &format!("{workspace}/sessions"), json!({})).await;
    assert_eq!(enqueued.status, StatusCode::CREATED, "{:#}", enqueued.body);
    assert_eq!(enqueued.body["sign_in_method"], Value::Null);
    assert_eq!(
        get(&kestrel, &workspace).await["profile"],
        Value::Null,
        "an enqueue handed a Trigger's Workspace the Operator's subscription"
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn initial_setup_reads_incomplete_whatever_is_named_saved_opened_or_enqueued() {
    let kestrel = acme("claude").await;
    let incomplete = json!({ "initial_setup_complete": false });
    assert_eq!(get(&kestrel, "/operator/install").await, incomplete);

    name_the_operator(&kestrel, "ada").await;
    save_the_subscription(&kestrel).await;
    let opened = open(&kestrel, json!({})).await;
    assert_eq!(opened.status, StatusCode::CREATED, "{:#}", opened.body);
    kestrel
        .stop_session(
            opened.body["session"]["id"]
                .as_str()
                .expect("an id")
                .parse()
                .expect("a session id"),
        )
        .await;
    let enqueued = post(
        &kestrel,
        &format!(
            "{ORGANIZATION}/workspaces/{}/sessions",
            opened.body["workspace"]["id"].as_str().expect("an id")
        ),
        json!({}),
    )
    .await;
    assert_eq!(enqueued.status, StatusCode::CREATED, "{:#}", enqueued.body);
    assert_eq!(get(&kestrel, "/operator/install").await, incomplete);

    let kestrel = kestrel.teardown().await.restart().await;
    assert_eq!(get(&kestrel, "/operator/install").await, incomplete);

    kestrel.teardown().await;
}

fn succeeded(finished: &Finished) {
    assert!(
        finished.status.success(),
        "the client refused:\n{}",
        finished.err
    );
}

fn is_the_naming_gap_at_a_shell(finished: &Finished) {
    assert_eq!(finished.status.code(), Some(78), "{}", finished.err);
    assert!(
        finished.err.contains("kestrel operator name"),
        "the refusal does not say how to name the Operator:\n{}",
        finished.err
    );
}

#[tokio::test]
async fn the_cli_writes_as_the_operator_it_names_and_renames() {
    let kestrel = acme("scripted").await;
    let shell = Invocation::default;

    let named = ran_by(&kestrel, &["operator", "name", "ada", "--json"], shell()).await;
    succeeded(&named);
    assert_eq!(named.json()["name"], "ada");
    let opened = ran_by(
        &kestrel,
        &[
            "workspace",
            "open",
            "--project",
            repository::NAME,
            "--agent",
            "builder",
            "--brief",
            "do the work",
            "--json",
        ],
        shell(),
    )
    .await;
    succeeded(&opened);
    assert_eq!(opened.json()["workspace"]["started_by"], "ada");

    succeeded(
        &ran_by(
            &kestrel,
            &["workspace", "post", "latest", "and add a test"],
            shell(),
        )
        .await,
    );
    succeeded(&ran_by(&kestrel, &["operator", "name", "grace"], shell()).await);
    let shown = ran_by(&kestrel, &["operator", "show", "--json"], shell()).await;
    assert_eq!(shown.json()["name"], "grace");
    assert_eq!(shown.json()["id"], named.json()["id"]);
    succeeded(
        &ran_by(
            &kestrel,
            &["workspace", "post", "latest", "and update the docs"],
            shell(),
        )
        .await,
    );
    succeeded(
        &ran_by(
            &kestrel,
            &[
                "workspace",
                "post",
                "latest",
                "and format it",
                "--as-participant",
                "bob",
            ],
            shell(),
        )
        .await,
    );

    let id = opened.json()["workspace"]["id"]
        .as_str()
        .expect("an id")
        .parse()
        .expect("a workspace id");
    let authors: Vec<String> = kestrel
        .transcript(id)
        .await
        .into_iter()
        .filter_map(|recorded| match recorded.entry {
            Entry::Said { participant, .. } => Some(participant),
            _ => None,
        })
        .collect();
    assert_eq!(authors, ["ada", "grace", "bob"]);

    kestrel.teardown().await;
}

#[tokio::test]
async fn with_nothing_in_scope_and_no_operator_the_cli_names_the_gap_and_declares_nothing() {
    let kestrel = Kestrel::boot().await;

    let listed = ran_by(&kestrel, &["project", "list"], Invocation::default()).await;
    is_the_naming_gap_at_a_shell(&listed);

    let started = ran_by(
        &kestrel,
        &["start", "--brief", "do the work"],
        Invocation::default()
            .cloned(repository::url(), repository::NAME)
            .within(repository::NAME),
    )
    .await;
    is_the_naming_gap_at_a_shell(&started);
    assert!(
        kestrel.organizations().await.is_empty(),
        "a start named an Organization nobody asked for"
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_start_works_in_the_operators_organization_whoever_owns_the_repository() {
    let kestrel = Kestrel::boot().await;
    succeeded(
        &ran_by(
            &kestrel,
            &["operator", "name", "ada"],
            Invocation::default(),
        )
        .await,
    );

    let started = ran_by(
        &kestrel,
        &[
            "start",
            "--brief",
            "do the work",
            "--repository",
            "https://github.com/acme/widgets.git",
            "--branch",
            "main",
            "--json",
        ],
        Invocation::default(),
    )
    .await;
    succeeded(&started);
    assert_eq!(started.json()["organization"]["name"], "ada");
    assert_eq!(started.json()["workspace"]["started_by"], "ada");
    let organizations: Vec<String> = kestrel
        .organizations()
        .await
        .into_iter()
        .map(|organization| organization.name)
        .collect();
    assert_eq!(organizations, ["ada"]);

    kestrel.teardown().await;
}

#[tokio::test]
async fn an_operator_is_named_only_what_a_participant_could_be_written_as() {
    let kestrel = Kestrel::boot().await;

    for unwritable in ["x".repeat(65), "con\u{7}trol".to_owned()] {
        let refused = put(
            &kestrel,
            "/operator/operator",
            json!({ "name": unwritable }),
        )
        .await;
        assert_eq!(
            refused.status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{:#}",
            refused.body
        );
        assert_eq!(refused.body["field"], "name");
    }
    let unnamed =
        sent(reqwest::Client::new().get(format!("{}/operator/operator", kestrel.operator()))).await;
    assert_eq!(unnamed.status, StatusCode::NOT_FOUND);

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_subscription_a_session_found_broken_is_not_replaced_by_the_organizations_key() {
    let kestrel = acme("claude").await;
    name_the_operator(&kestrel, "ada").await;
    save_the_subscription(&kestrel).await;
    hold_the_organizations_key(&kestrel).await;

    let opened = open(&kestrel, json!({})).await;
    assert_eq!(opened.status, StatusCode::CREATED, "{:#}", opened.body);
    let (session, on, _) = handed(&kestrel).await;
    let refused = Link::to(&kestrel.link())
        .report_body(
            &on.instance,
            Some(&on.credential),
            &json!({
                "kind": "finished",
                "session": session.id,
                "seq": 1,
                "exit": {"status": "failed", "because": "the agent would not open a session"},
                "evidence": {
                    "kind": "authentication_required",
                    "code": -32000,
                    "methods": [],
                    "method": null,
                },
            }),
        )
        .await;
    assert_eq!(refused.status(), StatusCode::ACCEPTED);
    let saved = get(&kestrel, &format!("{ORGANIZATION}/sign-ins")).await;
    let subscription = saved
        .as_array()
        .expect("saved sign-ins")
        .iter()
        .find(|saved| saved["method"] == "claude-setup-token")
        .expect("the subscription");
    assert_eq!(
        subscription["authentication"]["state"],
        "authentication_failed"
    );

    let next = open(&kestrel, json!({})).await;
    assert_eq!(next.status, StatusCode::CREATED, "{:#}", next.body);
    assert_eq!(next.body["workspace"]["profile"], "ada");
    assert_eq!(next.body["session"]["sign_in_method"], "claude-setup-token");
    let (_, _, variables) = handed(&kestrel).await;
    assert_eq!(variables, [SUBSCRIPTION]);

    let changed = open(&kestrel, json!({ "sign_in_method": "anthropic-api-key" })).await;
    assert_eq!(changed.status, StatusCode::CREATED, "{:#}", changed.body);
    assert_eq!(
        changed.body["workspace"]["profile"],
        Value::Null,
        "naming the Organization's key still took the Operator's Profile"
    );
    assert_eq!(
        changed.body["session"]["sign_in_method"],
        "anthropic-api-key"
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_post_that_starts_the_next_session_takes_the_subscription_saved_since() {
    let kestrel = acme("claude").await;
    name_the_operator(&kestrel, "ada").await;
    let opened = open(&kestrel, json!({})).await;
    let workspace = format!(
        "{ORGANIZATION}/workspaces/{}",
        opened.body["workspace"]["id"].as_str().expect("an id")
    );
    kestrel
        .stop_session(
            opened.body["session"]["id"]
                .as_str()
                .expect("an id")
                .parse()
                .expect("a session id"),
        )
        .await;

    save_the_subscription(&kestrel).await;
    let posted = post(
        &kestrel,
        &format!("{workspace}/messages"),
        json!({ "message": "carry on" }),
    )
    .await;
    assert_eq!(posted.status, StatusCode::OK, "{:#}", posted.body);
    assert_eq!(
        posted.body["session"]["sign_in_method"],
        "claude-setup-token"
    );
    assert_eq!(get(&kestrel, &workspace).await["profile"], "ada");

    kestrel.teardown().await;
}
