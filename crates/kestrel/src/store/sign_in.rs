use std::collections::BTreeMap;

use anyhow::{Context as _, Result};
use jiff::Timestamp;
use sqlx::{Row, SqliteConnection};

use crate::domain::{Organization, Session, SessionId, SessionState, SubscriptionProfile};
use crate::profile::{Entry, Kind};
use crate::sign_in::credential_use::CredentialUse;
use crate::sign_in::{Authentication, ModelUse, Source, State, UseResult, UseSource};
use crate::store::workspace::occupying;

pub struct SignIns<'a> {
    connection: &'a mut SqliteConnection,
}

/// Every write of material calls this in its own transaction, so no material is held without a
/// revision and the evidence that revision has so far.
pub(crate) async fn mint(
    connection: &mut SqliteConnection,
    authentication: &Authentication,
) -> Result<i64> {
    let revision: i64 = sqlx::query_scalar(
        "INSERT INTO material_revision (written_at) VALUES (?) RETURNING revision",
    )
    .bind(Timestamp::now().to_string())
    .fetch_one(&mut *connection)
    .await
    .context("minting a material revision")?;

    sqlx::query(
        "INSERT INTO authentication_evidence (revision, state, source, observed_at, provider_check)
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind(revision)
    .bind(authentication.state.as_str())
    .bind(authentication.source.as_str())
    .bind(authentication.observed_at.to_string())
    .bind(
        authentication
            .provider_check
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?,
    )
    .execute(&mut *connection)
    .await
    .context("recording what a material revision is known to do")?;

    Ok(revision)
}

pub(crate) async fn mint_refreshed(connection: &mut SqliteConnection, from: i64) -> Result<i64> {
    let revision = mint(connection, &Authentication::unchecked(Source::Refresh)).await?;
    sqlx::query("UPDATE material_revision SET refreshed_from = ? WHERE revision = ?")
        .bind(from)
        .bind(revision)
        .execute(&mut *connection)
        .await
        .context("recording which revision a refresh advanced")?;

    Ok(revision)
}

impl<'a> SignIns<'a> {
    pub(crate) fn over(connection: &'a mut SqliteConnection) -> Self {
        Self { connection }
    }

    pub async fn authentication(&mut self, revision: i64) -> Result<Authentication> {
        let row = sqlx::query(
            "SELECT state, source, observed_at, provider_check
             FROM authentication_evidence
             WHERE revision = ?",
        )
        .bind(revision)
        .fetch_one(&mut *self.connection)
        .await
        .with_context(|| format!("reading what material revision {revision} is known to do"))?;

        Ok(Authentication {
            state: row.get::<String, _>("state").parse()?,
            source: row.get::<String, _>("source").parse()?,
            observed_at: row.get::<String, _>("observed_at").parse()?,
            provider_check: row
                .get::<Option<String>, _>("provider_check")
                .map(|checked| serde_json::from_str(&checked))
                .transpose()?,
        })
    }

    pub async fn model_use(&mut self, revision: i64, harness: &str) -> Result<Vec<ModelUse>> {
        sqlx::query(
            "SELECT harness, model, image, result, source, observed_at
             FROM model_use_evidence
             WHERE revision = ? AND harness = ?
             ORDER BY model",
        )
        .bind(revision)
        .bind(harness)
        .fetch_all(&mut *self.connection)
        .await?
        .iter()
        .map(|row| {
            Ok(ModelUse {
                harness: row.get("harness"),
                model: row.get("model"),
                image: row.get("image"),
                result: row.get::<String, _>("result").parse()?,
                source: row.get::<String, _>("source").parse()?,
                observed_at: row.get::<String, _>("observed_at").parse()?,
            })
        })
        .collect()
    }

    pub async fn refreshed_from(&mut self, revision: i64) -> Result<Option<i64>> {
        Ok(
            sqlx::query_scalar("SELECT refreshed_from FROM material_revision WHERE revision = ?")
                .bind(revision)
                .fetch_one(&mut *self.connection)
                .await?,
        )
    }

    pub async fn observe(&mut self, revision: i64, state: State, source: Source) -> Result<()> {
        sqlx::query(
            "UPDATE authentication_evidence
             SET state = ?, source = ?, observed_at = ?
             WHERE revision = ?",
        )
        .bind(state.as_str())
        .bind(source.as_str())
        .bind(Timestamp::now().to_string())
        .bind(revision)
        .execute(&mut *self.connection)
        .await
        .with_context(|| format!("recording what material revision {revision} did"))?;

        Ok(())
    }

    pub async fn record_use(
        &mut self,
        revision: i64,
        harness: &str,
        model: &str,
        result: UseResult,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO model_use_evidence (revision, harness, model, result, source, observed_at)
             VALUES (?, ?, ?, ?, ?, ?)
             ON CONFLICT (revision, harness, model)
             DO UPDATE SET image = NULL, result = excluded.result, source = excluded.source,
                           observed_at = excluded.observed_at",
        )
        .bind(revision)
        .bind(harness)
        .bind(model)
        .bind(result.as_str())
        .bind(UseSource::Session.as_str())
        .bind(Timestamp::now().to_string())
        .execute(&mut *self.connection)
        .await
        .with_context(|| format!("recording what {harness} did with {model}"))?;

        Ok(())
    }

    /// Replaces what an earlier spawn of the same Session was handed.
    pub async fn record_supplied(
        &mut self,
        session: &Session,
        supplied: &BTreeMap<(Kind, String), (Option<&SubscriptionProfile>, i64)>,
    ) -> Result<()> {
        sqlx::query("DELETE FROM session_material WHERE session_id = ?")
            .bind(session.id.to_string())
            .execute(&mut *self.connection)
            .await?;

        let handed_at = Timestamp::now().to_string();
        for ((kind, name), (profile, revision)) in supplied {
            sqlx::query(
                "INSERT INTO session_material
                     (session_id, organization_id, profile_id, kind, name, revision, handed_at)
                 VALUES (?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(session.id.to_string())
            .bind(session.organization.to_string())
            .bind(profile.map(|profile| profile.id.to_string()))
            .bind(kind.as_str())
            .bind(name)
            .bind(revision)
            .bind(&handed_at)
            .execute(&mut *self.connection)
            .await
            .with_context(|| format!("recording what the session {} was handed", session.id))?;
        }

        Ok(())
    }

    pub async fn supplied_revision(
        &mut self,
        session: &Session,
        entry: &Entry,
    ) -> Result<Option<i64>> {
        Ok(sqlx::query_scalar(
            "SELECT revision FROM session_material
             WHERE session_id = ? AND kind = ? AND name = ?",
        )
        .bind(session.id.to_string())
        .bind(entry.kind.as_str())
        .bind(&entry.name)
        .fetch_optional(&mut *self.connection)
        .await?)
    }

    pub async fn advance_supplied(
        &mut self,
        session: &Session,
        entry: &Entry,
        revision: i64,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE session_material SET revision = ?
             WHERE session_id = ? AND kind = ? AND name = ?",
        )
        .bind(revision)
        .bind(session.id.to_string())
        .bind(entry.kind.as_str())
        .bind(&entry.name)
        .execute(&mut *self.connection)
        .await?;

        Ok(())
    }

    pub async fn session_using(
        &mut self,
        profile: &SubscriptionProfile,
        harness: &str,
    ) -> Result<Option<SessionId>> {
        sqlx::query_scalar::<_, String>(
            "SELECT s.id FROM session AS s
             JOIN workspace AS w ON w.id = s.workspace_id
             WHERE w.subscription_profile_id = ? AND s.harness = ?
               AND s.state IN (SELECT value FROM json_each(?))
             ORDER BY s.enqueued_at, s.id
             LIMIT 1",
        )
        .bind(profile.id.to_string())
        .bind(harness)
        .bind(occupying()?)
        .fetch_optional(&mut *self.connection)
        .await
        .with_context(|| format!("reading what uses the profile {}", profile.name))?
        .map(|id| Ok(id.parse()?))
        .transpose()
    }

    pub async fn holder(
        &mut self,
        profile: &SubscriptionProfile,
        harness: &str,
    ) -> Result<Option<String>> {
        Ok(sqlx::query_scalar(
            "SELECT holder FROM credential_use WHERE profile_id = ? AND harness = ?",
        )
        .bind(profile.id.to_string())
        .bind(harness)
        .fetch_optional(&mut *self.connection)
        .await?)
    }

    pub async fn hold(
        &mut self,
        profile: &SubscriptionProfile,
        held: &CredentialUse,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO credential_use (profile_id, organization_id, harness, holder, acquired_at)
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(profile.id.to_string())
        .bind(profile.organization.to_string())
        .bind(&held.harness)
        .bind(&held.holder)
        .bind(Timestamp::now().to_string())
        .execute(&mut *self.connection)
        .await
        .with_context(|| format!("lending the profile {} to {}", profile.name, held.holder))?;

        Ok(())
    }

    pub async fn release(&mut self, held: &CredentialUse) -> Result<()> {
        sqlx::query(
            "DELETE FROM credential_use WHERE profile_id = ? AND harness = ? AND holder = ?",
        )
        .bind(held.profile.to_string())
        .bind(&held.harness)
        .bind(&held.holder)
        .execute(&mut *self.connection)
        .await?;

        Ok(())
    }

    pub async fn release_all(&mut self) -> Result<()> {
        sqlx::query("DELETE FROM credential_use")
            .execute(&mut *self.connection)
            .await?;

        Ok(())
    }

    /// The lent Profile each pending Session on a serialized harness waits behind.
    pub async fn lent(
        &mut self,
        organization: &Organization,
        serialized: &[String],
    ) -> Result<Vec<(SessionId, String)>> {
        sqlx::query(
            "SELECT s.id, p.name
             FROM session AS s
             JOIN workspace AS w ON w.id = s.workspace_id
             JOIN credential_use AS u
               ON u.profile_id = w.subscription_profile_id AND u.harness = s.harness
             JOIN subscription_profile AS p ON p.id = u.profile_id
             WHERE s.organization_id = ?
               AND s.state NOT IN (SELECT value FROM json_each(?))
               AND s.state != ?
               AND s.harness IN (SELECT value FROM json_each(?))
             ORDER BY s.id",
        )
        .bind(organization.id.to_string())
        .bind(occupying()?)
        .bind(SessionState::Ended.as_str())
        .bind(serde_json::to_string(serialized)?)
        .fetch_all(&mut *self.connection)
        .await
        .context("reading which sessions a lent login holds back")?
        .iter()
        .map(|row| Ok((row.get::<String, _>("id").parse()?, row.get("name"))))
        .collect()
    }

    pub async fn lent_to_another(
        &mut self,
        session: &Session,
        serialized: &[String],
    ) -> Result<bool> {
        if !serialized.contains(&session.agent.harness) {
            return Ok(false);
        }

        Ok(sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM credential_use AS u
                            JOIN workspace AS w ON w.subscription_profile_id = u.profile_id
                            WHERE w.id = ? AND u.harness = ?)",
        )
        .bind(session.workspace.to_string())
        .bind(&session.agent.harness)
        .fetch_one(&mut *self.connection)
        .await?)
    }
}
