use std::collections::{HashMap, HashSet};

use anyhow::Result;
use jiff::Timestamp;

use crate::domain::{Organization, Preparing, Session, SessionId, SessionState, Workspace};
use crate::instance::{self, Admission};
use crate::link;
use crate::store::queue::Recorded;
use crate::store::{Store, Tx};
use crate::work;

pub struct Snapshot {
    pub recorded: Option<Recorded>,
    pub active_work: ActiveWork,
    pub instances: Instances,
    pub queued: Vec<Queued>,
    pub waiting: Vec<Waiting>,
    pub unbriefed: Vec<Unbriefed>,
}

pub struct ActiveWork {
    pub limit: Option<usize>,
    pub occupied: usize,
    pub occupants: Vec<Session>,
    pub elsewhere: usize,
}

pub struct Instances {
    pub limit: Option<usize>,
    pub count: usize,
    pub counted: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reason {
    Dependencies(Vec<String>),
    SubscriptionProfile {
        profile: String,
        session: Option<String>,
    },
    InstanceArchiving(String),
    LiveInstanceLimit(usize),
    ActiveWorkSlots(usize),
    Ahead {
        sessions: Vec<String>,
        elsewhere: usize,
    },
}

pub struct Queued {
    pub session: Session,
    pub position: Option<usize>,
    pub reasons: Vec<Reason>,
}

pub struct Waiting {
    pub session: Session,
    pub position: Option<usize>,
    pub pending_since: Option<Timestamp>,
    pub reasons: Vec<Reason>,
}

pub struct Unbriefed {
    pub session: Session,
    pub position: Option<usize>,
    pub pending_since: Option<Timestamp>,
    pub brief_since: Option<Timestamp>,
    pub reasons: Vec<Reason>,
}

pub enum Occupied {
    Claimed(Session),
    Resumed(Session),
}

struct Candidate {
    session: Session,
    workspace: Workspace,
    blocked: bool,
    held_since: Option<Timestamp>,
}

#[derive(Clone, Copy)]
enum Request {
    None,
    Provision,
    Claim(Timestamp),
    Resume(Timestamp),
    FirstTurn(Timestamp),
}

impl Request {
    fn since(self) -> Option<Timestamp> {
        match self {
            Self::Claim(at) | Self::Resume(at) | Self::FirstTurn(at) => Some(at),
            Self::None | Self::Provision => None,
        }
    }

    fn input(self) -> bool {
        matches!(self, Self::Resume(_) | Self::FirstTurn(_))
    }

    fn order(self, session: &Session) -> (Timestamp, u8, Timestamp, String) {
        let at = self.since().expect("only slot requests are ordered");
        let (kind, tie) = match self {
            Self::Resume(_) => (0, at),
            Self::FirstTurn(_) => (1, session.enqueued_at),
            Self::Claim(_) => (2, at),
            Self::None | Self::Provision => unreachable!("only slot requests are ordered"),
        };
        (at, kind, tie, session.id.to_string())
    }
}

struct Evaluation {
    request: Request,
    blocked: bool,
    profile_held: bool,
    admission: Option<Admission>,
}

impl Evaluation {
    fn eligible(&self) -> bool {
        !self.blocked
            && !self.profile_held
            && self.request.since().is_some()
            && self
                .admission
                .as_ref()
                .is_none_or(|admission| matches!(admission, Admission::Available))
    }

    fn can_attempt(&self, full: bool, input_since: Option<Timestamp>) -> bool {
        if self.blocked || self.profile_held {
            return false;
        }
        match self.request {
            Request::Provision => true,
            Request::Claim(at) => !full && input_since.is_none_or(|since| at < since),
            Request::Resume(_) | Request::FirstTurn(_) => !full,
            Request::None => false,
        }
    }
}

async fn candidates(tx: &mut Tx<'_>) -> Result<Vec<Candidate>> {
    let claimable: HashSet<SessionId> = tx
        .workspaces()
        .claimable_sessions()
        .await?
        .into_iter()
        .map(|session| session.id)
        .collect();
    let held: HashMap<SessionId, Timestamp> = tx.queue().held_input().await?.into_iter().collect();
    let mut candidates = Vec::new();
    for session in tx.workspaces().pending_sessions().await? {
        let workspace = tx.workspaces().get(session.workspace).await?;
        let blocked = session.state == SessionState::Queued && !claimable.contains(&session.id);
        let held_since = held.get(&session.id).copied();
        candidates.push(Candidate {
            session,
            workspace,
            blocked,
            held_since,
        });
    }
    Ok(candidates)
}

async fn evaluate(
    tx: &mut Tx<'_>,
    candidate: &Candidate,
    serialized: &[String],
) -> Result<Evaluation> {
    let request = match candidate.session.state {
        SessionState::Queued => {
            if work::awaiting_a_brief(tx, &candidate.workspace).await? {
                Request::Provision
            } else {
                Request::Claim(candidate.session.enqueued_at)
            }
        }
        SessionState::Waiting => candidate.held_since.map_or(Request::None, Request::Resume),
        SessionState::Unbriefed if candidate.session.preparing == Some(Preparing::HarnessReady) => {
            tx.log()
                .unfollowed_brief(&candidate.workspace)
                .await?
                .map_or(Request::None, |(_, at)| Request::FirstTurn(at))
        }
        _ => Request::None,
    };
    let profile_held = !matches!(request, Request::None | Request::Provision)
        && (tx
            .workspaces()
            .holds_profile(&candidate.session, serialized)
            .await?
            || tx
                .sign_ins()
                .lent_to_another(&candidate.session, serialized)
                .await?);
    let admission = if candidate.session.state == SessionState::Queued {
        Some(instance::admission(tx, &candidate.workspace).await?)
    } else {
        None
    };
    Ok(Evaluation {
        request,
        blocked: candidate.blocked,
        profile_held,
        admission,
    })
}

pub async fn dispatch(
    store: &Store,
    slots: usize,
    serialized: &[String],
) -> Result<Option<Occupied>> {
    let mut tx = store.begin().await?;
    let occupied = dispatching(&mut tx, slots, serialized, false).await?;
    tx.commit().await?;
    Ok(occupied)
}

pub(crate) async fn claim(store: &Store, serialized: &[String]) -> Result<Option<Session>> {
    let mut tx = store.begin().await?;
    let claimed = match dispatching(&mut tx, usize::MAX, serialized, true).await? {
        Some(Occupied::Claimed(session)) => Some(session),
        Some(Occupied::Resumed(_)) => unreachable!("fixture claims never resume input"),
        None => None,
    };
    tx.commit().await?;
    Ok(claimed)
}

async fn dispatching(
    tx: &mut Tx<'_>,
    slots: usize,
    serialized: &[String],
    claim_only: bool,
) -> Result<Option<Occupied>> {
    let full = tx.workspaces().occupying_slots().await? >= slots;
    let candidates = candidates(tx).await?;
    let mut input = None;
    if !claim_only {
        for candidate in &candidates {
            if candidate.session.state == SessionState::Queued {
                continue;
            }
            let evaluation = evaluate(tx, candidate, serialized).await?;
            if evaluation.eligible()
                && input
                    .as_ref()
                    .is_none_or(|(previous, request): &(&Candidate, Request)| {
                        evaluation.request.order(&candidate.session)
                            < request.order(&previous.session)
                    })
            {
                input = Some((candidate, evaluation.request));
            }
        }
    }
    let input_since = input.as_ref().and_then(|(_, request)| request.since());
    for candidate in &candidates {
        if candidate.session.state != SessionState::Queued || candidate.blocked {
            continue;
        }
        // Reclamation can release a Profile or change admission for later candidates.
        let evaluation = evaluate(tx, candidate, serialized).await?;
        if !evaluation.can_attempt(full, input_since) {
            continue;
        }
        match evaluation
            .admission
            .expect("queued candidates have admission")
        {
            Admission::Available => {
                let unbriefed = matches!(evaluation.request, Request::Provision);
                if let Some(session) = tx
                    .workspaces()
                    .claim_session(
                        &candidate.session,
                        Timestamp::now() + work::LEASE,
                        unbriefed,
                    )
                    .await?
                {
                    return Ok(Some(Occupied::Claimed(session)));
                }
            }
            Admission::Archivable(kept) => instance::reclaim(tx, &kept).await?,
            Admission::Archiving(_) | Admission::AtLimit(_) => {}
        }
    }
    if !full && let Some((candidate, request)) = input {
        match request {
            Request::FirstTurn(_) => {
                let brief = tx
                    .log()
                    .unfollowed_brief(&candidate.workspace)
                    .await?
                    .map(|(brief, _)| brief)
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "the session {} has no Brief to prompt",
                            candidate.session.id
                        )
                    })?;
                link::prompt(tx, &candidate.session, brief).await?;
            }
            Request::Resume(_) => work::prompt_pending(tx, &candidate.session).await?,
            _ => unreachable!("only input is resumed"),
        }
        return Ok(Some(Occupied::Resumed(
            tx.workspaces().session(candidate.session.id).await?,
        )));
    }
    Ok(None)
}

pub async fn snapshot(store: &Store, name: &str) -> Result<Snapshot> {
    let mut tx = store.read().await?;
    let organization = tx.organizations().named(name).await?;
    let recorded = tx.queue().recorded().await?;
    let serialized = recorded
        .as_ref()
        .map_or(&[][..], |recorded| &recorded.serialized_harnesses);
    let active_work = active_work(&mut tx, &organization, recorded.as_ref()).await?;
    let instances = instances(&mut tx, &organization).await?;
    let full = active_work
        .limit
        .is_some_and(|limit| active_work.occupied >= limit);
    let candidates = candidates(&mut tx).await?;
    let mut evaluated = Vec::new();
    for candidate in &candidates {
        evaluated.push(evaluate(&mut tx, candidate, serialized).await?);
    }
    let mut order: Vec<usize> = evaluated
        .iter()
        .enumerate()
        .filter(|(_, evaluation)| evaluation.eligible())
        .map(|(index, _)| index)
        .collect();
    order.sort_by_key(|&index| evaluated[index].request.order(&candidates[index].session));
    let input_since = evaluated
        .iter()
        .filter(|evaluation| evaluation.eligible() && evaluation.request.input())
        .filter_map(|evaluation| evaluation.request.since())
        .min();
    let mut held: HashMap<SessionId, Vec<Reason>> = HashMap::new();
    if recorded.is_some() {
        for (session, profile, holder) in
            tx.queue().profiles_held(&organization, serialized).await?
        {
            held.entry(session)
                .or_default()
                .push(Reason::SubscriptionProfile {
                    profile,
                    session: holder,
                });
        }
        for (session, profile) in tx.sign_ins().lent(&organization, serialized).await? {
            held.entry(session)
                .or_default()
                .push(Reason::SubscriptionProfile {
                    profile,
                    session: None,
                });
        }
    }
    let mut blocked: HashMap<SessionId, Vec<String>> = HashMap::new();
    for (session, blocker) in tx.queue().waiting_on(&organization).await? {
        blocked.entry(session.parse()?).or_default().push(blocker);
    }
    let identities: Vec<_> = candidates
        .iter()
        .map(|candidate| (&candidate.session.name, candidate.session.organization))
        .collect();
    let mut explanations = HashMap::new();
    if recorded.is_some() {
        for (index, candidate) in candidates.iter().enumerate() {
            if candidate.session.organization != organization.id || !evaluated[index].eligible() {
                continue;
            }
            let key = evaluated[index].request.order(&candidate.session);
            let mut sessions = Vec::new();
            let mut elsewhere = 0;
            for &ahead in &order {
                if evaluated[ahead].request.order(&candidates[ahead].session) >= key {
                    break;
                }
                let (name, owner) = identities[ahead];
                if owner == organization.id {
                    sessions.push(name.clone());
                } else {
                    elsewhere += 1;
                }
            }
            if !sessions.is_empty() || elsewhere > 0 {
                explanations.insert(
                    index,
                    Reason::Ahead {
                        sessions,
                        elsewhere,
                    },
                );
            }
        }
    }
    let mut queued = Vec::new();
    let mut waiting = Vec::new();
    let mut unbriefed = Vec::new();
    for (index, candidate) in candidates.into_iter().enumerate() {
        if candidate.session.organization != organization.id {
            continue;
        }
        let evaluation = &evaluated[index];
        let position = recorded.as_ref().and_then(|_| {
            order
                .iter()
                .position(|&ordered| ordered == index)
                .map(|position| position + 1)
        });
        let mut reasons = Vec::new();
        if let Some(blockers) = blocked.remove(&candidate.session.id) {
            reasons.push(Reason::Dependencies(blockers));
        }
        if evaluation.profile_held {
            reasons.extend(held.remove(&candidate.session.id).unwrap_or_default());
        }
        if let Some(admission) = &evaluation.admission {
            match admission {
                Admission::Available => {}
                Admission::Archiving(instance) => {
                    reasons.push(Reason::InstanceArchiving(instance.clone()))
                }
                Admission::Archivable(kept) if evaluation.can_attempt(full, input_since) => {
                    reasons.push(Reason::InstanceArchiving(kept.instance.clone()));
                }
                Admission::Archivable(_) => {
                    reasons.extend(
                        candidate
                            .workspace
                            .organization
                            .max_live_instances
                            .map(|limit| Reason::LiveInstanceLimit(limit.get())),
                    );
                }
                Admission::AtLimit(limit) => reasons.push(Reason::LiveInstanceLimit(limit.get())),
            }
        }
        if evaluation.request.input() && full {
            reasons.insert(
                0,
                Reason::ActiveWorkSlots(active_work.limit.expect("full slots have a limit")),
            );
        }
        if let Some(ahead) = explanations.remove(&index) {
            reasons.push(ahead);
        }
        match candidate.session.state {
            SessionState::Queued => queued.push(Queued {
                session: candidate.session,
                position,
                reasons,
            }),
            SessionState::Waiting => waiting.push(Waiting {
                session: candidate.session,
                position,
                pending_since: candidate.held_since,
                reasons,
            }),
            SessionState::Unbriefed => {
                let pending_since = tx
                    .workspaces()
                    .pending_since(candidate.session.workspace)
                    .await?;
                let brief_since = match evaluation.request {
                    Request::FirstTurn(at) => Some(at),
                    _ => None,
                };
                unbriefed.push(Unbriefed {
                    session: candidate.session,
                    position,
                    pending_since,
                    brief_since,
                    reasons,
                });
            }
            _ => unreachable!("only pending Sessions are candidates"),
        }
    }
    waiting.sort_by_key(|row| {
        (
            row.pending_since.is_none(),
            row.pending_since.unwrap_or(row.session.enqueued_at),
            row.session.id.to_string(),
        )
    });
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
    let occupants = tx.workspaces().sessions_occupying(organization).await?;
    let occupied = tx.workspaces().occupying_slots().await?;
    Ok(ActiveWork {
        limit: recorded.map(|recorded| recorded.active_work_slots),
        occupied,
        elsewhere: occupied - occupants.len(),
        occupants,
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
