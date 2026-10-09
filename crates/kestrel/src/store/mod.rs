pub mod agent;
pub mod app_flow;
pub mod integration;
pub mod lease_sweep;
pub mod operator;
pub mod organization;
pub mod profile;
pub mod project;
pub mod pull_request;
pub mod queue;
pub mod sign_in;
pub mod trigger;
pub mod workspace;

use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use anyhow::{Context as _, Result};
use jiff::Timestamp;
use sqlx::migrate::{MigrateError, Migrator};
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

        let migrator = sqlx::migrate!("src/store/migrations");
        // A pre-release build never migrates another build's store: read the migration
        // history before running anything, so an incompatible store is refused without
        // writing to it.
        let fresh = compatible(&pool, &migrator, data_dir).await?;
        migrator
            .run(&pool)
            .await
            .map_err(|error| migration_failed(error, fresh, data_dir))?;
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

/// A store from another build is refused, never migrated: schema changes edit the
/// migrations in place until release, so a checksum or version mismatch means the volume
/// holds another pre-release build's database.
async fn compatible(pool: &SqlitePool, migrator: &Migrator, data_dir: &Path) -> Result<bool> {
    let tracking: Option<String> = sqlx::query_scalar(
        "SELECT name FROM sqlite_master WHERE type = 'table' AND name = '_sqlx_migrations'",
    )
    .fetch_optional(pool)
    .await
    .with_context(|| format!("reading kestrel's database in {}", data_dir.display()))?;

    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM sqlite_master WHERE type = 'table' \
         AND name NOT LIKE 'sqlite_%' AND name != '_sqlx_migrations' ORDER BY name",
    )
    .fetch_all(pool)
    .await
    .with_context(|| format!("reading kestrel's database in {}", data_dir.display()))?;

    let Some(_) = tracking else {
        if tables.is_empty() {
            return Ok(true);
        }
        return Err(incompatible_store(
            format!("it holds {} but no migration history", tables.join(", ")),
            data_dir,
        ));
    };

    let applied: Vec<(i64, Vec<u8>, bool)> =
        sqlx::query_as("SELECT version, checksum, success FROM _sqlx_migrations ORDER BY version")
            .fetch_all(pool)
            .await
            .with_context(|| format!("reading kestrel's database in {}", data_dir.display()))?;

    if let Some((version, _, _)) = applied.iter().find(|(_, _, success)| !success) {
        anyhow::bail!(
            "migration {version} started but never finished applying to kestrel's database in {}",
            data_dir.display()
        );
    }

    for (version, checksum, _) in &applied {
        let Some(carried) = migrator.iter().find(|carried| carried.version == *version) else {
            return Err(incompatible_store(
                format!(
                    "migration {version} was applied to this store but this build does not carry it"
                ),
                data_dir,
            ));
        };
        if carried.checksum.as_ref() != checksum {
            return Err(incompatible_store(
                format!(
                    "migration {version} in this build differs from the migration this store was created with"
                ),
                data_dir,
            ));
        }
    }

    Ok(applied.is_empty() && tables.is_empty())
}

fn migration_failed(error: MigrateError, fresh: bool, data_dir: &Path) -> anyhow::Error {
    if let Some(detail) = incompatibility(&error, fresh) {
        return incompatible_store(detail, data_dir);
    }
    anyhow::Error::new(error).context(format!(
        "migrating kestrel's database in {}",
        data_dir.display()
    ))
}

fn incompatibility(error: &MigrateError, fresh: bool) -> Option<String> {
    match error {
        MigrateError::VersionMissing(version) => Some(format!(
            "migration {version} was applied to this store but this build does not carry it"
        )),
        MigrateError::VersionMismatch(version) => Some(format!(
            "migration {version} in this build differs from the migration this store was created with"
        )),
        MigrateError::VersionNotPresent(version) => Some(format!(
            "migration {version} was applied to this store but this build does not carry it"
        )),
        MigrateError::VersionTooOld(version, latest) => Some(format!(
            "migration {version} is older than this store's latest applied migration {latest}"
        )),
        MigrateError::VersionTooNew(version, latest) => Some(format!(
            "migration {version} is newer than this store's latest applied migration {latest}"
        )),
        MigrateError::ExecuteMigration(source, version) => conflict(source, Some(*version), fresh),
        MigrateError::Execute(source) => conflict(source, None, fresh),
        _ => None,
    }
}

/// A pending migration failing against a store that already holds another build's schema
/// is the same incompatibility; on a fresh store it is this build's own breakage, and a
/// permission, disk, lock or corruption failure is always its own cause.
fn conflict(source: &dyn std::fmt::Display, version: Option<i64>, fresh: bool) -> Option<String> {
    let cause = source.to_string().to_lowercase();
    if fresh || operational(&cause) || !schema_conflict(&cause) {
        return None;
    }
    match version {
        Some(version) => Some(format!(
            "migration {version} cannot apply to this store: {cause}"
        )),
        None => Some(format!("this store cannot be migrated: {cause}")),
    }
}

fn operational(cause: &str) -> bool {
    [
        "readonly",
        "read-only",
        "permission",
        "locked",
        "busy",
        "disk",
        "full",
        "space",
        "not a database",
        "malformed",
        "corrupt",
        "io error",
        "interrupted",
    ]
    .iter()
    .any(|pattern| cause.contains(pattern))
}

fn schema_conflict(cause: &str) -> bool {
    [
        "already exists",
        "duplicate",
        "no such table",
        "no such column",
        "has no column",
    ]
    .iter()
    .any(|pattern| cause.contains(pattern))
}

fn incompatible_store(detail: String, data_dir: &Path) -> anyhow::Error {
    anyhow::anyhow!(
        "kestrel cannot start with the store in {}: {}.\n\
         kestrel is pre-release and does not migrate stores between builds.\n\
         To start over, remove the volume (this deletes all saved kestrel data):\n\n\
         docker compose down --volumes",
        data_dir.join(DATABASE).display(),
        detail
    )
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

    pub fn operators(&mut self) -> operator::Operators<'_> {
        operator::Operators::over(&mut self.transaction)
    }

    pub fn organizations(&mut self) -> Organizations<'_> {
        Organizations::over(&mut self.transaction, self.keyring)
    }

    pub fn profiles(&mut self) -> Profiles<'_> {
        Profiles::over(&mut self.transaction, self.keyring)
    }

    pub fn sign_ins(&mut self) -> sign_in::SignIns<'_> {
        sign_in::SignIns::over(&mut self.transaction)
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

    pub fn lease_sweep(&mut self) -> lease_sweep::LeaseSweep<'_> {
        lease_sweep::LeaseSweep::over(&mut self.transaction)
    }

    pub fn queue(&mut self) -> Queue<'_> {
        Queue::over(&mut self.transaction, &mut self.touched)
    }

    pub fn app_flows(&mut self) -> app_flow::AppFlows<'_> {
        app_flow::AppFlows::over(&mut self.transaction, self.keyring)
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
    use crate::domain::{
        Agent, Declared as DomainDeclared, Organization, Project, SessionOption, SessionOptionKind,
        Workspace,
    };
    use crate::fanout::{Resource, Subscription, Watch};
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
            .declare(
                &organization,
                "builder",
                "opencode",
                &DomainDeclared {
                    model: Some("claude-opus-5".to_owned()),
                    ..DomainDeclared::default()
                },
            )
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
    async fn a_session_option_write_raises_a_session_notice_and_an_identical_one_raises_nothing() {
        let data_dir = TempDir::new().unwrap();
        let store = Store::open(data_dir.path()).await.unwrap();
        let (_, workspace, mut changes) = opened_with_notices(&store).await;

        let mut tx = store.begin().await.unwrap();
        let session = tx
            .workspaces()
            .enqueue_session(&workspace, None, DomainDeclared::default())
            .await
            .unwrap();
        tx.commit().await.unwrap();
        // The enqueue raised notices of its own; waiting them out leaves only the write under test.
        assert!(a_session_notice(&mut changes).await);

        let options = vec![SessionOption {
            id: "model".to_owned(),
            name: "Model".to_owned(),
            description: None,
            category: Some("model".to_owned()),
            kind: SessionOptionKind::Select {
                current: "scripted-max".to_owned(),
                values: Vec::new(),
                groups: Vec::new(),
            },
        }];
        let mut tx = store.begin().await.unwrap();
        assert!(
            tx.workspaces()
                .record_session_info(&session, Some("a title"), &options, &[])
                .await
                .unwrap()
        );
        tx.commit().await.unwrap();
        assert!(
            a_session_notice(&mut changes).await,
            "a Session-row option write raised no Session notice"
        );

        let mut tx = store.begin().await.unwrap();
        assert!(
            !tx.workspaces()
                .record_session_info(&session, Some("a title"), &options, &[])
                .await
                .unwrap()
        );
        tx.commit().await.unwrap();
        let waited = tokio::time::timeout(std::time::Duration::from_millis(400), async {
            a_session_notice(&mut changes).await
        })
        .await;
        assert!(
            !waited.unwrap_or(false),
            "an option write that changed nothing raised a notice"
        );

        let session = store
            .read()
            .await
            .unwrap()
            .workspaces()
            .session(session.id)
            .await
            .unwrap();
        assert_eq!(session.title.as_deref(), Some("a title"));
        assert_eq!(session.worked_model.as_deref(), Some("scripted-max"));
    }

    async fn a_session_notice(changes: &mut Subscription) -> bool {
        let waited = tokio::time::timeout(std::time::Duration::from_secs(1), async {
            loop {
                match changes.recv().await {
                    Some(Watch::Change(Resource::Session(_))) => return true,
                    Some(_) => continue,
                    None => return false,
                }
            }
        })
        .await;

        waited.unwrap_or(false)
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
    async fn a_work_report_raises_a_notice_once_written_and_a_repeat_raises_none() {
        let data_dir = TempDir::new().unwrap();
        let store = Store::open(data_dir.path()).await.unwrap();
        let (organization, workspace, mut changes) = opened_with_notices(&store).await;
        let reporter = workspace::Linked {
            instance: "local-exec/one".to_owned(),
            workspace: workspace.id,
            organization: organization.id,
        };
        let summary = || crate::live_work::Summary {
            repositories: vec![crate::live_work::Repository {
                repository: "kestrel".to_owned(),
                git: crate::live_work::Git::Unreadable {
                    because: "no such directory".to_owned(),
                },
            }],
            reported_at: Timestamp::now(),
        };

        let mut tx = store.begin().await.unwrap();
        tx.workspaces()
            .record_work_report(&reporter, &summary())
            .await
            .unwrap();
        tx.commit().await.unwrap();
        let noticed = tokio::time::timeout(std::time::Duration::from_secs(1), changes.recv()).await;
        assert!(matches!(noticed, Ok(Some(Watch::Change(_)))));

        let repeated = summary();
        let mut tx = store.begin().await.unwrap();
        tx.workspaces()
            .record_work_report(&reporter, &repeated)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        let waited =
            tokio::time::timeout(std::time::Duration::from_millis(400), changes.recv()).await;
        assert!(waited.is_err(), "a repeated work report raised a notice");

        let last = store
            .read()
            .await
            .unwrap()
            .workspaces()
            .work_reports(workspace.id)
            .await
            .unwrap()
            .remove(0);
        assert_eq!(last.instance, reporter.instance);
        assert_eq!(last.summary.repositories, repeated.repositories);
        assert_eq!(last.summary.reported_at, repeated.reported_at);
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
