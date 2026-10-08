//! A database another writer holds past the busy timeout delays what the control plane does, and
//! stops none of it.

mod support;

use std::time::Duration;

use kestrel::domain::{SessionId, SessionState, Workspace};
use reqwest::StatusCode;
use reqwest::header::RETRY_AFTER;
use support::scripted_agent::Script;
use support::{Kestrel, repository, scripted_agent, supervisor};

const PATIENCE: Duration = Duration::from_secs(30);
/// SQLite's own, which sqlx sets on every connection it opens.
const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

async fn a_workspace(kestrel: &Kestrel, organization: &str) -> Workspace {
    let organization = kestrel.declare_organization(organization).await;
    kestrel
        .declare_project(
            &organization,
            repository::NAME,
            &[repository::url().to_owned()],
            repository::BRANCH,
        )
        .await;
    kestrel
        .declare_agent(
            &organization,
            "builder",
            "opencode",
            Some(kestrel_scripted_agent::OTHER_MODEL),
        )
        .await;
    kestrel
        .hold_provider_credential(
            &organization,
            support::PROVIDER_KEY,
            support::A_PROVIDER_KEY,
        )
        .await;

    kestrel
        .open_workspace(&organization.name, repository::NAME, "builder")
        .await
}

async fn in_flight(kestrel: &Kestrel, session: SessionId) {
    let deadline = tokio::time::Instant::now() + PATIENCE;

    loop {
        let session = kestrel.session(session).await;
        if session.state == SessionState::Working && session.supervisor.is_some() {
            return;
        }
        assert!(
            session.exit.is_none() && tokio::time::Instant::now() < deadline,
            "the session {} is {} with the exit status {:?}, and never reached its supervisor",
            session.id,
            session.state,
            session.exit
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test]
async fn a_database_locked_past_the_busy_timeout_stops_neither_the_control_plane_nor_its_sessions()
{
    let kestrel = Kestrel::dispatching_to(
        supervisor::binary(),
        &scripted_agent::playing(Script::Dawdles),
    )
    .await;
    let first = a_workspace(&kestrel, "acme").await;
    let second = a_workspace(&kestrel, "globex").await;
    let working = kestrel.enqueue_session(first.id).await;
    in_flight(&kestrel, working.id).await;

    let kept = a_workspace(&kestrel, "initech").await;
    let on = kestrel
        .keep_an_instance(kept.id, "local-exec/kestrel-kept")
        .await;

    let answers = kestrel
        .while_the_database_is_locked(async {
            let started = tokio::time::Instant::now();
            let client = reqwest::Client::new();
            let (operator, link) = tokio::join!(
                client
                    .get(format!(
                        "{}{}",
                        kestrel.operator(),
                        kestrel::operator::ORGANIZATIONS
                    ))
                    .send(),
                client
                    .post(format!(
                        "{}{}",
                        kestrel.link(),
                        kestrel::link::REPORTS
                            .replace("{instance}", &on.instance.replace('/', "%2F"))
                    ))
                    .bearer_auth(on.credential.as_str())
                    .json(&serde_json::json!({"kind": "heartbeat"}))
                    .send(),
            );
            // Every pass of the dispatch loop has run into the lock by the time this is over.
            tokio::time::sleep(BUSY_TIMEOUT.saturating_sub(started.elapsed())).await;
            tokio::time::sleep(BUSY_TIMEOUT).await;
            [operator, link]
        })
        .await;

    let [operator, link] = answers;
    for answered in [&operator, &link] {
        let answered = answered.as_ref().expect("both boundaries should answer");
        assert_eq!(
            answered.status(),
            StatusCode::SERVICE_UNAVAILABLE,
            "{}",
            answered.url()
        );
        assert!(
            answered.headers().contains_key(RETRY_AFTER),
            "a busy database is a reason to ask again, and {} said nothing of when",
            answered.url()
        );
    }
    let refusal: serde_json::Value = operator
        .expect("the operator boundary should answer")
        .json()
        .await
        .expect("a JSON refusal");
    assert_eq!(refusal["kind"], "unavailable", "{refusal}");
    assert_eq!(refusal["context"]["operation"], "list_organizations");
    assert_eq!(refusal["context"]["retry_after_seconds"], 1);
    assert_eq!(refusal["next_steps"][0]["action"], "retry_read");
    assert_eq!(refusal["next_steps"][0]["retry_after_seconds"], 1);
    assert!(
        kestrel.is_running(),
        "the control plane stopped for a busy database"
    );

    let still = kestrel.session(working.id).await;
    assert_eq!(still.state, SessionState::Working, "{:?}", still.exit);
    kestrel.stop_session(working.id).await;

    let next = kestrel.enqueue_session(second.id).await;
    in_flight(&kestrel, next.id).await;

    kestrel.teardown().await;
}
