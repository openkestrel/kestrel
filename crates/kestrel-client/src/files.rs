use std::io::{IsTerminal as _, Write};

use anyhow::{Context as _, Result, bail};
use kestrel_operator_types::{FileEntryKind, FileListing, FileText};
use reqwest::Response;

use crate::exit::{Exit, Failed};
use crate::output::Presentation;
use crate::typed;

/// One line an entry, marked the way `ls -F` marks directories and symlinks, and prefixed with the
/// mark git gives it when the listing has one.
pub fn list(presentation: &Presentation, listing: &FileListing, empty: &str) -> Result<()> {
    if presentation.json() {
        return typed::whole(presentation, listing);
    }
    let mut out = std::io::stdout().lock();
    let terminal = std::io::stdout().is_terminal();
    if terminal && listing.entries.is_empty() {
        writeln!(out, "{empty}")?;
    }
    if terminal && !listing.entries.is_empty() && !listing.path.is_empty() {
        writeln!(out, "{}", listing.path)?;
    }
    for entry in &listing.entries {
        let marked = match &entry.kind {
            FileEntryKind::Directory => "/",
            FileEntryKind::Symlink => "@",
            FileEntryKind::File | FileEntryKind::Other => "",
        };
        match &entry.git {
            Some(git) => writeln!(out, "{:<10} {}{marked}", git.as_str(), entry.name)?,
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
    if inline {
        let answer: FileText = response.json().await.context(unreadable())?;
        if presentation.json() {
            return typed::whole(presentation, &answer);
        }
        let mut out = std::io::stdout().lock();
        out.write_all(answer.text.as_bytes())?;
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
    let mut out = std::io::stdout().lock();
    while let Some(chunk) = response.chunk().await.context(unreadable())? {
        out.write_all(&chunk)?;
    }
    Ok(out.flush()?)
}

fn unreadable() -> Failed {
    Failed::new(Exit::Unavailable, "reading the control plane's answer")
}
