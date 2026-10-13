use std::io::{IsTerminal as _, Write};

use anyhow::Result;
use kestrel_operator_types as wire;

use crate::output::Presentation;
use crate::typed;

/// Which read answered: changes carry a diff and complete per-file statistics, commits and
/// stashes carry prose, and neither is ever diffed here.
pub enum Read {
    Changes(wire::WorkspaceChanges),
    Commits(wire::WorkspaceCommits),
    Stashes(wire::WorkspaceStashes),
}

/// One repository's answer, whichever read it came from.
struct Body<'a> {
    repository: &'a str,
    text: &'a str,
    truncated: bool,
}

impl Read {
    fn bodies(&self) -> Vec<Body<'_>> {
        match self {
            Read::Changes(answer) => answer
                .repositories
                .iter()
                .map(|repository| Body {
                    repository: &repository.repository,
                    text: &repository.diff,
                    truncated: repository.truncated,
                })
                .collect(),
            Read::Commits(answer) => prose(&answer.repositories),
            Read::Stashes(answer) => prose(&answer.repositories),
        }
    }
}

fn prose(repositories: &[wire::RepositoryText]) -> Vec<Body<'_>> {
    repositories
        .iter()
        .map(|repository| Body {
            repository: &repository.repository,
            text: &repository.text,
            truncated: false,
        })
        .collect()
}

pub fn show(presentation: &Presentation, answer: &Read, empty: &str) -> Result<()> {
    if presentation.json() {
        return match answer {
            Read::Changes(answer) => typed::whole(presentation, answer),
            Read::Commits(answer) => typed::whole(presentation, answer),
            Read::Stashes(answer) => typed::whole(presentation, answer),
        };
    }
    let repositories = answer.bodies();
    let mut out = std::io::stdout().lock();
    let terminal = std::io::stdout().is_terminal();
    if terminal && repositories.iter().all(|body| body.text.is_empty()) {
        writeln!(out, "{empty}")?;
        return Ok(());
    }
    for body in repositories {
        if terminal && !body.text.is_empty() {
            writeln!(out, "{}:", body.repository)?;
        }
        write!(out, "{}", body.text)?;
        if terminal && body.truncated {
            writeln!(
                out,
                "\n[diff truncated at 2 MiB; per-file stats are complete]"
            )?;
        }
    }
    Ok(out.flush()?)
}
