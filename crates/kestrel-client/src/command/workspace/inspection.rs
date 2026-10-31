use anyhow::Result;
use kestrel_operator_types as wire;

use super::WorkspaceCommand;
use crate::command::{Running, empty_list};
use crate::{changes, files, shell, work};

/// The reads of what a Workspace's live Instance holds; every other command is the parent's.
pub(super) async fn run(running: Running<'_>, command: WorkspaceCommand) -> Result<()> {
    let Running {
        api,
        presentation,
        scoping,
        control_plane,
        ..
    } = running;
    match command {
        WorkspaceCommand::Work { workspace } => {
            let organization = scoping.resolve().await?.organization;
            let answer = api
                .get_as::<wire::WorkspaceWork>(&[
                    "organizations",
                    &organization,
                    "workspaces",
                    &workspace,
                    "work",
                ])
                .await?;
            work::show(presentation, &answer)?;
        }
        WorkspaceCommand::Changes {
            workspace,
            staged,
            changed,
            commit,
            paths,
        } => {
            let organization = scoping.resolve().await?.organization;
            let scope = if staged {
                "staged".to_owned()
            } else if changed {
                "changed".to_owned()
            } else if let Some(commit) = commit {
                format!("commit:{commit}")
            } else {
                "unpublished".to_owned()
            };
            let mut query = vec![("scope", scope.as_str())];
            query.extend(paths.iter().map(|path| ("path", path.as_str())));
            let answer = api
                .get_where_as::<wire::WorkspaceChanges>(
                    &[
                        "organizations",
                        &organization,
                        "workspaces",
                        &workspace,
                        "changes",
                    ],
                    &query,
                )
                .await?;
            changes::show(
                presentation,
                &changes::Read::Changes(answer),
                &empty_list(
                    control_plane,
                    "changes in this comparison",
                    Some(&organization),
                    &format!("workspace work {}", shell::quoted(&workspace)),
                ),
            )?;
        }
        command @ (WorkspaceCommand::Commits { .. } | WorkspaceCommand::Stashes { .. }) => {
            let organization = scoping.resolve().await?.organization;
            let path = |workspace, read| {
                [
                    "organizations",
                    organization.as_str(),
                    "workspaces",
                    workspace,
                    read,
                ]
            };
            let (workspace, read, answer) = match command {
                WorkspaceCommand::Commits { workspace } => {
                    let commits = api.get_as(&path(&workspace, "commits")).await?;
                    (workspace, "commits", changes::Read::Commits(commits))
                }
                WorkspaceCommand::Stashes { workspace } => {
                    let stashes = api.get_as(&path(&workspace, "stashes")).await?;
                    (workspace, "stashes", changes::Read::Stashes(stashes))
                }
                _ => unreachable!(),
            };
            changes::show(
                presentation,
                &answer,
                &empty_list(
                    control_plane,
                    read,
                    Some(&organization),
                    &format!("workspace work {}", shell::quoted(&workspace)),
                ),
            )?;
        }
        WorkspaceCommand::Files { workspace, path } => {
            let organization = scoping.resolve().await?.organization;
            let path = path.unwrap_or_default();
            let answer = api
                .get_where_as::<wire::FileListing>(
                    &[
                        "organizations",
                        &organization,
                        "workspaces",
                        &workspace,
                        "files",
                    ],
                    &[("path", &path)],
                )
                .await?;
            files::list(
                presentation,
                &answer,
                &empty_list(
                    control_plane,
                    "directory entries",
                    Some(&organization),
                    &format!("workspace files {}", shell::quoted(&workspace)),
                ),
            )?;
        }
        WorkspaceCommand::Read { workspace, path } => {
            let organization = scoping.resolve().await?.organization;
            let response = api
                .get_response(
                    &[
                        "organizations",
                        &organization,
                        "workspaces",
                        &workspace,
                        "file",
                    ],
                    &[("path", &path)],
                )
                .await?;
            files::read(response, presentation).await?;
        }
        _ => unreachable!("the parent handles every command that reads no checkout"),
    }

    Ok(())
}
