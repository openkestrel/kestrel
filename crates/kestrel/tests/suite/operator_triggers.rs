//! A Client managing Triggers, and the answers and refusals the operator document gives them.

use crate::support;

use kestrel::domain::EventRecordId;
use kestrel::operator;
use reqwest::StatusCode;
use serde_json::{Value, json};
use support::Kestrel;
use support::client;
use support::fixture::Fixture;
use support::operator_requests::{
    client, declared, failed, got, listed_nothing, recorded, succeeded, triggers_of,
};

fn trigger_at(organization: &str, trigger: &str) -> String {
    operator::TRIGGER
        .replace("{organization}", organization)
        .replace("{trigger}", trigger)
}

#[tokio::test]
async fn a_client_manages_triggers_without_opening_a_database() {
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
    let webhook = kestrel
        .register_webhook("acme", "events", "a-shared-secret")
        .await;
    let response = reqwest::Client::new()
        .post(format!("{}{}", kestrel.link(), webhook.webhook_path()))
        .bearer_auth("a-shared-secret")
        .header("content-type", "application/cloudevents+json")
        .body(
            json!({
                "id": "retained",
                "source": "urn:test",
                "specversion": "1.0",
                "type": "example",
                "time": "2026-09-19T00:00:00Z",
            })
            .to_string(),
        )
        .send()
        .await
        .expect("the webhook should answer");
    assert!(response.status().is_success());
    let retained = kestrel.events("acme").await[0].record_id.to_string();

    let declaration = [
        "trigger",
        "declare",
        "ready",
        "--organization",
        "acme",
        "--filter",
        r#"{"exact":{"type":"example"}}"#,
        "--brief",
        "Work {{ event.type }}",
        "--project",
        "kestrel",
        "--agent",
        "builder",
        "--json",
    ];
    let declared = recorded(&client(&kestrel, &declaration).await);
    let trigger = declared[0]["id"].as_str().expect("a trigger id").to_owned();
    assert_eq!(declared[0]["name"], "ready");
    assert_eq!(declared[0]["state"], "enabled");

    let listed = recorded(
        &client(
            &kestrel,
            &["trigger", "list", "--organization", "acme", "--json"],
        )
        .await,
    );
    assert_eq!(listed, declared);
    assert_eq!(
        recorded(
            &client(
                &kestrel,
                &[
                    "trigger",
                    "show",
                    "ready",
                    "--organization",
                    "acme",
                    "--json"
                ]
            )
            .await
        ),
        declared
    );
    assert_eq!(
        recorded(&client(&kestrel, &declaration).await)[0]["id"],
        trigger
    );

    let changed = recorded(
        &client(
            &kestrel,
            &[
                "trigger",
                "declare",
                "ready",
                "--organization",
                "acme",
                "--filter",
                r#"{"exact":{"type":"example"}}"#,
                "--brief",
                "Triage {{ event.type }}",
                "--project",
                "kestrel",
                "--agent",
                "builder",
                "--json",
            ],
        )
        .await,
    );
    assert_eq!(changed[0]["id"], trigger);
    assert_eq!(changed[0]["brief"], "Triage {{ event.type }}");
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    assert!(kestrel.workspaces("acme").await.is_empty());

    let test = vec![
        "trigger",
        "test",
        "ready",
        "--organization",
        "acme",
        "--event",
        &retained,
        "--json",
    ];
    let tested = recorded(&client(&kestrel, &test).await);
    assert_eq!(tested[0]["matches"], true);

    let disabled = recorded(
        &client(
            &kestrel,
            &[
                "trigger",
                "disable",
                "ready",
                "--organization",
                "acme",
                "--json",
            ],
        )
        .await,
    );
    assert_eq!(disabled[0]["state"], "disabled:operator");
    let enabled = recorded(
        &client(
            &kestrel,
            &[
                "trigger",
                "enable",
                "ready",
                "--organization",
                "acme",
                "--json",
            ],
        )
        .await,
    );
    assert_eq!(enabled[0]["state"], "enabled");

    kestrel.teardown().await;
}

#[tokio::test]
async fn the_operator_documents_trigger_answers_and_refusals() {
    let kestrel = Kestrel::boot().await;
    Fixture::acme().declare(&kestrel).await;
    let triggers = triggers_of("acme");
    let declaration = json!({
        "name": "sweep",
        "every": "1h",
        "brief": "Sweep {{ event.data.trigger }}",
        "project": "kestrel",
        "agent": "builder",
    });

    assert!(listed_nothing(&kestrel, &triggers).await);
    let (status, trigger) = declared(&kestrel, &triggers, &declaration).await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, repeated) = declared(&kestrel, &triggers, &declaration).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(repeated["id"], trigger["id"]);
    let path = trigger_at("acme", "sweep");
    let (status, _) = got(&kestrel, &path).await;
    assert_eq!(status, StatusCode::OK);
    let (status, tested) = declared(&kestrel, &format!("{path}/test"), &json!({})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(tested["matches"], true);
    for misnamed in [
        json!({ "issue": 60 }),
        json!({ "integration": "github" }),
        json!({ "issue": 60, "integration": "github", "event": EventRecordId::generate().to_string() }),
    ] {
        let (status, _) = declared(&kestrel, &format!("{path}/test"), &misnamed).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{misnamed}");
    }
    let (status, _) = declared(&kestrel, &format!("{path}/disable"), &json!({})).await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = declared(&kestrel, &format!("{path}/enable"), &json!({})).await;
    assert_eq!(status, StatusCode::OK);

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_trigger_declared_on_a_cron_prints_its_expression_and_zone() {
    let kestrel = Kestrel::boot().await;
    Fixture::acme().declare(&kestrel).await;
    let fields = ["name", "every", "cron", "zone", "filter"];
    let declare = |extra: &'static [&'static str]| {
        let mut args = vec![
            "trigger",
            "declare",
            "triage",
            "--organization",
            "acme",
            "--brief",
            "Triage",
            "--project",
            "kestrel",
            "--agent",
            "builder",
            "--json",
        ];
        args.extend_from_slice(extra);
        args
    };

    let triage = recorded(
        &client(
            &kestrel,
            &declare(&["--cron", "0 9 * * 1-5", "--zone", "America/New_York"]),
        )
        .await,
    );
    let triage: Vec<Value> = triage
        .iter()
        .map(|one| client::picked(one, &fields))
        .collect();
    assert_eq!(
        triage,
        [json!({
            "name": "triage",
            "every": null,
            "cron": "0 9 * * 1-5",
            "zone": "America/New_York",
            "filter": null,
        })]
    );
    let listed = recorded(
        &client(
            &kestrel,
            &["trigger", "list", "--organization", "acme", "--json"],
        )
        .await,
    );
    let listed: Vec<Value> = listed
        .iter()
        .map(|one| client::picked(one, &fields))
        .collect();
    assert_eq!(listed, triage);
    let shown = recorded(
        &client(
            &kestrel,
            &[
                "trigger",
                "show",
                "triage",
                "--organization",
                "acme",
                "--json",
            ],
        )
        .await,
    );
    assert_eq!(client::picked(&shown[0], &fields), triage[0]);

    for refused in [
        declare(&["--cron", "0 9 * * *", "--zone", "UTC", "--every", "1h"]),
        declare(&[
            "--cron",
            "0 9 * * *",
            "--zone",
            "UTC",
            "--filter",
            r#"{"exact":{"type":"x"}}"#,
        ]),
        declare(&["--cron", "0 9 * * *"]),
    ] {
        failed(&client(&kestrel, &refused).await);
    }

    kestrel.teardown().await;
}
