use std::io::{BufRead as _, IsTerminal as _, Read as _};

use anyhow::{Context as _, Result};

use crate::exit::{Exit, Failed};
use crate::terminal;

pub(super) fn given(value: &str) -> Result<String> {
    if value == "-" {
        return standard_input();
    }
    match value.strip_prefix('@') {
        Some(path) => std::fs::read_to_string(path)
            .with_context(|| Failed::new(Exit::Usage, format!("reading {path}"))),
        None => Ok(value.to_owned()),
    }
}

pub(super) fn standard_input() -> Result<String> {
    let mut text = String::new();
    std::io::stdin()
        .read_to_string(&mut text)
        .context("reading standard input")?;

    Ok(text)
}

/// The asking happens only where someone is there to read it: a pipe is read in silence, so
/// nothing driving the Client can block on a prompt it cannot see.
pub(super) fn read_standard_input(what: &str) -> Result<String> {
    let mut stdin = std::io::stdin();
    let unechoed = if stdin.is_terminal() {
        let guard = terminal::Unechoed::on(&stdin).ok_or_else(|| {
            Failed::new(
                Exit::Failure,
                "cannot disable terminal echo; no secret was read",
            )
        })?;
        eprintln!("reading {what} from standard input, unechoed; end it with ctrl-d");
        Some(guard)
    } else {
        None
    };

    let mut read = String::new();
    stdin.read_to_string(&mut read)?;
    if unechoed.is_some() {
        eprintln!();
    }

    Ok(read)
}

/// Both ends a person would answer through, so a prompt is never written where nobody reads it.
pub(super) fn interactive() -> bool {
    std::io::stdin().is_terminal() && std::io::stderr().is_terminal()
}

/// One line from a terminal with echo off, the prompt written only once echo is off.
pub(super) fn hidden_line(prompt: &str) -> Result<String> {
    let stdin = std::io::stdin();
    let _unechoed = terminal::Unechoed::on(&stdin).ok_or_else(|| {
        Failed::new(
            Exit::Failure,
            "cannot disable terminal echo; no secret was read",
        )
    })?;
    eprint!("{prompt}");
    let mut line = String::new();
    stdin
        .lock()
        .read_line(&mut line)
        .context("reading standard input")?;
    eprintln!();

    Ok(without_line_end(line))
}

/// An answer typed at a terminal, echoed as it is typed.
pub(super) fn line(prompt: &str) -> Result<String> {
    eprint!("{prompt}");
    let mut line = String::new();
    std::io::stdin()
        .lock()
        .read_line(&mut line)
        .context("reading standard input")?;

    Ok(without_line_end(line))
}

pub(super) fn without_line_end(mut text: String) -> String {
    if text.ends_with('\n') {
        text.pop();
        if text.ends_with('\r') {
            text.pop();
        }
    }
    text
}
