use std::io::{IsTerminal as _, Write};

use anyhow::Result;
use kestrel_operator_types::{
    WorkInstanceReport, WorkLastReport, WorkRepository, WorkRepositoryRead,
    WorkRepositoryUnreadable, WorkspaceWork,
};
use serde_json::Value;

pub fn show(answer: Value, json: bool) -> Result<()> {
    let terminal = std::io::stdout().is_terminal();
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
                last_report(&mut out, work.last_report, terminal)?;
                earlier(&mut out, work.earlier_reports, terminal)?;
            }
            WorkspaceWork::WorkNotAnswering(work) => {
                writeln!(out, "{}", work.message)?;
                last_report(&mut out, work.last_report, terminal)?;
                earlier(&mut out, work.earlier_reports, terminal)?;
            }
            WorkspaceWork::WorkReported(work) => {
                if terminal {
                    let age = work
                        .reported_at
                        .parse::<jiff::Timestamp>()
                        .map(|at| jiff::Timestamp::now().duration_since(at).as_secs().max(0))?;
                    writeln!(
                        out,
                        "Reported by the supervisor {age}s ago ({})",
                        work.reported_at
                    )?;
                }
                repositories(&mut out, work.repositories, terminal)?;
                earlier(&mut out, work.earlier_reports, terminal)?;
            }
        }
    }
    out.flush()?;
    Ok(())
}

fn last_report(out: &mut impl Write, last: WorkLastReport, terminal: bool) -> Result<()> {
    match last {
        WorkLastReport::WorkNoReport(_) => writeln!(out, "No work report received.")?,
        WorkLastReport::WorkInstanceReport(report) => {
            let whose = if report.instance_current {
                "its Instance"
            } else {
                "the earlier Instance"
            };
            writeln!(
                out,
                "Last reported by {whose} {} at {}",
                report.instance, report.reported_at
            )?;
            repositories(out, report.repositories, terminal)?;
        }
    }
    Ok(())
}

fn earlier(out: &mut impl Write, reports: Vec<WorkInstanceReport>, terminal: bool) -> Result<()> {
    for report in reports {
        writeln!(
            out,
            "\nEarlier reported by the Instance {} at {}",
            report.instance, report.reported_at
        )?;
        repositories(out, report.repositories, terminal)?;
    }
    Ok(())
}

fn repositories(
    out: &mut impl Write,
    repositories: Vec<WorkRepository>,
    terminal: bool,
) -> Result<()> {
    for repository in repositories {
        if terminal {
            let (WorkRepository::WorkRepositoryRead(WorkRepositoryRead { repository, .. })
            | WorkRepository::WorkRepositoryUnreadable(WorkRepositoryUnreadable {
                repository,
                ..
            })) = &repository;
            writeln!(out, "\n{repository}")?;
        }
        match repository {
            WorkRepository::WorkRepositoryUnreadable(WorkRepositoryUnreadable {
                because, ..
            }) => {
                writeln!(out, "  {because}")?;
            }
            WorkRepository::WorkRepositoryRead(WorkRepositoryRead {
                branch,
                changed,
                staged,
                committed,
                pushed,
                untracked,
                stashed,
                ..
            }) => {
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
    Ok(())
}

fn plural<'a>(count: i64, singular: &'a str, plural: &'a str) -> &'a str {
    if count == 1 { singular } else { plural }
}
