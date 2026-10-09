use anyhow::Result;
use jiff::Timestamp;
use sqlx::{Row, SqliteConnection};

use crate::domain::{Operator, OperatorId};
use crate::store::Declared;

pub struct Operators<'a> {
    connection: &'a mut SqliteConnection,
}

impl<'a> Operators<'a> {
    pub(crate) fn over(connection: &'a mut SqliteConnection) -> Self {
        Self { connection }
    }

    pub async fn current(&mut self) -> Result<Option<Operator>> {
        sqlx::query("SELECT id, name FROM operator WHERE singleton = 1")
            .fetch_optional(&mut *self.connection)
            .await?
            .map(|row| {
                Ok(Operator {
                    id: row.get::<String, _>("id").parse()?,
                    name: row.get("name"),
                })
            })
            .transpose()
    }

    pub async fn name(&mut self, name: &str) -> Result<Declared<Operator>> {
        let existing = self.current().await?;
        let created = existing.is_none();
        let operator = Operator {
            id: existing.map_or_else(OperatorId::generate, |operator| operator.id),
            name: name.to_owned(),
        };
        sqlx::query(
            "INSERT INTO operator (singleton, id, name, declared_at) VALUES (1, ?, ?, ?)
                     ON CONFLICT (singleton) DO UPDATE SET name = excluded.name",
        )
        .bind(operator.id.to_string())
        .bind(&operator.name)
        .bind(Timestamp::now().to_string())
        .execute(&mut *self.connection)
        .await?;
        Ok(Declared {
            record: operator,
            created,
        })
    }
}
