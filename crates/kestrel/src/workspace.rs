use anyhow::{Result, bail};
use jiff::{SignedDuration, Timestamp};

use crate::declined::{Declined, FieldRefusal, Kind};
use crate::domain::{
    Agent, Declared, Exit, HeldMessage, Organization, Preparing, Project, Session, SessionId,
    SessionState, StartedBy, SubscriptionProfile, Workspace, WorkspaceId, WorkspaceState,
};
use crate::instance;
use crate::log::{BriefSource, Cursor, Entry, Message, Page, Unreadable, Window};
use crate::participant;
use crate::store::workspace::{Opening, PendingSession, Unfinished};
use crate::store::{Store, Tx};
use crate::work;

/// Generous, because kestrel has no signal that a human is watching a Workspace: duration is
/// standing in for presence.
pub(crate) const IDLE: SignedDuration = SignedDuration::from_hours(24);

pub struct Open<'a> {
    pub project: &'a str,
    pub agent: &'a str,
    pub profile: Option<&'a str>,
    pub branch: Option<&'a str>,
    pub continues: Option<&'a str>,
    pub declared: Declared,
    pub brief: Option<&'a str>,
    pub participant: Option<&'a str>,
    pub depends_on: &'a [String],
}

pub(crate) struct Resolved<'a> {
    pub project: Project,
    pub agent: Agent,
    pub profile: Option<SubscriptionProfile>,
    pub continues: Option<Workspace>,
    pub branch: Option<&'a str>,
    pub declared: Declared,
    pub brief: Option<&'a str>,
    pub participant: Option<String>,
}

/// One write, or nothing left behind (ADR-0038).
pub async fn open(
    store: &Store,
    organization: &str,
    open: Open<'_>,
) -> Result<(Workspace, Session)> {
    let mut tx = store.begin().await?;
    let organization = tx.organizations().named(organization).await?;
    let blockers = work::blockers(&mut tx, &organization, open.depends_on).await?;
    let resolved = resolved(&mut tx, &organization, open).await?;
    let (workspace, session) = opened_in(&mut tx, &organization, &resolved).await?;
    let session = work::record_dependencies(&mut tx, session, &blockers).await?;
    tx.commit().await?;

    Ok((workspace, session))
}

/// No operator path opens a Workspace without a Session; fixtures that need that state call this.
pub async fn open_without_a_session(
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
    let resolved = resolved(
        &mut tx,
        &organization,
        Open {
            project,
            agent,
            profile,
            branch,
            continues,
            declared: Declared::default(),
            brief: None,
            participant: None,
            depends_on: &[],
        },
    )
    .await?;
    let workspace = tx
        .workspaces()
        .open(Opening {
            organization: &organization,
            project: &resolved.project,
            agent: &resolved.agent,
            profile: resolved.profile.as_ref(),
            branch: resolved.branch,
            correlation: None,
            continues: resolved.continues.as_ref(),
            started_by: None,
        })
        .await?;
    ensure_joined(&mut tx, &workspace, &resolved.agent.name).await?;
    tx.commit().await?;

    Ok(workspace)
}

/// Shared by open and start, so the same inputs give the same Transcript.
pub(crate) async fn opened_in(
    tx: &mut Tx<'_>,
    organization: &Organization,
    resolved: &Resolved<'_>,
) -> Result<(Workspace, Session)> {
    let workspace = tx
        .workspaces()
        .open(Opening {
            organization,
            project: &resolved.project,
            agent: &resolved.agent,
            profile: resolved.profile.as_ref(),
            branch: resolved.branch,
            correlation: None,
            continues: resolved.continues.as_ref(),
            started_by: resolved
                .participant
                .as_ref()
                .map(|participant| StartedBy::Participant(participant.clone())),
        })
        .await?;
    ensure_joined(tx, &workspace, &resolved.agent.name).await?;
    if let Some(participant) = &resolved.participant {
        ensure_joined(tx, &workspace, participant).await?;
    }
    if let Some(brief) = resolved.brief {
        tx.log()
            .append(
                &workspace,
                Entry::Brief {
                    source: BriefSource::Operator {
                        participant: resolved.participant.clone(),
                    },
                    brief: brief.to_owned(),
                },
            )
            .await?;
    }
    let session = tx
        .workspaces()
        .enqueue_session(&workspace, Some(&resolved.agent), resolved.declared.clone())
        .await?;

    Ok((workspace, session))
}

async fn resolved<'a>(
    tx: &mut Tx<'_>,
    organization: &Organization,
    open: Open<'a>,
) -> Result<Resolved<'a>> {
    let project = named(
        "project",
        tx.projects().named(organization, open.project).await,
    )?;
    let agent = named("agent", tx.agents().named(organization, open.agent).await)?;
    let profile = match open.profile {
        Some(profile) => Some(named(
            "profile",
            tx.profiles().named(organization, profile).await,
        )?),
        None => None,
    };
    let continues = match open.continues {
        Some(reference) => Some(named(
            "continues",
            continued(tx, organization, reference).await,
        )?),
        None => None,
    };

    if open.branch.is_some_and(|branch| branch.trim().is_empty()) {
        return Err(FieldRefusal::unacceptable(
            "branch",
            "a branch cannot be empty; omit it for the branch the workspace declares",
        )
        .into());
    }
    if open.branch.is_some() && continues.is_some() {
        return Err(FieldRefusal::unacceptable(
            "branch",
            "a continuation runs on the branch of the workspace it continues, and names none of its own",
        )
        .into());
    }
    for (field, value) in [
        ("model", &open.declared.model),
        ("mode", &open.declared.mode),
        ("thought_level", &open.declared.thought_level),
    ] {
        if value.as_ref().is_some_and(|value| value.trim().is_empty()) {
            return Err(FieldRefusal::unacceptable(
                field,
                format!("a {field} cannot be empty; omit it for the Agent's"),
            )
            .into());
        }
    }
    if open.brief.is_some_and(|brief| brief.trim().is_empty()) {
        return Err(FieldRefusal::unacceptable(
            "brief",
            "a brief cannot be empty; omit it to open without one",
        )
        .into());
    }
    let participant = match (open.participant, open.brief) {
        (Some(participant), Some(_)) => {
            Some(participant::accepted(tx, organization, participant).await?)
        }
        (Some(_), None) => {
            return Err(FieldRefusal::unacceptable(
                "participant",
                "a participant names the author of a brief, and this open carries none",
            )
            .into());
        }
        (None, _) => None,
    };

    Ok(Resolved {
        project,
        agent,
        profile,
        continues,
        branch: open.branch,
        declared: Declared::named(open.declared),
        brief: open.brief,
        participant,
    })
}

pub(crate) fn named<T>(field: &'static str, named: Result<T>) -> Result<T> {
    named.map_err(|error| match error.downcast::<Declined>() {
        Ok(declined) => {
            let kind = match &declined {
                Declined::Unacceptable(_) => Kind::Unacceptable,
                Declined::Missing(_) => Kind::Missing,
                Declined::Ambiguous(_) => Kind::Ambiguous,
                Declined::Taken(_) => Kind::Taken,
            };
            FieldRefusal {
                field,
                message: declined.to_string(),
                kind,
            }
            .into()
        }
        Err(error) => error,
    })
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
    Brief(&'a Session),
    Held,
    Wake(&'a Session),
    Prompt(&'a Session),
    Unbriefed(&'a Session),
}

impl UnfinishedSession {
    /// A waiting or unbriefed Session is not in flight: sealing (ADR-0024) or archiving its
    /// Instance ends it.
    pub fn in_flight(&self) -> Option<SessionId> {
        self.session.as_ref().and_then(|session| {
            (!matches!(
                session.state,
                SessionState::Ended | SessionState::Waiting | SessionState::Unbriefed
            ) || self.held_input)
                .then_some(session.id)
        })
    }

    /// A Session between turns, or one waiting for its Brief: its Instance can be taken without
    /// anything failing.
    pub fn yielding(&self) -> Option<Session> {
        self.session
            .as_ref()
            .filter(|session| {
                matches!(
                    session.state,
                    SessionState::Waiting | SessionState::Unbriefed
                )
            })
            .cloned()
    }

    pub async fn end_waiting(&self, tx: &mut Tx<'_>) -> Result<()> {
        if let Some(yielding) = self.yielding() {
            work::ending(tx, &yielding, Exit::Succeeded).await?;
        }

        Ok(())
    }

    pub fn post_destination(&self) -> PostDestination<'_> {
        match &self.session {
            None => PostDestination::Start,
            Some(session) if session.state == SessionState::Queued => {
                PostDestination::Brief(session)
            }
            Some(session) if session.state == SessionState::Waiting => {
                PostDestination::Wake(session)
            }
            Some(session) if session.state == SessionState::Trailing => {
                PostDestination::Prompt(session)
            }
            Some(session) if session.state == SessionState::Unbriefed => {
                PostDestination::Unbriefed(session)
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

pub async fn unfinished(store: &Store, id: WorkspaceId) -> Result<Option<Session>> {
    let mut tx = store.begin().await?;
    let workspace = tx.workspaces().get(id).await?;

    Ok(unfinished_session(&mut tx, &workspace).await?.session)
}

pub async fn workspaces(store: &Store, organization: &str) -> Result<Vec<Workspace>> {
    let mut tx = store.begin().await?;
    let organization = tx.organizations().named(organization).await?;

    tx.workspaces().all(&organization).await
}

pub async fn continuations(store: &Store, id: WorkspaceId) -> Result<Vec<WorkspaceId>> {
    store.begin().await?.workspaces().continuations(id).await
}

#[derive(Debug)]
pub struct Posted {
    pub session: Option<Session>,
    pub held_message: Option<HeldMessage>,
}

pub async fn post(
    store: &Store,
    id: WorkspaceId,
    participant: &str,
    message: &str,
) -> Result<Posted> {
    let mut tx = store.begin().await?;
    let workspace = tx.workspaces().get(id).await?;
    let posted = post_in(&mut tx, &workspace, participant, message).await?;
    tx.commit().await?;

    Ok(posted)
}

pub(crate) async fn post_in(
    tx: &mut Tx<'_>,
    workspace: &Workspace,
    participant: &str,
    message: &str,
) -> Result<Posted> {
    let participant = participant::accepted(tx, &workspace.organization, participant).await?;

    post_as(tx, workspace, &participant, message).await
}

pub(crate) async fn post_as(
    tx: &mut Tx<'_>,
    workspace: &Workspace,
    participant: &str,
    message: &str,
) -> Result<Posted> {
    workspace.accepts("message")?;
    let unfinished = unfinished_session(tx, workspace).await?;
    match unfinished.post_destination() {
        PostDestination::Start => {
            ensure_joined(tx, workspace, participant).await?;
            said(tx, workspace, participant, message).await?;
            let session = tx
                .workspaces()
                .enqueue_session(workspace, None, Declared::default())
                .await?;
            Ok(Posted {
                session: Some(session),
                held_message: None,
            })
        }
        PostDestination::Brief(queued) => {
            if work::awaiting_a_brief(tx, workspace).await? {
                brief(tx, workspace, participant, message).await?;
            } else {
                ensure_joined(tx, workspace, participant).await?;
                said(tx, workspace, participant, message).await?;
            }
            Ok(Posted {
                session: Some(queued.clone()),
                held_message: None,
            })
        }
        PostDestination::Held => {
            let held_message = tx
                .workspaces()
                .add_pending_message(workspace, participant, message)
                .await?;
            Ok(Posted {
                session: None,
                held_message: Some(held_message),
            })
        }
        // Held even for a waiting Session: its next turn waits for an active-work slot.
        PostDestination::Wake(waiting) => {
            let held_message = tx
                .workspaces()
                .add_pending_message(workspace, participant, message)
                .await?;
            Ok(Posted {
                session: Some(waiting.clone()),
                held_message: Some(held_message),
            })
        }
        // A trailing Session already holds the slot its next Turn takes.
        PostDestination::Prompt(trailing) => {
            let posted = tx
                .workspaces()
                .add_pending_message(workspace, participant, message)
                .await?;
            work::prompt_pending(tx, trailing).await?;
            let held_message = tx
                .workspaces()
                .held_messages(workspace.id)
                .await?
                .into_iter()
                .find(|held| held.id == posted.id);
            Ok(Posted {
                session: Some(tx.workspaces().session(trailing.id).await?),
                held_message,
            })
        }
        PostDestination::Unbriefed(unbriefed) => {
            let held_message = if unbriefed.preparing == Some(Preparing::HarnessReady) {
                brief_or_hold(tx, workspace, participant, message).await?
            } else {
                Some(
                    tx.workspaces()
                        .add_pending_message(workspace, participant, message)
                        .await?,
                )
            };

            Ok(Posted {
                session: Some(unbriefed.clone()),
                held_message,
            })
        }
    }
}

/// An author replaces a Held Message's text, in place rather than on the record.
pub async fn edit_message(
    store: &Store,
    workspace: WorkspaceId,
    id: i64,
    participant: &str,
    message: &str,
) -> Result<HeldMessage> {
    let mut tx = store.begin().await?;
    let workspace = tx.workspaces().get(workspace).await?;
    let participant = participant::accepted(&mut tx, &workspace.organization, participant).await?;
    let edited = tx
        .workspaces()
        .edit_held_message(&workspace, id, &participant, message)
        .await?;
    tx.commit().await?;

    Ok(edited)
}

/// An author takes a Held Message back, so no Turn sees it. Withdrawing records nothing.
pub async fn withdraw_message(
    store: &Store,
    workspace: WorkspaceId,
    id: i64,
    participant: &str,
) -> Result<()> {
    let mut tx = store.begin().await?;
    let workspace = tx.workspaces().get(workspace).await?;
    let participant = participant::accepted(&mut tx, &workspace.organization, participant).await?;
    tx.workspaces()
        .withdraw_held_message(&workspace, id, &participant)
        .await?;
    tx.commit().await?;

    Ok(())
}

pub async fn held_messages(store: &Store, id: WorkspaceId) -> Result<Vec<HeldMessage>> {
    store.begin().await?.workspaces().held_messages(id).await
}

/// A held message is handed back so its author can still edit or withdraw it.
async fn brief_or_hold(
    tx: &mut Tx<'_>,
    workspace: &Workspace,
    participant: &str,
    message: &str,
) -> Result<Option<HeldMessage>> {
    if work::awaiting_a_brief(tx, workspace).await? {
        brief(tx, workspace, participant, message).await?;
        Ok(None)
    } else {
        tx.workspaces()
            .add_pending_message(workspace, participant, message)
            .await
            .map(Some)
    }
}

async fn brief(
    tx: &mut Tx<'_>,
    workspace: &Workspace,
    participant: &str,
    message: &str,
) -> Result<()> {
    ensure_joined(tx, workspace, participant).await?;
    tx.log()
        .append(
            workspace,
            Entry::Brief {
                source: BriefSource::Operator {
                    participant: Some(participant.to_owned()),
                },
                brief: message.to_owned(),
            },
        )
        .await?;

    tx.queue().changed();
    Ok(())
}

pub(crate) async fn first_held_becomes_the_brief(
    tx: &mut Tx<'_>,
    workspace: &Workspace,
) -> Result<bool> {
    if !work::awaiting_a_brief(tx, workspace).await? {
        return Ok(false);
    }
    let Some(pending) = tx
        .workspaces()
        .take_oldest_pending_message(workspace)
        .await?
    else {
        return Ok(false);
    };
    ensure_joined(tx, workspace, &pending.participant).await?;
    tx.log()
        .append(
            workspace,
            Entry::Brief {
                source: BriefSource::Operator {
                    participant: Some(pending.participant),
                },
                brief: pending.message,
            },
        )
        .await?;

    Ok(true)
}

async fn ensure_joined(tx: &mut Tx<'_>, workspace: &Workspace, participant: &str) -> Result<()> {
    if !tx.log().has_joined(workspace, participant).await? {
        tx.log()
            .append(
                workspace,
                Entry::ParticipantJoined {
                    participant: participant.to_owned(),
                },
            )
            .await?;
    }

    Ok(())
}

/// Joins each new author of a drained batch, in message order, all immediately before the entry
/// that carries the batch.
pub(crate) async fn join_authors(
    tx: &mut Tx<'_>,
    workspace: &Workspace,
    messages: &[Message],
) -> Result<()> {
    for message in messages {
        ensure_joined(tx, workspace, &message.participant).await?;
    }

    Ok(())
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
                source: BriefSource::Trigger {
                    trigger: pending.trigger,
                },
                brief: pending.brief,
            },
        )
        .await?;

    tx.workspaces()
        .enqueue_session(workspace, Some(&pending.agent), pending.declared)
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
                session_id: None,
                completion: None,
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
    kinds: &crate::log::Kinds,
) -> Result<Page, Unreadable> {
    let mut tx = store.begin().await?;
    let workspace = tx.workspaces().get(id).await?;

    tx.log().page(&workspace, from, window, kinds).await
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
        bail!(Declined::Unacceptable(format!(
            "the workspace {} is open, and work continues in it rather than after it",
            sealed.id
        )));
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
                declared: Declared::default(),
            },
            state,
            preparing: None,
            exit: None,
            outcome_message: None,
            instance: None,
            supervisor: None,
            worked_model: None,
            title: None,
            options: Vec::new(),
            changing_options: Vec::new(),
            commands: Vec::new(),
            interrupting: None,
            enqueued_at: Timestamp::now(),
            started_at: None,
            ended_at: None,
            lease_expires_at: None,
            connected: None,
            usage: None,
            depends_on: Vec::new(),
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
        use SessionState::{Ended, Queued, Trailing, Unbriefed, Unreachable, Waiting, Working};
        let brief: fn(&PostDestination) -> bool = |post| matches!(post, PostDestination::Brief(_));
        let held: fn(&PostDestination) -> bool = |post| matches!(post, PostDestination::Held);
        let wake: fn(&PostDestination) -> bool = |post| matches!(post, PostDestination::Wake(_));
        let prompt: fn(&PostDestination) -> bool =
            |post| matches!(post, PostDestination::Prompt(_));
        let unbriefed: fn(&PostDestination) -> bool =
            |post| matches!(post, PostDestination::Unbriefed(_));
        #[rustfmt::skip]
        let cases = [
            Case { state: Queued,      held_input: false, in_flight: true,  post: brief },
            Case { state: Queued,      held_input: true,  in_flight: true,  post: brief },
            Case { state: Working,     held_input: false, in_flight: true,  post: held },
            Case { state: Working,     held_input: true,  in_flight: true,  post: held },
            Case { state: Trailing,    held_input: false, in_flight: true,  post: prompt },
            Case { state: Trailing,    held_input: true,  in_flight: true,  post: prompt },
            Case { state: Waiting,     held_input: false, in_flight: false, post: wake },
            Case { state: Waiting,     held_input: true,  in_flight: true,  post: wake },
            Case { state: Unbriefed,   held_input: false, in_flight: false, post: unbriefed },
            Case { state: Unbriefed,   held_input: true,  in_flight: true,  post: unbriefed },
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
                unfinished.yielding().is_some(),
                matches!(case.state, Waiting | Unbriefed),
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
