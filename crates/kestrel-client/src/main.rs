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
mod usage;
mod view;
mod work;

use std::process::ExitCode;

use anyhow::{Context as _, Result, bail};
use clap::builder::NonEmptyStringValueParser;
use clap::parser::ValueSource;
use clap::{CommandFactory as _, FromArgMatches as _, Parser};
use kestrel_operator_types as wire;
use reqwest::Url;

use crate::api::ControlPlane;
use crate::command::{Command, Running};
use crate::diagnostic::Invocation;
use crate::exit::{Exit, Failed};
use crate::output::Presentation;
use crate::scope::{Scope, Scoping, Source};
use crate::usage::{exit_after, json_value};

pub(crate) use crate::command::status::{Status, UnresolvedStatus};

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
    let running = Running {
        api: &api,
        presentation: &presentation,
        scoping,
        invocation,
        control_plane: &client.control_plane,
        url: &control_plane,
    };
    let Some(command) = client.command else {
        return readiness::show(&api, running.scoping, invocation, &presentation).await;
    };
    match command {
        Command::Start(start) => command::start::run(running, start).await,
        Command::Apply(apply) => command::declaration::run(running, apply).await,
        Command::Operator(command) => command::operator::run(running, command).await,
        Command::Organization(command) => command::organization::run(running, command).await,
        Command::Project(command) => command::project::run(running, command).await,
        Command::Agent(command) => command::agent::run(running, command).await,
        Command::Credential(command) => command::credential::run(running, command).await,
        Command::Login(login) => command::login::run(running, login).await,
        Command::SignIn(command) => command::sign_in::run(running, command).await,
        Command::Profile(command) => command::profile::run(running, command).await,
        Command::Integration(command) => command::integration::run(running, command).await,
        Command::Event(command) => command::event::run(running, command).await,
        Command::Trigger(command) => command::trigger::run(running, command).await,
        Command::Workspace(command) => command::workspace::run(running, command).await,
        Command::Session(command) => command::session::run(running, command).await,
        Command::Instance(command) => command::instance::run(running, command).await,
        Command::Status => {
            let location = command::status::StatusLocation {
                control_plane: client.control_plane.clone(),
                control_plane_source: control_plane_source.to_string(),
            };
            command::status::show(
                &api,
                &presentation,
                location,
                running.scoping.derive().await?,
            )
            .await
        }
        Command::Queue => {
            let organization = running.scoping.resolve().await?.organization;
            let snapshot: wire::Queue = api
                .get_as(&["organizations", &organization, "queue"])
                .await?;
            queue::show(&presentation, &snapshot, invocation)
        }
        Command::ExitCodes => unreachable!("the catalog is shown before the control plane is"),
    }
}
