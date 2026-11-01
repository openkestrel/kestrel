//! Registering, maintaining and retiring Integrations without saying their secrets.

use crate::support;

use jiff::SignedDuration;
use kestrel::domain::Direction;
use kestrel::operator;
use reqwest::StatusCode;
use serde_json::{Value, json};
use support::github_stub::GithubStub;
use support::operator_requests::{
    client, declared, got, integration_of, integrations_of, listed, recorded, requested,
};
use support::{Kestrel, PRIVATE_KEY};

#[tokio::test]
async fn a_client_registers_and_lists_integrations_without_saying_their_secrets() {
    let kestrel = Kestrel::boot().await;
    let stub = GithubStub::start();
    kestrel.declare_organization("acme").await;

    let github = client(
        &kestrel,
        &[
            "integration",
            "github",
            "register",
            "hub",
            "--organization",
            "acme",
            "--repository",
            "jtmthf/kestrel",
            "--app-id",
            "1",
            "--installation",
            "2",
            "--private-key",
            PRIVATE_KEY,
            "--api",
            &stub.base_url(),
            "--interval",
            "5m",
            "--json",
        ],
    )
    .await;
    let webhook = client(
        &kestrel,
        &[
            "integration",
            "register",
            "webhook",
            "ci",
            "--organization",
            "acme",
            "--secret",
            "a-shared-secret",
            "--json",
        ],
    )
    .await;
    let listed = client(
        &kestrel,
        &["integration", "list", "--organization", "acme", "--json"],
    )
    .await;

    let github = recorded(&github);
    let webhook = recorded(&webhook);
    let records = recorded(&listed);
    assert_eq!(records, [webhook.clone(), github.clone()].concat());
    assert_eq!(github[0]["kind"], "github");
    assert_eq!(github[0]["repository"], "jtmthf/kestrel");
    assert_eq!(github[0]["carries"], json!(["inbound", "outbound"]));
    assert_eq!(github[0]["polled_every"], "5m");
    assert_eq!(
        github[0]["webhook_path"],
        format!("/webhooks/{}", github[0]["id"].as_str().expect("an id"))
    );
    assert_eq!(webhook[0]["kind"], "webhook");
    assert_eq!(webhook[0]["carries"], json!(["inbound"]));
    assert_eq!(
        webhook[0]["webhook_path"],
        format!("/webhooks/{}", webhook[0]["id"].as_str().expect("an id"))
    );
    assert_eq!(webhook[0]["last_event_refusal"], Value::Null);
    let said = listed.out.join("\n");
    assert!(
        !said.contains(PRIVATE_KEY),
        "the listing spelled the private key out"
    );
    assert!(
        !said.contains("a-shared-secret"),
        "the listing spelled the webhook secret out"
    );
    assert_eq!(
        kestrel.integrations("acme").await[0].id.to_string(),
        webhook[0]["id"].as_str().expect("an id")
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn an_integration_is_registered_with_what_it_is_declared_to_carry() {
    let kestrel = Kestrel::boot().await;
    let stub = GithubStub::start();
    kestrel.declare_organization("acme").await;

    let (status, signed) = declared(
        &kestrel,
        &integrations_of("acme"),
        &json!({
            "kind": "github",
            "name": "hub",
            "repository": "jtmthf/kestrel",
            "app_id": 1,
            "installation": 2,
            "private_key": PRIVATE_KEY,
            "api": stub.base_url(),
            "carries": ["outbound"],
            "webhook_secret": "a-signing-secret",
        }),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED, "{signed}");
    assert_eq!(signed["carries"], json!(["outbound"]));
    assert_eq!(signed["polled_every"], Value::Null);
    assert_eq!(signed["webhook_path"], Value::Null, "{signed}");
    assert!(!signed.to_string().contains("a-signing-secret"));
    assert!(!signed.to_string().contains(PRIVATE_KEY));

    kestrel.teardown().await;
}

/// What a registration leaves out and what it nulls read alike, and a named value still stands:
/// the union's optional connection fields carry absent and null the same way.
#[tokio::test]
async fn a_registration_reads_an_omitted_connection_field_as_one_it_nulls() {
    let kestrel = Kestrel::boot().await;
    let stub = GithubStub::start();
    kestrel.declare_organization("acme").await;

    let github = async |name: &str, fields: Value| {
        let mut registration = json!({
            "kind": "github",
            "name": name,
            "repository": "jtmthf/kestrel",
            "app_id": 1,
            "installation": 2,
            "private_key": PRIVATE_KEY,
            "api": stub.base_url(),
        });
        registration
            .as_object_mut()
            .expect("an object")
            .extend(fields.as_object().expect("an object").clone());
        declared(&kestrel, &integrations_of("acme"), &registration).await
    };
    let webhook = async |name: &str, fields: Value| {
        let mut registration = json!({
            "kind": "webhook",
            "name": name,
            "secret": "a-shared-secret",
        });
        registration
            .as_object_mut()
            .expect("an object")
            .extend(fields.as_object().expect("an object").clone());
        declared(&kestrel, &integrations_of("acme"), &registration).await
    };

    let (omitted, absent) = github("hub", json!({})).await;
    let (nulled, null) = github(
        "hub-null",
        json!({ "carries": null, "interval": null, "webhook_secret": null }),
    )
    .await;
    let (named, given) = github("hub-polled", json!({ "interval": "5m" })).await;

    assert_eq!(omitted, StatusCode::CREATED, "{absent}");
    assert_eq!(nulled, StatusCode::CREATED, "{null}");
    assert_eq!(named, StatusCode::CREATED, "{given}");
    for answered in [&absent, &null] {
        assert_eq!(
            answered["carries"],
            json!(["inbound", "outbound"]),
            "{answered}"
        );
        assert_eq!(answered["polled_every"], "1m", "{answered}");
    }
    assert_eq!(given["carries"], json!(["inbound", "outbound"]), "{given}");
    assert_eq!(given["polled_every"], "5m", "{given}");

    let (omitted, absent) = webhook("ci", json!({})).await;
    let (nulled, null) = webhook("cd", json!({ "carries": null })).await;
    assert_eq!(omitted, StatusCode::CREATED, "{absent}");
    assert_eq!(nulled, StatusCode::CREATED, "{null}");
    assert_eq!(absent["carries"], json!(["inbound"]), "{absent}");
    assert_eq!(null["carries"], json!(["inbound"]), "{null}");

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_client_maintains_an_integration_without_changing_what_it_is() {
    let kestrel = Kestrel::boot().await;
    let stub = GithubStub::start();
    kestrel.declare_organization("acme").await;
    kestrel
        .register_integration(
            "acme",
            "hub",
            "jtmthf/kestrel",
            &stub.base_url(),
            &[Direction::Inbound, Direction::Outbound],
            SignedDuration::from_mins(1),
        )
        .await;
    let (status, shown) = got(&kestrel, &integration_of("acme", "hub")).await;
    assert_eq!(status, StatusCode::OK, "{shown}");
    assert_eq!(shown["state"], "enabled");
    assert_eq!(shown["diagnostic"], Value::Null);
    let revision = shown["revision"].as_i64().expect("a revision");

    let (status, refused) = requested(
        &kestrel,
        reqwest::Method::PATCH,
        &integration_of("acme", "hub"),
        Some(&json!({ "repository": "jtmthf/other" })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{refused}");
    assert_eq!(refused["context"]["field"], "repository");
    assert_eq!(refused["context"]["constraint"], "immutable");

    let (status, refused) = requested(
        &kestrel,
        reqwest::Method::PATCH,
        &integration_of("acme", "hub"),
        Some(&json!({ "interval": "0s" })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{refused}");
    assert_eq!(refused["context"]["field"], "interval");

    let (status, changed) = requested(
        &kestrel,
        reqwest::Method::PATCH,
        &integration_of("acme", "hub"),
        Some(&json!({ "name": "github", "carries": ["outbound"], "revision": revision })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{changed}");
    assert_eq!(changed["name"], "github");
    assert_eq!(changed["carries"], json!(["outbound"]));
    assert_eq!(changed["repository"], "jtmthf/kestrel");
    assert_eq!(changed["id"], shown["id"]);

    let (status, stale) = requested(
        &kestrel,
        reqwest::Method::PATCH,
        &integration_of("acme", "github"),
        Some(&json!({ "name": "hub", "revision": revision })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{stale}");

    let (status, disabled) = declared(
        &kestrel,
        &operator::INTEGRATION_DISABLE
            .replace("{organization}", "acme")
            .replace("{integration}", "github"),
        &json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{disabled}");
    assert_eq!(disabled["state"], "disabled");
    assert!(disabled["disabled_at"].is_string());
    assert_eq!(disabled["diagnostic"]["kind"], "state_conflict");
    assert!(
        disabled["diagnostic"]["next_steps"]
            .as_array()
            .expect("steps")
            .iter()
            .any(|step| step["action"] == "enable_integration"
                && step["integration"] == "github"
                && step["organization"] == "acme"),
        "{disabled}"
    );
    assert_eq!(
        listed(&kestrel, &integrations_of("acme")).await[0]["state"],
        "disabled"
    );

    let (status, enabled) = declared(
        &kestrel,
        &operator::INTEGRATION_ENABLE
            .replace("{organization}", "acme")
            .replace("{integration}", "github"),
        &json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{enabled}");
    assert_eq!(enabled["state"], "enabled");
    assert_eq!(enabled["diagnostic"], Value::Null);
    assert!(enabled["revision"].as_i64() > disabled["revision"].as_i64());
    assert!(!enabled.to_string().contains(PRIVATE_KEY));

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_client_retires_an_integration_and_still_reads_it() {
    let kestrel = Kestrel::boot().await;
    let stub = GithubStub::start();
    kestrel.declare_organization("acme").await;
    kestrel
        .register_integration(
            "acme",
            "github",
            "jtmthf/kestrel",
            &stub.base_url(),
            &[Direction::Inbound, Direction::Outbound],
            SignedDuration::from_mins(1),
        )
        .await;
    let retire = operator::INTEGRATION_RETIRE
        .replace("{organization}", "acme")
        .replace("{integration}", "github");
    let (_, shown) = got(&kestrel, &integration_of("acme", "github")).await;
    let revision = shown["revision"].as_i64().expect("a revision");

    let (status, stale) = declared(&kestrel, &retire, &json!({ "revision": revision - 1 })).await;
    assert_eq!(status, StatusCode::CONFLICT, "{stale}");
    let (status, unchosen) = requested(&kestrel, reqwest::Method::POST, &retire, None).await;
    assert!(status.is_client_error(), "{unchosen}");
    let (_, kept) = got(&kestrel, &integration_of("acme", "github")).await;
    assert_eq!(kept["state"], "enabled");

    let (status, retired) = declared(&kestrel, &retire, &json!({ "revision": revision })).await;
    assert_eq!(status, StatusCode::OK, "{retired}");
    assert_eq!(retired["state"], "retired");
    assert_eq!(retired["id"], shown["id"]);
    assert_eq!(retired["repository"], "jtmthf/kestrel");
    assert!(retired["retired_at"].is_string());
    assert_eq!(retired["canceled_posts"], json!([]));
    assert_eq!(retired["polled_every"], Value::Null);
    assert_eq!(retired["webhook_path"], Value::Null);
    assert_eq!(retired["diagnostic"]["kind"], "state_conflict");
    assert_eq!(retired["diagnostic"]["context"]["state"], "retired");
    assert_eq!(
        retired["diagnostic"]["context"]["organization"], "acme",
        "{retired}"
    );
    assert!(
        retired["diagnostic"]["next_steps"]
            .as_array()
            .expect("steps")
            .iter()
            .all(|step| step["action"] != "enable_integration"),
        "{retired}"
    );
    assert!(!retired.to_string().contains(PRIVATE_KEY));

    for path in [operator::INTEGRATION_ENABLE, operator::INTEGRATION_DISABLE] {
        let (status, refused) = declared(
            &kestrel,
            &path
                .replace("{organization}", "acme")
                .replace("{integration}", "github"),
            &json!({}),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT, "{refused}");
        assert_eq!(refused["context"]["state"], "retired");
    }
    let (status, refused) = requested(
        &kestrel,
        reqwest::Method::PATCH,
        &integration_of("acme", "github"),
        Some(&json!({ "name": "hub" })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{refused}");

    let (status, again) = declared(&kestrel, &retire, &json!({})).await;
    assert_eq!(status, StatusCode::OK, "{again}");
    assert_eq!(again["revision"], retired["revision"]);
    assert_eq!(
        listed(&kestrel, &integrations_of("acme")).await[0]["state"],
        "retired"
    );

    kestrel.teardown().await;
}
