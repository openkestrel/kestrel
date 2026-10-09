//! A refusal's typed reason and next steps, rendered for a terminal (ADR-0052).

use std::io::{BufRead, IsTerminal as _, Write};

use anyhow::{Context as _, Result};
use kestrel_operator_types as wire;
use reqwest::StatusCode;
use serde_json::json;

use crate::BINARY;
use crate::exit::{Exit, Failed};

const SERVICE: &str = "control plane";
const EVIDENCE: usize = 240;

/// What a next step is rendered against: the connection and command this invocation chose.
pub struct Invocation {
    pub control_plane: String,
    /// The control plane differs from the default, so every rendered command must name it.
    pub elsewhere: bool,
    pub operation: String,
    pub args: Vec<String>,
    pub json: bool,
}

pub fn message(diagnostic: &wire::Diagnostic) -> &str {
    use wire::Diagnostic as D;
    match diagnostic {
        D::MissingReferenceDiagnostic(d) => &d.message,
        D::AmbiguousReferenceDiagnostic(d) => &d.message,
        D::MalformedRequestDiagnostic(d) => &d.message,
        D::ForbiddenActionDiagnostic(d) => &d.message,
        D::StateConflictDiagnostic(d) => &d.message,
        D::ExpiredResourceDiagnostic(d) => &d.message,
        D::InvalidFieldDiagnostic(d) => &d.message,
        D::SetupGapDiagnostic(d) => &d.message,
        D::UnavailableDiagnostic(d) => &d.message,
        D::InstanceTimeoutDiagnostic(d) => &d.message,
        D::AuthenticationFailedDiagnostic(d) => &d.message,
        D::ExecutableMissingDiagnostic(d) => &d.message,
        D::UnknownFailureDiagnostic(d) => &d.message,
        D::ConnectionFailedDiagnostic(d) => &d.message,
        D::ClientFailureDiagnostic(d) => &d.message,
        D::UnknownResponseDiagnostic(d) => &d.message,
    }
}

fn next_steps(diagnostic: &wire::Diagnostic) -> &[wire::Action] {
    use wire::Diagnostic as D;
    match diagnostic {
        D::MissingReferenceDiagnostic(d) => &d.next_steps,
        D::AmbiguousReferenceDiagnostic(d) => &d.next_steps,
        D::MalformedRequestDiagnostic(d) => &d.next_steps,
        D::ForbiddenActionDiagnostic(d) => &d.next_steps,
        D::StateConflictDiagnostic(d) => &d.next_steps,
        D::ExpiredResourceDiagnostic(d) => &d.next_steps,
        D::InvalidFieldDiagnostic(d) => &d.next_steps,
        D::SetupGapDiagnostic(d) => &d.next_steps,
        D::UnavailableDiagnostic(d) => &d.next_steps,
        D::InstanceTimeoutDiagnostic(d) => &d.next_steps,
        D::AuthenticationFailedDiagnostic(d) => &d.next_steps,
        D::ExecutableMissingDiagnostic(d) => &d.next_steps,
        D::UnknownFailureDiagnostic(d) => &d.next_steps,
        D::ConnectionFailedDiagnostic(d) => &d.next_steps,
        D::ClientFailureDiagnostic(d) => &d.next_steps,
        D::UnknownResponseDiagnostic(d) => &d.next_steps,
    }
}

/// The kind decides the exit; the status only places the kinds a Client cannot classify.
pub fn exit_for(diagnostic: &wire::Diagnostic, status: StatusCode) -> Exit {
    use wire::Diagnostic as D;
    match diagnostic {
        D::MissingReferenceDiagnostic(_) | D::AmbiguousReferenceDiagnostic(_) => Exit::Unresolved,
        D::MalformedRequestDiagnostic(_)
        | D::ForbiddenActionDiagnostic(_)
        | D::StateConflictDiagnostic(_)
        | D::ExpiredResourceDiagnostic(_)
        | D::InvalidFieldDiagnostic(_) => Exit::Rejected,
        D::SetupGapDiagnostic(_) => Exit::NotReady,
        D::UnavailableDiagnostic(_)
        | D::InstanceTimeoutDiagnostic(_)
        | D::ConnectionFailedDiagnostic(_) => Exit::Unavailable,
        D::ClientFailureDiagnostic(_) => Exit::Failure,
        D::AuthenticationFailedDiagnostic(_)
        | D::ExecutableMissingDiagnostic(_)
        | D::UnknownFailureDiagnostic(_)
        | D::UnknownResponseDiagnostic(_) => by_status(status),
    }
}

/// The operator boundary answers 404 for a reference that resolves to no record or to several.
pub fn by_status(status: StatusCode) -> Exit {
    match status {
        StatusCode::NOT_FOUND => Exit::Unresolved,
        status if status.is_client_error() => Exit::Rejected,
        _ => Exit::Unavailable,
    }
}

/// What a request is, as far as a lost or unrecognised answer is concerned.
#[derive(Clone, Copy)]
pub struct Request<'a> {
    pub operation: &'a str,
    pub write: bool,
}

pub fn refused(
    status: StatusCode,
    retry_after: Option<i64>,
    body: &[u8],
    request: Request<'_>,
) -> Failed {
    if let Ok(diagnostic) = serde_json::from_slice::<wire::Diagnostic>(body) {
        return Failed::diagnosed(exit_for(&diagnostic, status), diagnostic);
    }
    let (message, evidence) = match serde_json::from_slice::<wire::Refusal>(body) {
        Ok(refusal) => (refusal.message, None),
        Err(_) => (
            format!("the control plane answered {status} with nothing kestrel recognises"),
            bounded(&String::from_utf8_lossy(body)),
        ),
    };
    let uncertain = status.is_server_error();
    Failed::diagnosed(
        by_status(status),
        unknown_response(
            message,
            request,
            Some(status),
            evidence,
            uncertain,
            retry_after,
        ),
    )
}

/// An answer that never arrived whole: a write it answered may already have taken effect.
pub fn unreadable(
    status: StatusCode,
    request: Request<'_>,
    error: &dyn std::fmt::Display,
) -> Failed {
    Failed::diagnosed(
        Exit::Unavailable,
        unknown_response(
            "the control plane's answer could not be read".to_owned(),
            request,
            Some(status),
            bounded(&error.to_string()),
            true,
            None,
        ),
    )
}

/// `sent` says whether the request may have reached the control plane before the connection
/// failed; one that never connected cannot have taken effect.
pub fn unreachable(control_plane: &str, request: Request<'_>, sent: bool) -> Failed {
    let mut next_steps = vec![wire::Action::CheckConnectionAction(
        wire::CheckConnectionAction {
            action: json!("check_connection"),
            service: SERVICE.to_owned(),
            compose: false,
        },
    )];
    if request.write && sent {
        next_steps.push(inspect_operation(request.operation, true));
    }
    Failed::diagnosed(
        Exit::Unavailable,
        wire::Diagnostic::ConnectionFailedDiagnostic(wire::ConnectionFailedDiagnostic {
            kind: json!("connection_failed"),
            message: format!("the control plane at {control_plane} could not be reached"),
            field: None,
            context: wire::ConnectionFailedContext {
                url: control_plane.to_owned(),
                operation: request.operation.to_owned(),
            },
            next_steps,
        }),
    )
}

fn unknown_response(
    message: String,
    request: Request<'_>,
    status: Option<StatusCode>,
    evidence: Option<String>,
    uncertain: bool,
    retry_after: Option<i64>,
) -> wire::Diagnostic {
    let next = if !uncertain {
        inspect_operation(request.operation, false)
    } else if request.write {
        inspect_operation(request.operation, true)
    } else {
        wire::Action::RetryReadAction(wire::RetryReadAction {
            action: json!("retry_read"),
            operation: request.operation.to_owned(),
            resource: None,
            retry_after_seconds: retry_after,
        })
    };
    wire::Diagnostic::UnknownResponseDiagnostic(wire::UnknownResponseDiagnostic {
        kind: json!("unknown_response"),
        message,
        field: None,
        context: wire::UnknownResponseContext {
            service: SERVICE.to_owned(),
            operation: request.operation.to_owned(),
            status: status.map(|status| i64::from(status.as_u16())),
            evidence,
        },
        next_steps: vec![next],
    })
}

fn inspect_operation(operation: &str, uncertain: bool) -> wire::Action {
    wire::Action::InspectOperationAction(wire::InspectOperationAction {
        action: json!("inspect_operation"),
        operation: operation.to_owned(),
        resource: None,
        uncertain,
    })
}

/// Evidence is a bounded single line, so an answer can neither flood a terminal nor steer it.
fn bounded(text: &str) -> Option<String> {
    let line: String = text
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .filter(|character| !character.is_control())
        .collect();
    if line.is_empty() {
        return None;
    }
    let mut kept: String = line.chars().take(EVIDENCE).collect();
    if kept.len() < line.len() {
        kept.push('…');
    }
    Some(kept)
}

/// A failure nothing classified becomes a Client diagnostic that still offers a next step.
fn fallback(error: &anyhow::Error, exit: Exit, invocation: &Invocation) -> wire::Diagnostic {
    let message = format!("{error:#}");
    if exit == Exit::Unavailable {
        return wire::Diagnostic::ConnectionFailedDiagnostic(wire::ConnectionFailedDiagnostic {
            kind: json!("connection_failed"),
            message,
            field: None,
            context: wire::ConnectionFailedContext {
                url: invocation.control_plane.clone(),
                operation: invocation.operation.clone(),
            },
            next_steps: vec![
                wire::Action::CheckConnectionAction(wire::CheckConnectionAction {
                    action: json!("check_connection"),
                    service: SERVICE.to_owned(),
                    compose: false,
                }),
                inspect_operation(&invocation.operation, true),
            ],
        });
    }
    wire::Diagnostic::ClientFailureDiagnostic(wire::ClientFailureDiagnostic {
        kind: json!("client_failure"),
        message,
        field: None,
        context: wire::ClientFailureContext {
            operation: invocation.operation.clone(),
            evidence: None,
        },
        next_steps: vec![inspect_operation(
            &invocation.operation,
            exit == Exit::Failure,
        )],
    })
}

/// Shows the failure on stderr and offers its runnable steps when someone is there to choose.
/// The exit is the original failure's, whatever a chosen step does.
pub fn report(error: &anyhow::Error, invocation: &Invocation) -> Exit {
    let exit = Exit::of(error);
    let diagnostic = error
        .downcast_ref::<Failed>()
        .and_then(Failed::diagnostic)
        .cloned()
        .unwrap_or_else(|| fallback(error, exit, invocation));

    let mut stderr = std::io::stderr().lock();
    if invocation.json {
        let _ = writeln!(
            stderr,
            "{}",
            serde_json::to_string(&diagnostic).unwrap_or_default()
        );
        return exit;
    }
    let steps = steps(&diagnostic, invocation);
    let _ = write!(
        stderr,
        "error: {}\n{}",
        message(&diagnostic),
        described(&diagnostic, &steps, invocation)
    );
    if std::io::stdin().is_terminal() && stderr.is_terminal() {
        let chosen = chosen(
            &steps,
            invocation,
            &mut std::io::stdin().lock(),
            &mut stderr,
        );
        drop(stderr);
        match chosen.and_then(|argv| argv.map(ran).transpose()) {
            Ok(_) => {}
            Err(error) => eprintln!("error: {error:#}"),
        }
    }
    exit
}

fn ran(argv: Vec<String>) -> Result<()> {
    std::process::Command::new(std::env::current_exe().context("finding this kestrel")?)
        .args(argv)
        .status()
        .context("running the chosen step")?;
    Ok(())
}

/// A Session's durable failure, shown as part of a successful read.
pub fn inline(diagnostic: &wire::Diagnostic, invocation: &Invocation) -> String {
    let steps = steps(diagnostic, invocation);
    format!(
        "{}\n{}",
        message(diagnostic),
        described(diagnostic, &steps, invocation)
    )
    .trim_end()
    .to_owned()
}

fn described(diagnostic: &wire::Diagnostic, steps: &[Step], invocation: &Invocation) -> String {
    let mut said = String::new();
    for fact in facts(diagnostic) {
        said.push_str(&format!("  {fact}\n"));
    }
    if steps.is_empty() {
        return said;
    }
    said.push_str("next:\n");
    let mut number = 0;
    for step in steps {
        let Some(command) = &step.command else {
            said.push_str(&format!("  -  {}\n", step.says));
            continue;
        };
        number += 1;
        said.push_str(&format!(
            "  {number}. {}\n     {}\n",
            step.says,
            step.shown(command, invocation)
        ));
        let needed: Vec<String> = command
            .iter()
            .filter_map(|word| match word {
                Word::Asked(input) => Some(input.shown()),
                Word::Given(_) => None,
            })
            .collect();
        if !needed.is_empty() {
            said.push_str(&format!("     needs {}\n", needed.join(" and ")));
        }
        if let Some(consequence) = &step.consequence {
            said.push_str(&format!(
                "     {consequence} It happens only if you choose it.\n"
            ));
        }
    }
    said
}

/// What a person needs beyond the sentence, from the typed context alone.
fn facts(diagnostic: &wire::Diagnostic) -> Vec<String> {
    use wire::Diagnostic as D;
    match diagnostic {
        D::AuthenticationFailedDiagnostic(d) => {
            let context = &d.context;
            let mut facts = vec![ran_by(&context.harness, context.image.as_deref())];
            if let Some(sign_in) = &context.sign_in {
                facts.push(format!("signed in with {sign_in}"));
            }
            facts.push(match &context.evidence.method {
                Some(method) => format!(
                    "the harness asked for authentication (code {}) by {method}",
                    context.evidence.code
                ),
                None => format!(
                    "the harness asked for authentication (code {})",
                    context.evidence.code
                ),
            });
            if context.expired == Some(true) {
                facts.push("the sign-in has expired".to_owned());
            }
            if context.covered == Some(false) {
                facts.push("the sign-in does not cover this harness".to_owned());
            }
            facts
        }
        D::ExecutableMissingDiagnostic(d) => vec![
            ran_by(&d.context.harness, d.context.image.as_deref()),
            format!(
                "`{}` could not run: {}",
                d.context.evidence.command,
                d.context.evidence.error.kind.as_str()
            ),
        ],
        D::UnknownFailureDiagnostic(d) => d
            .context
            .evidence
            .iter()
            .map(|evidence| evidence.summary.clone())
            .collect(),
        D::UnknownResponseDiagnostic(d) => d.context.evidence.iter().cloned().collect(),
        D::UnavailableDiagnostic(d) => d
            .context
            .retry_after_seconds
            .map(|seconds| format!("retry after {seconds} seconds"))
            .into_iter()
            .collect(),
        _ => Vec::new(),
    }
}

fn ran_by(harness: &str, image: Option<&str>) -> String {
    match image {
        Some(image) => format!("harness {harness} in image {image}"),
        None => format!("harness {harness}"),
    }
}

/// One next step as this Client offers it: a command when it has one, said in words otherwise.
struct Step {
    says: String,
    command: Option<Vec<Word>>,
    /// The Organization the command applies to, when it is scoped.
    organization: Option<String>,
    /// Destructive: shown, and taken only on an explicit yes.
    consequence: Option<String>,
    /// The invocation's own arguments, which already name their scope and control plane.
    repeated: bool,
}

enum Word {
    Given(String),
    Asked(Input),
}

struct Input {
    name: String,
    flag: Option<String>,
}

impl Input {
    fn named(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            flag: flag_for(name),
        }
    }

    fn placeholder(&self) -> String {
        format!("<{}>", self.name.to_uppercase())
    }

    fn shown(&self) -> String {
        match &self.flag {
            Some(flag) => flag.clone(),
            None => self.placeholder(),
        }
    }
}

/// The CLI's bindings for an action's fixed input vocabulary; a name is positional.
fn flag_for(input: &str) -> Option<String> {
    match input {
        "name" => None,
        "repositories" | "repository" => Some("--repository".to_owned()),
        other => Some(format!("--{}", other.replace('_', "-"))),
    }
}

impl Step {
    fn said(says: impl Into<String>) -> Self {
        Self {
            says: says.into(),
            command: None,
            organization: None,
            consequence: None,
            repeated: false,
        }
    }

    fn run(says: impl Into<String>, command: Vec<Word>, organization: Option<String>) -> Self {
        Self {
            command: Some(command),
            organization,
            ..Self::said(says)
        }
    }

    fn scope(&self, invocation: &Invocation) -> Vec<String> {
        if self.repeated {
            return Vec::new();
        }
        let mut words = Vec::new();
        if let Some(organization) = &self.organization {
            words.extend(["--organization".to_owned(), organization.clone()]);
        }
        if invocation.elsewhere {
            words.extend([
                "--control-plane".to_owned(),
                invocation.control_plane.clone(),
            ]);
        }
        words
    }

    fn shown(&self, command: &[Word], invocation: &Invocation) -> String {
        let mut shown = vec![BINARY.to_owned()];
        for word in command {
            match word {
                Word::Given(value) => shown.push(quoted(value)),
                Word::Asked(input) => {
                    shown.extend(input.flag.clone());
                    shown.push(input.placeholder());
                }
            }
        }
        shown.extend(self.scope(invocation).iter().map(|word| quoted(word)));
        shown.join(" ")
    }
}

fn given(words: &[&str]) -> Vec<Word> {
    words
        .iter()
        .map(|word| Word::Given((*word).to_owned()))
        .collect()
}

/// A value the action already holds, or the input it says must be collected.
fn held_or_asked(
    flag: Option<&str>,
    value: Option<&str>,
    name: &str,
    missing: &[String],
) -> Vec<Word> {
    match value {
        Some(value) if !missing.iter().any(|input| input == name) => flag
            .map(|flag| Word::Given(flag.to_owned()))
            .into_iter()
            .chain([Word::Given(value.to_owned())])
            .collect(),
        _ => vec![Word::Asked(Input::named(name))],
    }
}

fn steps(diagnostic: &wire::Diagnostic, invocation: &Invocation) -> Vec<Step> {
    let mut steps = Vec::new();
    if let wire::Diagnostic::AmbiguousReferenceDiagnostic(d) = diagnostic {
        for candidate in &d.context.candidates {
            let (command, organization) = inspection(
                &d.context.resource,
                Some(&candidate.id),
                d.context.organization.as_deref(),
            );
            steps.push(Step::run(
                format!(
                    "inspect the {} {} ({})",
                    noun(&d.context.resource),
                    candidate.name,
                    candidate.id
                ),
                command,
                organization,
            ));
        }
    }
    steps.extend(
        next_steps(diagnostic)
            .iter()
            .map(|action| step(action, invocation)),
    );
    steps
}

fn step(action: &wire::Action, invocation: &Invocation) -> Step {
    use wire::Action as A;
    match action {
        A::NameOperatorAction(_) => Step::said("name the Operator, then try again"),
        A::DeclareOrganizationAction(a) => {
            let mut command = given(&["organization", "declare"]);
            command.extend(held_or_asked(None, a.name.as_deref(), "name", &a.missing));
            Step::run("declare the Organization", command, None)
        }
        A::DeclareProjectAction(a) => {
            let mut command = given(&["project", "declare"]);
            command.extend(held_or_asked(None, a.name.as_deref(), "name", &a.missing));
            match &a.repositories {
                Some(repositories)
                    if !repositories.is_empty()
                        && !a.missing.iter().any(|input| input == "repositories") =>
                {
                    for repository in repositories {
                        command.extend(given(&["--repository", repository]));
                    }
                }
                _ => command.push(Word::Asked(Input::named("repository"))),
            }
            command.extend(held_or_asked(
                Some("--branch"),
                a.branch.as_deref(),
                "branch",
                &a.missing,
            ));
            Step::run("declare the Project", command, Some(a.organization.clone()))
        }
        A::DeclareAgentAction(a) => {
            let mut command = given(&["agent", "declare"]);
            command.extend(held_or_asked(None, a.name.as_deref(), "name", &a.missing));
            command.extend(held_or_asked(
                Some("--harness"),
                a.harness.as_deref(),
                "harness",
                &a.missing,
            ));
            Step::run("declare the Agent", command, Some(a.organization.clone()))
        }
        A::DeclareSubscriptionProfileAction(a) => {
            let mut command = given(&["profile", "declare"]);
            command.extend(held_or_asked(None, a.name.as_deref(), "name", &a.missing));
            command.extend(held_or_asked(
                Some("--owner"),
                a.owner.as_deref(),
                "owner",
                &a.missing,
            ));
            Step::run(
                "declare the Subscription Profile",
                command,
                Some(a.organization.clone()),
            )
        }
        A::SetProviderCredentialAction(a) => Step::run(
            format!(
                "hold the Provider Credential {}; its value is read from standard input",
                a.name
            ),
            given(&["credential", "set", &a.name]),
            Some(a.organization.clone()),
        ),
        A::InspectResourceAction(a) => {
            let (command, organization) = inspection(
                &a.resource,
                a.reference.as_deref(),
                a.organization.as_deref(),
            );
            let says = match &a.reference {
                Some(reference) => format!("inspect the {} {reference}", noun(&a.resource)),
                None => format!("list the {}s", noun(&a.resource)),
            };
            Step::run(says, command, organization)
        }
        A::ListResourcesAction(a) => {
            let (command, organization) = listing(&a.resource, a.organization.as_deref());
            Step::run(
                format!("list the {}s", noun(&a.resource)),
                command,
                organization,
            )
        }
        A::StopSessionAction(a) => Step {
            consequence: Some(a.consequence.clone()),
            ..Step::run(
                format!("stop the session {}", a.session),
                given(&["session", "stop", &a.session]),
                Some(a.organization.clone()),
            )
        },
        A::EnqueueSessionAction(a) => {
            let mut command = given(&["session", "enqueue", "--workspace", &a.workspace]);
            command.extend(
                a.missing
                    .iter()
                    .map(|input| Word::Asked(Input::named(input))),
            );
            Step::run(
                format!("enqueue a session in the workspace {}", a.workspace),
                command,
                Some(a.organization.clone()),
            )
        }
        A::ReleaseInstanceAction(a) => Step {
            consequence: Some(a.consequence.clone()),
            ..Step::run(
                format!("release the workspace {}'s instance", a.workspace),
                given(&["instance", "release", &a.workspace]),
                Some(a.organization.clone()),
            )
        },
        A::CorrectFieldAction(a) => corrected(a, invocation),
        A::SignInAction(a) => {
            let mut says = match &a.harness {
                Some(harness) => format!("sign in to {harness}"),
                None => "sign in".to_owned(),
            };
            if let Some(method) = &a.method {
                says.push_str(&format!(" by {method}"));
            }
            if let Some(sign_in) = &a.sign_in {
                says.push_str(&format!(" and hold it in {sign_in}"));
            }
            Step::said(says)
        }
        A::InspectHarnessImageAction(a) => Step::said(format!(
            "check that {} provides {}{}",
            a.image.as_deref().unwrap_or("the image"),
            a.command.as_deref().map_or_else(
                || "the harness".to_owned(),
                |command| format!("`{command}`")
            ),
            a.harness
                .as_deref()
                .map_or_else(String::new, |harness| format!(" for {harness}")),
        )),
        A::CheckConnectionAction(a) => {
            let mut says = format!(
                "check that the {} at {} is running, or point --control-plane or \
                 KESTREL_CONTROL_PLANE at the one that is",
                a.service, invocation.control_plane
            );
            if a.compose {
                says.push_str("; `docker compose ps` shows whether it is up");
            }
            Step::said(says)
        }
        A::RetryReadAction(a) => Step {
            repeated: true,
            ..Step::run(
                match a.retry_after_seconds {
                    Some(seconds) => format!("read again after {seconds} seconds"),
                    None => "read again".to_owned(),
                },
                invocation
                    .args
                    .iter()
                    .map(|arg| Word::Given(arg.clone()))
                    .collect(),
                None,
            )
        },
        A::InspectOperationAction(a) if a.uncertain => Step::said(format!(
            "{} may already have taken effect; look before running it again",
            a.operation
        )),
        A::InspectOperationAction(_) => {
            let mut command: Vec<Word> = invocation
                .operation
                .split(' ')
                .map(|word| Word::Given(word.to_owned()))
                .collect();
            command.push(Word::Given("--help".to_owned()));
            Step::run(
                format!("see what `{BINARY} {}` accepts", invocation.operation),
                command,
                None,
            )
        }
    }
}

/// The invocation again with only the refused value asked for, so the rest of it is kept.
fn corrected(action: &wire::CorrectFieldAction, invocation: &Invocation) -> Step {
    let mut says = format!("correct {}: {}", action.field, action.constraint);
    if let Some(allowed) = &action.allowed_values {
        says.push_str(&format!("; one of {}", allowed.join(", ")));
    }
    let Some(flag) = flag_for(&action.field) else {
        return Step::said(says);
    };
    let mut command = Vec::new();
    let mut found = false;
    let mut args = invocation.args.iter();
    while let Some(arg) = args.next() {
        if *arg == flag {
            args.next();
        } else if !arg.starts_with(&format!("{flag}=")) {
            command.push(Word::Given(arg.clone()));
            continue;
        }
        if !found {
            command.push(Word::Asked(Input::named(&action.field)));
            found = true;
        }
    }
    if !found {
        return Step::said(says);
    }
    Step {
        repeated: true,
        ..Step::run(says, command, None)
    }
}

/// The command that shows a record, and the Organization it is scoped to.
fn inspection(
    resource: &wire::Resource,
    reference: Option<&str>,
    organization: Option<&str>,
) -> (Vec<Word>, Option<String>) {
    use wire::Resource as R;
    let Some(reference) = reference else {
        return listing(resource, organization);
    };
    let scoped = organization.map(str::to_owned);
    match resource {
        R::Organization => (given(&["status"]), Some(reference.to_owned())),
        R::Workspace => (given(&["workspace", "show", reference]), scoped),
        R::Session => (given(&["session", "show", reference]), scoped),
        R::Trigger => (given(&["trigger", "show", reference]), scoped),
        R::Event => (given(&["event", "show", reference]), None),
        resource => listing(resource, organization),
    }
}

fn listing(resource: &wire::Resource, organization: Option<&str>) -> (Vec<Word>, Option<String>) {
    use wire::Resource as R;
    let scoped = organization.map(str::to_owned);
    let command = match resource {
        R::Organization => return (given(&["organization", "list"]), None),
        R::Project => "project",
        R::Agent => "agent",
        R::SubscriptionProfile => "profile",
        R::ProviderCredential => "credential",
        R::Integration => "integration",
        R::Trigger => "trigger",
        R::Event => "event",
        R::Instance => "instance",
        R::Workspace | R::Session | R::HeldMessage | R::TranscriptPayload => "workspace",
    };
    (given(&[command, "list"]), scoped)
}

fn noun(resource: &wire::Resource) -> &'static str {
    use wire::Resource as R;
    match resource {
        R::Organization => "organization",
        R::Project => "project",
        R::Agent => "agent",
        R::SubscriptionProfile => "subscription profile",
        R::ProviderCredential => "provider credential",
        R::Integration => "integration",
        R::Trigger => "trigger",
        R::Event => "event",
        R::Workspace => "workspace",
        R::Session => "session",
        R::Instance => "instance",
        R::HeldMessage => "held message",
        R::TranscriptPayload => "transcript payload",
    }
}

/// Nothing runs unless a step is chosen, every missing input is typed, and a destructive step
/// gets an explicit yes. Answers the arguments to run `kestrel` with.
fn chosen(
    steps: &[Step],
    invocation: &Invocation,
    input: &mut impl BufRead,
    output: &mut impl Write,
) -> Result<Option<Vec<String>>> {
    let runnable: Vec<&Step> = steps.iter().filter(|step| step.command.is_some()).collect();
    if runnable.is_empty() {
        return Ok(None);
    }
    let step = loop {
        write!(output, "run a step? its number, or nothing for none: ")?;
        output.flush()?;
        let Some(answer) = answered(input, output)? else {
            return Ok(None);
        };
        if answer.is_empty() {
            return Ok(None);
        }
        if let Some(step) = answer
            .parse::<usize>()
            .ok()
            .and_then(|number| runnable.get(number.wrapping_sub(1)))
        {
            break *step;
        }
    };

    let mut argv = Vec::new();
    for word in step.command.iter().flatten() {
        match word {
            Word::Given(value) => argv.push(value.clone()),
            Word::Asked(asked) => {
                write!(output, "{}: ", asked.name)?;
                output.flush()?;
                let Some(value) = answered(input, output)?.filter(|value| !value.is_empty()) else {
                    return Ok(None);
                };
                argv.extend(asked.flag.clone());
                argv.push(value);
            }
        }
    }
    if !step.repeated {
        argv.extend(step.scope(invocation));
        if !invocation.elsewhere {
            argv.extend([
                "--control-plane".to_owned(),
                invocation.control_plane.clone(),
            ]);
        }
    }
    if let Some(consequence) = &step.consequence {
        write!(output, "{consequence} {}? [y/N] ", step.says)?;
        output.flush()?;
        let yes = answered(input, output)?
            .is_some_and(|answer| matches!(answer.to_lowercase().as_str(), "y" | "yes"));
        if !yes {
            return Ok(None);
        }
    }
    Ok(Some(argv))
}

fn answered(input: &mut impl BufRead, output: &mut impl Write) -> Result<Option<String>> {
    let mut line = String::new();
    if input.read_line(&mut line).context("reading the answer")? == 0 {
        writeln!(output)?;
        return Ok(None);
    }
    Ok(Some(line.trim().to_owned()))
}

pub(crate) fn quoted(value: &str) -> String {
    if !value.is_empty()
        && value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "_-./:".contains(character))
    {
        value.to_owned()
    } else {
        format!("'{}'", value.replace('\'', "'\"'\"'"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn invocation() -> Invocation {
        Invocation {
            control_plane: "http://127.0.0.1:7718".to_owned(),
            elsewhere: false,
            operation: "workspace open".to_owned(),
            args: [
                "workspace",
                "open",
                "--project",
                "absent",
                "--agent",
                "worker",
            ]
            .map(str::to_owned)
            .to_vec(),
            json: false,
        }
    }

    fn diagnostic(value: serde_json::Value) -> wire::Diagnostic {
        serde_json::from_value(value).expect("a diagnostic")
    }

    fn missing_project() -> wire::Diagnostic {
        diagnostic(json!({
            "kind": "missing_reference",
            "message": "no project named absent in the organization Acme East",
            "field": null,
            "context": { "resource": "project", "reference": "absent", "organization": "Acme East" },
            "next_steps": [{
                "action": "declare_project",
                "organization": "Acme East",
                "name": "absent",
                "repositories": null,
                "branch": null,
                "missing": ["repositories", "branch"],
            }],
        }))
    }

    fn stopping() -> wire::Diagnostic {
        diagnostic(json!({
            "kind": "state_conflict",
            "message": "the workspace w already has the session s",
            "field": null,
            "context": {
                "operation": "enqueue_session",
                "resource": "workspace",
                "reference": "w",
                "organization": "acme",
                "state": "session_in_flight",
                "holding_session": "s",
            },
            "next_steps": [
                {
                    "action": "inspect_resource",
                    "resource": "session",
                    "reference": "s",
                    "organization": "acme",
                },
                {
                    "action": "stop_session",
                    "organization": "acme",
                    "session": "s",
                    "consequence": "Stopping the session now records it failed.",
                    "effect": "fails_session",
                    "requires_choice": true,
                },
            ],
        }))
    }

    fn rendered(diagnostic: &wire::Diagnostic, invocation: &Invocation) -> String {
        described(diagnostic, &steps(diagnostic, invocation), invocation)
    }

    fn chose(diagnostic: &wire::Diagnostic, typed: &str) -> (Option<Vec<String>>, String) {
        let invocation = invocation();
        let mut said = Vec::new();
        let argv = chosen(
            &steps(diagnostic, &invocation),
            &invocation,
            &mut typed.as_bytes(),
            &mut said,
        )
        .expect("a choice");
        (argv, String::from_utf8(said).expect("text"))
    }

    #[test]
    fn the_kind_decides_the_exit_whatever_the_status() {
        let cases = [
            (missing_project(), StatusCode::CONFLICT, Exit::Unresolved),
            (stopping(), StatusCode::NOT_FOUND, Exit::Rejected),
            (
                diagnostic(json!({
                    "kind": "setup_gap",
                    "message": "name the Operator",
                    "field": null,
                    "context": {
                        "prerequisite": "operator",
                        "resource": null,
                        "reference": null,
                        "organization": null,
                        "harness": null,
                        "method": null,
                        "sign_in": null,
                    },
                    "next_steps": [{ "action": "name_operator", "name": null, "missing": ["name"] }],
                })),
                StatusCode::CONFLICT,
                Exit::NotReady,
            ),
        ];

        for (diagnostic, status, exit) in cases {
            assert_eq!(exit_for(&diagnostic, status), exit);
        }
    }

    #[test]
    fn rewording_a_refusal_changes_nothing_but_its_sentence() {
        let mut reworded = serde_json::to_value(missing_project()).expect("json");
        reworded["message"] = json!("there is no such Project as the stop session thing");
        let reworded = diagnostic(reworded);

        assert_eq!(
            exit_for(&reworded, StatusCode::NOT_FOUND),
            exit_for(&missing_project(), StatusCode::NOT_FOUND)
        );
        assert_eq!(
            rendered(&reworded, &invocation()),
            rendered(&missing_project(), &invocation())
        );
    }

    #[test]
    fn missing_inputs_are_named_as_flags_and_never_guessed() {
        let said = rendered(&missing_project(), &invocation());

        assert!(
            said.contains(
                "kestrel project declare absent --repository <REPOSITORY> --branch <BRANCH> \
                 --organization 'Acme East'"
            ),
            "{said}"
        );
        assert!(said.contains("needs --repository and --branch"), "{said}");
        assert!(!said.contains("git"), "{said}");
    }

    #[test]
    fn a_chosen_control_plane_is_kept() {
        let invocation = Invocation {
            control_plane: "http://10.0.0.2:7718".to_owned(),
            elsewhere: true,
            ..invocation()
        };

        let said = rendered(&missing_project(), &invocation);

        assert!(
            said.contains("--organization 'Acme East' --control-plane http://10.0.0.2:7718"),
            "{said}"
        );
    }

    #[test]
    fn a_terminal_collects_what_is_missing() {
        let (argv, said) = chose(&missing_project(), "1\nhttps://example.com/repo\nmain\n");

        assert_eq!(
            argv.expect("the step should run"),
            [
                "project",
                "declare",
                "absent",
                "--repository",
                "https://example.com/repo",
                "--branch",
                "main",
                "--organization",
                "Acme East",
                "--control-plane",
                "http://127.0.0.1:7718",
            ]
        );
        assert!(said.contains("repository: "), "{said}");
    }

    #[test]
    fn nothing_runs_unless_a_step_is_chosen() {
        assert_eq!(chose(&missing_project(), "\n").0, None);
        assert_eq!(chose(&missing_project(), "").0, None);
        assert_eq!(chose(&missing_project(), "1\n\n").0, None);
    }

    #[test]
    fn a_destructive_step_shows_its_consequence_and_needs_a_yes() {
        let said = rendered(&stopping(), &invocation());
        assert!(
            said.contains("kestrel session stop s --organization acme"),
            "{said}"
        );
        assert!(
            said.contains(
                "Stopping the session now records it failed. It happens only if you choose it."
            ),
            "{said}"
        );

        let (declined, asked) = chose(&stopping(), "2\n\n");
        assert_eq!(declined, None);
        assert!(asked.contains("[y/N]"), "{asked}");

        let (stopped, _) = chose(&stopping(), "2\nyes\n");
        assert_eq!(
            stopped.expect("the stop should run")[..3],
            ["session", "stop", "s"]
        );
    }

    #[test]
    fn several_candidates_are_each_offered_for_inspection() {
        let ambiguous = diagnostic(json!({
            "kind": "ambiguous_reference",
            "message": "ab matches more than one session",
            "field": null,
            "context": {
                "resource": "session",
                "reference": "ab",
                "organization": "acme",
                "candidates": [
                    { "id": "ab12", "name": "brisk-otter" },
                    { "id": "ab34", "name": "calm-heron" },
                ],
            },
            "next_steps": [{ "action": "list_resources", "resource": "session", "organization": "acme" }],
        }));

        let said = rendered(&ambiguous, &invocation());

        assert!(
            said.contains("kestrel session show ab12 --organization acme"),
            "{said}"
        );
        assert!(
            said.contains("kestrel session show ab34 --organization acme"),
            "{said}"
        );
        assert!(said.contains("calm-heron"), "{said}");
    }

    #[test]
    fn a_corrected_field_keeps_the_rest_of_the_invocation() {
        let invalid = diagnostic(json!({
            "kind": "invalid_field",
            "message": "agent must name a declared agent",
            "field": "agent",
            "context": { "field": "agent", "constraint": "matches_declared", "allowed_values": null },
            "next_steps": [{
                "action": "correct_field",
                "operation": "open_workspace",
                "field": "agent",
                "constraint": "matches_declared",
                "allowed_values": null,
                "resource": null,
            }],
        }));

        let said = rendered(&invalid, &invocation());

        assert!(
            said.contains("kestrel workspace open --project absent --agent <AGENT>\n"),
            "{said}"
        );
    }

    #[test]
    fn an_unrecognised_answer_is_bounded_evidence_with_an_inspection() {
        let write = Request {
            operation: "session stop",
            write: true,
        };
        let failed = refused(
            StatusCode::BAD_GATEWAY,
            None,
            format!("<html>\u{1b}[31m{}</html>", "x".repeat(1000)).as_bytes(),
            write,
        );

        assert_eq!(failed.exit(), Exit::Unavailable);
        let diagnostic = failed.diagnostic().expect("a diagnostic");
        let said = rendered(diagnostic, &invocation());
        assert!(said.contains("may already have taken effect"), "{said}");
        assert!(!said.contains('\u{1b}'), "{said}");
        assert!(said.len() < 600, "{said}");
    }

    #[test]
    fn a_lost_read_offers_a_retry_with_its_delay_and_a_lost_write_never_does() {
        let read = refused(
            StatusCode::SERVICE_UNAVAILABLE,
            Some(3),
            b"",
            Request {
                operation: "session show",
                write: false,
            },
        );
        let said = rendered(read.diagnostic().expect("a diagnostic"), &invocation());
        assert!(said.contains("read again after 3 seconds"), "{said}");

        let write = unreachable(
            "http://127.0.0.1:7718",
            Request {
                operation: "session stop",
                write: true,
            },
            true,
        );
        let said = rendered(write.diagnostic().expect("a diagnostic"), &invocation());
        assert!(!said.contains("read again"), "{said}");
        assert!(said.contains("may already have taken effect"), "{said}");
    }

    #[test]
    fn a_plain_refusal_keeps_its_sentence_and_its_status() {
        let failed = refused(
            StatusCode::CONFLICT,
            None,
            br#"{"message":"that name is taken"}"#,
            Request {
                operation: "integration register",
                write: true,
            },
        );

        assert_eq!(failed.exit(), Exit::Rejected);
        assert_eq!(
            message(failed.diagnostic().expect("a diagnostic")),
            "that name is taken"
        );
    }
}
