//! The courier carries no cargo (ADR-0002), as something you can fail a build on: the
//! supervisor forwards, blocks, relays and holds a cursor, and never reasons about the domain.

mod support;

use std::fs;
use std::path::{Path, PathBuf};

const DOMAIN: [&str; 10] = [
    "workspace",
    "project",
    "organization",
    "transcript",
    "campaign",
    "trigger",
    "workflow",
    "approval",
    "policy",
    "audit",
];

/// The wire names of an adapter extension the supervisor declares to every harness (ADR-0040).
const EXTENSION_NAMES: [&str; 2] = [
    "opencode/child-session-updates",
    "opencode/session/child_update",
];

#[test]
fn nothing_in_the_supervisor_names_a_thing_only_the_control_plane_may_reason_about() {
    let sources = sources(&support::crate_root().join("src"));
    assert!(!sources.is_empty(), "the supervisor should have sources");

    for source in sources {
        let spoken = spoken(&source);

        for word in DOMAIN {
            assert!(
                !spoken.contains(word),
                "{} names {word}, which is the control plane's to know",
                source.display()
            );
        }
    }
}

/// kestrel implements someone else's client (ADR-0007), so which agent is on the other end of
/// it is not something the supervisor may look at.
#[test]
fn nothing_in_the_supervisor_names_an_agent_it_might_be_driving() {
    for source in sources(&support::crate_root().join("src")) {
        let spoken = spoken(&source);

        for agent in ["opencode", "claude", "codex", "gemini"] {
            assert!(
                !spoken.contains(agent),
                "{} names {agent}, and kestrel drives whatever speaks ACP",
                source.display()
            );
        }
    }
}

fn spoken(source: &Path) -> String {
    let spoken = fs::read_to_string(source)
        .expect("a readable source file")
        .to_lowercase();

    EXTENSION_NAMES
        .iter()
        .fold(spoken, |spoken, name| spoken.replace(name, ""))
}

fn sources(directory: &Path) -> Vec<PathBuf> {
    fs::read_dir(directory)
        .expect("a readable source directory")
        .flat_map(|entry| {
            let path = entry.expect("a readable directory entry").path();
            if path.is_dir() {
                sources(&path)
            } else {
                vec![path]
            }
        })
        .collect()
}
