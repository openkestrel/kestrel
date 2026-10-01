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
    Expired {
        expired_at: Timestamp,
    },
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
        result: Box<serde_json::Value>,
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
    TurnInterrupted {
        session: SessionId,
        participant: String,
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
            Entry::Expired { expired_at } => write!(f, "expired  {expired_at}"),
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
            Entry::TurnInterrupted {
                session,
                participant,
            } => write!(f, "turn interrupted  {session}  {participant}"),
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
        let kind = entry
            .kind()
            .context("expiry replaces an entry in place, never appends one")?;
        let appended_at = Timestamp::now();
        let seq: i64 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(seq), 0) + 1 FROM transcript_entry WHERE workspace_id = ?",
        )
        .bind(workspace.id.to_string())
        .fetch_one(&mut *self.connection)
        .await?;
        let mut body = serde_json::to_value(&entry)?;
        let payloads = externalize(workspace.id, seq, &mut body)?;

        let appended = sqlx::query(
            "INSERT INTO transcript_entry (workspace_id, organization_id, seq, body, appended_at, kind, session_id)
             SELECT
                 ?,
                 ?,
                 ?,
                 ?,
                 ?,
                 ?,
                 ?
             WHERE EXISTS (SELECT 1 FROM workspace WHERE id = ? AND state = ?)
             RETURNING seq",
        )
        .bind(workspace.id.to_string())
        .bind(workspace.organization.id.to_string())
        .bind(seq)
        .bind(serde_json::to_string(&body)?)
        .bind(appended_at.to_string())
        .bind(kind.as_str())
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

        let seq: i64 = appended.get("seq");
        for payload in payloads {
            sqlx::query("INSERT INTO transcript_payload (id, workspace_id, organization_id, seq, field, media_type, content) VALUES (?, ?, ?, ?, ?, ?, ?)")
                .bind(payload.id).bind(workspace.id.to_string())
                .bind(workspace.organization.id.to_string()).bind(seq)
                .bind(payload.field).bind(payload.media_type).bind(payload.content)
                .execute(&mut *self.connection).await?;
        }

        Ok(TranscriptEntry {
            kind,
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
            "SELECT seq, body
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

        Ok(
            match self
                .hydrate(workspace.id, row.get("seq"), row.get("body"))
                .await?
            {
                Entry::Said {
                    participant: said_by,
                    message,
                    ..
                } if said_by == participant => Some(message),
                _ => None,
            },
        )
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
            "SELECT seq, body
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
            } = self
                .hydrate(workspace.id, row.get("seq"), row.get("body"))
                .await?
                && who == participant
            {
                said.push(message);
            }
        }

        Ok(said)
    }

    /// The Brief that started this Session, and when it was written, if nothing has said anything
    /// since it: participants joining, Sessions starting and pull requests learned are not
    /// something said. Bounded to what the Transcript holds since the Workspace's last Session
    /// ended, or since it opened if none has, so a Brief that started an earlier Session is not
    /// mistaken for one starting this one. The moment orders an unbriefed Session's first Turn
    /// against held input.
    pub async fn unfollowed_brief(
        &mut self,
        workspace: &Workspace,
    ) -> Result<Option<(String, Timestamp)>> {
        let said = sqlx::query(
            "SELECT seq, body, appended_at
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
        let written_at = only.get::<String, _>("appended_at").parse()?;
        Ok(
            match self
                .hydrate(workspace.id, only.get("seq"), only.get("body"))
                .await?
            {
                Entry::Brief { brief, .. } => Some((brief, written_at)),
                _ => None,
            },
        )
    }

    /// Every entry the Transcript holds before the messages starting the Session about to begin,
    /// oldest first: a Brief or an earlier Session's history that a message which followed it
    /// reads as context rather than as its own instruction.
    pub async fn context_before_starting(&mut self, workspace: &Workspace) -> Result<Vec<Entry>> {
        let rows = sqlx::query(
            "SELECT seq, body
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

        let mut entries = Vec::new();
        for row in rows {
            entries.push(
                self.hydrate(workspace.id, row.get("seq"), row.get("body"))
                    .await?,
            );
        }
        Ok(entries)
    }

    /// The Said and Messages entries the Transcript holds since the Workspace's last Session
    /// ended, or since it opened if none has, flattened into the messages that make them up and
    /// ordered oldest first: the message or messages that started the Session about to begin.
    pub async fn starting_messages(&mut self, workspace: &Workspace) -> Result<Vec<Message>> {
        let rows = sqlx::query(
            "SELECT seq, body
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
            match self
                .hydrate(workspace.id, row.get("seq"), row.get("body"))
                .await?
            {
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
        let page = self.stored_page(workspace, from, window, kinds).await?;
        let mut entries = Vec::new();
        for record in page.entries {
            entries.push(TranscriptEntry {
                kind: record.kind,
                session_id: record.session_id,
                seq: record.seq,
                appended_at: record.appended_at,
                entry: self
                    .hydrate(workspace.id, record.seq, &record.entry.to_string())
                    .await?,
            });
        }
        Ok(Page {
            entries,
            cursor: page.cursor,
            more: page.more,
        })
    }

    pub async fn stored_page(
        &mut self,
        workspace: &Workspace,
        from: Option<Cursor>,
        window: Window,
        kinds: &Kinds,
    ) -> Result<StoredPage, Unreadable> {
        let from = self.position(workspace, from).await?;
        Ok(self
            .read_window(workspace, from, window, kinds, SeqRange::default())
            .await?)
    }

    pub async fn transcript_page(
        &mut self,
        workspace: &Workspace,
        from: Option<Cursor>,
        window: Window,
        kinds: &Kinds,
        summaries: bool,
        range: SeqRange,
    ) -> Result<TranscriptPage, Unreadable> {
        range.validate()?;
        let from = self.position(workspace, from).await?;
        let after = from
            .map_or(0, |cursor| cursor.seq)
            .max(range.first_seq.unwrap_or(1) - 1);
        let StoredPage {
            entries,
            cursor,
            more,
        } = self
            .read_window(workspace, from, window, kinds, range)
            .await?;
        let examined = cursor.map_or(after, |cursor| cursor.seq.max(after));
        let mut activities = Vec::new();
        if summaries
            && (!kinds.contains(Kind::Narration) || !kinds.contains(Kind::Detail))
            && examined > after
        {
            let boundary: i64 = sqlx::query_scalar(
                "SELECT COALESCE(MAX(seq), 0) FROM transcript_entry WHERE workspace_id = ? AND kind = 'shared_state' AND seq <= ?",
            ).bind(workspace.id.to_string()).bind(after).fetch_one(&mut *self.connection).await?;
            // Metadata stays inline even when the body's text or content is a payload reference.
            let metadata = sqlx::query(
                "SELECT seq, kind, json_extract(body, '$.type') AS type,
                        json_extract(body, '$.title') AS title, json_extract(body, '$.status') AS status,
                        json_extract(body, '$.closing_reason') AS closing_reason,
                        json_extract(body, '$.completion.started_at') AS started_at,
                        json_extract(body, '$.completion.finished_at') AS finished_at
                 FROM transcript_entry WHERE workspace_id = ? AND seq > ? AND seq <= ? ORDER BY seq",
            ).bind(workspace.id.to_string()).bind(boundary).bind(examined)
                .fetch_all(&mut *self.connection).await?;
            let mut activity: Option<Activity> = None;
            let mut omitted = false;
            for row in metadata {
                let seq: i64 = row.get("seq");
                let kind: Kind = row.get::<String, _>("kind").parse()?;
                if kind == Kind::SharedState {
                    if let Some(mut closed) = activity.take()
                        && omitted
                    {
                        closed.closed = true;
                        activities.push(closed);
                    }
                    omitted = false;
                    continue;
                }
                let summary = activity.get_or_insert_with(|| Activity::new(seq));
                summary.last_seq = seq;
                if kinds.contains(kind) {
                    continue;
                }
                omitted = true;
                let entry_type: String = row.get("type");
                match entry_type.as_str() {
                    "thought" => summary.counts.thoughts += 1,
                    "plan" => summary.counts.plans += 1,
                    "tool_call" => {
                        summary.counts.tool_calls += 1;
                        if row.get::<Option<String>, _>("status").as_deref() == Some("failed") {
                            summary.counts.failed_calls += 1;
                        }
                        summary.anomaly |= matches!(
                            row.get::<Option<String>, _>("closing_reason").as_deref(),
                            Some("interrupted" | "unresolved")
                        );
                    }
                    "expired" => summary.counts.tombstones += 1,
                    _ => {}
                }
                summary.latest = Some(ActivityItem {
                    kind,
                    title: row.get::<Option<String>, _>("title").or_else(|| {
                        match entry_type.as_str() {
                            "thought" => Some("Thought".into()),
                            "plan" => Some("Plan".into()),
                            _ => None,
                        }
                    }),
                    status: row.get("status"),
                });
                if let Some(started) = row.get::<Option<String>, _>("started_at") {
                    let started: Timestamp = started.parse().map_err(anyhow::Error::from)?;
                    summary.started_at =
                        Some(summary.started_at.map_or(started, |old| old.min(started)));
                }
                if let Some(finished) = row.get::<Option<String>, _>("finished_at") {
                    let finished: Timestamp = finished.parse().map_err(anyhow::Error::from)?;
                    summary.finished_at = Some(
                        summary
                            .finished_at
                            .map_or(finished, |old| old.max(finished)),
                    );
                }
            }
            if let Some(open) = activity
                && omitted
            {
                activities.push(open);
            }
        }
        Ok(TranscriptPage {
            entries,
            activities,
            more,
            cursor,
        })
    }

    async fn hydrate(&mut self, workspace: WorkspaceId, seq: i64, body: &str) -> Result<Entry> {
        let body = self
            .hydrate_entry(workspace, seq, serde_json::from_str(body)?)
            .await?;
        Ok(serde_json::from_value(body)?)
    }

    pub async fn hydrate_entry(
        &mut self,
        workspace: WorkspaceId,
        seq: i64,
        mut body: serde_json::Value,
    ) -> Result<serde_json::Value> {
        if body.get("payload_fields").is_none() {
            return Ok(body);
        }
        let payloads = sqlx::query("SELECT field, media_type, content FROM transcript_payload WHERE workspace_id = ? AND seq = ?")
            .bind(workspace.to_string()).bind(seq).fetch_all(&mut *self.connection).await?;
        for payload in payloads {
            let content: Vec<u8> = payload.get("content");
            let field: String = payload.get("field");
            body[&field] = if payload.get::<String, _>("media_type") == "application/json" {
                serde_json::from_slice(&content)?
            } else {
                serde_json::Value::String(String::from_utf8(content)?)
            };
        }
        body.as_object_mut()
            .expect("transcript entry")
            .remove("payload_fields");
        Ok(body)
    }

    pub async fn expire(&mut self, at: Timestamp) -> Result<usize> {
        const RETENTION: jiff::SignedDuration = jiff::SignedDuration::from_hours(30 * 24);
        const BATCH: i64 = 1_000;
        let due = sqlx::query(
            "SELECT workspace_id, seq FROM transcript_entry
             WHERE kind IN ('narration', 'detail') AND json_extract(body, '$.type') != 'expired'
               AND appended_at <= ? ORDER BY appended_at, workspace_id, seq LIMIT ?",
        )
        .bind((at - RETENTION).to_string())
        .bind(BATCH)
        .fetch_all(&mut *self.connection)
        .await?;
        let tombstone = serde_json::to_string(&Entry::Expired { expired_at: at })?;
        for row in &due {
            let workspace: String = row.get("workspace_id");
            let seq: i64 = row.get("seq");
            sqlx::query("DELETE FROM transcript_payload WHERE workspace_id = ? AND seq = ?")
                .bind(&workspace)
                .bind(seq)
                .execute(&mut *self.connection)
                .await?;
            sqlx::query("UPDATE transcript_entry SET body = ?, session_id = NULL WHERE workspace_id = ? AND seq = ?")
                .bind(&tombstone).bind(&workspace).bind(seq).execute(&mut *self.connection).await?;
        }
        Ok(due.len())
    }

    pub async fn payload(&mut self, workspace: &Workspace, id: &str) -> Result<PayloadRead> {
        let mut parts = id.split(':');
        let position = match (parts.next(), parts.next(), parts.next(), parts.next()) {
            (Some(owner), Some(seq), Some(field), None)
                if owner == workspace.id.to_string() && CONTENT_FIELDS.contains(&field) =>
            {
                seq.parse::<i64>().ok().filter(|seq| *seq > 0)
            }
            _ => None,
        };
        let Some(seq) = position else {
            return Ok(PayloadRead::Missing);
        };
        let row = sqlx::query("SELECT media_type, content FROM transcript_payload WHERE workspace_id = ? AND organization_id = ? AND id = ?")
            .bind(workspace.id.to_string()).bind(workspace.organization.id.to_string()).bind(id)
            .fetch_optional(&mut *self.connection).await?;
        if let Some(row) = row {
            return Ok(PayloadRead::Available(Payload {
                media_type: row.get("media_type"),
                content: row.get("content"),
            }));
        }
        let expired: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM transcript_entry WHERE workspace_id = ? AND seq = ? AND json_extract(body, '$.type') = 'expired')")
            .bind(workspace.id.to_string()).bind(seq).fetch_one(&mut *self.connection).await?;
        Ok(if expired {
            PayloadRead::Gone
        } else {
            PayloadRead::Missing
        })
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

    async fn read_window(
        &mut self,
        workspace: &Workspace,
        from: Option<Cursor>,
        window: Window,
        kinds: &Kinds,
        range: SeqRange,
    ) -> Result<StoredPage> {
        let after = from
            .map_or(0, |cursor| cursor.seq)
            .max(range.first_seq.unwrap_or(1) - 1);
        let rows = sqlx::query(
            "SELECT seq, body, appended_at, kind, session_id
             FROM transcript_entry
             WHERE workspace_id = ? AND seq > ? AND seq <= ?
             ORDER BY seq
             LIMIT ?",
        )
        .bind(workspace.id.to_string())
        .bind(after)
        .bind(range.last_seq.unwrap_or(i64::MAX))
        .bind(i64::try_from(window.0 + 1)?)
        .fetch_all(&mut *self.connection)
        .await
        .with_context(|| format!("reading the transcript of workspace {}", workspace.id))?;

        let more = rows.len() > window.0;
        let rows = &rows[..rows.len().min(window.0)];
        let cursor = rows
            .last()
            .map(|row| Cursor::at(workspace.id, row.get("seq")))
            .or(from);
        let mut entries = Vec::new();
        for row in rows {
            let kind = row.get::<String, _>("kind").parse()?;
            if kinds.contains(kind) {
                entries.push(StoredTranscriptEntry {
                    kind,
                    session_id: row
                        .get::<Option<String>, _>("session_id")
                        .map(|id| id.parse())
                        .transpose()?,
                    seq: row.get("seq"),
                    appended_at: row.get::<String, _>("appended_at").parse()?,
                    entry: serde_json::from_str(row.get("body"))?,
                });
            }
        }
        Ok(StoredPage {
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
    pub const fn kind(&self) -> Option<Kind> {
        match self {
            Self::Expired { .. } => None,
            Self::Thought { .. } | Self::Plan { .. } => Some(Kind::Narration),
            Self::ToolCall { .. } => Some(Kind::Detail),
            _ => Some(Kind::SharedState),
        }
    }
    pub const fn session_id(&self) -> Option<SessionId> {
        match self {
            Self::Said { session_id, .. } => *session_id,
            Self::Thought { session_id, .. }
            | Self::Plan { session_id, .. }
            | Self::ToolCall { session_id, .. } => Some(*session_id),
            Self::SessionStarted { session, .. }
            | Self::SessionEnded { session, .. }
            | Self::TurnInterrupted { session, .. } => Some(*session),
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

pub struct StoredTranscriptEntry {
    pub kind: Kind,
    pub session_id: Option<SessionId>,
    pub seq: i64,
    pub appended_at: Timestamp,
    pub entry: serde_json::Value,
}

pub struct StoredPage {
    pub entries: Vec<StoredTranscriptEntry>,
    pub cursor: Option<Cursor>,
    pub more: bool,
}

pub struct Payload {
    pub media_type: String,
    pub content: Vec<u8>,
}

struct NewPayload {
    id: String,
    field: String,
    media_type: &'static str,
    content: Vec<u8>,
}

const CONTENT_FIELDS: &[&str] = &[
    "brief", "message", "text", "entries", "messages", "input", "result",
];

fn externalize(
    workspace: WorkspaceId,
    seq: i64,
    body: &mut serde_json::Value,
) -> Result<Vec<NewPayload>> {
    let mut payloads = Vec::new();
    for &field in CONTENT_FIELDS {
        let Some(value) = body.get_mut(field) else {
            continue;
        };
        let text = matches!(field, "brief" | "message" | "text")
            .then(|| value.as_str())
            .flatten();
        let (content, media_type) = match text {
            Some(text) => (text.as_bytes().to_vec(), "text/plain; charset=utf-8"),
            None => (serde_json::to_vec(value)?, "application/json"),
        };
        if content.len() <= 64 * 1024 {
            continue;
        }
        let id = format!("{workspace}:{seq}:{field}");
        *value =
            serde_json::json!({"payload_id": id, "bytes": content.len(), "media_type": media_type});
        payloads.push(NewPayload {
            id,
            field: field.to_owned(),
            media_type,
            content,
        });
    }
    if !payloads.is_empty() {
        body["payload_fields"] = serde_json::json!(
            payloads
                .iter()
                .map(|payload| &payload.field)
                .collect::<Vec<_>>()
        );
    }
    Ok(payloads)
}

pub enum PayloadRead {
    Available(Payload),
    Gone,
    Missing,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SeqRange {
    pub first_seq: Option<i64>,
    pub last_seq: Option<i64>,
}
impl SeqRange {
    pub fn validate(self) -> Result<(), Unreadable> {
        if self.first_seq.is_some_and(|seq| seq < 1)
            || self.last_seq.is_some_and(|seq| seq < 1)
            || matches!((self.first_seq, self.last_seq), (Some(first), Some(last)) if first > last)
        {
            return Err(Unreadable::Cursor(
                "a seq range must be positive and ordered".into(),
            ));
        }
        Ok(())
    }
}

pub struct TranscriptPage {
    pub entries: Vec<StoredTranscriptEntry>,
    pub activities: Vec<Activity>,
    pub cursor: Option<Cursor>,
    pub more: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Activity {
    pub first_seq: i64,
    pub last_seq: i64,
    pub counts: ActivityCounts,
    pub latest: Option<ActivityItem>,
    pub started_at: Option<Timestamp>,
    pub finished_at: Option<Timestamp>,
    pub anomaly: bool,
    pub closed: bool,
}
impl Activity {
    fn new(seq: i64) -> Self {
        Self {
            first_seq: seq,
            last_seq: seq,
            counts: ActivityCounts::default(),
            latest: None,
            started_at: None,
            finished_at: None,
            anomaly: false,
            closed: false,
        }
    }
}
#[derive(Debug, Clone, Default, Serialize)]
pub struct ActivityCounts {
    pub tool_calls: u64,
    pub failed_calls: u64,
    pub thoughts: u64,
    pub plans: u64,
    pub tombstones: u64,
}
#[derive(Debug, Clone, Serialize)]
pub struct ActivityItem {
    pub kind: Kind,
    pub title: Option<String>,
    pub status: Option<String>,
}
