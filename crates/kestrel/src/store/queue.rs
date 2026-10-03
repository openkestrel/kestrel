use anyhow::{Context as _, Result};
use jiff::Timestamp;
use sqlx::{Row, SqliteConnection};

use crate::domain::{Exit, Organization, SessionId, SessionState};
use crate::fanout::Touched;
use crate::store::workspace::{UNSATISFIED_BLOCKER, held_input, live, occupying, profile_held};

/// What a work role that can dispatch recorded on start: the Active-Work Slot limit it
/// enforces, the harnesses it dispatches one Session at a time, and the Compute driver it
/// provisions with. A restart with new flags replaces it.
pub struct Recorded {
    pub active_work_slots: usize,
    pub serialized_harnesses: Vec<String>,
    pub driver: String,
}

pub struct Queue<'a> {
    connection: &'a mut SqliteConnection,
    touched: &'a mut Touched,
}

impl<'a> Queue<'a> {
    pub(crate) fn over(connection: &'a mut SqliteConnection, touched: &'a mut Touched) -> Self {
        Self {
            connection,
            touched,
        }
    }

    /// `None` says no work role is dispatching.
    pub async fn recorded(&mut self) -> Result<Option<Recorded>> {
        let Some(row) =
            sqlx::query("SELECT active_work_slots, serialized_harnesses, driver FROM work_role")
                .fetch_optional(&mut *self.connection)
                .await
                .context("reading what the work role recorded")?
        else {
            return Ok(None);
        };

        Ok(Some(Recorded {
            active_work_slots: usize::try_from(row.get::<i64, _>("active_work_slots"))?,
            serialized_harnesses: serde_json::from_str(
                &row.get::<String, _>("serialized_harnesses"),
            )?,
            driver: row.get("driver"),
        }))
    }

    pub async fn record(
        &mut self,
        slots: usize,
        serialized: &[String],
        driver: &str,
    ) -> Result<()> {
        let serialized = serde_json::to_string(serialized)?;

        sqlx::query("DELETE FROM work_role")
            .execute(&mut *self.connection)
            .await
            .context("forgetting what a work role recorded")?;
        sqlx::query(
            "INSERT INTO work_role (active_work_slots, serialized_harnesses, driver)
             VALUES (?, ?, ?)",
        )
        .bind(slots as i64)
        .bind(&serialized)
        .bind(driver)
        .execute(&mut *self.connection)
        .await
        .with_context(|| {
            format!("recording a work role that dispatches {slots} slots in {driver}")
        })?;

        self.touched.every_queue();

        Ok(())
    }

    /// Every blocking Session the Organization's queued ones still wait on, named: a blocker
    /// that has not ended successfully.
    pub async fn waiting_on(
        &mut self,
        organization: &Organization,
    ) -> Result<Vec<(String, String)>> {
        Ok(sqlx::query(sqlx::AssertSqlSafe(format!(
            "SELECT d.session_id AS queued, b.name AS blocker
             FROM session_dependency AS d
             JOIN session AS b ON b.id = d.blocker_id
             WHERE d.organization_id = ?
               AND {UNSATISFIED_BLOCKER}
             ORDER BY d.session_id, b.enqueued_at, b.id"
        )))
        .bind(organization.id.to_string())
        .bind(SessionState::Ended.as_str())
        .bind(Exit::Succeeded.status())
        .fetch_all(&mut *self.connection)
        .await
        .context("reading which queued sessions are blocked on dependencies")?
        .iter()
        .map(|row| {
            (
                row.get::<String, _>("queued"),
                row.get::<String, _>("blocker"),
            )
        })
        .collect())
    }

    /// The Sessions that occupy an Active-Work Slot, `true` when the Organization named holds
    /// the one that occupies it.
    pub async fn occupying(&mut self, organization: &Organization) -> Result<Vec<(String, bool)>> {
        Ok(sqlx::query(
            "SELECT s.name, s.organization_id = ? AS ours
             FROM session AS s
             WHERE s.state IN (SELECT value FROM json_each(?))
             ORDER BY s.enqueued_at, s.id",
        )
        .bind(organization.id.to_string())
        .bind(occupying()?)
        .fetch_all(&mut *self.connection)
        .await
        .context("reading which sessions occupy an active-work slot")?
        .iter()
        .map(|row| (row.get("name"), row.get("ours")))
        .collect())
    }

    /// The Instances the Organization counts against its live limit, apart from what its
    /// Workspaces keep and what is being archived: live Sessions on a Workspace that has
    /// kept none.
    pub async fn instances_in_flight(
        &mut self,
        organization: &Organization,
    ) -> Result<Vec<String>> {
        Ok(sqlx::query(
            "SELECT s.instance
             FROM session AS s
             JOIN workspace AS w ON w.id = s.workspace_id
             WHERE s.organization_id = ?
               AND s.state IN (SELECT value FROM json_each(?))
               AND w.instance IS NULL
               AND s.instance IS NOT NULL
             ORDER BY s.enqueued_at, s.id",
        )
        .bind(organization.id.to_string())
        .bind(live()?)
        .fetch_all(&mut *self.connection)
        .await
        .context("reading the instances live sessions execute on")?
        .iter()
        .map(|row| row.get("instance"))
        .collect())
    }

    /// The Instances the Organization keeps no Workspace for any more: idle ones waiting to
    /// be archived, some retained for Unpublished Work.
    pub async fn instances_leaving(&mut self, organization: &Organization) -> Result<Vec<String>> {
        Ok(sqlx::query(
            "SELECT instance FROM instance_archive
                 WHERE organization_id = ?
                 ORDER BY queued_at, instance",
        )
        .bind(organization.id.to_string())
        .fetch_all(&mut *self.connection)
        .await
        .context("reading the instances waiting to be archived")?
        .iter()
        .map(|row| row.get("instance"))
        .collect())
    }

    pub async fn profiles_held(
        &mut self,
        organization: &Organization,
        serialized: &[String],
    ) -> Result<Vec<(SessionId, String, String)>> {
        sqlx::query(concat!(
            "SELECT s.id AS held, a.name AS holder,
                    (SELECT name FROM subscription_profile WHERE id = w.subscription_profile_id)
                        AS profile
             FROM session AS s, ",
            profile_held!(),
            "
               AND s.organization_id = ?
               AND s.state IN (?, ?)
             ORDER BY s.id, a.enqueued_at, a.id"
        ))
        .bind(serde_json::to_string(serialized)?)
        .bind(occupying()?)
        .bind(organization.id.to_string())
        .bind(SessionState::Queued.as_str())
        .bind(SessionState::Waiting.as_str())
        .fetch_all(&mut *self.connection)
        .await
        .context("reading which sessions a subscription profile holds back")?
        .iter()
        .map(|row| {
            Ok((
                row.get::<String, _>("held").parse()?,
                row.get("profile"),
                row.get("holder"),
            ))
        })
        .collect()
    }

    pub async fn held_input(
        &mut self,
        organization: &Organization,
    ) -> Result<Vec<(SessionId, Timestamp)>> {
        sqlx::query(held_input!("s.organization_id = ?"))
            .bind(SessionState::Waiting.as_str())
            .bind(organization.id.to_string())
            .fetch_all(&mut *self.connection)
            .await
            .context("reading the input held for waiting sessions")?
            .iter()
            .map(|row| {
                Ok((
                    row.get::<String, _>("id").parse()?,
                    row.get::<String, _>("since").parse()?,
                ))
            })
            .collect()
    }
}
