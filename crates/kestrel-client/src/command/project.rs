use anyhow::{Result, bail};
use clap::Subcommand;
use kestrel_operator_types as wire;

use crate::command::Running;
use crate::empty_list;
use crate::exit::{Exit, Failed};
use crate::scope::Scoping;
use crate::{typed, view};

#[derive(Debug, Subcommand)]
pub(crate) enum ProjectCommand {
    /// Declare a Project, or make the one by this name what this declaration describes
    Declare {
        /// The name it is referred to by
        name: String,
        /// A repository the work happens against; repeat for many
        #[arg(long = "repository", value_name = "URL", required = true)]
        repositories: Vec<String>,
        /// The Integration a repository reaches GitHub as, checked against GitHub before the
        /// Project is saved; `REPOSITORY=NAME`, or `NAME` alone beside one repository
        #[arg(long = "integration", value_name = "[REPOSITORY=]NAME")]
        integrations: Vec<String>,
        /// The branch the work happens on
        #[arg(long)]
        branch: String,
    },
    /// List every Project in the Organization
    #[command(
        after_help = "Empty lists show scoped guidance at a terminal; non-JSON pipes emit nothing."
    )]
    List,
}

pub(crate) async fn run(
    command: ProjectCommand,
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
        ProjectCommand::Declare {
            name,
            repositories,
            integrations,
            branch,
        } => {
            let organization = scoping.resolve().await?.organization;
            let integrations = selections(&repositories, &integrations)?;
            let declaration = wire::ProjectDeclaration::builder(branch, name, repositories)
                .integrations(integrations)
                .build();
            let project: wire::Project = api
                .post_as(&["organizations", &organization, "projects"], &declaration)
                .await?;
            typed::show(presentation, &view::PROJECT_DECLARED, &project)?;
        }
        ProjectCommand::List => {
            let organization = scoping.resolve().await?.organization;
            let projects: wire::ListProjectsResponse = api
                .get_as(&["organizations", &organization, "projects"])
                .await?;
            typed::collection(
                presentation,
                &view::PROJECTS,
                &projects,
                &empty_list(
                    control_plane_given,
                    "Projects",
                    Some(&organization),
                    "project declare --help",
                ),
            )?;
        }
    }

    Ok(())
}

fn selections(
    repositories: &[String],
    integrations: &[String],
) -> Result<Vec<wire::RepositoryIntegrationSelection>> {
    integrations
        .iter()
        .map(|given| match (given.rsplit_once('='), repositories) {
            (Some((repository, integration)), _) => Ok(wire::RepositoryIntegrationSelection {
                repository: repository.to_owned(),
                integration: integration.to_owned(),
            }),
            (None, [repository]) => Ok(wire::RepositoryIntegrationSelection {
                repository: repository.clone(),
                integration: given.clone(),
            }),
            (None, _) => bail!(Failed::new(
                Exit::Usage,
                format!(
                    "--integration {given} names no repository; beside several, give it as \
                     REPOSITORY={given}"
                )
            )),
        })
        .collect()
}
