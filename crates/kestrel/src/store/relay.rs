use crate::timer::Clock;
use anyhow::Result;
use jiff::Timestamp;
use kestrel_operator_types as wire;
use sqlx::{Row, SqliteConnection};

use crate::domain::{OrganizationId, SubscriptionProfileId};
use crate::profile::Entry;
use crate::sign_in::relay::{Attempt, terminal};

pub struct Relays<'a> {
    connection: &'a mut SqliteConnection,
    clock: &'a Clock,
}

impl<'a> Relays<'a> {
    pub(crate) fn over(connection: &'a mut SqliteConnection, clock: &'a Clock) -> Self {
        Self { connection, clock }
    }

    pub async fn open(&mut self, attempt: &Attempt) -> Result<()> {
        let view = &attempt.view;
        sqlx::query(
            "INSERT INTO sign_in_relay
            (id, organization_id, profile_id, kind, name, harness, method, state, baseline,
             version, deadline, snapshot) VALUES (?, ?, ?, ?, ?, ?, ?, 'starting', ?, 1, ?, ?)",
        )
        .bind(&view.id)
        .bind(attempt.organization.to_string())
        .bind(attempt.profile.to_string())
        .bind(attempt.entry.kind.as_str())
        .bind(&attempt.entry.name)
        .bind(&view.harness)
        .bind(&view.method)
        .bind(attempt.baseline)
        .bind(view.deadline.parse::<Timestamp>()?.as_second())
        .bind(serde_json::to_string(view)?)
        .execute(&mut *self.connection)
        .await?;
        Ok(())
    }

    pub async fn get(&mut self, id: &str) -> Result<Option<Attempt>> {
        sqlx::query("SELECT * FROM sign_in_relay WHERE id = ?")
            .bind(id)
            .fetch_optional(&mut *self.connection)
            .await?
            .as_ref()
            .map(attempt)
            .transpose()
    }

    pub async fn active(
        &mut self,
        profile: SubscriptionProfileId,
        entry: &Entry,
    ) -> Result<Option<Attempt>> {
        sqlx::query(
            "SELECT * FROM sign_in_relay WHERE profile_id = ? AND kind = ? AND name = ?
            AND state IN ('starting', 'awaiting_browser', 'awaiting_code', 'saving')",
        )
        .bind(profile.to_string())
        .bind(entry.kind.as_str())
        .bind(&entry.name)
        .fetch_optional(&mut *self.connection)
        .await?
        .as_ref()
        .map(attempt)
        .transpose()
    }

    pub async fn claim(&mut self) -> Result<Option<Attempt>> {
        sqlx::query(
            "UPDATE sign_in_relay SET claimed = 1 WHERE id =
            (SELECT id FROM sign_in_relay WHERE state = 'starting' AND claimed = 0 LIMIT 1)
            RETURNING *",
        )
        .fetch_optional(&mut *self.connection)
        .await?
        .as_ref()
        .map(attempt)
        .transpose()
    }

    pub async fn update(
        &mut self,
        attempt: &mut Attempt,
        progress: wire::SignInRelayProgress,
    ) -> Result<()> {
        let expected = attempt.view.version;
        attempt.view.version += 1;
        attempt.view.finished_at = terminal(&progress).then(|| self.clock.now().to_string());
        attempt.view.progress = progress;
        let updated = sqlx::query("UPDATE sign_in_relay SET state = ?, version = ?, snapshot = ?, finished_at = ? WHERE id = ? AND version = ?")
            .bind(attempt.state()).bind(attempt.view.version)
            .bind(serde_json::to_string(&attempt.view)?)
            .bind(attempt.view.finished_at.as_ref().map(|at| at.parse::<Timestamp>().map(|at| at.as_second())).transpose()?)
            .bind(&attempt.view.id).bind(expected).execute(&mut *self.connection).await?;
        anyhow::ensure!(
            updated.rows_affected() == 1,
            "the sign-in relay changed before its transition was saved"
        );
        Ok(())
    }

    pub async fn take_completion(&mut self, id: &str) -> Result<bool> {
        Ok(sqlx::query(
            "UPDATE sign_in_relay SET state = 'completed' WHERE id = ?
            AND state = 'saving' AND deadline > ?",
        )
        .bind(id)
        .bind(self.clock.now().as_second())
        .execute(&mut *self.connection)
        .await?
        .rows_affected()
            == 1)
    }

    pub async fn unfinished(&mut self) -> Result<Vec<Attempt>> {
        sqlx::query("SELECT * FROM sign_in_relay WHERE state IN ('starting', 'awaiting_browser', 'awaiting_code', 'saving') LIMIT 32")
            .fetch_all(&mut *self.connection).await?.iter().map(attempt).collect()
    }

    pub async fn unattended(&mut self) -> Result<Vec<Attempt>> {
        sqlx::query("SELECT * FROM sign_in_relay WHERE cleaned = 0 AND claimed = 0 ORDER BY deadline LIMIT 32")
            .fetch_all(&mut *self.connection).await?.iter().map(attempt).collect()
    }

    pub async fn release_all_cleanup(&mut self) -> Result<()> {
        sqlx::query("UPDATE sign_in_relay SET claimed = 0 WHERE cleaned = 0")
            .execute(&mut *self.connection)
            .await?;
        Ok(())
    }

    pub async fn release_cleanup(&mut self, id: &str) -> Result<()> {
        sqlx::query("UPDATE sign_in_relay SET claimed = 0 WHERE id = ?")
            .bind(id)
            .execute(&mut *self.connection)
            .await?;
        Ok(())
    }

    pub async fn cleaned(&mut self, id: &str) -> Result<()> {
        sqlx::query("UPDATE sign_in_relay SET cleaned = 1 WHERE id = ?")
            .bind(id)
            .execute(&mut *self.connection)
            .await?;
        Ok(())
    }

    pub async fn forget_old_snapshots(&mut self) -> Result<()> {
        sqlx::query(
            "UPDATE sign_in_relay SET snapshot = NULL WHERE id IN
             (SELECT id FROM sign_in_relay WHERE finished_at <= ? AND cleaned = 1
              AND snapshot IS NOT NULL ORDER BY finished_at LIMIT 32)",
        )
        .bind(self.clock.now().as_second() - 86400)
        .execute(&mut *self.connection)
        .await?;
        Ok(())
    }
}

pub(crate) async fn cancel_slot(
    connection: &mut SqliteConnection,
    clock: &Clock,
    profile: SubscriptionProfileId,
    entry: &Entry,
) -> Result<()> {
    let mut relays = Relays::over(connection, clock);
    if let Some(mut attempt) = relays.active(profile, entry).await? {
        relays
            .update(
                &mut attempt,
                wire::SignInRelayProgress::RelayCancelled(wire::RelayCancelled {
                    state: Default::default(),
                }),
            )
            .await?;
    }
    Ok(())
}

fn attempt(row: &sqlx::sqlite::SqliteRow) -> Result<Attempt> {
    let snapshot: Option<String> = row.get("snapshot");
    Ok(Attempt {
        organization: row
            .get::<String, _>("organization_id")
            .parse::<OrganizationId>()?,
        profile: row.get::<String, _>("profile_id").parse()?,
        entry: Entry {
            kind: row.get::<String, _>("kind").parse()?,
            name: row.get("name"),
        },
        baseline: row.get("baseline"),
        view: match snapshot {
            Some(snapshot) => serde_json::from_str(&snapshot)?,
            None => wire::SignInRelay {
                id: row.get("id"),
                organization: String::new(),
                harness: row.get("harness"),
                method: row.get("method"),
                destination: wire::SignInHolding {
                    resource: wire::SignInHoldingResource::SubscriptionProfile,
                    reference: String::new(),
                    fills: crate::catalogue::sign_in_method(
                        "read_sign_in_relay",
                        &row.get::<String, _>("harness"),
                        &row.get::<String, _>("method"),
                    )?
                    .fills
                    .clone(),
                },
                version: row.get("version"),
                created_at: String::new(),
                deadline: Timestamp::from_second(row.get("deadline"))?.to_string(),
                finished_at: Some(Timestamp::from_second(row.get("finished_at"))?.to_string()),
                progress: wire::SignInRelayProgress::RelayExpired(wire::RelayExpired {
                    state: Default::default(),
                }),
            },
        },
    })
}
