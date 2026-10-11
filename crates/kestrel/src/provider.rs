//! A Provider Credential is held by the Organization rather than by an Agent, and is encrypted
//! at rest with the key beside the database.

use anyhow::{Result, bail};
use jiff::Timestamp;

use crate::declined::{Constraint, Reason, Resource};
use crate::domain::OrganizationId;
use crate::sign_in::{Authentication, Source};
use crate::store::Store;

/// A credential as everything but the spawn that carries it sees it: what it is read from,
/// and never what it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Held {
    pub variable: String,
    pub set_at: Timestamp,
    pub revision: i64,
}

pub async fn hold(store: &Store, organization: &str, variable: &str, secret: &str) -> Result<Held> {
    holdable("set_provider_credential", variable, secret)?;

    let mut tx = store.begin().await?;
    let organization = tx.organizations().named(organization).await?;
    let held = tx
        .organizations()
        .hold_provider_credential(
            organization.id,
            variable,
            secret,
            &Authentication::unchecked(Source::GenericWrite),
        )
        .await?;
    tx.commit().await?;

    Ok(held)
}

pub async fn held(store: &Store, organization: &str) -> Result<Vec<Held>> {
    let mut tx = store.begin().await?;
    let organization = tx.organizations().named(organization).await?;

    tx.organizations()
        .provider_credentials_held(organization.id)
        .await
}

pub async fn forget(store: &Store, organization: &str, variable: &str) -> Result<()> {
    let mut tx = store.begin().await?;
    let organization = tx.organizations().named(organization).await?;

    if !tx
        .organizations()
        .forget_provider_credential(organization.id, variable)
        .await?
    {
        bail!(Reason::MissingReference {
            resource: Resource::ProviderCredential,
            reference: variable.to_owned(),
            organization: Some(organization.name.clone()),
            within: None,
            message: format!(
                "the organization {} holds no provider credential named {variable}",
                organization.name
            ),
        });
    }

    tx.commit().await
}

pub(crate) fn holdable(operation: &'static str, variable: &str, secret: &str) -> Result<()> {
    named(operation, variable)?;
    if secret.is_empty() {
        bail!(Reason::InvalidField {
            field: "secret",
            operation,
            constraint: Constraint::NonEmpty,
            allowed: None,
            message: "a provider credential with nothing in it is not one".to_owned(),
        });
    }

    Ok(())
}

/// Asked before an Instance is provisioned, so nothing is decrypted to answer it.
pub async fn holds_any(store: &Store, organization: OrganizationId) -> Result<bool> {
    Ok(!store
        .begin()
        .await?
        .organizations()
        .provider_credentials_held(organization)
        .await?
        .is_empty())
}

pub(crate) fn named(operation: &'static str, variable: &str) -> Result<()> {
    if variable.starts_with("KESTREL_") {
        bail!(Reason::InvalidField {
            field: "variable",
            operation,
            constraint: Constraint::NotReserved,
            allowed: None,
            message: "KESTREL_ is reserved for the supervisor".to_owned(),
        });
    }

    let acceptable = variable
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || character == '_');
    let starts = variable
        .chars()
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic() || first == '_');

    if !acceptable || !starts {
        bail!(Reason::InvalidField {
            field: "variable",
            operation,
            constraint: Constraint::EnvironmentVariableName,
            allowed: None,
            message: format!(
                "{variable} is not an environment variable a Harness could be spawned with"
            ),
        });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_credential_is_named_by_the_variable_a_harness_reads_it_from() {
        assert!(named("set_provider_credential", "ANTHROPIC_API_KEY").is_ok());
        assert!(named("set_provider_credential", "_KEY2").is_ok());
    }

    #[test]
    fn a_name_no_process_could_carry_is_refused() {
        for refused in ["", "2KEY", "A KEY", "A=KEY", "A-KEY", "clé"] {
            assert!(
                named("set_provider_credential", refused).is_err(),
                "{refused} was accepted"
            );
        }
    }
}
