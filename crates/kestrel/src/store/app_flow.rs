use anyhow::{Result, bail};
use jiff::Timestamp;
use kestrel_operator_types::{GithubAppFlow, GithubAppFlowPhase};
use sqlx::{Row, SqliteConnection};

use crate::declined::{Constraint, Reason};
use crate::keyring::Keyring;

pub struct AppFlows<'a> {
    connection: &'a mut SqliteConnection,
    keyring: &'a Keyring,
}

pub struct Held {
    pub status: GithubAppFlow,
    pub configuration: Option<String>,
}

impl<'a> AppFlows<'a> {
    pub(crate) fn over(connection: &'a mut SqliteConnection, keyring: &'a Keyring) -> Self {
        Self {
            connection,
            keyring,
        }
    }

    pub async fn create(&mut self, configuration: &str, status: &GithubAppFlow) -> Result<()> {
        sqlx::query("INSERT INTO github_app_flow VALUES (?, 'ready', ?, ?, ?)")
            .bind(&status.flow)
            .bind(&status.expires_at)
            .bind(self.keyring.seal(&status.flow, configuration)?)
            .bind(serde_json::to_string(status)?)
            .execute(&mut *self.connection)
            .await?;
        Ok(())
    }

    pub async fn expire(&mut self, state: Option<&str>) -> Result<()> {
        sqlx::query("UPDATE github_app_flow SET phase = 'expired', configuration_sealed = NULL, outcome = json_set(outcome, '$.phase', 'expired', '$.guidance', ?) WHERE state IN (SELECT state FROM github_app_flow WHERE expires_at <= ? AND phase != 'expired' AND (? IS NULL OR state = ?) ORDER BY expires_at LIMIT 32)")
            .bind("Setup expired. Start again; delete or uninstall an abandoned App manually in GitHub settings.")
            .bind(crate::store::due(Timestamp::now())).bind(state).bind(state)
            .execute(&mut *self.connection).await?;
        Ok(())
    }

    pub async fn read(&mut self, state: &str) -> Result<Held> {
        let row = sqlx::query(
            "SELECT configuration_sealed, outcome FROM github_app_flow WHERE state = ?",
        )
        .bind(state)
        .fetch_optional(&mut *self.connection)
        .await?;
        let Some(row) = row else {
            bail!(Reason::InvalidField {
                field: "flow",
                operation: "show_github_app_flow",
                constraint: Constraint::WellFormed,
                allowed: None,
                message: "The GitHub App setup flow is unknown. Start setup again.".into()
            });
        };
        let sealed: Option<String> = row.get("configuration_sealed");
        Ok(Held {
            status: serde_json::from_str(row.get("outcome"))?,
            configuration: sealed
                .map(|sealed| self.keyring.unseal(state, &sealed))
                .transpose()?,
        })
    }

    pub async fn save(
        &mut self,
        expected: GithubAppFlowPhase,
        configuration: Option<&str>,
        status: &GithubAppFlow,
    ) -> Result<()> {
        let changed = sqlx::query("UPDATE github_app_flow SET phase = ?, configuration_sealed = ?, outcome = ? WHERE state = ? AND phase = ?")
            .bind(serde_json::to_value(&status.phase)?.as_str().expect("a flow phase is a string"))
            .bind(configuration.map(|configuration| self.keyring.seal(&status.flow, configuration)).transpose()?)
            .bind(serde_json::to_string(status)?).bind(&status.flow)
            .bind(serde_json::to_value(expected)?.as_str().expect("a flow phase is a string"))
            .execute(&mut *self.connection).await?.rows_affected();
        if changed != 1 {
            bail!(Reason::InvalidField {
                field: "flow",
                operation: "github_app_setup",
                constraint: Constraint::WellFormed,
                allowed: None,
                message: "The GitHub App setup phase changed; inspect setup before retrying."
                    .into()
            });
        }
        Ok(())
    }
}
