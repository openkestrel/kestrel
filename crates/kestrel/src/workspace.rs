use anyhow::{Result, bail};
use jiff::{SignedDuration, Timestamp};

use crate::domain::{
    Exit, Organization, Session, SessionId, SessionState, Workspace, WorkspaceId, WorkspaceState,
};
use crate::fanout::{self, Change};
use crate::instance;
use crate::log::{Cursor, Entry, Page, Unreadable, Window};
use crate::store::workspace::{Opening, PendingSession, Unfinished};
use crate::store::{Store, Tx};
use crate::work;

/// Generous, because kestrel has no signal that a human is watching a Workspace: duration is
/// standing in for presence.
const IDLE: SignedDuration = SignedDuration::from_hours(24);

pub async fn open(
    store: &Store,
    organization: &str,
    project: &str,
    agent: &str,
    profile: Option<&str>,
    branch: Option<&str>,
    continues: Option<&str>,
) -> Result<Workspace> {
    let mut tx = store.begin().await?;

    let organization = tx.organizations().named(organization).await?;
    let project = tx.projects().named(&organization, project).await?;
    let agent = tx.agents().named(&organization, agent).await?;
    let profile = match profile {
        Some(profile) => Some(tx.profiles().named(&organization, profile).await?),
        None => None,
    };
    let continues = match continues {
        Some(reference) => Some(continued(&mut tx, &organization, reference).await?),
        None => None,
    };

    let workspace = tx
        .workspaces()
        .open(Opening {
            organization: &organization,
            project: &project,
            agent: &agent,
            profile: profile.as_ref(),
            branch,
            correlation: None,
            continues: continues.as_ref(),
            started_by: None,
        })
        .await?;
    tx.log()
        .append(
            &workspace,
            Entry::ParticipantJoined {
                participant: workspace.opened_with.name.clone(),
            },
        )
        .await?;

    tx.commit().await?;
    fanout::publish(Change::WorkspaceOpened(&workspace));

    Ok(workspace)
}

pub async fn seal(store: &Store, id: WorkspaceId) -> Result<Workspace> {
    let mut tx = store.begin().await?;
    let workspace = tx.workspaces().get(id).await?;

    if workspace.state == WorkspaceState::Sealed {
        bail!("the workspace {id} is already sealed, and a sealed workspace is never reopened");
    }
    if let Some(holding) = unfinished_session(&mut tx, &workspace).await?.in_flight() {
        bail!("the session {holding} is still in flight in the workspace {id}");
    }
    instance::archive_on_seal(&mut tx, &workspace).await?;

    let sealed_at = tx.workspaces().seal(&workspace).await?;
    tx.commit().await?;

    let sealed = Workspace {
        state: WorkspaceState::Sealed,
        sealed_at: Some(sealed_at),
        ..workspace
    };
    fanout::publish(Change::WorkspaceSealed(&sealed));

    Ok(sealed)
}

/// Unattended sealing, through the same command a person seals with, so nothing here can
/// decide differently to `seal`.
pub async fn seal_idle(store: &Store) -> Result<Vec<Workspace>> {
    let mut sealed = Vec::new();

    for id in idle(store).await? {
        sealed.push(seal(store, id).await?);
    }

    Ok(sealed)
}

async fn idle(store: &Store) -> Result<Vec<WorkspaceId>> {
    let mut tx = store.begin().await?;
    let mut idle = Vec::new();

    for workspace in tx.workspaces().idle(Timestamp::now() - IDLE).await? {
        let holds_unpublished_work = match tx.workspaces().kept_instance(workspace.id).await? {
            Some(kept) => {
                instance::unpublished(&workspace.checkout.repositories, kept.observed.as_deref())
                    .is_some()
            }
            None => false,
        };
        if !holds_unpublished_work && unfinished_session(&mut tx, &workspace).await?.idle() {
            idle.push(workspace.id);
        }
    }

    Ok(idle)
}

pub(crate) struct UnfinishedSession {
    session: Option<Session>,
    held_input: bool,
}

pub(crate) async fn unfinished_session(
    tx: &mut Tx<'_>,
    workspace: &Workspace,
) -> Result<UnfinishedSession> {
    Ok(match tx.workspaces().unfinished_session(workspace).await? {
        Some(Unfinished {
            session,
            held_input,
        }) => UnfinishedSession {
            session: Some(session),
            held_input,
        },
        None => UnfinishedSession {
            session: None,
            held_input: false,
        },
    })
}

pub(crate) enum PostDestination<'a> {
    Start,
    Brief,
    Held,
    Wake(&'a Session),
}

impl UnfinishedSession {
    /// A waiting Session is not in flight: sealing (ADR-0024) or archiving its Instance ends it.
    pub fn in_flight(&self) -> Option<SessionId> {
        self.session.as_ref().and_then(|session| {
            (!matches!(session.state, SessionState::Ended | SessionState::Waiting)
                || self.held_input)
                .then_some(session.id)
        })
    }

    pub fn waiting(&self) -> Option<Session> {
        self.session
            .as_ref()
            .filter(|session| session.state == SessionState::Waiting)
            .cloned()
    }

    pub async fn end_waiting(&self, tx: &mut Tx<'_>) -> Result<()> {
        if let Some(waiting) = self.waiting() {
            work::ending(tx, &waiting, Exit::Succeeded).await?;
        }

        Ok(())
    }

    pub fn post_destination(&self) -> PostDestination<'_> {
        match &self.session {
            None => PostDestination::Start,
            Some(session) if session.state == SessionState::Queued => PostDestination::Brief,
            Some(session) if session.state == SessionState::Waiting => {
                PostDestination::Wake(session)
            }
            Some(_) => PostDestination::Held,
        }
    }

    pub fn refuses_enqueue(&self) -> Option<SessionId> {
        self.session.as_ref().map(|session| session.id)
    }

    pub fn idle(&self) -> bool {
        self.in_flight().is_none()
    }
}

pub async fn show(store: &Store, id: WorkspaceId) -> Result<Workspace> {
    store.begin().await?.workspaces().get(id).await
}

pub async fn workspaces(store: &Store, organization: &str) -> Result<Vec<Workspace>> {
    let mut tx = store.begin().await?;
    let organization = tx.organizations().named(organization).await?;

    tx.workspaces().all(&organization).await
}

pub async fn continuations(store: &Store, id: WorkspaceId) -> Result<Vec<WorkspaceId>> {
    store.begin().await?.workspaces().continuations(id).await
}

pub async fn post(
    store: &Store,
    id: WorkspaceId,
    participant: &str,
    message: &str,
) -> Result<Option<Session>> {
    let mut tx = store.begin().await?;
    let workspace = tx.workspaces().get(id).await?;
    let session = post_in(&mut tx, &workspace, participant, message).await?;
    tx.commit().await?;

    Ok(session)
}

pub(crate) async fn post_in(
    tx: &mut Tx<'_>,
    workspace: &Workspace,
    participant: &str,
    message: &str,
) -> Result<Option<Session>> {
    workspace.accepts("message")?;

    let unfinished = unfinished_session(tx, workspace).await?;
    match unfinished.post_destination() {
        PostDestination::Start => {
            said(tx, workspace, participant, message).await?;
            Ok(Some(
                tx.workspaces()
                    .enqueue_session(workspace, None, None)
                    .await?,
            ))
        }
        PostDestination::Brief => {
            said(tx, workspace, participant, message).await?;
            Ok(None)
        }
        PostDestination::Held => {
            tx.workspaces()
                .add_pending_message(workspace, participant, message)
                .await?;
            Ok(None)
        }
        // Held even for a waiting Session: its next turn waits for an active-work slot.
        PostDestination::Wake(waiting) => {
            tx.workspaces()
                .add_pending_message(workspace, participant, message)
                .await?;
            Ok(Some(waiting.clone()))
        }
    }
}

/// Waits for the unfinished Session to let go (ADR-0014), and ends one waiting between turns
/// rather than wait on it: a waiting Session may wait indefinitely, and ending it there is how it
/// succeeds (ADR-0031).
pub(crate) async fn start_in(
    tx: &mut Tx<'_>,
    workspace: &Workspace,
    pending: PendingSession,
) -> Result<Option<Session>> {
    workspace.accepts("session")?;

    let unfinished = unfinished_session(tx, workspace).await?;
    if unfinished.refuses_enqueue().is_none() {
        return Ok(Some(briefed(tx, workspace, pending).await?));
    }

    tx.workspaces()
        .add_pending_session(workspace, &pending)
        .await?;
    unfinished.end_waiting(tx).await?;

    Ok(None)
}

pub(crate) async fn briefed(
    tx: &mut Tx<'_>,
    workspace: &Workspace,
    pending: PendingSession,
) -> Result<Session> {
    tx.log()
        .append(
            workspace,
            Entry::Brief {
                trigger: Some(pending.trigger),
                brief: pending.brief,
            },
        )
        .await?;

    tx.workspaces()
        .enqueue_session(workspace, Some(&pending.agent), None)
        .await
}

async fn said(
    tx: &mut Tx<'_>,
    workspace: &Workspace,
    participant: &str,
    message: &str,
) -> Result<()> {
    tx.log()
        .append(
            workspace,
            Entry::Said {
                participant: participant.to_owned(),
                message: message.to_owned(),
            },
        )
        .await?;

    Ok(())
}

pub async fn transcript(
    store: &Store,
    id: WorkspaceId,
    from: Option<Cursor>,
    window: Window,
) -> Result<Page, Unreadable> {
    let mut tx = store.begin().await?;
    let workspace = tx.workspaces().get(id).await?;

    tx.log().page(&workspace, from, window).await
}

pub async fn resolve(store: &Store, organization: &str, reference: &str) -> Result<Workspace> {
    let mut tx = store.begin().await?;
    let organization = tx.organizations().named(organization).await?;

    tx.workspaces().resolved(&organization, reference).await
}

/// Only a sealed Workspace is continued: work an open one could still take belongs in it.
async fn continued(
    tx: &mut Tx<'_>,
    organization: &Organization,
    reference: &str,
) -> Result<Workspace> {
    let sealed = tx.workspaces().resolved(organization, reference).await?;

    if sealed.state != WorkspaceState::Sealed {
        bail!(
            "the workspace {} is open, and work continues in it rather than after it",
            sealed.id
        );
    }

    Ok(sealed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{Agent, AgentId, OrganizationId};

    fn session(state: SessionState) -> Session {
        let organization = OrganizationId::generate();
        Session {
            id: SessionId::generate(),
            name: "session".into(),
            organization,
            workspace: WorkspaceId::generate(),
            agent: Agent {
                id: AgentId::generate(),
                organization,
                name: "builder".into(),
                harness: "opencode".into(),
                model: None,
            },
            state,
            exit: None,
            outcome_message: None,
            instance: None,
            supervisor: None,
            worked_model: None,
            enqueued_at: Timestamp::now(),
            started_at: None,
            ended_at: None,
            lease_expires_at: None,
            connected: None,
            usage: None,
        }
    }

    struct Case {
        state: SessionState,
        held_input: bool,
        in_flight: bool,
        post: fn(&PostDestination) -> bool,
    }

    #[test]
    fn unfinished_session_rules_cover_every_phase_with_and_without_held_input() {
        use SessionState::{Ended, Queued, Unreachable, Waiting, Working};
        let brief: fn(&PostDestination) -> bool = |post| matches!(post, PostDestination::Brief);
        let held: fn(&PostDestination) -> bool = |post| matches!(post, PostDestination::Held);
        let wake: fn(&PostDestination) -> bool = |post| matches!(post, PostDestination::Wake(_));
        #[rustfmt::skip]
        let cases = [
            Case { state: Queued,      held_input: false, in_flight: true,  post: brief },
            Case { state: Queued,      held_input: true,  in_flight: true,  post: brief },
            Case { state: Working,     held_input: false, in_flight: true,  post: held },
            Case { state: Working,     held_input: true,  in_flight: true,  post: held },
            Case { state: Waiting,     held_input: false, in_flight: false, post: wake },
            Case { state: Waiting,     held_input: true,  in_flight: true,  post: wake },
            Case { state: Ended,       held_input: false, in_flight: false, post: held },
            Case { state: Ended,       held_input: true,  in_flight: true,  post: held },
            Case { state: Unreachable, held_input: false, in_flight: true,  post: held },
            Case { state: Unreachable, held_input: true,  in_flight: true,  post: held },
        ];
        for case in cases {
            let session = session(case.state);
            let unfinished = UnfinishedSession {
                session: Some(session.clone()),
                held_input: case.held_input,
            };
            let label = format!("{} with held input {}", case.state, case.held_input);
            assert_eq!(unfinished.in_flight().is_some(), case.in_flight, "{label}");
            assert_eq!(unfinished.idle(), !case.in_flight, "{label}");
            assert!((case.post)(&unfinished.post_destination()), "{label}");
            assert_eq!(unfinished.refuses_enqueue(), Some(session.id), "{label}");
            assert_eq!(
                unfinished.waiting().is_some(),
                case.state == Waiting,
                "{label}"
            );
        }
        let empty = UnfinishedSession {
            session: None,
            held_input: false,
        };
        assert!(matches!(empty.post_destination(), PostDestination::Start));
        assert!(empty.idle());
        assert_eq!(empty.refuses_enqueue(), None);
    }
}
