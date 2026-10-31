use std::fmt;

pub(super) struct Failure {
    pub(super) round: Round,
    pub(super) problem: Problem,
    pub(super) evidence: String,
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let when = match self.round {
            Round::BeforeTheRestart => "before the restart",
            Round::AfterTheRestart => "after the restart",
        };
        let problem = match self.problem {
            Problem::HarnessLaunch => "a harness launch problem",
            Problem::Authentication => "an authentication problem",
            Problem::Entitlement => "an entitlement problem",
            Problem::Persistence => "a persistence problem: the login worked before the restart",
            Problem::Unclassified => {
                "a problem that is none of launch, authentication, entitlement or persistence"
            }
        };

        write!(f, "failed {when} with {problem}\n{}", self.evidence)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Round {
    BeforeTheRestart,
    AfterTheRestart,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Problem {
    HarnessLaunch,
    Authentication,
    Entitlement,
    Persistence,
    Unclassified,
}

/// Checked before authentication, because a refused plan can say 401 too.
const ENTITLEMENT: &[&str] = &[
    "only authorized for use with",
    "not included in your plan",
    "your plan does not",
    "usage limit",
    "forbidden",
    "403",
    "not entitled",
    "insufficient credits",
    "credit balance",
];
const AUTHENTICATION: &[&str] = &[
    "401",
    "unauthorized",
    "authentication_error",
    "authentication failed",
    "invalid api key",
    "invalid_api_key",
    "invalid_grant",
    "must be logged in",
    "not logged in",
    "login expired",
    "token expired",
    "refresh token",
    "/login",
    "auth_required",
    "authentication required",
    "failed to authenticate",
    "authenticates only at an interactive terminal",
];
const LAUNCH: &[&str] = &[
    "no such file",
    "command not found",
    "permission denied",
    "exec format error",
    "acp v1",
    "exited before",
    "names a harness",
];

pub(super) fn diagnosed(round: Round, answered: bool, evidence: &str) -> Problem {
    let evidence = evidence.to_lowercase();
    let says = |phrases: &[&str]| phrases.iter().any(|phrase| mentions(&evidence, phrase));

    if says(ENTITLEMENT) {
        Problem::Entitlement
    } else if says(AUTHENTICATION) {
        match round {
            Round::BeforeTheRestart => Problem::Authentication,
            Round::AfterTheRestart => Problem::Persistence,
        }
    } else if !answered && says(LAUNCH) {
        Problem::HarnessLaunch
    } else {
        Problem::Unclassified
    }
}

/// A session id holds `401` as readily as a status line does.
fn mentions(text: &str, phrase: &str) -> bool {
    let bounded = |c: Option<char>| c.is_none_or(|c| !c.is_alphanumeric());

    text.match_indices(phrase).any(|(at, _)| {
        bounded(text[..at].chars().next_back()) && bounded(text[at + phrase.len()..].chars().next())
    })
}

#[test]
fn a_rejected_login_is_an_authentication_problem() {
    for evidence in [
        "API Error: 401 {\"type\":\"error\",\"error\":{\"type\":\"authentication_error\"}}",
        "Invalid API key · Please run /login",
        "this agent must be logged in before it answers session/new",
        "Login expired · Please run /login",
        "Authentication required: provider authentication required: {}",
        "Failed to authenticate. API Error: 401 OAuth access token is invalid.",
    ] {
        assert_eq!(
            diagnosed(Round::BeforeTheRestart, false, evidence),
            Problem::Authentication,
            "{evidence}"
        );
    }
}

#[test]
fn a_login_the_plan_does_not_cover_is_an_entitlement_problem() {
    for evidence in [
        "This credential is only authorized for use with Claude Code and cannot be used for other API requests.",
        "You've hit your usage limit. Upgrade to Pro",
        "unexpected status 403 Forbidden",
        "the model opencode-go/glm-5.3 is not included in your plan",
    ] {
        assert_eq!(
            diagnosed(Round::BeforeTheRestart, false, evidence),
            Problem::Entitlement,
            "{evidence}"
        );
    }
}

#[test]
fn a_harness_that_never_came_up_is_a_launch_problem() {
    for evidence in [
        "No such file or directory (os error 2)",
        "kestrel speaks ACP v1, and this agent answered v0",
        "sh: 1: codex-acp: command not found",
    ] {
        assert_eq!(
            diagnosed(Round::BeforeTheRestart, false, evidence),
            Problem::HarnessLaunch,
            "{evidence}"
        );
    }
}

#[test]
fn a_login_refused_only_after_the_restart_is_a_persistence_problem() {
    assert_eq!(
        diagnosed(
            Round::AfterTheRestart,
            false,
            "Login expired · Please run /login"
        ),
        Problem::Persistence
    );
    assert_eq!(
        diagnosed(Round::AfterTheRestart, false, "No such file or directory"),
        Problem::HarnessLaunch
    );
}

#[test]
fn identifiers_are_not_status_codes() {
    assert_eq!(
        diagnosed(
            Round::BeforeTheRestart,
            true,
            "the session 01a0acf7-3f4d-7952-a4a0-f401a181b089 answered pelican"
        ),
        Problem::Unclassified
    );
}

#[test]
fn a_throttle_is_not_a_plan_refusal() {
    assert_eq!(
        diagnosed(
            Round::BeforeTheRestart,
            true,
            "429 Too Many Requests: rate limit exceeded, retry later"
        ),
        Problem::Unclassified
    );
}
