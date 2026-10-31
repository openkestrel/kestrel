//! Opt-in smoke checks that each harness in the `kestrel-dev` image reaches a real model on a
//! person's own subscription, then again after the control plane and the Instance are both
//! replaced (ADR-0025). Each reads its login from the variables it names and fails, rather than
//! passing vacuously, when they are unset; USAGE.md says how to run them.

#[path = "subscription/diagnosis.rs"]
mod diagnosis;
#[path = "subscription/login.rs"]
mod login;
#[path = "subscription/secrets.rs"]
mod secrets;
mod support;

use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use kestrel::domain::{Exit, Session, SessionState, Workspace};
use kestrel::log;
use kestrel::profile::Entry;
use support::Kestrel;
use support::fixture::Fixture;
use support::image::{self, Container};

use diagnosis::{Failure, Round, diagnosed};
use login::{handed_back, secrets};

const PATIENCE: Duration = Duration::from_secs(300);
/// Asked for something the prompt does not contain, so an error the harness echoes back as
/// its answer is never mistaken for a model's.
const PROMPT: &str = "Reply with the word harrier spelled backwards, in lowercase, and nothing else. Do not use any tools.";
const ANSWER: &str = "reirrah";
const ORGANIZATION: &str = "smoke";
const PROJECT: &str = "smoke";
const AGENT: &str = "smoke";
const PROFILE: &str = "smoke";

struct Subject {
    harness: &'static str,
    command: &'static str,
    model: Option<String>,
    variables: Vec<(&'static str, String)>,
    files: Vec<Login>,
}

/// A login file read from the host, handed back there once the smoke is over so a refresh the
/// harness made does not leave the person holding a token it revoked.
struct Login {
    path: &'static str,
    host: PathBuf,
    as_read: String,
}

fn required(variable: &str, what: &str) -> String {
    std::env::var(variable)
        .ok()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| panic!("set {variable} to {what} to run this smoke check"))
}

#[tokio::test]
#[ignore = "calls a real model on a person's Codex subscription"]
async fn codex_answers_on_a_chatgpt_login_before_and_after_a_restart() {
    let host = PathBuf::from(required(
        "KESTREL_SMOKE_CODEX_AUTH",
        "the path of a file-backed Codex auth.json, which the refreshed login is written back to",
    ));
    let as_read = fs::read_to_string(&host).expect("the Codex login should read");

    smoke(Subject {
        harness: "codex",
        command: "codex-acp",
        model: None,
        variables: Vec::new(),
        files: vec![Login {
            path: ".codex/auth.json",
            host,
            as_read,
        }],
    })
    .await;
}

#[tokio::test]
#[ignore = "waits for a person to approve a real Codex device code"]
async fn codex_device_authorization_saves_the_login_a_person_approves() {
    required(
        "KESTREL_SMOKE_CODEX_DEVICE_AUTH",
        "1, then open the printed page and enter its code",
    );
    let kestrel =
        Kestrel::dispatching_harnesses_in(image::development(), &[("codex", "codex-acp")]).await;
    let client = reqwest::Client::new();
    let operator = kestrel.operator();
    client
        .put(format!("{operator}/operator/operator"))
        .json(&serde_json::json!({"name": "smoke"}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    let relay: serde_json::Value = client
        .post(format!(
            "{operator}/operator/organizations/smoke/sign-ins/codex/codex-device-auth/relay"
        ))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let address = format!(
        "{operator}/operator/organizations/smoke/sign-ins/codex/codex-device-auth/relay/{}",
        relay["id"].as_str().unwrap()
    );
    let mut shown = false;
    let finished = loop {
        let current: serde_json::Value = client
            .get(&address)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let progress = &current["progress"];
        if progress["state"] == "awaiting_browser" && !shown {
            eprintln!("open {} and enter {}", progress["url"], progress["code"]);
            shown = true;
        }
        if matches!(
            progress["state"].as_str(),
            Some("completed" | "failed" | "expired" | "cancelled")
        ) {
            break current;
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    };
    kestrel.teardown().await;
    let saved = &finished["progress"]["sign_in"];
    assert_eq!(
        saved["authentication"]["state"], "login_completed",
        "{finished}"
    );
    assert_eq!(saved["model_use"], serde_json::json!([]));
}

#[tokio::test]
#[ignore = "calls a real model on a person's OpenCode Go subscription"]
async fn opencode_answers_on_an_opencode_go_key_before_and_after_a_restart() {
    let key = required("KESTREL_SMOKE_OPENCODE_API_KEY", "an OpenCode Go key");
    let model = required(
        "KESTREL_SMOKE_OPENCODE_MODEL",
        "an OpenCode Go model, such as opencode-go/glm-5.3",
    );
    assert!(
        model.starts_with("opencode-go/"),
        "{model} is not an OpenCode Go model, so it would not exercise the subscription"
    );

    smoke(Subject {
        harness: "opencode",
        command: "opencode acp --print-logs",
        model: Some(model),
        variables: vec![("OPENCODE_API_KEY", key)],
        files: Vec::new(),
    })
    .await;
}

#[tokio::test]
#[ignore = "calls a real model on a person's Claude subscription"]
async fn claude_answers_on_a_claude_plan_token_before_and_after_a_restart() {
    let token = required(
        "KESTREL_SMOKE_CLAUDE_OAUTH_TOKEN",
        "a token `claude setup-token` printed",
    );

    smoke(Subject {
        harness: "claude",
        command: "claude-agent-acp",
        model: None,
        variables: vec![("CLAUDE_CODE_OAUTH_TOKEN", token)],
        files: Vec::new(),
    })
    .await;
}

/// The login is handed back and the fixture torn down before anything is asserted, so a
/// failing smoke still leaves the person's login where it found it.
async fn smoke(subject: Subject) {
    let kestrel = Kestrel::dispatching_harnesses_in(
        image::development(),
        &[(subject.harness, subject.command)],
    )
    .await;
    declared(&kestrel, &subject).await;

    let first = attempt(&kestrel, &subject, Round::BeforeTheRestart).await;
    let (kestrel, second) = match &first {
        Ok(session) => {
            let kestrel = kestrel.kill_and_restart().await;
            if let Some(instance) = &session.instance {
                Container::named(instance).destroy();
            }
            let second = attempt(&kestrel, &subject, Round::AfterTheRestart).await;
            (kestrel, Some(second))
        }
        Err(_) => (kestrel, None),
    };
    handed_back(&kestrel, &subject).await;
    kestrel.teardown().await;

    let harness = subject.harness;
    let first = first.unwrap_or_else(|failure| panic!("{harness} {failure}"));
    let second = second
        .expect("a second attempt follows a first that worked")
        .unwrap_or_else(|failure| panic!("{harness} {failure}"));
    assert_ne!(
        first.instance, second.instance,
        "{harness} answered after the restart on the Instance it answered on before it"
    );
}

async fn declared(kestrel: &Kestrel, subject: &Subject) {
    Fixture::acme()
        .organization(ORGANIZATION)
        .project(PROJECT)
        .without_repositories()
        .agent_name(AGENT)
        .harness(subject.harness)
        .model(subject.model.as_deref())
        .declare(kestrel)
        .await;
    kestrel
        .declare_profile(ORGANIZATION, PROFILE, "smoke")
        .await
        .expect("the profile should declare");
    for (name, value) in &subject.variables {
        let entry = Entry::variable(name).expect("a variable");
        kestrel
            .hold_in_profile(ORGANIZATION, PROFILE, &entry, value)
            .await;
    }
    for login in &subject.files {
        let entry = Entry::file(login.path).expect("a file");
        kestrel
            .hold_in_profile(ORGANIZATION, PROFILE, &entry, &login.as_read)
            .await;
    }
}

async fn attempt(kestrel: &Kestrel, subject: &Subject, round: Round) -> Result<Session, Failure> {
    let workspace = kestrel
        .open_workspace_with(ORGANIZATION, PROJECT, AGENT, PROFILE)
        .await;
    let session = kestrel.post(workspace.id, "operator", PROMPT).await;
    let (session, answered) = settled(kestrel, session).await;
    let said = said_by_the_agent(kestrel, &workspace).await;
    if session.state != SessionState::Ended {
        kestrel.stop_session(session.id).await;
    }

    let failed = match &session.exit {
        Some(Exit::Failed { because }) => Some(because.clone()),
        _ => None,
    };
    if answered && failed.is_none() && said.to_lowercase().contains(ANSWER) {
        return Ok(session);
    }

    let evidence = format!(
        "{}\nthe agent said: {said}",
        match (&failed, answered) {
            (Some(because), _) => format!("the session failed: {because}"),
            (None, true) => format!("the agent answered without {ANSWER:?}"),
            (None, false) => format!("the agent had not answered within {PATIENCE:?}"),
        }
    );

    Err(Failure {
        round,
        problem: diagnosed(round, answered, &evidence),
        evidence: secrets(kestrel, subject).await.redacted(&evidence),
    })
}

/// Waited for without panicking, because a smoke that times out still hands its login back.
async fn settled(kestrel: &Kestrel, session: Session) -> (Session, bool) {
    let deadline = tokio::time::Instant::now() + PATIENCE;

    loop {
        let answered = kestrel
            .turns(session.id)
            .await
            .iter()
            .any(|turn| turn.answered_at.is_some());
        let session = kestrel.session(session.id).await;
        if answered
            || session.state == SessionState::Ended
            || tokio::time::Instant::now() >= deadline
        {
            return (session, answered);
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

async fn said_by_the_agent(kestrel: &Kestrel, workspace: &Workspace) -> String {
    kestrel
        .transcript(workspace.id)
        .await
        .into_iter()
        .filter_map(|entry| match entry.entry {
            log::Entry::Said {
                participant,
                message,
                ..
            } if participant == workspace.opened_with.name => Some(message),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}
