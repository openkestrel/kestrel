use anyhow::{Context as _, Result};
use jiff::Timestamp;
use sqlx::{Row, SqliteConnection};

use crate::sign_in::{Authentication, ModelUse};

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
}
