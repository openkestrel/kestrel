use std::io::{IsTerminal as _, Write as _};

use anyhow::{Context as _, Result, bail};
use clap::Subcommand;
use kestrel_operator_types as wire;
use serde_json::json;

use crate::canceled_posts;
use crate::command::Running;
use crate::diagnostic::{self, Invocation};
use crate::exit::{Exit, Failed};
use crate::output::Presentation;
use crate::scope::Scoping;
use crate::{direction, empty_list, given, read_standard_input};
use crate::{typed, view};

#[derive(Debug, Subcommand)]
pub(crate) enum IntegrationCommand {
    /// Create a GitHub App and register its repository Integration through the browser
    CreateGithubApp {
        name: String,
        /// Create the private App under this GitHub organization instead of your personal account
        #[arg(long)]
        app_organization: Option<String>,
        /// Browser-reachable loopback origin (defaults to the control-plane URL)
        #[arg(long)]
        callback_base: Option<String>,
        /// Public HTTPS origin of the webhook listener; omit to poll GitHub
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
pub(crate) enum GithubCommand {
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

pub(crate) async fn run(
    command: IntegrationCommand,
    scoping: Scoping<'_>,
    running: Running<'_>,
) -> Result<()> {
    let Running {
        api,
        presentation,
        control_plane,
        control_plane_given,
        invocation,
        ..
    } = running;
    match command {
        IntegrationCommand::CreateGithubApp {
            name,
            app_organization,
            callback_base,
            webhook_base,
            api: github_api,
        } => {
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
            typed::show(presentation, &view::GITHUB_APP_STARTED, &started)?;
        }
        IntegrationCommand::Github(GithubCommand::Register {
            name,
            repository,
            app_id,
            installation,
            private_key,
            carries,
            interval,
            webhook_secret,
            api: github_api,
        }) => {
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
            typed::show(presentation, &view::INTEGRATION_DECLARED, &registered)?;
        }
        IntegrationCommand::Github(GithubCommand::ReplaceKey {
            name,
            private_key_file,
            revision,
        }) => {
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
            shown_integration(presentation, invocation, replaced)?;
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
                    control_plane_given,
                    "Integrations",
                    Some(&organization),
                    "integration github register --help",
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
    let shown = view::IntegrationShown {
        integration,
        canceled_posts,
        diagnostic,
    };

    typed::show_as(presentation, &view::INTEGRATION, &shown.integration, &shown)
}
