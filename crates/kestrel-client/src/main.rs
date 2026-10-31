mod api;
mod changes;
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

use std::io::{IsTerminal as _, Read as _, Write as _};
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
use crate::diagnostic::Invocation;
use crate::exit::{Exit, Failed};
use crate::output::Presentation;
use crate::scope::{Derived, Scope, Scoping, Source};

const BINARY: &str = "kestrel";
const CONTROL_PLANE_VARIABLE: &str = "KESTREL_CONTROL_PLANE";
const ORGANIZATION_VARIABLE: &str = "KESTREL_ORGANIZATION";

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

#[derive(Debug, Subcommand)]
enum OperatorCommand {
    /// Name the Operator, or rename them; their Subscription Profiles stay theirs
    Name {
        /// The name written on what they author from here on
        name: String,
    },
    /// Show the Operator
    Show,
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
            Command::Operator(_) | Command::Organization(_) | Command::ExitCodes => false,
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
    #[command(
        after_help = "Empty lists show scoped guidance at a terminal; non-JSON pipes emit nothing."
    )]
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
        /// A free-text owner; omit to record stable Operator ownership
        #[arg(long)]
        owner: Option<String>,
    },
    /// Hold generic material from hidden terminal input or stdin; create an absent Profile
    #[command(
        after_help = "Choose --variable NAME or --file PATH. Variables lose one trailing newline; UTF-8 files are preserved exactly. Generic writes are unchecked and make no provider or model call; catalogued sign-in imports validate the method's material instead."
    )]
    Set {
        /// The profile it is held in
        name: String,
        /// A free-text owner for creation only; omit to record stable Operator ownership
        #[arg(long)]
        owner: Option<String>,
        #[command(flatten)]
        entry: ProfileEntry,
    },
    /// List every profile in the Organization with what each holds
    #[command(
        after_help = "Empty lists show scoped guidance at a terminal; non-JSON pipes emit nothing."
    )]
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
    /// Create a GitHub App and register its repository Integration through the browser
    CreateGithubApp {
        name: String,
        /// Create the private App under this GitHub organization instead of your personal account
        #[arg(long)]
        app_organization: Option<String>,
        /// Browser-reachable loopback origin (defaults to the control-plane URL)
        #[arg(long)]
        callback_base: Option<String>,
        /// Public HTTPS origin of the webhook listener; omit to use the deployment setting
        #[arg(long)]
        webhook_base: Option<String>,
        #[arg(long, env = "KESTREL_GITHUB_API", hide = true)]
        api: Option<String>,
    },
    /// GitHub Integrations
    #[command(subcommand)]
    Github(GithubCommand),
    /// Register an Integration
    #[command(subcommand)]
    Register(RegisterCommand),
    /// List every Integration in the Organization
    #[command(
        after_help = "Empty lists show scoped guidance at a terminal; non-JSON pipes emit nothing."
    )]
    List,
    /// Show an Integration: its state, revision, directions and why it is out of use
    Show { name: String },
    /// Read one page of repositories the Integration's App can see, with their default branches
    Repositories {
        name: String,
        #[arg(long, default_value_t = 1)]
        page: u32,
        #[arg(long, default_value_t = 30)]
        per_page: u32,
    },
    /// Change an Integration's name, directions or poll interval; what it connects to is fixed
    Change {
        name: String,
        /// The name it is referred to by from now on
        #[arg(long, value_name = "NAME")]
        rename: Option<String>,
        /// A direction it carries — inbound, outbound; repeat for both. Replaces what it carried
        #[arg(long = "carries", value_name = "DIRECTION", value_parser = direction)]
        carries: Vec<wire::Direction>,
        /// How often the poll reads the App's Delivery log
        #[arg(long, value_name = "DURATION")]
        interval: Option<String>,
        /// Refuse the change if the Integration is no longer at this revision
        #[arg(long, value_name = "REVISION")]
        revision: Option<i64>,
    },
    /// Pause every use of an Integration, keeping its credentials, cursor and pending posts
    Disable { name: String },
    /// Resume a disabled Integration; its poll catches up on what GitHub still keeps
    Enable { name: String },
    /// Permanently end an Integration's use: erase the credentials kestrel holds, keep its history
    #[command(
        after_help = "Retiring cannot be undone, and changes nothing on GitHub: the App, its \
                      installation and its keys stay as they are."
    )]
    Retire {
        name: String,
        /// Retire it without being asked to type its name; required where nothing can be asked
        #[arg(long)]
        yes: bool,
    },
    /// Acknowledge the latest oversized Event refused by an Integration
    AcknowledgeRefusal { name: String },
}

#[derive(Debug, Subcommand)]
enum GithubCommand {
    /// Register a connection to GitHub, watching one repository
    Register {
        /// The name it is referred to by
        name: String,
        /// The repository it watches, as owner/name
        #[arg(long, value_name = "OWNER/NAME")]
        repository: String,
        /// The GitHub App's ID
        #[arg(long, env = "KESTREL_GITHUB_APP_ID", value_name = "ID")]
        app_id: i64,
        /// The ID of the installation the App was installed as
        #[arg(long, env = "KESTREL_GITHUB_INSTALLATION_ID", value_name = "ID")]
        installation: i64,
        /// The App's private key: text, `@FILE`, or `-` for standard input
        #[arg(
            long,
            env = "KESTREL_GITHUB_PRIVATE_KEY",
            value_name = "KEY",
            hide_env_values = true,
            allow_hyphen_values = true
        )]
        private_key: String,
        /// A direction it carries — inbound, outbound; repeat for both
        #[arg(
            long = "carries",
            value_name = "DIRECTION",
            value_parser = direction,
            default_values = ["inbound", "outbound"]
        )]
        carries: Vec<wire::Direction>,
        /// How often the poll reads the App's Delivery log
        #[arg(long, value_name = "DURATION", default_value = "1m")]
        interval: String,
        /// The App's webhook secret; given one, a Delivery that reaches kestrel's webhook is
        /// recorded on arrival, ahead of the poll that would otherwise read it
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
    /// Replace the App private key an Integration signs with; GitHub must accept it for the
    /// same App, installation and repository before the previous key is given up
    ReplaceKey {
        name: String,
        /// Read the key from this file instead of standard input
        #[arg(long, value_name = "FILE")]
        private_key_file: Option<std::path::PathBuf>,
        /// Refuse the replacement if the Integration is no longer at this revision
        #[arg(long, value_name = "REVISION")]
        revision: Option<i64>,
    },
    /// Rotate the webhook secret of the App an Integration signs as, resuming one interrupted
    #[command(subcommand)]
    RotateSecret(RotateSecretCommand),
    /// Set the App hook URL or inspect recent Delivery results; polling always continues
    #[command(subcommand)]
    Webhook(WebhookCommand),
}

#[derive(Debug, Subcommand)]
enum WebhookCommand {
    /// Change only the App hook URL; acknowledgement does not prove reachability
    SetUrl {
        name: String,
        /// Public HTTPS origin; omitted to use the control plane's deployment setting
        #[arg(long)]
        base: Option<String>,
        #[arg(long)]
        revision: Option<i64>,
    },
    /// Read the configured URL and recent HTTP results from GitHub's Delivery log
    Status { name: String },
}

#[derive(Debug, Subcommand)]
enum RotateSecretCommand {
    /// Make a replacement secret and set it on GitHub; until GitHub acknowledges it, deliveries
    /// signed with either secret are accepted
    Start {
        name: String,
        /// Refuse the rotation if the Integration is no longer at this revision
        #[arg(long, value_name = "REVISION")]
        revision: Option<i64>,
    },
    /// Show whether a rotation is unfinished, and how to finish it
    Status { name: String },
    /// Set the same saved replacement on GitHub again, and finish the rotation once it is
    /// acknowledged
    Resume { name: String },
}

#[derive(Debug, Subcommand)]
enum RegisterCommand {
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
    /// List the Events recorded for the Organization, most recent first
    #[command(
        after_help = "Empty lists show scoped guidance at a terminal; non-JSON pipes emit nothing."
    )]
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
    /// List every Trigger in the Organization
    #[command(
        after_help = "Empty lists show scoped guidance at a terminal; non-JSON pipes emit nothing."
    )]
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
    /// List every Organization
    #[command(
        after_help = "Empty lists show scoped guidance at a terminal; non-JSON pipes emit nothing."
    )]
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
        /// The Integration a repository reaches GitHub as, checked against GitHub before the
        /// Project is saved; `REPOSITORY=NAME`, or `NAME` alone beside one repository
        #[arg(long = "integration", value_name = "[REPOSITORY=]NAME")]
        integrations: Vec<String>,
        /// The branch the work happens on
        #[arg(long)]
        branch: String,
    },
    /// List every Project in the Organization
    #[command(
        after_help = "Empty lists show scoped guidance at a terminal; non-JSON pipes emit nothing."
    )]
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
    /// List every Agent in the Organization
    #[command(
        after_help = "Empty lists show scoped guidance at a terminal; non-JSON pipes emit nothing."
    )]
    List,
}

#[derive(Debug, Subcommand)]
enum InstanceCommand {
    /// List every Instance kept because it may hold the only copy of its Workspace's work, and why
    #[command(
        after_help = "Empty lists show scoped guidance at a terminal; non-JSON pipes emit nothing."
    )]
    List,
    /// Destroy a Workspace's Instance, discarding whatever it holds that was never pushed
    Release {
        /// The Workspace whose Instance it is, by generated name, identifier, any unambiguous
        /// prefix of its identifier, or `latest`
        workspace: String,
        /// The participant releasing it. Without it, the Operator
        #[arg(long)]
        as_participant: Option<String>,
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
        /// The Sign-in Method its first Session is handed, without its harness's others
        #[arg(long = "sign-in", value_name = "METHOD")]
        sign_in: Option<String>,
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
        /// The name the Brief is written under. Without it, the Operator's
        #[arg(long)]
        as_participant: Option<String>,
        /// A Session it waits on until that one ends successfully, by generated name,
        /// identifier, any unambiguous prefix of its identifier, or `latest`; repeatable
        #[arg(long, value_name = "SESSION")]
        depends_on: Vec<String>,
    },
    /// List Workspaces; an empty terminal lists the scoped next step, while a pipe emits nothing
    #[command(
        after_help = "Empty lists show scoped guidance at a terminal; non-JSON pipes emit nothing."
    )]
    List,
    #[command(
        alias = "diff",
        after_help = "Terminal output includes repository headings and empty-list guidance. Redirected output omits headings and guidance; --json returns the complete operator response.",
        about = "Inspect a Workspace's Unpublished Work",
        long_about = "Inspect a Workspace's Unpublished Work across its repositories.

By default, compare the working tree (including untracked files) with origin/<work-branch>. Before the first push, compare with the merge base of HEAD and origin/<checkout-base>. The Project's checkout base is the starting branch; the Workspace's declared work branch holds its work.

Scopes: default Unpublished Work; --changed for unstaged tracked changes; --staged for staged changes; COMMIT (a hexadecimal commit id) for one commit. Scopes are mutually exclusive. Paths after -- are literal <repo>/<path> filters, using checkout directory names from workspace files.

Examples:
  kestrel workspace changes latest
  kestrel workspace changes latest --changed -- repo/src/main.rs
  kestrel workspace changes latest --staged
  kestrel workspace changes latest abcd1234 -- repo/README.md"
    )]
    Changes {
        workspace: String,
        /// Compare staged changes with HEAD
        #[arg(long, conflicts_with_all = ["changed", "commit"])]
        staged: bool,
        /// Compare unstaged tracked changes with the index
        #[arg(long, conflicts_with = "commit")]
        changed: bool,
        /// Show one commit against its parent
        commit: Option<String>,
        /// Literal repository-qualified paths: <repo>/<path>
        #[arg(last = true)]
        paths: Vec<String>,
    },
    #[command(
        alias = "log",
        after_help = "Terminal output includes repository headings and empty-list guidance. Redirected output omits headings and guidance; --json returns the complete operator response.",
        about = "Inspect commits that have not reached a remote",
        long_about = "List commits reachable from any local branch or detached HEAD that no remote-tracking branch reaches, grouped by repository. This covers the Workspace's live checkouts, not just its declared work branch or checkout base. No comparison scope or path filter is supported.

Examples:
  kestrel workspace commits latest
  kestrel workspace changes latest abcd1234 -- repo/README.md

Inspect one commit's patch with changes COMMIT (a hexadecimal commit id); paths there are literal <repo>/<path> filters. The Project's checkout base starts the checkout; the Workspace's work branch holds its work."
    )]
    Commits { workspace: String },
    #[command(
        alias = "stash",
        after_help = "Terminal output includes repository headings and empty-list guidance. Redirected output omits headings and guidance; --json returns the complete operator response.",
        about = "Inspect stashes in a Workspace's live checkouts",
        long_about = "List stashes grouped by repository without diffing them. All repositories in the Workspace's live Instance are included; no comparison scope or path filter is supported. Stashes are separate from the default Unpublished Work comparison in changes. The checkout base starts the checkout; the Workspace's declared work branch holds its work.

Examples:
  kestrel workspace stashes latest
  kestrel workspace changes latest -- repo/README.md

The changes path filter uses literal <repo>/<path>, with checkout directory names from workspace files."
    )]
    Stashes { workspace: String },
    #[command(
        alias = "status",
        after_help = "Terminal output includes repository headings and empty-list guidance. Redirected output omits headings and guidance; --json returns the complete operator response.",
        about = "Inspect the supervisor's latest report of a Workspace's work",
        long_about = "Show the supervisor's latest per-repository work summary and its report time. This is a reported snapshot, not a fresh diff. Changed and staged counts describe the working tree; committed counts cover commits on local branches or detached HEAD that no remote-tracking branch reaches. The default changes comparison is against origin/<work-branch>, or the merge base of HEAD and origin/<checkout-base> before the first push. The checkout base starts the checkout; the Workspace's declared work branch holds its work.

No comparison scope or path filter is supported here. Use changes for default Unpublished Work, --changed, --staged or one COMMIT, and literal <repo>/<path> filters.

Examples:
  kestrel workspace work latest
  kestrel workspace changes latest --staged -- repo/src/main.rs
  kestrel workspace files latest"
    )]
    Work { workspace: String },
    /// List one directory of a Workspace's live Instance, each entry marked tracked, untracked
    /// or ignored; with no path, its repositories
    #[command(
        alias = "ls",
        after_help = "An empty directory shows scoped inspection guidance at a terminal; non-JSON pipes emit nothing. Redirected listings omit directory headings; --json returns the complete operator response."
    )]
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
        /// The participant saying the message. Without it, the Operator
        #[arg(long)]
        as_participant: Option<String>,
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
    /// Read a Workspace's Transcript, one entry a line, and the cursor a later read
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
        /// The participant that wrote the message, and only that one. Without it, the Operator
        #[arg(long)]
        as_participant: Option<String>,
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
        /// The participant that wrote the message, and only that one. Without it, the Operator
        #[arg(long)]
        as_participant: Option<String>,
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
        /// The Sign-in Method it is handed, or the one the Workspace's latest Session was
        #[arg(long = "sign-in", value_name = "METHOD")]
        sign_in: Option<String>,
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
    #[command(
        after_help = "Empty lists show scoped guidance at a terminal; non-JSON pipes emit nothing."
    )]
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
        /// The participant asking. Without it, the Operator
        #[arg(long)]
        as_participant: Option<String>,
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
        /// The person making the change. Without it, the Operator
        #[arg(long)]
        as_participant: Option<String>,
    },
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

fn operation(matches: &clap::ArgMatches) -> String {
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
        Command::Operator(OperatorCommand::Name { name }) => {
            let operator: wire::Operator = api
                .put_as(&["operator"], &wire::OperatorDeclaration { name })
                .await?;
            typed::show(&presentation, &view::OPERATOR, &operator)?;
        }
        Command::Operator(OperatorCommand::Show) => {
            let operator: wire::Operator = api.get_as(&["operator"]).await?;
            typed::show(&presentation, &view::OPERATOR, &operator)?;
        }
        Command::Organization(OrganizationCommand::Declare {
            name,
            max_live_instances,
        }) => {
            let declaration = wire::OrganizationDeclaration {
                name,
                max_live_instances: Some(
                    max_live_instances.map(|most| i64::try_from(most.get()).unwrap_or(i64::MAX)),
                ),
            };
            let organization: wire::Organization =
                api.post_as(&["organizations"], &declaration).await?;
            typed::show(&presentation, &view::ORGANIZATION_DECLARED, &organization)?;
        }
        Command::Organization(OrganizationCommand::List) => {
            let organizations: wire::ListOrganizationsResponse =
                api.get_as(&["organizations"]).await?;
            typed::collection(
                &presentation,
                &view::ORGANIZATIONS,
                &organizations,
                &empty_list(
                    &client.control_plane,
                    "Organizations",
                    None,
                    "organization declare --help",
                ),
            )?;
        }
        Command::Project(ProjectCommand::Declare {
            name,
            repositories,
            integrations,
            branch,
        }) => {
            let organization = scoping.resolve().await?.organization;
            let integrations = selections(&repositories, &integrations)?;
            let declaration = wire::ProjectDeclaration::builder(branch, name, repositories)
                .integrations(integrations)
                .build();
            let project: wire::Project = api
                .post_as(&["organizations", &organization, "projects"], &declaration)
                .await?;
            typed::show(&presentation, &view::PROJECT_DECLARED, &project)?;
        }
        Command::Project(ProjectCommand::List) => {
            let organization = scoping.resolve().await?.organization;
            let projects: wire::ListProjectsResponse = api
                .get_as(&["organizations", &organization, "projects"])
                .await?;
            typed::collection(
                &presentation,
                &view::PROJECTS,
                &projects,
                &empty_list(
                    &client.control_plane,
                    "Projects",
                    Some(&organization),
                    "project declare --help",
                ),
            )?;
        }
        Command::Agent(AgentCommand::Declare {
            name,
            harness,
            model,
        }) => {
            let organization = scoping.resolve().await?.organization;
            let mut declaration = wire::AgentDeclaration::new(harness, name);
            declaration.model = Some(model);
            let agent: wire::Agent = api
                .post_as(&["organizations", &organization, "agents"], &declaration)
                .await?;
            typed::show(&presentation, &view::AGENT_DECLARED, &agent)?;
        }
        Command::Agent(AgentCommand::Model { name, model }) => {
            let organization = scoping.resolve().await?.organization;
            let agent: wire::Agent = api
                .put_as(
                    &["organizations", &organization, "agents", &name, "model"],
                    &wire::AgentModel { model: Some(model) },
                )
                .await?;
            typed::show(&presentation, &view::AGENT_MODEL, &agent)?;
        }
        Command::Agent(AgentCommand::List) => {
            let organization = scoping.resolve().await?.organization;
            let agents: wire::ListAgentsResponse = api
                .get_as(&["organizations", &organization, "agents"])
                .await?;
            typed::collection(
                &presentation,
                &view::AGENTS,
                &agents,
                &empty_list(
                    &client.control_plane,
                    "Agents",
                    Some(&organization),
                    "agent declare --help",
                ),
            )?;
        }
        Command::Credential(CredentialCommand::Set { variable }) => {
            let organization = scoping.resolve().await?.organization;
            let held: wire::ProviderCredential = api
                .put_as(
                    &["organizations", &organization, "credentials", &variable],
                    &wire::ProviderCredentialSecret {
                        secret: read_the_secret()?,
                    },
                )
                .await?;
            typed::show(&presentation, &view::CREDENTIAL, &held)?;
        }
        Command::Credential(CredentialCommand::List) => {
            let organization = scoping.resolve().await?.organization;
            let credentials: wire::ListProviderCredentialsResponse = api
                .get_as(&["organizations", &organization, "credentials"])
                .await?;
            typed::collection(
                &presentation,
                &view::CREDENTIALS,
                &credentials,
                &empty_list(
                    &client.control_plane,
                    "Provider Credentials",
                    Some(&organization),
                    "credential set --help",
                ),
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
            let profile: wire::SubscriptionProfile = api
                .post_as(
                    &["organizations", &organization, "profiles"],
                    &wire::SubscriptionProfileDeclaration { name, owner },
                )
                .await?;
            typed::show(&presentation, &view::PROFILE_DECLARED, &profile)?;
        }
        Command::Profile(ProfileCommand::Set { name, owner, entry }) => {
            let organization = scoping.resolve().await?.organization;
            let secret = read_the_login(entry.file.is_some())?;
            let profiles: wire::ListSubscriptionProfilesResponse = api
                .get_as(&["organizations", &organization, "profiles"])
                .await?;
            let absent = !profiles.iter().any(|profile| profile.name == name);
            if absent {
                let _: wire::SubscriptionProfile = api
                    .post_as(
                        &["organizations", &organization, "profiles"],
                        &wire::SubscriptionProfileDeclaration {
                            name: name.clone(),
                            owner,
                        },
                    )
                    .await?;
            }
            let login: wire::SubscriptionProfileLogin = api
                .put_as(
                    &entry.path(&organization, &name),
                    &wire::ProviderCredentialSecret { secret },
                )
                .await
                .map_err(|error| {
                    if absent {
                        diagnostic::after_save(
                            error,
                            &format!("The Profile {name} in Organization {organization} was already declared"),
                        )
                    } else {
                        error
                    }
                })?;
            typed::show(&presentation, &view::LOGIN, &login)?;
        }
        Command::Profile(ProfileCommand::List) => {
            let organization = scoping.resolve().await?.organization;
            let profiles: wire::ListSubscriptionProfilesResponse = api
                .get_as(&["organizations", &organization, "profiles"])
                .await?;
            typed::collection(
                &presentation,
                &view::PROFILES,
                &profiles,
                &empty_list(
                    &client.control_plane,
                    "Subscription Profiles",
                    Some(&organization),
                    "profile declare --help",
                ),
            )?;
        }
        Command::Profile(ProfileCommand::Forget { name, entry }) => {
            let organization = scoping.resolve().await?.organization;
            api.delete(&entry.path(&organization, &name), &json!({}))
                .await?;
        }
        Command::Integration(IntegrationCommand::CreateGithubApp {
            name,
            app_organization,
            callback_base,
            webhook_base,
            api: github_api,
        }) => {
            let organization = scoping.resolve().await?.organization;
            let started: wire::GithubAppStarted = api
                .post_as(
                    &["organizations", &organization, "github-app"],
                    &wire::GithubAppStart {
                        name,
                        callback_base: callback_base.unwrap_or_else(|| control_plane.to_string()),
                        webhook_base: webhook_base.map(Some),
                        api: github_api
                            .map(|api| serde_json::from_value(json!(api)))
                            .transpose()?,
                        app_organization: app_organization.map(Some),
                    },
                )
                .await?;
            typed::show(&presentation, &view::GITHUB_APP_STARTED, &started)?;
        }
        Command::Integration(IntegrationCommand::Github(GithubCommand::Register {
            name,
            repository,
            app_id,
            installation,
            private_key,
            carries,
            interval,
            webhook_secret,
            api: github_api,
        })) => {
            let organization = scoping.resolve().await?.organization;
            let registration =
                wire::IntegrationRegistration::GithubRegistration(wire::GithubRegistration {
                    kind: json!("github"),
                    name,
                    repository,
                    app_id,
                    installation,
                    private_key: given(&private_key)?,
                    carries: Some(carries),
                    interval: Some(Some(interval)),
                    webhook_secret: webhook_secret.map(Some),
                    api: github_api.map(Some),
                });
            let registered: wire::Integration = api
                .post_as(
                    &["organizations", &organization, "integrations"],
                    &registration,
                )
                .await?;
            typed::show(&presentation, &view::INTEGRATION_DECLARED, &registered)?;
        }
        Command::Integration(IntegrationCommand::Github(GithubCommand::ReplaceKey {
            name,
            private_key_file,
            revision,
        })) => {
            let organization = scoping.resolve().await?.organization;
            let private_key = match &private_key_file {
                Some(file) => std::fs::read_to_string(file).with_context(|| {
                    Failed::new(Exit::Usage, format!("reading {}", file.display()))
                })?,
                None => read_standard_input("the App's private key")?,
            };
            let replaced: wire::Integration = api
                .put_as(
                    &[
                        "organizations",
                        &organization,
                        "integrations",
                        &name,
                        "private-key",
                    ],
                    &wire::IntegrationPrivateKeyReplacement {
                        private_key,
                        revision,
                    },
                )
                .await?;
            shown_integration(&presentation, invocation, replaced)?;
        }
        Command::Integration(IntegrationCommand::Github(GithubCommand::Webhook(command))) => {
            let organization = scoping.resolve().await?.organization;
            match command {
                WebhookCommand::SetUrl {
                    name,
                    base,
                    revision,
                } => {
                    let integration: wire::Integration = api
                        .put_as(
                            &[
                                "organizations",
                                &organization,
                                "integrations",
                                &name,
                                "webhook",
                            ],
                            &wire::IntegrationWebhookUrl { base, revision },
                        )
                        .await?;
                    shown_integration(&presentation, invocation, integration)?;
                }
                WebhookCommand::Status { name } => {
                    let hook: wire::IntegrationWebhook = api
                        .get_as(&[
                            "organizations",
                            &organization,
                            "integrations",
                            &name,
                            "webhook",
                        ])
                        .await?;
                    typed::show(&presentation, &view::INTEGRATION_WEBHOOK, &hook)?;
                }
            }
        }
        Command::Integration(IntegrationCommand::Github(GithubCommand::RotateSecret(rotating))) => {
            let organization = scoping.resolve().await?.organization;
            let (RotateSecretCommand::Start { name, .. }
            | RotateSecretCommand::Status { name }
            | RotateSecretCommand::Resume { name }) = &rotating;
            let path = ["organizations", &organization, "integrations", name];
            let rotation = [path.as_slice(), &["webhook-secret-rotation"]].concat();
            let integration: wire::Integration = match rotating {
                RotateSecretCommand::Start { revision, .. } => {
                    api.post_as(
                        &rotation,
                        &wire::IntegrationWebhookSecretRotation { revision },
                    )
                    .await?
                }
                RotateSecretCommand::Status { .. } => api.get_as(&path).await?,
                RotateSecretCommand::Resume { .. } => {
                    api.post_as(&[rotation.as_slice(), &["resume"]].concat(), &())
                        .await?
                }
            };
            shown_rotation(&presentation, invocation, integration)?;
        }
        Command::Integration(IntegrationCommand::Register(RegisterCommand::Webhook {
            name,
            secret,
        })) => {
            let organization = scoping.resolve().await?.organization;
            let registration =
                wire::IntegrationRegistration::WebhookRegistration(wire::WebhookRegistration {
                    kind: json!("webhook"),
                    name,
                    secret,
                    carries: None,
                });
            let registered: wire::Integration = api
                .post_as(
                    &["organizations", &organization, "integrations"],
                    &registration,
                )
                .await?;
            typed::show(&presentation, &view::INTEGRATION_DECLARED, &registered)?;
        }
        Command::Integration(IntegrationCommand::Show { name }) => {
            let organization = scoping.resolve().await?.organization;
            let integration: wire::Integration = api
                .get_as(&["organizations", &organization, "integrations", &name])
                .await?;
            shown_integration(&presentation, invocation, integration)?;
        }
        Command::Integration(IntegrationCommand::Repositories {
            name,
            page,
            per_page,
        }) => {
            let organization = scoping.resolve().await?.organization;
            let page = page.to_string();
            let per_page = per_page.to_string();
            let repositories: wire::IntegrationRepositories = api
                .get_where_as(
                    &[
                        "organizations",
                        &organization,
                        "integrations",
                        &name,
                        "repositories",
                    ],
                    &[("page", &page), ("per_page", &per_page)],
                )
                .await?;
            let mut out = std::io::stdout().lock();
            if presentation.json() {
                typed::document(&mut out, &presentation, &repositories)?;
            } else {
                for repository in repositories.repositories {
                    writeln!(
                        out,
                        "{}\t{}\t{}\t{}\t{}",
                        repository.repository,
                        repository.address,
                        repository.default_branch,
                        if repository.private {
                            "private"
                        } else {
                            "public"
                        },
                        if repository.watched {
                            "watched"
                        } else {
                            "not watched"
                        },
                    )?;
                }
                if let Some(next) = repositories.next_page {
                    writeln!(out, "next page {next} (--page {next})")?;
                }
            }
        }
        Command::Integration(IntegrationCommand::Change {
            name,
            rename,
            carries,
            interval,
            revision,
        }) => {
            let organization = scoping.resolve().await?.organization;
            let change = wire::IntegrationChange {
                name: rename,
                carries: (!carries.is_empty()).then_some(carries),
                interval,
                revision,
            };
            let changed: wire::Integration = api
                .patch_as(
                    &["organizations", &organization, "integrations", &name],
                    &change,
                )
                .await?;
            shown_integration(&presentation, invocation, changed)?;
        }
        Command::Integration(IntegrationCommand::Disable { name }) => {
            let organization = scoping.resolve().await?.organization;
            let disabled: wire::Integration = api
                .post_as(
                    &[
                        "organizations",
                        &organization,
                        "integrations",
                        &name,
                        "disable",
                    ],
                    &json!({}),
                )
                .await?;
            shown_integration(&presentation, invocation, disabled)?;
        }
        Command::Integration(IntegrationCommand::Enable { name }) => {
            let organization = scoping.resolve().await?.organization;
            let enabled: wire::Integration = api
                .post_as(
                    &[
                        "organizations",
                        &organization,
                        "integrations",
                        &name,
                        "enable",
                    ],
                    &json!({}),
                )
                .await?;
            shown_integration(&presentation, invocation, enabled)?;
        }
        Command::Integration(IntegrationCommand::Retire { name, yes }) => {
            let organization = scoping.resolve().await?.organization;
            let path = ["organizations", &organization, "integrations", &name];
            let reviewed: wire::Integration = api.get_as(&path).await?;
            if !yes && !matches!(reviewed.state, wire::IntegrationState::Retired) {
                if !(std::io::stdin().is_terminal() && std::io::stderr().is_terminal()) {
                    bail!(
                        "retiring the Integration {name} is permanent and nothing here can be \
                         asked to confirm it; pass --yes to retire it"
                    );
                }
                eprintln!(
                    "retiring the Integration {name} of the Organization {organization} will"
                );
                for consequence in RETIRING {
                    eprintln!("  {consequence}");
                }
                if !typed(&name)? {
                    eprintln!("nothing was retired");
                    return Ok(());
                }
            }
            let retired: wire::Integration = api
                .post_as(
                    &[path.as_slice(), &["retire"]].concat(),
                    &wire::IntegrationRetirement {
                        revision: Some(reviewed.revision),
                    },
                )
                .await?;
            shown_integration(&presentation, invocation, retired)?;
            if matches!(reviewed.kind, wire::IntegrationKind::Github) {
                eprintln!("{}", github_cleanup(&reviewed));
            }
        }
        Command::Integration(IntegrationCommand::List) => {
            let organization = scoping.resolve().await?.organization;
            let integrations: wire::ListIntegrationsResponse = api
                .get_as(&["organizations", &organization, "integrations"])
                .await?;
            typed::collection(
                &presentation,
                &view::INTEGRATIONS,
                &integrations,
                &empty_list(
                    &client.control_plane,
                    "Integrations",
                    Some(&organization),
                    "integration github register --help",
                ),
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
            let events: wire::ListEventsResponse = api
                .get_where_as(
                    &["organizations", &organization, "events"],
                    &[("limit", &limit.to_string())],
                )
                .await?;
            typed::collection(
                &presentation,
                &view::EVENTS,
                &events,
                &empty_list(
                    &client.control_plane,
                    "Events",
                    Some(&organization),
                    "integration list",
                ),
            )?;
        }
        Command::Event(EventCommand::Show { record }) => {
            let event: wire::EventRecord = api.get_as(&["events", &record]).await?;
            typed::show(&presentation, &view::EVENT, &event)?;
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
            let declaration = wire::TriggerDeclaration {
                name,
                brief,
                branch: branch.map(Some),
                correlation: correlation.map(Some),
                on_miss: on_miss.map(Some),
                on_open_workspace: on_open_workspace.map(Some),
                project,
                agent,
                allows: Some(allows),
                profile: profile.map(Some),
                filter: None,
                every: None,
                cron: None,
                zone: None,
                model: None,
                mode: None,
                thought_level: None,
                variant: trigger_condition(filter, every, cron, zone),
            };
            let declared: wire::Trigger = api
                .post_as(&["organizations", &organization, "triggers"], &declaration)
                .await?;
            if declared.admits_outsiders {
                warn_of_outsiders(&declared.name);
            }
            typed::show(&presentation, &view::TRIGGER_DECLARED, &declared)?;
        }
        Command::Trigger(TriggerCommand::Apply { file, dry_run }) => {
            let organization = scoping.resolve().await?.organization;
            let declarations = trigger_file(&file)?;
            let mut path = vec!["organizations", organization.as_str(), "applied-triggers"];
            if dry_run {
                path.push("preview");
            }
            let applied: wire::AppliedTriggers = api.post_as(&path, &declarations).await?;
            if json {
                show_applied_whole(&presentation, &applied, &applied.admitting_outsiders)?;
            } else {
                show_applied_triggers(applied);
            }
        }
        Command::Trigger(TriggerCommand::Dispatch {
            name,
            integration,
            issue,
            instruction,
            agent,
        }) => {
            let organization = scoping.resolve().await?.organization;
            let mut dispatch = wire::TriggerDispatch::new(integration, issue);
            dispatch.instruction = instruction.as_deref().map(given).transpose()?.map(Some);
            dispatch.agent = agent.map(Some);
            let fired: wire::Fired = api
                .post_as(
                    &[
                        "organizations",
                        &organization,
                        "triggers",
                        &name,
                        "dispatch",
                    ],
                    &dispatch,
                )
                .await?;
            typed::show_as(
                &presentation,
                &view::FIRED,
                &fired,
                &view::FiredShown::of(&fired),
            )?;
        }
        Command::Trigger(TriggerCommand::List) => {
            let organization = scoping.resolve().await?.organization;
            let triggers: wire::ListTriggersResponse = api
                .get_as(&["organizations", &organization, "triggers"])
                .await?;
            typed::collection(
                &presentation,
                &view::TRIGGERS,
                &triggers,
                &empty_list(
                    &client.control_plane,
                    "Triggers",
                    Some(&organization),
                    "trigger declare --help",
                ),
            )?;
        }
        Command::Trigger(TriggerCommand::Show { name }) => {
            let organization = scoping.resolve().await?.organization;
            let trigger: wire::Trigger = api
                .get_as(&["organizations", &organization, "triggers", &name])
                .await?;
            typed::show(&presentation, &view::TRIGGER, &trigger)?;
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
            let declared = file.as_deref().map(trigger_document).transpose()?;
            let instruction = instruction.as_deref().map(given).transpose()?;
            let event = event
                .map(|event| {
                    event.parse::<uuid::Uuid>().map_err(|_| {
                        Failed::new(Exit::Usage, format!("{event} is not an Event's record"))
                    })
                })
                .transpose()?;
            let tested: wire::TriggerTestResult = api
                .post_as(
                    &["organizations", &organization, "triggers", &name, "test"],
                    &wire::TriggerTest {
                        event: Some(event),
                        integration: Some(integration),
                        issue: Some(issue),
                        instruction: Some(instruction),
                        agent: Some(agent),
                        declared: Some(declared),
                    },
                )
                .await?;
            typed::show(&presentation, &view::TRIGGER_TEST, &tested)?;
        }
        Command::Trigger(TriggerCommand::Disable { name }) => {
            let organization = scoping.resolve().await?.organization;
            let trigger: wire::Trigger = api
                .post_as(
                    &["organizations", &organization, "triggers", &name, "disable"],
                    &json!({}),
                )
                .await?;
            typed::show(&presentation, &view::TRIGGER_STATE, &trigger)?;
        }
        Command::Trigger(TriggerCommand::Enable { name }) => {
            let organization = scoping.resolve().await?.organization;
            let trigger: wire::Trigger = api
                .post_as(
                    &["organizations", &organization, "triggers", &name, "enable"],
                    &json!({}),
                )
                .await?;
            typed::show(&presentation, &view::TRIGGER_STATE, &trigger)?;
        }
        Command::Workspace(WorkspaceCommand::Open {
            project,
            agent,
            profile,
            sign_in,
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
            let opened: wire::Opened = api
                .post_as(
                    &["organizations", &organization, "workspaces"],
                    &wire::WorkspaceDeclaration {
                        project,
                        agent,
                        profile: Some(profile),
                        sign_in_method: Some(sign_in),
                        branch: Some(branch),
                        continues: Some(continues),
                        model: Some(model),
                        mode: Some(mode),
                        thought_level: Some(thought_level),
                        brief: Some(brief),
                        participant: Some(as_participant),
                        depends_on: Some(Some(depends_on)),
                    },
                )
                .await?;
            typed::show(&presentation, &view::OPENED, &opened)?;
        }
        Command::Workspace(WorkspaceCommand::List) => {
            let organization = scoping.resolve().await?.organization;
            typed::collection(
                &presentation,
                &view::WORKSPACES,
                &api.get_as::<wire::ListWorkspacesResponse>(&[
                    "organizations",
                    &organization,
                    "workspaces",
                ])
                .await?,
                &empty_list(
                    &client.control_plane,
                    "Workspaces",
                    Some(&organization),
                    "workspace open --help",
                ),
            )?;
        }
        Command::Workspace(WorkspaceCommand::Work { workspace }) => {
            let organization = scoping.resolve().await?.organization;
            let answer = api
                .get_as::<wire::WorkspaceWork>(&[
                    "organizations",
                    &organization,
                    "workspaces",
                    &workspace,
                    "work",
                ])
                .await?;
            work::show(&presentation, &answer)?;
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
                .get_where_as::<wire::WorkspaceChanges>(
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
            changes::show(
                &presentation,
                &changes::Read::Changes(answer),
                &empty_list(
                    &client.control_plane,
                    "changes in this comparison",
                    Some(&organization),
                    &format!("workspace work {}", shell::quoted(&workspace)),
                ),
            )?;
        }
        Command::Workspace(
            command @ (WorkspaceCommand::Commits { .. } | WorkspaceCommand::Stashes { .. }),
        ) => {
            let organization = scoping.resolve().await?.organization;
            let path = |workspace, read| {
                [
                    "organizations",
                    organization.as_str(),
                    "workspaces",
                    workspace,
                    read,
                ]
            };
            let (workspace, read, answer) = match command {
                WorkspaceCommand::Commits { workspace } => {
                    let commits = api.get_as(&path(&workspace, "commits")).await?;
                    (workspace, "commits", changes::Read::Commits(commits))
                }
                WorkspaceCommand::Stashes { workspace } => {
                    let stashes = api.get_as(&path(&workspace, "stashes")).await?;
                    (workspace, "stashes", changes::Read::Stashes(stashes))
                }
                _ => unreachable!(),
            };
            changes::show(
                &presentation,
                &answer,
                &empty_list(
                    &client.control_plane,
                    read,
                    Some(&organization),
                    &format!("workspace work {}", shell::quoted(&workspace)),
                ),
            )?;
        }
        Command::Workspace(WorkspaceCommand::Files { workspace, path }) => {
            let organization = scoping.resolve().await?.organization;
            let path = path.unwrap_or_default();
            let answer = api
                .get_where_as::<wire::FileListing>(
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
            files::list(
                &presentation,
                &answer,
                &empty_list(
                    &client.control_plane,
                    "directory entries",
                    Some(&organization),
                    &format!("workspace files {}", shell::quoted(&workspace)),
                ),
            )?;
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
            files::read(response, &presentation).await?;
        }
        Command::Workspace(WorkspaceCommand::Show { workspace }) => {
            let organization = scoping.resolve().await?.organization;
            typed::show(
                &presentation,
                &view::WORKSPACE,
                &api.get_as::<wire::Workspace>(&[
                    "organizations",
                    &organization,
                    "workspaces",
                    &workspace,
                ])
                .await?,
            )?;
        }
        Command::Workspace(WorkspaceCommand::Post {
            workspace,
            as_participant,
            message,
        }) => {
            let organization = scoping.resolve().await?.organization;
            let answer: wire::Posted = api
                .post_as(
                    &[
                        "organizations",
                        &organization,
                        "workspaces",
                        &workspace,
                        "messages",
                    ],
                    &wire::WorkspaceMessage {
                        participant: as_participant,
                        message,
                    },
                )
                .await?;
            if json {
                typed::show(&presentation, &view::POSTED, &answer)?;
            } else if let Some(session) = &answer.session {
                typed::show(&presentation, &view::SESSION_DECLARED, session)?;
            } else if let Some(held) = &answer.held_message {
                typed::show(&presentation, &view::HELD_DECLARED, held)?;
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
            let answer: wire::HeldMessage = api
                .put_as(
                    &[
                        "organizations",
                        &organization,
                        "workspaces",
                        &workspace,
                        "messages",
                        &id.to_string(),
                    ],
                    &wire::WorkspaceMessage {
                        participant: as_participant,
                        message,
                    },
                )
                .await?;
            typed::show(&presentation, &view::HELD_MESSAGE, &answer)?;
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
                &wire::WorkspaceMessageWithdrawal {
                    participant: as_participant,
                },
            )
            .await?;
        }
        Command::Workspace(WorkspaceCommand::Seal { workspace }) => {
            let organization = scoping.resolve().await?.organization;
            typed::show(
                &presentation,
                &view::WORKSPACE_DECLARED,
                &api.post_as::<wire::Workspace>(
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
            sign_in,
            model,
            mode,
            thought_level,
            depends_on,
        }) => {
            let organization = scoping.resolve().await?.organization;
            typed::show(
                &presentation,
                &view::SESSION_DECLARED,
                &api.post_as::<wire::Session>(
                    &[
                        "organizations",
                        &organization,
                        "workspaces",
                        &workspace,
                        "sessions",
                    ],
                    &wire::SessionDeclaration {
                        agent: Some(agent),
                        sign_in_method: Some(sign_in),
                        model: Some(model),
                        mode: Some(mode),
                        thought_level: Some(thought_level),
                        depends_on: Some(Some(depends_on)),
                    },
                )
                .await?,
            )?;
        }
        Command::Session(SessionCommand::List { workspace }) => {
            let organization = scoping.resolve().await?.organization;
            typed::collection(
                &presentation,
                &view::SESSIONS,
                &api.get_as::<wire::ListSessionsResponse>(&[
                    "organizations",
                    &organization,
                    "workspaces",
                    &workspace,
                    "sessions",
                ])
                .await?,
                &empty_list(
                    &client.control_plane,
                    "Sessions",
                    Some(&organization),
                    &format!("workspace show {}", shell::quoted(&workspace)),
                ),
            )?;
        }
        Command::Session(SessionCommand::Show { session }) => {
            let organization = scoping.resolve().await?.organization;
            shown_session(
                &presentation,
                invocation,
                &api.get_as::<wire::Session>(&[
                    "organizations",
                    &organization,
                    "sessions",
                    &session,
                ])
                .await?,
            )?;
        }
        Command::Session(SessionCommand::Interrupt {
            session,
            as_participant,
        }) => {
            let organization = scoping.resolve().await?.organization;
            typed::show(
                &presentation,
                &view::INTERRUPTED,
                &api.post_as::<wire::Session>(
                    &[
                        "organizations",
                        &organization,
                        "sessions",
                        &session,
                        "interrupt",
                    ],
                    &wire::SessionInterrupt {
                        participant: as_participant,
                    },
                )
                .await?,
            )?;
        }
        Command::Session(SessionCommand::Stop { session }) => {
            let organization = scoping.resolve().await?.organization;
            typed::show(
                &presentation,
                &view::STOPPED,
                &api.post_as::<wire::Session>(
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
            let body = wire::OptionChange {
                participant: Some(as_participant),
                category: declares(&option).then(|| Some(option.clone())),
                option: (!declares(&option)).then(|| Some(option.clone())),
                value,
            };
            let changed: wire::Session = api
                .post_as(
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
            shown_session(&presentation, invocation, &changed)?;
        }
        Command::Instance(InstanceCommand::List) => {
            let organization = scoping.resolve().await?.organization;
            typed::collection(
                &presentation,
                &view::INSTANCES,
                &api.get_as::<wire::ListHeldInstancesResponse>(&[
                    "organizations",
                    &organization,
                    "instances",
                ])
                .await?,
                &empty_list(
                    &client.control_plane,
                    "Instances",
                    Some(&organization),
                    "workspace list",
                ),
            )?;
        }
        Command::Instance(InstanceCommand::Release {
            workspace,
            as_participant,
        }) => {
            let organization = scoping.resolve().await?.organization;
            typed::show(
                &presentation,
                &view::RELEASED,
                &api.post_as::<wire::ReleasedInstance>(
                    &[
                        "organizations",
                        &organization,
                        "workspaces",
                        &workspace,
                        "instance",
                        "release",
                    ],
                    &wire::InstanceRelease {
                        participant: as_participant.map(Some),
                    },
                )
                .await?,
            )?;
        }
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
async fn started(
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
    let mut existing = start::Existing::read(declared, projects, agents, credentials);
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
        if declared {
            let declaring: wire::ListProjectsResponse = api
                .get_where_as(
                    &["organizations", &organization.value, "projects"],
                    &[("repository", &origin)],
                )
                .await?;
            existing.declaring = Some(declaring.into_iter().map(|record| record.name).collect());
        }
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

fn selections(
    repositories: &[String],
    integrations: &[String],
) -> Result<Vec<wire::RepositoryIntegrationSelection>> {
    integrations
        .iter()
        .map(|given| match (given.rsplit_once('='), repositories) {
            (Some((repository, integration)), _) => Ok(wire::RepositoryIntegrationSelection {
                repository: repository.to_owned(),
                integration: integration.to_owned(),
            }),
            (None, [repository]) => Ok(wire::RepositoryIntegrationSelection {
                repository: repository.clone(),
                integration: given.clone(),
            }),
            (None, _) => bail!(Failed::new(
                Exit::Usage,
                format!(
                    "--integration {given} names no repository; beside several, give it as \
                     REPOSITORY={given}"
                )
            )),
        })
        .collect()
}

async fn resolved(api: &ControlPlane, repositories: Vec<String>) -> Result<Vec<String>> {
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

fn empty_list(
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

const RETIRING: [&str; 5] = [
    "erase the credentials kestrel holds for it, for good",
    "cancel what it has not yet posted, and the Firings held on it",
    "refuse every later use, change or enable; no other Integration takes its place",
    "keep it and its name, and the Projects, Triggers, Events and Workspaces that refer to it",
    "change nothing on GitHub",
];

fn typed(name: &str) -> Result<bool> {
    eprint!("type {name} to retire it: ");
    let mut answer = String::new();
    std::io::stdin()
        .read_line(&mut answer)
        .context("reading the answer")?;

    Ok(answer.trim() == name)
}

/// Optional, and the operator's to do: kestrel changes nothing on GitHub (ADR-0056).
fn github_cleanup(integration: &wire::Integration) -> String {
    let app = integration.bot_login.as_deref().unwrap_or("the GitHub App");
    let repository = integration
        .repository
        .as_deref()
        .unwrap_or("its repository");
    format!(
        "kestrel erased its own copy of {app}'s credentials. It changed nothing on GitHub: the \
         App is still installed on {repository} and its private keys still work. A request \
         already sent, or a token already issued, was not recalled; an installation token \
         lapses within the hour.\n\
         If nothing else uses the App, you can finish on GitHub, in the App's settings:\n  \
         delete its private keys under General\n  \
         uninstall it from {repository} under Install App\n  \
         delete the App under Advanced"
    )
}

/// What retiring an Integration with unsent posts left unsaid, each as one line.
fn canceled_posts(integration: &wire::Integration) -> String {
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

fn shown_integration(
    presentation: &Presentation,
    invocation: &Invocation,
    integration: wire::Integration,
) -> Result<()> {
    let canceled_posts = canceled_posts(&integration);
    let diagnostic = integration.diagnostic.as_ref().map_or_else(
        || crate::output::ABSENT.to_owned(),
        |diagnostic| diagnostic::inline(diagnostic, invocation),
    );
    let rotation = rotation(&integration, invocation);
    let shown = view::IntegrationShown {
        integration,
        canceled_posts,
        diagnostic,
        rotation,
    };

    typed::show_as(presentation, &view::INTEGRATION, &shown.integration, &shown)
}

fn rotation(integration: &wire::Integration, invocation: &Invocation) -> String {
    integration.webhook_secret_rotation.as_ref().map_or_else(
        || crate::output::ABSENT.to_owned(),
        |rotation| {
            format!(
                "unfinished since {}\n{}",
                rotation.started_at,
                diagnostic::inline(&rotation.diagnostic, invocation)
            )
        },
    )
}

/// The whole Integration as JSON, so a script reads the rotation where every Client does.
fn shown_rotation(
    presentation: &Presentation,
    invocation: &Invocation,
    integration: wire::Integration,
) -> Result<()> {
    let shown = view::RotationShown {
        rotation: match &integration.webhook_secret_rotation {
            Some(_) => rotation(&integration, invocation),
            None => "none under way".to_owned(),
        },
        integration,
    };

    typed::show_as(presentation, &view::ROTATION, &shown.integration, &shown)
}

fn direction(given: &str) -> Result<wire::Direction, String> {
    [wire::Direction::Inbound, wire::Direction::Outbound]
        .into_iter()
        .find(|direction| direction.as_str() == given)
        .ok_or_else(|| format!("{given} is not a direction: inbound or outbound"))
}

/// A failed Session is still a successful read: its diagnostic is shown, never raised.
fn shown_session(
    presentation: &Presentation,
    invocation: &Invocation,
    session: &wire::Session,
) -> Result<()> {
    let shown = view::SessionShown {
        session: session.clone(),
        diagnostic: session.diagnostic.as_ref().map_or_else(
            || crate::output::ABSENT.to_owned(),
            |diagnostic| diagnostic::inline(diagnostic, invocation),
        ),
        prose: matches!(presentation, Presentation::Human(_)),
    };

    typed::show_as(presentation, &view::SESSION, session, &shown)
}

fn warn_about_cache(changed: &wire::Session, named: &str) {
    if !changed.options.iter().any(|option| {
        (option.id == named || option.category.as_deref() == Some(named)) && option.warns_cache
    }) {
        return;
    }
    match changed.usage.as_ref().map(|usage| usage.context_used) {
        Some(tokens) => eprintln!(
            "changing this makes the next turn re-read the context without the prompt cache: {tokens} tokens"
        ),
        None => eprintln!(
            "changing this makes the next turn re-read the context without the prompt cache"
        ),
    }
}

fn declares(category: &str) -> bool {
    matches!(category, "model" | "mode" | "thought_level")
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

/// The retired `--json FIELDS` took the next word as its value, so any word there is refused
/// rather than read as an argument, which could post a field name as a message.
fn json_value(args: &[String]) -> Option<(&str, Vec<String>)> {
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

/// The schedule form as the generated union carries it: exactly one of a filter, an interval,
/// or a cron expression with its zone.
fn trigger_condition(
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

/// The declaration file as the generated request carries it, so a document the control plane
/// could not read is refused before it is sent.
fn trigger_document(file: &str) -> Result<wire::TriggerFile> {
    declaration_document(trigger_file(file)?)
}

fn declaration_document(document: Value) -> Result<wire::TriggerFile> {
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
fn unread_field(sent: &Value, read: &Value) -> Option<String> {
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

fn sign(action: &wire::DeclarationChangeAction) -> char {
    match action {
        wire::DeclarationChangeAction::Add => '+',
        wire::DeclarationChangeAction::Change => '~',
        wire::DeclarationChangeAction::Unchanged => '=',
        wire::DeclarationChangeAction::Remove => '-',
    }
}

fn show_applied_triggers(applied: wire::AppliedTriggers) {
    if applied.changes.is_empty() {
        println!("no changes");
    }
    for change in applied.changes {
        println!("{} {}", sign(&change.action), change.name);
        show_differences(change.differences);
    }
    for name in applied.admitting_outsiders {
        warn_of_outsiders(&name);
    }
}

fn show_differences(differences: Vec<wire::DeclarationDifference>) {
    for difference in differences {
        println!("    {}", difference.field);
        for (sign, value) in [('-', difference.was), ('+', difference.becomes)] {
            for line in value.iter().flat_map(|value| value.lines()) {
                println!("      {sign} {line}");
            }
        }
    }
}

fn show_declaration(applied: wire::AppliedDeclaration) {
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

fn show_applied_whole(
    presentation: &Presentation,
    applied: &impl serde::Serialize,
    admitting_outsiders: &[String],
) -> Result<()> {
    for name in admitting_outsiders {
        warn_of_outsiders(name);
    }
    typed::whole(presentation, applied)
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

#[derive(Serialize)]
struct StatusLocation {
    control_plane: String,
    control_plane_source: String,
}

#[derive(Serialize)]
struct Status {
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
struct UnresolvedStatus {
    #[serde(flatten)]
    location: StatusLocation,
    organization: Option<String>,
    organization_source: Option<String>,
    organizations: Vec<String>,
    next: String,
}

async fn status(
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

fn next_command(project: Option<&wire::Project>, agent: Option<&wire::Agent>) -> String {
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

/// A file is held exactly as it was read, because it is written back exactly as it was held.
fn read_the_login(file: bool) -> Result<String> {
    let mut read = read_standard_input("a login")?;
    if !file && read.ends_with('\n') {
        read.pop();
        if read.ends_with('\r') {
            read.pop();
        }
    }

    if read.is_empty() {
        bail!(Failed::new(
            Exit::Usage,
            "a login is read from standard input, and nothing was on it"
        ));
    }

    Ok(read)
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
