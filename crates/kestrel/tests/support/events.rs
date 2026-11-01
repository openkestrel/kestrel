//! Fixtures for discovering Events on a repository kestrel does not own, by polling its GitHub
//! App's Delivery log (ADR-0055): watch a stub's repository and step the wheel to what it records.

use std::time::Duration;

use jiff::SignedDuration;
use kestrel::domain::Direction;

use super::Kestrel;
use super::github_stub::{self, GithubStub};

pub const POLL_INTERVAL: Duration = Duration::from_secs(1);
pub const REPOSITORY: &str = "jtmthf/kestrel";
pub const BOTH: &[Direction] = &[Direction::Inbound, Direction::Outbound];
pub const LISTED: &str = "/app/hook/deliveries?";
pub const FETCHED: &str = "/app/hook/deliveries/";

pub async fn watching(kestrel: &Kestrel, stub: &GithubStub, carries: &[Direction]) {
    kestrel.declare_organization("acme").await;
    kestrel
        .register_integration(
            "acme",
            "github",
            REPOSITORY,
            &stub.base_url(),
            carries,
            SignedDuration::from_secs(1),
        )
        .await;
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

pub fn labelled_ready(issue: i64) -> github_stub::Delivery {
    github_stub::labelled(issue, "ready-for-agent")
}
