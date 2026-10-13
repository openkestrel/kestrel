use std::io::{IsTerminal as _, Write};

use anyhow::Result;
use serde::Deserialize;
use serde_json::Value;

use crate::output::Presentation;

#[derive(Deserialize)]
struct Repositories {
    repositories: Vec<Repository>,
}

#[derive(Deserialize)]
struct Repository {
    repository: String,
    #[serde(default)]
    diff: String,
    #[serde(default)]
    text: String,
    #[serde(default)]
    truncated: bool,
}

pub fn show(answer: Value, presentation: &Presentation, empty: &str) -> Result<()> {
    let mut out = std::io::stdout().lock();
    if presentation.json() {
        crate::output::document(&mut out, presentation, &answer)?;
        return Ok(out.flush()?);
    }
    let answer: Repositories = serde_json::from_value(answer)?;
    let terminal = std::io::stdout().is_terminal();
    if terminal
        && answer
            .repositories
            .iter()
            .all(|repository| repository.diff.is_empty() && repository.text.is_empty())
    {
        writeln!(out, "{empty}")?;
        return Ok(());
    }
    for repository in answer.repositories {
        if terminal && (!repository.diff.is_empty() || !repository.text.is_empty()) {
            writeln!(out, "{}:", repository.repository)?;
        }
        write!(out, "{}{}", repository.diff, repository.text)?;
        if terminal && repository.truncated {
            writeln!(
                out,
                "\n[diff truncated at 2 MiB; per-file stats are complete]"
            )?;
        }
    }
    Ok(())
}
