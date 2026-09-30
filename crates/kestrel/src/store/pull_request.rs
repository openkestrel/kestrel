use anyhow::{Context as _, Result};
use jiff::Timestamp;
use sqlx::sqlite::SqliteRow;
use sqlx::{Row, SqliteConnection};

use crate::domain::{
    Event, Organization, OrganizationId, PullRequest, Workspace, WorkspaceId, WorkspaceState,
};
use crate::store::{due, integration, workspace};

pub enum Considered {
    Attached(WorkspaceId),
    Unmatched,
    Ambiguous,
}

pub struct PullRequests<'a> {
    connection: &'a mut SqliteConnection,
}

impl<'a> PullRequests<'a> {
    pub(crate) fn over(connection: &'a mut SqliteConnection) -> Self {
        Self { connection }
    }

    /// Only a signed GitHub Integration's: a generic webhook may name any type it likes.
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
               AND integration.signing_secret IS NOT NULL
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

    pub async fn open_on_branch(
        &mut self,
        organization: OrganizationId,
        branch: &str,
    ) -> Result<Vec<Workspace>> {
        let ids = sqlx::query(
            "SELECT id FROM workspace
             WHERE organization_id = ? AND state = ? AND branch = ?
             ORDER BY opened_at, id",
        )
        .bind(organization.to_string())
        .bind(WorkspaceState::Open.as_str())
        .bind(branch)
        .fetch_all(&mut *self.connection)
        .await
        .with_context(|| format!("reading the open workspaces on the branch {branch}"))?
        .iter()
        .map(|row| Ok(row.get::<String, _>("id").parse()?))
        .collect::<Result<Vec<WorkspaceId>>>()?;

        let mut open = Vec::with_capacity(ids.len());
        for id in ids {
            open.push(workspace::read(&mut *self.connection, id).await?);
        }

        Ok(open)
    }

    /// A value already fresher at its source is kept, so a delivery that arrives late cannot
    /// roll the Workspace's state back.
    pub async fn learn(&mut self, workspace: &Workspace, learned: &PullRequest) -> Result<()> {
        sqlx::query(
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
             WHERE excluded.updated_at > pull_request.updated_at",
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
        .execute(&mut *self.connection)
        .await
        .with_context(|| {
            format!(
                "recording pull request {} on the workspace {}",
                learned.url, workspace.id
            )
        })?;

        Ok(())
    }

    pub async fn consider(&mut self, event: &Event, considered: &Considered) -> Result<()> {
        let (outcome, workspace) = match considered {
            Considered::Attached(workspace) => ("attached", Some(workspace.to_string())),
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

        Ok(())
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

    /// Only a signed Integration is delivered pull request Events; one that is polled reads the
    /// issue timeline, which never says a pull request opened.
    pub async fn watched(&mut self, organization: &Organization) -> Result<Vec<String>> {
        sqlx::query(
            "SELECT repository FROM integration
             WHERE organization_id = ? AND kind = 'github' AND inbound = TRUE
               AND signing_secret IS NOT NULL",
        )
        .bind(organization.id.to_string())
        .fetch_all(&mut *self.connection)
        .await
        .context("reading the repositories an inbound integration watches")?
        .iter()
        .map(|row| Ok(row.get("repository")))
        .collect()
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
