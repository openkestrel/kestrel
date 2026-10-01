//! An Agent's model, mode and thought level are configuration rather than a rebuild (ADR-0007).

use anyhow::Result;

use crate::domain::{Agent, Declared};
use crate::store::{Declared as DeclaredRecord, Store};

pub async fn declare(
    store: &Store,
    organization: &str,
    name: &str,
    harness: &str,
    declared: &Declared,
) -> Result<DeclaredRecord<Agent>> {
    let declared = Declared::named(declared.clone());
    let mut tx = store.begin().await?;
    let organization = tx.organizations().named(organization).await?;

    let declared = tx
        .agents()
        .declare(&organization, name, harness, &declared)
        .await?;
    tx.commit().await?;

    Ok(declared)
}

pub async fn set_model(
    store: &Store,
    organization: &str,
    name: &str,
    model: Option<&str>,
) -> Result<Agent> {
    let model = names(model);
    let mut tx = store.begin().await?;
    let organization = tx.organizations().named(organization).await?;
    let agent = tx.agents().named(&organization, name).await?;

    let agent = tx.agents().set_model(&agent, model).await?;
    tx.commit().await?;

    Ok(agent)
}

pub async fn agents(store: &Store, organization: &str) -> Result<Vec<Agent>> {
    let mut tx = store.begin().await?;
    let organization = tx.organizations().named(organization).await?;

    tx.agents().all(&organization).await
}

/// A model named as nothing is a model nobody named.
fn names(model: Option<&str>) -> Option<&str> {
    model.filter(|model| !model.is_empty())
}
