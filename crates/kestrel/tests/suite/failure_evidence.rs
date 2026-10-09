//! How a Session failed, as generic facts the supervisor established, carried across the link,
//! kept with the Session and read back as a typed diagnostic (ADR-0052).

use crate::support;

use kestrel::domain::{Exit, Session, Workspace};
use kestrel::log::Entry;
use reqwest::StatusCode;
use serde_json::{Value, json};
use support::client;
use support::link_client::Link;
use support::scripted_agent::{self, Script};
use support::{HARNESS, Kestrel, OnTheLink};

async fn a_workspace(kestrel: &Kestrel) -> Workspace {
    let organization = kestrel.declare_organization("acme").await;
    kestrel
        .declare_project(
            &organization,
            support::repository::NAME,
            &[support::repository::url().to_owned()],
            support::repository::BRANCH,
        )
        .await;
    kestrel
        .declare_agent(&organization, "builder", HARNESS, None)
        .await;
    kestrel
        .hold_provider_credential(
            &organization,
            support::PROVIDER_KEY,
            support::A_PROVIDER_KEY,
        )
        .await;

    kestrel.open_workspace("acme", "kestrel", "builder").await
}

async fn ended_spawning(command: &str) -> (Kestrel, Session) {
    let kestrel = Kestrel::dispatching_to(support::supervisor::binary(), command).await;
    let workspace = a_workspace(&kestrel).await;
    let session = kestrel.enqueue_session(workspace.id).await;
    let ended = kestrel.after_one_turn(session.id).await;

    (kestrel, ended)
}

async fn read(kestrel: &Kestrel, session: &Session) -> Value {
    let response = reqwest::get(format!(
        "{}/operator/organizations/acme/sessions/{}",
        kestrel.operator(),
        session.id
    ))
    .await
    .expect("the operator should answer");
    assert_eq!(response.status(), StatusCode::OK, "a failed Session reads");

    response.json().await.expect("a Session")
}

fn failed(session: &Session) {
    assert!(
        matches!(session.exit, Some(Exit::Failed { .. })),
        "the session ended {:?}",
        session.exit
    );
}

#[tokio::test]
async fn an_agent_that_must_be_signed_in_leaves_authentication_evidence_on_its_session() {
    let (kestrel, session) = ended_spawning(&scripted_agent::playing(Script::Insists)).await;
    failed(&session);

    let read = read(&kestrel, &session).await;
    let diagnostic = &read["diagnostic"];
    assert_eq!(diagnostic["kind"], "authentication_failed", "{read:#}");
    assert_eq!(
        diagnostic["context"]["evidence"],
        json!({
            "kind": "authentication_required",
            "code": -32000,
            "methods": ["its-own"],
            "method": null,
        })
    );
    assert_eq!(diagnostic["context"]["session"], session.id.to_string());
    assert_eq!(diagnostic["context"]["harness"], HARNESS);
    assert_eq!(diagnostic["context"]["expired"], Value::Null);
    assert_eq!(diagnostic["context"]["covered"], Value::Null);
    assert_eq!(diagnostic["next_steps"][0]["action"], "sign_in");
    assert_eq!(diagnostic["next_steps"][0]["harness"], HARNESS);
    assert_eq!(diagnostic["next_steps"][1]["action"], "inspect_resource");
    assert_eq!(diagnostic["next_steps"][1]["organization"], "acme");

    kestrel.teardown().await;
}

#[tokio::test]
async fn showing_a_failed_session_is_a_successful_read_that_says_why() {
    let (kestrel, session) = ended_spawning(&scripted_agent::playing(Script::Insists)).await;

    let operator = kestrel.operator();
    let id = session.id.to_string();
    let shown = tokio::task::spawn_blocking(move || {
        client::ran_on_a_terminal(&operator, &["session", "show", &id], 200, "")
    })
    .await
    .expect("the client should run");

    assert!(shown.status.success(), "{}", shown.said);
    assert!(
        shown
            .said
            .contains("the harness asked for authentication (code -32000)"),
        "{}",
        shown.said
    );
    assert!(
        shown.said.contains(&format!(
            "kestrel session show {} --organization acme",
            session.id
        )),
        "{}",
        shown.said
    );
    assert!(!shown.said.contains("run a step?"), "{}", shown.said);
    kestrel.teardown().await;
}

#[tokio::test]
async fn a_harness_that_is_not_installed_leaves_the_spawn_error_on_its_session() {
    let (kestrel, session) = ended_spawning("kestrel-no-such-harness acp").await;
    failed(&session);

    let read = read(&kestrel, &session).await;
    let diagnostic = &read["diagnostic"];
    assert_eq!(diagnostic["kind"], "executable_missing", "{read:#}");
    assert_eq!(
        diagnostic["context"]["executable"],
        "kestrel-no-such-harness"
    );
    assert_eq!(
        diagnostic["context"]["evidence"]["command"],
        "kestrel-no-such-harness"
    );
    assert_eq!(
        diagnostic["context"]["evidence"]["error"]["kind"],
        "not_found"
    );
    assert_eq!(
        diagnostic["next_steps"][0]["action"],
        "inspect_harness_image"
    );
    assert_eq!(
        diagnostic["next_steps"][0]["command"],
        "kestrel-no-such-harness"
    );

    kestrel.teardown().await;
}

/// The agent's error reads like an expired login and repeats the Provider Credential it was
/// handed; neither its wording nor the secret reaches what kestrel keeps.
#[tokio::test]
async fn output_that_only_reads_like_an_expired_login_stays_unknown_and_holds_no_secret() {
    let (kestrel, session) = ended_spawning(&scripted_agent::playing(Script::Complains)).await;
    failed(&session);

    let read = read(&kestrel, &session).await;
    let diagnostic = &read["diagnostic"];
    assert_eq!(diagnostic["kind"], "unknown_failure", "{read:#}");
    assert_eq!(diagnostic["context"]["evidence"]["kind"], "unknown");
    let because = read["exit"]["because"].as_str().expect("why it failed");
    assert!(because.contains("401"), "{because}");
    assert!(
        !read.to_string().contains(support::A_PROVIDER_KEY),
        "a secret was kept: {read:#}"
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_session_that_failed_without_evidence_reads_with_no_diagnostic() {
    let kestrel = Kestrel::boot().await;
    let workspace = a_workspace(&kestrel).await;
    let session = kestrel.dispatch_session(workspace.id).await;
    kestrel.fail_session(&session, "the lease expired").await;

    assert_eq!(read(&kestrel, &session).await["diagnostic"], Value::Null);

    kestrel.teardown().await;
}

fn finished(session: &Session, seq: i64, evidence: &Value) -> Value {
    json!({
        "kind": "finished",
        "session": session.id,
        "seq": seq,
        "exit": {"status": "failed", "because": "the agent would not open a session"},
        "evidence": evidence,
    })
}

async fn reported(link: &Link, on: &OnTheLink, body: &Value) -> StatusCode {
    link.report_body(&on.instance, Some(&on.credential), body)
        .await
        .status()
}

#[tokio::test]
async fn evidence_is_taken_once_and_survives_a_restart() {
    let kestrel = Kestrel::boot().await;
    let workspace = a_workspace(&kestrel).await;
    let (session, on) = kestrel.dispatch_to_the_link(workspace.id).await;
    let link = Link::to(&kestrel.link());
    let required = json!({
        "kind": "authentication_required",
        "code": -32000,
        "methods": ["its-own"],
        "method": "its-own",
    });

    let started = json!({"kind": "started", "session": session.id, "seq": 1});
    for _ in 0..2 {
        assert_eq!(reported(&link, &on, &started).await, StatusCode::ACCEPTED);
    }
    assert_eq!(
        reported(&link, &on, &finished(&session, 2, &required)).await,
        StatusCode::ACCEPTED
    );
    assert_eq!(
        reported(
            &link,
            &on,
            &finished(
                &session,
                2,
                &json!({"kind": "unknown", "summary": "a replay"})
            ),
        )
        .await,
        StatusCode::GONE,
        "a Session its finished report ended is no longer the supervisor's"
    );
    assert_eq!(
        kestrel
            .every_entry(workspace.id)
            .await
            .iter()
            .filter(|entry| matches!(
                entry,
                Entry::SessionStarted { .. } | Entry::SessionEnded { .. }
            ))
            .count(),
        2
    );
    assert_eq!(
        read(&kestrel, &session).await["diagnostic"]["context"]["evidence"],
        required
    );

    let kestrel = kestrel.kill().await.restart().await;
    let read = read(&kestrel, &session).await;
    assert_eq!(read["diagnostic"]["kind"], "authentication_failed");
    assert_eq!(read["diagnostic"]["context"]["evidence"], required);
    assert_eq!(read["diagnostic"]["next_steps"][0]["method"], "its-own");

    kestrel.teardown().await;
}

#[tokio::test]
async fn evidence_the_link_document_describes_is_taken() {
    let kestrel = Kestrel::boot().await;
    a_workspace(&kestrel).await;
    let link = Link::to(&kestrel.link());

    for evidence in [
        json!({"kind": "authentication_required", "code": -32000, "methods": [], "method": null}),
        json!({"kind": "executable_missing", "command": "codex-acp", "error": {"kind": "not_found", "code": 2}}),
        json!({"kind": "unknown", "summary": "the agent stopped"}),
    ] {
        let workspace = kestrel.open_workspace("acme", "kestrel", "builder").await;
        let (session, on) = kestrel.dispatch_to_the_link(workspace.id).await;
        assert_eq!(
            reported(&link, &on, &finished(&session, 1, &evidence)).await,
            StatusCode::ACCEPTED,
            "{evidence}"
        );
    }

    kestrel.teardown().await;
}
