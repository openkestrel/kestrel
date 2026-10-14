mod api;
mod changes;
mod command;
mod diagnostic;
mod exit;
mod files;
mod filter;
mod output;
mod queue;
mod readiness;
mod scope;
mod shell;
mod sse;
mod start;
mod terminal;
mod transcript;
mod typed;
mod view;
mod work;

use std::io::{IsTerminal as _, Read as _};
use std::process::ExitCode;

use anyhow::{Context as _, Result, bail};
use clap::builder::NonEmptyStringValueParser;
use clap::error::{ContextKind, ContextValue};
use clap::parser::ValueSource;
use clap::{Args, CommandFactory as _, FromArgMatches as _, Parser, Subcommand};
use kestrel_operator_types as wire;
use reqwest::Url;
use serde::Serialize;
use serde_json::{Value, json};

use crate::api::ControlPlane;
use crate::command::Running;
use crate::command::{
    agent::*, credential::*, event::*, instance::*, integration::*, operator::*, organization::*,
    profile::*, project::*, session::*, trigger::*, workspace::*,
};
use crate::diagnostic::Invocation;
use crate::exit::{Exit, Failed};
use crate::output::Presentation;
use crate::scope::{Derived, Scope, Scoping, Source};

pub(crate) const BINARY: &str = "kestrel";
pub(crate) const CONTROL_PLANE_VARIABLE: &str = "KESTREL_CONTROL_PLANE";
pub(crate) const ORGANIZATION_VARIABLE: &str = "KESTREL_ORGANIZATION";

#[derive(Debug, Parser)]
#[command(
    name = BINARY,
    version,
    about = "Reach a kestrel control plane over its operator boundary. With no command, say whether work can start and what is missing when it cannot.",
    after_help = "Exit codes: `kestrel exit-codes` lists each one, what it means, and when to branch on it.",
    disable_help_subcommand = true
)]
struct Client {
    #[command(subcommand)]
    command: Option<Command>,

    /// The control plane's operator boundary
    #[arg(
        long,
        env = CONTROL_PLANE_VARIABLE,
        global = true,
        value_name = "URL",
        value_parser = NonEmptyStringValueParser::new(),
        default_value = "http://127.0.0.1:7718"
    )]
    control_plane: String,

    /// The Organization this invocation applies to; without it, a committed
    /// .kestrel/organization between the working directory and its repository root, then the
    /// only Organization
    #[arg(
        long,
        env = ORGANIZATION_VARIABLE,
        global = true,
        value_name = "NAME",
        value_parser = NonEmptyStringValueParser::new()
    )]
    organization: Option<String>,

    /// Emit the complete response as one JSON document, or a Transcript as one JSON record a
    /// line; without it a terminal gets the presentation chosen for the command and anything
    /// else gets it tab-delimited
    #[arg(long, global = true)]
    json: bool,

    /// Filter the complete JSON response through a jq expression, as jaq implements it. A
    /// string result prints raw and any other result as JSON, one a line; no results is success
    #[arg(long, global = true, value_name = "EXPR", value_parser = NonEmptyStringValueParser::new())]
    jq: Option<String>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Go from whatever the control plane holds to a Session carrying a Brief, declaring what is
    /// missing from what this clone says, and explaining every value before applying it
    Start(Start),
    /// Preview and apply one Project, Agent and Trigger declaration document
    Apply(Apply),
    /// Name and show the Operator: the person who runs this install, and the default author of
    /// anything a command leaves unnamed
    #[command(subcommand)]
    Operator(OperatorCommand),
    /// Declare and list Organizations
    #[command(subcommand)]
    Organization(OrganizationCommand),
    /// Declare and list Projects
    #[command(subcommand)]
    Project(ProjectCommand),
    /// Declare and list Agents
    #[command(subcommand)]
    Agent(AgentCommand),
    /// Hold, list and forget the Provider Credentials an Organization's Sessions reach a model with
    #[command(subcommand)]
    Credential(CredentialCommand),
    /// Declare Subscription Profiles, and hold, list and forget the logins in them
    #[command(subcommand)]
    Profile(ProfileCommand),
    /// Register and list Integrations: credentialed connections to external systems
    #[command(subcommand)]
    Integration(IntegrationCommand),
    /// Read the Events recorded for an Organization
    #[command(subcommand)]
    Event(EventCommand),
    /// Declare, inspect, test and control Triggers
    #[command(subcommand)]
    Trigger(TriggerCommand),
    /// Read Workspaces
    #[command(subcommand)]
    Workspace(WorkspaceCommand),
    /// Show, enqueue, list and stop Sessions
    #[command(subcommand)]
    Session(SessionCommand),
    /// List the Instances held for work that exists nowhere else, and release them
    #[command(subcommand)]
    Instance(InstanceCommand),
    /// Read the queue: the limits, one row per occupant with its phase, and one per queued Session
    Queue,
    /// Print the resolved scope, where each value came from, what exists in it, and what to
    /// session next
    Status,
    /// Print every exit code, what it means, and when a script should branch on it
    ExitCodes,
}

#[derive(Debug, Args)]
pub(crate) struct Apply {
    /// The declaration file, or `-` for standard input
    #[arg(short = 'f', long, value_name = "FILE")]
    file: String,
}

#[derive(Debug, Args)]
pub(crate) struct Start {
    /// The Brief the Workspace starts with; `@FILE` reads it from a file and `-` from standard
    /// input
    #[arg(long)]
    brief: String,
    /// A repository the work happens against; repeat for many. Without it, the clone's origin
    #[arg(long = "repository", value_name = "URL")]
    repositories: Vec<String>,
    /// The branch the work happens on. Without it, a Project's own, then origin's default,
    /// then the branch checked out
    #[arg(long)]
    branch: Option<String>,
    /// The Project, declared if missing. Without it, the one declaring the repositories, then
    /// the repository's name
    #[arg(long)]
    project: Option<String>,
    /// The Agent, declared if missing. Without it, the only one, then its harness's name
    #[arg(long)]
    agent: Option<String>,
    /// The Harness a declared Agent is driven by. Without it, the declared Agent's, then
    /// kestrel's default
    #[arg(long)]
    harness: Option<String>,
    /// The model a declared Agent works with. Without it, its Harness's default
    #[arg(long)]
    model: Option<String>,
    /// A Provider Credential for the Organization to hold, read from the environment variable
    /// of this name; repeat for many
    #[arg(long = "credential", value_name = "VARIABLE")]
    credentials: Vec<String>,
    /// Apply the plan without asking. A start asks only when standard input and standard error
    /// are both a terminal
    #[arg(long, short)]
    yes: bool,
}

impl Command {
    fn scoped(&self) -> bool {
        match self {
            Command::Start(_)
            | Command::Apply(_)
            | Command::Project(_)
            | Command::Agent(_)
            | Command::Credential(_)
            | Command::Profile(_)
            | Command::Integration(_)
            | Command::Trigger(_)
            | Command::Workspace(_)
            | Command::Session(_)
            | Command::Instance(_)
            | Command::Queue
            | Command::Status => true,
            Command::Operator(_) | Command::Organization(_) | Command::ExitCodes => false,
            Command::Event(event) => match event {
                EventCommand::List { .. } => true,
                EventCommand::Show { .. } => false,
            },
        }
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some((value, corrected)) = json_value(&args) {
        let invocation = Invocation {
            elsewhere: false,
            control_plane: String::new(),
            operation: Client::command()
                .try_get_matches_from(std::iter::once("kestrel".to_owned()).chain(corrected))
                .map(|matches| operation(&matches))
                .unwrap_or_default(),
            args: args.clone(),
            json: true,
        };
        let refused = Failed::new(
            Exit::Usage,
            format!(
                "--json takes no value, and was given `{value}`; it emits the complete \
                 response, so select from it with jq, as in \
                 `kestrel agent list --json | jq '.[] | {{id, name}}'`, and pass arguments \
                 before --json"
            ),
        );
        return ExitCode::from(diagnostic::report(&refused.into(), &invocation).code());
    }
    let matches = Client::command()
        .try_get_matches()
        .unwrap_or_else(|error| exit_after(&error));
    let client = Client::from_arg_matches(&matches).unwrap_or_else(|error| exit_after(&error));
    let invocation = Invocation {
        elsewhere: matches.value_source("control_plane") != Some(ValueSource::DefaultValue),
        control_plane: client.control_plane.clone(),
        operation: operation(&matches),
        args,
        json: client.json || client.jq.is_some(),
    };
    // A malformed filter is refused here, before any consumer can read or change anything.
    let filter = match client.jq.as_deref().map(filter::Filter::compile) {
        Some(Ok(filter)) => Some(filter),
        Some(Err(why)) => {
            let refused = Failed::new(Exit::Usage, format!("--jq is not a valid filter: {why}"));
            return ExitCode::from(diagnostic::report(&refused.into(), &invocation).code());
        }
        None => None,
    };
    let exit = match run(client, filter, &matches, &invocation).await {
        Ok(()) => Exit::Success,
        Err(error) => diagnostic::report(&error, &invocation),
    };

    ExitCode::from(exit.code())
}

pub(crate) fn operation(matches: &clap::ArgMatches) -> String {
    let mut words = Vec::new();
    let mut here = matches;
    while let Some((name, deeper)) = here.subcommand() {
        words.push(name);
        here = deeper;
    }
    words.join(" ")
}

async fn run(
    client: Client,
    filter: Option<filter::Filter>,
    matches: &clap::ArgMatches,
    invocation: &Invocation,
) -> Result<()> {
    let json = invocation.json;
    let presentation = Presentation::chosen(json, filter);

    let named = client.organization.map(|organization| Scope {
        organization,
        source: match matches.value_source("organization") {
            Some(ValueSource::EnvVariable) => Source::Environment,
            _ => Source::Flag,
        },
    });
    if named
        .as_ref()
        .is_some_and(|scope| matches!(scope.source, Source::Flag))
        && !client.command.as_ref().is_none_or(Command::scoped)
    {
        bail!(Failed::new(
            Exit::Usage,
            "--organization scopes nothing here; this command names its record directly"
        ));
    }
    if let Some(Command::ExitCodes) = client.command {
        let catalog = Exit::ALL.map(Exit::record);
        return typed::collection(&presentation, &view::EXIT_CODES, &catalog, "");
    }

    let control_plane: Url = client.control_plane.parse().with_context(|| {
        Failed::new(
            Exit::Usage,
            format!("{} is no control-plane URL", client.control_plane),
        )
    })?;
    let control_plane_source = match matches.value_source("control_plane") {
        Some(ValueSource::CommandLine) => "--control-plane",
        Some(ValueSource::EnvVariable) => CONTROL_PLANE_VARIABLE,
        _ => "default",
    };
    let api = ControlPlane::at(control_plane.clone(), &invocation.operation);
    let scoping = Scoping::new(&api, named);

    let Some(command) = client.command else {
        return readiness::show(&api, scoping, invocation, &presentation).await;
    };
    let running = Running {
        api: &api,
        presentation: &presentation,
        json,
        control_plane: &control_plane,
        control_plane_given: &client.control_plane,
        invocation,
    };
    match command {
        Command::Start(start) => {
            started(&api, &presentation, scoping, start).await?;
        }
        Command::Apply(Apply { file }) => {
            let declaration = declaration(&file)?;
            let organization = scoping.resolve().await?.organization;
            if json {
                let applied: wire::AppliedDeclaration = api
                    .post_as(
                        &["organizations", &organization, "declaration"],
                        &declaration,
                    )
                    .await?;
                show_applied_whole(&presentation, &applied, &applied.admitting_outsiders)?;
            } else {
                let preview = api
                    .post_as(
                        &["organizations", &organization, "declaration", "preview"],
                        &declaration,
                    )
                    .await?;
                show_declaration(preview);
                api.post_as::<wire::AppliedDeclaration>(
                    &["organizations", &organization, "declaration"],
                    &declaration,
                )
                .await?;
            }
        }
        Command::Operator(command) => command::operator::run(command, scoping, running).await?,
        Command::Organization(command) => {
            command::organization::run(command, scoping, running).await?
        }
        Command::Project(command) => command::project::run(command, scoping, running).await?,
        Command::Agent(command) => command::agent::run(command, scoping, running).await?,
        Command::Credential(command) => command::credential::run(command, scoping, running).await?,
        Command::Profile(command) => command::profile::run(command, scoping, running).await?,
        Command::Integration(command) => {
            command::integration::run(command, scoping, running).await?
        }
        Command::Event(command) => command::event::run(command, scoping, running).await?,
        Command::Trigger(command) => command::trigger::run(command, scoping, running).await?,
        Command::Workspace(command) => command::workspace::run(command, scoping, running).await?,
        Command::Session(command) => command::session::run(command, scoping, running).await?,
        Command::Instance(command) => command::instance::run(command, scoping, running).await?,
        Command::Status => {
            let location = StatusLocation {
                control_plane: client.control_plane.clone(),
                control_plane_source: control_plane_source.to_string(),
            };
            status(&api, &presentation, location, scoping.derive().await?).await?;
        }
        Command::Queue => {
            let organization = scoping.resolve().await?.organization;
            let snapshot: wire::Queue = api
                .get_as(&["organizations", &organization, "queue"])
                .await?;
            queue::show(&presentation, &snapshot, invocation)?;
        }
        Command::ExitCodes => unreachable!("the catalog is shown before the control plane is"),
    }

    Ok(())
}

/// Every value is explained on stderr before anything changes, so what stdout carries is what
/// the start reached and nothing else.
pub(crate) async fn started(
    api: &ControlPlane,
    presentation: &Presentation,
    scoping: Scoping<'_>,
    mut start: Start,
) -> Result<()> {
    let brief = given(&start.brief)?;
    let mut clone =
        start::LocalClone::of(&std::env::current_dir().context("reading the working directory")?);
    let (named, existing) = match scoping.derive().await? {
        Derived::Scope(scope) => (Some(scope), Vec::new()),
        Derived::Unnamed { existing } if existing.is_empty() => {
            return Err(scope::nothing_to_scope(api).await);
        }
        Derived::Unnamed { existing } => (None, existing),
    };
    let organization =
        start::organization(named, &existing).map_err(|missing| incomplete(&[missing]))?;

    let declared = scope::organizations(api)
        .await?
        .contains(&organization.value);
    let (projects, agents, credentials) = if declared {
        let within = |records| ["organizations", &organization.value, records];
        tokio::try_join!(
            async { api.get_as(&within("projects")).await },
            async { api.get_as(&within("agents")).await },
            async { api.get_as(&within("credentials")).await },
        )?
    } else {
        (Vec::new(), Vec::new(), Vec::new())
    };
    let existing = start::Existing::read(declared, projects, agents, credentials);
    let names_a_project = start.project.as_ref().is_some_and(|name| {
        existing
            .projects
            .iter()
            .any(|declared| &declared.name == name)
    });
    // Planning matches declared Projects by address, so it compares what a declaration would hold.
    if !start.repositories.is_empty() {
        start.repositories = resolved(api, start.repositories).await?;
    } else if !names_a_project && let Some(origin) = clone.origin.take() {
        clone.origin = resolved(api, vec![origin]).await?.pop();
    }

    let secrets = start::secrets(&start.credentials, |variable| std::env::var(variable).ok());
    let plan = start::plan(
        organization,
        start::Given {
            repositories: start.repositories,
            branch: start.branch,
            project: start.project,
            agent: start.agent,
            harness: start.harness,
            model: start.model,
            credentials: start.credentials,
        },
        &clone,
        &existing,
    );
    let (plan, secrets) = match (plan, secrets) {
        (Ok(plan), Ok(secrets)) => (plan, secrets),
        (plan, secrets) => {
            let mut missing = plan.err().unwrap_or_default();
            missing.extend(secrets.err().unwrap_or_default());
            return Err(incomplete(&missing));
        }
    };

    let explained = plan.explained();
    let column = explained
        .iter()
        .map(|row| row.what.len())
        .max()
        .unwrap_or_default();
    eprintln!("starting work with");
    for row in &explained {
        let (what, value, because, flag) = (row.what, &row.value, row.because, row.flag);
        if row.given {
            eprintln!("  {what:column$}  {value}  ({because})");
        } else {
            eprintln!("  {what:column$}  {value}  ({because}; {flag} overrides it)");
        }
    }

    if !start.yes && std::io::stdin().is_terminal() && std::io::stderr().is_terminal() {
        eprintln!("applying it will");
        for step in plan.applying(&existing) {
            eprintln!("  {step}");
        }
        if !confirmed()? {
            eprintln!("nothing was applied");
            return Ok(());
        }
    }

    let started: wire::Started = api
        .post_as(&["starts"], &plan.body(&brief, &secrets))
        .await?;
    for (kind, settled) in [
        ("organization", &started.organization),
        ("project", &started.project),
        ("agent", &started.agent),
    ] {
        if settled.created {
            eprintln!("declared the {kind} {}", settled.name);
        }
    }
    typed::show(presentation, &view::STARTED, &started)
}

pub(crate) async fn resolved(api: &ControlPlane, repositories: Vec<String>) -> Result<Vec<String>> {
    let resolution: wire::RepositoryResolution = api
        .post_as(
            &["repository-resolutions"],
            &wire::RepositoryResolutionRequest::new(repositories),
        )
        .await?;

    Ok(resolution
        .repositories
        .into_iter()
        .map(|repository| repository.address)
        .collect())
}

pub(crate) fn empty_list(
    control_plane: &str,
    absent: &str,
    organization: Option<&str>,
    action: &str,
) -> String {
    let scope = organization.map_or_else(String::new, |name| {
        format!(" in Organization {}", shell::quoted(name))
    });
    let flag = organization.map_or_else(String::new, |name| {
        format!(" --organization {}", shell::quoted(name))
    });
    format!(
        "No {absent}{scope}.\nInspect the next step with:\n  kestrel{flag} {action} --control-plane {}",
        shell::quoted(control_plane)
    )
}

pub(crate) fn typed(name: &str) -> Result<bool> {
    eprint!("type {name} to retire it: ");
    let mut answer = String::new();
    std::io::stdin()
        .read_line(&mut answer)
        .context("reading the answer")?;

    Ok(answer.trim() == name)
}

/// What retiring an Integration with unsent posts left unsaid, each as one line.
pub(crate) fn canceled_posts(integration: &wire::Integration) -> String {
    typed::joined(
        &integration
            .canceled_posts
            .iter()
            .map(|post| {
                let said = post
                    .turn
                    .map_or_else(|| "outcome".to_owned(), |turn| format!("turn {turn}"));
                let attempted = post.attempted_at.as_ref().map_or_else(String::new, |at| {
                    format!(" (a request went out at {at}, so it may be there all the same)")
                });
                format!("{} {said}: {}{attempted}", post.session, post.reason)
            })
            .collect::<Vec<_>>(),
    )
}

pub(crate) fn direction(given: &str) -> Result<wire::Direction, String> {
    [wire::Direction::Inbound, wire::Direction::Outbound]
        .into_iter()
        .find(|direction| direction.as_str() == given)
        .ok_or_else(|| format!("{given} is not a direction: inbound or outbound"))
}

pub(crate) fn declares(category: &str) -> bool {
    matches!(category, "model" | "mode" | "thought_level")
}

pub(crate) fn confirmed() -> Result<bool> {
    loop {
        eprint!("apply this plan? [Y/n] ");
        let mut answer = String::new();
        if std::io::stdin()
            .read_line(&mut answer)
            .context("reading the answer")?
            == 0
        {
            eprintln!();
            return Ok(false);
        }
        match answer.trim().to_lowercase().as_str() {
            "" | "y" | "yes" => return Ok(true),
            "n" | "no" => return Ok(false),
            _ => {}
        }
    }
}

pub(crate) fn incomplete(missing: &[start::Missing]) -> anyhow::Error {
    let flags: Vec<&str> = missing.iter().map(|missing| missing.flag).collect();
    let reasons: Vec<String> = missing
        .iter()
        .map(|missing| format!("{}: {}", missing.flag, missing.because))
        .collect();

    Failed::new(
        Exit::Usage,
        format!(
            "nothing says what to start with; pass {} ({})",
            flags.join(" and "),
            reasons.join("; ")
        ),
    )
    .into()
}

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

pub(crate) fn explain_invalid_subcommand(error: &clap::Error, args: &[String]) -> Option<String> {
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

pub(crate) fn declaration(file: &str) -> Result<Value> {
    parsed(&read_file(file)?)
}

/// An empty file is sent as an empty mapping, so the control plane refuses it for the
/// `triggers` it lacks rather than for being no mapping at all.
pub(crate) fn trigger_file(file: &str) -> Result<Value> {
    let text = read_file(file)?;
    if text.trim().is_empty() {
        return Ok(json!({}));
    }

    parsed(&text)
}

/// The schedule form as the generated union carries it: exactly one of a filter, an interval,
/// or a cron expression with its zone.
pub(crate) fn trigger_condition(
    filter: Option<Value>,
    every: Option<String>,
    cron: Option<String>,
    zone: Option<String>,
) -> wire::TriggerDeclarationVariant {
    match (filter, every, cron) {
        (Some(filter), _, _) => {
            wire::TriggerDeclarationVariant::EventTriggerCondition(json!({ "filter": filter }))
        }
        (None, Some(every), _) => {
            wire::TriggerDeclarationVariant::IntervalTriggerCondition(json!({ "every": every }))
        }
        (None, None, Some(cron)) => wire::TriggerDeclarationVariant::CronTriggerCondition(
            json!({ "cron": cron, "zone": zone }),
        ),
        (None, None, None) => {
            unreachable!("clap requires a filter, an interval or a cron expression")
        }
    }
}

pub(crate) fn declaration_document(document: Value) -> Result<wire::TriggerFile> {
    let typed: wire::TriggerFile = serde_json::from_value(document.clone())
        .context(Failed::new(Exit::Usage, "reading the declaration file"))?;
    match unread_field(&document, &serde_json::to_value(&typed)?) {
        Some(field) => bail!(Failed::new(
            Exit::Usage,
            format!("the declaration file: {field}")
        )),
        None => Ok(typed),
    }
}

/// The generated file reads past a field it does not know, so a document with one is refused
/// rather than sent with the field silently dropped.
pub(crate) fn unread_field(sent: &Value, read: &Value) -> Option<String> {
    fn within(sent: &Value, read: &Value, path: &mut Vec<String>) -> Option<String> {
        for (name, value) in sent.as_object()? {
            let Some(kept) = read.get(name) else {
                return Some(name.clone());
            };
            path.push(name.clone());
            if let Some(field) = within(value, kept, path) {
                return Some(field);
            }
            path.pop();
        }

        None
    }

    let mut path = Vec::new();
    let field = within(sent, read, &mut path)?;
    Some(if path.is_empty() {
        format!("unknown field `{field}`")
    } else {
        format!("{}: unknown field `{field}`", path.join("."))
    })
}

pub(crate) fn parsed(text: &str) -> Result<Value> {
    yaml_serde::from_str(text).context(Failed::new(Exit::Usage, "reading the declaration file"))
}

pub(crate) fn read_file(file: &str) -> Result<String> {
    if file == "-" {
        return standard_input();
    }

    std::fs::read_to_string(file)
        .with_context(|| Failed::new(Exit::Usage, format!("reading the declaration file {file}")))
}

pub(crate) fn given(value: &str) -> Result<String> {
    if value == "-" {
        return standard_input();
    }
    match value.strip_prefix('@') {
        Some(path) => std::fs::read_to_string(path)
            .with_context(|| Failed::new(Exit::Usage, format!("reading {path}"))),
        None => Ok(value.to_owned()),
    }
}

pub(crate) fn standard_input() -> Result<String> {
    let mut text = String::new();
    std::io::stdin()
        .read_to_string(&mut text)
        .context("reading standard input")?;

    Ok(text)
}

pub(crate) fn sign(action: &wire::DeclarationChangeAction) -> char {
    match action {
        wire::DeclarationChangeAction::Add => '+',
        wire::DeclarationChangeAction::Change => '~',
        wire::DeclarationChangeAction::Unchanged => '=',
        wire::DeclarationChangeAction::Remove => '-',
    }
}

pub(crate) fn show_differences(differences: Vec<wire::DeclarationDifference>) {
    for difference in differences {
        println!("    {}", difference.field);
        for (sign, value) in [('-', difference.was), ('+', difference.becomes)] {
            for line in value.iter().flat_map(|value| value.lines()) {
                println!("      {sign} {line}");
            }
        }
    }
}

pub(crate) fn show_declaration(applied: wire::AppliedDeclaration) {
    for declared in applied.declarations {
        println!(
            "{} {} {}",
            sign(&declared.action),
            declared.kind,
            declared.name
        );
        show_differences(declared.differences);
    }
    for name in applied.admitting_outsiders {
        warn_of_outsiders(&name);
    }
}

pub(crate) fn show_applied_whole(
    presentation: &Presentation,
    applied: &impl serde::Serialize,
    admitting_outsiders: &[String],
) -> Result<()> {
    for name in admitting_outsiders {
        warn_of_outsiders(name);
    }
    typed::whole(presentation, applied)
}

pub(crate) fn warn_of_outsiders(name: &str) {
    eprintln!(
        "warning: the trigger {name} fires for events from people outside the organization. \
         Until 0.4, it is an unsupervised agent with your credentials on your repository, \
         briefed by whatever a stranger writes. Filter on the author_association GitHub \
         reports as OWNER, MEMBER or COLLABORATOR to decline strangers, or keep it as a \
         decision you made on purpose."
    );
}

#[derive(Serialize)]
pub(crate) struct StatusLocation {
    control_plane: String,
    control_plane_source: String,
}

#[derive(Serialize)]
pub(crate) struct Status {
    #[serde(flatten)]
    location: StatusLocation,
    organization: String,
    organization_source: String,
    projects: usize,
    agents: usize,
    triggers: usize,
    workspaces: usize,
    integrations: usize,
    credentials: usize,
    profiles: usize,
    next: String,
}

#[derive(Serialize)]
pub(crate) struct UnresolvedStatus {
    #[serde(flatten)]
    location: StatusLocation,
    organization: Option<String>,
    organization_source: Option<String>,
    organizations: Vec<String>,
    next: String,
}

pub(crate) async fn status(
    api: &ControlPlane,
    presentation: &Presentation,
    location: StatusLocation,
    derived: Derived,
) -> Result<()> {
    let scope = match derived {
        Derived::Scope(scope) => scope,
        Derived::Unnamed { existing } => {
            let next = if existing.is_empty() {
                format!("{BINARY} organization declare <name>")
            } else {
                format!("{BINARY} status --organization <name>")
            };
            typed::show(
                presentation,
                &view::UNRESOLVED,
                &UnresolvedStatus {
                    location,
                    organization: None,
                    organization_source: None,
                    organizations: existing,
                    next,
                },
            )?;
            return Ok(());
        }
    };

    let organization = scope.organization.as_str();
    let (projects, agents, triggers, workspaces, integrations, credentials, profiles) = tokio::try_join!(
        async {
            api.get_as::<wire::ListProjectsResponse>(&["organizations", organization, "projects"])
                .await
        },
        async {
            api.get_as::<wire::ListAgentsResponse>(&["organizations", organization, "agents"])
                .await
        },
        async {
            api.get_as::<wire::ListTriggersResponse>(&["organizations", organization, "triggers"])
                .await
        },
        async {
            api.get_as::<wire::ListWorkspacesResponse>(&[
                "organizations",
                organization,
                "workspaces",
            ])
            .await
        },
        async {
            api.get_as::<wire::ListIntegrationsResponse>(&[
                "organizations",
                organization,
                "integrations",
            ])
            .await
        },
        async {
            api.get_as::<wire::ListProviderCredentialsResponse>(&[
                "organizations",
                organization,
                "credentials",
            ])
            .await
        },
        async {
            api.get_as::<wire::ListSubscriptionProfilesResponse>(&[
                "organizations",
                organization,
                "profiles",
            ])
            .await
        },
    )?;
    typed::show(
        presentation,
        &view::STATUS,
        &Status {
            location,
            organization: organization.to_owned(),
            organization_source: scope.source.to_string(),
            projects: projects.len(),
            agents: agents.len(),
            triggers: triggers.len(),
            workspaces: workspaces.len(),
            integrations: integrations.len(),
            credentials: credentials.len(),
            profiles: profiles.len(),
            next: next_command(projects.first(), agents.first()),
        },
    )?;

    Ok(())
}

pub(crate) fn next_command(project: Option<&wire::Project>, agent: Option<&wire::Agent>) -> String {
    match (project, agent) {
        (Some(project), Some(agent)) => {
            format!(
                "{BINARY} workspace open --project {} --agent {}",
                project.name, agent.name
            )
        }
        (Some(_), None) => format!("{BINARY} agent declare <name>"),
        (None, _) => {
            format!("{BINARY} project declare <name> --repository <url> --branch <branch>")
        }
    }
}

/// The asking happens only where someone is there to read it: a pipe is read in silence, so
/// nothing driving the Client can block on a prompt it cannot see.
pub(crate) fn read_standard_input(what: &str) -> Result<String> {
    let mut stdin = std::io::stdin();
    let unechoed = stdin.is_terminal().then(|| {
        eprintln!("reading {what} from standard input, unechoed; end it with ctrl-d");
        terminal::Unechoed::on(&stdin)
    });

    let mut read = String::new();
    stdin.read_to_string(&mut read)?;
    if unechoed.is_some() {
        eprintln!();
    }

    Ok(read)
}

#[cfg(test)]
mod parser_tests {
    use super::*;

    #[test]
    fn queue_custody_reason_renders_the_diagnostics_repair() {
        let reason = json!({
            "kind": "subscription_profile", "profile": "jack", "session": "holding",
            "diagnostic": {
                "kind": "state_conflict", "message": "the sign-in is in use",
                "context": { "operation": "use_sign_in", "resource": "subscription_profile",
                    "reference": "jack", "organization": "acme", "state": "credential_in_use", "holding_session": "holding" },
                "next_steps": [{ "action": "inspect_resource", "resource": "session", "reference": "holding", "organization": "acme" }]
            }
        });
        let reason: wire::QueueReason = serde_json::from_value(reason).unwrap();
        let rendered = queue::reason_in_words(
            &reason,
            &diagnostic::tests::invocation(),
            &Presentation::Delimited,
        );
        assert!(
            serde_json::from_value::<wire::QueueReason>(json!({ "kind": "subscription_profile" }))
                .is_err()
        );
        assert!(rendered.contains("the sign-in is in use"), "{rendered}");
        assert!(
            rendered.contains("kestrel session show holding --organization acme"),
            "{rendered}"
        );
    }

    #[test]
    fn an_organization_named_workspace_does_not_change_the_guessed_noun() {
        let arguments = ["--organization", "workspace", "session", "start"];
        let error = Client::command()
            .try_get_matches_from(["kestrel"].into_iter().chain(arguments))
            .expect_err("start is not a session verb");
        let arguments = arguments.map(str::to_owned);
        let explanation = explain_invalid_subcommand(&error, &arguments).expect("session help");

        assert!(
            explanation.contains("kestrel session enqueue"),
            "{explanation}"
        );
        assert!(
            explanation.contains("Accepted session verbs:"),
            "{explanation}"
        );
    }

    fn args(given: &[&str]) -> Vec<String> {
        given.iter().map(|&arg| arg.to_owned()).collect()
    }

    #[test]
    fn any_value_given_to_json_is_refused_and_dropped_from_the_retry() {
        let given = args(&["workspace", "post", "w", "--json", "fix a, then b"]);
        assert_eq!(
            json_value(&given),
            Some(("fix a, then b", args(&["workspace", "post", "w", "--json"])))
        );
        assert_eq!(
            json_value(&args(&["agent", "list", "--json=id"])),
            Some(("id", args(&["agent", "list", "--json"])))
        );
        assert_eq!(
            json_value(&args(&["agent", "list", "--json", "--organization", "a"])),
            None
        );
        assert_eq!(
            json_value(&args(&["workspace", "changes", "w", "--", "--json", "a"])),
            None
        );
        let parsed = Client::try_parse_from(["kestrel", "agent", "list", "--json"])
            .expect("--json alone is a flag");
        assert!(parsed.json);
    }

    #[test]
    fn a_trigger_declaration_is_the_generated_request_the_flags_describe() {
        let declaration = wire::TriggerDeclaration {
            name: "ready".to_owned(),
            brief: "Work on {{ event.type }}".to_owned(),
            branch: Some(None),
            correlation: Some(None),
            on_miss: Some(None),
            on_open_workspace: Some(None),
            project: "kestrel".to_owned(),
            agent: "builder".to_owned(),
            allows: Some(vec!["reviewer".to_owned()]),
            profile: Some(None),
            filter: None,
            every: None,
            cron: None,
            zone: None,
            model: None,
            mode: None,
            thought_level: None,
            variant: trigger_condition(Some(json!({ "exact": { "type": "x" } })), None, None, None),
        };

        assert_eq!(
            serde_json::to_value(&declaration).unwrap(),
            json!({
                "name": "ready", "brief": "Work on {{ event.type }}", "project": "kestrel",
                "agent": "builder", "allows": ["reviewer"], "branch": null, "correlation": null,
                "on_miss": null, "on_open_workspace": null, "profile": null,
                "filter": { "exact": { "type": "x" } },
            })
        );
        assert_eq!(
            serde_json::to_value(trigger_condition(
                None,
                None,
                Some("0 9 * * 1-5".to_owned()),
                Some("UTC".to_owned()),
            ))
            .unwrap(),
            json!({ "cron": "0 9 * * 1-5", "zone": "UTC" })
        );
    }

    #[test]
    fn a_test_request_carries_the_declaration_file_the_control_plane_reads() {
        let document = json!({ "triggers": { "ready": {
            "filter": { "exact": { "type": "x" } }, "brief": "Work", "project": "kestrel",
            "agent": "builder",
        } } });

        let typed = declaration_document(document.clone()).expect("the file the document holds");
        assert_eq!(serde_json::to_value(&typed).unwrap(), document);

        let stray = declaration_document(json!({ "triggers": { "ready": {
            "filter": { "exact": { "type": "x" } }, "brief": "Work", "project": "kestrel",
            "agent": "builder", "agnet": "builder",
        } } }));
        assert_eq!(
            stray.unwrap_err().to_string(),
            "the declaration file: triggers.ready: unknown field `agnet`"
        );
    }

    #[test]
    fn a_misspelling_uses_the_parsers_suggested_verb() {
        let error = Client::command()
            .try_get_matches_from(["kestrel", "workspace", "sael"])
            .expect_err("sael is not a workspace verb");
        let explanation = explain_invalid_subcommand(&error, &["workspace".into(), "sael".into()])
            .expect("workspace help");

        assert!(
            explanation.contains("kestrel workspace seal"),
            "{explanation}"
        );
    }
}
