//! An incompatible store fails startup with a diagnostic and a reset command, without
//! touching the data; anything else that fails to open keeps its own cause.

use crate::support;

use std::path::{Path, PathBuf};
use std::process::Command;

use support::Database;
use tempfile::TempDir;

const DATABASE: &str = "kestrel.db";
const KEY: &str = "kestrel.key";
const RESET: &str = "docker compose down --volumes";

fn database(data_dir: &Path) -> PathBuf {
    data_dir.join(DATABASE)
}

fn key(data_dir: &Path) -> PathBuf {
    data_dir.join(KEY)
}

async fn seeded() -> TempDir {
    let data_dir = TempDir::new().expect("a temporary data directory");
    let store = kestrel::store::Store::open(data_dir.path())
        .await
        .expect("a fresh store opens");
    let mut tx = store.begin().await.expect("a write transaction");
    tx.organizations()
        .declare("acme", None)
        .await
        .expect("an organization");
    tx.commit().await.expect("the declaration commits");

    data_dir
}

fn refused(error: anyhow::Error) -> String {
    format!("{error:?}")
}

async fn fails(data_dir: &Path, what: &str) -> String {
    match kestrel::store::Store::open(data_dir).await {
        Ok(_) => panic!("{what}"),
        Err(error) => refused(error),
    }
}

fn assert_incompatible(message: &str) {
    for expected in [
        RESET,
        "deletes all saved kestrel data",
        "pre-release",
        "migration",
    ] {
        assert!(
            message.contains(expected),
            "an incompatible store was refused without {expected:?}:\n{message}"
        );
    }
    assert!(
        message.contains(&format!("\n{RESET}")) || message.contains(&format!("\n{RESET}\n")),
        "the reset command is not on a copyable line of its own:\n{message}"
    );
}

fn assert_other(message: &str) {
    assert!(
        !message.contains(RESET),
        "a non-schema failure recommended destroying the volume:\n{message}"
    );
}

#[tokio::test]
async fn a_fresh_data_directory_initializes_and_reopens() {
    let data_dir = TempDir::new().expect("a temporary data directory");

    kestrel::store::Store::open(data_dir.path())
        .await
        .expect("a fresh store opens");
    assert!(database(data_dir.path()).exists());
    assert!(key(data_dir.path()).exists());

    kestrel::store::Store::open(data_dir.path())
        .await
        .expect("a compatible restart opens");
}

#[tokio::test]
async fn a_compatible_restart_keeps_its_organizations() {
    let data_dir = seeded().await;

    let store = kestrel::store::Store::open(data_dir.path())
        .await
        .expect("a compatible restart opens");
    let organization = store
        .read()
        .await
        .expect("a read transaction")
        .organizations()
        .named("acme")
        .await
        .expect("the declared organization");

    assert_eq!(organization.name, "acme");
}

#[tokio::test]
async fn a_store_from_an_edited_migration_is_refused_and_left_alone() {
    let data_dir = seeded().await;
    let tampered = Database::at(data_dir.path());
    tampered.edit_the_first_migration().await;
    tampered.checkpoint().await;
    let kept_database = std::fs::read(database(data_dir.path())).expect("the database");
    let kept_key = std::fs::read(key(data_dir.path())).expect("the encryption key");

    let message = fails(data_dir.path(), "an edited migration opened").await;

    assert!(
        message.contains("migration 1"),
        "the refusal names nothing:\n{message}"
    );
    assert_incompatible(&message);
    assert_eq!(
        std::fs::read(database(data_dir.path())).expect("the database"),
        kept_database,
        "the refused open changed the database"
    );
    assert_eq!(
        std::fs::read(key(data_dir.path())).expect("the encryption key"),
        kept_key,
        "the refused open changed the encryption key"
    );
    assert_eq!(
        Database::at(data_dir.path()).organizations().await,
        ["acme"]
    );
}

#[tokio::test]
async fn a_store_from_a_newer_build_is_refused_and_left_alone() {
    let data_dir = seeded().await;
    let tampered = Database::at(data_dir.path());
    tampered.record_a_migration_from_a_newer_build(99999).await;
    tampered.checkpoint().await;
    let kept_database = std::fs::read(database(data_dir.path())).expect("the database");
    let kept_key = std::fs::read(key(data_dir.path())).expect("the encryption key");

    let message = fails(data_dir.path(), "a newer build's store opened").await;

    assert!(
        message.contains("migration 99999"),
        "the refusal names nothing:\n{message}"
    );
    assert_incompatible(&message);
    assert_eq!(
        std::fs::read(database(data_dir.path())).expect("the database"),
        kept_database,
        "the refused open changed the database"
    );
    assert_eq!(
        std::fs::read(key(data_dir.path())).expect("the encryption key"),
        kept_key,
        "the refused open changed the encryption key"
    );
    assert_eq!(
        Database::at(data_dir.path()).organizations().await,
        ["acme"]
    );
}

#[tokio::test]
async fn tables_without_a_migration_history_are_refused_and_left_alone() {
    let data_dir = TempDir::new().expect("a temporary data directory");
    let leftover = Database::at(data_dir.path());
    leftover.leave_a_table_without_history().await;

    let message = fails(data_dir.path(), "tables without history opened").await;

    assert!(
        message.contains("leftover"),
        "the refusal names nothing:\n{message}"
    );
    assert_incompatible(&message);
    assert!(
        !key(data_dir.path()).exists(),
        "the refused open generated an encryption key"
    );
    assert!(
        !leftover.has_a_migration_history().await,
        "the refused open started a migration history"
    );
    assert_eq!(leftover.rows_left_over().await, [1]);
}

#[tokio::test]
async fn a_garbage_database_keeps_its_cause_not_a_reset() {
    let data_dir = TempDir::new().expect("a temporary data directory");
    std::fs::write(database(data_dir.path()), b"not a database").expect("garbage");

    let message = fails(data_dir.path(), "garbage opened").await;

    assert!(
        message.contains("database"),
        "the refusal lost its cause:\n{message}"
    );
    assert_other(&message);
}

#[tokio::test]
async fn a_data_directory_that_is_a_file_keeps_its_cause_not_a_reset() {
    let data_dir = TempDir::new().expect("a temporary data directory");
    let file = data_dir.path().join("file");
    std::fs::write(&file, b"not a directory").expect("a file");

    let message = fails(&file, "a file opened as a data directory").await;

    assert!(
        message.contains("data directory"),
        "the refusal lost its cause:\n{message}"
    );
    assert_other(&message);
}

#[test]
fn an_incompatible_store_fails_the_binary_with_the_reset_command() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a runtime");
    let data_dir = runtime.block_on(seeded());
    runtime.block_on(async {
        let tampered = Database::at(data_dir.path());
        tampered.edit_the_first_migration().await;
        tampered.checkpoint().await;
    });
    let kept_database = std::fs::read(database(data_dir.path())).expect("the database");
    let kept_key = std::fs::read(key(data_dir.path())).expect("the encryption key");

    let output = Command::new(env!("CARGO_BIN_EXE_kestrel-control-plane"))
        .arg("serve")
        .env("KESTREL_DATA_DIR", data_dir.path())
        .env("KESTREL_LISTEN", "127.0.0.1:0")
        .env("KESTREL_OPERATOR_LISTEN", "127.0.0.1:0")
        .output()
        .expect("the control plane runs");

    assert!(
        !output.status.success(),
        "an incompatible store started the control plane"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_incompatible(&stderr);
    assert_eq!(
        std::fs::read(database(data_dir.path())).expect("the database"),
        kept_database,
        "the failed startup changed the database"
    );
    assert_eq!(
        std::fs::read(key(data_dir.path())).expect("the encryption key"),
        kept_key,
        "the failed startup changed the encryption key"
    );
}
