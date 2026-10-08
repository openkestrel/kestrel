use std::num::NonZeroUsize;

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

use crate::compute::IdleHint;
use crate::declined::{Next, Reason, Resource, Step};
use crate::domain::{Workspace, WorkspaceId, WorkspaceState};
use crate::log::Entry;
use crate::store::workspace::Kept;
use crate::store::{Store, Tx};
use crate::workspace;

pub(crate) async fn queue_idle_hint(tx: &mut Tx<'_>, id: WorkspaceId) -> Result<()> {
    let workspace = tx.workspaces().get(id).await?;
    if let Some(kept) = idle_recoverable(tx, &workspace).await? {
        tx.workspaces()
            .queue_idle_hint(
                &workspace,
                &IdleHint {
                    instance: kept.instance,
                    idle_since: workspace.last_active_at,
                    archive_deadline: workspace.last_active_at + workspace::IDLE,
                },
            )
            .await?;
    }
    Ok(())
}

async fn idle_recoverable(tx: &mut Tx<'_>, workspace: &Workspace) -> Result<Option<Kept>> {
    if workspace.state != WorkspaceState::Open
        || tx
            .workspaces()
            .unfinished_session(workspace)
            .await?
            .is_some()
    {
        return Ok(None);
    }
    Ok(tx
        .workspaces()
        .kept_instance(workspace.id)
        .await?
        .filter(|kept| {
            unpublished(&workspace.checkout.repositories, kept.observed.as_deref()).is_none()
        }))
}

pub(crate) async fn deliver_idle_hints(
    store: &Store,
    mut deliver: impl FnMut(&IdleHint) -> std::io::Result<()>,
) -> Result<()> {
    let mut tx = store.begin().await?;
    for (id, hint) in tx.workspaces().take_idle_hints().await? {
        let workspace = tx.workspaces().get(id).await?;
        if workspace.last_active_at != hint.idle_since {
            continue;
        }
        let Some(kept) = idle_recoverable(&mut tx, &workspace).await? else {
            continue;
        };
        if kept.instance != hint.instance {
            continue;
        }
        // The ending is committed; keep the write lock so no Session claims this Instance mid-hint.
        if let Err(error) = deliver(&hint) {
            tracing::warn!(instance = %hint.instance, %error, "an instance's idle hint could not be delivered");
        }
    }
    tx.commit().await
}

pub enum Admission {
    Available,
    Archiving(String),
    Archivable(Kept),
    AtLimit(NonZeroUsize),
}

/// Judges without acting, so the queue can ask what dispatch would find.
pub async fn admission(tx: &mut Tx<'_>, workspace: &Workspace) -> Result<Admission> {
    if tx.workspaces().instance(workspace.id).await?.is_some() {
        return Ok(Admission::Available);
    }
    let Some(limit) = workspace.organization.max_live_instances else {
        return Ok(Admission::Available);
    };
    if tx
        .workspaces()
        .live_instance_count(&workspace.organization)
        .await?
        < limit.get()
    {
        return Ok(Admission::Available);
    }
    if let Some(instance) = tx
        .workspaces()
        .instance_being_archived(&workspace.organization)
        .await?
    {
        return Ok(Admission::Archiving(instance));
    }

    for kept in tx
        .workspaces()
        .kept_instances(&workspace.organization)
        .await?
    {
        let candidate = tx.workspaces().get(kept.workspace).await?;
        if workspace::unfinished_session(tx, &candidate).await?.idle()
            && unpublished(&candidate.checkout.repositories, kept.observed.as_deref()).is_none()
        {
            return Ok(Admission::Archivable(kept));
        }
    }

    Ok(Admission::AtLimit(limit))
}

pub async fn reclaim(tx: &mut Tx<'_>, kept: &Kept) -> Result<()> {
    let workspace = tx.workspaces().get(kept.workspace).await?;
    archive(tx, &workspace, &kept.instance).await
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Observed {
    pub repository: String,
    #[serde(flatten)]
    pub git: Git,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "git", rename_all = "snake_case")]
pub enum Git {
    Read {
        branch: Option<String>,
        untracked: u64,
        uncommitted: u64,
        stashes: u64,
        unpushed: u64,
    },
    Unreadable {
        because: String,
    },
}

/// A checkout nobody reported on is judged to hold work, since nothing says it does not.
pub fn unpublished(repositories: &[String], observed: Option<&[Observed]>) -> Option<String> {
    let Some(observed) = observed else {
        return Some("no session reported what its checkout holds".to_owned());
    };

    let mut held: Vec<String> = repositories
        .iter()
        .filter(|repository| {
            !observed
                .iter()
                .any(|item| item.repository == repository.as_str())
        })
        .map(|repository| format!("{repository} was not reported"))
        .collect();
    held.extend(observed.iter().filter_map(held_in));
    (!held.is_empty()).then(|| held.join("; "))
}

fn held_in(observed: &Observed) -> Option<String> {
    let repository = &observed.repository;
    let (branch, counted) = match &observed.git {
        Git::Unreadable { because } => {
            return Some(format!("{repository} could not be read: {because}"));
        }
        Git::Read {
            branch,
            untracked,
            uncommitted,
            stashes,
            unpushed,
        } => (
            branch,
            [
                (*unpushed, "unpushed commit"),
                (*uncommitted, "uncommitted change"),
                (*untracked, "untracked file"),
                (*stashes, "stash"),
            ],
        ),
    };

    let held: Vec<String> = counted
        .into_iter()
        .filter(|(count, _)| *count > 0)
        .map(|(count, what)| match count {
            1 => format!("1 {what}"),
            _ if what.ends_with('h') => format!("{count} {what}es"),
            _ => format!("{count} {what}s"),
        })
        .collect();
    if held.is_empty() {
        return None;
    }

    Some(match branch {
        Some(branch) => format!("{repository} on {branch} has {}", held.join(", ")),
        None => format!("{repository} has {}", held.join(", ")),
    })
}

#[derive(Debug, Clone)]
pub struct Held {
    pub workspace: WorkspaceId,
    pub instance: String,
    pub because: String,
}

pub async fn held(store: &Store, organization: &str) -> Result<Vec<Held>> {
    let mut tx = store.begin().await?;
    let organization = tx.organizations().named(organization).await?;
    let mut held = Vec::new();

    for kept in tx.workspaces().kept_instances(&organization).await? {
        if let Some(judged) = judged(&mut tx, kept).await? {
            held.push(judged);
        }
    }

    Ok(held)
}

pub async fn held_by(store: &Store, workspace: WorkspaceId) -> Result<Option<Held>> {
    let mut tx = store.begin().await?;
    let Some(kept) = tx.workspaces().kept_instance(workspace).await? else {
        return Ok(None);
    };

    judged(&mut tx, kept).await
}

/// An Instance a session is using or about to use is not held: what it holds is not yet known.
async fn judged(tx: &mut Tx<'_>, kept: Kept) -> Result<Option<Held>> {
    let workspace = tx.workspaces().get(kept.workspace).await?;
    if workspace::unfinished_session(tx, &workspace)
        .await?
        .in_flight()
        .is_some()
    {
        return Ok(None);
    }

    Ok(
        unpublished(&workspace.checkout.repositories, kept.observed.as_deref()).map(|because| {
            Held {
                workspace: kept.workspace,
                instance: kept.instance,
                because,
            }
        }),
    )
}

/// The one way work that may exist nowhere else is ever discarded, so only a person calls it.
pub async fn release(store: &Store, id: WorkspaceId, participant: &str) -> Result<String> {
    let mut tx = store.begin().await?;
    let workspace = tx.workspaces().get(id).await?;
    workspace.accepts("release_instance", "release")?;

    let Some(kept) = tx.workspaces().kept_instance(id).await? else {
        bail!(workspace::workspace_conflict(
            "release_instance",
            &workspace,
            "no_instance",
            format!("the workspace {id} has no instance to release"),
        ));
    };
    if let Some(holding) = workspace::unfinished_session(&mut tx, &workspace)
        .await?
        .in_flight()
    {
        bail!(workspace::held_by(
            "release_instance",
            "session_in_flight",
            &workspace,
            holding,
            format!(
                "the session {} is still in flight on the instance {}",
                holding.id, kept.instance
            ),
        ));
    }

    tx.log()
        .append(
            &workspace,
            Entry::InstanceReleased {
                participant: participant.to_owned(),
                instance: kept.instance.clone(),
                unpublished: unpublished(
                    &workspace.checkout.repositories,
                    kept.observed.as_deref(),
                ),
            },
        )
        .await?;
    archive(&mut tx, &workspace, &kept.instance).await?;
    tx.commit().await?;

    Ok(kept.instance)
}

pub(crate) async fn archive_on_seal(tx: &mut Tx<'_>, workspace: &Workspace) -> Result<()> {
    let Some(kept) = tx.workspaces().kept_instance(workspace.id).await? else {
        return workspace::unfinished_session(tx, workspace)
            .await?
            .end_waiting(tx)
            .await;
    };

    if let Some(because) = unpublished(&workspace.checkout.repositories, kept.observed.as_deref()) {
        let reference = workspace.id.to_string();
        bail!(Reason::StateConflict {
            operation: "seal_workspace",
            resource: Resource::Workspace,
            reference: reference.clone(),
            organization: Some(workspace.organization.name.clone()),
            state: "unpublished_work",
            holding_session: None,
            next: Next::inspect(Resource::Workspace, &reference)
                .then(Step::EnqueueSession {
                    workspace: reference.clone(),
                })
                .then(Step::ReleaseInstance {
                    workspace: reference,
                    instance: kept.instance.clone(),
                }),
            message: format!(
                "the workspace {}'s instance {} may hold the only copy of its work ({because}); publish \
                 it from a follow-up session, or release the instance to discard it",
                workspace.id, kept.instance
            ),
        });
    }

    archive(tx, workspace, &kept.instance).await
}

/// Destroying an Instance under a waiting Session would record it failed, though it has done
/// everything asked of it.
async fn archive(tx: &mut Tx<'_>, workspace: &Workspace, instance: &str) -> Result<()> {
    workspace::unfinished_session(tx, workspace)
        .await?
        .end_waiting(tx)
        .await?;
    tx.workspaces().archive_instance(workspace, instance).await
}

pub async fn to_archive(store: &Store) -> Result<Vec<String>> {
    store
        .begin()
        .await?
        .workspaces()
        .instances_to_archive()
        .await
}

pub async fn archived(store: &Store, instance: &str) -> Result<()> {
    let mut tx = store.begin().await?;
    tx.workspaces().instance_archived(instance).await?;

    tx.commit().await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(untracked: u64, uncommitted: u64, stashes: u64, unpushed: u64) -> Observed {
        Observed {
            repository: "https://github.com/acme/widgets".to_owned(),
            git: Git::Read {
                branch: Some("kestrel/work".to_owned()),
                untracked,
                uncommitted,
                stashes,
                unpushed,
            },
        }
    }

    #[test]
    fn a_checkout_nobody_reported_on_is_held() {
        assert!(unpublished(&[], None).is_some());
    }

    #[test]
    fn a_clean_pushed_checkout_holds_nothing() {
        assert_eq!(
            unpublished(
                &["https://github.com/acme/widgets".to_owned()],
                Some(&[read(0, 0, 0, 0)])
            ),
            None
        );
    }

    #[test]
    fn a_project_with_no_repositories_holds_nothing() {
        assert_eq!(unpublished(&[], Some(&[])), None);
    }

    #[test]
    fn a_repository_missing_from_a_report_is_held() {
        assert_eq!(
            unpublished(&["https://github.com/acme/widgets".to_owned()], Some(&[])).as_deref(),
            Some("https://github.com/acme/widgets was not reported")
        );
    }

    #[test]
    fn each_kind_of_local_work_is_held_on_its_own() {
        for observed in [
            read(1, 0, 0, 0),
            read(0, 1, 0, 0),
            read(0, 0, 1, 0),
            read(0, 0, 0, 1),
        ] {
            assert!(
                unpublished(
                    &["https://github.com/acme/widgets".to_owned()],
                    Some(std::slice::from_ref(&observed))
                )
                .is_some(),
                "{observed:?} was judged to hold nothing"
            );
        }
    }

    #[test]
    fn the_reason_names_the_repository_its_branch_and_what_it_holds() {
        assert_eq!(
            unpublished(
                &["https://github.com/acme/widgets".to_owned()],
                Some(&[read(1, 0, 2, 3)])
            )
            .as_deref(),
            Some(
                "https://github.com/acme/widgets on kestrel/work has 3 unpushed commits, \
                 1 untracked file, 2 stashes"
            )
        );
    }

    #[test]
    fn an_unreadable_repository_is_held_even_beside_a_clean_one() {
        let unreadable = Observed {
            repository: "https://github.com/acme/gadgets".to_owned(),
            git: Git::Unreadable {
                because: "there is no checkout at gadgets".to_owned(),
            },
        };

        assert_eq!(
            unpublished(
                &[
                    "https://github.com/acme/widgets".to_owned(),
                    "https://github.com/acme/gadgets".to_owned(),
                ],
                Some(&[read(0, 0, 0, 0), unreadable])
            )
            .as_deref(),
            Some(
                "https://github.com/acme/gadgets could not be read: there is no checkout at gadgets"
            )
        );
    }

    #[test]
    fn a_report_reads_as_the_supervisor_sends_it() {
        let sent = serde_json::json!({
            "repository": "https://github.com/acme/widgets",
            "git": "read",
            "branch": null,
            "untracked": 0,
            "uncommitted": 1,
            "stashes": 0,
            "unpushed": 0,
        });

        let observed: Observed = serde_json::from_value(sent).expect("an observation");

        assert_eq!(
            observed.git,
            Git::Read {
                branch: None,
                untracked: 0,
                uncommitted: 1,
                stashes: 0,
                unpushed: 0,
            }
        );
    }
}
