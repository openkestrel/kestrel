mod api;
mod changes;
mod corrective;
mod exit;
mod files;
mod output;
mod scope;
mod sse;
mod start;
mod transcript;
mod view;
mod work;

use std::io::{IsTerminal as _, Read as _, Write as _};
use std::process::ExitCode;

use anyhow::{Context as _, Result, bail};
use clap::builder::NonEmptyStringValueParser;
use clap::error::{ContextKind, ContextValue};
use clap::parser::ValueSource;
use clap::{Args, CommandFactory as _, FromArgMatches as _, Parser, Subcommand};
use reqwest::Url;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::api::ControlPlane;
use crate::exit::{Exit, Failed};
use crate::output::{Presentation, show};
use crate::scope::{Derived, Scope, Scoping, Source};

const BINARY: &str = "kestrel";
const CONTROL_PLANE_VARIABLE: &str = "KESTREL_CONTROL_PLANE";
const ORGANIZATION_VARIABLE: &str = "KESTREL_ORGANIZATION";

#[derive(Debug, Parser)]
#[command(
    name = BINARY,
    version,
    about = "Reach a kestrel control plane over its operator boundary.",
    after_help = "Exit codes: `kestrel exit-codes` lists each one, what it means, and when to branch on it.",
    disable_help_subcommand = true
)]
struct Client {
    #[command(subcommand)]
    command: Command,

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

    /// Emit these fields and no others, as JSON, one record a line; without it a terminal
    /// gets the presentation chosen for the command and anything else gets it tab-delimited
    #[arg(long, global = true, value_name = "FIELDS", num_args = 0..=1, default_missing_value = "")]
    json: Option<String>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Go from whatever the control plane holds to a Session carrying a Brief, declaring what is
    /// missing from what this clone says, and explaining every value before applying it
    Start(Start),
    /// Preview and apply one Project, Agent and Trigger declaration document
    Apply(Apply),
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
struct Apply {
    /// The declaration file, or `-` for standard input
    #[arg(short = 'f', long, value_name = "FILE")]
    file: String,
}

#[derive(Debug, Args)]
struct Start {
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
            Command::Organization(_) | Command::ExitCodes => false,
            Command::Event(event) => match event {
                EventCommand::List { .. } => true,
                EventCommand::Show { .. } => false,
            },
        }
    }
}

#[derive(Debug, Subcommand)]
#[command(
    after_help = "Manage a Provider Credential with `kestrel credential set <variable>` and `kestrel credential forget <variable>`."
)]
enum CredentialCommand {
    /// Hold a Provider Credential against an Organization, read from standard input
    Set {
        /// The environment variable a Harness reads it from
        variable: String,
    },
    /// List what the Organization holds, by the variable each is read from and never by value
    List,
    /// Forget a Provider Credential the Organization holds
    Forget {
        /// The environment variable it is read from
        variable: String,
    },
}

#[derive(Debug, Subcommand)]
enum ProfileCommand {
    /// Declare a Subscription Profile: a person's login to a subscribed Harness
    Declare {
        /// The name a Workspace or Trigger names it by
        name: String,
        /// The person it belongs to, which never changes
        #[arg(long)]
        owner: String,
    },
    /// Hold a login in a profile, read from standard input
    Set {
        /// The profile it is held in
        name: String,
        #[command(flatten)]
        entry: ProfileEntry,
    },
    /// List every profile in the Organization with what each holds, one JSON record a line
    List,
    /// Forget a login a profile holds
    Forget {
        /// The profile it is held in
        name: String,
        #[command(flatten)]
        entry: ProfileEntry,
    },
}

#[derive(Debug, Args)]
#[group(required = true, multiple = false)]
struct ProfileEntry {
    /// An environment variable the Harness is spawned with
    #[arg(long, value_name = "NAME")]
    variable: Option<String>,
    /// A file beneath the agent's home, handed back after each Session so a refreshed login
    /// persists
    #[arg(long, value_name = "PATH")]
    file: Option<String>,
}

impl ProfileEntry {
    fn path<'a>(&'a self, organization: &'a str, profile: &'a str) -> Vec<&'a str> {
        let mut path = vec!["organizations", &organization, "profiles", profile];
        match (&self.variable, &self.file) {
            (Some(variable), _) => path.extend(["variables", variable.as_str()]),
            (None, Some(file)) => path.extend(["files", file.as_str()]),
            (None, None) => unreachable!("clap requires one of them"),
        }

        path
    }
}

#[derive(Debug, Subcommand)]
enum IntegrationCommand {
    /// Register an Integration
    #[command(subcommand)]
    Register(RegisterCommand),
    /// List every Integration in the Organization, one JSON record a line
    List,
    /// Acknowledge the latest oversized Event refused by an Integration
    AcknowledgeRefusal { name: String },
}

#[derive(Debug, Subcommand)]
enum RegisterCommand {
    /// A connection to GitHub, watching one repository
    Github {
        /// The name it is referred to by
        name: String,
        /// The repository it watches, as owner/name
        #[arg(long, value_name = "OWNER/NAME")]
        repository: String,
        /// The credential it presents to GitHub
        #[arg(
            long,
            env = "KESTREL_GITHUB_TOKEN",
            value_name = "TOKEN",
            hide_env_values = true
        )]
        token: String,
        /// A direction it carries — inbound, outbound; repeat for both
        #[arg(
            long = "carries",
            value_name = "DIRECTION",
            default_values = ["inbound", "outbound"]
        )]
        carries: Vec<String>,
        /// How often the poll asks GitHub what has happened
        #[arg(long, value_name = "DURATION", default_value = "1m")]
        interval: String,
        /// The secret GitHub signs webhook deliveries with; given one, kestrel receives the
        /// repository's events by webhook and stops polling for them
        #[arg(
            long,
            env = "KESTREL_GITHUB_WEBHOOK_SECRET",
            value_name = "SECRET",
            hide_env_values = true
        )]
        webhook_secret: Option<String>,
        #[arg(long, env = "KESTREL_GITHUB_API", hide = true)]
        api: Option<String>,
    },
    /// A generic endpoint any producer can POST CloudEvents to
    Webhook {
        /// The name it is referred to by
        name: String,
        /// The secret a sender presents as `Authorization: Bearer <secret>`
        #[arg(
            long,
            env = "KESTREL_WEBHOOK_SECRET",
            value_name = "SECRET",
            hide_env_values = true
        )]
        secret: String,
    },
}

#[derive(Debug, Subcommand)]
enum EventCommand {
    /// List the Events recorded for the Organization, most recent first, one JSON record a line
    List {
        /// How many to list at most
        #[arg(long, default_value_t = 50)]
        limit: usize,
    },
    /// Show one Event's whole envelope and payload
    Show {
        /// The Event's record identifier
        record: String,
    },
}

#[derive(Debug, Subcommand)]
enum TriggerCommand {
    /// Make the Organization's applied Triggers what a declaration file says, printing the diff
    Apply {
        /// The declaration file, or `-` for standard input
        #[arg(short = 'f', long, value_name = "FILE")]
        file: String,
        /// Print the diff without making it
        #[arg(long)]
        dry_run: bool,
    },
    /// Declare a Trigger, or make the named one what this declaration describes
    Declare {
        /// The name it is referred to by
        name: String,
        /// The Events it matches: a CloudEvents filter as JSON; `@FILE` reads it from a file
        /// and `-` from standard input
        #[arg(long, value_name = "JSON", required_unless_present_any = ["every", "cron"])]
        filter: Option<String>,
        /// Fire on an interval counted from the declaration, in place of a filter
        #[arg(long, value_name = "DURATION", conflicts_with = "filter")]
        every: Option<String>,
        /// Fire on a five-field cron expression, in place of a filter or an interval
        #[arg(
            long,
            value_name = "EXPRESSION",
            conflicts_with_all = ["filter", "every"],
            requires = "zone"
        )]
        cron: Option<String>,
        /// The IANA time zone the cron expression is read in, such as UTC or America/New_York
        #[arg(long, value_name = "ZONE", requires = "cron")]
        zone: Option<String>,
        /// The Brief a firing hands its Workspace, rendered over an Event; `@FILE` reads it from
        /// a file and `-` from standard input
        #[arg(long)]
        brief: String,
        /// The branch a firing's work happens on, rendered from the Event
        #[arg(long)]
        branch: Option<String>,
        /// The key that finds an open Workspace for this work, rendered from the Event
        #[arg(long)]
        correlation: Option<String>,
        /// What to do when correlation finds no open Workspace: open or ignore
        #[arg(long, value_name = "OPEN|IGNORE")]
        on_miss: Option<String>,
        /// What to do when correlation finds an open Workspace: continue its waiting Session, or
        /// start a new Session with this Trigger's Agent once the unfinished one lets go
        #[arg(long, value_name = "CONTINUE|NEW-SESSION", requires = "correlation")]
        on_open_workspace: Option<String>,
        /// The Project a firing's work happens against
        #[arg(long)]
        project: String,
        /// The Agent a firing starts work with
        #[arg(long)]
        agent: String,
        /// Another Agent an agent:<name> label may choose instead; repeat for many
        #[arg(long = "allow", value_name = "AGENT")]
        allows: Vec<String>,
        /// The Subscription Profile a firing's Sessions use
        #[arg(long)]
        profile: Option<String>,
    },
    /// List every Trigger in the Organization, one JSON record a line
    List,
    /// Show a Trigger
    Show { name: String },
    /// Say whether a Trigger matches an Event and what it would render, starting no work
    Test {
        name: String,
        /// The Event's record; absent tests the next elapsing of a scheduled Trigger
        #[arg(long)]
        event: Option<String>,
        /// The GitHub Integration the issue is read through
        #[arg(long, requires = "issue")]
        integration: Option<String>,
        /// Test against the Event a dispatch of this issue would record, recording nothing
        #[arg(
            long,
            value_name = "NUMBER",
            requires = "integration",
            conflicts_with = "event"
        )]
        issue: Option<i64>,
        /// Test the Trigger as a declaration file declares it rather than as it was applied;
        /// `-` for standard input
        #[arg(short = 'f', long, value_name = "FILE")]
        file: Option<String>,
        /// The instruction a dispatch supplies for the Brief to render; `@FILE` reads it from a
        /// file and `-` from standard input
        #[arg(long)]
        instruction: Option<String>,
        /// An Agent the Trigger allows, in place of the one it or a label would choose
        #[arg(long)]
        agent: Option<String>,
    },
    /// Start a Trigger's work on an issue now, whether or not its filter matches anything
    Dispatch {
        /// The Trigger whose work starts
        name: String,
        /// The GitHub Integration the issue is read through, and its Outcome said back through
        #[arg(long)]
        integration: String,
        /// The issue to work on
        #[arg(long, value_name = "NUMBER")]
        issue: i64,
        /// The instruction the Brief reads as `instruction`; `@FILE` reads it from a file and
        /// `-` from standard input
        #[arg(long)]
        instruction: Option<String>,
        /// An Agent the Trigger allows, in place of the one it or a label would choose
        #[arg(long)]
        agent: Option<String>,
    },
    /// Stop a Trigger firing, without forgetting it
    Disable { name: String },
    /// Let a disabled Trigger fire again
    Enable { name: String },
}

#[derive(Debug, Subcommand)]
enum OrganizationCommand {
    /// Declare an Organization, or change its live Instance limit
    Declare {
        /// The name it is referred to by
        name: String,
        /// The most Instances the Organization may keep live at once
        #[arg(long)]
        max_live_instances: Option<std::num::NonZeroUsize>,
    },
    /// List every Organization, one JSON record a line
    List,
}

#[derive(Debug, Subcommand)]
enum ProjectCommand {
    /// Declare a Project, or make the one by this name what this declaration describes
    Declare {
        /// The name it is referred to by
        name: String,
        /// A repository the work happens against; repeat for many
        #[arg(long = "repository", value_name = "URL", required = true)]
        repositories: Vec<String>,
        /// The branch the work happens on
        #[arg(long)]
        branch: String,
    },
    /// List every Project in the Organization, one JSON record a line
    List,
}

#[derive(Debug, Subcommand)]
enum AgentCommand {
    /// Declare an Agent, or make the one by this name what this declaration describes
    Declare {
        /// The name it is referred to by
        name: String,
        /// The Harness that drives it
        #[arg(long, default_value = "opencode")]
        harness: String,
        /// The model it works with; left out, it names none and its Harness's default
        /// is the answer
        #[arg(long)]
        model: Option<String>,
    },
    /// Change the model an Agent works with, leaving every Session in flight on the one it has
    Model {
        /// The name it is referred to by
        name: String,
        /// The model it works with; left out, it names none and its Harness's default
        /// is the answer
        #[arg(long)]
        model: Option<String>,
    },
    /// List every Agent in the Organization, one JSON record a line
    List,
}

#[derive(Debug, Subcommand)]
enum InstanceCommand {
    /// List every Instance kept because it may hold the only copy of its Workspace's work, and why
    List,
    /// Destroy a Workspace's Instance, discarding whatever it holds that was never pushed
    Release {
        /// The Workspace whose Instance it is, by generated name, identifier, any unambiguous
        /// prefix of its identifier, or `latest`
        workspace: String,
        /// The participant releasing it
        #[arg(long, default_value = "operator")]
        as_participant: String,
    },
}

#[derive(Debug, Subcommand)]
enum WorkspaceCommand {
    /// Open a Workspace against a Project and an Agent, and enqueue its first Session
    Open {
        /// The Project its work happens against
        #[arg(long)]
        project: String,
        /// The Agent that participates in it
        #[arg(long)]
        agent: String,
        /// The Subscription Profile its Sessions use
        #[arg(long)]
        profile: Option<String>,
        /// The branch its work happens on
        #[arg(long)]
        branch: Option<String>,
        /// The sealed Workspace this one carries on from, by generated name, identifier, any
        /// unambiguous prefix of its identifier, or `latest`
        #[arg(long, value_name = "WORKSPACE")]
        continues: Option<String>,
        /// The Brief the Workspace starts with; `@FILE` reads it from a file and `-` from
        /// standard input
        #[arg(long)]
        brief: Option<String>,
        /// The model the first Session runs on. Without it, the Agent's
        #[arg(long)]
        model: Option<String>,
        /// The mode the first Session runs in. Without it, the Agent's
        #[arg(long)]
        mode: Option<String>,
        /// The thought level the first Session runs at. Without it, the Agent's
        #[arg(long)]
        thought_level: Option<String>,
        /// The name the Brief is written under. Without it, it is the operator's
        #[arg(long)]
        as_participant: Option<String>,
        /// A Session it waits on until that one ends successfully, by generated name,
        /// identifier, any unambiguous prefix of its identifier, or `latest`; repeatable
        #[arg(long, value_name = "SESSION")]
        depends_on: Vec<String>,
    },
    /// List every Workspace in the Organization
    List,
    #[command(alias = "diff")]
    Changes {
        workspace: String,
        #[arg(long, conflicts_with_all = ["changed", "commit"])]
        staged: bool,
        #[arg(long, conflicts_with = "commit")]
        changed: bool,
        commit: Option<String>,
        #[arg(last = true)]
        paths: Vec<String>,
    },
    #[command(alias = "log")]
    Commits { workspace: String },
    #[command(alias = "stash")]
    Stashes { workspace: String },
    #[command(alias = "status")]
    Work { workspace: String },
    /// List one directory of a Workspace's live Instance, each entry marked tracked, untracked
    /// or ignored; with no path, its repositories
    #[command(alias = "ls")]
    Files {
        workspace: String,
        /// `<repo>/<path>`
        path: Option<String>,
    },
    /// Write a file in a Workspace's live Instance to standard output, byte for byte
    #[command(alias = "cat")]
    Read {
        workspace: String,
        /// `<repo>/<path>`
        path: String,
    },
    /// Show a Workspace
    Show {
        /// Its generated name, its identifier, any unambiguous prefix of its identifier, or
        /// `latest`
        workspace: String,
    },
    /// Add a participant's message; starts a Session or queues its next Turn
    Post {
        /// Its generated name, its identifier, any unambiguous prefix of its identifier, or
        /// `latest`
        workspace: String,
        /// The participant saying the message
        #[arg(long)]
        as_participant: String,
        /// What the participant says
        message: String,
    },
    /// Change a Held Message before a Turn takes it
    #[command(subcommand)]
    Message(MessageCommand),
    /// Seal a Workspace: readable ever after, and never reopened
    Seal {
        /// Its generated name, its identifier, any unambiguous prefix of its identifier, or
        /// `latest`
        workspace: String,
    },
    /// Read a Workspace's Transcript, one JSON entry a line, and the cursor a later read
    /// resumes from
    Transcript {
        /// Its generated name, its identifier, any unambiguous prefix of its identifier, or
        /// `latest`
        workspace: String,
        /// Resume after the cursor a previous read ended with
        #[arg(long)]
        cursor: Option<String>,
        /// Keep reading as entries are appended, until the Workspace is sealed
        #[arg(long)]
        follow: bool,
        /// Join the Workspace's presence as this person while following
        #[arg(long, requires = "follow")]
        as_participant: Option<String>,
        #[arg(long, default_value = "shared_state")]
        kinds: String,
        #[arg(long)]
        no_summaries: bool,
    },
}

#[derive(Debug, Subcommand)]
enum MessageCommand {
    /// Replace a Held Message's text; only its author's name is accepted
    Edit {
        /// Its generated name, its identifier, any unambiguous prefix of its identifier, or
        /// `latest`
        workspace: String,
        /// The id the post answered with
        id: i64,
        /// The participant that wrote the message, and only that one
        #[arg(long)]
        as_participant: String,
        /// What the message now says
        message: String,
    },
    /// Take a Held Message back, so no Turn sees it; only its author's name is accepted
    Withdraw {
        /// Its generated name, its identifier, any unambiguous prefix of its identifier, or
        /// `latest`
        workspace: String,
        /// The id the post answered with
        id: i64,
        /// The participant that wrote the message, and only that one
        #[arg(long)]
        as_participant: String,
    },
}

#[derive(Debug, Subcommand)]
enum SessionCommand {
    /// Enqueue a Session in a Workspace, for the work role to claim and dispatch
    Enqueue {
        /// The Workspace it executes on behalf of, by generated name, identifier, any
        /// unambiguous prefix of its identifier, or `latest`
        #[arg(long)]
        workspace: String,
        /// The Agent it runs, or the one the Workspace's latest Session ran
        #[arg(long)]
        agent: Option<String>,
        /// The model it works with, or none for its Agent's or Harness's default
        #[arg(long)]
        model: Option<String>,
        /// The mode it works in, or none for its Agent's or Harness's default
        #[arg(long)]
        mode: Option<String>,
        /// The thought level it works at, or none for its Agent's or Harness's default
        #[arg(long)]
        thought_level: Option<String>,
        /// A Session it waits on until that one ends successfully, by generated name,
        /// identifier, any unambiguous prefix of its identifier, or `latest`; repeatable
        #[arg(long, value_name = "SESSION")]
        depends_on: Vec<String>,
    },
    /// List every Session in a Workspace
    List {
        /// The Workspace the Sessions execute on behalf of, by generated name, identifier, any
        /// unambiguous prefix of its identifier, or `latest`
        #[arg(long)]
        workspace: String,
    },
    /// Show a Session
    Show {
        /// Its generated name, its identifier, any unambiguous prefix of its identifier, or
        /// `latest`
        session: String,
    },
    /// Cancel a Session's working turn without ending it, so held messages go to the agent at once
    Interrupt {
        /// Its generated name, its identifier, any unambiguous prefix of its identifier, or
        /// `latest`
        session: String,
        /// The participant asking
        #[arg(long)]
        as_participant: String,
    },
    /// End a Session: it succeeds between turns, and fails mid-turn or before it started
    Stop {
        /// Its generated name, its identifier, any unambiguous prefix of its identifier, or
        /// `latest`
        session: String,
    },
    /// Change one of a Session's options between Turns
    Option {
        #[command(subcommand)]
        command: SessionOptionCommand,
    },
}

#[derive(Debug, Subcommand)]
enum SessionOptionCommand {
    /// Set an option, by the id the harness gave it or by its category
    Set {
        /// Its generated name, its identifier, any unambiguous prefix of its identifier, or
        /// `latest`
        session: String,
        /// The option's id, or one of the categories model, mode or thought_level
        option: String,
        /// The value to set, as the harness offers it
        value: String,
        /// The person making the change
        #[arg(long)]
        as_participant: String,
    },
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let exit = match run().await {
        Ok(()) => Exit::Success,
        Err(error) => {
            eprintln!("Error: {error:?}");
            Exit::of(&error)
        }
    };

    ExitCode::from(exit.code())
}

async fn run() -> Result<()> {
    let matches = Client::command()
        .try_get_matches()
        .unwrap_or_else(|error| exit_after(&error));
    let client = Client::from_arg_matches(&matches).unwrap_or_else(|error| exit_after(&error));
    let presentation = Presentation::chosen(
        if matches!(
            &client.command,
            Command::Workspace(
                WorkspaceCommand::Work { .. }
                    | WorkspaceCommand::Files { .. }
                    | WorkspaceCommand::Read { .. }
                    | WorkspaceCommand::Changes { .. }
                    | WorkspaceCommand::Commits { .. }
                    | WorkspaceCommand::Stashes { .. }
            )
        ) {
            None
        } else {
            client.json.as_deref()
        },
    )?;

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
        && !client.command.scoped()
    {
        bail!(Failed::new(
            Exit::Usage,
            "--organization scopes nothing here; this command names its record directly"
        ));
    }
    if let Command::ExitCodes = client.command {
        let catalog = Exit::ALL.map(Exit::record);
        return show(
            &presentation,
            &view::EXIT_CODES,
            &Value::from(catalog.to_vec()),
        );
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
    let api = ControlPlane::at(control_plane.clone());
    let scoping = Scoping::new(&api, named);

    match client.command {
        Command::Start(start) => {
            started(&api, &presentation, scoping, start).await?;
        }
        Command::Apply(Apply { file }) => {
            let declaration = declaration(&file)?;
            let organization = scoping.resolve().await?.organization;
            let preview = api
                .post(
                    &["organizations", &organization, "declaration", "preview"],
                    &declaration,
                )
                .await?;
            show_declaration(&preview)?;
            api.post(
                &["organizations", &organization, "declaration"],
                &declaration,
            )
            .await?;
        }
        Command::Organization(OrganizationCommand::Declare {
            name,
            max_live_instances,
        }) => {
            show(
                &presentation,
                &view::DECLARED,
                &api.post(
                    &["organizations"],
                    &json!({ "name": name, "max_live_instances": max_live_instances }),
                )
                .await?,
            )?;
        }
        Command::Organization(OrganizationCommand::List) => {
            show(
                &presentation,
                &view::ORGANIZATIONS,
                &api.get(&["organizations"]).await?,
            )?;
        }
        Command::Project(ProjectCommand::Declare {
            name,
            repositories,
            branch,
        }) => {
            let organization = scoping.resolve().await?.organization;
            let declaration = json!({
                "name": name,
                "repositories": repositories,
                "branch": branch,
            });
            show(
                &presentation,
                &view::DECLARED,
                &api.post(&["organizations", &organization, "projects"], &declaration)
                    .await?,
            )?;
        }
        Command::Project(ProjectCommand::List) => {
            let organization = scoping.resolve().await?.organization;
            show(
                &presentation,
                &view::PROJECTS,
                &api.get(&["organizations", &organization, "projects"])
                    .await?,
            )?;
        }
        Command::Agent(AgentCommand::Declare {
            name,
            harness,
            model,
        }) => {
            let organization = scoping.resolve().await?.organization;
            let declaration = json!({
                "name": name,
                "harness": harness,
                "model": model,
            });
            show(
                &presentation,
                &view::DECLARED,
                &api.post(&["organizations", &organization, "agents"], &declaration)
                    .await?,
            )?;
        }
        Command::Agent(AgentCommand::Model { name, model }) => {
            let organization = scoping.resolve().await?.organization;
            show(
                &presentation,
                &view::AGENT_MODEL,
                &api.put(
                    &["organizations", &organization, "agents", &name, "model"],
                    &json!({ "model": model }),
                )
                .await?,
            )?;
        }
        Command::Agent(AgentCommand::List) => {
            let organization = scoping.resolve().await?.organization;
            show(
                &presentation,
                &view::AGENTS,
                &api.get(&["organizations", &organization, "agents"]).await?,
            )?;
        }
        Command::Credential(CredentialCommand::Set { variable }) => {
            let organization = scoping.resolve().await?.organization;
            let secret = json!({ "secret": read_the_secret()? });
            show(
                &presentation,
                &view::CREDENTIAL,
                &api.put(
                    &["organizations", &organization, "credentials", &variable],
                    &secret,
                )
                .await?,
            )?;
        }
        Command::Credential(CredentialCommand::List) => {
            let organization = scoping.resolve().await?.organization;
            show(
                &presentation,
                &view::CREDENTIALS,
                &api.get(&["organizations", &organization, "credentials"])
                    .await?,
            )?;
        }
        Command::Credential(CredentialCommand::Forget { variable }) => {
            let organization = scoping.resolve().await?.organization;
            api.delete(
                &["organizations", &organization, "credentials", &variable],
                &json!({}),
            )
            .await?;
        }
        Command::Profile(ProfileCommand::Declare { name, owner }) => {
            let organization = scoping.resolve().await?.organization;
            show(
                &presentation,
                &view::DECLARED,
                &api.post(
                    &["organizations", &organization, "profiles"],
                    &json!({ "name": name, "owner": owner }),
                )
                .await?,
            )?;
        }
        Command::Profile(ProfileCommand::Set { name, entry }) => {
            let organization = scoping.resolve().await?.organization;
            let login = json!({ "secret": read_the_login(entry.file.is_some())? });
            show(
                &presentation,
                &view::LOGIN,
                &api.put(&entry.path(&organization, &name), &login).await?,
            )?;
        }
        Command::Profile(ProfileCommand::List) => {
            let organization = scoping.resolve().await?.organization;
            show(
                &presentation,
                &view::PROFILES,
                &api.get(&["organizations", &organization, "profiles"])
                    .await?,
            )?;
        }
        Command::Profile(ProfileCommand::Forget { name, entry }) => {
            let organization = scoping.resolve().await?.organization;
            api.delete(&entry.path(&organization, &name), &json!({}))
                .await?;
        }
        Command::Integration(IntegrationCommand::Register(register)) => {
            let organization = scoping.resolve().await?.organization;
            let registration = match register {
                RegisterCommand::Github {
                    name,
                    repository,
                    token,
                    carries,
                    interval,
                    webhook_secret,
                    api,
                } => json!({
                    "kind": "github",
                    "name": name,
                    "repository": repository,
                    "token": token,
                    "carries": carries,
                    "interval": interval,
                    "webhook_secret": webhook_secret,
                    "api": api,
                }),
                RegisterCommand::Webhook { name, secret } => {
                    json!({ "kind": "webhook", "name": name, "secret": secret })
                }
            };
            show(
                &presentation,
                &view::DECLARED,
                &api.post(
                    &["organizations", &organization, "integrations"],
                    &registration,
                )
                .await?,
            )?;
        }
        Command::Integration(IntegrationCommand::List) => {
            let organization = scoping.resolve().await?.organization;
            show(
                &presentation,
                &view::INTEGRATIONS,
                &api.get(&["organizations", &organization, "integrations"])
                    .await?,
            )?;
        }
        Command::Integration(IntegrationCommand::AcknowledgeRefusal { name }) => {
            let organization = scoping.resolve().await?.organization;
            api.delete(
                &[
                    "organizations",
                    &organization,
                    "integrations",
                    &name,
                    "event-refusal",
                ],
                &json!({}),
            )
            .await?;
        }
        Command::Event(EventCommand::List { limit }) => {
            let organization = scoping.resolve().await?.organization;
            show(
                &presentation,
                &view::EVENTS,
                &api.get_where(
                    &["organizations", &organization, "events"],
                    &[("limit", &limit.to_string())],
                )
                .await?,
            )?;
        }
        Command::Event(EventCommand::Show { record }) => {
            show(
                &presentation,
                &view::EVENT,
                &api.get(&["events", &record]).await?,
            )?;
        }
        Command::Trigger(TriggerCommand::Declare {
            name,
            filter,
            every,
            cron,
            zone,
            brief,
            branch,
            correlation,
            on_miss,
            on_open_workspace,
            project,
            agent,
            allows,
            profile,
        }) => {
            let organization = scoping.resolve().await?.organization;
            if filter.as_deref() == Some("-") && brief == "-" {
                bail!(Failed::new(
                    Exit::Usage,
                    "the filter and the brief cannot both be read from standard input"
                ));
            }
            let filter = filter
                .map(|filter| {
                    serde_json::from_str::<Value>(&given(&filter)?)
                        .context(Failed::new(Exit::Usage, "a trigger filter is JSON"))
                })
                .transpose()?;
            let brief = given(&brief)?;
            let mut declaration = json!({
                "name": name,
                "brief": brief,
                "branch": branch,
                "correlation": correlation,
                "on_miss": on_miss,
                "on_open_workspace": on_open_workspace,
                "project": project,
                "agent": agent,
                "allows": allows,
                "profile": profile,
            });
            let declaration = declaration
                .as_object_mut()
                .expect("a trigger declaration is an object");
            if let Some(filter) = filter {
                declaration.insert("filter".to_owned(), filter);
            }
            if let Some(every) = every {
                declaration.insert("every".to_owned(), Value::String(every));
            }
            if let Some(cron) = cron {
                declaration.insert("cron".to_owned(), Value::String(cron));
            }
            if let Some(zone) = zone {
                declaration.insert("zone".to_owned(), Value::String(zone));
            }
            let declared = api
                .post(&["organizations", &organization, "triggers"], &declaration)
                .await?;
            if declared["admits_outsiders"] == true {
                warn_of_outsiders(&name);
            }
            show(&presentation, &view::DECLARED, &declared)?;
        }
        Command::Trigger(TriggerCommand::Apply { file, dry_run }) => {
            let organization = scoping.resolve().await?.organization;
            let declarations = trigger_file(&file)?;
            let mut path = vec!["organizations", organization.as_str(), "applied-triggers"];
            if dry_run {
                path.push("preview");
            }
            show_applied_triggers(&api.post(&path, &declarations).await?)?;
        }
        Command::Trigger(TriggerCommand::Dispatch {
            name,
            integration,
            issue,
            instruction,
            agent,
        }) => {
            let organization = scoping.resolve().await?.organization;
            let instruction = instruction.as_deref().map(given).transpose()?;
            show(
                &presentation,
                &view::FIRED,
                &api.post(
                    &[
                        "organizations",
                        &organization,
                        "triggers",
                        &name,
                        "dispatch",
                    ],
                    &json!({
                        "integration": integration,
                        "issue": issue,
                        "instruction": instruction,
                        "agent": agent,
                    }),
                )
                .await?,
            )?;
        }
        Command::Trigger(TriggerCommand::List) => {
            let organization = scoping.resolve().await?.organization;
            show(
                &presentation,
                &view::TRIGGERS,
                &api.get(&["organizations", &organization, "triggers"])
                    .await?,
            )?;
        }
        Command::Trigger(TriggerCommand::Show { name }) => {
            let organization = scoping.resolve().await?.organization;
            show(
                &presentation,
                &view::TRIGGER,
                &api.get(&["organizations", &organization, "triggers", &name])
                    .await?,
            )?;
        }
        Command::Trigger(TriggerCommand::Test {
            name,
            event,
            integration,
            issue,
            file,
            instruction,
            agent,
        }) => {
            let organization = scoping.resolve().await?.organization;
            if file.as_deref() == Some("-") && instruction.as_deref() == Some("-") {
                bail!(Failed::new(
                    Exit::Usage,
                    "the declaration file and the instruction cannot both be read from standard input"
                ));
            }
            let declared = file.as_deref().map(trigger_file).transpose()?;
            let instruction = instruction.as_deref().map(given).transpose()?;
            show(
                &presentation,
                &view::TRIGGER_TEST,
                &api.post(
                    &["organizations", &organization, "triggers", &name, "test"],
                    &json!({
                        "event": event,
                        "integration": integration,
                        "issue": issue,
                        "instruction": instruction,
                        "agent": agent,
                        "declared": declared,
                    }),
                )
                .await?,
            )?;
        }
        Command::Trigger(TriggerCommand::Disable { name }) => {
            let organization = scoping.resolve().await?.organization;
            show(
                &presentation,
                &view::TRIGGER_STATE,
                &api.post(
                    &["organizations", &organization, "triggers", &name, "disable"],
                    &json!({}),
                )
                .await?,
            )?;
        }
        Command::Trigger(TriggerCommand::Enable { name }) => {
            let organization = scoping.resolve().await?.organization;
            show(
                &presentation,
                &view::TRIGGER_STATE,
                &api.post(
                    &["organizations", &organization, "triggers", &name, "enable"],
                    &json!({}),
                )
                .await?,
            )?;
        }
        Command::Workspace(WorkspaceCommand::Open {
            project,
            agent,
            profile,
            branch,
            continues,
            brief,
            model,
            mode,
            thought_level,
            as_participant,
            depends_on,
        }) => {
            let organization = scoping.resolve().await?.organization;
            let brief = brief.as_deref().map(given).transpose()?;
            let opened = api
                .post(
                    &["organizations", &organization, "workspaces"],
                    &json!({
                        "project": project,
                        "agent": agent,
                        "profile": profile,
                        "branch": branch,
                        "continues": continues,
                        "model": model,
                        "mode": mode,
                        "thought_level": thought_level,
                        "brief": brief,
                        "participant": as_participant,
                        "depends_on": depends_on,
                    }),
                )
                .await?;
            show(
                &presentation,
                &view::OPENED,
                &json!({
                    "workspace": opened["workspace"]["name"],
                    "workspace_id": opened["workspace"]["id"],
                    "session": opened["session"]["name"],
                    "session_id": opened["session"]["id"],
                }),
            )?;
        }
        Command::Workspace(WorkspaceCommand::List) => {
            let organization = scoping.resolve().await?.organization;
            show(
                &presentation,
                &view::WORKSPACES,
                &api.get(&["organizations", &organization, "workspaces"])
                    .await?,
            )?;
        }
        Command::Workspace(WorkspaceCommand::Work { workspace }) => {
            let organization = scoping.resolve().await?.organization;
            let answer = api
                .get(&[
                    "organizations",
                    &organization,
                    "workspaces",
                    &workspace,
                    "work",
                ])
                .await?;
            work::show(answer, client.json.is_some())?;
        }
        Command::Workspace(WorkspaceCommand::Changes {
            workspace,
            staged,
            changed,
            commit,
            paths,
        }) => {
            let organization = scoping.resolve().await?.organization;
            let scope = if staged {
                "staged".to_owned()
            } else if changed {
                "changed".to_owned()
            } else if let Some(commit) = commit {
                format!("commit:{commit}")
            } else {
                "unpublished".to_owned()
            };
            let mut query = vec![("scope", scope.as_str())];
            query.extend(paths.iter().map(|path| ("path", path.as_str())));
            let answer = api
                .get_where(
                    &[
                        "organizations",
                        &organization,
                        "workspaces",
                        &workspace,
                        "changes",
                    ],
                    &query,
                )
                .await?;
            changes::show(answer, client.json.is_some())?;
        }
        Command::Workspace(
            command @ (WorkspaceCommand::Commits { .. } | WorkspaceCommand::Stashes { .. }),
        ) => {
            let (workspace, read) = match command {
                WorkspaceCommand::Commits { workspace } => (workspace, "commits"),
                WorkspaceCommand::Stashes { workspace } => (workspace, "stashes"),
                _ => unreachable!(),
            };
            let organization = scoping.resolve().await?.organization;
            let answer = api
                .get(&[
                    "organizations",
                    &organization,
                    "workspaces",
                    &workspace,
                    read,
                ])
                .await?;
            changes::show(answer, client.json.is_some())?;
        }
        Command::Workspace(WorkspaceCommand::Files { workspace, path }) => {
            let organization = scoping.resolve().await?.organization;
            let path = path.unwrap_or_default();
            let answer = api
                .get_where(
                    &[
                        "organizations",
                        &organization,
                        "workspaces",
                        &workspace,
                        "files",
                    ],
                    &[("path", &path)],
                )
                .await?;
            files::list(answer, client.json.is_some())?;
        }
        Command::Workspace(WorkspaceCommand::Read { workspace, path }) => {
            let organization = scoping.resolve().await?.organization;
            let response = api
                .get_response(
                    &[
                        "organizations",
                        &organization,
                        "workspaces",
                        &workspace,
                        "file",
                    ],
                    &[("path", &path)],
                )
                .await?;
            files::read(response, client.json.is_some()).await?;
        }
        Command::Workspace(WorkspaceCommand::Show { workspace }) => {
            let organization = scoping.resolve().await?.organization;
            show(
                &presentation,
                &view::WORKSPACE,
                &api.get(&["organizations", &organization, "workspaces", &workspace])
                    .await?,
            )?;
        }
        Command::Workspace(WorkspaceCommand::Post {
            workspace,
            as_participant,
            message,
        }) => {
            let organization = scoping.resolve().await?.organization;
            let answer = api
                .post(
                    &[
                        "organizations",
                        &organization,
                        "workspaces",
                        &workspace,
                        "messages",
                    ],
                    &json!({ "participant": as_participant, "message": message }),
                )
                .await?;
            let session = &answer["session"];
            let held = &answer["held_message"];
            if client.json.is_some() {
                show(&presentation, &view::DECLARED, &answer)?;
            } else if !session.is_null() {
                show(&presentation, &view::DECLARED, session)?;
            } else if !held.is_null() {
                show(&presentation, &view::DECLARED, held)?;
            } else {
                eprintln!("queued as the next turn of the session already in flight");
            }
        }
        Command::Workspace(WorkspaceCommand::Message(MessageCommand::Edit {
            workspace,
            id,
            as_participant,
            message,
        })) => {
            let organization = scoping.resolve().await?.organization;
            let answer = api
                .put(
                    &[
                        "organizations",
                        &organization,
                        "workspaces",
                        &workspace,
                        "messages",
                        &id.to_string(),
                    ],
                    &json!({ "participant": as_participant, "message": message }),
                )
                .await?;
            show(&presentation, &view::HELD_MESSAGE, &answer)?;
        }
        Command::Workspace(WorkspaceCommand::Message(MessageCommand::Withdraw {
            workspace,
            id,
            as_participant,
        })) => {
            let organization = scoping.resolve().await?.organization;
            api.delete(
                &[
                    "organizations",
                    &organization,
                    "workspaces",
                    &workspace,
                    "messages",
                    &id.to_string(),
                ],
                &json!({ "participant": as_participant }),
            )
            .await?;
        }
        Command::Workspace(WorkspaceCommand::Seal { workspace }) => {
            let organization = scoping.resolve().await?.organization;
            show(
                &presentation,
                &view::DECLARED,
                &api.post(
                    &[
                        "organizations",
                        &organization,
                        "workspaces",
                        &workspace,
                        "seal",
                    ],
                    &json!({}),
                )
                .await?,
            )?;
        }
        Command::Workspace(WorkspaceCommand::Transcript {
            workspace,
            cursor,
            follow,
            as_participant,
            kinds,
            no_summaries,
        }) => {
            let organization = scoping.resolve().await?.organization;
            let read = transcript::read(
                &control_plane,
                &organization,
                &workspace,
                cursor,
                transcript::Selection {
                    follow,
                    kinds: &kinds,
                    summaries: !no_summaries,
                    as_participant: as_participant.as_deref(),
                },
                &presentation,
            )
            .await?;
            // Beside the Transcript rather than in it, so stdout carries entries and nothing else.
            if let Some(cursor) = read {
                eprintln!("cursor  {cursor}");
            }
        }
        Command::Session(SessionCommand::Enqueue {
            workspace,
            agent,
            model,
            mode,
            thought_level,
            depends_on,
        }) => {
            let organization = scoping.resolve().await?.organization;
            show(
                &presentation,
                &view::DECLARED,
                &api.post(
                    &[
                        "organizations",
                        &organization,
                        "workspaces",
                        &workspace,
                        "sessions",
                    ],
                    &json!({
                        "agent": agent,
                        "model": model,
                        "mode": mode,
                        "thought_level": thought_level,
                        "depends_on": depends_on,
                    }),
                )
                .await?,
            )?;
        }
        Command::Session(SessionCommand::List { workspace }) => {
            let organization = scoping.resolve().await?.organization;
            show(
                &presentation,
                &view::SESSIONS,
                &api.get(&[
                    "organizations",
                    &organization,
                    "workspaces",
                    &workspace,
                    "sessions",
                ])
                .await?,
            )?;
        }
        Command::Session(SessionCommand::Show { session }) => {
            let organization = scoping.resolve().await?.organization;
            shown_session(
                &presentation,
                &api.get(&["organizations", &organization, "sessions", &session])
                    .await?,
            )?;
        }
        Command::Session(SessionCommand::Interrupt {
            session,
            as_participant,
        }) => {
            let organization = scoping.resolve().await?.organization;
            show(
                &presentation,
                &view::INTERRUPTED,
                &api.post(
                    &[
                        "organizations",
                        &organization,
                        "sessions",
                        &session,
                        "interrupt",
                    ],
                    &json!({ "participant": as_participant }),
                )
                .await?,
            )?;
        }
        Command::Session(SessionCommand::Stop { session }) => {
            let organization = scoping.resolve().await?.organization;
            show(
                &presentation,
                &view::STOPPED,
                &api.post(
                    &["organizations", &organization, "sessions", &session, "stop"],
                    &json!({}),
                )
                .await?,
            )?;
        }
        Command::Session(SessionCommand::Option { command }) => {
            let SessionOptionCommand::Set {
                session,
                option,
                value,
                as_participant,
            } = command;
            let organization = scoping.resolve().await?.organization;
            let body = if declares(&option) {
                json!({ "participant": as_participant, "category": option, "value": value })
            } else {
                json!({ "participant": as_participant, "option": option, "value": value })
            };
            let changed = api
                .post(
                    &[
                        "organizations",
                        &organization,
                        "sessions",
                        &session,
                        "options",
                    ],
                    &body,
                )
                .await?;
            warn_about_cache(&changed, &option);
            shown_session(&presentation, &changed)?;
        }
        Command::Instance(InstanceCommand::List) => {
            let organization = scoping.resolve().await?.organization;
            show(
                &presentation,
                &view::INSTANCES,
                &api.get(&["organizations", &organization, "instances"])
                    .await?,
            )?;
        }
        Command::Instance(InstanceCommand::Release {
            workspace,
            as_participant,
        }) => {
            let organization = scoping.resolve().await?.organization;
            show(
                &presentation,
                &view::RELEASED,
                &api.post(
                    &[
                        "organizations",
                        &organization,
                        "workspaces",
                        &workspace,
                        "instance",
                        "release",
                    ],
                    &json!({ "participant": as_participant }),
                )
                .await?,
            )?;
        }
        Command::Status => {
            let location = json!({
                "control_plane": client.control_plane,
                "control_plane_source": control_plane_source,
            });
            status(&api, &presentation, location, scoping.derive().await?).await?;
        }
        Command::Queue => {
            let organization = scoping.resolve().await?.organization;
            let snapshot = api.get(&["organizations", &organization, "queue"]).await?;
            shown_queue(&presentation, &snapshot)?;
        }
        Command::ExitCodes => unreachable!("the catalog is shown before the control plane is"),
    }

    Ok(())
}

/// Every value is explained on stderr before anything changes, so what stdout carries is what
/// the start reached and nothing else.
async fn started(
    api: &ControlPlane,
    presentation: &Presentation,
    scoping: Scoping<'_>,
    start: Start,
) -> Result<()> {
    let brief = given(&start.brief)?;
    let clone =
        start::LocalClone::of(&std::env::current_dir().context("reading the working directory")?);
    let (named, existing) = match scoping.derive().await? {
        Derived::Scope(scope) => (Some(scope), Vec::new()),
        Derived::Unnamed { existing } => (None, existing),
    };
    let organization =
        start::organization(named, &existing, &clone).map_err(|missing| incomplete(&[missing]))?;

    let declared = names(&api.get(&["organizations"]).await?).contains(&organization.value);
    let (projects, agents, credentials) = if declared {
        let within = async |records| {
            api.get(&["organizations", &organization.value, records])
                .await
        };
        tokio::try_join!(within("projects"), within("agents"), within("credentials"))?
    } else {
        (json!([]), json!([]), json!([]))
    };
    let existing = start::Existing::read(declared, &projects, &agents, &credentials);

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

    let started = api.post(&["starts"], &plan.body(&brief, &secrets)).await?;
    for (kind, settled) in [
        ("organization", &started["organization"]),
        ("project", &started["project"]),
        ("agent", &started["agent"]),
    ] {
        if settled["created"] == true {
            eprintln!("declared the {kind} {}", rendered(&settled["name"]));
        }
    }
    show(
        presentation,
        &view::STARTED,
        &json!({
            "organization": started["organization"]["name"],
            "project": started["project"]["name"],
            "agent": started["agent"]["name"],
            "workspace": started["workspace"]["name"],
            "workspace_id": started["workspace"]["id"],
            "session": started["session"]["name"],
            "session_id": started["session"]["id"],
        }),
    )
}

/// The limits and their occupancy said first, so every row below is read against what it
/// counts against. A script asks `--json` for the fields and gets the rows alone, each one
/// carrying the limits it arrived with.
/// A `--json` read asks for the fields themselves and gets them exactly as served.
fn shown_session(presentation: &Presentation, session: &Value) -> Result<()> {
    let mut record = session.clone();
    if !matches!(presentation, Presentation::Json(_)) {
        record["options"] = Value::from(session_options(&record["options"]));
        record["changing_options"] = Value::from(changing_options(&record["changing_options"]));
        record["commands"] = Value::from(session_commands(&record["commands"]));
    }

    show(presentation, &view::SESSION, &record)
}

fn changing_options(changing: &Value) -> String {
    changing
        .as_array()
        .into_iter()
        .flatten()
        .map(|change| {
            format!(
                "{}: {} ({})",
                change["option"].as_str().unwrap_or("option"),
                change["value"].as_str().unwrap_or(""),
                change["participant"].as_str().unwrap_or("")
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// ADR-0041: a change that makes the next Turn re-read the context without the prompt cache says
/// so, with the size of the context it last reported.
fn warn_about_cache(changed: &Value, named: &str) {
    let warns = changed["options"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|option| {
            option["id"].as_str() == Some(named) || option["category"].as_str() == Some(named)
        })
        .is_some_and(|option| option["warns_cache"] == true);
    if !warns {
        return;
    }

    match changed["usage"]["context_used"].as_u64() {
        Some(tokens) => eprintln!(
            "changing this makes the next turn re-read the context without the prompt cache: \
             {tokens} tokens"
        ),
        None => eprintln!(
            "changing this makes the next turn re-read the context without the prompt cache"
        ),
    }
}

fn declares(category: &str) -> bool {
    matches!(category, "model" | "mode" | "thought_level")
}

fn session_options(options: &Value) -> String {
    options
        .as_array()
        .into_iter()
        .flatten()
        .map(|option| {
            let category = option["category"]
                .as_str()
                .or_else(|| option["id"].as_str())
                .unwrap_or("option");
            let current = match &option["current"] {
                Value::String(value) => value.clone(),
                other => other.to_string(),
            };

            format!("{category}: {current}")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn session_commands(commands: &Value) -> String {
    commands
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|command| command["name"].as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

fn shown_queue(presentation: &Presentation, snapshot: &Value) -> Result<()> {
    let slots = &snapshot["active_work"];
    let instances = &snapshot["instances"];

    if !matches!(presentation, Presentation::Json(_)) {
        let mut out = std::io::stdout().lock();
        let said = [
            (
                "active-work slots",
                occupancy(slots, "no dispatch configuration recorded", "occupied"),
            ),
            (
                "live instances",
                occupancy(instances, "unbounded", "counted"),
            ),
            ("environment", environment(snapshot)),
        ];
        let width = said
            .iter()
            .map(|(label, _)| label.chars().count())
            .max()
            .unwrap_or_default();
        for (label, line) in said {
            writeln!(out, "{label:width$}  {line}").context("writing the queue's limits")?;
        }
        out.flush().context("writing to standard output")?;
    }

    let mut rows = Vec::new();
    for occupant in slots["occupants"].as_array().into_iter().flatten() {
        let mut record = occupant.as_object().cloned().with_context(|| {
            Failed::new(
                Exit::Unavailable,
                "reading one of the snapshot's Active-Work Slot occupants",
            )
        })?;
        let phase = record.remove("phase").unwrap_or(Value::Null);
        record.insert("state".to_owned(), phase);
        record.insert("position".to_owned(), Value::Null);
        record.insert("pending_since".to_owned(), Value::Null);
        record.insert("preparing".to_owned(), Value::Null);
        record.insert("brief_since".to_owned(), Value::Null);
        record.insert("reasons".to_owned(), Value::Array(Vec::new()));
        record.insert("why".to_owned(), Value::from(why(&record)));
        record.insert("active_work".to_owned(), slots.clone());
        record.insert("instances".to_owned(), instances.clone());
        rows.push(Value::Object(record));
    }
    for state in ["queued", "waiting", "unbriefed"] {
        let section = snapshot[state].as_array().with_context(|| {
            Failed::new(
                Exit::Unavailable,
                format!("reading the snapshot's {state} Sessions"),
            )
        })?;
        for row in section {
            let mut record = row.as_object().cloned().with_context(|| {
                Failed::new(
                    Exit::Unavailable,
                    format!("reading one of the snapshot's {state} Sessions"),
                )
            })?;
            record.insert("state".to_owned(), Value::from(state));
            record.entry("position").or_insert(Value::Null);
            record.entry("pending_since").or_insert(Value::Null);
            record.entry("brief_since").or_insert(Value::Null);
            record.entry("preparing").or_insert(Value::Null);
            record.entry("reasons").or_insert(Value::Array(Vec::new()));
            record.insert("why".to_owned(), Value::from(why(&record)));
            record.insert("active_work".to_owned(), slots.clone());
            record.insert("instances".to_owned(), instances.clone());
            rows.push(Value::Object(record));
        }
    }

    show(presentation, &view::QUEUE, &Value::from(rows))
}

fn why(row: &serde_json::Map<String, Value>) -> String {
    let mut said: Vec<String> = Vec::new();
    if let Some(preparing) = row["preparing"].as_str() {
        said.push(match preparing {
            "starting_harness" => "starting harness".to_owned(),
            "harness_ready" => "harness ready".to_owned(),
            step => step.to_owned(),
        });
    }
    if let Some(since) = row["brief_since"].as_str() {
        said.push(format!("Brief since {since}"));
    }
    if let Some(since) = row["pending_since"].as_str() {
        said.push(format!("input since {since}"));
    }
    said.extend(
        row["reasons"]
            .as_array()
            .into_iter()
            .flatten()
            .map(reason_in_words),
    );
    if said.is_empty() {
        said.push(
            match row["state"].as_str() {
                Some("working") => "in a turn",
                Some("trailing") => "answered; its agent is still working",
                Some("waiting") => "waiting for a turn",
                Some("unbriefed") => "getting ready for its first message",
                _ => "ready",
            }
            .to_owned(),
        );
    }

    said.join("; ")
}

fn environment(snapshot: &Value) -> String {
    snapshot["work_role"]["driver"].as_str().map_or_else(
        || "no dispatch configuration recorded".to_owned(),
        str::to_owned,
    )
}

fn reason_in_words(reason: &Value) -> String {
    let names = |field: &str| {
        reason[field]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(", ")
    };
    let text = |field: &str| reason[field].as_str().unwrap_or("?").to_owned();
    let limit = reason["limit"].as_u64().unwrap_or_default();
    let plural = if limit == 1 { "" } else { "s" };

    match reason["kind"].as_str().unwrap_or_default() {
        "dependencies" => format!("waits on {}", names("sessions")),
        "subscription_profile" => {
            format!(
                "the Subscription Profile {} is held by {}",
                text("profile"),
                text("session")
            )
        }
        "instance_archiving" => {
            format!("waits for the Instance {} to be archived", text("instance"))
        }
        "live_instance_limit" => format!("at the limit of {limit} live Instance{plural}"),
        "active_work_slots" => format!("all {limit} Active-Work Slot{plural} occupied"),
        "ahead" => {
            let mut ahead = names("sessions");
            if let Some(elsewhere) = reason["elsewhere"].as_u64() {
                if !ahead.is_empty() {
                    ahead.push_str(", ");
                }
                ahead.push_str(&format!(
                    "{elsewhere} Session{} in other Organizations",
                    if elsewhere == 1 { "" } else { "s" }
                ));
            }
            format!("behind {ahead}")
        }
        kind => kind.to_owned(),
    }
}

/// A limit and what counts against it as one line, naming what the Organization can name.
fn occupancy(section: &Value, without_limit: &str, unit: &str) -> String {
    let limit = match section["limit"].as_u64() {
        Some(limit) => format!("limit {limit};"),
        None => format!("{without_limit};"),
    };
    let counted = section["count"]
        .as_u64()
        .or_else(|| section["occupied"].as_u64())
        .unwrap_or(0);
    let mut line = format!("{limit} {counted} {unit}");
    let named = named_in(section);
    if !named.is_empty() {
        line.push_str(&format!(" ({})", named.join(", ")));
    }

    line
}

fn named_in(section: &Value) -> Vec<&str> {
    let of = section["counted"]
        .as_array()
        .or_else(|| section["occupants"].as_array());
    of.map_or_else(Vec::new, |of| {
        of.iter()
            .filter_map(|named| named.as_str().or_else(|| named["name"].as_str()))
            .collect()
    })
}

fn confirmed() -> Result<bool> {
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

fn rendered(value: &Value) -> String {
    value
        .as_str()
        .map_or_else(|| value.to_string(), str::to_owned)
}

/// Nothing is asked for: whatever drives the Client may have no terminal to answer on.
fn incomplete(missing: &[start::Missing]) -> anyhow::Error {
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

/// Clap's own exit codes would do, but the catalog is what a script was promised.
fn exit_after(error: &clap::Error) -> ! {
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
mod parser_tests {
    use super::*;

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

#[derive(Deserialize)]
struct AppliedDeclaration {
    declarations: Vec<Declared>,
    admitting_outsiders: Vec<String>,
}

#[derive(Deserialize)]
struct Declared {
    kind: String,
    name: String,
    action: Action,
    differences: Vec<Difference>,
}

#[derive(Deserialize)]
struct Difference {
    field: String,
    was: Option<String>,
    becomes: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum Action {
    Add,
    Change,
    Unchanged,
}

fn declaration(file: &str) -> Result<Value> {
    parsed(&read_file(file)?)
}

/// An empty file is sent as an empty mapping, so the control plane refuses it for the
/// `triggers` it lacks rather than for being no mapping at all.
fn trigger_file(file: &str) -> Result<Value> {
    let text = read_file(file)?;
    if text.trim().is_empty() {
        return Ok(json!({}));
    }

    parsed(&text)
}

fn parsed(text: &str) -> Result<Value> {
    yaml_serde::from_str(text).context(Failed::new(Exit::Usage, "reading the declaration file"))
}

fn read_file(file: &str) -> Result<String> {
    if file == "-" {
        return standard_input();
    }

    std::fs::read_to_string(file)
        .with_context(|| Failed::new(Exit::Usage, format!("reading the declaration file {file}")))
}

fn given(value: &str) -> Result<String> {
    if value == "-" {
        return standard_input();
    }
    match value.strip_prefix('@') {
        Some(path) => std::fs::read_to_string(path)
            .with_context(|| Failed::new(Exit::Usage, format!("reading {path}"))),
        None => Ok(value.to_owned()),
    }
}

fn standard_input() -> Result<String> {
    let mut text = String::new();
    std::io::stdin()
        .read_to_string(&mut text)
        .context("reading standard input")?;

    Ok(text)
}

#[derive(Deserialize)]
struct AppliedTriggers {
    changes: Vec<TriggerChange>,
    admitting_outsiders: Vec<String>,
}

#[derive(Deserialize)]
struct TriggerChange {
    name: String,
    action: TriggerAction,
    differences: Vec<Difference>,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum TriggerAction {
    Add,
    Change,
    Remove,
}

fn show_applied_triggers(value: &Value) -> Result<()> {
    let applied: AppliedTriggers = serde_json::from_value(value.clone()).context(Failed::new(
        Exit::Unavailable,
        "reading the applied triggers",
    ))?;
    if applied.changes.is_empty() {
        println!("no changes");
    }
    for change in applied.changes {
        let sign = match change.action {
            TriggerAction::Add => '+',
            TriggerAction::Change => '~',
            TriggerAction::Remove => '-',
        };
        println!("{sign} {}", change.name);
        show_differences(change.differences);
    }
    for name in applied.admitting_outsiders {
        warn_of_outsiders(&name);
    }

    Ok(())
}

fn show_differences(differences: Vec<Difference>) {
    for difference in differences {
        println!("    {}", difference.field);
        for (sign, value) in [('-', difference.was), ('+', difference.becomes)] {
            for line in value.iter().flat_map(|value| value.lines()) {
                println!("      {sign} {line}");
            }
        }
    }
}

fn show_declaration(value: &Value) -> Result<()> {
    let applied: AppliedDeclaration = serde_json::from_value(value.clone()).context(
        Failed::new(Exit::Unavailable, "reading the declaration preview"),
    )?;
    for declared in applied.declarations {
        let sign = match declared.action {
            Action::Add => '+',
            Action::Change => '~',
            Action::Unchanged => '=',
        };
        println!("{sign} {} {}", declared.kind, declared.name);
        show_differences(declared.differences);
    }
    for name in applied.admitting_outsiders {
        warn_of_outsiders(&name);
    }

    Ok(())
}

fn warn_of_outsiders(name: &str) {
    eprintln!(
        "warning: the trigger {name} fires for events from people outside the organization. \
         Until 0.4, it is an unsupervised agent with your credentials on your repository, \
         briefed by whatever a stranger writes. Filter on the author_association GitHub \
         reports as OWNER, MEMBER or COLLABORATOR to decline strangers, or keep it as a \
         decision you made on purpose."
    );
}

async fn status(
    api: &ControlPlane,
    presentation: &Presentation,
    location: Value,
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
            show(
                presentation,
                &view::UNRESOLVED,
                &merged(
                    location,
                    json!({
                        "organization": null,
                        "organization_source": null,
                        "organizations": existing,
                        "next": next,
                    }),
                ),
            )?;
            return Ok(());
        }
    };

    let organization = scope.organization.as_str();
    let within = async |records| api.get(&["organizations", organization, records]).await;
    let (projects, agents, triggers, workspaces, integrations, credentials, profiles) = tokio::try_join!(
        within("projects"),
        within("agents"),
        within("triggers"),
        within("workspaces"),
        within("integrations"),
        within("credentials"),
        within("profiles"),
    )?;
    let project_names = names(&projects);
    let agent_names = names(&agents);

    show(
        presentation,
        &view::STATUS,
        &merged(
            location,
            json!({
                "organization": organization,
                "organization_source": scope.source.to_string(),
                "projects": count(&projects),
                "agents": count(&agents),
                "triggers": count(&triggers),
                "workspaces": count(&workspaces),
                "integrations": count(&integrations),
                "credentials": count(&credentials),
                "profiles": count(&profiles),
                "next": next_command(&project_names, &agent_names),
            }),
        ),
    )?;

    Ok(())
}

fn merged(mut record: Value, more: Value) -> Value {
    if let (Some(record), Value::Object(more)) = (record.as_object_mut(), more) {
        record.extend(more);
    }
    record
}

fn count(records: &Value) -> usize {
    records.as_array().map_or(0, Vec::len)
}

fn names(records: &Value) -> Vec<String> {
    records
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|record| record["name"].as_str().map(str::to_owned))
        .collect()
}

fn next_command(projects: &[String], agents: &[String]) -> String {
    match (projects.first(), agents.first()) {
        (Some(project), Some(agent)) => {
            format!("{BINARY} workspace open --project {project} --agent {agent}")
        }
        (Some(_), None) => format!("{BINARY} agent declare <name>"),
        (None, _) => {
            format!("{BINARY} project declare <name> --repository <url> --branch <branch>")
        }
    }
}

/// A file is held exactly as it was read, because it is written back exactly as it was held.
fn read_the_login(file: bool) -> Result<String> {
    let read = read_standard_input("a login")?;

    if read.trim().is_empty() {
        bail!(Failed::new(
            Exit::Usage,
            "a login is read from standard input, and nothing was on it"
        ));
    }

    Ok(if file { read } else { read.trim().to_owned() })
}

/// Off standard input rather than out of an argument, so a provider's key is never in a shell
/// history or in what `ps` shows of this process.
fn read_the_secret() -> Result<String> {
    let secret = read_standard_input("a provider credential")?
        .trim()
        .to_owned();

    if secret.is_empty() {
        bail!(Failed::new(
            Exit::Usage,
            "a provider credential is read from standard input, and nothing was on it"
        ));
    }

    Ok(secret)
}

/// The asking happens only where someone is there to read it: a pipe is read in silence, so
/// nothing driving the Client can block on a prompt it cannot see.
fn read_standard_input(what: &str) -> Result<String> {
    let mut stdin = std::io::stdin();
    if stdin.is_terminal() {
        eprintln!("reading {what} from standard input; end it with ctrl-d");
    }

    let mut read = String::new();
    stdin.read_to_string(&mut read)?;

    Ok(read)
}
