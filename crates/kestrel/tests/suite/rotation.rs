//! A GitHub App's webhook secret is rotated as a durable operation: the replacement is saved
//! before GitHub is asked, both secrets verify until GitHub acknowledges it, and an interrupted
//! rotation is resumed by setting the same replacement again (ADR-0056).

use crate::support;

use std::time::Duration;

use hmac::{Hmac, KeyInit as _, Mac as _};
use kestrel::declined::{Reason, Step};
use kestrel::domain::{Integration, RotationFailure};
use reqwest::StatusCode;
use sha2::Sha256;
use support::fixture::Fixture;
use support::github_stub::{self, GithubStub, ScriptedResponse};
use support::{Kestrel, labelled_on};

const PATIENCE: Duration = Duration::from_secs(30);
const REPOSITORY: &str = "jtmthf/kestrel";
const HOOK: &str = "/app/hook/config";
const OLD: &str = "the-secret-github-signed-with-first";

async fn signed(kestrel: &Kestrel, stub: &GithubStub) -> Integration {
    kestrel.declare_organization("acme").await;
    kestrel
        .register_signed_github("acme", "github", REPOSITORY, &stub.base_url(), OLD)
        .await
}

/// Every secret kestrel asked GitHub to sign with, in the order it asked.
fn set_secrets(stub: &GithubStub) -> Vec<String> {
    stub.requests()
        .iter()
        .filter(|request| request.method == "PATCH" && request.url == HOOK)
        .map(|request| {
            serde_json::from_str::<serde_json::Value>(&request.body).expect("a JSON body")["secret"]
                .as_str()
                .expect("a secret is what was set")
                .to_owned()
        })
        .collect()
}

fn replacement(stub: &GithubStub) -> String {
    let secrets = set_secrets(stub);
    secrets.last().expect("github was asked").clone()
}

fn signature(secret: &str, body: &[u8]) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("an HMAC key");
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

async fn deliver(
    kestrel: &Kestrel,
    integration: &Integration,
    delivery: &github_stub::Delivery,
    guid: &str,
    secret: &str,
) -> StatusCode {
    let body = delivery.payload.to_string().into_bytes();
    reqwest::Client::new()
        .post(format!("{}{}", kestrel.link(), integration.webhook_path()))
        .header("content-type", "application/json")
        .header("x-github-event", &delivery.event)
        .header("x-github-delivery", guid)
        .header("x-hub-signature-256", signature(secret, &body))
        .body(body)
        .send()
        .await
        .expect("the webhook answers")
        .status()
}

async fn verifies(kestrel: &Kestrel, integration: &Integration, secret: &str) -> bool {
    let guid = uuid::Uuid::now_v7().to_string();
    let answered = deliver(
        kestrel,
        integration,
        &github_stub::labelled(7, "unwatched"),
        &guid,
        secret,
    )
    .await;
    assert!(
        answered == StatusCode::ACCEPTED || answered == StatusCode::UNAUTHORIZED,
        "answered {answered}"
    );
    answered == StatusCode::ACCEPTED
}

fn reason(refused: &anyhow::Error) -> &Reason {
    refused
        .downcast_ref::<Reason>()
        .unwrap_or_else(|| panic!("an untyped refusal: {refused:#}"))
}

fn offers_resume(next: &kestrel::declined::Next) -> bool {
    next.then.contains(&Step::ResumeWebhookSecretRotation {
        integration: "github".to_owned(),
    })
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

#[tokio::test]
async fn a_rotation_github_acknowledges_leaves_only_the_replacement_verifying() {
    let stub = GithubStub::start();
    let kestrel = Kestrel::boot().await;
    let before = signed(&kestrel, &stub).await;

    let rotated = kestrel
        .rotate_webhook_secret("acme", "github", None)
        .await
        .expect("a rotation github acknowledges should finish");

    assert!(rotated.rotation.is_none(), "{:?}", rotated.rotation);
    assert_eq!(rotated.id, before.id);
    let secret = replacement(&stub);
    assert_ne!(secret, OLD);
    assert!(secret.len() >= 32, "a guessable replacement: {secret}");
    let patched = stub
        .requests()
        .into_iter()
        .find(|request| request.method == "PATCH")
        .expect("github was asked");
    assert!(
        patched
            .headers
            .iter()
            .any(|(name, value)| name == "authorization" && value.starts_with("Bearer ey")),
        "the change was not asked for as the App"
    );
    assert!(verifies(&kestrel, &rotated, &secret).await);
    assert!(!verifies(&kestrel, &rotated, OLD).await);

    kestrel.teardown().await;
}

#[tokio::test]
async fn until_github_acknowledges_both_secrets_verify_and_polling_carries_on() {
    let stub = GithubStub::start();
    let kestrel = Kestrel::boot().await;
    Fixture::acme().declare(&kestrel).await;
    kestrel
        .declare_trigger(
            "acme",
            "ready",
            &labelled_on(REPOSITORY, "ready-for-agent"),
            "kestrel",
            "builder",
        )
        .await;
    let integration = kestrel
        .register_signed_github("acme", "github", REPOSITORY, &stub.base_url(), OLD)
        .await;
    stub.script_answer("PATCH", HOOK, ScriptedResponse::answering(502));

    let refused = kestrel
        .rotate_webhook_secret("acme", "github", None)
        .await
        .expect_err("github did not acknowledge the change");

    let Reason::GithubUnavailable { again, .. } = reason(&refused) else {
        panic!("{refused:#}");
    };
    assert_eq!(
        *again,
        kestrel::declined::Again::ResumeWebhookSecretRotation
    );
    let unfinished = kestrel.integration("acme", "github").await;
    let rotation = unfinished
        .rotation
        .as_ref()
        .expect("an unfinished rotation");
    assert!(rotation.attempted_at.is_some());
    assert_eq!(rotation.failure, Some(RotationFailure::Unanswered));
    let secret = replacement(&stub);
    assert!(verifies(&kestrel, &integration, OLD).await);
    assert!(verifies(&kestrel, &integration, &secret).await);

    let delivery = github_stub::labelled(43, "ready-for-agent");
    let guid = stub.deliver(delivery.clone());
    assert_eq!(
        deliver(&kestrel, &integration, &delivery, &guid, &secret).await,
        StatusCode::ACCEPTED
    );
    let polled_before = polls(&stub);
    eventually("the poll reading past the delivery", async || {
        polls(&stub) > polled_before + 2
    })
    .await;
    let events: Vec<_> = kestrel
        .events("acme")
        .await
        .into_iter()
        .filter(|event| event.occurrence.id == guid)
        .collect();
    assert_eq!(events.len(), 1, "{events:?}");
    eventually("the delivery firing its trigger", async || {
        !kestrel.firings(events[0].record_id).await.is_empty()
    })
    .await;
    assert_eq!(kestrel.firings(events[0].record_id).await.len(), 1);

    let resumed = kestrel
        .resume_webhook_secret_rotation("acme", "github")
        .await
        .expect("github acknowledging the change finishes the rotation");

    assert!(resumed.rotation.is_none());
    assert_eq!(set_secrets(&stub), [secret.clone(), secret.clone()]);
    assert!(verifies(&kestrel, &integration, &secret).await);
    assert!(!verifies(&kestrel, &integration, OLD).await);

    kestrel.teardown().await;
}

fn polls(stub: &GithubStub) -> usize {
    stub.requests()
        .iter()
        .filter(|request| request.url.starts_with("/app/hook/deliveries?"))
        .count()
}

#[tokio::test]
async fn a_restart_before_github_acknowledges_keeps_the_rotation_to_resume() {
    let stub = GithubStub::start();
    let kestrel = Kestrel::boot().await;
    let integration = signed(&kestrel, &stub).await;
    stub.script_answer("PATCH", HOOK, ScriptedResponse::answering(500));
    kestrel
        .rotate_webhook_secret("acme", "github", None)
        .await
        .expect_err("github did not acknowledge the change");
    let secret = replacement(&stub);

    let kestrel = kestrel.kill_and_restart().await;

    let unfinished = kestrel.integration("acme", "github").await;
    assert!(unfinished.rotation.is_some());
    assert!(verifies(&kestrel, &integration, OLD).await);
    assert!(verifies(&kestrel, &integration, &secret).await);
    let resumed = kestrel
        .resume_webhook_secret_rotation("acme", "github")
        .await
        .expect("the rotation resumes");
    assert!(resumed.rotation.is_none());
    assert_eq!(set_secrets(&stub), [secret.clone(), secret.clone()]);
    assert!(!verifies(&kestrel, &integration, OLD).await);

    kestrel.teardown().await;
}

/// GitHub masks the secret it reads back, so only setting the same one again proves it.
#[tokio::test]
async fn an_acknowledgement_lost_across_a_restart_is_resumed_by_setting_the_same_replacement() {
    let stub = GithubStub::start();
    let kestrel = Kestrel::boot().await;
    let integration = signed(&kestrel, &stub).await;
    stub.script_answer(
        "PATCH",
        HOOK,
        ScriptedResponse::ok("{}").after(Duration::from_secs(5)),
    );
    let rotating = kestrel.rotating_webhook_secret("acme", "github");
    eventually("github being asked", async || {
        !set_secrets(&stub).is_empty()
    })
    .await;
    rotating.abort();
    let secret = replacement(&stub);

    let kestrel = kestrel.kill_and_restart().await;

    let unfinished = kestrel.integration("acme", "github").await;
    let rotation = unfinished
        .rotation
        .as_ref()
        .expect("an unconfirmed rotation");
    assert!(rotation.attempted_at.is_some());
    assert_eq!(rotation.failure, None, "nothing said how the attempt ended");
    kestrel
        .resume_webhook_secret_rotation("acme", "github")
        .await
        .expect("the rotation resumes");
    assert_eq!(set_secrets(&stub), [secret.clone(), secret.clone()]);
    assert!(verifies(&kestrel, &integration, &secret).await);
    assert!(!verifies(&kestrel, &integration, OLD).await);

    kestrel.teardown().await;
}

#[tokio::test]
async fn an_app_another_integration_uses_is_not_rotated_and_the_other_is_not_named() {
    let stub = GithubStub::start();
    let kestrel = Kestrel::boot().await;
    signed(&kestrel, &stub).await;
    kestrel.declare_organization("elsewhere-corp").await;
    kestrel
        .register_signed_github(
            "elsewhere-corp",
            "their-private-connection",
            "elsewhere-corp/secret-plans",
            &stub.base_url(),
            "their-secret",
        )
        .await;
    kestrel
        .disable_integration("elsewhere-corp", "their-private-connection")
        .await;

    let refused = kestrel
        .rotate_webhook_secret("acme", "github", None)
        .await
        .expect_err("a shared App's hook is not this Integration's alone");

    let Reason::Forbidden { constraint, .. } = reason(&refused) else {
        panic!("{refused:#}");
    };
    assert_eq!(*constraint, "shared_app");
    let said = format!("{refused:#} {:?}", reason(&refused));
    for private in ["elsewhere-corp", "their-private-connection", "secret-plans"] {
        assert!(!said.contains(private), "{private} leaked: {said}");
    }
    assert!(set_secrets(&stub).is_empty());
    assert!(
        kestrel
            .integration("acme", "github")
            .await
            .rotation
            .is_none()
    );

    kestrel
        .retire_integration("elsewhere-corp", "their-private-connection")
        .await;
    kestrel
        .rotate_webhook_secret("acme", "github", None)
        .await
        .expect("a retired Integration does not share the App");

    kestrel.teardown().await;
}

#[tokio::test]
async fn retirement_ends_an_unfinished_rotation_and_erases_both_secrets() {
    let stub = GithubStub::start();
    let kestrel = Kestrel::boot().await;
    let integration = signed(&kestrel, &stub).await;
    stub.script_answer("PATCH", HOOK, ScriptedResponse::answering(502));
    kestrel
        .rotate_webhook_secret("acme", "github", None)
        .await
        .expect_err("github did not acknowledge the change");

    let retired = kestrel.retire_integration("acme", "github").await;

    assert!(retired.rotation.is_none());
    assert_eq!(kestrel.database().webhook_secret_rotations().await, 0);
    assert!(!kestrel.authenticates_webhooks(&retired).await);
    let refused = kestrel
        .resume_webhook_secret_rotation("acme", "github")
        .await
        .expect_err("a retired Integration has nothing to rotate");
    let Reason::StateConflict { state, .. } = reason(&refused) else {
        panic!("{refused:#}");
    };
    assert_eq!(*state, "retired");
    assert_eq!(set_secrets(&stub).len(), 1);
    assert!(!verifies(&kestrel, &integration, &replacement(&stub)).await);

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_rotation_begun_while_one_is_unfinished_is_refused_with_how_to_resume_it() {
    let stub = GithubStub::start();
    let kestrel = Kestrel::boot().await;
    signed(&kestrel, &stub).await;
    stub.script_answer("PATCH", HOOK, ScriptedResponse::answering(502));
    kestrel
        .rotate_webhook_secret("acme", "github", None)
        .await
        .expect_err("github did not acknowledge the change");

    let refused = kestrel
        .rotate_webhook_secret("acme", "github", None)
        .await
        .expect_err("one rotation at a time");

    let Reason::StateConflict { state, next, .. } = reason(&refused) else {
        panic!("{refused:#}");
    };
    assert_eq!(*state, "rotating_webhook_secret");
    assert!(offers_resume(next), "{next:?}");
    assert_eq!(set_secrets(&stub).len(), 1);

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_private_key_github_rejects_is_replaced_and_the_rotation_resumed() {
    let stub = GithubStub::start();
    let kestrel = Kestrel::boot().await;
    signed(&kestrel, &stub).await;
    stub.script_answer("PATCH", HOOK, ScriptedResponse::answering(401));

    let refused = kestrel
        .rotate_webhook_secret("acme", "github", None)
        .await
        .expect_err("github rejected the App's key");

    let Reason::StateConflict { state, next, .. } = reason(&refused) else {
        panic!("{refused:#}");
    };
    assert_eq!(*state, "credentials_rejected");
    assert_eq!(
        next.then,
        [
            Step::ReplacePrivateKey {
                integration: "github".to_owned()
            },
            Step::ResumeWebhookSecretRotation {
                integration: "github".to_owned()
            },
        ]
    );
    assert_eq!(
        kestrel
            .integration("acme", "github")
            .await
            .rotation
            .expect("unfinished")
            .failure,
        Some(RotationFailure::CredentialsRejected)
    );
    kestrel
        .replace_private_key("acme", "github", support::REPLACEMENT_PRIVATE_KEY, None)
        .await
        .expect("a key is replaced while the secret rotates");
    let resumed = kestrel
        .resume_webhook_secret_rotation("acme", "github")
        .await
        .expect("the rotation resumes with the replaced key");
    assert!(resumed.rotation.is_none());

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_rotation_decided_against_an_older_revision_asks_github_nothing() {
    let stub = GithubStub::start();
    let kestrel = Kestrel::boot().await;
    let before = signed(&kestrel, &stub).await;
    kestrel.disable_integration("acme", "github").await;

    let refused = kestrel
        .rotate_webhook_secret("acme", "github", Some(before.revision))
        .await
        .expect_err("the integration changed since");

    let Reason::StateConflict { state, .. } = reason(&refused) else {
        panic!("{refused:#}");
    };
    assert_eq!(*state, "revised");
    assert!(set_secrets(&stub).is_empty());

    kestrel.teardown().await;
}

#[tokio::test]
async fn only_a_github_integration_holding_a_webhook_secret_rotates_one() {
    let stub = GithubStub::start();
    let kestrel = Kestrel::boot().await;
    kestrel.declare_organization("acme").await;
    kestrel
        .register_webhook("acme", "ci", "a-shared-secret")
        .await;
    kestrel
        .register_integration(
            "acme",
            "github",
            REPOSITORY,
            &stub.base_url(),
            &[kestrel::domain::Direction::Inbound],
            jiff::SignedDuration::from_hours(1),
        )
        .await;

    for (name, constraint) in [("ci", "github_app"), ("github", "signing_secret")] {
        let refused = kestrel
            .rotate_webhook_secret("acme", name, None)
            .await
            .expect_err("nothing to rotate");
        let Reason::Forbidden {
            constraint: refused_for,
            ..
        } = reason(&refused)
        else {
            panic!("{refused:#}");
        };
        assert_eq!(*refused_for, constraint);
    }
    assert!(set_secrets(&stub).is_empty());

    kestrel.teardown().await;
}
