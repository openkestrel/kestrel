use std::io::Write;

use anyhow::Result;
use kestrel_operator_types::{WorkLastReport, WorkRepository, WorkspaceWork};
use serde_json::Value;

pub fn show(answer: Value, json: bool) -> Result<()> {
    let mut out = std::io::stdout().lock();
    if json {
        writeln!(out, "{answer}")?;
    } else {
        match serde_json::from_value::<WorkspaceWork>(answer)? {
            WorkspaceWork::WorkNoInstance(work) => {
                writeln!(out, "no Instance; declared branch {}", work.branch)?;
                if let Some(pull_request) = work.pull_request {
                    writeln!(out, "{pull_request}")?;
                }
                last_report(&mut out, work.last_report)?;
            }
            WorkspaceWork::WorkNotAnswering(work) => {
                writeln!(out, "{}", work.message)?;
                last_report(&mut out, work.last_report)?;
            }
            WorkspaceWork::WorkReported(work) => {
                writeln!(
                    out,
                    "Reported by the supervisor {}s ago ({})",
                    age(&work.reported_at)?,
                    work.reported_at
                )?;
                repositories(&mut out, work.repositories)?;
            }
        }
    }
    out.flush()?;
    Ok(())
}

fn last_report(out: &mut impl Write, report: WorkLastReport) -> Result<()> {
    match report {
        WorkLastReport::WorkNoReport(_) => writeln!(out, "No work report received.")?,
        WorkLastReport::WorkInstanceReport(report) => {
            let no_longer_held = if report.current_instance {
                ""
            } else {
                ", no longer the Workspace's"
            };
            writeln!(
                out,
                "\nLast reported by the Instance {}{no_longer_held} {}s ago ({})",
                report.instance,
                age(&report.reported_at)?,
                report.reported_at
            )?;
            repositories(out, report.repositories)?;
        }
    }
    Ok(())
}

fn age(reported_at: &str) -> Result<i64> {
    Ok(jiff::Timestamp::now()
        .duration_since(reported_at.parse()?)
        .as_secs()
        .max(0))
}

fn repositories(out: &mut impl Write, repositories: Vec<WorkRepository>) -> Result<()> {
    for repository in repositories {
        match repository {
            WorkRepository::WorkRepositoryUnreadable(repository) => {
                writeln!(out, "\n{}", repository.repository)?;
                writeln!(out, "  {}", repository.because)?;
            }
            WorkRepository::WorkRepositoryRead(repository) => {
                writeln!(out, "\n{}", repository.repository)?;
                writeln!(
                    out,
                    "On branch {}",
                    repository.branch.as_deref().unwrap_or("(detached HEAD)")
                )?;
                for (label, changes) in [
                    ("Changed", repository.changed),
                    ("Staged", repository.staged),
                ] {
                    writeln!(
                        out,
                        "  {label}: {} {}, +{} -{} lines",
                        changes.files,
                        plural(changes.files, "file", "files"),
                        changes.added,
                        changes.removed
                    )?;
                }
                let committed = repository.committed;
                writeln!(
                    out,
                    "  Committed: {} {}, +{} -{} lines",
                    committed.commits,
                    plural(committed.commits, "commit", "commits"),
                    committed.added,
                    committed.removed
                )?;
                writeln!(
                    out,
                    "  Pushed: {}",
                    repository.pushed.as_deref().unwrap_or("none")
                )?;
                writeln!(out, "  Untracked: {}", repository.untracked)?;
                writeln!(out, "  Stashed: {}", repository.stashed)?;
            }
        }
    }
    Ok(())
}

fn plural<'a>(count: i64, singular: &'a str, plural: &'a str) -> &'a str {
    if count == 1 { singular } else { plural }
}
