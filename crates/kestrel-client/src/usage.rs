use clap::CommandFactory as _;
use clap::error::{ContextKind, ContextValue};

use crate::Client;
use crate::exit::Exit;

/// The retired `--json FIELDS` took the next word as its value, so any word there is refused
/// rather than read as an argument, which could post a field name as a message.
pub(crate) fn json_value(args: &[String]) -> Option<(&str, Vec<String>)> {
    let end = args
        .iter()
        .position(|arg| arg == "--")
        .unwrap_or(args.len());
    let at = args[..end]
        .iter()
        .position(|arg| arg.starts_with("--json="))
        .or_else(|| {
            args[..end]
                .windows(2)
                .position(|pair| pair[0] == "--json" && !pair[1].starts_with('-'))
                .map(|at| at + 1)
        })?;
    let mut corrected = args.to_vec();
    let value = match args[at].strip_prefix("--json=") {
        Some(value) => {
            corrected[at] = "--json".to_owned();
            value
        }
        None => {
            corrected.remove(at);
            &args[at]
        }
    };
    Some((value, corrected))
}

/// Clap's own exit codes would do, but the catalog is what a script was promised.
pub(crate) fn exit_after(error: &clap::Error) -> ! {
    let _ = error.print();
    if let Some(explanation) =
        explain_invalid_subcommand(error, &std::env::args().skip(1).collect::<Vec<_>>())
    {
        eprint!("{explanation}");
    }
    let exit = if error.use_stderr() {
        Exit::Usage
    } else {
        Exit::Success
    };
    std::process::exit(exit.code().into())
}

fn explain_invalid_subcommand(error: &clap::Error, args: &[String]) -> Option<String> {
    if error.kind() != clap::error::ErrorKind::InvalidSubcommand {
        return None;
    }
    let Some(ContextValue::String(guessed)) = error.get(ContextKind::InvalidSubcommand) else {
        return None;
    };
    let noun = args
        .windows(2)
        .rev()
        .find(|pair| (pair[0] == "workspace" || pair[0] == "session") && pair[1] == *guessed)?[0]
        .as_str();
    let correct = match (noun, guessed.as_str()) {
        ("workspace", "create" | "new" | "start" | "begin") => Some("open"),
        ("workspace", "close" | "stop" | "end" | "finish") => Some("seal"),
        ("session", "start" | "create" | "launch" | "execute" | "queue") => Some("enqueue"),
        _ => None,
    }
    .map(str::to_owned)
    .or_else(|| match error.get(ContextKind::SuggestedSubcommand) {
        Some(ContextValue::Strings(suggestions)) if suggestions.len() == 1 => {
            suggestions.first().cloned()
        }
        _ => None,
    });
    let verbs = Client::command()
        .find_subcommand(noun)?
        .get_subcommands()
        .map(clap::Command::get_name)
        .collect::<Vec<_>>()
        .join(", ");
    let try_command = correct.map_or_else(String::new, |correct| {
        format!("Try `kestrel {noun} {correct}`.\n")
    });
    Some(format!("{try_command}Accepted {noun} verbs: {verbs}\n"))
}

#[cfg(test)]
mod tests;
