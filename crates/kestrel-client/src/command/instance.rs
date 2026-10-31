use anyhow::Result;
use clap::Subcommand;
use kestrel_operator_types as wire;

use super::{Running, empty_list};
use crate::{typed, view};

#[derive(Debug, Subcommand)]
pub(crate) enum InstanceCommand {
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

pub(crate) async fn run(running: Running<'_>, command: InstanceCommand) -> Result<()> {
    let Running {
        api,
        presentation,
        scoping,
        control_plane,
        ..
    } = running;
    match command {
        InstanceCommand::List => {
            let organization = scoping.resolve().await?.organization;
            typed::collection(
                presentation,
                &view::INSTANCES,
                &api.get_as::<wire::ListHeldInstancesResponse>(&[
                    "organizations",
                    &organization,
                    "instances",
                ])
                .await?,
                &empty_list(
                    control_plane,
                    "Instances",
                    Some(&organization),
                    "workspace list",
                ),
            )?;
        }
        InstanceCommand::Release {
            workspace,
            as_participant,
        } => {
            let organization = scoping.resolve().await?.organization;
            typed::show(
                presentation,
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
    }

    Ok(())
}
