use anyhow::Result;
use clap::Subcommand;
use kestrel_operator_types as wire;

use super::{Running, empty_list};
use crate::{typed, view};

#[derive(Debug, Subcommand)]
pub(crate) enum OrganizationCommand {
    /// Declare an Organization, or change its live Instance limit
    Declare {
        /// The name it is referred to by
        name: String,
        /// The most Instances the Organization may keep live at once
        #[arg(long)]
        max_live_instances: Option<std::num::NonZeroUsize>,
    },
    /// List every Organization
    #[command(
        after_help = "Empty lists show scoped guidance at a terminal; non-JSON pipes emit nothing."
    )]
    List,
}

pub(crate) async fn run(running: Running<'_>, command: OrganizationCommand) -> Result<()> {
    let Running {
        api,
        presentation,
        control_plane,
        ..
    } = running;
    match command {
        OrganizationCommand::Declare {
            name,
            max_live_instances,
        } => {
            let declaration = wire::OrganizationDeclaration {
                name,
                max_live_instances: Some(
                    max_live_instances.map(|most| i64::try_from(most.get()).unwrap_or(i64::MAX)),
                ),
            };
            let organization: wire::Organization =
                api.post_as(&["organizations"], &declaration).await?;
            typed::show(presentation, &view::ORGANIZATION_DECLARED, &organization)?;
        }
        OrganizationCommand::List => {
            let organizations: wire::ListOrganizationsResponse =
                api.get_as(&["organizations"]).await?;
            typed::collection(
                presentation,
                &view::ORGANIZATIONS,
                &organizations,
                &empty_list(
                    control_plane,
                    "Organizations",
                    None,
                    "organization declare --help",
                ),
            )?;
        }
    }

    Ok(())
}
