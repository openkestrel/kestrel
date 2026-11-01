use anyhow::Result;

use super::address::address;
use super::identity::Identity;
use super::resolution::{Purpose, Resolved, resolved};
use crate::declined::{Constraint, Reason};
use crate::domain::Integration;
use crate::integration::{self, github};
use crate::store::Store;

pub use kestrel_operator_types::RepositoryIntegrationSelection as Selection;

pub struct Selecting<'a> {
    pub operation: &'static str,
    pub repositories_field: &'static str,
    pub integrations_field: &'static str,
    pub organization: &'a str,
    pub repositories: &'a [String],
    pub selections: &'a [Selection],
}

pub struct Declaring<'a> {
    pub selecting: Selecting<'a>,
    pub branch_field: &'static str,
    pub project: &'a str,
    pub branch: &'a str,
}

pub struct Bound {
    pub resolved: Resolved,
    pub integration: Option<Integration>,
}

pub async fn resolution(
    store: &Store,
    selecting: &Selecting<'_>,
    purpose: Purpose,
) -> Result<Vec<Bound>> {
    let refused = |field, constraint, message: &str| Reason::InvalidField {
        field,
        operation: selecting.operation,
        constraint,
        allowed: None,
        message: message.to_owned(),
    };
    if selecting.selections.is_empty() {
        return Ok(resolved(
            selecting.operation,
            selecting.repositories_field,
            selecting.repositories,
            purpose,
        )?
        .into_iter()
        .map(|resolved| Bound {
            resolved,
            integration: None,
        })
        .collect());
    }
    if purpose == Purpose::PublicSetup {
        return Err(refused(
            selecting.integrations_field,
            Constraint::DeclarationPurpose,
            "public-repository setup selects no integration; resolve a declaration to select one",
        )
        .into());
    }
    if selecting.organization.is_empty() {
        return Err(refused(
            "organization",
            Constraint::NonEmpty,
            "an integration is selected within an organization; name it",
        )
        .into());
    }

    bound(store, selecting).await
}

/// Asks GitHub nothing: which Integration each repository selects, and the HTTPS address a
/// selected one is checked out by, since an App lends no SSH authority (ADR-0057).
pub async fn bound(store: &Store, selecting: &Selecting<'_>) -> Result<Vec<Bound>> {
    let mut bound: Vec<Bound> = resolved(
        selecting.operation,
        selecting.repositories_field,
        selecting.repositories,
        Purpose::Declaration,
    )?
    .into_iter()
    .map(|resolved| Bound {
        resolved,
        integration: None,
    })
    .collect();
    if selecting.selections.is_empty() {
        return Ok(bound);
    }

    let refused = |constraint: Constraint, message: String| Reason::InvalidField {
        field: selecting.integrations_field,
        operation: selecting.operation,
        constraint,
        allowed: None,
        message,
    };
    let mut tx = store.read().await?;
    let organization = tx.organizations().named(selecting.organization).await?;
    for selection in selecting.selections {
        address(&selection.repository).map_err(|why| refused(Constraint::GitRepository, why))?;
        let Some(repository) = bound
            .iter_mut()
            .find(|bound| Identity::same(&bound.resolved.address, &selection.repository))
        else {
            return Err(refused(
                Constraint::DeclaredRepository,
                format!(
                    "{} is given the integration {}, and the project does not declare it",
                    selection.repository, selection.integration
                ),
            )
            .into());
        };
        if repository.integration.is_some() {
            return Err(refused(
                Constraint::DeclaredRepository,
                format!("{} is given an integration twice", selection.repository),
            )
            .into());
        }
        let integration = tx
            .integrations()
            .named(&organization, &selection.integration)
            .await?;
        let connection =
            integration::lending(&integration, &organization.name, selecting.operation)?;
        let Some(names) = github::named_repository(&repository.resolved.address)
            .filter(|names| names.eq_ignore_ascii_case(&connection.repository))
        else {
            return Err(refused(
                Constraint::WatchedByIntegration,
                format!(
                    "the integration {} watches {}, which {} does not name",
                    integration.name, connection.repository, selection.repository
                ),
            )
            .into());
        };
        if !repository
            .resolved
            .address
            .get(..8)
            .is_some_and(|scheme| scheme.eq_ignore_ascii_case("https://"))
        {
            repository.resolved.address = github::https_address(&names);
        }
        repository.integration = Some(integration);
    }

    Ok(bound)
}
