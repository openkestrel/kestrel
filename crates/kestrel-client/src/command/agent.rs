use anyhow::Result;
use clap::Subcommand;
use kestrel_operator_types as wire;

use super::{Running, empty_list};
use crate::{typed, view};

#[derive(Debug, Subcommand)]
pub(crate) enum AgentCommand {
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

pub(crate) async fn run(running: Running<'_>, command: AgentCommand) -> Result<()> {
    let Running {
        api,
        presentation,
        scoping,
        control_plane,
        ..
    } = running;
    match command {
        AgentCommand::Declare {
            name,
            harness,
            model,
        } => {
            let organization = scoping.resolve().await?.organization;
            let mut declaration = wire::AgentDeclaration::new(harness, name);
            declaration.model = Some(model);
            let agent: wire::Agent = api
                .post_as(&["organizations", &organization, "agents"], &declaration)
                .await?;
            typed::show(presentation, &view::AGENT_DECLARED, &agent)?;
        }
        AgentCommand::Model { name, model } => {
            let organization = scoping.resolve().await?.organization;
            let agent: wire::Agent = api
                .put_as(
                    &["organizations", &organization, "agents", &name, "model"],
                    &wire::AgentModel { model: Some(model) },
                )
                .await?;
            typed::show(presentation, &view::AGENT_MODEL, &agent)?;
        }
        AgentCommand::List => {
            let organization = scoping.resolve().await?.organization;
            let agents: wire::ListAgentsResponse = api
                .get_as(&["organizations", &organization, "agents"])
                .await?;
            typed::collection(
                presentation,
                &view::AGENTS,
                &agents,
                &empty_list(
                    control_plane,
                    "Agents",
                    Some(&organization),
                    "agent declare --help",
                ),
            )?;
        }
    }

    Ok(())
}
