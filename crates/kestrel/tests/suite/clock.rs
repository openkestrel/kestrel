//! The control plane reads time from the clock `Timer` holds (ADR-0059), so a stepped test moves
//! it: time the control plane was present for, or time it was away (ADR-0054).

use crate::support;

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use kestrel::domain::{Exit, SessionState, Workspace};
use kestrel::work::{GAP, GRACE, LEASE};
use support::Kestrel;
use support::fixture::Fixture;

/// Each reads the time without asking the clock.
const READINGS: [&str; 5] = [
    "Timestamp::now",
    "Instant::now",
    "SystemTime::now",
    "Zoned::now",
    ".elapsed()",
];

/// Each keeps real time on purpose: what it times is a process or the default for production.
const SYSTEM_CLOCK_READERS: [&str; 4] = [
    "compute/docker.rs",
    "compute/local_exec.rs",
    "sign_in/relay.rs",
    "store/mod.rs",
];

async fn a_workspace(kestrel: &Kestrel) -> Workspace {
    Fixture::acme().model("claude-opus-5").open(kestrel).await
}

#[test]
fn nothing_in_the_control_plane_reads_the_time_but_its_clock() {
    let source = support::crate_root().join("src");
    let clock = source.join("timer.rs");
    let sources = sources(&source);
    assert!(sources.len() > 1, "the control plane should have sources");

    for path in sources.iter().filter(|path| **path != clock) {
        let text = fs::read_to_string(path).expect("a readable source file");
        for reading in READINGS {
            assert!(
                !text.contains(reading),
                "{} reads {reading}, and only the clock `Timer` holds reads the time",
                path.display()
            );
        }
        let relative = path.strip_prefix(&source).expect("a path under src");
        let unit_tests = relative.ends_with("tests.rs");
        let production = ["#[cfg(test)]", "#[cfg(all(test"]
            .iter()
            .filter_map(|tests| text.find(tests))
            .min()
            .map_or(text.as_str(), |tests| &text[..tests]);
        assert!(
            unit_tests
                || SYSTEM_CLOCK_READERS
                    .iter()
                    .any(|reader| relative.ends_with(reader))
                || !production.contains("Clock::System"),
            "{} reads the system clock past the clock it was given",
            path.display()
        );
    }
}

#[tokio::test]
async fn a_lease_advanced_past_expires_as_any_lease_does_because_the_control_plane_was_there() {
    let kestrel = Kestrel::stepped().await;
    let workspace = a_workspace(&kestrel).await;
    let (session, _on) = kestrel.dispatch_to_the_link(workspace.id).await;

    kestrel.advance((LEASE + GAP).unsigned_abs()).await;

    let ended = kestrel.session(session.id).await;
    assert_eq!(ended.state, SessionState::Ended);
    assert_eq!(
        ended.exit,
        Some(Exit::Failed {
            because: "the supervisor stopped holding the session's lease out, and it expired"
                .to_owned()
        })
    );
    kestrel.teardown().await;
}

#[tokio::test]
async fn a_lease_that_lapsed_while_the_control_plane_was_away_is_given_grace_and_then_expires() {
    let kestrel = Kestrel::stepped().await;
    let workspace = a_workspace(&kestrel).await;
    let (session, _on) = kestrel.dispatch_to_the_link(workspace.id).await;
    kestrel.work_pass().await;
    let left = kestrel.now();

    kestrel.away_for(Duration::from_secs(9 * 60));
    kestrel.work_pass().await;

    let graced = kestrel.session(session.id).await;
    assert_ne!(graced.state, SessionState::Ended);
    assert_eq!(graced.lease_expires_at, Some(kestrel.now() + GRACE));
    let back = kestrel.now();
    kestrel.advance(GRACE.unsigned_abs()).await;
    let ended = kestrel.session(session.id).await;
    assert_eq!(
        ended.exit,
        Some(Exit::Failed {
            because: format!(
                "the control plane was not running from {left} to {back}, and the supervisor did not reach it within 30 s after"
            )
        })
    );
    kestrel.teardown().await;
}

fn sources(directory: &Path) -> Vec<PathBuf> {
    fs::read_dir(directory)
        .expect("a readable source directory")
        .flat_map(|entry| {
            let path = entry.expect("a readable directory entry").path();
            if path.is_dir() {
                sources(&path)
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                vec![path]
            } else {
                Vec::new()
            }
        })
        .collect()
}
