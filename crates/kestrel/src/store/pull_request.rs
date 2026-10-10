use anyhow::{Context as _, Result};
use jiff::Timestamp;
use sqlx::sqlite::SqliteRow;
use sqlx::{Row, SqliteConnection};

use crate::domain::{
    Event, EventRecordId, Organization, OrganizationId, PullRequest, Workspace, WorkspaceId,
    WorkspaceState,
};
use crate::fanout::Touched;
use crate::store::{due, integration, workspace};

pub enum Considered {
    Attached(WorkspaceId),
    Sealed,
    Unmatched,
    Ambiguous,
}

pub struct Consideration {
    pub outcome: String,
    pub candidates: Vec<(WorkspaceId, WorkspaceState)>,
}

#[derive(Clone, Copy)]
enum Freshness {
    Newer,
    /// What ties with a value is kept unless the source settled the tie.
    AtLeast,
}

pub struct PullRequests<'a> {
    connection: &'a mut SqliteConnection,
    touched: &'a mut Touched,
}

impl<'a> PullRequests<'a> {
    pub(crate) fn over(connection: &'a mut SqliteConnection, touched: &'a mut Touched) -> Self {
        Self {
            connection,
            touched,
        }
    }

    /// Only a GitHub Integration's: a generic webhook may name any type it likes.
    pub async fn unconsidered(&mut self, types: &[&str], limit: usize) -> Result<Vec<Event>> {
        sqlx::query(
            "SELECT event.record_id, event.organization_id, event.integration_id, event.id,
                    event.source, event.specversion, event.type, event.subject, event.time,
                    event.data, event.recorded_at
             FROM event
             JOIN integration ON integration.id = event.integration_id
             LEFT JOIN pull_request_attachment AS considered
                 ON considered.event_record_id = event.record_id
             WHERE integration.kind = 'github' AND integration.inbound = TRUE
               AND event.type IN (SELECT value FROM json_each(?))
               AND considered.event_record_id IS NULL
             ORDER BY event.recorded_at, event.record_id
             LIMIT ?",
        )
        .bind(serde_json::to_string(types)?)
        .bind(i64::try_from(limit)?)
        .fetch_all(&mut *self.connection)
        .await
        .context("reading the pull request events not yet considered")?
        .iter()
        .map(integration::event)
        .collect()
    }

    /// Sealed ones too: those a sealed Workspace matched are recorded rather than attached to.
    pub async fn on_branch(
        &mut self,
        organization: OrganizationId,
        branch: &str,
    ) -> Result<Vec<Workspace>> {
        let ids = sqlx::query(
            "SELECT id FROM workspace
             WHERE organization_id = ? AND branch = ?
             ORDER BY opened_at, id",
        )
        .bind(organization.to_string())
        .bind(branch)
        .fetch_all(&mut *self.connection)
        .await
        .with_context(|| format!("reading the workspaces on the branch {branch}"))?
        .iter()
        .map(|row| Ok(row.get::<String, _>("id").parse()?))
        .collect::<Result<Vec<WorkspaceId>>>()?;

        let mut on_branch = Vec::with_capacity(ids.len());
        for id in ids {
            on_branch.push(workspace::read(&mut *self.connection, id).await?);
        }

        Ok(on_branch)
    }

    /// A value already fresher at its source is kept, so a delivery that arrives late cannot
    /// roll the Workspace's state back.
    pub async fn learn(&mut self, workspace: &Workspace, learned: &PullRequest) -> Result<()> {
        self.record(workspace, learned, Freshness::Newer).await
    }

    /// The source's answer replaces what it ties with, for a tie the Integration itself was read
    /// back for.
    pub async fn reconcile(&mut self, workspace: &Workspace, learned: &PullRequest) -> Result<()> {
        self.record(workspace, learned, Freshness::AtLeast).await
    }

    /// False for an observation already held: a repeated delivery appends nothing.
    pub async fn observe(
        &mut self,
        workspace: &Workspace,
        learned: &PullRequest,
        action: &str,
    ) -> Result<bool> {
        let recorded = sqlx::query(
            "INSERT INTO pull_request_observation
                 (workspace_id, url, action, state, head_revision, updated_at, event_record_id)
             VALUES (?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT (workspace_id, url, action, state, head_revision, updated_at) DO NOTHING",
        )
        .bind(workspace.id.to_string())
        .bind(&learned.url)
        .bind(action)
        .bind(learned.state.as_str())
        .bind(&learned.head_revision)
        .bind(due(learned.updated_at))
        .bind(learned.event.to_string())
        .execute(&mut *self.connection)
        .await
        .with_context(|| {
            format!(
                "recording the observed pull request {} on the workspace {}",
                learned.url, workspace.id
            )
        })?;

        Ok(recorded.rows_affected() > 0)
    }

    pub async fn consider(
        &mut self,
        event: &Event,
        considered: &Considered,
        candidates: &[(WorkspaceId, WorkspaceState)],
    ) -> Result<()> {
        let (outcome, workspace) = match considered {
            Considered::Attached(workspace) => ("attached", Some(workspace.to_string())),
            Considered::Sealed => ("sealed", None),
            Considered::Unmatched => ("unmatched", None),
            Considered::Ambiguous => ("ambiguous", None),
        };

        sqlx::query(
            "INSERT INTO pull_request_attachment
                 (event_record_id, organization_id, outcome, workspace_id, considered_at)
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(event.record_id.to_string())
        .bind(event.organization.to_string())
        .bind(outcome)
        .bind(workspace)
        .bind(Timestamp::now().to_string())
        .execute(&mut *self.connection)
        .await
        .with_context(|| {
            format!(
                "recording what the pull request event {} matched",
                event.record_id
            )
        })?;

        for (workspace, state) in candidates {
            sqlx::query(
                "INSERT INTO pull_request_candidate (event_record_id, workspace_id, state)
                 VALUES (?, ?, ?)",
            )
            .bind(event.record_id.to_string())
            .bind(workspace.to_string())
            .bind(state.as_str())
            .execute(&mut *self.connection)
            .await
            .with_context(|| {
                format!(
                    "recording the workspace {workspace} the pull request event {} matched",
                    event.record_id
                )
            })?;
        }

        Ok(())
    }

    pub async fn considered(&mut self, event: EventRecordId) -> Result<Option<Consideration>> {
        let outcome = sqlx::query_scalar::<_, String>(
            "SELECT outcome FROM pull_request_attachment WHERE event_record_id = ?",
        )
        .bind(event.to_string())
        .fetch_optional(&mut *self.connection)
        .await
        .with_context(|| format!("reading what the pull request event {event} matched"))?;
        let Some(outcome) = outcome else {
            return Ok(None);
        };

        let candidates = sqlx::query(
            "SELECT workspace_id, state FROM pull_request_candidate
             WHERE event_record_id = ?
             ORDER BY workspace_id",
        )
        .bind(event.to_string())
        .fetch_all(&mut *self.connection)
        .await
        .with_context(|| format!("reading the candidates of the pull request event {event}"))?
        .iter()
        .map(|row| {
            Ok((
                row.get::<String, _>("workspace_id").parse()?,
                row.get::<String, _>("state").parse()?,
            ))
        })
        .collect::<Result<Vec<(WorkspaceId, WorkspaceState)>>>()?;

        Ok(Some(Consideration {
            outcome,
            candidates,
        }))
    }

    pub async fn of(&mut self, workspace: WorkspaceId) -> Result<Vec<PullRequest>> {
        sqlx::query(
            "SELECT repository, number, url, title, state, head_branch, head_revision,
                    updated_at, event_record_id
             FROM pull_request
             WHERE workspace_id = ?
             ORDER BY repository, number",
        )
        .bind(workspace.to_string())
        .fetch_all(&mut *self.connection)
        .await
        .with_context(|| format!("reading the pull requests of the workspace {workspace}"))?
        .iter()
        .map(pull_request)
        .collect()
    }

    pub async fn value(
        &mut self,
        workspace: WorkspaceId,
        url: &str,
    ) -> Result<Option<PullRequest>> {
        sqlx::query(
            "SELECT repository, number, url, title, state, head_branch, head_revision,
                    updated_at, event_record_id
             FROM pull_request
             WHERE workspace_id = ? AND url = ?",
        )
        .bind(workspace.to_string())
        .bind(url)
        .fetch_optional(&mut *self.connection)
        .await
        .with_context(|| format!("reading the pull request {url} of the workspace {workspace}"))?
        .as_ref()
        .map(pull_request)
        .transpose()
    }

    pub async fn watched(&mut self, organization: &Organization) -> Result<Vec<String>> {
        sqlx::query(
            "SELECT repository FROM integration
             WHERE organization_id = ? AND kind = 'github' AND inbound = TRUE
               AND state <> 'retired'",
        )
        .bind(organization.id.to_string())
        .fetch_all(&mut *self.connection)
        .await
        .context("reading the repositories an inbound integration watches")?
        .iter()
        .map(|row| Ok(row.get("repository")))
        .collect()
    }

    async fn record(
        &mut self,
        workspace: &Workspace,
        learned: &PullRequest,
        freshness: Freshness,
    ) -> Result<()> {
        let replacing_ties = matches!(freshness, Freshness::AtLeast);
        let learned_at = sqlx::query(
            "INSERT INTO pull_request
                 (workspace_id, organization_id, repository, number, url, title, state,
                  head_branch, head_revision, updated_at, event_record_id)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT (workspace_id, url) DO UPDATE SET
                 title = excluded.title,
                 state = excluded.state,
                 head_branch = excluded.head_branch,
                 head_revision = excluded.head_revision,
                 updated_at = excluded.updated_at,
                 event_record_id = excluded.event_record_id
             WHERE excluded.updated_at > pull_request.updated_at
                OR (excluded.updated_at = pull_request.updated_at AND ?)",
        )
        .bind(workspace.id.to_string())
        .bind(workspace.organization.id.to_string())
        .bind(&learned.repository)
        .bind(learned.number)
        .bind(&learned.url)
        .bind(&learned.title)
        .bind(learned.state.as_str())
        .bind(&learned.head_branch)
        .bind(&learned.head_revision)
        .bind(due(learned.updated_at))
        .bind(learned.event.to_string())
        .bind(replacing_ties)
        .execute(&mut *self.connection)
        .await
        .with_context(|| {
            format!(
                "recording pull request {} on the workspace {}",
                learned.url, workspace.id
            )
        })?;

        if learned_at.rows_affected() > 0 {
            self.touched.workspace(workspace);
        }

        Ok(())
    }
}

fn pull_request(row: &SqliteRow) -> Result<PullRequest> {
    Ok(PullRequest {
        repository: row.get("repository"),
        number: row.get("number"),
        url: row.get("url"),
        title: row.get("title"),
        state: row.get::<String, _>("state").parse()?,
        head_branch: row.get("head_branch"),
        head_revision: row.get("head_revision"),
        updated_at: row.get::<String, _>("updated_at").parse()?,
        event: row.get::<String, _>("event_record_id").parse()?,
    })
}
