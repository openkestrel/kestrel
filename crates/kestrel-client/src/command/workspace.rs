use anyhow::Result;
use clap::Subcommand;
use kestrel_operator_types as wire;
use serde_json::json;

use crate::command::Running;
use crate::scope::Scoping;
use crate::{changes, files, shell, transcript, typed, view, work};
use crate::{empty_list, given};

#[derive(Debug, Subcommand)]
pub(crate) enum WorkspaceCommand {
    /// Open a Workspace against a Project and an Agent, and enqueue its first Session
    Open {
        /// The Project its work happens against
        #[arg(long)]
        project: String,
        /// The Agent that participates in it
        #[arg(long)]
        agent: String,
        /// The Subscription Profile its Sessions use
        #[arg(long)]
        profile: Option<String>,
        /// The Sign-in Method its first Session is handed, without its harness's others
        #[arg(long = "sign-in", value_name = "METHOD")]
        sign_in: Option<String>,
        /// The branch its work happens on
        #[arg(long)]
        branch: Option<String>,
        /// The sealed Workspace this one carries on from, by generated name, identifier, any
        /// unambiguous prefix of its identifier, or `latest`
        #[arg(long, value_name = "WORKSPACE")]
        continues: Option<String>,
        /// The Brief the Workspace starts with; `@FILE` reads it from a file and `-` from
        /// standard input
        #[arg(long)]
        brief: Option<String>,
        /// The model the first Session runs on. Without it, the Agent's
        #[arg(long)]
        model: Option<String>,
        /// The mode the first Session runs in. Without it, the Agent's
        #[arg(long)]
        mode: Option<String>,
        /// The thought level the first Session runs at. Without it, the Agent's
        #[arg(long)]
        thought_level: Option<String>,
        /// The name the Brief is written under. Without it, the Operator's
        #[arg(long)]
        as_participant: Option<String>,
        /// A Session it waits on until that one ends successfully, by generated name,
        /// identifier, any unambiguous prefix of its identifier, or `latest`; repeatable
        #[arg(long, value_name = "SESSION")]
        depends_on: Vec<String>,
    },
    /// List Workspaces; an empty terminal lists the scoped next step, while a pipe emits nothing
    #[command(
        after_help = "Empty lists show scoped guidance at a terminal; non-JSON pipes emit nothing."
    )]
    List,
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
    /// Show a Workspace
    Show {
        /// Its generated name, its identifier, any unambiguous prefix of its identifier, or
        /// `latest`
        workspace: String,
    },
    /// Add a participant's message; starts a Session or queues its next Turn
    Post {
        /// Its generated name, its identifier, any unambiguous prefix of its identifier, or
        /// `latest`
        workspace: String,
        /// The participant saying the message. Without it, the Operator
        #[arg(long)]
        as_participant: Option<String>,
        /// What the participant says
        message: String,
    },
    /// Change a Held Message before a Turn takes it
    #[command(subcommand)]
    Message(MessageCommand),
    /// Seal a Workspace: readable ever after, and never reopened
    Seal {
        /// Its generated name, its identifier, any unambiguous prefix of its identifier, or
        /// `latest`
        workspace: String,
    },
    /// Read a Workspace's Transcript, one entry a line, and the cursor a later read
    /// resumes from
    Transcript {
        /// Its generated name, its identifier, any unambiguous prefix of its identifier, or
        /// `latest`
        workspace: String,
        /// Resume after the cursor a previous read ended with
        #[arg(long)]
        cursor: Option<String>,
        /// Keep reading as entries are appended, until the Workspace is sealed
        #[arg(long)]
        follow: bool,
        /// Join the Workspace's presence as this person while following
        #[arg(long, requires = "follow")]
        as_participant: Option<String>,
        #[arg(long, default_value = "shared_state")]
        kinds: String,
        #[arg(long)]
        no_summaries: bool,
    },
}

#[derive(Debug, Subcommand)]
pub(crate) enum MessageCommand {
    /// Replace a Held Message's text; only its author's name is accepted
    Edit {
        /// Its generated name, its identifier, any unambiguous prefix of its identifier, or
        /// `latest`
        workspace: String,
        /// The id the post answered with
        id: i64,
        /// The participant that wrote the message, and only that one. Without it, the Operator
        #[arg(long)]
        as_participant: Option<String>,
        /// What the message now says
        message: String,
    },
    /// Take a Held Message back, so no Turn sees it; only its author's name is accepted
    Withdraw {
        /// Its generated name, its identifier, any unambiguous prefix of its identifier, or
        /// `latest`
        workspace: String,
        /// The id the post answered with
        id: i64,
        /// The participant that wrote the message, and only that one. Without it, the Operator
        #[arg(long)]
        as_participant: Option<String>,
    },
}

pub(crate) async fn run(
    command: WorkspaceCommand,
    scoping: Scoping<'_>,
    running: Running<'_>,
) -> Result<()> {
    let Running {
        api,
        presentation,
        json,
        control_plane,
        control_plane_given,
        ..
    } = running;
    match command {
        WorkspaceCommand::Open {
            project,
            agent,
            profile,
            sign_in,
            branch,
            continues,
            brief,
            model,
            mode,
            thought_level,
            as_participant,
            depends_on,
        } => {
            let organization = scoping.resolve().await?.organization;
            let brief = brief.as_deref().map(given).transpose()?;
            let opened: wire::Opened = api
                .post_as(
                    &["organizations", &organization, "workspaces"],
                    &wire::WorkspaceDeclaration {
                        project,
                        agent,
                        profile: Some(profile),
                        sign_in_method: Some(sign_in),
                        branch: Some(branch),
                        continues: Some(continues),
                        model: Some(model),
                        mode: Some(mode),
                        thought_level: Some(thought_level),
                        brief: Some(brief),
                        participant: Some(as_participant),
                        depends_on: Some(Some(depends_on)),
                    },
                )
                .await?;
            typed::show(presentation, &view::OPENED, &opened)?;
        }
        WorkspaceCommand::List => {
            let organization = scoping.resolve().await?.organization;
            typed::collection(
                presentation,
                &view::WORKSPACES,
                &api.get_as::<wire::ListWorkspacesResponse>(&[
                    "organizations",
                    &organization,
                    "workspaces",
                ])
                .await?,
                &empty_list(
                    control_plane_given,
                    "Workspaces",
                    Some(&organization),
                    "workspace open --help",
                ),
            )?;
        }
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
                    control_plane_given,
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
                    control_plane_given,
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
                    control_plane_given,
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
        WorkspaceCommand::Show { workspace } => {
            let organization = scoping.resolve().await?.organization;
            typed::show(
                presentation,
                &view::WORKSPACE,
                &api.get_as::<wire::Workspace>(&[
                    "organizations",
                    &organization,
                    "workspaces",
                    &workspace,
                ])
                .await?,
            )?;
        }
        WorkspaceCommand::Post {
            workspace,
            as_participant,
            message,
        } => {
            let organization = scoping.resolve().await?.organization;
            let answer: wire::Posted = api
                .post_as(
                    &[
                        "organizations",
                        &organization,
                        "workspaces",
                        &workspace,
                        "messages",
                    ],
                    &wire::WorkspaceMessage {
                        participant: as_participant,
                        message,
                    },
                )
                .await?;
            if json {
                typed::show(presentation, &view::POSTED, &answer)?;
            } else if let Some(session) = &answer.session {
                typed::show(presentation, &view::SESSION_DECLARED, session)?;
            } else if let Some(held) = &answer.held_message {
                typed::show(presentation, &view::HELD_DECLARED, held)?;
            } else {
                eprintln!("queued as the next turn of the session already in flight");
            }
        }
        WorkspaceCommand::Message(MessageCommand::Edit {
            workspace,
            id,
            as_participant,
            message,
        }) => {
            let organization = scoping.resolve().await?.organization;
            let answer: wire::HeldMessage = api
                .put_as(
                    &[
                        "organizations",
                        &organization,
                        "workspaces",
                        &workspace,
                        "messages",
                        &id.to_string(),
                    ],
                    &wire::WorkspaceMessage {
                        participant: as_participant,
                        message,
                    },
                )
                .await?;
            typed::show(presentation, &view::HELD_MESSAGE, &answer)?;
        }
        WorkspaceCommand::Message(MessageCommand::Withdraw {
            workspace,
            id,
            as_participant,
        }) => {
            let organization = scoping.resolve().await?.organization;
            api.delete(
                &[
                    "organizations",
                    &organization,
                    "workspaces",
                    &workspace,
                    "messages",
                    &id.to_string(),
                ],
                &wire::WorkspaceMessageWithdrawal {
                    participant: as_participant,
                },
            )
            .await?;
        }
        WorkspaceCommand::Seal { workspace } => {
            let organization = scoping.resolve().await?.organization;
            typed::show(
                presentation,
                &view::WORKSPACE_DECLARED,
                &api.post_as::<wire::Workspace>(
                    &[
                        "organizations",
                        &organization,
                        "workspaces",
                        &workspace,
                        "seal",
                    ],
                    &json!({}),
                )
                .await?,
            )?;
        }
        WorkspaceCommand::Transcript {
            workspace,
            cursor,
            follow,
            as_participant,
            kinds,
            no_summaries,
        } => {
            let organization = scoping.resolve().await?.organization;
            let read = transcript::read(
                control_plane,
                &organization,
                &workspace,
                cursor,
                transcript::Selection {
                    follow,
                    kinds: &kinds,
                    summaries: !no_summaries,
                    as_participant: as_participant.as_deref(),
                },
                presentation,
            )
            .await?;
            // Beside the Transcript rather than in it, so stdout carries entries and nothing else.
            if let Some(cursor) = read {
                eprintln!("cursor  {cursor}");
            }
        }
    }

    Ok(())
}
