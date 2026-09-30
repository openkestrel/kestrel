use std::io::Write;

use anyhow::{Context as _, Result};
use reqwest::Response;
use serde::Deserialize;
use serde_json::Value;

use crate::exit::{Exit, Failed};

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
pub fn list(answer: Value, json: bool) -> Result<()> {
    let mut out = std::io::stdout().lock();
    if json {
        writeln!(out, "{answer}")?;
        return Ok(out.flush()?);
    }
    let listing: Listing = serde_json::from_value(answer)?;
    if !listing.path.is_empty() {
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
    if listing.truncated {
        writeln!(
            out,
            "{} of {} entries shown",
            listing.entries.len(),
            listing.total
        )?;
    }
    Ok(out.flush()?)
}

pub async fn read(mut response: Response, json: bool) -> Result<()> {
    let inline = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|kind| kind.to_str().ok())
        .is_some_and(|kind| kind.starts_with("application/json"));
    let mut out = std::io::stdout().lock();
    if inline {
        let answer: Value = response.json().await.context(unreadable())?;
        if json {
            writeln!(out, "{answer}")?;
        } else {
            let text: Text = serde_json::from_value(answer)?;
            out.write_all(text.text.as_bytes())?;
        }
        return Ok(out.flush()?);
    }
    while let Some(chunk) = response.chunk().await.context(unreadable())? {
        out.write_all(&chunk)?;
    }
    Ok(out.flush()?)
}

fn unreadable() -> Failed {
    Failed::new(Exit::Unavailable, "reading the control plane's answer")
}
