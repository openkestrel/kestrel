use std::collections::HashMap;

use anyhow::{Context as _, Result};
use jiff::Timestamp;

use crate::domain::{Organization, Session, SessionId, SessionState};
use crate::instance::{self, Admission};
use crate::store::queue::Recorded;
use crate::store::{Store, Tx};
use crate::work;

/// One coherent read of an Organization's queue: positions, reasons and limits derived at read
/// time from the state one transaction saw, by the rules dispatch applies, never a value a
/// dispatcher happened to store.
pub struct Snapshot {
    /// What the dispatching work role recorded. `None` says no work role is dispatching.
    pub recorded: Option<Recorded>,
    pub active_work: ActiveWork,
    pub instances: Instances,
    pub queued: Vec<Queued>,
    pub waiting: Vec<Waiting>,
    pub unbriefed: Vec<Unbriefed>,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reason {
    Dependencies(Vec<String>),
    SubscriptionProfile {
        profile: String,
        session: String,
    },
    /// An idle Instance being archived, or one dispatch will archive, to make room.
    InstanceArchiving(String),
    LiveInstanceLimit(usize),
    ActiveWorkSlots(usize),
    Ahead(Vec<String>),
}

pub struct Queued {
    pub session: Session,
    /// `None` while any reason holds it, and its enqueue place back once none does.
    pub position: Option<usize>,
    pub reasons: Vec<Reason>,
}

pub struct Waiting {
    pub session: Session,
    /// When the oldest input held for its next turn arrived; `None` while nothing is held.
    pub pending_since: Option<Timestamp>,
    pub reasons: Vec<Reason>,
}

/// Holds an Instance and no Active-Work Slot.
pub struct Unbriefed {
    pub session: Session,
    pub pending_since: Option<Timestamp>,
}

pub async fn snapshot(store: &Store, name: &str) -> Result<Snapshot> {
    let mut tx = store.read().await?;
    let organization = tx.organizations().named(name).await?;
    let recorded = tx.queue().recorded().await?;
    let serialized = recorded
        .as_ref()
        .map_or(&[][..], |recorded| &recorded.serialized_harnesses);

    let mut held: HashMap<SessionId, Vec<Reason>> = HashMap::new();
    for (session, profile, holder) in tx.queue().profiles_held(&organization, serialized).await? {
        held.entry(session)
            .or_default()
            .push(Reason::SubscriptionProfile {
                profile,
                session: holder,
            });
    }

    let active_work = active_work(&mut tx, &organization, recorded.as_ref()).await?;
    let instances = instances(&mut tx, &organization).await?;
    let queued = queued(&mut tx, &organization, &mut held).await?;
    let waiting = waiting(&mut tx, &organization, &active_work, &queued, &mut held).await?;
    let unbriefed = unbriefed(&mut tx, &organization).await?;

    Ok(Snapshot {
        recorded,
        active_work,
        instances,
        queued,
        waiting,
        unbriefed,
    })
}

async fn active_work(
    tx: &mut Tx<'_>,
    organization: &Organization,
    recorded: Option<&Recorded>,
) -> Result<ActiveWork> {
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

async fn queued(
    tx: &mut Tx<'_>,
    organization: &Organization,
    held: &mut HashMap<SessionId, Vec<Reason>>,
) -> Result<Vec<Queued>> {
    let mut blocked: HashMap<SessionId, Vec<String>> = HashMap::new();
    for (queued, blocker) in tx.queue().waiting_on(organization).await? {
        blocked.entry(queued.parse()?).or_default().push(blocker);
    }

    let mut ready = 0;
    let mut queued = Vec::new();
    for session in tx
        .workspaces()
        .sessions_in(organization, SessionState::Queued)
        .await?
    {
        let mut reasons = Vec::new();
        if let Some(blockers) = blocked.remove(&session.id) {
            reasons.push(Reason::Dependencies(blockers));
        }
        let workspace = tx.workspaces().get(session.workspace).await?;
        // Dispatch claims a Session with nothing to start it with without a slot and without the
        // serialized Profile, so neither can be why it is still queued.
        let awaiting_a_brief = work::awaiting_a_brief(tx, &workspace).await?;
        if !awaiting_a_brief {
            reasons.extend(held.remove(&session.id).unwrap_or_default());
        }
        match instance::admission(tx, &workspace).await? {
            Admission::Available => {}
            Admission::Archiving(instance) => reasons.push(Reason::InstanceArchiving(instance)),
            // Dispatch archives only for a Session it could claim.
            Admission::Archivable(kept) if reasons.is_empty() => {
                reasons.push(Reason::InstanceArchiving(kept.instance));
            }
            Admission::Archivable(_) => {
                let limit = workspace.organization.max_live_instances;
                reasons.extend(limit.map(|limit| Reason::LiveInstanceLimit(limit.get())));
            }
            Admission::AtLimit(limit) => reasons.push(Reason::LiveInstanceLimit(limit.get())),
        }

        let position = (!awaiting_a_brief && reasons.is_empty()).then(|| {
            ready += 1;
            ready
        });
        queued.push(Queued {
            session,
            position,
            reasons,
        });
    }

    Ok(queued)
}

/// They hold their Instances, so they are counted there, and no Active-Work Slot, so they are never
/// numbered.
async fn unbriefed(tx: &mut Tx<'_>, organization: &Organization) -> Result<Vec<Unbriefed>> {
    let mut unbriefed = Vec::new();
    for session in tx
        .workspaces()
        .sessions_in(organization, SessionState::Unbriefed)
        .await?
    {
        let pending_since = tx.workspaces().pending_since(session.workspace).await?;
        unbriefed.push(Unbriefed {
            session,
            pending_since,
        });
    }

    Ok(unbriefed)
}

/// Those with held input come first, in the order a freed slot prompts them.
async fn waiting(
    tx: &mut Tx<'_>,
    organization: &Organization,
    active_work: &ActiveWork,
    queued: &[Queued],
    held: &mut HashMap<SessionId, Vec<Reason>>,
) -> Result<Vec<Waiting>> {
    let full = active_work
        .limit
        .filter(|limit| active_work.occupied >= *limit);
    let mut sessions: HashMap<SessionId, Session> = HashMap::new();
    let mut in_enqueue_order = Vec::new();
    for session in tx
        .workspaces()
        .sessions_in(organization, SessionState::Waiting)
        .await?
    {
        in_enqueue_order.push(session.id);
        sessions.insert(session.id, session);
    }

    let mut waiting = Vec::new();
    let mut earlier_input = Vec::new();
    for (id, since) in tx.queue().held_input(organization).await? {
        let session = sessions
            .remove(&id)
            .with_context(|| format!("the waiting session {id} was read twice"))?;
        let mut reasons: Vec<Reason> = full.map(Reason::ActiveWorkSlots).into_iter().collect();
        let profile = held.remove(&id).unwrap_or_default();
        let prompted_in_turn = profile.is_empty();
        reasons.extend(profile);
        let ahead: Vec<String> = queued
            .iter()
            .filter(|queued| {
                queued.position.is_some() && work::goes_before_input(&queued.session, since)
            })
            .map(|queued| queued.session.name.clone())
            .chain(earlier_input.iter().cloned())
            .collect();
        if !ahead.is_empty() {
            reasons.push(Reason::Ahead(ahead));
        }
        // A slot never goes to input a serialized profile holds back, so it is ahead of nothing.
        if prompted_in_turn {
            earlier_input.push(session.name.clone());
        }

        waiting.push(Waiting {
            session,
            pending_since: Some(since),
            reasons,
        });
    }

    for id in in_enqueue_order {
        if let Some(session) = sessions.remove(&id) {
            waiting.push(Waiting {
                session,
                pending_since: None,
                reasons: Vec::new(),
            });
        }
    }

    Ok(waiting)
}
