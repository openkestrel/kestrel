use std::io::{IsTerminal as _, Write};

use anyhow::{Context as _, Result, bail};
use reqwest::Response;
use serde::Deserialize;
use serde_json::Value;

use crate::exit::{Exit, Failed};
use crate::output::Presentation;

#[derive(Deserialize)]
struct Listing {
    path: String,
    entries: Vec<Entry>,
    total: u64,
    truncated: bool,
}

#[derive(Deserialize)]
struct Entry {
    name: String,
    kind: String,
    git: Option<String>,
}

#[derive(Deserialize)]
struct Text {
    text: String,
}

/// One line an entry, marked the way `ls -F` marks directories and symlinks.
pub fn list(answer: Value, presentation: &Presentation, empty: &str) -> Result<()> {
    let mut out = std::io::stdout().lock();
    if presentation.json() {
        crate::output::document(&mut out, presentation, &answer)?;
        return Ok(out.flush()?);
    }
    let listing: Listing = serde_json::from_value(answer)?;
    let terminal = std::io::stdout().is_terminal();
    if terminal && listing.entries.is_empty() {
        writeln!(out, "{empty}")?;
    }
    if terminal && !listing.entries.is_empty() && !listing.path.is_empty() {
        writeln!(out, "{}", listing.path)?;
    }
    for entry in &listing.entries {
        let marked = match entry.kind.as_str() {
            "directory" => "/",
            "symlink" => "@",
            _ => "",
        };
        match &entry.git {
            Some(git) => writeln!(out, "{git:<10} {}{marked}", entry.name)?,
            None => writeln!(out, "{}{marked}", entry.name)?,
        }
    }
    if terminal && listing.truncated {
        writeln!(
            out,
            "{} of {} entries shown",
            listing.entries.len(),
            listing.total
        )?;
    }
    Ok(out.flush()?)
}

pub async fn read(mut response: Response, presentation: &Presentation) -> Result<()> {
    let inline = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|kind| kind.to_str().ok())
        .is_some_and(|kind| kind.starts_with("application/json"));
    let mut out = std::io::stdout().lock();
    if inline {
        let answer: Value = response.json().await.context(unreadable())?;
        if presentation.json() {
            crate::output::document(&mut out, presentation, &answer)?;
        } else {
            let text: Text = serde_json::from_value(answer)?;
            out.write_all(text.text.as_bytes())?;
        }
        return Ok(out.flush()?);
    }
    // A file the control plane hands back as its own bytes has no response to filter, so this is
    // a raw-content exception rather than an invented record.
    if presentation.filter().is_some() {
        bail!(Failed::new(
            Exit::Usage,
            "--jq filters a JSON answer, and this file is answered as its own bytes; read it \
             without --jq"
        ));
    }
    while let Some(chunk) = response.chunk().await.context(unreadable())? {
        out.write_all(&chunk)?;
    }
    Ok(out.flush()?)
}

fn unreadable() -> Failed {
    Failed::new(Exit::Unavailable, "reading the control plane's answer")
}
