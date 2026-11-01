//! Transcript payloads above 64 KiB stored as references, and the generated events a read decodes
//! as.

use crate::support;

use reqwest::StatusCode;
use serde_json::{Value, json};
use support::Kestrel;
use support::operator_requests::{
    PAST_RETENTION, an_open_workspace, payload_entries, retention_stream, transcript_of,
};

#[tokio::test]
async fn transcript_payloads_above_64_kib_are_references_fetched_byte_for_byte() {
    let kestrel = Kestrel::boot().await;
    let (workspace, session) = an_open_workspace(&kestrel, 0).await;
    let inline = "é".repeat(32768);
    let large = format!("{inline}!");
    kestrel.said(&session, &inline).await;
    kestrel.said(&session, &large).await;
    let entries = payload_entries(&kestrel, &workspace).await;
    let said: Vec<_> = entries
        .iter()
        .filter(|record| {
            record["entry"]["type"] == "said" && record["entry"]["participant"] == "builder"
        })
        .collect();
    assert_eq!(said[0]["entry"]["message"], inline);
    assert_eq!(said[1]["entry"]["payload_fields"], json!(["message"]));
    let reference = &said[1]["entry"]["message"];
    assert_eq!(reference["bytes"], 65537);
    assert_eq!(reference["media_type"], "text/plain; charset=utf-8");
    let payload = reference["payload_id"]
        .as_str()
        .expect("a payload reference");
    let response = reqwest::Client::new()
        .get(format!(
            "{}{}/payloads/{payload}",
            kestrel.operator(),
            transcript_of("acme", &workspace)
        ))
        .send()
        .await
        .expect("the payload should answer");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.bytes().await.expect("payload bytes").as_ref(),
        large.as_bytes()
    );
    kestrel.teardown().await;
}

#[tokio::test]
async fn transcript_payloads_cannot_be_fetched_through_another_workspace() {
    let kestrel = Kestrel::boot().await;
    let (workspace, session) = an_open_workspace(&kestrel, 0).await;
    kestrel.said(&session, &"x".repeat(65537)).await;
    let entries = payload_entries(&kestrel, &workspace).await;
    let payload = entries
        .iter()
        .find_map(|record| record["entry"]["message"]["payload_id"].as_str())
        .expect("a reference");
    let other = kestrel.open_workspace("acme", "kestrel", "builder").await;
    let response = reqwest::Client::new()
        .get(format!(
            "{}{}/payloads/{payload}",
            kestrel.operator(),
            transcript_of("acme", &other.id.to_string())
        ))
        .send()
        .await
        .expect("the operator should refuse");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    kestrel.teardown().await;
}

#[tokio::test]
async fn transcript_payloads_and_their_entry_roll_back_when_the_payload_write_fails() {
    let kestrel = Kestrel::boot().await;
    let (workspace, session) = an_open_workspace(&kestrel, 0).await;
    let on = kestrel.on_the_link(&session).await;
    let link = support::link_client::Link::to(&kestrel.link());
    let started = link
        .report_body(
            &on.instance,
            Some(&on.credential),
            &json!({"session": session.id, "seq": 1, "kind": "started"}),
        )
        .await;
    assert_eq!(started.status(), StatusCode::ACCEPTED);
    let before = payload_entries(&kestrel, &workspace).await;
    kestrel.database().refuse_payload_writes().await;
    let response = link.report_body(&on.instance, Some(&on.credential), &json!({"session":session.id, "seq":2, "kind":"said", "message":"x".repeat(65537), "completion":{"started_at":"2026-09-29T12:00:00Z", "finished_at":"2026-09-29T12:01:00Z", "turn_outcome":null}})).await;
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(payload_entries(&kestrel, &workspace).await, before);
    let next = before.last().unwrap()["seq"].as_i64().unwrap() + 1;
    let reference = format!("{workspace}:{next}:message");
    let response = reqwest::Client::new()
        .get(format!(
            "{}{}/payloads/{reference}",
            kestrel.operator(),
            transcript_of("acme", &workspace)
        ))
        .send()
        .await
        .expect("the payload lookup should answer");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    kestrel.teardown().await;
}

#[tokio::test]
async fn transcript_payloads_keep_json_plans_and_completion_metadata() {
    let kestrel = Kestrel::stepped().await;
    let (workspace, session) = an_open_workspace(&kestrel, 0).await;
    let on = kestrel.on_the_link(&session).await;
    let link = support::link_client::Link::to(&kestrel.link());
    let entries = json!([{"content": "🦅".repeat(16384), "priority": "high", "status": "pending"}]);
    let completion = json!({"started_at":"2026-09-29T12:00:00Z", "finished_at":"2026-09-29T12:01:00Z", "turn_outcome":null});
    for report in [
        json!({"session":session.id, "seq":1, "kind":"started"}),
        json!({"session":session.id, "seq":2, "kind":"plan", "entries":entries, "completion":completion}),
    ] {
        let response = link
            .report_body(&on.instance, Some(&on.credential), &report)
            .await;
        assert_eq!(response.status(), StatusCode::ACCEPTED);
    }
    let records = payload_entries(&kestrel, &workspace).await;
    let plan = records
        .iter()
        .find(|record| record["entry"]["type"] == "plan")
        .expect("a plan");
    assert_eq!(plan["entry"]["completion"], completion);
    assert_eq!(plan["session_id"], session.id.to_string());
    assert_eq!(plan["kind"], "narration");
    let payload = plan["entry"]["entries"]["payload_id"]
        .as_str()
        .expect("a reference");
    let response = reqwest::Client::new()
        .get(format!(
            "{}{}/payloads/{payload}",
            kestrel.operator(),
            transcript_of("acme", &workspace)
        ))
        .send()
        .await
        .expect("payload response");
    assert_eq!(response.headers()["content-type"], "application/json");
    assert_eq!(
        response.bytes().await.unwrap().as_ref(),
        serde_json::to_vec(&entries).unwrap()
    );
    kestrel.away_for(PAST_RETENTION);
    kestrel
        .retain_transcript()
        .await
        .expect("retention should run");
    let response = reqwest::Client::new()
        .get(format!(
            "{}{}/payloads/{payload}",
            kestrel.operator(),
            transcript_of("acme", &workspace)
        ))
        .send()
        .await
        .expect("expired payload response");
    assert_eq!(response.status(), StatusCode::GONE);
    let refusal: Value = response.json().await.expect("a JSON refusal");
    assert_eq!(refusal["kind"], "expired_resource");
    assert_eq!(refusal["context"]["resource"], "transcript_payload");
    assert_eq!(refusal["context"]["reference"], payload);
    assert_eq!(refusal["context"]["operation"], "transcript_payload");
    assert_eq!(refusal["next_steps"][0]["action"], "inspect_resource");
    assert_eq!(refusal["next_steps"][0]["resource"], "workspace");
    kestrel.teardown().await;
}

#[tokio::test]
async fn transcript_payloads_are_hydrated_for_the_supervisor_context() {
    let kestrel = Kestrel::boot().await;
    let (workspace, session) = an_open_workspace(&kestrel, 0).await;
    let on = kestrel.on_the_link(&session).await;
    let message = "x".repeat(65537);
    kestrel.said(&session, &message).await;
    let response = support::link_client::Link::to(&kestrel.link())
        .entries(&on.instance, Some(&on.credential), None, None)
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let read: Value = response.json().await.expect("supervisor context");
    assert!(
        read["entries"]
            .as_array()
            .unwrap()
            .iter()
            .any(|record| record["entry"]["message"] == message)
    );
    assert!(
        payload_entries(&kestrel, &workspace)
            .await
            .iter()
            .any(|record| record["entry"]["message"]["payload_id"].is_string())
    );
    kestrel.teardown().await;
}

#[tokio::test]
async fn a_transcript_read_decodes_as_the_generated_events_under_their_documented_names() {
    use kestrel_operator_types as wire;

    let kestrel = Kestrel::boot().await;
    let (workspace, session) = an_open_workspace(&kestrel, 1).await;
    kestrel.said(&session, &"x".repeat(65537)).await;

    let read = retention_stream(&kestrel, &workspace, "kinds=shared_state,narration,detail").await;
    let mut names = Vec::new();
    let mut references = 0;
    for frame in read.split("\n\n").filter(|frame| frame.contains("data:")) {
        let field = |field: &str| {
            frame
                .lines()
                .find_map(|line| line.strip_prefix(field))
                .map(str::trim)
        };
        let (name, data) = (
            field("event:").expect("a name"),
            field("data:").expect("data"),
        );
        let event = wire::Event::from_sse(name, data)
            .unwrap_or_else(|| panic!("the document names no {name} event"))
            .unwrap_or_else(|error| panic!("{data} is no {name} event: {error}"));
        assert_eq!(event.event_name(), name);
        assert_eq!(
            serde_json::to_value(&event).expect("an event"),
            serde_json::from_str::<Value>(data).expect("json"),
            "the generated {name} event drops what the boundary serves"
        );
        if let wire::Event::Recorded(recorded) = &event {
            assert_eq!(
                field("id:"),
                Some(format!("{workspace}:{}", recorded.seq).as_str())
            );
            if let wire::Entry::Message(said) = &recorded.entry
                && let wire::MessageContent::PayloadReference(reference) = &said.message
            {
                assert_eq!(reference.bytes, 65537);
                assert_eq!(said.payload_fields, Some(vec!["message".to_owned()]));
                references += 1;
            }
        }
        names.push(name);
    }

    assert!(names.contains(&"entry"), "{names:?}");
    assert_eq!(names.last(), Some(&"end"), "{names:?}");
    assert_eq!(
        references, 1,
        "the oversized message was not read as a reference"
    );
    kestrel.teardown().await;
}
