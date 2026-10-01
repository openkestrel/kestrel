pub mod agent;
pub mod integration;
pub mod organization;
pub mod profile;
pub mod project;
pub mod pull_request;
pub mod queue;
pub mod trigger;
pub mod workspace;

use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use anyhow::{Context as _, Result};
use jiff::Timestamp;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqliteRow};
use sqlx::{Row, Sqlite, SqlitePool, Transaction};
use tracing::trace;

use crate::fanout::{Notices, Touched};
use crate::keyring::Keyring;
use crate::log::Log;
use crate::store::agent::Agents;
use crate::store::integration::Integrations;
use crate::store::organization::Organizations;
use crate::store::profile::Profiles;
use crate::store::project::Projects;
use crate::store::pull_request::PullRequests;
use crate::store::queue::Queue;
use crate::store::trigger::Triggers;
use crate::store::workspace::Workspaces;

const DATABASE: &str = "kestrel.db";
/// `waited_us` spans the pool acquire and `BEGIN IMMEDIATE` together, and is emitted on failure too.
pub const WRITE_LOCK: &str = "kestrel::store::write_lock";

#[derive(Clone)]
pub struct Store {
    pool: SqlitePool,
    reads: SqlitePool,
    keyring: Arc<Keyring>,
    notices: Notices,
}

impl Store {
    pub async fn open(data_dir: &Path) -> Result<Self> {
        tokio::fs::create_dir_all(data_dir)
            .await
            .with_context(|| format!("creating kestrel's data directory {}", data_dir.display()))?;

        let options = SqliteConnectOptions::new()
            .filename(data_dir.join(DATABASE))
            .create_if_missing(true)
            .foreign_keys(true)
            .journal_mode(SqliteJournalMode::Wal);

        let pool = SqlitePool::connect_with(options.clone())
            .await
            .with_context(|| format!("opening kestrel's database in {}", data_dir.display()))?;

        sqlx::migrate!("src/store/migrations")
            .run(&pool)
            .await
            .context("migrating kestrel's database")?;
        let reads = SqlitePool::connect_with(options.read_only(true))
            .await
            .with_context(|| format!("opening kestrel's database in {}", data_dir.display()))?;

        Ok(Self {
            pool,
            reads,
            keyring: Arc::new(Keyring::beside(data_dir)?),
            notices: Notices::default(),
        })
    }

    /// The change hub every transaction on this store publishes to, and which the operator
    /// boundary subscribes its change streams to.
    pub fn notices(&self) -> Notices {
        self.notices.clone()
    }

    /// Takes the write lock up front: SQLite refuses a deferred transaction that reads and then
    /// writes while another has written, rather than making it wait its turn. Only a transaction
    /// that never writes may take `read` instead.
    pub async fn begin(&self) -> Result<Tx<'_>> {
        let asked = Instant::now();
        let transaction = self.pool.begin_with("BEGIN IMMEDIATE").await;
        trace!(
            target: WRITE_LOCK,
            waited_us = u64::try_from(asked.elapsed().as_micros()).unwrap_or(u64::MAX),
            "waited for the write lock"
        );

        Ok(Tx {
            transaction: transaction?,
            keyring: &self.keyring,
            touched: Touched::default(),
            publishes: Some(self.notices.clone()),
        })
    }

    /// Under WAL a reader waits for no writer, and the connection is read-only so a read that
    /// tries to write is refused rather than left racing the write lock it never took.
    pub async fn read(&self) -> Result<Tx<'_>> {
        Ok(Tx {
            transaction: self.reads.begin().await?,
            keyring: &self.keyring,
            touched: Touched::default(),
            publishes: None,
        })
    }
}

/// Never a reason to stop: the same work asked again later can succeed.
pub fn busy(error: &anyhow::Error) -> bool {
    const SQLITE_BUSY: i32 = 5;
    const SQLITE_LOCKED: i32 = 6;

    error
        .chain()
        .any(|cause| match cause.downcast_ref::<sqlx::Error>() {
            Some(sqlx::Error::Database(database)) => database
                .code()
                .and_then(|code| code.parse::<i32>().ok())
                // The extended codes (SQLITE_BUSY_SNAPSHOT, ...) keep the primary in the low byte.
                .is_some_and(|code| matches!(code & 0xff, SQLITE_BUSY | SQLITE_LOCKED)),
            Some(sqlx::Error::PoolTimedOut) => true,
            _ => false,
        })
}

pub struct Tx<'a> {
    transaction: Transaction<'a, Sqlite>,
    keyring: &'a Keyring,
    touched: Touched,
    /// None on a read, which never publishes: `Touched` is only collected where a write commits.
    publishes: Option<Notices>,
}

impl Tx<'_> {
    pub fn log(&mut self) -> Log<'_> {
        Log::over(&mut self.transaction)
    }

    pub fn organizations(&mut self) -> Organizations<'_> {
        Organizations::over(&mut self.transaction, self.keyring)
    }

    pub fn profiles(&mut self) -> Profiles<'_> {
        Profiles::over(&mut self.transaction, self.keyring)
    }

    pub fn projects(&mut self) -> Projects<'_> {
        Projects::over(&mut self.transaction)
    }

    pub fn agents(&mut self) -> Agents<'_> {
        Agents::over(&mut self.transaction)
    }

    pub fn workspaces(&mut self) -> Workspaces<'_> {
        Workspaces::over(&mut self.transaction, &mut self.touched)
    }

    pub fn queue(&mut self) -> Queue<'_> {
        Queue::over(&mut self.transaction, &mut self.touched)
    }

    pub fn integrations(&mut self) -> Integrations<'_> {
        Integrations::over(&mut self.transaction, self.keyring)
    }

    pub fn triggers(&mut self) -> Triggers<'_> {
        Triggers::over(&mut self.transaction)
    }

    pub fn pull_requests(&mut self) -> PullRequests<'_> {
        PullRequests::over(&mut self.transaction, &mut self.touched)
    }

    pub async fn commit(self) -> Result<()> {
        let Tx {
            transaction,
            touched,
            publishes,
            ..
        } = self;
        transaction.commit().await?;
        if let Some(notices) = publishes {
            notices.publish(touched);
        }

        Ok(())
    }
}

pub struct Declared<T> {
    pub record: T,
    pub created: bool,
}

/// A due time is the one timestamp SQL compares rather than reads back, and at the precision
/// jiff prints by default a whole second sorts after the fractions of it.
fn due(at: Timestamp) -> String {
    format!("{at:.9}")
}

fn timestamp(row: &SqliteRow, column: &str) -> Result<Option<Timestamp>> {
    row.get::<Option<String>, _>(column)
        .map(|at| at.parse())
        .transpose()
        .map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;
    use crate::domain::{Agent, Organization, Project, Workspace};
    use crate::log::Entry;

    async fn declared(store: &Store) -> (Organization, Project, Agent) {
        let mut tx = store.begin().await.unwrap();
        let organization = tx
            .organizations()
            .declare("acme", None)
            .await
            .unwrap()
            .record;
        let project = tx
            .projects()
            .declare(
                &organization,
                "kestrel",
                &["https://github.com/jtmthf/kestrel".to_owned()],
                "main",
            )
            .await
            .unwrap()
            .record;
        let agent = tx
            .agents()
            .declare(&organization, "builder", "opencode", Some("claude-opus-5"))
            .await
            .unwrap()
            .record;
        tx.commit().await.unwrap();

        (organization, project, agent)
    }

    async fn opened_with_notices(
        store: &Store,
    ) -> (Organization, Workspace, crate::fanout::Subscription) {
        let (organization, project, agent) = declared(store).await;
        let mut changes = store.notices().subscribe(organization.id);

        let mut tx = store.begin().await.unwrap();
        let workspace = tx
            .workspaces()
            .open(workspace::Opening {
                organization: &organization,
                project: &project,
                agent: &agent,
                profile: None,
                branch: None,
                correlation: None,
                continues: None,
                started_by: None,
            })
            .await
            .unwrap();
        tx.commit().await.unwrap();

        // Opening is a notice of its own; waiting it out leaves only what each test writes.
        let opened = tokio::time::timeout(std::time::Duration::from_secs(1), changes.recv())
            .await
            .expect("opening the workspace raised no notice");
        assert!(matches!(opened, Some(crate::fanout::Watch::Change(_))));

        (organization, workspace, changes)
    }

    #[test]
    fn a_due_time_at_a_whole_second_sorts_before_the_moments_after_it() {
        let whole: Timestamp = "2026-09-01T12:00:00Z".parse().unwrap();
        let after: Timestamp = "2026-09-01T12:00:00.5Z".parse().unwrap();

        assert!(due(whole) < due(after));
        assert_eq!(due(whole).parse::<Timestamp>().unwrap(), whole);
    }

    #[tokio::test]
    async fn a_read_waits_for_no_writer() {
        let data_dir = TempDir::new().unwrap();
        let store = Store::open(data_dir.path()).await.unwrap();
        let (organization, _, _) = declared(&store).await;
        let writing = store.begin().await.unwrap();

        let read = tokio::time::timeout(std::time::Duration::from_secs(1), async {
            store.read().await?.organizations().named("acme").await
        })
        .await
        .expect("a read waited for the write lock")
        .unwrap();

        assert_eq!(read.id, organization.id);
        drop(writing);
    }

    #[tokio::test]
    async fn a_read_cannot_write() {
        let data_dir = TempDir::new().unwrap();
        let store = Store::open(data_dir.path()).await.unwrap();

        let mut read = store.read().await.unwrap();

        assert!(read.organizations().declare("acme", None).await.is_err());
    }

    #[tokio::test]
    async fn a_transcript_entry_alone_raises_no_notice() {
        let data_dir = TempDir::new().unwrap();
        let store = Store::open(data_dir.path()).await.unwrap();
        let (_, workspace, mut changes) = opened_with_notices(&store).await;

        let mut tx = store.begin().await.unwrap();
        tx.log()
            .append(
                &workspace,
                Entry::ParticipantJoined {
                    participant: "builder".to_owned(),
                },
            )
            .await
            .unwrap();
        tx.commit().await.unwrap();

        let waited =
            tokio::time::timeout(std::time::Duration::from_millis(400), changes.recv()).await;
        assert!(waited.is_err(), "a transcript append raised a notice");
    }

    #[tokio::test]
    async fn a_write_that_never_commits_raises_no_notice() {
        let data_dir = TempDir::new().unwrap();
        let store = Store::open(data_dir.path()).await.unwrap();
        let (_, workspace, mut changes) = opened_with_notices(&store).await;

        let mut tx = store.begin().await.unwrap();
        tx.workspaces().seal(&workspace).await.unwrap();
        drop(tx);

        let waited =
            tokio::time::timeout(std::time::Duration::from_millis(400), changes.recv()).await;
        assert!(waited.is_err(), "a rolled-back write raised a notice");
    }

    #[tokio::test]
    async fn a_transaction_that_fails_part_way_leaves_neither_the_workspace_nor_its_entry() {
        let data_dir = TempDir::new().unwrap();
        let store = Store::open(data_dir.path()).await.unwrap();
        let (organization, project, agent) = declared(&store).await;

        let mut tx = store.begin().await.unwrap();
        let workspace = tx
            .workspaces()
            .open(workspace::Opening {
                organization: &organization,
                project: &project,
                agent: &agent,
                profile: None,
                branch: None,
                correlation: None,
                continues: None,
                started_by: None,
            })
            .await
            .unwrap();
        tx.log()
            .append(
                &workspace,
                Entry::ParticipantJoined {
                    participant: agent.name.clone(),
                },
            )
            .await
            .unwrap();
        drop(tx);

        let mut tx = store.begin().await.unwrap();
        assert!(tx.workspaces().get(workspace.id).await.is_err());
        let entries: i64 = sqlx::query("SELECT COUNT(*) AS entries FROM transcript_entry")
            .fetch_one(&mut *tx.transaction)
            .await
            .unwrap()
            .get("entries");
        assert_eq!(entries, 0);
    }
}
