use anyhow::Result;
use clap::Subcommand;
use kestrel_operator_types as wire;

use super::{Running, empty_list};
use crate::{typed, view};

#[derive(Debug, Subcommand)]
pub(crate) enum EventCommand {
    /// List the Events recorded for the Organization, most recent first
    #[command(
        after_help = "Empty lists show scoped guidance at a terminal; non-JSON pipes emit nothing."
    )]
    List {
        /// How many to list at most
        #[arg(long, default_value_t = 50)]
        limit: usize,
    },
    /// Show one Event's whole envelope and payload
    Show {
        /// The Event's record identifier
        record: String,
    },
}

pub(crate) async fn run(running: Running<'_>, command: EventCommand) -> Result<()> {
    let Running {
        api,
        presentation,
        scoping,
        control_plane,
        ..
    } = running;
    match command {
        EventCommand::List { limit } => {
            let organization = scoping.resolve().await?.organization;
            let events: wire::ListEventsResponse = api
                .get_where_as(
                    &["organizations", &organization, "events"],
                    &[("limit", &limit.to_string())],
                )
                .await?;
            typed::collection(
                presentation,
                &view::EVENTS,
                &events,
                &empty_list(
                    control_plane,
                    "Events",
                    Some(&organization),
                    "integration list",
                ),
            )?;
        }
        EventCommand::Show { record } => {
            let event: wire::EventRecord = api.get_as(&["events", &record]).await?;
            typed::show(presentation, &view::EVENT, &event)?;
        }
    }

    Ok(())
}
