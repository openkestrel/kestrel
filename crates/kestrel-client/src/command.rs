pub(crate) mod agent;
pub(crate) mod credential;
pub(crate) mod declaration;
pub(crate) mod event;
mod input;
pub(crate) mod instance;
pub(crate) mod integration;
pub(crate) mod login;
pub(crate) mod operator;
pub(crate) mod organization;
pub(crate) mod profile;
pub(crate) mod project;
mod relay;
pub(crate) mod session;
pub(crate) mod sign_in;
pub(crate) mod start;
pub(crate) mod status;
pub(crate) mod trigger;
pub(crate) mod workspace;

use clap::Subcommand;
use reqwest::Url;

use crate::api::ControlPlane;
use crate::diagnostic::Invocation;
use crate::output::Presentation;
use crate::scope::Scoping;
use crate::shell;

use self::agent::AgentCommand;
use self::credential::CredentialCommand;
use self::declaration::Apply;
use self::event::EventCommand;
use self::instance::InstanceCommand;
use self::integration::IntegrationCommand;
use self::login::Login;
use self::operator::OperatorCommand;
use self::organization::OrganizationCommand;
use self::profile::ProfileCommand;
use self::project::ProjectCommand;
use self::session::SessionCommand;
use self::sign_in::SignInCommand;
use self::start::Start;
use self::trigger::TriggerCommand;
use self::workspace::WorkspaceCommand;

#[derive(Debug, Subcommand)]
pub(crate) enum Command {
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
    /// Sign in to a harness, or import a token, key or login file, and save it where its Sign-in
    /// Method says: a subscription in your own Profile, a key with the Organization
    Login(Login),
    /// Test saved sign-ins
    #[command(subcommand)]
    SignIn(SignInCommand),
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
    /// Generate shell integration for dynamic completion of commands, flags and scoped names
    Completions {
        #[arg(value_enum)]
        shell: crate::completion::Shell,
    },
}

impl Command {
    pub(crate) fn scoped(&self) -> bool {
        match self {
            Command::Start(_)
            | Command::Apply(_)
            | Command::Project(_)
            | Command::Agent(_)
            | Command::Credential(_)
            | Command::Login(_)
            | Command::SignIn(_)
            | Command::Profile(_)
            | Command::Integration(_)
            | Command::Trigger(_)
            | Command::Workspace(_)
            | Command::Session(_)
            | Command::Instance(_)
            | Command::Queue
            | Command::Status => true,
            Command::Operator(_)
            | Command::Organization(_)
            | Command::ExitCodes
            | Command::Completions { .. } => false,
            Command::Event(event) => match event {
                EventCommand::List { .. } => true,
                EventCommand::Show { .. } => false,
            },
        }
    }
}

/// The control plane is kept as given beside its parsed URL, because guidance names it back as given.
pub(crate) struct Running<'a> {
    pub(crate) api: &'a ControlPlane,
    pub(crate) presentation: &'a Presentation,
    pub(crate) scoping: Scoping<'a>,
    pub(crate) invocation: &'a Invocation,
    pub(crate) control_plane: &'a str,
    pub(crate) url: &'a Url,
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
