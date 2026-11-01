//! Retention expiring a sealed Workspace's narration and detail, in bounded batches, rolling back
//! on failure.

use crate::support;

use std::time::Duration;

use kestrel::log::Entry;
use reqwest::StatusCode;
use serde_json::{Value, json};
use support::Kestrel;
use support::operator_requests::{
    PAST_RETENTION, an_open_workspace, payload_entries, retention_stream, transcript_of,
};

#[tokio::test]
async fn activity_summaries_carry_distinct_tool_kinds_without_expansion() {
    let kestrel = Kestrel::stepped().await;
    let (workspace, session) = an_open_workspace(&kestrel, 0).await;
    let on = kestrel.on_the_link(&session).await;
    let link = support::link_client::Link::to(&kestrel.link());
    let completion = json!({"started_at":"2026-10-01T00:00:00Z", "finished_at":"2026-10-01T00:01:00Z", "turn_outcome":null});
    for (seq, kind) in ["read", "execute", "read", "custom"]
        .into_iter()
        .enumerate()
    {
        let report = json!({"session":session.id,"seq":seq + 1,"kind":"tool_call","call_id":format!("call-{seq}"),"title":"work","tool_kind":kind,"status":"completed","input":{},"result":{},"closing_reason":null,"completion":completion});
        assert_eq!(
            link.report_body(&on.instance, Some(&on.credential), &report)
                .await
                .status(),
            StatusCode::ACCEPTED
        );
    }
    let stream = retention_stream(&kestrel, &workspace, "").await;
    let activity: Value = stream
        .split("event: activity\n")
        .nth(1)
        .unwrap()
        .lines()
        .find_map(|line| line.strip_prefix("data: "))
        .map(|data| serde_json::from_str(data).unwrap())
        .unwrap();
    assert_eq!(activity["tool_kinds"], json!(["read", "execute", "custom"]));
    assert_eq!(activity["counts"]["tool_calls"], 4);
    kestrel.teardown().await;
}

#[tokio::test]
async fn retention_expires_sealed_workspace_content_and_preserves_positions_and_shared_state() {
    let kestrel = Kestrel::stepped().await;
    let (workspace, session) = an_open_workspace(&kestrel, 0).await;
    let on = kestrel.on_the_link(&session).await;
    let link = support::link_client::Link::to(&kestrel.link());
    let completion = json!({"started_at":"2020-01-01T00:00:00Z", "finished_at":"2020-01-01T00:01:00Z", "turn_outcome":null});
    for (seq, mut report) in [
        json!({"kind":"thought","text":"private reasoning","completion":completion}),
        json!({"kind":"tool_call","call_id":"one","title":"private title","tool_kind":"read","status":"failed","input":{},"result":"x".repeat(65537),"closing_reason":"interrupted","completion":completion}),
        json!({"kind":"said","message":"public narrative".repeat(6000),"completion":completion}),
    ].into_iter().enumerate() {
        report["session"] = json!(session.id);
        report["seq"] = json!(seq + 1);
        assert_eq!(link.report_body(&on.instance, Some(&on.credential), &report).await.status(), StatusCode::ACCEPTED);
    }
    kestrel.stop_session(session.id).await;
    kestrel.release_instance(session.workspace).await;
    kestrel.seal_workspace(session.workspace).await;
    kestrel.away_for(PAST_RETENTION);
    let before = payload_entries(&kestrel, &workspace).await;
    let work: Vec<_> = before
        .iter()
        .filter(|r| r["kind"] != "shared_state")
        .collect();
    assert_eq!(work.len(), 2);
    let payload = work[1]["entry"]["result"]["payload_id"].as_str().unwrap();
    assert_eq!(kestrel.retain_transcript().await.unwrap(), 2);
    let after = payload_entries(&kestrel, &workspace).await;
    assert_eq!(after.len(), before.len());
    for (before, after) in before.iter().zip(&after) {
        if before["kind"] == "shared_state" {
            assert_eq!(before, after);
            continue;
        }
        assert_eq!(after["seq"], before["seq"]);
        assert_eq!(after["kind"], before["kind"]);
        assert_eq!(after["appended_at"], before["appended_at"]);
        assert_eq!(after["session_id"], Value::Null);
        assert_eq!(after["entry"].as_object().unwrap().len(), 2);
        assert_eq!(after["entry"]["type"], "expired");
        assert_eq!(
            after["entry"]["expired_at"]
                .as_str()
                .unwrap()
                .parse::<jiff::Timestamp>()
                .unwrap(),
            kestrel.now()
        );
    }
    let detail = retention_stream(&kestrel, &workspace, "kinds=detail&summaries=false").await;
    let detail: Vec<Value> = detail
        .lines()
        .filter_map(|l| l.strip_prefix("data: "))
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter(|r| r.get("entry").is_some())
        .collect();
    assert_eq!(
        detail,
        vec![
            after
                .iter()
                .find(|r| r["kind"] == "detail")
                .unwrap()
                .clone()
        ]
    );
    let default = retention_stream(&kestrel, &workspace, "").await;
    assert!(!default.contains("\"type\":\"expired\""));
    let activity: Value = default
        .split("event: activity\n")
        .nth(1)
        .unwrap()
        .lines()
        .find_map(|l| l.strip_prefix("data: "))
        .map(|l| serde_json::from_str(l).unwrap())
        .unwrap();
    assert_eq!(
        activity["counts"],
        json!({"tool_calls":0,"failed_calls":0,"thoughts":0,"plans":0,"tombstones":2})
    );
    assert_eq!(
        activity["latest"],
        json!({"kind":"detail","title":null,"status":null})
    );
    assert_eq!(activity["started_at"], Value::Null);
    assert_eq!(activity["finished_at"], Value::Null);
    assert_eq!(activity["anomaly"], false);
    let resumed = reqwest::Client::new()
        .get(format!(
            "{}{}?follow=false&kinds=detail&summaries=false",
            kestrel.operator(),
            transcript_of("acme", &workspace)
        ))
        .header("Last-Event-ID", format!("{workspace}:{}", work[0]["seq"]))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(resumed.contains("\"type\":\"expired\""));
    let response = reqwest::Client::new()
        .get(format!(
            "{}{}/payloads/{payload}",
            kestrel.operator(),
            transcript_of("acme", &workspace)
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::GONE);
    let shared_payload = before
        .iter()
        .find_map(|r| r["entry"]["message"]["payload_id"].as_str())
        .unwrap();
    let response = reqwest::Client::new()
        .get(format!(
            "{}{}/payloads/{shared_payload}",
            kestrel.operator(),
            transcript_of("acme", &workspace)
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.text().await.unwrap(),
        "public narrative".repeat(6000)
    );
    assert_eq!(kestrel.retain_transcript().await.unwrap(), 0);
    kestrel.teardown().await;
}

#[tokio::test]
async fn retention_rolls_back_payload_deletion_and_every_tombstone_on_failure() {
    let kestrel = Kestrel::stepped().await;
    let (workspace, session) = an_open_workspace(&kestrel, 0).await;
    let on = kestrel.on_the_link(&session).await;
    let link = support::link_client::Link::to(&kestrel.link());
    let completion = json!({"started_at":"2020-01-01T00:00:00Z", "finished_at":"2020-01-01T00:01:00Z", "turn_outcome":null});
    let content = "x".repeat(65537);
    for (seq, mut report) in [
        json!({"kind":"tool_call","call_id":"one","title":"read","tool_kind":"read","status":"completed","input":{},"result":content,"closing_reason":null,"completion":completion}),
        json!({"kind":"thought","text":content,"completion":completion}),
    ].into_iter().enumerate() {
        report["session"] = json!(session.id);
        report["seq"] = json!(seq + 1);
        assert_eq!(link.report_body(&on.instance, Some(&on.credential), &report).await.status(), StatusCode::ACCEPTED);
    }
    kestrel.away_for(PAST_RETENTION);
    let before = payload_entries(&kestrel, &workspace).await;
    kestrel.database().refuse_retention().await;
    assert!(kestrel.retain_transcript().await.is_err());
    assert_eq!(payload_entries(&kestrel, &workspace).await, before);
    for record in before.iter().filter(|r| r["kind"] != "shared_state") {
        let field = if record["kind"] == "detail" {
            "result"
        } else {
            "text"
        };
        let payload = record["entry"][field]["payload_id"].as_str().unwrap();
        let response = reqwest::Client::new()
            .get(format!(
                "{}{}/payloads/{payload}",
                kestrel.operator(),
                transcript_of("acme", &workspace)
            ))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let expected = if field == "result" {
            serde_json::to_vec(&content).unwrap()
        } else {
            content.as_bytes().to_vec()
        };
        assert_eq!(response.bytes().await.unwrap().as_ref(), expected);
    }
    kestrel.database().allow_retention().await;
    assert_eq!(kestrel.retain_transcript().await.unwrap(), 2);
    assert_eq!(
        payload_entries(&kestrel, &workspace)
            .await
            .iter()
            .filter(|r| r["entry"]["type"] == "expired")
            .count(),
        2
    );
    kestrel.teardown().await;
}

#[tokio::test]
async fn retention_continues_bounded_batches_and_uses_append_time() {
    let kestrel = Kestrel::stepped().await;
    let (workspace, session) = an_open_workspace(&kestrel, 0).await;
    let on = kestrel.on_the_link(&session).await;
    let link = support::link_client::Link::to(&kestrel.link());
    let completion = json!({"started_at":"2020-01-01T00:00:00Z", "finished_at":"2020-01-01T00:01:00Z", "turn_outcome":null});
    for seq in 1..=1001 {
        let report = json!({"session":session.id,"seq":seq,"kind":"thought","text":format!("thought {seq}"),"completion":completion});
        assert_eq!(
            link.report_body(&on.instance, Some(&on.credential), &report)
                .await
                .status(),
            StatusCode::ACCEPTED
        );
    }
    kestrel.away_for(Duration::from_secs(29 * 24 * 60 * 60));
    assert_eq!(kestrel.retain_transcript().await.unwrap(), 0);
    kestrel.away_for(Duration::from_secs(2 * 24 * 60 * 60));
    let fresh = json!({"session":session.id,"seq":1002,"kind":"thought","text":"appended recently despite ancient completion","completion":completion});
    assert_eq!(
        link.report_body(&on.instance, Some(&on.credential), &fresh)
            .await
            .status(),
        StatusCode::ACCEPTED
    );
    assert_eq!(kestrel.retain_transcript().await.unwrap(), 1000);
    let first = payload_entries(&kestrel, &workspace).await;
    assert_eq!(
        first
            .iter()
            .filter(|r| r["entry"]["type"] == "expired")
            .count(),
        1000
    );
    assert_eq!(
        first
            .iter()
            .filter(|r| r["entry"]["type"] == "thought")
            .count(),
        2
    );
    assert_eq!(kestrel.retain_transcript().await.unwrap(), 1);
    let second = payload_entries(&kestrel, &workspace).await;
    assert_eq!(
        second
            .iter()
            .filter(|r| r["entry"]["type"] == "expired")
            .count(),
        1001
    );
    let fresh: Vec<_> = second
        .iter()
        .filter(|r| r["entry"]["type"] == "thought")
        .collect();
    assert_eq!(fresh.len(), 1);
    assert_eq!(
        fresh[0]["entry"]["text"],
        "appended recently despite ancient completion"
    );
    assert_eq!(
        first.iter().map(|r| &r["seq"]).collect::<Vec<_>>(),
        second.iter().map(|r| &r["seq"]).collect::<Vec<_>>()
    );
    assert_eq!(kestrel.retain_transcript().await.unwrap(), 0);
    let page: Value = reqwest::Client::new()
        .get(format!(
            "{}/link/instances/{}/entries?kinds=narration&summaries=false",
            kestrel.link(),
            on.instance.replace('/', "%2F")
        ))
        .bearer_auth(on.credential.as_str())
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        page["entries"][0]["entry"],
        second
            .iter()
            .find(|r| r["entry"]["type"] == "expired")
            .unwrap()["entry"]
    );
    let typed: Entry = serde_json::from_value(page["entries"][0]["entry"].clone())
        .expect("typed readers must accept tombstones");
    assert!(matches!(typed, Entry::Expired { .. }));
    kestrel.teardown().await;
}

#[tokio::test]
async fn retention_work_role_finds_due_entries_on_startup() {
    let kestrel = Kestrel::stepped().await;
    let (workspace, session) = an_open_workspace(&kestrel, 0).await;
    let on = kestrel.on_the_link(&session).await;
    let link = support::link_client::Link::to(&kestrel.link());
    let report = json!({"session":session.id,"seq":1,"kind":"thought","text":"old reasoning","completion":{"started_at":"2020-01-01T00:00:00Z","finished_at":"2020-01-01T00:01:00Z","turn_outcome":null}});
    assert_eq!(
        link.report_body(&on.instance, Some(&on.credential), &report)
            .await
            .status(),
        StatusCode::ACCEPTED
    );
    kestrel.away_for(PAST_RETENTION);
    let kestrel = kestrel.kill_and_restart().await;
    kestrel.go_live().await;
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if payload_entries(&kestrel, &workspace)
                .await
                .iter()
                .any(|r| r["entry"]["type"] == "expired")
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .expect("the work role should expire due entries on startup");
    kestrel.teardown().await;
}
