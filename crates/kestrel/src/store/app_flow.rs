use anyhow::{Result, bail};
use jiff::Timestamp;
use sqlx::{Row, SqliteConnection};

use crate::declined::Declined;
use crate::keyring::Keyring;

pub struct AppFlows<'a> {
    connection: &'a mut SqliteConnection,
    keyring: &'a Keyring,
}

impl<'a> AppFlows<'a> {
    pub(crate) fn over(connection: &'a mut SqliteConnection, keyring: &'a Keyring) -> Self {
        Self {
            connection,
            keyring,
        }
    }

    pub async fn create(
        &mut self,
        state: &str,
        configuration: &str,
        expires: Timestamp,
    ) -> Result<()> {
        sqlx::query("DELETE FROM github_app_flow WHERE expires_at <= ?")
            .bind(crate::store::due(Timestamp::now()))
            .execute(&mut *self.connection)
            .await?;
        sqlx::query("INSERT INTO github_app_flow VALUES (?, 'ready', ?, ?)")
            .bind(state)
            .bind(crate::store::due(expires))
            .bind(self.keyring.seal(state, configuration)?)
            .execute(&mut *self.connection)
            .await?;
        Ok(())
    }

    pub async fn read(&mut self, state: &str, phase: &str) -> Result<String> {
        let row = sqlx::query("SELECT configuration_sealed FROM github_app_flow WHERE state = ? AND phase = ? AND expires_at > ?")
            .bind(state).bind(phase).bind(crate::store::due(Timestamp::now()))
            .fetch_optional(&mut *self.connection).await?;
        let Some(row) = row else {
            bail!(Declined::Unacceptable(
                "the GitHub App setup state is unknown, expired, or already used".into()
            ));
        };
        self.keyring.unseal(state, row.get("configuration_sealed"))
    }

    pub async fn claim(&mut self, state: &str) -> Result<String> {
        let configuration = self.read(state, "ready").await?;
        sqlx::query(
            "UPDATE github_app_flow SET phase = 'exchanging' WHERE state = ? AND phase = 'ready'",
        )
        .bind(state)
        .execute(&mut *self.connection)
        .await?;
        Ok(configuration)
    }

    pub async fn converted(&mut self, state: &str, configuration: &str) -> Result<()> {
        sqlx::query("UPDATE github_app_flow SET phase = 'converted', configuration_sealed = ? WHERE state = ? AND phase = 'exchanging'")
            .bind(self.keyring.seal(state, configuration)?).bind(state)
            .execute(&mut *self.connection).await?;
        Ok(())
    }

    pub async fn remove(&mut self, state: &str) -> Result<()> {
        sqlx::query("DELETE FROM github_app_flow WHERE state = ?")
            .bind(state)
            .execute(&mut *self.connection)
            .await?;
        Ok(())
    }
}
