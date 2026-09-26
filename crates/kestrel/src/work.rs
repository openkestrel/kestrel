use std::fmt;

use anyhow::{Context as _, Result, bail};
use jiff::{SignedDuration, Timestamp};
use serde::{Deserialize, Serialize};
use tracing::{debug, info};

use crate::domain::{Exit, Session, SessionId, Turn, Usage, WorkspaceId};
use crate::instance::Observed;
use crate::integration::delivery;
use crate::link;
use crate::link::credential::Secret;
use crate::log::{Entry, Message};
use crate::store::workspace::{PendingMessage, Taken};
use crate::store::{Store, Tx};
use crate::workspace;

const CREDENTIAL_LIFETIME: SignedDuration = SignedDuration::from_hours(12);

/// A supervisor cannot say it is alive while the control plane is not listening, so this
/// outlasts a restart under a live one by enough that an upgrade does not reap the Sessions it
/// was carrying; a dead supervisor holds a Workspace's active-Session slot until it is up.
const LEASE: SignedDuration = SignedDuration::from_mins(2);

/// The Secret is returned once, to be handed to the Session's supervisor as it starts; `Store`
/// keeps only its digest, so it cannot be recovered afterwards.
pub struct Claimed {
    pub session: Session,
    pub credential: Secret,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Report {
    Connected { version: String },
    Heartbeat,
    Stderr { lines: Vec<String> },
    Started,
    Model { model: String },
    Said { message: String },
    Used { usage: Usage },
    Answered,
    Checkout { repositories: Vec<Observed> },
    Finished { exit: Exit },
}

impl Report {
    const fn numbered(&self) -> bool {
        match self {
            Report::Connected { .. } | Report::Heartbeat | Report::Stderr { .. } => false,
            Report::Started
            | Report::Model { .. }
            | Report::Said { .. }
            | Report::Used { .. }
            | Report::Answered
            | Report::Checkout { .. }
            | Report::Finished { .. } => true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Reported {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seq: Option<i64>,
    #[serde(flatten)]
    pub report: Report,
}

#[derive(Debug)]
pub enum ReportRefused {
    MissingSequence,
    SkippedSequence(i64),
    Unavailable(anyhow::Error),
}

impl fmt::Display for ReportRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingSequence => {
                write!(
                    f,
                    "a report of this kind carries a seq, and this one carries none"
                )
            }
            Self::SkippedSequence(seq) => {
                write!(
                    f,
                    "the report {seq} skips one this session has yet to report"
                )
            }
            Self::Unavailable(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for ReportRefused {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Unavailable(error) => Some(error.as_ref()),
            _ => None,
        }
    }
}

impl From<anyhow::Error> for ReportRefused {
    fn from(error: anyhow::Error) -> Self {
        Self::Unavailable(error)
    }
}

pub async fn enqueue(
    store: &Store,
    workspace: WorkspaceId,
    model: Option<&str>,
) -> Result<Session> {
    let mut tx = store.begin().await?;
    let workspace = tx.workspaces().get(workspace).await?;
    workspace.accepts("session")?;

    if let Some(holding) = workspace::unfinished_session(&mut tx, &workspace)
        .await?
        .refuses_enqueue()
    {
        bail!(
            "the workspace {} already has the session {holding} in it, and a workspace has one at a time",
            workspace.id
        );
    }

    let session = tx.workspaces().enqueue_session(&workspace, model).await?;
    tx.commit().await?;

    Ok(session)
}

/// A queued Session is dispatched at most once: what this hands back is already active, so a
/// second claimant asking at the same moment is handed something else, or nothing.
pub async fn claim(store: &Store, serialized: &[String]) -> Result<Option<Claimed>> {
    let mut tx = store.begin().await?;
    let claimed = claiming(&mut tx, serialized, None).await?;
    tx.commit().await?;

    Ok(claimed)
}

pub enum Occupied {
    Claimed(Claimed),
    Resumed(Session),
}

/// A waiting Session holds no slot (ADR-0024), so a free one goes to whichever asked for it
/// first: a queued Session, or input held for a waiting Session.
pub async fn occupy(
    store: &Store,
    slots: usize,
    serialized: &[String],
) -> Result<Option<Occupied>> {
    let mut tx = store.begin().await?;
    if tx.workspaces().occupying_slots().await? >= slots {
        return Ok(None);
    }

    let held = tx.workspaces().oldest_held_input(serialized).await?;
    let occupied =
        match claiming(&mut tx, serialized, held.as_ref().map(|(_, since)| *since)).await? {
            Some(claimed) => Occupied::Claimed(claimed),
            None => {
                let Some((session, _)) = held else {
                    tx.commit().await?;
                    return Ok(None);
                };
                prompt_pending(&mut tx, &session).await?;
                Occupied::Resumed(tx.workspaces().session(session.id).await?)
            }
        };
    tx.commit().await?;

    Ok(Some(occupied))
}

async fn claiming(
    tx: &mut Tx<'_>,
    serialized: &[String],
    enqueued_before: Option<Timestamp>,
) -> Result<Option<Claimed>> {
    let claimable = tx
        .workspaces()
        .claimable_sessions(serialized, enqueued_before)
        .await?;
    for queued in claimable {
        let workspace = tx.workspaces().get(queued.workspace).await?;
        match crate::instance::admit(tx, &workspace).await? {
            crate::instance::Admission::Available => {
                let Some(session) = tx
                    .workspaces()
                    .claim_session(&queued, Timestamp::now() + LEASE)
                    .await?
                else {
                    continue;
                };
                let credential = Secret::mint();
                tx.workspaces()
                    .issue_credential(
                        &session,
                        &credential.digest(),
                        Timestamp::now() + CREDENTIAL_LIFETIME,
                    )
                    .await?;
                return Ok(Some(Claimed {
                    session,
                    credential,
                }));
            }
            crate::instance::Admission::Waiting(because) => {
                tx.workspaces().wait_for_instance(&queued, &because).await?;
            }
        }
    }

    Ok(None)
}

pub async fn session(store: &Store, id: SessionId) -> Result<Session> {
    store.begin().await?.workspaces().session(id).await
}

pub async fn resolve_session(
    store: &Store,
    organization: &str,
    reference: &str,
) -> Result<Session> {
    let mut tx = store.begin().await?;
    let organization = tx.organizations().named(organization).await?;

    tx.workspaces()
        .resolved_session(&organization, reference)
        .await
}

pub async fn sessions(store: &Store, workspace: WorkspaceId) -> Result<Vec<Session>> {
    let mut tx = store.begin().await?;
    let workspace = tx.workspaces().get(workspace).await?;

    tx.workspaces().sessions(&workspace).await
}

/// Report acceptance and its effects share one transaction so a failed append remains replayable
/// (ADR-0004).
pub async fn report(
    store: &Store,
    session: &Session,
    Reported { seq, report }: Reported,
) -> Result<(), ReportRefused> {
    let mut tx = store.begin().await?;

    if report.numbered() {
        let seq = seq.ok_or(ReportRefused::MissingSequence)?;
        match tx.workspaces().take_report(session, seq).await? {
            Taken::Next => {}
            Taken::Again => {
                debug!(session = %session.id, seq, "a supervisor reported something again");
                return Ok(());
            }
            Taken::Skipped => return Err(ReportRefused::SkippedSequence(seq)),
        }
    }

    match report {
        Report::Connected { version } => {
            tx.workspaces().record_connected(session, &version).await?;
            info!(session = %session.id, version, "a supervisor reported itself connected");
        }
        Report::Heartbeat => {
            tx.workspaces()
                .hold_lease(session, Timestamp::now() + LEASE)
                .await?;
            debug!(session = %session.id, "a supervisor reported itself alive");
        }
        Report::Stderr { lines } => {
            for line in lines {
                info!(session = %session.id, line, "its harness wrote to stderr");
            }
        }
        Report::Started => {
            if tx.workspaces().record_started(session).await? {
                let workspace = tx.workspaces().get(session.workspace).await?;
                tx.log()
                    .append(
                        &workspace,
                        Entry::SessionStarted {
                            session: session.id,
                        },
                    )
                    .await?;
            }
            info!(session = %session.id, "a supervisor reported its session started");
        }
        Report::Model { model } => {
            tx.workspaces().record_worked_model(session, &model).await?;
            info!(session = %session.id, model, "a supervisor reported the model its agent is on");
        }
        Report::Said { message } => {
            let workspace = tx.workspaces().get(session.workspace).await?;
            tx.log()
                .append(
                    &workspace,
                    Entry::Said {
                        participant: workspace.agent.name.clone(),
                        message,
                    },
                )
                .await?;
            info!(session = %session.id, "a supervisor reported what its agent said");
        }
        Report::Used { usage } => {
            info!(session = %session.id, %usage, "a supervisor reported what its agent used");
            tx.workspaces().record_usage(session, &usage).await?;
        }
        Report::Answered => {
            if let Some((turn, from_seq)) = tx.workspaces().answer_turn(session).await? {
                let workspace = tx.workspaces().get(session.workspace).await?;
                let said = tx
                    .log()
                    .said_since(&workspace, from_seq, &workspace.agent.name)
                    .await?;
                if !said.is_empty() {
                    delivery::record_turn(&mut tx, session, &workspace, turn, &said).await?;
                }
                tx.workspaces()
                    .record_active(session.workspace, Timestamp::now())
                    .await?;
            }
            info!(session = %session.id, "a supervisor reported its agent answered a turn");
        }
        Report::Checkout { repositories } => {
            tx.workspaces()
                .record_observed(session.workspace, &repositories)
                .await?;
            info!(session = %session.id, "a supervisor reported what its checkout holds");
        }
        Report::Finished { exit } => {
            let stands = ending(&mut tx, session, exit).await?;
            info!(session = %session.id, %stands, "a supervisor reported its session finished");
        }
    }
    tx.commit().await?;

    Ok(())
}

pub async fn instance(store: &Store, workspace: WorkspaceId) -> Result<Option<String>> {
    store.begin().await?.workspaces().instance(workspace).await
}

pub async fn executes_on(store: &Store, session: &Session, instance: &str) -> Result<()> {
    let mut tx = store.begin().await?;
    tx.workspaces()
        .record_instance(session.workspace, Some(instance))
        .await?;
    tx.workspaces()
        .record_session_instance(session, instance)
        .await?;

    tx.commit().await
}

/// Forgotten in the same breath as the Session fails, so no later Session is handed an Instance
/// nothing can resume, and none is handed a fresh one before this Session says what was lost.
pub async fn instance_lost(store: &Store, session: &Session, because: &str) -> Result<Exit> {
    let mut tx = store.begin().await?;
    tx.workspaces()
        .record_instance(session.workspace, None)
        .await?;
    let stands = ending(
        &mut tx,
        session,
        Exit::Failed {
            because: because.to_owned(),
        },
    )
    .await?;
    tx.commit().await?;

    Ok(stands)
}

pub async fn supervised(store: &Store, session: &Session, supervisor: &str) -> Result<()> {
    let mut tx = store.begin().await?;
    tx.workspaces()
        .record_supervisor(session, supervisor)
        .await?;

    tx.commit().await
}

pub async fn supervisors_to_stop(store: &Store) -> Result<Vec<(Session, String)>> {
    store
        .begin()
        .await?
        .workspaces()
        .supervisors_to_stop()
        .await
}

pub async fn supervisor_gone(store: &Store, session: &Session) -> Result<Option<Session>> {
    let mut tx = store.begin().await?;
    tx.workspaces().record_supervisor_gone(session).await?;
    let continued = continue_pending(&mut tx, session.workspace).await?;
    tx.commit().await?;

    Ok(continued)
}

pub async fn turns(store: &Store, session: SessionId) -> Result<Vec<Turn>> {
    store.begin().await?.workspaces().turns(session).await
}

/// A waiting Session has done everything asked of it, so stopping it there is how it
/// succeeds; stopping one mid-turn abandons what its agent was still doing.
pub async fn stop(store: &Store, id: SessionId) -> Result<Exit> {
    let mut tx = store.begin().await?;
    let session = tx.workspaces().session(id).await?;
    let Some(exit) = session.state.stop_exit() else {
        bail!("the session {id} has already ended");
    };
    let stands = stopping(&mut tx, &session, exit).await?;
    tx.commit().await?;

    Ok(stands)
}

/// Told as well as ended, so a supervisor leaves the link rather than dialling back in to be
/// refused.
pub(crate) async fn stopping(tx: &mut Tx<'_>, session: &Session, exit: Exit) -> Result<Exit> {
    tx.workspaces()
        .send_instruction(session, link::Instruction::Stop)
        .await?;

    ending(tx, session, exit).await
}

pub async fn complete(store: &Store, session: &Session) -> Result<Exit> {
    end(store, session, Exit::Succeeded).await
}

pub async fn fail(store: &Store, session: &Session, because: &str) -> Result<Exit> {
    end(
        store,
        session,
        Exit::Failed {
            because: because.to_owned(),
        },
    )
    .await
}

async fn end(store: &Store, session: &Session, exit: Exit) -> Result<Exit> {
    let mut tx = store.begin().await?;
    let stands = ending(&mut tx, session, exit).await?;
    tx.commit().await?;

    Ok(stands)
}

/// A Session ends once. Whoever gets there first — the supervisor reporting itself finished, the
/// claimant finding it gone, `timer` finding its lease expired — decides the exit status, and
/// what comes back is the one that stands.
pub(crate) async fn ending(tx: &mut Tx<'_>, session: &Session, exit: Exit) -> Result<Exit> {
    let workspace = tx.workspaces().get(session.workspace).await?;
    let said = tx.log().last_said_for_session(&workspace).await?;
    let stands = if tx
        .workspaces()
        .end_session(session, &exit, said.as_deref())
        .await?
    {
        tx.log()
            .append(
                &workspace,
                Entry::SessionEnded {
                    session: session.id,
                    exit: exit.clone(),
                },
            )
            .await?;
        tx.workspaces().invalidate_credentials(session).await?;
        delivery::record_outcome(tx, session, &workspace, &exit, said.as_deref()).await?;
        if let Exit::Failed { .. } = exit {
            cascade_unreachable(tx, session.id).await?;
        }
        if tx.workspaces().supervisor_is_gone(session).await? {
            continue_pending(tx, session.workspace).await?;
        }
        exit
    } else {
        tx.workspaces()
            .session(session.id)
            .await?
            .exit
            .context("a session that has ended has an exit status")?
    };

    Ok(stands)
}

/// Tolerance defaults to all-must-succeed, so a Session that failed makes every Session still
/// queued on it unreachable — and whatever was in turn waiting on one of those, since a blocker
/// that will never succeed cannot meet an all-must-succeed tolerance either. Never touches a
/// Session a claimant already took past this blocker before it failed.
async fn cascade_unreachable(tx: &mut Tx<'_>, blocker: SessionId) -> Result<()> {
    let mut newly_unreachable = vec![blocker];

    while let Some(blocker) = newly_unreachable.pop() {
        for dependent in tx.workspaces().dependents_of(blocker).await? {
            if tx.workspaces().mark_unreachable(&dependent).await? {
                newly_unreachable.push(dependent.id);
            }
        }
    }

    Ok(())
}

async fn continue_pending(tx: &mut Tx<'_>, workspace: WorkspaceId) -> Result<Option<Session>> {
    let workspace = tx.workspaces().get(workspace).await?;
    if workspace::unfinished_session(tx, &workspace)
        .await?
        .refuses_enqueue()
        .is_some()
    {
        return Ok(None);
    }

    let pending = tx.workspaces().take_pending_messages(&workspace).await?;
    if pending.is_empty() {
        return Ok(None);
    }

    tx.log()
        .append(
            &workspace,
            Entry::Messages {
                messages: messages(pending),
            },
        )
        .await?;

    Ok(Some(
        tx.workspaces().enqueue_session(&workspace, None).await?,
    ))
}

async fn prompt_pending(tx: &mut Tx<'_>, session: &Session) -> Result<()> {
    let workspace = tx.workspaces().get(session.workspace).await?;
    let pending = tx.workspaces().take_pending_messages(&workspace).await?;
    if pending.is_empty() {
        return Ok(());
    }

    let messages = messages(pending);
    let prompt = follow_up(&messages);
    tx.log()
        .append(&workspace, Entry::Messages { messages })
        .await?;

    link::prompt(tx, session, prompt).await
}

/// A lone message reaches the agent as written, so a skill invocation it leads with is still
/// recognised; several are attributed, so the agent can tell who asked for what.
pub(crate) fn follow_up(messages: &[Message]) -> String {
    match messages {
        [only] => only.message.clone(),
        several => several
            .iter()
            .map(|message| format!("{}: {}", message.participant, message.message))
            .collect::<Vec<_>>()
            .join("\n\n"),
    }
}

fn messages(pending: Vec<PendingMessage>) -> Vec<Message> {
    pending
        .into_iter()
        .map(|pending| Message {
            participant: pending.participant,
            message: pending.body,
        })
        .collect()
}

#[cfg(test)]
mod tests;
