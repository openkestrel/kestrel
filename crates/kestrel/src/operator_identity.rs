use anyhow::{Result, bail};

use crate::declined::{Constraint, Reason};
use crate::domain::Operator;
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

pub async fn name(store: &Store, name: &str) -> Result<Declared<Operator>> {
    let name = name.trim();
    if name.is_empty() {
        bail!(Reason::InvalidField {
            field: "name",
            operation: "name_operator",
            constraint: Constraint::NonEmpty,
            allowed: None,
            message: "the Operator needs a name".to_owned(),
        });
    }
    let mut tx = store.begin().await?;
    let declared = tx.operators().name(name).await?;
    if declared.created && tx.organizations().all().await?.is_empty() {
        tx.organizations().declare(name, None).await?;
    }
    tx.commit().await?;
    Ok(declared)
}
