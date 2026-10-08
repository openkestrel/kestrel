//! The one place a test reaches past the store to its tables: each method is a fault the store
//! has no path to, or an observation of what it keeps at rest.

use std::collections::BTreeSet;
use std::future::Future;
use std::path::Path;

use jiff::Timestamp;
use kestrel::domain::{IntegrationId, OrganizationId, Session, WorkspaceId};
use kestrel::link::Instruction;
use sqlx::{Row, SqlitePool};

use super::PATIENCE;

pub struct Database<'a> {
    data_dir: &'a Path,
}

/// What the GitHub App manifest flow left in its Integration row.
pub struct GithubIntegrationAtRest {
    pub app_id: i64,
    pub installation_id: i64,
    pub private_key_sealed: String,
    pub signing_secret: String,
    pub polled: bool,
}

impl<'a> Database<'a> {
    pub fn at(data_dir: &'a Path) -> Self {
        Self { data_dir }
    }

    async fn pool(&self) -> SqlitePool {
        SqlitePool::connect(&format!(
            "sqlite://{}?mode=rwc",
            self.data_dir.join("kestrel.db").display()
        ))
        .await
        .expect("the database should open")
    }

    async fn execute(&self, statement: &'static str, binds: &[&str]) {
        let pool = self.pool().await;
        let mut query = sqlx::query(statement);
        for bind in binds {
            query = query.bind(*bind);
        }
        query
            .execute(&pool)
            .await
            .unwrap_or_else(|error| panic!("{statement}: {error}"));
        pool.close().await;
    }

    /// Takes SQLite's write lock the way another process on the same file would.
    pub async fn while_locked<T>(&self, meanwhile: impl Future<Output = T>) -> T {
        let pool = self.pool().await;
        let mut holder = pool.acquire().await.expect("a connection");
        sqlx::query("BEGIN IMMEDIATE")
            .execute(&mut *holder)
            .await
            .expect("the write lock should be free to take");

        let done = meanwhile.await;

        sqlx::query("ROLLBACK")
            .execute(&mut *holder)
            .await
            .expect("the write lock should release");
        drop(holder);
        pool.close().await;

        done
    }

    pub async fn backdate_transcript(&self, workspace: WorkspaceId, at: Timestamp) {
        self.execute(
            "UPDATE transcript_entry SET appended_at = ? WHERE workspace_id = ?",
            &[&at.to_string(), &workspace.to_string()],
        )
        .await;
    }

    pub async fn backdate_entry(&self, workspace: WorkspaceId, seq: i64, at: Timestamp) {
        let pool = self.pool().await;
        sqlx::query(
            "UPDATE transcript_entry SET appended_at = ? WHERE workspace_id = ? AND seq = ?",
        )
        .bind(at.to_string())
        .bind(workspace.to_string())
        .bind(seq)
        .execute(&pool)
        .await
        .expect("the entry should backdate");
        pool.close().await;
    }

    pub async fn refuse_retention(&self) {
        self.execute(
            "CREATE TRIGGER refuse_retention BEFORE UPDATE ON transcript_entry WHEN OLD.kind = 'narration' AND json_extract(NEW.body, '$.type') = 'expired' BEGIN SELECT RAISE(ABORT, 'retention write failed'); END",
            &[],
        )
        .await;
    }

    pub async fn allow_retention(&self) {
        self.execute("DROP TRIGGER refuse_retention", &[]).await;
    }

    pub async fn refuse_payload_writes(&self) {
        self.execute(
            "CREATE TRIGGER refuse_payload BEFORE INSERT ON transcript_payload BEGIN SELECT RAISE(ABORT, 'payload write failed'); END",
            &[],
        )
        .await;
    }

    pub async fn receive_held_messages_at(&self, workspace: WorkspaceId, at: Timestamp) {
        self.execute(
            "UPDATE pending_message SET received_at = ? WHERE workspace_id = ? AND state = 'held'",
            &[&at.to_string(), &workspace.to_string()],
        )
        .await;
    }

    pub async fn enqueue_at(&self, session: &Session, at: Timestamp) {
        self.execute(
            "UPDATE session SET enqueued_at = ? WHERE id = ?",
            &[&at.to_string(), &session.id.to_string()],
        )
        .await;
    }

    /// The `ended`/NULL row migration 0003 leaves behind for every Session predating kestrel
    /// scheduling; ending a Session through the store always records an exit.
    pub async fn end_without_an_exit(&self, session: &Session) {
        self.execute(
            "UPDATE session SET state = 'ended', ended_at = ?, exit = NULL WHERE id = ?",
            &[&Timestamp::now().to_string(), &session.id.to_string()],
        )
        .await;
    }

    pub async fn last_lease_sweep(&self, at: Timestamp) {
        self.execute(
            "INSERT INTO lease_sweep (id, last_pass_at) VALUES (1, ?) ON CONFLICT(id) DO UPDATE SET last_pass_at = excluded.last_pass_at",
            &[&format!("{at:.9}")],
        )
        .await;
    }

    /// Every Integration last read and polled at `at`, and due to poll now.
    pub async fn unpolled_since(&self, at: Timestamp) {
        self.execute(
            "UPDATE integration SET deliveries_read_from = ?, last_polled_at = ?, poll_due_at = ?",
            &[
                &at.to_string(),
                &at.to_string(),
                &Timestamp::now().to_string(),
            ],
        )
        .await;
    }

    pub async fn expire_github_app_flows(&self) {
        self.execute(
            "UPDATE github_app_flow SET phase = 'ready', expires_at = '2000-01-01'",
            &[],
        )
        .await;
    }

    /// Records `count` labelled Events as already received and checkpointed, because recording
    /// that many through an Integration is a load scenario of its own.
    pub async fn record_events(
        &self,
        organization: OrganizationId,
        integration: IntegrationId,
        count: i64,
        source: &str,
        data: &str,
    ) {
        let pool = self.pool().await;
        sqlx::query(
            "WITH RECURSIVE n (n) AS (SELECT 0 UNION ALL SELECT n + 1 FROM n WHERE n + 1 < ?)
             INSERT INTO event (record_id, organization_id, integration_id, id, source, specversion,
                                type, time, data, recorded_at)
             SELECT printf('history-%d', n), ?, ?, printf('history-%d', n), ?, '1.0',
                    'com.github.issues.labeled', ?, ?, ?
               FROM n",
        )
        .bind(count)
        .bind(organization.to_string())
        .bind(integration.to_string())
        .bind(source)
        .bind(Timestamp::now().to_string())
        .bind(data)
        .bind(Timestamp::now().to_string())
        .execute(&pool)
        .await
        .expect("the events should record");
        pool.close().await;
        self.checkpoint().await;
    }

    /// Moves every frame in the WAL into the database file, so its bytes are all there is.
    pub async fn checkpoint(&self) {
        self.execute("PRAGMA wal_checkpoint(TRUNCATE)", &[]).await;
    }

    pub async fn edit_the_first_migration(&self) {
        self.execute(
            "UPDATE _sqlx_migrations SET checksum = randomblob(48) WHERE version = 1",
            &[],
        )
        .await;
    }

    pub async fn record_a_migration_from_a_newer_build(&self, version: i64) {
        let pool = self.pool().await;
        sqlx::query(
            "INSERT INTO _sqlx_migrations (version, description, success, checksum, execution_time)
             VALUES (?, 'from a newer build', TRUE, randomblob(48), 0)",
        )
        .bind(version)
        .execute(&pool)
        .await
        .expect("a newer build's history");
        pool.close().await;
    }

    pub async fn leave_a_table_without_history(&self) {
        self.execute("CREATE TABLE leftover (id INTEGER PRIMARY KEY)", &[])
            .await;
        self.execute("INSERT INTO leftover (id) VALUES (1)", &[])
            .await;
    }

    pub async fn rows_left_over(&self) -> Vec<i64> {
        let pool = self.pool().await;
        let rows = sqlx::query_scalar("SELECT id FROM leftover")
            .fetch_all(&pool)
            .await
            .expect("the leftover table should read");
        pool.close().await;
        rows
    }

    pub async fn has_a_migration_history(&self) -> bool {
        let pool = self.pool().await;
        let tracking: Option<String> = sqlx::query_scalar(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name = '_sqlx_migrations'",
        )
        .fetch_optional(&pool)
        .await
        .expect("the schema should read");
        pool.close().await;
        tracking.is_some()
    }

    /// Read without opening the store, which would refuse an incompatible one.
    pub async fn organizations(&self) -> Vec<String> {
        let pool = self.pool().await;
        let names = sqlx::query_scalar("SELECT name FROM organization ORDER BY name")
            .fetch_all(&pool)
            .await
            .expect("the organizations should read");
        pool.close().await;
        names
    }

    pub async fn holds_messages(&self, workspace: WorkspaceId) -> bool {
        let pool = self.pool().await;
        let held = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM pending_message WHERE workspace_id = ? AND state = 'held')",
        )
        .bind(workspace.to_string())
        .fetch_one(&pool)
        .await
        .expect("held messages should read");
        pool.close().await;
        held
    }

    pub async fn instructions(&self, session: &Session) -> Vec<Instruction> {
        let pool = self.pool().await;
        let bodies: Vec<String> = sqlx::query_scalar(
            "SELECT body FROM link_instruction WHERE session_id = ? ORDER BY seq",
        )
        .bind(session.id.to_string())
        .fetch_all(&pool)
        .await
        .expect("the session's instructions should read");
        pool.close().await;

        bodies
            .iter()
            .map(|body| serde_json::from_str(body).expect("an instruction body"))
            .collect()
    }

    /// Waits for one: a Session's instruction can be written after the state the caller was
    /// waiting on.
    pub async fn latest_instruction(&self, session: &Session) -> Instruction {
        let deadline = tokio::time::Instant::now() + PATIENCE;
        loop {
            if let Some(latest) = self.instructions(session).await.pop() {
                return latest;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "the session {} was never sent an instruction",
                session.id
            );
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    }

    pub async fn instances(&self) -> BTreeSet<String> {
        let pool = self.pool().await;
        let instances: Vec<String> =
            sqlx::query_scalar("SELECT DISTINCT instance FROM session WHERE instance IS NOT NULL")
                .fetch_all(&pool)
                .await
                .expect("the instances should read");
        pool.close().await;
        instances.into_iter().collect()
    }

    pub async fn sessions(&self) -> Vec<String> {
        let pool = self.pool().await;
        let sessions = sqlx::query_scalar("SELECT id FROM session")
            .fetch_all(&pool)
            .await
            .expect("the sessions should read");
        pool.close().await;
        sessions
    }

    pub async fn sealed_github_app_configuration(&self) -> String {
        let pool = self.pool().await;
        let sealed = sqlx::query_scalar("SELECT configuration_sealed FROM github_app_flow")
            .fetch_one(&pool)
            .await
            .expect("the app flow should read");
        pool.close().await;
        sealed
    }

    pub async fn github_integration_at_rest(&self) -> GithubIntegrationAtRest {
        let pool = self.pool().await;
        let row = sqlx::query(
            "SELECT app_id, installation_id, private_key_sealed, signing_secret, poll_due_at FROM integration",
        )
        .fetch_one(&pool)
        .await
        .expect("the integration should read");
        pool.close().await;

        GithubIntegrationAtRest {
            app_id: row.get("app_id"),
            installation_id: row.get("installation_id"),
            private_key_sealed: row.get("private_key_sealed"),
            signing_secret: row.get("signing_secret"),
            polled: row.get::<Option<String>, _>("poll_due_at").is_some(),
        }
    }
}
