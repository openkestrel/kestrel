//! Listing, showing and acknowledging an Organization's Events.

use crate::support;

use kestrel::domain::EventRecordId;
use kestrel::operator;
use reqwest::StatusCode;
use serde_json::{Value, json};
use support::Kestrel;
use support::operator_requests::{
    client, failed, got, integrations_of, listed, recorded, requested, succeeded,
};

fn event_refusal_of(organization: &str, integration: &str) -> String {
    operator::EVENT_REFUSAL
        .replace("{organization}", organization)
        .replace("{integration}", integration)
}

fn events_of(organization: &str) -> String {
    operator::EVENTS.replace("{organization}", organization)
}

fn event_at(record: &str) -> String {
    operator::EVENT.replace("{record}", record)
}

#[tokio::test]
async fn a_client_acknowledges_the_event_an_integration_refused() {
    let kestrel = Kestrel::boot().await;
    kestrel.declare_organization("acme").await;
    let webhook = kestrel
        .register_webhook("acme", "ci", "a-shared-secret")
        .await;
    reqwest::Client::new()
        .post(format!("{}{}", kestrel.link(), webhook.webhook_path()))
        .bearer_auth("a-shared-secret")
        .header("content-type", "text/plain")
        .body("x".repeat(1024 * 1024 + 1))
        .send()
        .await
        .expect("the webhook answers");

    let refused = recorded(
        &client(
            &kestrel,
            &["integration", "list", "--organization", "acme", "--json"],
        )
        .await,
    );
    let acknowledged = client(
        &kestrel,
        &[
            "integration",
            "acknowledge-refusal",
            "ci",
            "--organization",
            "acme",
        ],
    )
    .await;
    let (status, _) = requested(
        &kestrel,
        reqwest::Method::DELETE,
        &event_refusal_of("acme", "ci"),
        None,
    )
    .await;
    let (unknown, _) = requested(
        &kestrel,
        reqwest::Method::DELETE,
        &event_refusal_of("acme", "pager"),
        None,
    )
    .await;

    let refusal = &refused[0]["last_event_refusal"];
    assert!(
        refusal["bytes"].as_u64().expect("a size") > 1024 * 1024,
        "{refusal}"
    );
    assert!(refusal["reason"].is_string(), "{refusal}");
    assert!(succeeded(&acknowledged).is_empty());
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(unknown, StatusCode::NOT_FOUND);
    assert_eq!(
        listed(&kestrel, &integrations_of("acme")).await[0]["last_event_refusal"],
        Value::Null
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_client_lists_an_organizations_events_and_shows_one_whole() {
    let kestrel = Kestrel::boot().await;
    kestrel.declare_organization("acme").await;
    let webhook = kestrel
        .register_webhook("acme", "ci", "a-shared-secret")
        .await;
    for id in ["deploy-1", "deploy-2"] {
        let answered = reqwest::Client::new()
            .post(format!("{}{}", kestrel.link(), webhook.webhook_path()))
            .bearer_auth("a-shared-secret")
            .header("content-type", "application/cloudevents+json")
            .body(
                json!({
                    "specversion": "1.0",
                    "id": id,
                    "source": "/argo/sensors/deploy",
                    "type": "io.argoproj.deployed",
                    "subject": "kestrel",
                    "data": { "image": "kestrel:1" },
                })
                .to_string(),
            )
            .send()
            .await
            .expect("the webhook answers");
        assert_eq!(answered.status(), StatusCode::ACCEPTED);
    }

    let events = recorded(
        &client(
            &kestrel,
            &["event", "list", "--organization", "acme", "--json"],
        )
        .await,
    );
    let limited = recorded(
        &client(
            &kestrel,
            &[
                "event",
                "list",
                "--organization",
                "acme",
                "--limit",
                "1",
                "--json",
            ],
        )
        .await,
    );
    let record = events[0]["record"].as_str().expect("a record id");
    let shown = recorded(&client(&kestrel, &["event", "show", record, "--json"]).await);

    assert_eq!(events.len(), 2);
    assert_eq!(limited, events[..1]);
    assert_eq!(shown, events[..1]);
    assert_eq!(shown[0]["integration"], webhook.id.to_string());
    assert_eq!(shown[0]["event"]["source"], "/argo/sensors/deploy");
    assert_eq!(shown[0]["event"]["type"], "io.argoproj.deployed");
    assert_eq!(shown[0]["event"]["specversion"], "1.0");
    assert_eq!(shown[0]["event"]["subject"], "kestrel");
    assert_eq!(shown[0]["event"]["data"], json!({ "image": "kestrel:1" }));
    assert_eq!(
        kestrel.events("acme").await[0].record_id.to_string(),
        record
    );
    let (_, over_the_boundary) = got(&kestrel, &format!("{}?limit=1", events_of("acme"))).await;
    assert_eq!(over_the_boundary.as_array().map(Vec::len), Some(1));
    assert_eq!(over_the_boundary[0]["record"], limited[0]["record"]);

    kestrel.teardown().await;
}

#[tokio::test]
async fn an_event_nobody_recorded_is_refused() {
    let kestrel = Kestrel::boot().await;

    let (unrecorded, refusal) =
        got(&kestrel, &event_at(&EventRecordId::generate().to_string())).await;
    let (malformed, _) = got(&kestrel, &event_at("yesterday")).await;
    let (nowhere, _) = got(&kestrel, &events_of("acme")).await;
    let showing = client(&kestrel, &["event", "show", "yesterday"]).await;

    assert_eq!(unrecorded, StatusCode::NOT_FOUND);
    assert!(
        refusal["message"]
            .as_str()
            .expect("a message")
            .starts_with("no event"),
        "{refusal}"
    );
    assert_eq!(malformed, StatusCode::NOT_FOUND);
    assert_eq!(nowhere, StatusCode::NOT_FOUND);
    assert!(failed(&showing).contains("no event yesterday"));

    kestrel.teardown().await;
}
