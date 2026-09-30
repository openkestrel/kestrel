use std::io::Write;

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

pub fn show(answer: Value, json: bool) -> Result<()> {
    let mut out = std::io::stdout().lock();
    if json {
        writeln!(out, "{answer}")?;
        return Ok(());
    }
    let answer: Repositories = serde_json::from_value(answer)?;
    for repository in answer.repositories {
        writeln!(out, "{}:", repository.repository)?;
        write!(out, "{}{}", repository.diff, repository.text)?;
        if repository.truncated {
            writeln!(
                out,
                "\n[diff truncated at 2 MiB; per-file stats are complete]"
            )?;
        }
    }
    Ok(())
}
