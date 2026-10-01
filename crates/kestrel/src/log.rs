use std::fmt;
use std::str::FromStr;

use anyhow::{Context as _, Result, bail};
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqliteConnection};

use crate::domain::{
    EventRecordId, Exit, PullRequestState, SessionId, Workspace, WorkspaceId, WorkspaceState,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Entry {
    ParticipantJoined {
        participant: String,
    },
    Brief {
        trigger: Option<String>,
        brief: String,
    },
    SessionStarted {
        session: SessionId,
        agent: String,
    },
    Said {
        participant: String,
        message: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        session_id: Option<SessionId>,
        #[serde(skip_serializing_if = "Option::is_none")]
        completion: Option<Completion>,
    },
    Thought {
        session_id: SessionId,
        text: String,
        completion: Completion,
    },
    Plan {
        session_id: SessionId,
        entries: Vec<PlanEntry>,
        completion: Completion,
    },
    ToolCall {
        session_id: SessionId,
        call_id: String,
        title: String,
        tool_kind: String,
        status: String,
        input: serde_json::Value,
        result: serde_json::Value,
        closing_reason: Option<String>,
        completion: Completion,
    },
    Messages {
        messages: Vec<Message>,
    },
    SessionEnded {
        session: SessionId,
        exit: Exit,
    },
    InstanceReleased {
        participant: String,
        instance: String,
        unpublished: Option<String>,
    },
    PullRequest {
        event: EventRecordId,
        repository: String,
        number: i64,
        url: String,
        title: String,
        action: String,
        state: PullRequestState,
    },
}

impl fmt::Display for Entry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Entry::ParticipantJoined { participant } => {
                write!(f, "participant joined  {participant}")
            }
            Entry::Brief {
                trigger: Some(trigger),
                brief,
            } => write!(f, "brief  {trigger}  {brief}"),
            Entry::Brief {
                trigger: None,
                brief,
            } => write!(f, "brief  {brief}"),
            Entry::SessionStarted { session, agent } => {
                write!(f, "session started  {session}  {agent}")
            }
            Entry::Said {
                participant,
                message,
                ..
            } => write!(f, "said  {participant}  {message}"),
            Entry::Thought { text, .. } => write!(f, "thought  {text}"),
            Entry::Plan { entries, .. } => write!(
                f,
                "plan  {}",
                entries
                    .iter()
                    .map(|entry| entry.content.as_str())
                    .collect::<Vec<_>>()
                    .join("  ")
            ),
            Entry::ToolCall { title, status, .. } => write!(f, "tool call  {title}  {status}"),
            Entry::Messages { messages } => write!(
                f,
                "messages  {}",
                messages
                    .iter()
                    .map(|message| format!("{}  {}", message.participant, message.message))
                    .collect::<Vec<_>>()
                    .join("  ")
            ),
            Entry::SessionEnded { session, exit } => write!(f, "session ended  {session}  {exit}"),
            Entry::InstanceReleased {
                participant,
                instance,
                unpublished,
            } => {
                write!(f, "instance released  {participant}  {instance}")?;
                match unpublished {
                    Some(unpublished) => write!(f, "  discarding {unpublished}"),
                    None => Ok(()),
                }
            }
            Entry::PullRequest {
                url, action, state, ..
            } => write!(f, "pull request {action}  {url}  {}", state.as_str()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    pub participant: String,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct TranscriptEntry {
    pub kind: Kind,
    pub session_id: Option<SessionId>,
    pub seq: i64,
    pub appended_at: Timestamp,
    pub entry: Entry,
}

pub struct Log<'a> {
    connection: &'a mut SqliteConnection,
}

impl<'a> Log<'a> {
    pub(crate) fn over(connection: &'a mut SqliteConnection) -> Self {
        Self { connection }
    }

    /// The state is read in the statement that appends rather than off the `Workspace` handed
    /// in, so one sealed after the caller read it refuses all the same.
    pub async fn append(&mut self, workspace: &Workspace, entry: Entry) -> Result<TranscriptEntry> {
        let appended_at = Timestamp::now();

        let appended = sqlx::query(
            "INSERT INTO transcript_entry (workspace_id, organization_id, seq, body, appended_at, kind, session_id)
             SELECT
                 ?,
                 ?,
                 (SELECT COALESCE(MAX(seq), 0) + 1 FROM transcript_entry WHERE workspace_id = ?),
                 ?,
                 ?,
                 ?,
                 ?
             WHERE EXISTS (SELECT 1 FROM workspace WHERE id = ? AND state = ?)
             RETURNING seq",
        )
        .bind(workspace.id.to_string())
        .bind(workspace.organization.id.to_string())
        .bind(workspace.id.to_string())
        .bind(serde_json::to_string(&entry)?)
        .bind(appended_at.to_string())
        .bind(entry.kind().as_str())
        .bind(entry.session_id().map(|id| id.to_string()))
        .bind(workspace.id.to_string())
        .bind(WorkspaceState::Open.as_str())
        .fetch_optional(&mut *self.connection)
        .await
        .with_context(|| format!("appending to the transcript of workspace {}", workspace.id))?
        .with_context(|| {
            format!(
                "the workspace {} is sealed, and accepts no new transcript entry",
                workspace.id
            )
        })?;

        Ok(TranscriptEntry {
            kind: entry.kind(),
            session_id: entry.session_id(),
            seq: appended.get("seq"),
            appended_at,
            entry,
        })
    }

    /// Whether a name has taken a turn here, answered from the Transcript and never a set kept
    /// beside it.
    pub async fn has_joined(&mut self, workspace: &Workspace, participant: &str) -> Result<bool> {
        let joined = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS (
                 SELECT 1 FROM transcript_entry
                 WHERE workspace_id = ?
                   AND json_extract(body, '$.type') = 'participant_joined'
                   AND json_extract(body, '$.participant') = ?
             )",
        )
        .bind(workspace.id.to_string())
        .bind(participant)
        .fetch_one(&mut *self.connection)
        .await
        .with_context(|| format!("reading the participants of workspace {}", workspace.id))?;

        Ok(joined)
    }

    pub async fn last_said_for_session(
        &mut self,
        workspace: &Workspace,
        participant: &str,
    ) -> Result<Option<String>> {
        let latest = sqlx::query(
            "SELECT body
             FROM transcript_entry
             WHERE workspace_id = ? AND json_extract(body, '$.type') = 'said'
               AND json_extract(body, '$.participant') = ?
               AND seq > COALESCE((
                   SELECT MAX(seq) FROM transcript_entry
                   WHERE workspace_id = ? AND json_extract(body, '$.type') = 'session_ended'
               ), 0)
             ORDER BY seq DESC
             LIMIT 1",
        )
        .bind(workspace.id.to_string())
        .bind(participant)
        .bind(workspace.id.to_string())
        .fetch_optional(&mut *self.connection)
        .await
        .with_context(|| format!("reading the transcript of workspace {}", workspace.id))?;

        let Some(row) = latest else {
            return Ok(None);
        };

        Ok(match serde_json::from_str(row.get("body"))? {
            Entry::Said {
                participant: said_by,
                message,
                ..
            } if said_by == participant => Some(message),
            _ => None,
        })
    }

    /// What one participant said after a Turn was prompted, oldest first, which is that Turn's
    /// response to report.
    pub async fn said_since(
        &mut self,
        workspace: &Workspace,
        seq: i64,
        participant: &str,
    ) -> Result<Vec<String>> {
        let rows = sqlx::query(
            "SELECT body
             FROM transcript_entry
             WHERE workspace_id = ? AND seq > ?
               AND json_extract(body, '$.type') = 'said'
             ORDER BY seq",
        )
        .bind(workspace.id.to_string())
        .bind(seq)
        .fetch_all(&mut *self.connection)
        .await
        .with_context(|| format!("reading what was said in workspace {}", workspace.id))?;

        let mut said = Vec::new();
        for row in rows {
            if let Entry::Said {
                participant: who,
                message,
                ..
            } = serde_json::from_str(row.get("body"))?
                && who == participant
            {
                said.push(message);
            }
        }

        Ok(said)
    }

    /// The Brief that started this Session, if nothing has said anything since it: participants
    /// joining, Sessions starting and pull requests learned are not something said. Bounded to what the Transcript holds
    /// since the Workspace's last Session ended, or since it opened if none has, so a Brief that
    /// started an earlier Session is not mistaken for one starting this one.
    pub async fn unfollowed_brief(&mut self, workspace: &Workspace) -> Result<Option<String>> {
        let said = sqlx::query(
            "SELECT body
             FROM transcript_entry
             WHERE workspace_id = ?
               AND kind = 'shared_state'
               AND json_extract(body, '$.type')
                   NOT IN ('participant_joined', 'session_started', 'pull_request')
               AND seq > COALESCE((
                   SELECT MAX(seq) FROM transcript_entry
                   WHERE workspace_id = ? AND json_extract(body, '$.type') = 'session_ended'
               ), 0)
             ORDER BY seq
             LIMIT 2",
        )
        .bind(workspace.id.to_string())
        .bind(workspace.id.to_string())
        .fetch_all(&mut *self.connection)
        .await
        .with_context(|| format!("reading the transcript of workspace {}", workspace.id))?;

        let [only] = said.as_slice() else {
            return Ok(None);
        };
        Ok(match serde_json::from_str(only.get("body"))? {
            Entry::Brief { brief, .. } => Some(brief),
            _ => None,
        })
    }

    /// Every entry the Transcript holds before the messages starting the Session about to begin,
    /// oldest first: a Brief or an earlier Session's history that a message which followed it
    /// reads as context rather than as its own instruction.
    pub async fn context_before_starting(&mut self, workspace: &Workspace) -> Result<Vec<Entry>> {
        let rows = sqlx::query(
            "SELECT body
             FROM transcript_entry
             WHERE workspace_id = ?
               AND kind = 'shared_state'
               AND json_extract(body, '$.type') != 'participant_joined'
               AND seq < (
                   SELECT MIN(seq) FROM transcript_entry
                   WHERE workspace_id = ?
                     AND json_extract(body, '$.type') IN ('said', 'messages')
                     AND seq > COALESCE((
                         SELECT MAX(seq) FROM transcript_entry
                         WHERE workspace_id = ? AND json_extract(body, '$.type') = 'session_ended'
                     ), 0)
               )
             ORDER BY seq",
        )
        .bind(workspace.id.to_string())
        .bind(workspace.id.to_string())
        .bind(workspace.id.to_string())
        .fetch_all(&mut *self.connection)
        .await
        .with_context(|| format!("reading the transcript of workspace {}", workspace.id))?;

        rows.iter()
            .map(|row| Ok(serde_json::from_str(row.get("body"))?))
            .collect()
    }

    /// The Said and Messages entries the Transcript holds since the Workspace's last Session
    /// ended, or since it opened if none has, flattened into the messages that make them up and
    /// ordered oldest first: the message or messages that started the Session about to begin.
    pub async fn starting_messages(&mut self, workspace: &Workspace) -> Result<Vec<Message>> {
        let rows = sqlx::query(
            "SELECT body
             FROM transcript_entry
             WHERE workspace_id = ?
               AND json_extract(body, '$.type') IN ('said', 'messages')
               AND seq > COALESCE((
                   SELECT MAX(seq) FROM transcript_entry
                   WHERE workspace_id = ? AND json_extract(body, '$.type') = 'session_ended'
               ), 0)
             ORDER BY seq",
        )
        .bind(workspace.id.to_string())
        .bind(workspace.id.to_string())
        .fetch_all(&mut *self.connection)
        .await
        .with_context(|| format!("reading the transcript of workspace {}", workspace.id))?;

        let mut messages = Vec::new();
        for row in rows {
            match serde_json::from_str(row.get("body"))? {
                Entry::Said {
                    participant,
                    message,
                    ..
                } => messages.push(Message {
                    participant,
                    message,
                }),
                Entry::Messages { messages: mut said } => messages.append(&mut said),
                _ => {}
            }
        }

        Ok(messages)
    }

    pub async fn page(
        &mut self,
        workspace: &Workspace,
        from: Option<Cursor>,
        window: Window,
        kinds: &Kinds,
    ) -> Result<Page, Unreadable> {
        let from = self.position(workspace, from).await?;

        Ok(self.after(workspace, from, window, kinds).await?)
    }

    async fn position(
        &mut self,
        workspace: &Workspace,
        cursor: Option<Cursor>,
    ) -> Result<Option<Cursor>, Unreadable> {
        let Some(cursor) = cursor else {
            return Ok(None);
        };

        if cursor.workspace != workspace.id {
            return Err(Unreadable::Cursor(format!(
                "the cursor {cursor} walks another transcript"
            )));
        }

        let known =
            sqlx::query("SELECT seq FROM transcript_entry WHERE workspace_id = ? AND seq = ?")
                .bind(workspace.id.to_string())
                .bind(cursor.seq)
                .fetch_optional(&mut *self.connection)
                .await?;

        match known {
            Some(_) => Ok(Some(cursor)),
            None => Err(Unreadable::Cursor(format!(
                "the cursor {cursor} is no position in this transcript"
            ))),
        }
    }

    /// One entry beyond the window is read and dropped, which is what tells a reader whether
    /// more are waiting without asking a second time.
    async fn after(
        &mut self,
        workspace: &Workspace,
        from: Option<Cursor>,
        window: Window,
        kinds: &Kinds,
    ) -> Result<Page> {
        let rows = sqlx::query(
            "SELECT seq, body, appended_at, kind, session_id
             FROM transcript_entry
             WHERE workspace_id = ? AND seq > ?
             ORDER BY seq
             LIMIT ?",
        )
        .bind(workspace.id.to_string())
        .bind(from.map_or(0, |cursor| cursor.seq))
        .bind(i64::try_from(window.0 + 1)?)
        .fetch_all(&mut *self.connection)
        .await
        .with_context(|| format!("reading the transcript of workspace {}", workspace.id))?;

        let more = rows.len() > window.0;
        let entries = rows
            .iter()
            .take(window.0)
            .map(|row| {
                Ok(TranscriptEntry {
                    kind: row.get::<String, _>("kind").parse()?,
                    session_id: row
                        .get::<Option<String>, _>("session_id")
                        .map(|id| id.parse())
                        .transpose()?,
                    seq: row.get("seq"),
                    appended_at: row.get::<String, _>("appended_at").parse()?,
                    entry: serde_json::from_str(row.get("body"))?,
                })
            })
            .collect::<Result<Vec<_>>>()?;

        let cursor = entries
            .last()
            .map(|entry| Cursor {
                workspace: workspace.id,
                seq: entry.seq,
            })
            .or(from);
        let entries = entries
            .into_iter()
            .filter(|entry| kinds.contains(entry.kind))
            .collect();
        Ok(Page {
            cursor,
            entries,
            more,
        })
    }
}

pub struct Page {
    pub entries: Vec<TranscriptEntry>,
    pub cursor: Option<Cursor>,
    pub more: bool,
}

/// A position in one Transcript rather than a handle the control plane holds open, so it
/// still walks after the process that issued it is gone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cursor {
    workspace: WorkspaceId,
    seq: i64,
}

impl Cursor {
    pub const fn at(workspace: WorkspaceId, seq: i64) -> Self {
        Self { workspace, seq }
    }
}

impl fmt::Display for Cursor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.workspace, self.seq)
    }
}

impl FromStr for Cursor {
    type Err = anyhow::Error;

    fn from_str(text: &str) -> Result<Self> {
        let unreadable = || format!("{text} is no cursor");
        let (workspace, seq) = text.split_once(':').with_context(unreadable)?;

        Ok(Self {
            workspace: workspace.parse().with_context(unreadable)?,
            seq: seq.parse().with_context(unreadable)?,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Window(usize);

impl Window {
    pub const DEFAULT: Self = Self(100);
    const MOST: usize = 500;

    pub fn or_default(entries: Option<usize>) -> Result<Self> {
        entries.map_or(Ok(Self::DEFAULT), Self::of)
    }

    pub fn of(entries: usize) -> Result<Self> {
        if entries == 0 || entries > Self::MOST {
            bail!("a window is 1 to {} entries, not {entries}", Self::MOST);
        }

        Ok(Self(entries))
    }
}

#[derive(Debug)]
pub enum Unreadable {
    /// Resuming from the beginning instead would hand the reader what it has already walked,
    /// as though it were new.
    Cursor(String),
    Unavailable(anyhow::Error),
}

impl fmt::Display for Unreadable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Unreadable::Cursor(why) => f.write_str(why),
            Unreadable::Unavailable(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for Unreadable {}

impl From<anyhow::Error> for Unreadable {
    fn from(error: anyhow::Error) -> Self {
        Unreadable::Unavailable(error)
    }
}

impl From<sqlx::Error> for Unreadable {
    fn from(error: sqlx::Error) -> Self {
        Unreadable::Unavailable(error.into())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    SharedState,
    Narration,
    Detail,
}

impl Kind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SharedState => "shared_state",
            Self::Narration => "narration",
            Self::Detail => "detail",
        }
    }
}
impl FromStr for Kind {
    type Err = anyhow::Error;
    fn from_str(value: &str) -> Result<Self> {
        match value {
            "shared_state" => Ok(Self::SharedState),
            "narration" => Ok(Self::Narration),
            "detail" => Ok(Self::Detail),
            _ => bail!("{value} is no transcript kind"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Kinds(Vec<Kind>);
impl Default for Kinds {
    fn default() -> Self {
        Self(vec![Kind::SharedState])
    }
}
impl FromStr for Kinds {
    type Err = anyhow::Error;
    fn from_str(value: &str) -> Result<Self> {
        Ok(Self(
            value.split(',').map(str::parse).collect::<Result<_>>()?,
        ))
    }
}
impl Kinds {
    pub fn contains(&self, kind: Kind) -> bool {
        self.0.contains(&kind)
    }
}
impl Entry {
    pub const fn kind(&self) -> Kind {
        match self {
            Self::Thought { .. } | Self::Plan { .. } => Kind::Narration,
            Self::ToolCall { .. } => Kind::Detail,
            _ => Kind::SharedState,
        }
    }
    pub const fn session_id(&self) -> Option<SessionId> {
        match self {
            Self::Said { session_id, .. } => *session_id,
            Self::Thought { session_id, .. }
            | Self::Plan { session_id, .. }
            | Self::ToolCall { session_id, .. } => Some(*session_id),
            Self::SessionStarted { session, .. } | Self::SessionEnded { session, .. } => {
                Some(*session)
            }
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Completion {
    pub started_at: jiff::Timestamp,
    pub finished_at: jiff::Timestamp,
    pub turn_outcome: Option<TurnOutcome>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanEntry {
    pub content: String,
    pub priority: String,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum TurnOutcome {
    Answered { stop_reason: String },
    Cancelled,
    Failed { because: String },
}

impl Completion {
    pub fn at(now: jiff::Timestamp) -> Self {
        Self {
            started_at: now,
            finished_at: now,
            turn_outcome: None,
        }
    }
}
