use std::collections::HashMap;

use anyhow::Result;

use super::address::{address, cloned_into};
use crate::declined::{Constraint, Reason};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    pub given: String,
    pub address: String,
    pub directory: String,
}

/// Public-repository setup selects no Integration, so it takes only what an anonymous HTTPS clone
/// can read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Purpose {
    Declaration,
    PublicSetup,
}

/// Checks syntax and checkout directories only: no source is contacted, and a local path names
/// the Instance's filesystem, so a resolved address promises no successful checkout (ADR-0057).
pub fn resolved(
    operation: &'static str,
    field: &'static str,
    repositories: &[String],
    purpose: Purpose,
) -> Result<Vec<Resolved>> {
    let refused = |constraint: Constraint, message: String| Reason::InvalidField {
        field,
        operation,
        constraint,
        allowed: None,
        message,
    };
    if repositories.is_empty() {
        return Err(refused(
            Constraint::NonEmpty,
            "a project names at least one repository".to_owned(),
        )
        .into());
    }

    let mut resolved = Vec::with_capacity(repositories.len());
    let mut claimed = HashMap::new();
    for given in repositories {
        let address = address(given).map_err(|why| refused(Constraint::GitRepository, why))?;
        if purpose == Purpose::PublicSetup && !address.starts_with("https://") {
            return Err(refused(
                Constraint::HttpsRepository,
                format!(
                    "{given} is not an HTTPS address: a public repository is given as an \
                     https:// URL, owner/repo or github.com/owner/repo"
                ),
            )
            .into());
        }
        let directory = cloned_into(&address);
        if matches!(directory, "" | "." | "..") {
            return Err(refused(
                Constraint::CheckoutDirectory,
                format!("{given} names no directory to check it out into"),
            )
            .into());
        }
        if let Some(earlier) = claimed.insert(directory.to_owned(), given) {
            return Err(refused(
                Constraint::DistinctCheckoutDirectories,
                format!("{earlier} and {given} would both be checked out into {directory}"),
            )
            .into());
        }
        resolved.push(Resolved {
            given: given.clone(),
            directory: directory.to_owned(),
            address,
        });
    }

    Ok(resolved)
}
