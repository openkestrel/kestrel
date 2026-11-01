//! A Client reading a Transcript over the operator boundary, from a cursor and across a restart.

use crate::support;

use kestrel::domain::SessionId;
use reqwest::StatusCode;
use serde_json::Value;
use support::Kestrel;
use support::client::{self, Client};
use support::operator_requests::{an_open_workspace, transcript_of};

async fn recorded_seqs(kestrel: &Kestrel, workspace: &str) -> Vec<i64> {
    kestrel
        .transcript(workspace.parse().expect("a workspace id"))
        .await
        .iter()
        .map(|entry| entry.seq)
        .collect()
}

fn seqs(lines: &[String]) -> Vec<i64> {
    lines
        .iter()
        .map(|line| {
            let entry: Value = serde_json::from_str(line)
                .unwrap_or_else(|error| panic!("{line} is not an entry: {error}"));
            entry["seq"].as_i64().expect("a seq")
        })
        .collect()
}

#[tokio::test]
async fn a_client_in_its_own_process_reads_a_transcript_over_the_operator_boundary() {
    let kestrel = Kestrel::boot().await;
    let (workspace, _) = an_open_workspace(&kestrel, 2).await;
    let (operator, reading) = (kestrel.operator(), workspace.clone());

    let read = tokio::task::spawn_blocking(move || {
        client::ran(&operator, &["workspace", "transcript", &reading, "--json"])
    })
    .await
    .expect("the client should run");

    assert!(read.status.success(), "the client failed:\n{}", read.err);
    assert_eq!(seqs(&read.out), recorded_seqs(&kestrel, &workspace).await);
    let said: Value = serde_json::from_str(&read.out[read.out.len() - 1]).expect("an entry");
    assert_eq!(said["entry"]["type"], "said");
    assert_eq!(said["entry"]["message"], "message 2");
    assert!(
        read.err.contains("cursor  "),
        "the client printed no cursor to resume from:\n{}",
        read.err
    );
    assert!(
        read.left_behind.is_empty(),
        "the client wrote {:?} where it ran",
        read.left_behind
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_client_handed_a_cursor_reads_only_what_came_after_it() {
    let kestrel = Kestrel::boot().await;
    let (workspace, session) = an_open_workspace(&kestrel, 1).await;
    let operator = kestrel.operator();

    let first = {
        let (operator, workspace) = (operator.clone(), workspace.clone());
        tokio::task::spawn_blocking(move || {
            client::ran(&operator, &["workspace", "transcript", &workspace])
        })
        .await
        .expect("the client should run")
    };
    let cursor = first
        .err
        .lines()
        .find_map(|line| line.strip_prefix("cursor  "))
        .expect("a cursor")
        .to_owned();
    kestrel.said(&session, "said after the first read").await;

    let second = tokio::task::spawn_blocking(move || {
        client::ran(
            &operator,
            &["workspace", "transcript", &workspace, "--cursor", &cursor],
        )
    })
    .await
    .expect("the client should run");

    assert!(
        second.status.success(),
        "the client failed:\n{}",
        second.err
    );
    assert_eq!(second.out.len(), 1, "read {:?}", second.out);
    assert!(second.out[0].contains("said after the first read"));

    kestrel.teardown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_following_client_resumes_across_a_restart_without_repeating_an_entry() {
    let kestrel = Kestrel::boot().await;
    let (workspace, session) = an_open_workspace(&kestrel, 2).await;
    let before = recorded_seqs(&kestrel, &workspace).await.len();

    let mut client = Client::spawn(
        &kestrel.operator(),
        &["workspace", "transcript", &workspace, "--follow", "--json"],
    );
    let mut read = Vec::new();
    for _ in 0..before {
        read.push(tokio::task::block_in_place(|| client.line()));
    }

    kestrel.said(&session, "said while it followed").await;
    read.push(tokio::task::block_in_place(|| client.line()));
    kestrel.complete_session(&session).await;

    let kestrel = kestrel.teardown().await.restart().await;
    kestrel.said(&session, "said after the restart").await;
    kestrel
        .seal_workspace(workspace.parse().expect("a workspace id"))
        .await;

    let finished = tokio::task::block_in_place(|| client.finish());
    read.extend(finished.out);

    assert!(
        finished.status.success(),
        "the client failed:\n{}",
        finished.err
    );
    assert_eq!(seqs(&read), recorded_seqs(&kestrel, &workspace).await);
    assert!(
        read.last()
            .expect("an entry")
            .contains("said after the restart")
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_client_asking_for_no_such_workspace_is_refused() {
    let kestrel = Kestrel::boot().await;
    kestrel.declare_organization("acme").await;
    let operator = kestrel.operator();

    let read = tokio::task::spawn_blocking(move || {
        client::ran(
            &operator,
            &[
                "workspace",
                "transcript",
                "01a0a2d8-baf8-7c02-99fa-7280f174c14a",
            ],
        )
    })
    .await
    .expect("the client should run");

    assert!(!read.status.success());
    assert!(
        read.err
            .contains("no workspace in the organization acme matches"),
        "{}",
        read.err
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_cursor_from_another_transcript_is_refused_rather_than_restarting_the_walk() {
    let kestrel = Kestrel::boot().await;
    let (workspace, _) = an_open_workspace(&kestrel, 1).await;
    let elsewhere = format!("{}:1", SessionId::generate());

    let response = reqwest::Client::new()
        .get(format!(
            "{}{}",
            kestrel.operator(),
            transcript_of("acme", &workspace)
        ))
        .header("last-event-id", elsewhere)
        .send()
        .await
        .expect("the operator boundary should answer");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    kestrel.teardown().await;
}
