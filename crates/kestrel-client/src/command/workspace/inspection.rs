use anyhow::Result;
use clap::Subcommand;
use kestrel_operator_types as wire;

use crate::command::{Running, empty_list};
use crate::{changes, files, shell, work};

#[derive(Debug, Subcommand)]
pub(crate) enum InspectionCommand {
    #[command(
        alias = "diff",
        after_help = "Terminal output includes repository headings and empty-list guidance. Redirected output omits headings and guidance; --json returns the complete operator response.",
        about = "Inspect a Workspace's Unpublished Work",
        long_about = "Inspect a Workspace's Unpublished Work across its repositories.

By default, compare the working tree (including untracked files) with origin/<work-branch>. Before the first push, compare with the merge base of HEAD and origin/<checkout-base>. The Project's checkout base is the starting branch; the Workspace's declared work branch holds its work.

Scopes: default Unpublished Work; --changed for unstaged tracked changes; --staged for staged changes; COMMIT (a hexadecimal commit id) for one commit. Scopes are mutually exclusive. Paths after -- are literal <repo>/<path> filters, using checkout directory names from workspace files.

Examples:
  kestrel workspace changes latest
  kestrel workspace changes latest --changed -- repo/src/main.rs
  kestrel workspace changes latest --staged
  kestrel workspace changes latest abcd1234 -- repo/README.md"
    )]
    Changes {
        workspace: String,
        /// Compare staged changes with HEAD
        #[arg(long, conflicts_with_all = ["changed", "commit"])]
        staged: bool,
        /// Compare unstaged tracked changes with the index
        #[arg(long, conflicts_with = "commit")]
        changed: bool,
        /// Show one commit against its parent
        commit: Option<String>,
        /// Literal repository-qualified paths: <repo>/<path>
        #[arg(last = true)]
        paths: Vec<String>,
    },
    #[command(
        alias = "log",
        after_help = "Terminal output includes repository headings and empty-list guidance. Redirected output omits headings and guidance; --json returns the complete operator response.",
        about = "Inspect commits that have not reached a remote",
        long_about = "List commits reachable from any local branch or detached HEAD that no remote-tracking branch reaches, grouped by repository. This covers the Workspace's live checkouts, not just its declared work branch or checkout base. No comparison scope or path filter is supported.

Examples:
  kestrel workspace commits latest
  kestrel workspace changes latest abcd1234 -- repo/README.md

Inspect one commit's patch with changes COMMIT (a hexadecimal commit id); paths there are literal <repo>/<path> filters. The Project's checkout base starts the checkout; the Workspace's work branch holds its work."
    )]
    Commits { workspace: String },
    #[command(
        alias = "stash",
        after_help = "Terminal output includes repository headings and empty-list guidance. Redirected output omits headings and guidance; --json returns the complete operator response.",
        about = "Inspect stashes in a Workspace's live checkouts",
        long_about = "List stashes grouped by repository without diffing them. All repositories in the Workspace's live Instance are included; no comparison scope or path filter is supported. Stashes are separate from the default Unpublished Work comparison in changes. The checkout base starts the checkout; the Workspace's declared work branch holds its work.

Examples:
  kestrel workspace stashes latest
  kestrel workspace changes latest -- repo/README.md

The changes path filter uses literal <repo>/<path>, with checkout directory names from workspace files."
    )]
    Stashes { workspace: String },
    #[command(
        alias = "status",
        after_help = "Terminal output includes repository headings and empty-list guidance. Redirected output omits headings and guidance; --json returns the complete operator response.",
        about = "Inspect the supervisor's latest report of a Workspace's work",
        long_about = "Show the supervisor's latest per-repository work summary and its report time. This is a reported snapshot, not a fresh diff. Changed and staged counts describe the working tree; committed counts cover commits on local branches or detached HEAD that no remote-tracking branch reaches. The default changes comparison is against origin/<work-branch>, or the merge base of HEAD and origin/<checkout-base> before the first push. The checkout base starts the checkout; the Workspace's declared work branch holds its work.

No comparison scope or path filter is supported here. Use changes for default Unpublished Work, --changed, --staged or one COMMIT, and literal <repo>/<path> filters.

Examples:
  kestrel workspace work latest
  kestrel workspace changes latest --staged -- repo/src/main.rs
  kestrel workspace files latest"
    )]
    Work { workspace: String },
    /// List one directory of a Workspace's live Instance, each entry marked tracked, untracked
    /// or ignored; with no path, its repositories
    #[command(
        alias = "ls",
        after_help = "An empty directory shows scoped inspection guidance at a terminal; non-JSON pipes emit nothing. Redirected listings omit directory headings; --json returns the complete operator response."
    )]
    Files {
        workspace: String,
        /// `<repo>/<path>`
        path: Option<String>,
    },
    /// Write a file in a Workspace's live Instance to standard output, byte for byte
    #[command(alias = "cat")]
    Read {
        workspace: String,
        /// `<repo>/<path>`
        path: String,
    },
}

pub(super) async fn run(running: Running<'_>, command: InspectionCommand) -> Result<()> {
    let Running {
        api,
        presentation,
        scoping,
        control_plane,
        ..
    } = running;
    match command {
        InspectionCommand::Work { workspace } => {
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
        InspectionCommand::Changes {
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
        command @ (InspectionCommand::Commits { .. } | InspectionCommand::Stashes { .. }) => {
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
                InspectionCommand::Commits { workspace } => {
                    let commits = api.get_as(&path(&workspace, "commits")).await?;
                    (workspace, "commits", changes::Read::Commits(commits))
                }
                InspectionCommand::Stashes { workspace } => {
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
        InspectionCommand::Files { workspace, path } => {
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
        InspectionCommand::Read { workspace, path } => {
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
    }

    Ok(())
}
