//! `git` as the test suites run it on the host: on the test's own terms, never the developer's
//! global configuration. A global `commit.gpgsign` with a signer that is locked, absent, or
//! prompting would otherwise fail a commit with nothing pointing at signing.

use std::process::Command;

pub const HERMETIC: [(&str, &str); 6] = [
    ("GIT_CONFIG_GLOBAL", "/dev/null"),
    ("GIT_CONFIG_NOSYSTEM", "1"),
    ("GIT_AUTHOR_NAME", "kestrel"),
    ("GIT_AUTHOR_EMAIL", "kestrel@example.com"),
    ("GIT_COMMITTER_NAME", "kestrel"),
    ("GIT_COMMITTER_EMAIL", "kestrel@example.com"),
];

pub fn command() -> Command {
    let mut git = Command::new("git");
    git.envs(HERMETIC);

    git
}

pub fn shell_exports() -> String {
    HERMETIC
        .iter()
        .map(|(name, value)| format!("export {name}={value}"))
        .collect::<Vec<_>>()
        .join("\n")
}
