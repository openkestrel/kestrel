mod answer;
mod kind;
mod offer;
mod step;

use std::io::{IsTerminal as _, Write as _};
use std::sync::atomic::{AtomicBool, Ordering};

use kestrel_operator_types as wire;
use serde_json::json;

use crate::diagnostic::answer::{bounded, inspect_operation, unknown_response};
use crate::exit::{Exit, Failed};

pub use crate::diagnostic::answer::{Request, refusal, unreachable, unreadable};
pub use crate::diagnostic::kind::{after_save, carried, field, message};

static WROTE: AtomicBool = AtomicBool::new(false);

/// Once a write has been sent, nothing this invocation offers may send it again.
pub fn writing() {
    WROTE.store(true, Ordering::Relaxed);
}

pub struct Invocation {
    pub control_plane: String,
    /// The control plane differs from the default, so every rendered command must name it.
    pub elsewhere: bool,
    pub operation: String,
    pub args: Vec<String>,
    pub json: bool,
    pub verbose: bool,
}

/// A failure nothing classified still offers a next step. An `Unavailable` one had an answer
/// the Client could not read; any other happened on this machine.
fn fallback(
    error: &anyhow::Error,
    exit: Exit,
    invocation: &Invocation,
    wrote: bool,
) -> wire::Diagnostic {
    let message = bounded(&error.to_string()).unwrap_or_else(|| "kestrel failed".to_owned());
    let evidence = bounded(&format!("{error:#}"));
    if exit == Exit::Unavailable {
        let request = Request {
            operation: &invocation.operation,
            write: wrote,
        };
        return unknown_response(
            message,
            request.operation,
            None,
            evidence,
            request.lost(None),
        );
    }
    wire::Diagnostic::ClientFailureDiagnostic(wire::ClientFailureDiagnostic {
        kind: json!("client_failure"),
        message,
        field: None,
        context: wire::ClientFailureContext {
            operation: invocation.operation.clone(),
            evidence,
        },
        next_steps: vec![inspect_operation(&invocation.operation, wrote)],
    })
}

/// The exit is the original failure's, whatever a step chosen afterwards does.
pub fn report(error: &anyhow::Error, invocation: &Invocation) -> Exit {
    let exit = Exit::of(error);
    let wrote = WROTE.load(Ordering::Relaxed);
    let diagnostic = error
        .downcast_ref::<Failed>()
        .and_then(Failed::diagnostic)
        .cloned()
        .unwrap_or_else(|| fallback(error, exit, invocation, wrote));

    let mut stderr = std::io::stderr().lock();
    if invocation.json {
        let _ = writeln!(
            stderr,
            "{}",
            serde_json::to_string(&diagnostic).unwrap_or_default()
        );
        return exit;
    }
    let steps = step::steps(&diagnostic, invocation, wrote);
    let _ = write!(
        stderr,
        "error: {}\n{}",
        message(&diagnostic),
        step::described(&diagnostic, &steps, invocation)
    );
    if invocation.verbose {
        let _ = writeln!(
            stderr,
            "diagnostic: {}",
            serde_json::to_string(&diagnostic).unwrap_or_default()
        );
    }
    if std::io::stdin().is_terminal() && stderr.is_terminal() {
        let chosen = offer::chosen(
            &steps,
            invocation,
            &mut std::io::stdin().lock(),
            &mut stderr,
        );
        drop(stderr);
        if let Err(error) = chosen.and_then(|argv| argv.map(offer::launched).transpose()) {
            eprintln!("error: {error:#}");
        }
    }
    exit
}

/// A Session's durable failure, as part of a successful read.
pub fn inline(diagnostic: &wire::Diagnostic, invocation: &Invocation) -> String {
    let steps = step::steps(diagnostic, invocation, false);
    format!(
        "{}\n{}",
        message(diagnostic),
        step::described(diagnostic, &steps, invocation)
    )
    .trim_end()
    .to_owned()
}

#[cfg(test)]
pub(crate) mod tests;
