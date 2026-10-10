use crate::support;

use std::time::Duration;

use hmac::{Hmac, KeyInit as _, Mac as _};
use jiff::SignedDuration;
use kestrel::domain::{Direction, IntegrationState};
use kestrel::integration::Change;
use sha2::Sha256;
use support::Kestrel;
use support::github_stub::{self, GithubStub};

const PATIENCE: Duration = Duration::from_secs(30);
const REPOSITORY: &str = "jtmthf/kestrel";
const BOTH: &[Direction] = &[Direction::Inbound, Direction::Outbound];
const LISTED: &str = "/app/hook/deliveries?";

async fn watching(kestrel: &Kestrel, stub: &GithubStub) {
    kestrel.declare_organization("acme").await;
    kestrel
        .register_integration(
            "acme",
            "github",
            REPOSITORY,
            &stub.base_url(),
            BOTH,
            SignedDuration::from_millis(1),
        )
        .await;
}

fn polls(stub: &GithubStub) -> usize {
    stub.requests()
        .iter()
        .filter(|request| request.url.starts_with(LISTED))
        .count()
}

async fn eventually(what: &str, done: impl AsyncFn() -> bool) {
    let deadline = tokio::time::Instant::now() + PATIENCE;
    while !done().await {
        assert!(
            tokio::time::Instant::now() < deadline,
            "{what} never happened"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

async fn a_while() {
    tokio::time::sleep(Duration::from_millis(500)).await;
}

#[tokio::test]
async fn a_disabled_integration_is_not_polled_and_catches_up_when_enabled() {
    let stub = GithubStub::start();
    let kestrel = Kestrel::boot().await;
    watching(&kestrel, &stub).await;
    eventually("a first poll", async || polls(&stub) > 0).await;

    let disabled = kestrel.disable_integration("acme", "github").await;
    assert_eq!(disabled.state, IntegrationState::Disabled);
    a_while().await;
    let asked = polls(&stub);
    stub.deliver(github_stub::labelled(43, "ready-for-agent"));
    a_while().await;

    assert_eq!(polls(&stub), asked, "a disabled integration was polled");
    assert!(kestrel.events("acme").await.is_empty());

    let enabled = kestrel.enable_integration("acme", "github").await;
    assert_eq!(enabled.state, IntegrationState::Enabled);
    assert!(enabled.revision > disabled.revision);
    eventually(
        "the delivery made while disabled being recorded",
        async || kestrel.events("acme").await.len() == 1,
    )
    .await;

    kestrel.teardown().await;
}

#[tokio::test]
async fn removing_the_inbound_direction_pauses_polling_and_restoring_it_resumes() {
    let stub = GithubStub::start();
    let kestrel = Kestrel::boot().await;
    watching(&kestrel, &stub).await;
    eventually("a first poll", async || polls(&stub) > 0).await;

    let outbound = kestrel
        .change_integration(
            "acme",
            "github",
            Change {
                carries: Some(&[Direction::Outbound]),
                ..Change::default()
            },
        )
        .await
        .expect("the directions should change");
    assert_eq!(outbound.carries, [Direction::Outbound]);
    a_while().await;
    let asked = polls(&stub);
    stub.deliver(github_stub::labelled(43, "ready-for-agent"));
    a_while().await;
    assert_eq!(
        polls(&stub),
        asked,
        "an outbound-only integration was polled"
    );

    kestrel
        .change_integration(
            "acme",
            "github",
            Change {
                carries: Some(BOTH),
                ..Change::default()
            },
        )
        .await
        .expect("the directions should change back");
    eventually("the delivery being caught up", async || {
        kestrel.events("acme").await.len() == 1
    })
    .await;

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_name_and_interval_change_in_place_and_bump_the_revision() {
    let stub = GithubStub::start();
    let kestrel = Kestrel::boot().await;
    watching(&kestrel, &stub).await;
    let before = kestrel.integration("acme", "github").await;

    let changed = kestrel
        .change_integration(
            "acme",
            "github",
            Change {
                name: Some("hub"),
                interval: Some(SignedDuration::from_mins(5)),
                revision: Some(before.revision),
                ..Change::default()
            },
        )
        .await
        .expect("the name and interval should change");

    assert_eq!(changed.id, before.id);
    assert_eq!(changed.name, "hub");
    assert_eq!(
        changed.github().expect("a github one").interval,
        SignedDuration::from_mins(5)
    );
    assert!(changed.revision > before.revision);
    assert_eq!(
        changed.github().expect("a github one").repository,
        before.github().expect("a github one").repository
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn an_invalid_change_leaves_what_was_saved() {
    let stub = GithubStub::start();
    let kestrel = Kestrel::boot().await;
    watching(&kestrel, &stub).await;
    let before = kestrel.integration("acme", "github").await;

    for change in [
        Change {
            interval: Some(SignedDuration::ZERO),
            name: Some("renamed"),
            ..Change::default()
        },
        Change {
            carries: Some(&[]),
            ..Change::default()
        },
    ] {
        assert!(
            kestrel
                .change_integration("acme", "github", change)
                .await
                .is_err()
        );
    }

    let after = kestrel.integration("acme", "github").await;
    assert_eq!(after.name, before.name);
    assert_eq!(after.carries, before.carries);
    assert_eq!(after.revision, before.revision);

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_change_against_an_older_revision_is_refused() {
    let stub = GithubStub::start();
    let kestrel = Kestrel::boot().await;
    watching(&kestrel, &stub).await;
    let before = kestrel.integration("acme", "github").await;
    kestrel.disable_integration("acme", "github").await;

    let stale = kestrel
        .change_integration(
            "acme",
            "github",
            Change {
                name: Some("hub"),
                revision: Some(before.revision),
                ..Change::default()
            },
        )
        .await;

    assert!(stale.is_err(), "a stale change overwrote a newer one");
    assert_eq!(kestrel.integration("acme", "github").await.name, "github");

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_generic_webhook_cannot_be_given_the_outbound_direction() {
    let kestrel = Kestrel::boot().await;
    kestrel.declare_organization("acme").await;
    kestrel
        .register_webhook("acme", "ci", "a-shared-secret")
        .await;

    let refused = kestrel
        .change_integration(
            "acme",
            "ci",
            Change {
                carries: Some(BOTH),
                ..Change::default()
            },
        )
        .await;

    assert!(refused.is_err());
    assert_eq!(
        kestrel.integration("acme", "ci").await.carries,
        [Direction::Inbound]
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_poll_begun_before_a_disable_records_nothing() {
    let stub = GithubStub::start();
    let kestrel = Kestrel::boot().await;
    kestrel.declare_organization("acme").await;
    kestrel
        .register_integration(
            "acme",
            "github",
            REPOSITORY,
            &stub.base_url(),
            BOTH,
            SignedDuration::from_hours(1),
        )
        .await;
    eventually("a first poll", async || polls(&stub) > 0).await;
    let stale = kestrel.integration("acme", "github").await;
    kestrel.disable_integration("acme", "github").await;
    stub.deliver(github_stub::labelled(43, "ready-for-agent"));

    let polled = kestrel.poll_as(&stale).await;

    assert_eq!(polled.recorded, 0);
    assert!(kestrel.events("acme").await.is_empty());
    let after = kestrel.integration("acme", "github").await;
    assert_eq!(after.deliveries_read_from, stale.deliveries_read_from);
    assert_eq!(after.state, IntegrationState::Disabled);

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_signed_webhook_to_a_disabled_integration_is_refused_and_the_poll_catches_it_up() {
    let stub = GithubStub::start();
    let kestrel = Kestrel::boot().await;
    kestrel.declare_organization("acme").await;
    let webhook_secret = uuid::Uuid::now_v7().to_string();
    let integration = kestrel
        .register_signed_github(
            "acme",
            "github",
            REPOSITORY,
            &stub.base_url(),
            &webhook_secret,
        )
        .await;
    kestrel.disable_integration("acme", "github").await;
    let delivery = github_stub::labelled(43, "ready-for-agent");
    let guid = stub.deliver(delivery.clone());
    let body = delivery.payload.to_string().into_bytes();
    let mut mac = Hmac::<Sha256>::new_from_slice(webhook_secret.as_bytes()).expect("an HMAC key");
    mac.update(&body);
    let signature: String = mac
        .finalize()
        .into_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();

    let answered = reqwest::Client::new()
        .post(format!("{}{}", kestrel.link(), integration.webhook_path()))
        .header("content-type", "application/json")
        .header("x-github-event", &delivery.event)
        .header("x-github-delivery", &guid)
        .header("x-hub-signature-256", format!("sha256={signature}"))
        .body(body)
        .send()
        .await
        .expect("the webhook answers")
        .status();

    assert!(answered.is_client_error(), "answered {answered}");
    assert!(kestrel.events("acme").await.is_empty());

    kestrel.enable_integration("acme", "github").await;
    eventually("the refused delivery being polled", async || {
        kestrel.events("acme").await.len() == 1
    })
    .await;
    assert_eq!(kestrel.events("acme").await[0].occurrence.id, guid);

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_change_that_changes_nothing_keeps_the_revision() {
    let stub = GithubStub::start();
    let kestrel = Kestrel::boot().await;
    watching(&kestrel, &stub).await;
    let before = kestrel.integration("acme", "github").await;

    let unchanged = kestrel
        .change_integration(
            "acme",
            "github",
            Change {
                name: Some("github"),
                carries: Some(BOTH),
                ..Change::default()
            },
        )
        .await
        .expect("an empty change is not refused");

    assert_eq!(unchanged.revision, before.revision);

    kestrel.teardown().await;
}

#[tokio::test]
async fn retiring_erases_what_it_authenticates_with_and_keeps_its_history() {
    let stub = GithubStub::start();
    let kestrel = Kestrel::boot().await;
    kestrel.declare_organization("acme").await;
    let registered = kestrel
        .register_signed_github(
            "acme",
            "github",
            REPOSITORY,
            &stub.base_url(),
            &uuid::Uuid::now_v7().to_string(),
        )
        .await;
    stub.deliver(github_stub::labelled(43, "ready-for-agent"));
    eventually("the delivery being recorded", async || {
        kestrel.events("acme").await.len() == 1
    })
    .await;

    let retired = kestrel.retire_integration("acme", "github").await;

    assert_eq!(retired.id, registered.id);
    assert_eq!(retired.state, IntegrationState::Retired);
    assert!(retired.retired_at.is_some());
    assert!(retired.revision > registered.revision);
    let github = retired.github().expect("still a github one");
    assert_eq!(github.repository, REPOSITORY);
    assert_eq!(github.credential.id, 1);
    assert!(github.credential.private_key().is_none());
    assert!(!kestrel.authenticates_webhooks(&retired).await);
    let events = kestrel.events("acme").await;
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].integration, Some(retired.id));

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_retired_integration_is_never_maintained_or_enabled_again() {
    let stub = GithubStub::start();
    let kestrel = Kestrel::boot().await;
    watching(&kestrel, &stub).await;
    let retired = kestrel.retire_integration("acme", "github").await;

    assert!(
        kestrel
            .try_enable_integration("acme", "github")
            .await
            .is_err()
    );
    assert!(
        kestrel
            .change_integration(
                "acme",
                "github",
                Change {
                    name: Some("hub"),
                    ..Change::default()
                },
            )
            .await
            .is_err()
    );

    let again = kestrel.retire_integration("acme", "github").await;
    assert_eq!(again.revision, retired.revision);
    assert_eq!(again.retired_at, retired.retired_at);
    assert_eq!(again.name, "github");

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_retired_integration_is_not_polled() {
    let stub = GithubStub::start();
    let kestrel = Kestrel::boot().await;
    watching(&kestrel, &stub).await;
    eventually("a first poll", async || polls(&stub) > 0).await;

    kestrel.retire_integration("acme", "github").await;
    a_while().await;
    let asked = stub.requests().len();
    stub.deliver(github_stub::labelled(43, "ready-for-agent"));
    a_while().await;

    assert_eq!(
        stub.requests().len(),
        asked,
        "a retired integration reached github"
    );
    assert!(kestrel.events("acme").await.is_empty());

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_poll_begun_before_retirement_asks_and_records_nothing() {
    let stub = GithubStub::start();
    let kestrel = Kestrel::boot().await;
    kestrel.declare_organization("acme").await;
    kestrel
        .register_integration(
            "acme",
            "github",
            REPOSITORY,
            &stub.base_url(),
            BOTH,
            SignedDuration::from_hours(1),
        )
        .await;
    eventually("a first poll", async || polls(&stub) > 0).await;
    let stale = kestrel.integration("acme", "github").await;
    kestrel.retire_integration("acme", "github").await;
    stub.deliver(github_stub::labelled(43, "ready-for-agent"));
    let asked = stub.requests().len();

    let polled = kestrel.poll_as(&stale).await;

    assert_eq!(polled.recorded, 0);
    assert_eq!(
        stub.requests().len(),
        asked,
        "a key read before retirement was presented after it"
    );
    assert!(kestrel.events("acme").await.is_empty());
    let after = kestrel.integration("acme", "github").await;
    assert_eq!(after.state, IntegrationState::Retired);
    assert_eq!(after.poll_due_at, None);
    assert_eq!(after.last_event_refusal.map(|refusal| refusal.reason), None);

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_webhook_to_a_retired_integration_is_refused_whatever_it_presents() {
    let kestrel = Kestrel::boot().await;
    kestrel.declare_organization("acme").await;
    let secret = uuid::Uuid::now_v7().to_string();
    let integration = kestrel.register_webhook("acme", "ci", &secret).await;
    kestrel.retire_integration("acme", "ci").await;

    let answered = reqwest::Client::new()
        .post(format!("{}{}", kestrel.link(), integration.webhook_path()))
        .bearer_auth(&secret)
        .header("content-type", "application/json")
        .body("{}")
        .send()
        .await
        .expect("the webhook answers")
        .status();

    assert!(answered.is_client_error(), "answered {answered}");
    assert!(kestrel.events("acme").await.is_empty());

    kestrel.teardown().await;
}
