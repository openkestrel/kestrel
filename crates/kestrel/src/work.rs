use std::fmt;

use anyhow::{Context as _, Result, bail};
use jiff::{SignedDuration, Timestamp};
use serde::{Deserialize, Serialize};
use tracing::{debug, info};

use crate::declined::Declined;
use crate::domain::{
    ChangingOption, Declared, Exit, HeldMessage, Session, SessionCommand, SessionId, SessionOption,
    SessionState, Turn, Usage, Workspace, WorkspaceId,
};
use crate::instance::Observed;
use crate::integration::delivery;
use crate::link;
use crate::live_work::{RunningTool, RunningUnit};
use crate::log::{ClosingReason, Completion, Entry, Message, PlanEntry, ToolStatus};
use crate::participant;
use crate::store::workspace::Taken;
use crate::store::{Store, Tx};
use crate::workspace;

/// A supervisor cannot say it is alive while the control plane is not listening, so this
/// outlasts a restart under a live one by enough that an upgrade does not reap the Sessions it
/// was carrying; a dead supervisor holds a Workspace's active-Session slot until it is up.
pub(crate) const LEASE: SignedDuration = SignedDuration::from_mins(2);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Report {
    Connected {
        version: String,
    },
    Heartbeat,
    Work {
        repositories: Vec<crate::live_work::Repository>,
    },
    Stderr {
        lines: Vec<String>,
    },
    Ready,
    Started,
    Model {
        model: String,
    },
    Said {
        message: String,
        completion: Completion,
    },
    Thought {
        text: String,
        completion: Completion,
    },
    Plan {
        entries: Vec<PlanEntry>,
        completion: Completion,
    },
    ToolCall {
        call_id: String,
        title: String,
        tool_kind: String,
        status: ToolStatus,
        input: serde_json::Value,
        result: Box<serde_json::Value>,
        closing_reason: Option<ClosingReason>,
        completion: Completion,
    },
    SessionState {
        tools: Vec<RunningTool>,
        units: Vec<RunningUnit>,
        message_buffering: bool,
        thought_buffering: bool,
        #[serde(default)]
        usage: Option<Usage>,
        #[serde(default)]
        last_activity_at: Option<Timestamp>,
    },
    /// Never a row: held in memory beside the running tools (ADR-0041).
    Usage {
        usage: Usage,
    },
    /// Idempotent and unnumbered, so a reconnect can say it again (ADR-0041).
    SessionInfo {
        #[serde(default)]
        title: Option<String>,
        #[serde(default)]
        options: Vec<SessionOption>,
        #[serde(default)]
        commands: Vec<SessionCommand>,
    },
    OptionChanged {
        participant: String,
        option: String,
        category: String,
        #[serde(default)]
        from: Option<String>,
        #[serde(default)]
        to: Option<String>,
        #[serde(default)]
        refused: Option<String>,
        #[serde(default)]
        options: Vec<SessionOption>,
    },
    Answered {
        #[serde(default)]
        usage: Option<Usage>,
    },
    /// `trailing` while units the agent started still run past the interrupt.
    Interrupted {
        #[serde(default)]
        trailing: bool,
    },
    Settled,
    Checkout {
        repositories: Vec<Observed>,
    },
    Finished {
        exit: Exit,
        #[serde(default)]
        usage: Option<Usage>,
    },
}

impl Report {
    const fn numbered(&self) -> bool {
        match self {
            Report::Connected { .. }
            | Report::Heartbeat
            | Report::Stderr { .. }
            | Report::Work { .. }
            | Report::Ready
            | Report::SessionState { .. }
            | Report::Usage { .. }
            | Report::SessionInfo { .. } => false,
            Report::Started
            | Report::Model { .. }
            | Report::Said { .. }
            | Report::Thought { .. }
            | Report::Plan { .. }
            | Report::ToolCall { .. }
            | Report::OptionChanged { .. }
            | Report::Answered { .. }
            | Report::Interrupted { .. }
            | Report::Settled
            | Report::Checkout { .. }
            | Report::Finished { .. } => true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Reported {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<SessionId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seq: Option<i64>,
    #[serde(flatten)]
    pub report: Report,
}

#[derive(Debug)]
pub enum ReportRefused {
    MissingSession,
    Gone(SessionId),
    MissingSequence,
    SkippedSequence(i64),
    Unavailable(anyhow::Error),
}

impl fmt::Display for ReportRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingSession => {
                write!(
                    f,
                    "a report of this kind names its session, and this one names none"
                )
            }
            Self::Gone(session) => write!(
                f,
                "the session {session} is not one this instance carries: it has ended, or its \
                 lease has passed and its control plane has let it go"
            ),
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
    agent: Option<&str>,
    declared: Declared,
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

    let named = match agent {
        Some(named) => Some(tx.agents().named(&workspace.organization, named).await?),
        None => None,
    };
    let session = tx
        .workspaces()
        .enqueue_session(&workspace, named.as_ref(), declared)
        .await?;
    tx.commit().await?;

    Ok(session)
}

pub async fn has_had_session(store: &Store, workspace: WorkspaceId) -> Result<bool> {
    store
        .read()
        .await?
        .workspaces()
        .has_had_session(workspace)
        .await
}

pub(crate) async fn awaiting_a_brief(tx: &mut Tx<'_>, workspace: &Workspace) -> Result<bool> {
    Ok(link::instruction(tx, workspace).await?.is_none())
}

pub async fn claim(store: &Store, serialized: &[String]) -> Result<Option<Session>> {
    crate::scheduling::claim(store, serialized).await
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

pub enum Named<'a> {
    Option(&'a str),
    Category(&'a str),
}

pub enum OptionRefusal {
    Phase(String),
    Unacceptable { field: &'static str, why: String },
    Missing(String),
    Named(anyhow::Error),
    Unavailable(anyhow::Error),
}

impl From<anyhow::Error> for OptionRefusal {
    fn from(error: anyhow::Error) -> Self {
        Self::Unavailable(error)
    }
}

pub struct OptionSet {
    pub session: Session,
    /// Waits on the harness (202) rather than setting a queued Session's declared value at once
    /// (200).
    pub live: bool,
}

pub async fn set_option(
    store: &Store,
    organization: &str,
    reference: &str,
    participant: &str,
    named: Named<'_>,
    value: &str,
) -> Result<OptionSet, OptionRefusal> {
    let mut tx = store.begin().await?;
    let organization = tx.organizations().named(organization).await?;
    let participant = match participant::accepted(&mut tx, &organization, participant).await {
        Ok(name) => name,
        Err(error) => return Err(OptionRefusal::Named(error)),
    };
    let session = tx
        .workspaces()
        .resolved_session(&organization, reference)
        .await?;

    let live = match session.state {
        SessionState::Queued => false,
        SessionState::Waiting | SessionState::Trailing => true,
        SessionState::Unbriefed if !session.options.is_empty() => true,
        phase => return Err(OptionRefusal::Phase(phase_refusal(phase, &session))),
    };

    if live {
        let option = match named {
            Named::Option(id) => session.options.iter().find(|option| option.id == id),
            Named::Category(category) => session
                .options
                .iter()
                .find(|option| option.is_category(category)),
        };
        let Some(option) = option else {
            return Err(OptionRefusal::Missing(format!(
                "the session {} offers no option {}",
                session.id,
                match named {
                    Named::Option(id) => id,
                    Named::Category(category) => category,
                }
            )));
        };
        let category = option.category.clone().unwrap_or_else(|| option.id.clone());
        if !offers(option, value) {
            return Err(OptionRefusal::Unacceptable {
                field: "value",
                why: format!("the {category} option does not offer {value}"),
            });
        }

        let changing = ChangingOption {
            option: option.id.clone(),
            category,
            value: value.to_owned(),
            participant: participant.clone(),
        };
        tx.workspaces()
            .add_changing_option(&session, &changing)
            .await?;
        tx.workspaces()
            .send_instruction(
                &session,
                link::Instruction::SetOption {
                    option: changing.option.clone(),
                    value: changing.value.clone(),
                    participant,
                },
            )
            .await?
            .ok_or_else(|| {
                OptionRefusal::Phase(format!(
                    "the session {} is on no instance to change",
                    session.id
                ))
            })?;
    } else {
        let category = match named {
            Named::Category(category) => {
                if !declares(category) {
                    return Err(OptionRefusal::Unacceptable {
                        field: "category",
                        why: format!(
                            "a queued session declares {}, {} or {}, not {category}",
                            SessionOption::MODEL,
                            SessionOption::MODE,
                            SessionOption::THOUGHT_LEVEL
                        ),
                    });
                }
                category
            }
            // A queued Session has reported no options, so only a category it declares is one.
            Named::Option(option) if declares(option) => option,
            Named::Option(option) => {
                return Err(OptionRefusal::Missing(format!(
                    "the queued session {} has no option {option}",
                    session.id
                )));
            }
        };
        let from = match category {
            SessionOption::MODEL => session.agent.declared.model.clone(),
            SessionOption::MODE => session.agent.declared.mode.clone(),
            _ => session.agent.declared.thought_level.clone(),
        };
        tx.workspaces()
            .set_declared_option(&session, category, value)
            .await?;
        let workspace = tx.workspaces().get(session.workspace).await?;
        tx.log()
            .append(
                &workspace,
                Entry::OptionChanged {
                    session: session.id,
                    participant,
                    option: category.to_owned(),
                    category: category.to_owned(),
                    from,
                    to: Some(value.to_owned()),
                    refused: None,
                },
            )
            .await?;
    }

    let session = tx.workspaces().session(session.id).await?;
    tx.commit().await?;

    Ok(OptionSet { session, live })
}

fn phase_refusal(phase: SessionState, session: &Session) -> String {
    match phase {
        SessionState::Working => format!(
            "the session {} is working, and an option cannot change mid-turn",
            session.id
        ),
        SessionState::Ended => format!("the session {} has ended", session.id),
        SessionState::Unreachable => format!("the session {} is unreachable", session.id),
        SessionState::Unbriefed => format!(
            "the session {} is unbriefed with no options yet",
            session.id
        ),
        SessionState::Queued | SessionState::Waiting | SessionState::Trailing => {
            format!("the session {} cannot change options", session.id)
        }
    }
}

fn declares(category: &str) -> bool {
    matches!(
        category,
        SessionOption::MODEL | SessionOption::MODE | SessionOption::THOUGHT_LEVEL
    )
}

fn offers(option: &SessionOption, value: &str) -> bool {
    match &option.kind {
        crate::domain::SessionOptionKind::Select { values, groups, .. } => {
            values.iter().any(|offered| offered.value == value)
                || groups
                    .iter()
                    .any(|group| group.values.iter().any(|offered| offered.value == value))
        }
        crate::domain::SessionOptionKind::Boolean { .. } => matches!(value, "true" | "false"),
    }
}

/// Report acceptance and its effects share one transaction so a failed append remains replayable
/// (ADR-0004).
pub async fn report(
    store: &Store,
    instance: &str,
    Reported {
        session,
        seq,
        report,
    }: Reported,
) -> Result<(), ReportRefused> {
    match report {
        Report::Work { .. } => return Ok(()),
        Report::Stderr { lines } => {
            for line in lines {
                info!(instance, line, "a harness wrote to stderr");
            }
            return Ok(());
        }
        Report::Connected { version } => {
            let mut tx = store.begin().await?;
            tx.workspaces().record_connected(instance, &version).await?;
            tx.commit().await?;
            info!(instance, version, "a supervisor reported itself connected");
            return Ok(());
        }
        Report::Heartbeat => {
            let mut tx = store.begin().await?;
            tx.workspaces().record_reached(instance).await?;
            tx.workspaces()
                .hold_leases_on(instance, Timestamp::now() + LEASE)
                .await?;
            tx.commit().await?;
            debug!(instance, "a supervisor reported itself alive");
            return Ok(());
        }
        _ => {}
    }

    let session = session.ok_or(ReportRefused::MissingSession)?;
    let mut tx = store.begin().await?;
    let session = tx
        .workspaces()
        .carried(instance, session)
        .await?
        .ok_or(ReportRefused::Gone(session))?;
    reported(&mut tx, &session, seq, report).await?;
    tx.commit().await?;

    Ok(())
}

pub async fn resume_trailing(store: &Store, session: &Session) -> Result<()> {
    let mut tx = store.begin().await?;
    resuming_trailing(&mut tx, session).await?;
    tx.commit().await
}

async fn resuming_trailing(tx: &mut Tx<'_>, session: &Session) -> Result<()> {
    if tx.workspaces().resume_trailing(session).await? {
        info!(session = %session.id, "a waiting session's agent is working again");
    }
    Ok(())
}

/// What a Session's own report does, reached here without asking which Instance carries it.
pub async fn report_on(
    store: &Store,
    session: &Session,
    seq: Option<i64>,
    report: Report,
) -> Result<(), ReportRefused> {
    let mut tx = store.begin().await?;
    reported(&mut tx, session, seq, report).await?;
    tx.commit().await?;

    Ok(())
}

async fn reported(
    tx: &mut Tx<'_>,
    session: &Session,
    seq: Option<i64>,
    report: Report,
) -> Result<(), ReportRefused> {
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

    if matches!(
        report,
        Report::Said { .. }
            | Report::Thought { .. }
            | Report::Plan { .. }
            | Report::ToolCall { .. }
    ) {
        resuming_trailing(tx, session).await?;
    }

    match report {
        Report::Connected { .. }
        | Report::Heartbeat
        | Report::Stderr { .. }
        | Report::Work { .. } => {
            unreachable!("a report about the instance is taken before one about its session")
        }
        Report::Ready => {
            if tx.workspaces().record_ready(session).await? {
                let workspace = tx.workspaces().get(session.workspace).await?;
                if workspace::first_held_becomes_the_brief(tx, &workspace).await? {
                    info!(session = %session.id, "the first held message became the Brief");
                }
                info!(session = %session.id, "a supervisor reported its harness ready");
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
                            agent: session.agent.name.clone(),
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
        Report::SessionInfo {
            title,
            options,
            commands,
        } => {
            let written = tx
                .workspaces()
                .record_session_info(session, title.as_deref(), &options, &commands)
                .await?;
            info!(
                session = %session.id,
                title = title.as_deref().unwrap_or_default(),
                options = options.len(),
                commands = commands.len(),
                written,
                "a supervisor reported what its session holds"
            );
        }
        Report::OptionChanged {
            participant,
            option,
            category,
            from,
            to,
            refused,
            options,
        } => {
            let taken = tx
                .workspaces()
                .take_changing_option(session, &option, &participant)
                .await?;
            if !options.is_empty() {
                tx.workspaces().record_options(session, &options).await?;
            }
            let workspace = tx.workspaces().get(session.workspace).await?;
            tx.log()
                .append(
                    &workspace,
                    Entry::OptionChanged {
                        session: session.id,
                        participant,
                        option,
                        category,
                        from,
                        to,
                        refused,
                    },
                )
                .await?;
            info!(
                session = %session.id,
                settled = taken.is_some(),
                "a supervisor answered a session's option change"
            );
        }
        Report::Said {
            message,
            completion,
        } => {
            let workspace = tx.workspaces().get(session.workspace).await?;
            tx.log()
                .append(
                    &workspace,
                    Entry::Said {
                        participant: session.agent.name.clone(),
                        message,
                        session_id: Some(session.id),
                        completion: Some(completion),
                    },
                )
                .await?;
            info!(session = %session.id, "a supervisor reported what its agent said");
        }
        Report::Thought { text, completion } => {
            let workspace = tx.workspaces().get(session.workspace).await?;
            tx.log()
                .append(
                    &workspace,
                    Entry::Thought {
                        session_id: session.id,
                        text,
                        completion,
                    },
                )
                .await?;
        }
        Report::Plan {
            entries,
            completion,
        } => {
            let workspace = tx.workspaces().get(session.workspace).await?;
            tx.log()
                .append(
                    &workspace,
                    Entry::Plan {
                        session_id: session.id,
                        entries,
                        completion,
                    },
                )
                .await?;
        }
        Report::SessionState { .. } => {}
        Report::Usage { .. } => {}
        Report::ToolCall {
            call_id,
            title,
            tool_kind,
            status,
            input,
            result,
            closing_reason,
            completion,
        } => {
            let workspace = tx.workspaces().get(session.workspace).await?;
            tx.log()
                .append(
                    &workspace,
                    Entry::ToolCall {
                        session_id: session.id,
                        call_id,
                        title,
                        tool_kind,
                        status,
                        input,
                        result,
                        closing_reason,
                        completion,
                    },
                )
                .await?;
        }
        Report::Answered { usage } => {
            if let Some((turn, from_seq)) = tx
                .workspaces()
                .answer_turn(session, SessionState::Trailing)
                .await?
            {
                let workspace = tx.workspaces().get(session.workspace).await?;
                let said = tx
                    .log()
                    .said_since(&workspace, from_seq, &session.agent.name)
                    .await?;
                if !said.is_empty() {
                    delivery::record_turn(tx, session, &workspace, turn, &said).await?;
                }
                tx.workspaces()
                    .record_active(session.organization, session.workspace, Timestamp::now())
                    .await?;
                if let Some(usage) = usage {
                    info!(session = %session.id, %usage, "a supervisor reported what its agent used");
                    tx.workspaces().record_usage(session, &usage).await?;
                }
                prompt_pending(tx, session).await?;
            }
            info!(session = %session.id, "a supervisor reported its agent answered a turn");
        }
        Report::Interrupted { trailing } => {
            let workspace = tx.workspaces().get(session.workspace).await?;
            if let Some(interrupting) = tx.workspaces().take_interrupting(session).await? {
                tx.log()
                    .append(
                        &workspace,
                        Entry::TurnInterrupted {
                            session: session.id,
                            participant: interrupting.participant,
                        },
                    )
                    .await?;
            }
            let then = if trailing {
                SessionState::Trailing
            } else {
                SessionState::Waiting
            };
            if tx.workspaces().answer_turn(session, then).await?.is_some() {
                tx.workspaces()
                    .record_active(session.organization, session.workspace, Timestamp::now())
                    .await?;
            }
            // The slot the interrupted Turn held goes straight to what is held for it, so the
            // Workspace never waits between the two.
            prompt_pending(tx, session).await?;
            info!(session = %session.id, "a supervisor reported its agent's turn interrupted");
        }
        Report::Settled => {
            if tx.workspaces().settle(session).await? {
                tx.workspaces()
                    .record_active(session.organization, session.workspace, Timestamp::now())
                    .await?;
            }
            info!(session = %session.id, "a supervisor reported its agent's work settled");
        }
        Report::Checkout { repositories } => {
            tx.workspaces()
                .record_observed(session.organization, session.workspace, &repositories)
                .await?;
            tx.workspaces().record_checked_out(session).await?;
            info!(session = %session.id, "a supervisor reported what its checkout holds");
        }
        Report::Finished { exit, usage } => {
            let stands = ending(tx, session, exit).await?;
            if let Some(usage) = usage {
                info!(session = %session.id, %usage, "a supervisor reported what its agent used");
                tx.workspaces().record_usage(session, &usage).await?;
            }
            info!(session = %session.id, %stands, "a supervisor reported its session finished");
        }
    }

    Ok(())
}

pub async fn instance(store: &Store, workspace: WorkspaceId) -> Result<Option<String>> {
    store.begin().await?.workspaces().instance(workspace).await
}

pub async fn executes_on(store: &Store, session: &Session, instance: &str) -> Result<()> {
    let mut tx = store.begin().await?;
    tx.workspaces()
        .record_instance(session.organization, session.workspace, Some(instance))
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
    if let Some(instance) = tx.workspaces().instance(session.workspace).await? {
        tx.workspaces().forget_supervisor(&instance).await?;
    }
    tx.workspaces()
        .record_instance(session.organization, session.workspace, None)
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

pub async fn supervised(
    store: &Store,
    session: &Session,
    instance: &str,
    supervisor: Option<&str>,
) -> Result<()> {
    let mut tx = store.begin().await?;
    tx.workspaces()
        .record_supervisor(session, instance, supervisor)
        .await?;

    tx.commit().await
}

pub(crate) fn expired_lease() -> Exit {
    Exit::Failed {
        because: "the supervisor stopped holding the session's lease out, and it expired"
            .to_owned(),
    }
}

/// Forgotten with the Sessions it was carrying, so the next Session on the Instance starts
/// another.
///
/// A lease that lapsed first decides the reason, so one lapse reads the same whether the sweep or
/// the supervisor's exit gets to the Session first.
pub async fn supervisor_exited(store: &Store, instance: &str, because: &str) -> Result<()> {
    let mut tx = store.begin().await?;
    tx.workspaces().forget_supervisor(instance).await?;
    let now = Timestamp::now();
    for session in tx.workspaces().live_sessions_on(instance).await? {
        let exit = if session.lease_expires_at.is_some_and(|at| at <= now) {
            expired_lease()
        } else {
            Exit::Failed {
                because: because.to_owned(),
            }
        };
        ending(&mut tx, &session, exit).await?;
    }

    tx.commit().await
}

pub async fn turns(store: &Store, session: SessionId) -> Result<Vec<Turn>> {
    store.begin().await?.workspaces().turns(session).await
}

/// Anyone who can post can interrupt; a second request while one is pending sends nothing more.
pub async fn interrupt(store: &Store, id: SessionId, participant: &str) -> Result<Session> {
    let mut tx = store.begin().await?;
    let session = tx.workspaces().session(id).await?;
    let organization = tx.organizations().by_id(session.organization).await?;
    let participant = participant::accepted(&mut tx, &organization, participant).await?;

    match session.state {
        SessionState::Working => {}
        SessionState::Trailing => {
            return Err(Declined::Taken(format!(
                "the session {id} is trailing: its turn has already answered, \
                 so stop the session to end the work its agent is still doing"
            ))
            .into());
        }
        phase => {
            return Err(Declined::Taken(format!(
                "the session {id} is {phase}, and only a working turn can be interrupted"
            ))
            .into());
        }
    }
    if tx
        .workspaces()
        .request_interrupt(&session, &participant)
        .await?
    {
        let turn = tx
            .workspaces()
            .unanswered_turn(&session)
            .await?
            .ok_or_else(|| anyhow::anyhow!("the working session {id} has no unanswered turn"))?;
        tx.workspaces()
            .send_instruction(&session, link::Instruction::Interrupt { turn })
            .await?;
    }
    let interrupting = tx.workspaces().session(id).await?;
    tx.commit().await?;

    Ok(interrupting)
}

/// A waiting Session has done everything asked of it, so stopping it there is how it
/// succeeds; stopping one mid-turn abandons what its agent was still doing.
pub async fn stop(store: &Store, id: SessionId, running: &[RunningTool]) -> Result<Exit> {
    let mut tx = store.begin().await?;
    let session = tx.workspaces().session(id).await?;
    let Some(exit) = session.state.stop_exit() else {
        bail!("the session {id} has already ended");
    };
    // The Session has ended before its supervisor hears the stop, so no report closes these.
    let workspace = tx.workspaces().get(session.workspace).await?;
    let now = Timestamp::now();
    for tool in running {
        if tx
            .log()
            .has_tool_call(&workspace, session.id, &tool.call_id)
            .await?
        {
            continue;
        }
        tx.log()
            .append(
                &workspace,
                Entry::ToolCall {
                    session_id: session.id,
                    call_id: tool.call_id.clone(),
                    title: tool.title.clone(),
                    tool_kind: tool.tool_kind.clone(),
                    status: tool.status,
                    input: serde_json::Value::Null,
                    result: Box::new(serde_json::Value::Null),
                    closing_reason: Some(ClosingReason::Interrupted),
                    completion: Completion {
                        started_at: tool.started_at,
                        finished_at: now,
                        turn_outcome: None,
                    },
                },
            )
            .await?;
    }
    let stands = ending(&mut tx, &session, exit).await?;
    tx.commit().await?;

    Ok(stands)
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
/// what comes back is the one that stands. Its Instance's supervisor is told, so the next
/// Session's `start` always follows this one's `stop` down the stream.
pub(crate) async fn ending(tx: &mut Tx<'_>, session: &Session, exit: Exit) -> Result<Exit> {
    let workspace = tx.workspaces().get(session.workspace).await?;
    let said = tx
        .log()
        .last_said_for_session(&workspace, &session.agent.name)
        .await?;
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
        tx.workspaces()
            .send_instruction(session, link::Instruction::Stop)
            .await?;
        delivery::record_outcome(tx, session, &workspace, &exit, said.as_deref()).await?;
        if let Exit::Failed { .. } = exit {
            cascade_unreachable(tx, session.id).await?;
        }
        continue_pending(tx, session.workspace).await?;
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

    if let Some(pending) = tx.workspaces().take_pending_session(&workspace).await? {
        return Ok(Some(workspace::briefed(tx, &workspace, pending).await?));
    }
    // The Session that just ended is the last one to have reached a harness, so its commands
    // decide which message is one; the Session about to start has none yet.
    let commands = tx.workspaces().latest_commands(workspace.id).await?;
    let pending = tx
        .workspaces()
        .take_held_messages(&workspace, &commands)
        .await?;
    if pending.is_empty() {
        return Ok(None);
    }

    let messages = messages(pending);
    workspace::join_authors(tx, &workspace, &messages).await?;
    tx.log()
        .append(&workspace, Entry::Messages { messages })
        .await?;

    Ok(Some(
        tx.workspaces()
            .enqueue_session(&workspace, None, Declared::default())
            .await?,
    ))
}

pub(crate) async fn prompt_pending(tx: &mut Tx<'_>, session: &Session) -> Result<()> {
    let workspace = tx.workspaces().get(session.workspace).await?;
    let pending = tx
        .workspaces()
        .take_held_messages(&workspace, &session.commands)
        .await?;
    if pending.is_empty() {
        return Ok(());
    }

    let messages = messages(pending);
    workspace::join_authors(tx, &workspace, &messages).await?;
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

fn messages(held: Vec<HeldMessage>) -> Vec<Message> {
    held.into_iter()
        .map(|held| Message {
            participant: held.participant,
            message: held.message,
        })
        .collect()
}

#[cfg(test)]
mod tests;
