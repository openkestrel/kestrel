use std::fmt;

use kestrel_operator_types as wire;
use serde_json::{Value, json};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Exit {
    Success,
    Failure,
    Usage,
    Unresolved,
    Rejected,
    Unavailable,
    NotReady,
}

impl Exit {
    pub const ALL: [Exit; 7] = [
        Exit::Success,
        Exit::Failure,
        Exit::Usage,
        Exit::Unresolved,
        Exit::Rejected,
        Exit::Unavailable,
        Exit::NotReady,
    ];

    /// Published by `kestrel exit-codes` and branched on by scripts, so a number never moves.
    pub const fn code(self) -> u8 {
        match self {
            Exit::Success => 0,
            Exit::Failure => 1,
            Exit::Usage => 2,
            Exit::Unresolved => 3,
            Exit::Rejected => 4,
            Exit::Unavailable => 5,
            Exit::NotReady => 78,
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Exit::Success => "success",
            Exit::Failure => "failure",
            Exit::Usage => "usage",
            Exit::Unresolved => "unresolved",
            Exit::Rejected => "rejected",
            Exit::Unavailable => "unavailable",
            Exit::NotReady => "not_ready",
        }
    }

    const fn meaning(self) -> &'static str {
        match self {
            Exit::Success => "The command did what it was asked.",
            Exit::Failure => "The Client failed on this machine, such as writing its output.",
            Exit::Usage => "The invocation is invalid: a command, flag, argument or input.",
            Exit::Unresolved => "Something named matches nothing, or more than one thing.",
            Exit::Rejected => "The control plane understood the request and declined it.",
            Exit::Unavailable => "The control plane could not be reached, or could not answer.",
            Exit::NotReady => "A setup prerequisite the work needs is missing.",
        }
    }

    const fn branch(self) -> &'static str {
        match self {
            Exit::Success => "Carry on; stdout holds what the command produced.",
            Exit::Failure => "Stop; the operation may have taken effect.",
            Exit::Usage => "Fix the command line; unchanged, it fails the same way.",
            Exit::Unresolved => "Name it exactly, or declare it first.",
            Exit::Rejected => "Change the request; repeated, it is declined again.",
            Exit::Unavailable => "Retry later; the operation may have taken effect.",
            Exit::NotReady => "Complete the setup step it names, then run the command again.",
        }
    }

    pub fn record(self) -> Value {
        json!({
            "code": self.code(),
            "name": self.name(),
            "meaning": self.meaning(),
            "branch": self.branch(),
        })
    }

    /// Wrapping a failure in more context keeps its exit code; only another `Failed` changes it.
    pub fn of(error: &anyhow::Error) -> Exit {
        error
            .downcast_ref::<Failed>()
            .map_or(Exit::Failure, Failed::exit)
    }
}

#[derive(Debug)]
pub struct Failed {
    exit: Exit,
    why: String,
    diagnostic: Option<Box<wire::Diagnostic>>,
}

impl Failed {
    pub fn new(exit: Exit, why: impl Into<String>) -> Self {
        Self {
            exit,
            why: why.into(),
            diagnostic: None,
        }
    }

    pub fn diagnosed(exit: Exit, diagnostic: wire::Diagnostic) -> Self {
        Self {
            exit,
            why: crate::diagnostic::message(&diagnostic).to_owned(),
            diagnostic: Some(Box::new(diagnostic)),
        }
    }

    pub fn exit(&self) -> Exit {
        self.exit
    }

    pub fn diagnostic(&self) -> Option<&wire::Diagnostic> {
        self.diagnostic.as_deref()
    }
}

impl fmt::Display for Failed {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.why)
    }
}

impl std::error::Error for Failed {}

#[cfg(test)]
mod tests {
    use anyhow::{Context as _, anyhow};

    use super::*;

    #[test]
    fn the_published_numbers_never_move() {
        let published: Vec<(u8, &str)> = Exit::ALL
            .iter()
            .map(|exit| (exit.code(), exit.name()))
            .collect();

        assert_eq!(
            published,
            [
                (0, "success"),
                (1, "failure"),
                (2, "usage"),
                (3, "unresolved"),
                (4, "rejected"),
                (5, "unavailable"),
                (78, "not_ready"),
            ]
        );
    }

    #[test]
    fn more_detail_keeps_the_exit_code() {
        let failed = anyhow!(Failed::new(Exit::Unresolved, "no workspace matches abc"))
            .context("showing the workspace")
            .context("and more besides");

        assert_eq!(Exit::of(&failed), Exit::Unresolved);
    }

    #[test]
    fn a_classified_context_classifies_the_error_it_wraps() {
        let failed: anyhow::Result<()> = Err(std::io::Error::other("refused"))
            .with_context(|| Failed::new(Exit::Unavailable, "reaching the control plane"));

        assert_eq!(Exit::of(&failed.expect_err("a failure")), Exit::Unavailable);
    }

    #[test]
    fn an_unclassified_failure_is_a_failure() {
        assert_eq!(
            Exit::of(&anyhow!("writing to standard output")),
            Exit::Failure
        );
    }
}
