mod github;

use std::io::{IsTerminal as _, Write as _};

use anyhow::{Context as _, Result, bail};
use clap::Subcommand;
use kestrel_operator_types as wire;
use serde_json::json;

use self::github::GithubCommand;
use super::{Running, empty_list};
use crate::diagnostic::{self, Invocation};
use crate::output::Presentation;
use crate::{typed, view};

#[derive(Debug, Subcommand)]
pub(crate) enum IntegrationCommand {
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
pub(crate) enum RegisterCommand {
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

pub(crate) async fn run(running: Running<'_>, command: IntegrationCommand) -> Result<()> {
    let Running {
        api,
        presentation,
        scoping,
        invocation,
        control_plane,
        url,
    } = running;
    match command {
        IntegrationCommand::Github(command) => {
            let running = Running {
                api,
                presentation,
                scoping,
                invocation,
                control_plane,
                url,
            };
            github::run(running, command).await?;
        }
        IntegrationCommand::Register(RegisterCommand::Webhook { name, secret }) => {
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
            typed::show(presentation, &view::INTEGRATION_DECLARED, &registered)?;
        }
        IntegrationCommand::Show { name } => {
            let organization = scoping.resolve().await?.organization;
            let integration: wire::Integration = api
                .get_as(&["organizations", &organization, "integrations", &name])
                .await?;
            shown_integration(presentation, invocation, integration)?;
        }
        IntegrationCommand::Repositories {
            name,
            page,
            per_page,
        } => {
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
                typed::document(&mut out, presentation, &repositories)?;
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
        IntegrationCommand::Change {
            name,
            rename,
            carries,
            interval,
            revision,
        } => {
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
            shown_integration(presentation, invocation, changed)?;
        }
        IntegrationCommand::Disable { name } => {
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
            shown_integration(presentation, invocation, disabled)?;
        }
        IntegrationCommand::Enable { name } => {
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
            shown_integration(presentation, invocation, enabled)?;
        }
        IntegrationCommand::Retire { name, yes } => {
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
            shown_integration(presentation, invocation, retired)?;
            if matches!(reviewed.kind, wire::IntegrationKind::Github) {
                eprintln!("{}", github_cleanup(&reviewed));
            }
        }
        IntegrationCommand::List => {
            let organization = scoping.resolve().await?.organization;
            let integrations: wire::ListIntegrationsResponse = api
                .get_as(&["organizations", &organization, "integrations"])
                .await?;
            typed::collection(
                presentation,
                &view::INTEGRATIONS,
                &integrations,
                &empty_list(
                    control_plane,
                    "Integrations",
                    Some(&organization),
                    "integration github connect --help",
                ),
            )?;
        }
        IntegrationCommand::AcknowledgeRefusal { name } => {
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
    }

    Ok(())
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

pub(super) fn shown_integration(
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

pub(super) fn rotation(integration: &wire::Integration, invocation: &Invocation) -> String {
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

pub(super) fn direction(given: &str) -> Result<wire::Direction, String> {
    [wire::Direction::Inbound, wire::Direction::Outbound]
        .into_iter()
        .find(|direction| direction.as_str() == given)
        .ok_or_else(|| format!("{given} is not a direction: inbound or outbound"))
}
