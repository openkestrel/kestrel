use anyhow::{Result, bail};
use clap::Subcommand;
use kestrel_operator_types as wire;
use serde_json::json;

use crate::command::Running;
use crate::empty_list;
use crate::exit::{Exit, Failed};
use crate::read_standard_input;
use crate::scope::Scoping;
use crate::{typed, view};

#[derive(Debug, Subcommand)]
#[command(
    after_help = "Manage a Provider Credential with `kestrel credential set <variable>` and `kestrel credential forget <variable>`."
)]
pub(crate) enum CredentialCommand {
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

pub(crate) async fn run(
    command: CredentialCommand,
    scoping: Scoping<'_>,
    running: Running<'_>,
) -> Result<()> {
    let Running {
        api,
        presentation,
        control_plane_given,
        ..
    } = running;
    match command {
        CredentialCommand::Set { variable } => {
            let organization = scoping.resolve().await?.organization;
            let held: wire::ProviderCredential = api
                .put_as(
                    &["organizations", &organization, "credentials", &variable],
                    &wire::ProviderCredentialSecret {
                        secret: read_the_secret()?,
                    },
                )
                .await?;
            typed::show(presentation, &view::CREDENTIAL, &held)?;
        }
        CredentialCommand::List => {
            let organization = scoping.resolve().await?.organization;
            let credentials: wire::ListProviderCredentialsResponse = api
                .get_as(&["organizations", &organization, "credentials"])
                .await?;
            typed::collection(
                presentation,
                &view::CREDENTIALS,
                &credentials,
                &empty_list(
                    control_plane_given,
                    "Provider Credentials",
                    Some(&organization),
                    "credential set --help",
                ),
            )?;
        }
        CredentialCommand::Forget { variable } => {
            let organization = scoping.resolve().await?.organization;
            api.delete(
                &["organizations", &organization, "credentials", &variable],
                &json!({}),
            )
            .await?;
        }
    }

    Ok(())
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
