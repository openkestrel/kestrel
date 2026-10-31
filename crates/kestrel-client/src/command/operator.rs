use anyhow::Result;
use clap::Subcommand;
use kestrel_operator_types as wire;

use super::Running;
use crate::{typed, view};

#[derive(Debug, Subcommand)]
pub(crate) enum OperatorCommand {
    /// Name the Operator, or rename them; their Subscription Profiles stay theirs
    Name {
        /// The name written on what they author from here on
        name: String,
    },
    /// Show the Operator
    Show,
}

pub(crate) async fn run(running: Running<'_>, command: OperatorCommand) -> Result<()> {
    let Running {
        api, presentation, ..
    } = running;
    match command {
        OperatorCommand::Name { name } => {
            let operator: wire::Operator = api
                .put_as(&["operator"], &wire::OperatorDeclaration { name })
                .await?;
            typed::show(presentation, &view::OPERATOR, &operator)?;
        }
        OperatorCommand::Show => {
            let operator: wire::Operator = api.get_as(&["operator"]).await?;
            typed::show(presentation, &view::OPERATOR, &operator)?;
        }
    }

    Ok(())
}
