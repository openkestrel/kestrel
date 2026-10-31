use anyhow::Result;
use clap::Subcommand;
use kestrel_operator_types as wire;
use serde_json::json;

use super::{Running, empty_list};
use crate::diagnostic::{self, Invocation};
use crate::output::Presentation;
use crate::{shell, typed, view};

#[derive(Debug, Subcommand)]
pub(crate) enum SessionCommand {
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
pub(crate) enum SessionOptionCommand {
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

pub(crate) async fn run(running: Running<'_>, command: SessionCommand) -> Result<()> {
    let Running {
        api,
        presentation,
        scoping,
        invocation,
        control_plane,
        ..
    } = running;
    match command {
        SessionCommand::Enqueue {
            workspace,
            agent,
            sign_in,
            model,
            mode,
            thought_level,
            depends_on,
        } => {
            let organization = scoping.resolve().await?.organization;
            typed::show(
                presentation,
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
        SessionCommand::List { workspace } => {
            let organization = scoping.resolve().await?.organization;
            typed::collection(
                presentation,
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
                    control_plane,
                    "Sessions",
                    Some(&organization),
                    &format!("workspace show {}", shell::quoted(&workspace)),
                ),
            )?;
        }
        SessionCommand::Show { session } => {
            let organization = scoping.resolve().await?.organization;
            shown_session(
                presentation,
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
        SessionCommand::Interrupt {
            session,
            as_participant,
        } => {
            let organization = scoping.resolve().await?.organization;
            typed::show(
                presentation,
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
        SessionCommand::Stop { session } => {
            let organization = scoping.resolve().await?.organization;
            typed::show(
                presentation,
                &view::STOPPED,
                &api.post_as::<wire::Session>(
                    &["organizations", &organization, "sessions", &session, "stop"],
                    &json!({}),
                )
                .await?,
            )?;
        }
        SessionCommand::Option { command } => {
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
            shown_session(presentation, invocation, &changed)?;
        }
    }

    Ok(())
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
