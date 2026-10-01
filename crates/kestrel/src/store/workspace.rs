use anyhow::{Context as _, Result};
use jiff::Timestamp;
use sqlx::sqlite::SqliteRow;
use sqlx::{Row, SqliteConnection};

use crate::domain::{
    Agent, Checkout, Connected, Cost, Exit, Organization, OrganizationId, Project, Session,
    SessionId, SessionState, StartedBy, SubscriptionProfile, Turn, Usage, Workspace, WorkspaceId,
    WorkspaceState,
};
use crate::fanout::Touched;
use crate::instance::Observed;
use crate::link::{Instruction, SentInstruction};
use crate::reference::{self, Candidate, Reference};
use crate::store::{agent, due, organization, profile, project, timestamp};

macro_rules! sessions_where {
    ($tail:literal) => {
        sessions_where!("", $tail)
    };
    ($columns:literal, $tail:literal) => {
        concat!(
            "SELECT id, name, organization_id, workspace_id, agent_id,
                    (SELECT name FROM agent WHERE agent.id = session.agent_id) AS agent_name,
                    harness, state, exit, exit_because, outcome_message, instance,
                    supervisor, enqueued_at, started_at, ended_at, lease_expires_at, connected_at,
                    supervisor_version, model, worked_model, context_used, context_size, cost_amount,
                    cost_currency",
            $columns,
            "
             FROM session
             WHERE ",
            $tail
        )
    };
}

/// Another Session, aliased `a`, working on a Workspace, `o`, that shares the Subscription Profile
/// of `s`'s Workspace, `w`, on the same serialized harness: what the dispatcher passes `s` over for
/// and what the queue names as holding the profile, in one place.
macro_rules! profile_held {
    () => {
        "workspace AS w
         JOIN workspace AS o ON o.subscription_profile_id = w.subscription_profile_id
         JOIN session AS a ON a.workspace_id = o.id AND a.harness = s.harness
         WHERE w.id = s.workspace_id
           AND s.harness IN (SELECT value FROM json_each(?))
           AND a.state = ?"
    };
}
pub(crate) use profile_held;

macro_rules! profile_free {
    () => {
        concat!("NOT EXISTS (SELECT 1 FROM ", profile_held!(), ")")
    };
}

/// Each Waiting Session's held input, oldest first: the order a freed slot prompts them in.
macro_rules! held_input {
    ($condition:expr) => {
        concat!(
            "SELECT s.id, MIN(p.received_at) AS since
             FROM session AS s
             JOIN pending_message AS p ON p.workspace_id = s.workspace_id
             WHERE s.state = ? AND ",
            $condition,
            "
             GROUP BY s.id
             ORDER BY since, s.id"
        )
    };
}
pub(crate) use held_input;

/// A blocker that has not ended successfully, with `session` aliased `b`: what the dispatcher
/// skips a queued Session over and what the queue names it by, in one place.
pub(crate) const UNSATISFIED_BLOCKER: &str = "NOT (b.state = ? AND b.exit IS ?)";

/// What became of a report the link was handed: the next in the supervisor's sequence, one
/// taken already — where a replay after an answer that never arrived lands — or one that
/// skips a report the Session has yet to make, which would leave a gap nothing fills.
pub enum Taken {
    Next,
    Again,
    Skipped,
}

/// A Workspace's Instance, and what its checkout was last observed to hold: `None` until a Session
/// on it reports, and forgotten whenever another Session starts on it.
pub struct Kept {
    pub workspace: WorkspaceId,
    pub instance: String,
    pub observed: Option<Vec<Observed>>,
}

pub struct PendingMessage {
    pub participant: String,
    pub body: String,
}

pub struct PendingSession {
    pub agent: Agent,
    pub trigger: String,
    pub brief: String,
}

pub struct Opening<'a> {
    pub organization: &'a Organization,
    pub project: &'a Project,
    pub agent: &'a Agent,
    pub profile: Option<&'a SubscriptionProfile>,
    /// None declares the Workspace a branch of its own.
    pub branch: Option<&'a str>,
    pub correlation: Option<&'a str>,
    pub continues: Option<&'a Workspace>,
    pub started_by: Option<StartedBy>,
}

pub struct Workspaces<'a> {
    connection: &'a mut SqliteConnection,
    touched: &'a mut Touched,
}

impl<'a> Workspaces<'a> {
    pub(crate) fn over(connection: &'a mut SqliteConnection, touched: &'a mut Touched) -> Self {
        Self {
            connection,
            touched,
        }
    }

    pub async fn open(&mut self, opening: Opening<'_>) -> Result<Workspace> {
        let opened_at = Timestamp::now();
        let id = WorkspaceId::generate();
        let workspace = loop {
            let workspace = Workspace {
                id,
                name: generated_name(),
                organization: opening.organization.clone(),
                project: opening.project.clone(),
                opened_with: opening.agent.clone(),
                profile: opening.profile.cloned(),
                checkout: Checkout {
                    repositories: opening.project.repositories.clone(),
                    base: opening.project.branch.clone(),
                    branch: opening
                        .branch
                        .map_or_else(|| format!("kestrel/{id}"), ToOwned::to_owned),
                },
                correlation: opening.correlation.map(ToOwned::to_owned),
                state: WorkspaceState::Open,
                opened_at,
                last_active_at: opened_at,
                sealed_at: None,
                continues: opening.continues.map(|sealed| sealed.id),
                started_by: opening.started_by.clone(),
            };

            let inserted = sqlx::query(
                "INSERT INTO workspace
                     (id, name, organization_id, project_id, agent_id, subscription_profile_id,
                      base, branch, correlation, state, opened_at, last_active_at, continues,
                      event_record_id, started_by_participant)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                 ON CONFLICT (organization_id, name) DO NOTHING",
            )
            .bind(workspace.id.to_string())
            .bind(&workspace.name)
            .bind(workspace.organization.id.to_string())
            .bind(workspace.project.id.to_string())
            .bind(workspace.opened_with.id.to_string())
            .bind(
                workspace
                    .profile
                    .as_ref()
                    .map(|profile| profile.id.to_string()),
            )
            .bind(&workspace.checkout.base)
            .bind(&workspace.checkout.branch)
            .bind(&workspace.correlation)
            .bind(workspace.state.as_str())
            .bind(workspace.opened_at.to_string())
            .bind(due(workspace.last_active_at))
            .bind(workspace.continues.map(|sealed| sealed.to_string()))
            .bind(match &workspace.started_by {
                Some(StartedBy::Event(event)) => Some(event.to_string()),
                _ => None,
            })
            .bind(match &workspace.started_by {
                Some(StartedBy::Participant(participant)) => Some(participant.clone()),
                _ => None,
            })
            .execute(&mut *self.connection)
            .await
            .context("opening a workspace")?;

            if inserted.rows_affected() == 1 {
                break workspace;
            }
        };

        for (position, url) in workspace.checkout.repositories.iter().enumerate() {
            sqlx::query(
                "INSERT INTO workspace_repository (workspace_id, organization_id, position, url)
                 VALUES (?, ?, ?, ?)",
            )
            .bind(workspace.id.to_string())
            .bind(workspace.organization.id.to_string())
            .bind(i64::try_from(position)?)
            .bind(url)
            .execute(&mut *self.connection)
            .await
            .with_context(|| format!("fixing the repository {url} to the workspace {id}"))?;
        }

        self.touched.workspace(&workspace);

        Ok(workspace)
    }

    pub async fn holding_correlation(
        &mut self,
        organization: &Organization,
        correlation: &str,
    ) -> Result<Option<WorkspaceId>> {
        sqlx::query(
            "SELECT id FROM workspace WHERE organization_id = ? AND state = ? AND correlation = ?",
        )
        .bind(organization.id.to_string())
        .bind(WorkspaceState::Open.as_str())
        .bind(correlation)
        .fetch_optional(&mut *self.connection)
        .await
        .with_context(|| {
            format!("reading which open workspace holds the correlation {correlation}")
        })?
        .map(|row| Ok(row.get::<String, _>("id").parse()?))
        .transpose()
    }

    pub async fn seal(&mut self, workspace: &Workspace) -> Result<Timestamp> {
        let sealed_at = Timestamp::now();

        sqlx::query("UPDATE workspace SET state = ?, sealed_at = ? WHERE id = ?")
            .bind(WorkspaceState::Sealed.as_str())
            .bind(sealed_at.to_string())
            .bind(workspace.id.to_string())
            .execute(&mut *self.connection)
            .await
            .with_context(|| format!("sealing the workspace {}", workspace.id))?;

        self.touched.workspace(workspace);

        Ok(sealed_at)
    }

    pub async fn record_active(
        &mut self,
        organization: OrganizationId,
        workspace: WorkspaceId,
        at: Timestamp,
    ) -> Result<()> {
        sqlx::query("UPDATE workspace SET last_active_at = ? WHERE id = ?")
            .bind(due(at))
            .bind(workspace.to_string())
            .execute(&mut *self.connection)
            .await
            .with_context(|| format!("recording the workspace {workspace} active"))?;

        self.touched.workspace_id(organization, workspace);

        Ok(())
    }

    pub async fn idle(&mut self, before: Timestamp) -> Result<Vec<Workspace>> {
        let ids = sqlx::query(
            "SELECT id
             FROM workspace
             WHERE state = ? AND last_active_at <= ?
             ORDER BY last_active_at, id",
        )
        .bind(WorkspaceState::Open.as_str())
        .bind(due(before))
        .fetch_all(&mut *self.connection)
        .await
        .context("sweeping idle workspaces")?
        .iter()
        .map(|row| Ok(row.get::<String, _>("id").parse()?))
        .collect::<Result<Vec<WorkspaceId>>>()?;

        let mut idle = Vec::with_capacity(ids.len());
        for id in ids {
            idle.push(read(&mut *self.connection, id).await?);
        }

        Ok(idle)
    }

    pub async fn get(&mut self, id: WorkspaceId) -> Result<Workspace> {
        read(self.connection, id).await
    }

    pub async fn find(&mut self, id: WorkspaceId) -> Result<Option<Workspace>> {
        find(self.connection, id).await
    }

    /// Resolves what an operator typed to exactly one Workspace in the organization, refusing
    /// when nothing matched or when several did.
    pub async fn resolved(
        &mut self,
        organization: &Organization,
        typed: &str,
    ) -> Result<Workspace> {
        let reference = Reference::read(typed);

        if reference.is_latest() {
            let latest = sqlx::query(
                "SELECT id FROM workspace
                 WHERE organization_id = ?
                 ORDER BY opened_at DESC, id DESC
                 LIMIT 1",
            )
            .bind(organization.id.to_string())
            .fetch_optional(&mut *self.connection)
            .await
            .context("reading the most recent workspace")?;
            let Some(latest) = latest else {
                return Err(reference::missing("workspace", &organization.name, typed));
            };

            return read(
                &mut *self.connection,
                latest.get::<String, _>("id").parse()?,
            )
            .await;
        }

        let given = reference
            .given()
            .expect("a reference that is not the latest names one");
        let named = sqlx::query("SELECT id FROM workspace WHERE organization_id = ? AND name = ?")
            .bind(organization.id.to_string())
            .bind(given)
            .fetch_optional(&mut *self.connection)
            .await
            .context("reading a workspace by its generated name")?;
        if let Some(named) = named {
            return read(&mut *self.connection, named.get::<String, _>("id").parse()?).await;
        }

        if let Some(prefix) = reference.prefix() {
            let matched = sqlx::query(
                "SELECT id, name FROM workspace
                 WHERE organization_id = ? AND REPLACE(LOWER(id), '-', '') LIKE ? || '%'
                 ORDER BY name, id",
            )
            .bind(organization.id.to_string())
            .bind(prefix)
            .fetch_all(&mut *self.connection)
            .await
            .context("reading workspaces by identifier prefix")?
            .iter()
            .map(candidate)
            .collect::<Vec<_>>();

            match matched.as_slice() {
                [] => {}
                [only] => return read(&mut *self.connection, only.id.parse()?).await,
                _ => {
                    return Err(reference::ambiguous(
                        "workspace",
                        &organization.name,
                        given,
                        &matched,
                    ));
                }
            }
        }

        Err(reference::missing("workspace", &organization.name, given))
    }

    /// A Session on the same terms as a Workspace.
    pub async fn resolved_session(
        &mut self,
        organization: &Organization,
        typed: &str,
    ) -> Result<Session> {
        let reference = Reference::read(typed);

        if reference.is_latest() {
            let latest = sqlx::query(sessions_where!(
                "organization_id = ?
                 ORDER BY enqueued_at DESC, id DESC
                 LIMIT 1"
            ))
            .bind(organization.id.to_string())
            .fetch_optional(&mut *self.connection)
            .await
            .context("reading the most recent session")?;
            let Some(latest) = latest else {
                return Err(reference::missing("session", &organization.name, typed));
            };

            return session(&latest);
        }

        let given = reference
            .given()
            .expect("a reference that is not the latest names one");
        let named = sqlx::query(sessions_where!("organization_id = ? AND name = ?"))
            .bind(organization.id.to_string())
            .bind(given)
            .fetch_optional(&mut *self.connection)
            .await
            .context("reading a session by its generated name")?;
        if let Some(named) = named {
            return session(&named);
        }

        if let Some(prefix) = reference.prefix() {
            let matched = sqlx::query(
                "SELECT id, name FROM session
                 WHERE organization_id = ? AND REPLACE(LOWER(id), '-', '') LIKE ? || '%'
                 ORDER BY name, id",
            )
            .bind(organization.id.to_string())
            .bind(prefix)
            .fetch_all(&mut *self.connection)
            .await
            .context("reading sessions by identifier prefix")?
            .iter()
            .map(candidate)
            .collect::<Vec<_>>();

            match matched.as_slice() {
                [] => {}
                [only] => return self.session(only.id.parse()?).await,
                _ => {
                    return Err(reference::ambiguous(
                        "session",
                        &organization.name,
                        given,
                        &matched,
                    ));
                }
            }
        }

        Err(reference::missing("session", &organization.name, given))
    }

    pub async fn all(&mut self, organization: &Organization) -> Result<Vec<Workspace>> {
        let ids = sqlx::query(
            "SELECT id FROM workspace WHERE organization_id = ? ORDER BY opened_at, id",
        )
        .bind(organization.id.to_string())
        .fetch_all(&mut *self.connection)
        .await
        .context("reading an organization's workspaces")?
        .iter()
        .map(|row| Ok(row.get::<String, _>("id").parse()?))
        .collect::<Result<Vec<WorkspaceId>>>()?;

        let mut workspaces = Vec::with_capacity(ids.len());
        for id in ids {
            workspaces.push(read(&mut *self.connection, id).await?);
        }

        Ok(workspaces)
    }

    /// Read on its own rather than with the Workspace: every path that reports a Session reads one,
    /// and none of them looks at what continues it.
    pub async fn continuations(&mut self, sealed: WorkspaceId) -> Result<Vec<WorkspaceId>> {
        sqlx::query("SELECT id FROM workspace WHERE continues = ? ORDER BY opened_at, id")
            .bind(sealed.to_string())
            .fetch_all(&mut *self.connection)
            .await
            .with_context(|| format!("reading what continues the workspace {sealed}"))?
            .iter()
            .map(|row| Ok(row.get::<String, _>("id").parse()?))
            .collect()
    }

    pub(crate) async fn has_had_session(&mut self, workspace: WorkspaceId) -> Result<bool> {
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM session WHERE workspace_id = ?)")
            .bind(workspace.to_string())
            .fetch_one(&mut *self.connection)
            .await
            .with_context(|| {
                format!("reading whether the workspace {workspace} ever had a session")
            })
    }

    /// Held from the moment work is enqueued rather than dispatched: two Sessions queued in one
    /// Workspace would otherwise both be handed out.
    pub(crate) async fn unfinished_session(
        &mut self,
        workspace: &Workspace,
    ) -> Result<Option<Unfinished>> {
        let holding = sqlx::query(sessions_where!(
            ",
                    EXISTS (SELECT 1 FROM pending_message p WHERE p.workspace_id = session.workspace_id)
                    OR EXISTS (SELECT 1 FROM pending_session p WHERE p.workspace_id = session.workspace_id)
                        AS held_input",
            "workspace_id = ?
               AND state NOT IN (?, ?)
             ORDER BY enqueued_at, id
             LIMIT 1"
        ))
        .bind(workspace.id.to_string())
        .bind(SessionState::Ended.as_str())
        .bind(SessionState::Unreachable.as_str())
        .fetch_optional(&mut *self.connection)
        .await
        .with_context(|| format!("reading what session the workspace {} has", workspace.id))?;

        holding
            .map(|row| {
                Ok(Unfinished {
                    session: session(&row)?,
                    held_input: row.get("held_input"),
                })
            })
            .transpose()
    }

    pub async fn sealed_holding_correlation(
        &mut self,
        organization: &Organization,
        correlation: &str,
    ) -> Result<Option<Workspace>> {
        let sealed = sqlx::query(
            "SELECT id FROM workspace
             WHERE organization_id = ? AND state = ? AND correlation = ?
             ORDER BY sealed_at DESC, id DESC
             LIMIT 1",
        )
        .bind(organization.id.to_string())
        .bind(WorkspaceState::Sealed.as_str())
        .bind(correlation)
        .fetch_optional(&mut *self.connection)
        .await
        .with_context(|| {
            format!("reading which sealed workspace held the correlation {correlation}")
        })?;

        let Some(sealed) = sealed else {
            return Ok(None);
        };
        let id = sealed.get::<String, _>("id").parse()?;

        Ok(Some(read(&mut *self.connection, id).await?))
    }

    /// The latest Session's Agent as that Session froze it, so continuing work never moves onto a
    /// redeclared harness or model.
    async fn latest_agent(&mut self, workspace: &Workspace) -> Result<Agent> {
        let latest = sqlx::query(sessions_where!(
            "workspace_id = ? ORDER BY enqueued_at DESC, id DESC LIMIT 1"
        ))
        .bind(workspace.id.to_string())
        .fetch_optional(&mut *self.connection)
        .await
        .with_context(|| format!("reading the latest agent of the workspace {}", workspace.id))?;

        match latest {
            Some(row) => Ok(session(&row)?.agent),
            None => Ok(workspace.opened_with.clone()),
        }
    }

    /// No `agent` continues with the latest Session's; `model` stands in for whatever the Agent names.
    pub async fn enqueue_session(
        &mut self,
        workspace: &Workspace,
        agent: Option<&Agent>,
        model: Option<&str>,
    ) -> Result<Session> {
        let agent = match agent {
            Some(agent) => agent.clone(),
            None => self.latest_agent(workspace).await?,
        };
        let session = loop {
            let session = Session {
                id: SessionId::generate(),
                name: generated_name(),
                organization: workspace.organization.id,
                workspace: workspace.id,
                agent: Agent {
                    model: model.map(str::to_owned).or_else(|| agent.model.clone()),
                    ..agent.clone()
                },
                state: SessionState::Queued,
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
            };

            let inserted = sqlx::query(
                "INSERT INTO session
                     (id, name, organization_id, workspace_id, agent_id, harness, model, state,
                      enqueued_at)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
                 ON CONFLICT (organization_id, name) DO NOTHING",
            )
            .bind(session.id.to_string())
            .bind(&session.name)
            .bind(session.organization.to_string())
            .bind(session.workspace.to_string())
            .bind(session.agent.id.to_string())
            .bind(&session.agent.harness)
            .bind(&session.agent.model)
            .bind(session.state.as_str())
            .bind(due(session.enqueued_at))
            .execute(&mut *self.connection)
            .await
            .with_context(|| format!("enqueueing a session in the workspace {}", workspace.id))?;

            if inserted.rows_affected() == 1 {
                break session;
            }
        };

        self.touched.session(&session);
        self.touched.queue(session.organization);
        self.record_active(workspace.organization.id, workspace.id, session.enqueued_at)
            .await?;

        Ok(session)
    }

    pub async fn add_pending_message(
        &mut self,
        workspace: &Workspace,
        participant: &str,
        body: &str,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO pending_message (
                 workspace_id, organization_id, seq, participant, body, received_at
             )
             SELECT ?, ?, COALESCE(MAX(seq), 0) + 1, ?, ?, ?
             FROM pending_message
             WHERE workspace_id = ?",
        )
        .bind(workspace.id.to_string())
        .bind(workspace.organization.id.to_string())
        .bind(participant)
        .bind(body)
        .bind(due(Timestamp::now()))
        .bind(workspace.id.to_string())
        .execute(&mut *self.connection)
        .await
        .with_context(|| {
            format!(
                "holding a pending message in the workspace {}",
                workspace.id
            )
        })?;

        self.touched.workspace(workspace);

        Ok(())
    }

    /// Leaves any that arrived after a pending Session, to drain after it.
    pub async fn take_pending_messages(
        &mut self,
        workspace: &Workspace,
    ) -> Result<Vec<PendingMessage>> {
        let rows = sqlx::query(
            "DELETE FROM pending_message
             WHERE workspace_id = ?
               AND NOT EXISTS (
                   SELECT 1 FROM pending_session s
                   WHERE s.workspace_id = pending_message.workspace_id
                     AND s.received_at <= pending_message.received_at
               )
             RETURNING participant, body, seq",
        )
        .bind(workspace.id.to_string())
        .fetch_all(&mut *self.connection)
        .await
        .with_context(|| {
            format!(
                "taking pending messages from the workspace {}",
                workspace.id
            )
        })?;

        let mut messages = rows
            .iter()
            .map(|row| {
                (
                    row.get::<i64, _>("seq"),
                    PendingMessage {
                        participant: row.get("participant"),
                        body: row.get("body"),
                    },
                )
            })
            .collect::<Vec<_>>();
        messages.sort_by_key(|(seq, _)| *seq);

        if !messages.is_empty() {
            self.touched.workspace(workspace);
        }

        Ok(messages.into_iter().map(|(_, message)| message).collect())
    }

    pub async fn add_pending_session(
        &mut self,
        workspace: &Workspace,
        pending: &PendingSession,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO pending_session (
                 workspace_id, organization_id, seq, agent_id, trigger, brief, received_at
             )
             SELECT ?, ?, COALESCE(MAX(seq), 0) + 1, ?, ?, ?, ?
             FROM pending_session
             WHERE workspace_id = ?",
        )
        .bind(workspace.id.to_string())
        .bind(workspace.organization.id.to_string())
        .bind(pending.agent.id.to_string())
        .bind(&pending.trigger)
        .bind(&pending.brief)
        .bind(due(Timestamp::now()))
        .bind(workspace.id.to_string())
        .execute(&mut *self.connection)
        .await
        .with_context(|| {
            format!(
                "holding a pending session in the workspace {}",
                workspace.id
            )
        })?;

        self.touched.workspace(workspace);

        Ok(())
    }

    /// Taken only when no pending message arrived before it, so a Workspace drains in the order
    /// its input arrived.
    pub async fn take_pending_session(
        &mut self,
        workspace: &Workspace,
    ) -> Result<Option<PendingSession>> {
        let Some(row) = sqlx::query(
            "DELETE FROM pending_session
             WHERE workspace_id = ?1
               AND seq = (SELECT MIN(seq) FROM pending_session WHERE workspace_id = ?1)
               AND NOT EXISTS (
                   SELECT 1 FROM pending_message m
                   WHERE m.workspace_id = ?1 AND m.received_at < pending_session.received_at
               )
             RETURNING agent_id, trigger, brief",
        )
        .bind(workspace.id.to_string())
        .fetch_optional(&mut *self.connection)
        .await
        .with_context(|| {
            format!(
                "taking a pending session from the workspace {}",
                workspace.id
            )
        })?
        else {
            return Ok(None);
        };

        self.touched.workspace(workspace);

        Ok(Some(PendingSession {
            agent: agent::with_id(
                &mut *self.connection,
                &workspace.organization,
                row.get::<String, _>("agent_id").parse()?,
            )
            .await?,
            trigger: row.get("trigger"),
            brief: row.get("brief"),
        }))
    }

    pub async fn declare_blocked(&mut self, session: &Session, blocker: &Session) -> Result<()> {
        if session.organization != blocker.organization {
            anyhow::bail!(
                "the session {} and the session {} it is blocked on are in different organizations",
                session.id,
                blocker.id
            );
        }

        sqlx::query(
            "INSERT INTO session_dependency (session_id, blocker_id, organization_id)
             VALUES (?, ?, ?)",
        )
        .bind(session.id.to_string())
        .bind(blocker.id.to_string())
        .bind(session.organization.to_string())
        .execute(&mut *self.connection)
        .await
        .with_context(|| {
            format!(
                "declaring the session {} blocked on {}",
                session.id, blocker.id
            )
        })?;

        self.touched.queue(session.organization);

        Ok(())
    }

    /// Every queued Session still waiting on this one as a blocker. Tolerance defaults to
    /// all-must-succeed, so any one of them is enough to name a dependent whose tolerance a
    /// blocker that just ended without succeeding can no longer meet.
    pub async fn dependents_of(&mut self, blocker: SessionId) -> Result<Vec<Session>> {
        sqlx::query(sessions_where!(
            "state = ?
               AND id IN (SELECT session_id FROM session_dependency WHERE blocker_id = ?)
             ORDER BY enqueued_at, id"
        ))
        .bind(SessionState::Queued.as_str())
        .bind(blocker.to_string())
        .fetch_all(&mut *self.connection)
        .await
        .with_context(|| format!("reading what is still waiting on the session {blocker}"))?
        .iter()
        .map(session)
        .collect()
    }

    /// A queued Session whose declared tolerance can no longer be met: terminal like an ended
    /// Session, but never claimed and never carrying an exit status, because nothing failed.
    /// `false` when the Session was no longer queued, so a claimant that got there first stands.
    pub async fn mark_unreachable(&mut self, session: &Session) -> Result<bool> {
        let marked = sqlx::query(
            "UPDATE session SET state = ?, ended_at = ?
                 WHERE id = ? AND state = ?",
        )
        .bind(SessionState::Unreachable.as_str())
        .bind(Timestamp::now().to_string())
        .bind(session.id.to_string())
        .bind(SessionState::Queued.as_str())
        .execute(&mut *self.connection)
        .await
        .with_context(|| format!("marking the session {} unreachable", session.id))?;

        if marked.rows_affected() > 0 {
            self.touched.session(session);
            self.touched.queue(session.organization);
        }

        Ok(marked.rows_affected() > 0)
    }

    pub async fn claimable_sessions(&mut self, serialized: &[String]) -> Result<Vec<Session>> {
        let claimable = format!(
            "SELECT s.id
             FROM session AS s
             WHERE s.state = ?
               AND NOT EXISTS (
                   SELECT 1
                   FROM session_dependency AS d
                   JOIN session AS b ON b.id = d.blocker_id
                   WHERE d.session_id = s.id
                     AND {UNSATISFIED_BLOCKER}
               )
               AND {}
             ORDER BY s.enqueued_at, s.id",
            profile_free!()
        );
        let ids = sqlx::query(sqlx::AssertSqlSafe(claimable))
            .bind(SessionState::Queued.as_str())
            .bind(SessionState::Ended.as_str())
            .bind(Exit::Succeeded.status())
            .bind(serde_json::to_string(serialized)?)
            .bind(SessionState::Working.as_str())
            .fetch_all(&mut *self.connection)
            .await
            .context("reading claimable sessions")?;

        let mut sessions = Vec::with_capacity(ids.len());
        for id in ids {
            sessions.push(self.session(id.get::<String, _>("id").parse()?).await?);
        }
        Ok(sessions)
    }

    pub async fn claim_session(
        &mut self,
        session: &Session,
        lease_until: Timestamp,
    ) -> Result<Option<Session>> {
        let claimed = sqlx::query(
            "UPDATE session
             SET state = ?, claimed_at = ?, lease_expires_at = ?
             WHERE id = ? AND state = ?
             RETURNING id",
        )
        .bind(SessionState::Working.as_str())
        .bind(Timestamp::now().to_string())
        .bind(due(lease_until))
        .bind(session.id.to_string())
        .bind(SessionState::Queued.as_str())
        .fetch_optional(&mut *self.connection)
        .await
        .with_context(|| format!("claiming the queued session {}", session.id))?;

        match claimed {
            Some(_) => {
                self.touched.session(session);
                self.touched.queue(session.organization);

                Ok(Some(self.session(session.id).await?))
            }
            None => Ok(None),
        }
    }

    pub async fn session(&mut self, id: SessionId) -> Result<Session> {
        let row = sqlx::query(sessions_where!("id = ?"))
            .bind(id.to_string())
            .fetch_optional(&mut *self.connection)
            .await?
            .with_context(|| format!("no session {id}"))?;

        session(&row)
    }

    pub async fn sessions(&mut self, workspace: &Workspace) -> Result<Vec<Session>> {
        sqlx::query(sessions_where!("workspace_id = ? ORDER BY enqueued_at, id"))
            .bind(workspace.id.to_string())
            .fetch_all(&mut *self.connection)
            .await?
            .iter()
            .map(session)
            .collect()
    }

    pub async fn sessions_in(
        &mut self,
        organization: &Organization,
        state: SessionState,
    ) -> Result<Vec<Session>> {
        sqlx::query(sessions_where!(
            "organization_id = ? AND state = ? ORDER BY enqueued_at, id"
        ))
        .bind(organization.id.to_string())
        .bind(state.as_str())
        .fetch_all(&mut *self.connection)
        .await
        .with_context(|| format!("reading the organization's {state} sessions"))?
        .iter()
        .map(session)
        .collect()
    }

    /// The figures are cumulative, so the last report of them is the one that stands.
    pub async fn record_usage(&mut self, session: &Session, usage: &Usage) -> Result<()> {
        sqlx::query(
            "UPDATE session
             SET context_used = ?, context_size = ?, cost_amount = ?, cost_currency = ?
             WHERE id = ?",
        )
        .bind(usage.context_used as i64)
        .bind(usage.context_size as i64)
        .bind(usage.cost.as_ref().map(|cost| cost.amount))
        .bind(usage.cost.as_ref().map(|cost| cost.currency.clone()))
        .bind(session.id.to_string())
        .execute(&mut *self.connection)
        .await
        .with_context(|| format!("recording what session {} used", session.id))?;

        self.touched.session(session);

        Ok(())
    }

    pub async fn record_worked_model(&mut self, session: &Session, model: &str) -> Result<()> {
        sqlx::query("UPDATE session SET worked_model = ? WHERE id = ?")
            .bind(model)
            .bind(session.id.to_string())
            .execute(&mut *self.connection)
            .await
            .with_context(|| format!("recording the model session {} is on", session.id))?;

        self.touched.session(session);

        Ok(())
    }

    pub async fn instance(&mut self, workspace: WorkspaceId) -> Result<Option<String>> {
        let row = sqlx::query("SELECT instance FROM workspace WHERE id = ?")
            .bind(workspace.to_string())
            .fetch_one(&mut *self.connection)
            .await
            .with_context(|| format!("reading the instance of the workspace {workspace}"))?;

        Ok(row.get("instance"))
    }

    /// `None` forgets an Instance that is gone, so the Workspace's next Session provisions another.
    pub async fn record_instance(
        &mut self,
        organization: OrganizationId,
        workspace: WorkspaceId,
        instance: Option<&str>,
    ) -> Result<()> {
        sqlx::query("UPDATE workspace SET instance = ?, observed = NULL WHERE id = ?")
            .bind(instance)
            .bind(workspace.to_string())
            .execute(&mut *self.connection)
            .await
            .with_context(|| format!("recording the instance of the workspace {workspace}"))?;

        self.touched.workspace_id(organization, workspace);

        Ok(())
    }

    pub async fn record_observed(
        &mut self,
        organization: OrganizationId,
        workspace: WorkspaceId,
        observed: &[Observed],
    ) -> Result<()> {
        sqlx::query("UPDATE workspace SET observed = ? WHERE id = ? AND instance IS NOT NULL")
            .bind(serde_json::to_string(observed)?)
            .bind(workspace.to_string())
            .execute(&mut *self.connection)
            .await
            .with_context(|| {
                format!("recording what the workspace {workspace}'s checkout holds")
            })?;

        self.touched.workspace_id(organization, workspace);

        Ok(())
    }

    pub async fn kept_instance(&mut self, workspace: WorkspaceId) -> Result<Option<Kept>> {
        sqlx::query(
            "SELECT id, instance, observed FROM workspace WHERE id = ? AND instance IS NOT NULL",
        )
        .bind(workspace.to_string())
        .fetch_optional(&mut *self.connection)
        .await
        .with_context(|| format!("reading the instance of the workspace {workspace}"))?
        .map(|row| kept(&row))
        .transpose()
    }

    pub async fn kept_instances(&mut self, organization: &Organization) -> Result<Vec<Kept>> {
        sqlx::query(
            "SELECT id, instance, observed
             FROM workspace
             WHERE organization_id = ? AND instance IS NOT NULL
             ORDER BY last_active_at, id",
        )
        .bind(organization.id.to_string())
        .fetch_all(&mut *self.connection)
        .await
        .context("reading the instances workspaces keep")?
        .iter()
        .map(kept)
        .collect()
    }

    /// The Workspace lets go of its Instance at once, so no Session is handed one about to be
    /// destroyed.
    pub async fn archive_instance(&mut self, workspace: &Workspace, instance: &str) -> Result<()> {
        self.record_instance(workspace.organization.id, workspace.id, None)
            .await?;
        self.forget_supervisor(instance).await?;
        sqlx::query(
            "INSERT INTO instance_archive (instance, organization_id, workspace_id, queued_at)
             VALUES (?, ?, ?, ?)",
        )
        .bind(instance)
        .bind(workspace.organization.id.to_string())
        .bind(workspace.id.to_string())
        .bind(Timestamp::now().to_string())
        .execute(&mut *self.connection)
        .await
        .with_context(|| format!("queueing the instance {instance} to be archived"))?;

        self.touched.queue(workspace.organization.id);

        Ok(())
    }

    pub async fn instances_to_archive(&mut self) -> Result<Vec<String>> {
        sqlx::query_scalar("SELECT instance FROM instance_archive ORDER BY queued_at, instance")
            .fetch_all(&mut *self.connection)
            .await
            .context("reading the instances waiting to be archived")
    }

    pub async fn live_instance_count(&mut self, organization: &Organization) -> Result<usize> {
        let count: i64 = sqlx::query_scalar(
            "SELECT
                 (SELECT COUNT(*) FROM workspace WHERE organization_id = ? AND instance IS NOT NULL)
               + (SELECT COUNT(*) FROM instance_archive WHERE organization_id = ?)
               + (SELECT COUNT(*)
                  FROM session
                  JOIN workspace ON workspace.id = session.workspace_id
                  WHERE session.organization_id = ?
                    AND session.state IN (SELECT value FROM json_each(?))
                    AND workspace.instance IS NULL)",
        )
        .bind(organization.id.to_string())
        .bind(organization.id.to_string())
        .bind(organization.id.to_string())
        .bind(live()?)
        .fetch_one(&mut *self.connection)
        .await
        .context("counting an organization's live instances")?;
        Ok(count as usize)
    }

    pub async fn instance_being_archived(
        &mut self,
        organization: &Organization,
    ) -> Result<Option<String>> {
        sqlx::query_scalar(
            "SELECT instance FROM instance_archive
             WHERE organization_id = ?
             ORDER BY queued_at, instance
             LIMIT 1",
        )
        .bind(organization.id.to_string())
        .fetch_optional(&mut *self.connection)
        .await
        .context("reading an organization's instance being archived")
    }

    pub async fn instance_archived(&mut self, instance: &str) -> Result<()> {
        let organization: Option<String> =
            sqlx::query_scalar("SELECT organization_id FROM instance_archive WHERE instance = ?")
                .bind(instance)
                .fetch_optional(&mut *self.connection)
                .await
                .with_context(|| format!("reading the organization of the instance {instance}"))?;

        sqlx::query("DELETE FROM instance_archive WHERE instance = ?")
            .bind(instance)
            .execute(&mut *self.connection)
            .await
            .with_context(|| format!("recording the instance {instance} archived"))?;

        if let Some(organization) = organization {
            self.touched.queue(organization.parse()?);
        }

        Ok(())
    }

    pub async fn record_session_instance(
        &mut self,
        session: &Session,
        instance: &str,
    ) -> Result<()> {
        sqlx::query("UPDATE session SET instance = ? WHERE id = ?")
            .bind(instance)
            .bind(session.id.to_string())
            .execute(&mut *self.connection)
            .await
            .with_context(|| {
                format!("recording the instance session {} executes on", session.id)
            })?;

        self.touched.session(session);

        Ok(())
    }

    /// The Secret's digest is recorded before the supervisor that presents it is started, so its
    /// first dial is never refused; the name follows once the driver has one.
    pub async fn start_supervisor(
        &mut self,
        workspace: &Workspace,
        instance: &str,
        digest: &str,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO supervisor (instance, organization_id, token_hash, started_at)
             VALUES (?, ?, ?, ?)
             ON CONFLICT (instance) DO UPDATE
             SET token_hash = excluded.token_hash, name = NULL, version = NULL,
                 started_at = excluded.started_at, reached_at = NULL",
        )
        .bind(instance)
        .bind(workspace.organization.id.to_string())
        .bind(digest)
        .bind(Timestamp::now().to_string())
        .execute(&mut *self.connection)
        .await
        .with_context(|| format!("recording a supervisor started on the instance {instance}"))?;

        Ok(())
    }

    pub async fn name_supervisor(
        &mut self,
        instance: &str,
        digest: &str,
        name: &str,
    ) -> Result<()> {
        sqlx::query("UPDATE supervisor SET name = ? WHERE instance = ? AND token_hash = ?")
            .bind(name)
            .bind(instance)
            .bind(digest)
            .execute(&mut *self.connection)
            .await
            .with_context(|| format!("naming the supervisor of the instance {instance}"))?;

        Ok(())
    }

    pub async fn supervisor(&mut self, instance: &str) -> Result<Option<Supervisor>> {
        sqlx::query("SELECT name, reached_at FROM supervisor WHERE instance = ?")
            .bind(instance)
            .fetch_optional(&mut *self.connection)
            .await
            .with_context(|| format!("reading the supervisor of the instance {instance}"))?
            .map(|row| {
                Ok(Supervisor {
                    name: row.get("name"),
                    reached_at: timestamp(&row, "reached_at")?,
                })
            })
            .transpose()
    }

    /// Its credential goes with it, so nothing presenting that credential is let on the link again.
    pub async fn forget_supervisor(&mut self, instance: &str) -> Result<()> {
        sqlx::query("DELETE FROM supervisor WHERE instance = ?")
            .bind(instance)
            .execute(&mut *self.connection)
            .await
            .with_context(|| format!("forgetting the supervisor of the instance {instance}"))?;

        Ok(())
    }

    /// Only while a Workspace still holds the Instance: one let go takes its link with it.
    pub async fn linked(&mut self, digest: &str) -> Result<Option<Linked>> {
        sqlx::query(
            "SELECT supervisor.instance, workspace.id AS workspace_id
             FROM supervisor
             JOIN workspace ON workspace.instance = supervisor.instance
             WHERE supervisor.token_hash = ?",
        )
        .bind(digest)
        .fetch_optional(&mut *self.connection)
        .await
        .context("reading which instance a link credential belongs to")?
        .map(|row| {
            Ok(Linked {
                instance: row.get("instance"),
                workspace: row.get::<String, _>("workspace_id").parse()?,
            })
        })
        .transpose()
    }

    pub async fn record_reached(&mut self, instance: &str) -> Result<()> {
        sqlx::query("UPDATE supervisor SET reached_at = ? WHERE instance = ?")
            .bind(Timestamp::now().to_string())
            .bind(instance)
            .execute(&mut *self.connection)
            .await
            .with_context(|| format!("recording the supervisor of {instance} reached the link"))?;

        Ok(())
    }

    /// Recorded on the Sessions it is carrying too, since a Session reads which supervisor it ran
    /// through and at what version.
    pub async fn record_connected(&mut self, instance: &str, version: &str) -> Result<()> {
        let now = Timestamp::now().to_string();
        let sessions = self.live_sessions_on(instance).await?;
        sqlx::query("UPDATE supervisor SET reached_at = ?, version = ? WHERE instance = ?")
            .bind(&now)
            .bind(version)
            .bind(instance)
            .execute(&mut *self.connection)
            .await
            .with_context(|| format!("recording the supervisor of {instance} connected"))?;
        sqlx::query(
            "UPDATE session SET connected_at = ?, supervisor_version = ?
             WHERE instance = ? AND state IN (SELECT value FROM json_each(?))",
        )
        .bind(&now)
        .bind(version)
        .bind(instance)
        .bind(live()?)
        .execute(&mut *self.connection)
        .await
        .with_context(|| format!("recording the supervisor of {instance} connected"))?;

        for session in sessions {
            self.touched.session(&session);
        }

        Ok(())
    }

    /// What the supervisor last said of itself goes with it, so a Session begun over a link that
    /// was already open still reads as connected.
    pub async fn record_supervisor(
        &mut self,
        session: &Session,
        instance: &str,
        supervisor: Option<&str>,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE session
             SET supervisor = ?,
                 connected_at = (SELECT reached_at FROM supervisor WHERE instance = ?),
                 supervisor_version = (SELECT version FROM supervisor WHERE instance = ?)
             WHERE id = ?",
        )
        .bind(supervisor)
        .bind(instance)
        .bind(instance)
        .bind(session.id.to_string())
        .execute(&mut *self.connection)
        .await
        .with_context(|| format!("recording the supervisor of session {}", session.id))?;

        self.touched.session(session);

        Ok(())
    }

    /// A lease that has already passed is not revived: the sweep is about to end its Session, and
    /// a control plane coming back takes no supervisor's word for one it has let go.
    pub async fn hold_leases_on(&mut self, instance: &str, until: Timestamp) -> Result<()> {
        let now = Timestamp::now();
        let sessions = self.live_sessions_on(instance).await?;

        sqlx::query(
            "UPDATE session SET lease_expires_at = ?
             WHERE instance = ? AND state IN (SELECT value FROM json_each(?))
               AND lease_expires_at > ?",
        )
        .bind(due(until))
        .bind(instance)
        .bind(live()?)
        .bind(due(now))
        .execute(&mut *self.connection)
        .await
        .with_context(|| format!("holding the leases of the sessions on {instance}"))?;

        for session in sessions {
            if session.lease_expires_at.is_some_and(|at| at > now) {
                self.touched.session(&session);
            }
        }

        Ok(())
    }

    pub async fn carried(&mut self, instance: &str, id: SessionId) -> Result<Option<Session>> {
        sqlx::query(sessions_where!(
            "id = ? AND instance = ? AND state IN (SELECT value FROM json_each(?))
               AND lease_expires_at > ?"
        ))
        .bind(id.to_string())
        .bind(instance)
        .bind(live()?)
        .bind(due(Timestamp::now()))
        .fetch_optional(&mut *self.connection)
        .await
        .with_context(|| format!("reading whether {instance} carries the session {id}"))?
        .map(|row| session(&row))
        .transpose()
    }

    pub async fn live_sessions_on(&mut self, instance: &str) -> Result<Vec<Session>> {
        sqlx::query(sessions_where!(
            "instance = ? AND state IN (SELECT value FROM json_each(?)) ORDER BY enqueued_at, id"
        ))
        .bind(instance)
        .bind(live()?)
        .fetch_all(&mut *self.connection)
        .await
        .with_context(|| format!("reading the sessions on {instance}"))?
        .iter()
        .map(session)
        .collect()
    }

    pub async fn hold_lease(&mut self, session: &Session, until: Timestamp) -> Result<()> {
        sqlx::query(
            "UPDATE session SET lease_expires_at = ?
             WHERE id = ? AND state IN (SELECT value FROM json_each(?))",
        )
        .bind(due(until))
        .bind(session.id.to_string())
        .bind(live()?)
        .execute(&mut *self.connection)
        .await
        .with_context(|| format!("holding the lease of session {} until {until}", session.id))?;

        self.touched.session(session);

        Ok(())
    }

    pub async fn expired_leases(&mut self, at: Timestamp) -> Result<Vec<Session>> {
        sqlx::query(sessions_where!(
            "state IN (SELECT value FROM json_each(?)) AND lease_expires_at <= ?
             ORDER BY lease_expires_at"
        ))
        .bind(live()?)
        .bind(due(at))
        .fetch_all(&mut *self.connection)
        .await
        .context("sweeping expired leases")?
        .iter()
        .map(session)
        .collect()
    }

    /// `false` when the Session had already started, so a supervisor that reconnects and says
    /// so again adds no second Transcript entry.
    pub async fn record_started(&mut self, session: &Session) -> Result<bool> {
        let started =
            sqlx::query("UPDATE session SET started_at = ? WHERE id = ? AND started_at IS NULL")
                .bind(Timestamp::now().to_string())
                .bind(session.id.to_string())
                .execute(&mut *self.connection)
                .await
                .with_context(|| format!("recording the session {} started", session.id))?;

        if started.rows_affected() > 0 {
            self.touched.session(session);
        }

        Ok(started.rows_affected() > 0)
    }

    /// Read and write under the same write lock every `Tx` takes up front, so the check and
    /// whatever the report changes commit together or not at all (ADR-0004).
    pub async fn take_report(&mut self, session: &Session, seq: i64) -> Result<Taken> {
        let taken: i64 = sqlx::query("SELECT reports_taken FROM session WHERE id = ?")
            .bind(session.id.to_string())
            .fetch_one(&mut *self.connection)
            .await
            .with_context(|| format!("reading what session {} has reported", session.id))?
            .get("reports_taken");

        if seq != taken + 1 {
            return Ok(match (1..=taken).contains(&seq) {
                true => Taken::Again,
                false => Taken::Skipped,
            });
        }

        sqlx::query("UPDATE session SET reports_taken = ? WHERE id = ?")
            .bind(seq)
            .bind(session.id.to_string())
            .execute(&mut *self.connection)
            .await
            .with_context(|| format!("taking the report {seq} of session {}", session.id))?;

        Ok(Taken::Next)
    }

    /// `false` when the Session had already ended: whoever ends it first decides its exit status.
    pub async fn end_session(
        &mut self,
        session: &Session,
        exit: &Exit,
        message: Option<&str>,
    ) -> Result<bool> {
        let ended = sqlx::query(
            "UPDATE session
             SET state = ?, ended_at = ?, exit = ?, exit_because = ?, outcome_message = ?, lease_expires_at = NULL
             WHERE id = ? AND state != ?",
        )
        .bind(SessionState::Ended.as_str())
        .bind(Timestamp::now().to_string())
        .bind(exit.status())
        .bind(exit.because())
        .bind(message)
        .bind(session.id.to_string())
        .bind(SessionState::Ended.as_str())
        .execute(&mut *self.connection)
        .await
        .with_context(|| format!("ending the session {}", session.id))?;

        if ended.rows_affected() == 0 {
            return Ok(false);
        }
        self.touched.session(session);
        self.touched.queue(session.organization);
        self.record_active(session.organization, session.workspace, Timestamp::now())
            .await?;

        Ok(true)
    }

    /// Down the stream of the Instance the Session executes on; `None` for a Session that never
    /// reached one, which has nothing listening for it.
    pub async fn send_instruction(
        &mut self,
        session: &Session,
        instruction: Instruction,
    ) -> Result<Option<SentInstruction>> {
        let sent = sqlx::query(
            "INSERT INTO link_instruction (instance, seq, session_id, organization_id, body, sent_at)
             SELECT instance,
                    (SELECT COALESCE(MAX(seq), 0) + 1 FROM link_instruction
                      WHERE link_instruction.instance = session.instance),
                    id, organization_id, ?, ?
             FROM session
             WHERE id = ? AND instance IS NOT NULL
             RETURNING seq",
        )
        .bind(serde_json::to_string(&instruction)?)
        .bind(Timestamp::now().to_string())
        .bind(session.id.to_string())
        .fetch_optional(&mut *self.connection)
        .await
        .with_context(|| format!("sending an instruction to the session {}", session.id))?;

        Ok(sent.map(|sent| SentInstruction {
            seq: sent.get("seq"),
            session: session.id,
            instruction,
        }))
    }

    pub async fn instructions_after(
        &mut self,
        instance: &str,
        cursor: i64,
    ) -> Result<Vec<SentInstruction>> {
        sqlx::query(
            "SELECT seq, session_id, body
             FROM link_instruction
             WHERE instance = ? AND seq > ?
             ORDER BY seq",
        )
        .bind(instance)
        .bind(cursor)
        .fetch_all(&mut *self.connection)
        .await?
        .iter()
        .map(|row| {
            Ok(SentInstruction {
                seq: row.get("seq"),
                session: row.get::<String, _>("session_id").parse()?,
                instruction: serde_json::from_str(row.get("body"))?,
            })
        })
        .collect()
    }

    pub async fn first_turn(&mut self, session: &Session) -> Result<i64> {
        self.turn(session).await
    }

    /// `None` when the Session was not waiting, so a replayed prompt starts no second turn.
    pub async fn prompt_turn(&mut self, session: &Session) -> Result<Option<i64>> {
        let moved = sqlx::query("UPDATE session SET state = ? WHERE id = ? AND state = ?")
            .bind(SessionState::Working.as_str())
            .bind(session.id.to_string())
            .bind(SessionState::Waiting.as_str())
            .execute(&mut *self.connection)
            .await?;
        if moved.rows_affected() == 0 {
            return Ok(None);
        }

        self.touched.session(session);
        self.touched.queue(session.organization);

        self.turn(session).await.map(Some)
    }

    /// The Turn is anchored to the last Transcript entry before its prompt, so what the Agent
    /// says during it is what follows that, never anything said before.
    async fn turn(&mut self, session: &Session) -> Result<i64> {
        let prompted = sqlx::query(
            "INSERT INTO turn (session_id, organization_id, seq, prompted_at, from_seq)
             VALUES (
                 ?,
                 ?,
                 (SELECT COALESCE(MAX(seq), 0) + 1 FROM turn WHERE session_id = ?),
                 ?,
                 (SELECT COALESCE(MAX(seq), 0) FROM transcript_entry
                   WHERE workspace_id = (SELECT workspace_id FROM session WHERE id = ?))
             )
             RETURNING seq",
        )
        .bind(session.id.to_string())
        .bind(session.organization.to_string())
        .bind(session.id.to_string())
        .bind(Timestamp::now().to_string())
        .bind(session.id.to_string())
        .fetch_one(&mut *self.connection)
        .await
        .with_context(|| format!("prompting a turn of the session {}", session.id))?;

        Ok(prompted.get("seq"))
    }

    /// The seq of the Turn waiting on an answer and the Transcript position its prompt followed,
    /// or `None` when no Turn was waiting, so an answer replayed after a reconnect closes
    /// nothing twice.
    pub async fn answer_turn(&mut self, session: &Session) -> Result<Option<(i64, i64)>> {
        let answered = sqlx::query(
            "UPDATE turn SET answered_at = ?
             WHERE session_id = ? AND answered_at IS NULL
               AND EXISTS (SELECT 1 FROM session WHERE id = ? AND state = ?)
             RETURNING seq, from_seq",
        )
        .bind(Timestamp::now().to_string())
        .bind(session.id.to_string())
        .bind(session.id.to_string())
        .bind(SessionState::Working.as_str())
        .fetch_optional(&mut *self.connection)
        .await
        .with_context(|| format!("answering the turn of the session {}", session.id))?;

        if answered.is_some() {
            sqlx::query("UPDATE session SET state = ? WHERE id = ? AND state = ?")
                .bind(SessionState::Waiting.as_str())
                .bind(session.id.to_string())
                .bind(SessionState::Working.as_str())
                .execute(&mut *self.connection)
                .await?;

            self.touched.session(session);
            self.touched.queue(session.organization);
        }
        Ok(answered.map(|row| (row.get("seq"), row.get("from_seq"))))
    }

    pub async fn turns(&mut self, session: SessionId) -> Result<Vec<Turn>> {
        sqlx::query(
            "SELECT seq, prompted_at, answered_at
             FROM turn
             WHERE session_id = ?
             ORDER BY seq",
        )
        .bind(session.to_string())
        .fetch_all(&mut *self.connection)
        .await
        .with_context(|| format!("reading the turns of the session {session}"))?
        .iter()
        .map(|row| {
            Ok(Turn {
                seq: row.get("seq"),
                prompted_at: row.get::<String, _>("prompted_at").parse()?,
                answered_at: timestamp(row, "answered_at")?,
            })
        })
        .collect()
    }

    pub async fn occupying_slots(&mut self) -> Result<usize> {
        let row = sqlx::query("SELECT COUNT(*) AS occupying FROM session WHERE state = ?")
            .bind(SessionState::Working.as_str())
            .fetch_one(&mut *self.connection)
            .await
            .context("counting the sessions occupying an active-work slot")?;

        Ok(usize::try_from(row.get::<i64, _>("occupying"))?)
    }

    pub async fn oldest_held_input(
        &mut self,
        serialized: &[String],
    ) -> Result<Option<(Session, Timestamp)>> {
        let row = sqlx::query(concat!(held_input!(profile_free!()), " LIMIT 1"))
            .bind(SessionState::Waiting.as_str())
            .bind(serde_json::to_string(serialized)?)
            .bind(SessionState::Working.as_str())
            .fetch_optional(&mut *self.connection)
            .await
            .context("reading which waiting session has input held longest")?;

        let Some(row) = row else {
            return Ok(None);
        };
        let since = row.get::<String, _>("since").parse()?;

        Ok(Some((
            self.session(row.get::<String, _>("id").parse()?).await?,
            since,
        )))
    }
}

pub struct Supervisor {
    pub name: Option<String>,
    pub reached_at: Option<Timestamp>,
}

pub struct Linked {
    pub instance: String,
    pub workspace: WorkspaceId,
}

pub(crate) struct Unfinished {
    pub session: Session,
    pub held_input: bool,
}

/// A waiting Session still heartbeats and still holds its Instance.
pub(crate) fn live() -> Result<String> {
    Ok(serde_json::to_string(
        &SessionState::LIVE.map(SessionState::as_str),
    )?)
}

pub(crate) async fn read(connection: &mut SqliteConnection, id: WorkspaceId) -> Result<Workspace> {
    find(connection, id)
        .await?
        .with_context(|| format!("no workspace {id}"))
}

async fn find(connection: &mut SqliteConnection, id: WorkspaceId) -> Result<Option<Workspace>> {
    let Some(row) = sqlx::query(
        "SELECT name, organization_id, project_id, agent_id, subscription_profile_id, base, branch,
                correlation, state, opened_at, last_active_at, sealed_at, continues, event_record_id,
                started_by_participant
         FROM workspace
         WHERE id = ?",
    )
    .bind(id.to_string())
    .fetch_optional(&mut *connection)
    .await?
    else {
        return Ok(None);
    };

    let organization =
        organization::with_id(connection, row.get::<String, _>("organization_id").parse()?).await?;
    let project = project::with_id(
        connection,
        &organization,
        row.get::<String, _>("project_id").parse()?,
    )
    .await?;
    let opened_with = agent::with_id(
        connection,
        &organization,
        row.get::<String, _>("agent_id").parse()?,
    )
    .await?;
    let profile = match row.get::<Option<String>, _>("subscription_profile_id") {
        Some(id) => Some(profile::with_id(connection, id.parse()?).await?),
        None => None,
    };
    let repositories = sqlx::query(
        "SELECT url FROM workspace_repository WHERE workspace_id = ? ORDER BY position",
    )
    .bind(id.to_string())
    .fetch_all(&mut *connection)
    .await?
    .iter()
    .map(|row| row.get("url"))
    .collect();

    Ok(Some(Workspace {
        id,
        name: row.get("name"),
        organization,
        project,
        opened_with,
        profile,
        checkout: Checkout {
            repositories,
            base: row.get("base"),
            branch: row.get("branch"),
        },
        correlation: row.get("correlation"),
        state: row.get::<String, _>("state").parse()?,
        opened_at: row.get::<String, _>("opened_at").parse()?,
        last_active_at: row.get::<String, _>("last_active_at").parse()?,
        sealed_at: timestamp(&row, "sealed_at")?,
        continues: row
            .get::<Option<String>, _>("continues")
            .map(|sealed| sealed.parse())
            .transpose()?,
        started_by: match (
            row.get::<Option<String>, _>("event_record_id"),
            row.get::<Option<String>, _>("started_by_participant"),
        ) {
            (Some(event), _) => Some(StartedBy::Event(event.parse()?)),
            (None, Some(participant)) => Some(StartedBy::Participant(participant)),
            (None, None) => None,
        },
    }))
}

fn candidate(row: &SqliteRow) -> Candidate {
    Candidate {
        id: row.get("id"),
        name: row.get("name"),
    }
}

fn session(row: &SqliteRow) -> Result<Session> {
    let exit: Option<String> = row.get("exit");
    let connected_at: Option<String> = row.get("connected_at");

    Ok(Session {
        id: row.get::<String, _>("id").parse()?,
        name: row.get("name"),
        organization: row.get::<String, _>("organization_id").parse()?,
        workspace: row.get::<String, _>("workspace_id").parse()?,
        agent: Agent {
            id: row.get::<String, _>("agent_id").parse()?,
            organization: row.get::<String, _>("organization_id").parse()?,
            name: row.get("agent_name"),
            harness: row.get("harness"),
            model: row.get("model"),
        },
        state: row.get::<String, _>("state").parse()?,
        exit: exit
            .map(|status| Exit::read(&status, row.get("exit_because")))
            .transpose()?,
        outcome_message: row.get("outcome_message"),
        instance: row.get("instance"),
        supervisor: row.get("supervisor"),
        worked_model: row.get("worked_model"),
        enqueued_at: row.get::<String, _>("enqueued_at").parse()?,
        started_at: timestamp(row, "started_at")?,
        ended_at: timestamp(row, "ended_at")?,
        lease_expires_at: timestamp(row, "lease_expires_at")?,
        connected: match connected_at {
            Some(at) => Some(Connected {
                at: at.parse()?,
                version: row.get("supervisor_version"),
            }),
            None => None,
        },
        usage: usage(row),
    })
}

fn generated_name() -> String {
    const ADJECTIVES: &[&str] = &[
        "agile", "amber", "brisk", "bright", "calm", "clever", "coral", "crisp", "daring", "eager",
        "ember", "fable", "gentle", "golden", "grand", "happy", "hidden", "jolly", "keen", "kind",
        "lively", "lucky", "merry", "mighty", "nimble", "noble", "plucky", "proud", "quick",
        "quiet", "rapid", "silver",
    ];
    const NOUNS: &[&str] = &[
        "badger", "beacon", "cedar", "comet", "falcon", "fern", "fox", "harbor", "heron",
        "juniper", "kite", "lantern", "maple", "meadow", "otter", "owl", "pebble", "pioneer",
        "raven", "river", "robin", "sailor", "sparrow", "summit", "thistle", "valley", "willow",
        "wren", "yarrow", "zephyr", "acorn", "brook",
    ];

    let mut bytes = [0; 10];
    getrandom::fill(&mut bytes).expect("the operating system should have entropy to spare");
    let suffix: String = bytes[2..]
        .iter()
        .map(|byte| char::from(b'a' + byte % 26))
        .collect();
    format!(
        "{}-{}-{suffix}",
        ADJECTIVES[usize::from(bytes[0]) % ADJECTIVES.len()],
        NOUNS[usize::from(bytes[1]) % NOUNS.len()]
    )
}

fn kept(row: &SqliteRow) -> Result<Kept> {
    Ok(Kept {
        workspace: row.get::<String, _>("id").parse()?,
        instance: row.get("instance"),
        observed: row
            .get::<Option<String>, _>("observed")
            .map(|observed| serde_json::from_str(&observed))
            .transpose()?,
    })
}

fn usage(row: &SqliteRow) -> Option<Usage> {
    let context_used: Option<i64> = row.get("context_used");
    let currency: Option<String> = row.get("cost_currency");

    Some(Usage {
        context_used: context_used? as u64,
        context_size: row.get::<i64, _>("context_size") as u64,
        cost: match (row.get::<Option<f64>, _>("cost_amount"), currency) {
            (Some(amount), Some(currency)) => Some(Cost { amount, currency }),
            _ => None,
        },
    })
}
