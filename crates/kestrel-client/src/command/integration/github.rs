mod connect;
use crate::browser as reach;

use anyhow::{Context as _, Result};
use clap::Subcommand;
use kestrel_operator_types as wire;
use serde_json::json;

use super::{direction, rotation, shown_integration};
use crate::command::Running;
use crate::command::input::{given, read_standard_input};
use crate::diagnostic::Invocation;
use crate::exit::{Exit, Failed};
use crate::output::Presentation;
use crate::{typed, view};

#[derive(Debug, Subcommand)]
pub(crate) enum GithubCommand {
    /// Create a GitHub App through the browser and connect one repository it is installed on
    Connect(connect::Connect),
    /// Register an existing GitHub App's installation, watching one repository
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
pub(crate) enum WebhookCommand {
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
pub(crate) enum RotateSecretCommand {
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

pub(super) async fn run(running: Running<'_>, command: GithubCommand) -> Result<()> {
    let Running {
        api,
        presentation,
        scoping,
        invocation,
        control_plane,
        url,
    } = running;
    match command {
        GithubCommand::Connect(connect) => {
            let running = Running {
                api,
                presentation,
                scoping,
                invocation,
                control_plane,
                url,
            };
            connect::run(running, connect).await?;
        }
        GithubCommand::Register {
            name,
            repository,
            app_id,
            installation,
            private_key,
            carries,
            interval,
            webhook_secret,
            api: github_api,
        } => {
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
        GithubCommand::ReplaceKey {
            name,
            private_key_file,
            revision,
        } => {
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
        GithubCommand::Webhook(command) => {
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
                    shown_integration(presentation, invocation, integration)?;
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
                    typed::show(presentation, &view::INTEGRATION_WEBHOOK, &hook)?;
                }
            }
        }
        GithubCommand::RotateSecret(rotating) => {
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
            shown_rotation(presentation, invocation, integration)?;
        }
    }

    Ok(())
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
