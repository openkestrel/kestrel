use std::collections::HashMap;

use anyhow::Result;

use crate::domain::{Organization, Session, SessionId};
use crate::store::{Store, Tx};

/// One coherent read of an Organization's queue: positions, names and limits derived at read
/// time from the state one transaction saw, never a value a dispatcher happened to store.
pub struct Snapshot {
    /// What the dispatching work role recorded. `None` says no work role is dispatching.
    pub recorded: Option<crate::store::queue::Recorded>,
    pub active_work: ActiveWork,
    pub instances: Instances,
    pub queued: Vec<Queued>,
}

pub struct ActiveWork {
    /// The Active-Work Slot limit the recorded dispatch carries. `None` while no work role is
    /// dispatching: nothing enforces one.
    pub limit: Option<usize>,
    /// Every Working Session, whatever Organization holds it: one slot is a unit of the
    /// control plane's pool, shared by every Organization.
    pub occupied: usize,
    /// The slots this Organization names its occupants of.
    pub occupants: Vec<String>,
    /// Occupants of other Organizations, counted and never named.
    pub elsewhere: usize,
}

pub struct Instances {
    /// The Organization's live Instance limit, or `None` when unbounded.
    pub limit: Option<usize>,
    /// What counts against it, idle Instances retained for Unpublished Work included.
    pub count: usize,
    /// The counted Instances that can be named.
    pub counted: Vec<String>,
}

pub struct Queued {
    pub session: Session,
    /// The Session's place among the ones dispatch will consider, numbered from 1 in enqueue
    /// order. `None` while it is blocked on dependencies; it takes its place back once every
    /// blocker has ended successfully.
    pub position: Option<usize>,
    /// The Sessions it waits on, when a dependency blocks it.
    pub waits_on: Vec<String>,
}

pub async fn snapshot(store: &Store, name: &str) -> Result<Snapshot> {
    let mut tx = store.read().await?;
    let organization = tx.organizations().named(name).await?;

    Ok(Snapshot {
        recorded: tx.queue().recorded().await?,
        active_work: active_work(&mut tx, &organization).await?,
        instances: instances(&mut tx, &organization).await?,
        queued: queued(&mut tx, &organization).await?,
    })
}

async fn active_work(tx: &mut Tx<'_>, organization: &Organization) -> Result<ActiveWork> {
    let recorded = tx.queue().recorded().await?;
    let mut occupants = Vec::new();
    let mut elsewhere = 0;
    for (name, ours) in tx.queue().occupying(organization).await? {
        if ours {
            occupants.push(name);
        } else {
            elsewhere += 1;
        }
    }

    Ok(ActiveWork {
        limit: recorded.map(|recorded| recorded.active_work_slots),
        occupied: occupants.len() + elsewhere,
        occupants,
        elsewhere,
    })
}

async fn instances(tx: &mut Tx<'_>, organization: &Organization) -> Result<Instances> {
    let mut counted: Vec<String> = tx
        .workspaces()
        .kept_instances(organization)
        .await?
        .into_iter()
        .map(|kept| kept.instance)
        .collect();
    counted.extend(tx.queue().instances_leaving(organization).await?);
    counted.extend(tx.queue().instances_in_flight(organization).await?);

    Ok(Instances {
        limit: organization.max_live_instances.map(|limit| limit.get()),
        count: tx.workspaces().live_instance_count(organization).await?,
        counted,
    })
}

async fn queued(tx: &mut Tx<'_>, organization: &Organization) -> Result<Vec<Queued>> {
    let mut blocked: HashMap<SessionId, Vec<String>> = HashMap::new();
    for (queued, blocker) in tx.queue().waiting_on(organization).await? {
        blocked.entry(queued.parse()?).or_default().push(blocker);
    }

    let mut ready = 1;
    let mut queued = Vec::new();
    for session in tx.workspaces().queued_sessions(organization).await? {
        let waits_on = blocked.remove(&session.id).unwrap_or_default();
        let position = match waits_on.is_empty() {
            true => {
                let numbered = ready;
                ready += 1;
                Some(numbered)
            }
            false => None,
        };
        queued.push(Queued {
            session,
            position,
            waits_on,
        });
    }

    Ok(queued)
}
