//! Fixtures for discovering Events on a repository kestrel does not own, by polling its GitHub
//! App's Delivery log (ADR-0055): watch a stub's repository and wait for what is recorded.

use std::time::Duration;

use jiff::SignedDuration;
use kestrel::domain::{Direction, Event};

use super::Kestrel;
use super::github_stub::{self, GithubStub};

pub const PATIENCE: Duration = Duration::from_secs(30);
pub const REPOSITORY: &str = "jtmthf/kestrel";
pub const BOTH: &[Direction] = &[Direction::Inbound, Direction::Outbound];
pub const LISTED: &str = "/app/hook/deliveries?";
pub const FETCHED: &str = "/app/hook/deliveries/";

/// Sooner than the wheel's own sweep, so what paces these tests is the sweep rather than a
/// wait written into them.
pub fn eagerly() -> SignedDuration {
    SignedDuration::from_millis(1)
}

pub async fn watching(kestrel: &Kestrel, stub: &GithubStub, carries: &[Direction]) {
    kestrel.declare_organization("acme").await;
    kestrel
        .register_integration(
            "acme",
            "github",
            REPOSITORY,
            &stub.base_url(),
            carries,
            eagerly(),
        )
        .await;
}

pub async fn recorded(kestrel: &Kestrel, count: usize) -> Vec<Event> {
    let deadline = tokio::time::Instant::now() + PATIENCE;

    loop {
        let events = kestrel.events("acme").await;
        if events.len() >= count {
            return events;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "{count} events were never recorded; {} were",
            events.len()
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// How many of a stub's requests listed the Delivery log, rather than the one-off `GET /app` a
/// registration makes to learn the Integration's bot login.
pub fn polls(stub: &GithubStub) -> usize {
    requested(stub, LISTED)
}

pub fn requested(stub: &GithubStub, path: &str) -> usize {
    stub.requests()
        .iter()
        .filter(|request| request.url.starts_with(path))
        .count()
}

pub async fn polled(stub: &GithubStub, times: usize) {
    let deadline = tokio::time::Instant::now() + PATIENCE;

    loop {
        let asked = polls(stub);
        if asked >= times {
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "github was polled {asked} times rather than {times}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

pub fn labelled_ready(issue: i64) -> github_stub::Delivery {
    github_stub::labelled(issue, "ready-for-agent")
}
