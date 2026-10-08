use std::io::{IsTerminal as _, Write};

use anyhow::Result;
use serde::Deserialize;
use serde_json::Value;

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

pub fn show(answer: Value, json: bool, empty: &str) -> Result<()> {
    let mut out = std::io::stdout().lock();
    if json {
        writeln!(out, "{answer}")?;
        return Ok(());
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
