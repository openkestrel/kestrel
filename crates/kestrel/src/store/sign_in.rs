use std::collections::BTreeMap;

use crate::timer::Clock;
use anyhow::{Context as _, Result};
use jiff::Timestamp;
use sqlx::{Row, SqliteConnection};

use crate::domain::{Session, SessionId, SubscriptionProfile};
use crate::profile::{Entry, Kind};
use crate::sign_in::credential_use::CredentialUse;
use crate::sign_in::model_test::Test;
use crate::sign_in::{
    Authentication, ModelUse, Source, State, TestOutcome, TestState, UseResult, UseSource,
};
use crate::store::workspace::occupying;

pub struct SignIns<'a> {
    connection: &'a mut SqliteConnection,
    clock: &'a Clock,
}

/// Every write of material calls this in its own transaction, so no material is held without a
/// revision and the evidence that revision has so far.
pub(crate) async fn mint(
    connection: &mut SqliteConnection,
    authentication: &Authentication,
    now: Timestamp,
) -> Result<i64> {
    let revision: i64 = sqlx::query_scalar(
        "INSERT INTO material_revision (written_at) VALUES (?) RETURNING revision",
    )
    .bind(now.to_string())
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

pub(crate) async fn mint_refreshed(
    connection: &mut SqliteConnection,
    from: i64,
    now: Timestamp,
) -> Result<i64> {
    let revision = mint(
        connection,
        &Authentication::unchecked(Source::Refresh, now),
        now,
    )
    .await?;
    sqlx::query("UPDATE material_revision SET refreshed_from = ? WHERE revision = ?")
        .bind(from)
        .bind(revision)
        .execute(&mut *connection)
        .await
        .context("recording which revision a refresh advanced")?;

    Ok(revision)
}

impl<'a> SignIns<'a> {
    pub(crate) fn over(connection: &'a mut SqliteConnection, clock: &'a Clock) -> Self {
        Self { connection, clock }
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
        .bind(self.clock.now().to_string())
        .bind(revision)
        .execute(&mut *self.connection)
        .await
        .with_context(|| format!("recording what material revision {revision} did"))?;

        Ok(())
    }

    /// `image` is what a model test ran in; a Session records none.
    pub async fn record_use(
        &mut self,
        revision: i64,
        harness: &str,
        model: &str,
        result: UseResult,
        source: UseSource,
        image: Option<&str>,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO model_use_evidence
                 (revision, harness, model, image, result, source, observed_at)
             VALUES (?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT (revision, harness, model)
             DO UPDATE SET image = excluded.image, result = excluded.result,
                           source = excluded.source, observed_at = excluded.observed_at",
        )
        .bind(revision)
        .bind(harness)
        .bind(model)
        .bind(image)
        .bind(result.as_str())
        .bind(source.as_str())
        .bind(self.clock.now().to_string())
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

        let handed_at = self.clock.now().to_string();
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
        except: Option<SessionId>,
    ) -> Result<Option<SessionId>> {
        sqlx::query_scalar::<_, String>(
            "SELECT s.id FROM session AS s
             JOIN workspace AS w ON w.id = s.workspace_id
             WHERE w.subscription_profile_id = ? AND s.harness = ?
               AND s.state IN (SELECT value FROM json_each(?))
               AND (? IS NULL OR s.id != ?)
             ORDER BY s.enqueued_at, s.id
             LIMIT 1",
        )
        .bind(profile.id.to_string())
        .bind(harness)
        .bind(occupying()?)
        .bind(except.map(|id| id.to_string()))
        .bind(except.map(|id| id.to_string()))
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
        .bind(self.clock.now().to_string())
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

    pub async fn open_test(&mut self, test: &Test) -> Result<()> {
        sqlx::query(
            "INSERT INTO sign_in_test
                 (id, organization_id, harness, method, profile_id, revision, requested_model,
                  model, image, state, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&test.id)
        .bind(test.organization.to_string())
        .bind(&test.harness)
        .bind(&test.method)
        .bind(test.profile.map(|profile| profile.to_string()))
        .bind(test.revision)
        .bind(&test.requested_model)
        .bind(&test.model)
        .bind(&test.image)
        .bind(test.state.as_str())
        .bind(test.created_at.to_string())
        .execute(&mut *self.connection)
        .await
        .with_context(|| format!("opening the sign-in test {}", test.id))?;

        Ok(())
    }

    pub async fn test(&mut self, id: &str) -> Result<Option<Test>> {
        sqlx::query("SELECT * FROM sign_in_test WHERE id = ?")
            .bind(id)
            .fetch_optional(&mut *self.connection)
            .await?
            .as_ref()
            .map(test)
            .transpose()
    }

    pub async fn unfinished_tests(&mut self) -> Result<Vec<Test>> {
        sqlx::query(
            "SELECT * FROM sign_in_test
             WHERE state IN ('pending', 'running') ORDER BY created_at, id",
        )
        .fetch_all(&mut *self.connection)
        .await?
        .iter()
        .map(test)
        .collect()
    }

    /// The oldest pending test, now running: claimed here so two passes never run one twice.
    pub async fn claim_test(&mut self) -> Result<Option<Test>> {
        sqlx::query(
            "UPDATE sign_in_test SET state = 'running', started_at = ?
             WHERE id = (SELECT id FROM sign_in_test WHERE state = 'pending'
                         ORDER BY created_at, id LIMIT 1)
             RETURNING *",
        )
        .bind(self.clock.now().to_string())
        .fetch_optional(&mut *self.connection)
        .await?
        .as_ref()
        .map(test)
        .transpose()
    }

    pub async fn resolve_test_model(&mut self, id: &str, model: &str) -> Result<()> {
        sqlx::query("UPDATE sign_in_test SET model = ? WHERE id = ? AND state = 'running'")
            .bind(model)
            .bind(id)
            .execute(&mut *self.connection)
            .await?;

        Ok(())
    }

    /// False when the test had already ended, which leaves how it ended as it was.
    pub async fn end_test(
        &mut self,
        id: &str,
        state: TestState,
        outcome: Option<TestOutcome>,
        message: Option<&str>,
    ) -> Result<bool> {
        let ended = sqlx::query(
            "UPDATE sign_in_test SET state = ?, outcome = ?, message = ?, finished_at = ?
             WHERE id = ? AND state IN ('pending', 'running')",
        )
        .bind(state.as_str())
        .bind(outcome.map(TestOutcome::as_str))
        .bind(message)
        .bind(self.clock.now().to_string())
        .bind(id)
        .execute(&mut *self.connection)
        .await
        .with_context(|| format!("ending the sign-in test {id}"))?;

        Ok(ended.rows_affected() > 0)
    }
}

fn test(row: &sqlx::sqlite::SqliteRow) -> Result<Test> {
    let at = |column: &str| -> Result<Option<Timestamp>> {
        Ok(row
            .get::<Option<String>, _>(column)
            .map(|at| at.parse())
            .transpose()?)
    };

    Ok(Test {
        id: row.get("id"),
        organization: row.get::<String, _>("organization_id").parse()?,
        harness: row.get("harness"),
        method: row.get("method"),
        profile: row
            .get::<Option<String>, _>("profile_id")
            .map(|id| id.parse())
            .transpose()?,
        revision: row.get("revision"),
        requested_model: row.get("requested_model"),
        model: row.get("model"),
        image: row.get("image"),
        state: row.get::<String, _>("state").parse()?,
        outcome: row
            .get::<Option<String>, _>("outcome")
            .map(|outcome| outcome.parse())
            .transpose()?,
        message: row.get("message"),
        created_at: row.get::<String, _>("created_at").parse()?,
        started_at: at("started_at")?,
        finished_at: at("finished_at")?,
    })
}
