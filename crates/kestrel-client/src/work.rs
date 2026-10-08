use std::io::{IsTerminal as _, Write};

use anyhow::Result;
use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
enum Work {
    Reported {
        repositories: Vec<Repository>,
        reported_at: jiff::Timestamp,
    },
    NoInstance {
        branch: String,
        pull_request: Option<String>,
    },
    NotAnswering {
        message: String,
    },
}

#[derive(Deserialize)]
struct Repository {
    repository: String,
    #[serde(flatten)]
    git: Git,
}

#[derive(Deserialize)]
#[serde(tag = "git", rename_all = "snake_case")]
enum Git {
    Read {
        branch: Option<String>,
        changed: Changes,
        staged: Changes,
        committed: Commits,
        pushed: Option<String>,
        untracked: u64,
        stashed: u64,
    },
    Unreadable {
        because: String,
    },
}

#[derive(Deserialize)]
struct Changes {
    files: u64,
    added: u64,
    removed: u64,
}

#[derive(Deserialize)]
struct Commits {
    commits: u64,
    added: u64,
    removed: u64,
}

pub fn show(answer: Value, json: bool) -> Result<()> {
    let terminal = std::io::stdout().is_terminal();
    let mut out = std::io::stdout().lock();
    if json {
        writeln!(out, "{answer}")?;
    } else {
        match serde_json::from_value::<Work>(answer)? {
            Work::NoInstance {
                branch,
                pull_request,
            } => {
                writeln!(out, "no Instance; declared branch {branch}")?;
                if let Some(pull_request) = pull_request {
                    writeln!(out, "{pull_request}")?;
                }
            }
            Work::NotAnswering { message } => writeln!(out, "{message}")?,
            Work::Reported {
                repositories,
                reported_at,
            } => {
                let age = jiff::Timestamp::now()
                    .duration_since(reported_at)
                    .as_secs()
                    .max(0);
                if terminal {
                    writeln!(out, "Reported by the supervisor {age}s ago ({reported_at})")?;
                }
                for repository in repositories {
                    if terminal {
                        writeln!(out, "\n{}", repository.repository)?;
                    }
                    match repository.git {
                        Git::Unreadable { because } => writeln!(out, "  {because}")?,
                        Git::Read {
                            branch,
                            changed,
                            staged,
                            committed,
                            pushed,
                            untracked,
                            stashed,
                        } => {
                            writeln!(
                                out,
                                "On branch {}",
                                branch.as_deref().unwrap_or("(detached HEAD)")
                            )?;
                            for (label, changes) in [("Changed", changed), ("Staged", staged)] {
                                writeln!(
                                    out,
                                    "  {label}: {} {}, +{} -{} lines",
                                    changes.files,
                                    plural(changes.files, "file", "files"),
                                    changes.added,
                                    changes.removed
                                )?;
                            }
                            writeln!(
                                out,
                                "  Committed: {} {}, +{} -{} lines",
                                committed.commits,
                                plural(committed.commits, "commit", "commits"),
                                committed.added,
                                committed.removed
                            )?;
                            writeln!(out, "  Pushed: {}", pushed.as_deref().unwrap_or("none"))?;
                            writeln!(out, "  Untracked: {untracked}")?;
                            writeln!(out, "  Stashed: {stashed}")?;
                        }
                    }
                }
            }
        }
    }
    out.flush()?;
    Ok(())
}

fn plural<'a>(count: u64, singular: &'a str, plural: &'a str) -> &'a str {
    if count == 1 { singular } else { plural }
}
