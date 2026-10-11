use anyhow::Result;

use crate::declined::Reason;
use crate::domain::Operator;
use crate::participant;
use crate::store::{Declared, Store};

pub async fn current(store: &Store) -> Result<Operator> {
    store
        .read()
        .await?
        .operators()
        .current()
        .await?
        .ok_or_else(|| {
            Reason::MissingOperator {
                operation: "show_operator",
            }
            .into()
        })
}

pub async fn initial_setup_complete(store: &Store) -> Result<bool> {
    store
        .read()
        .await?
        .operators()
        .initial_setup_complete()
        .await
}

pub async fn name(store: &Store, name: &str) -> Result<Declared<Operator>> {
    let name = participant::writable(name, "name", "name_operator")?;
    let mut tx = store.begin().await?;
    let declared = tx.operators().name(name).await?;
    if declared.created && tx.organizations().all().await?.is_empty() {
        tx.organizations().declare(name, None).await?;
    }
    tx.commit().await?;
    Ok(declared)
}
