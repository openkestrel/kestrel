use anyhow::{Result, bail};
use clap::{Args, Subcommand};
use kestrel_operator_types as wire;
use serde_json::json;

use super::input::read_standard_input;
use super::{Running, empty_list};
use crate::exit::{Exit, Failed};
use crate::{diagnostic, typed, view};

#[derive(Debug, Subcommand)]
pub(crate) enum ProfileCommand {
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
pub(crate) struct ProfileEntry {
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

pub(crate) async fn run(running: Running<'_>, command: ProfileCommand) -> Result<()> {
    let Running {
        api,
        presentation,
        scoping,
        control_plane,
        ..
    } = running;
    match command {
        ProfileCommand::Declare { name, owner } => {
            let organization = scoping.resolve().await?.organization;
            let profile: wire::SubscriptionProfile = api
                .post_as(
                    &["organizations", &organization, "profiles"],
                    &wire::SubscriptionProfileDeclaration { name, owner },
                )
                .await?;
            typed::show(presentation, &view::PROFILE_DECLARED, &profile)?;
        }
        ProfileCommand::Set { name, owner, entry } => {
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
            typed::show(presentation, &view::LOGIN, &login)?;
        }
        ProfileCommand::List => {
            let organization = scoping.resolve().await?.organization;
            let profiles: wire::ListSubscriptionProfilesResponse = api
                .get_as(&["organizations", &organization, "profiles"])
                .await?;
            typed::collection(
                presentation,
                &view::PROFILES,
                &profiles,
                &empty_list(
                    control_plane,
                    "Subscription Profiles",
                    Some(&organization),
                    "profile declare --help",
                ),
            )?;
        }
        ProfileCommand::Forget { name, entry } => {
            let organization = scoping.resolve().await?.organization;
            api.delete(&entry.path(&organization, &name), &json!({}))
                .await?;
        }
    }

    Ok(())
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
