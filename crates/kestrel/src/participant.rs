//! The one rule every write that names a person uses.

use anyhow::{Result, bail};

use crate::declined::{Constraint, Reason};
use crate::domain::Organization;
use crate::store::Tx;

/// The field a Client is told to fix when a declared name is refused.
pub const FIELD: &str = "participant";
const LONGEST: usize = 64;

/// The one name rule, which hands back the trimmed name: 1 to 64 characters, no control
/// characters, and never the name of an Agent in the Organization.
pub async fn accepted(
    tx: &mut Tx<'_>,
    organization: &Organization,
    name: &str,
    operation: &'static str,
) -> Result<String> {
    let name = name.trim();
    let refuse = |constraint: Constraint, message: String| -> Result<String> {
        bail!(Reason::InvalidField {
            field: FIELD,
            operation,
            constraint,
            allowed: None,
            message,
        })
    };

    if name.is_empty() {
        return refuse(
            Constraint::NonEmpty,
            "a message names its participant".to_owned(),
        );
    }
    if name.chars().count() > LONGEST {
        return refuse(
            Constraint::MaxLength,
            format!(
                "a participant name is at most {LONGEST} characters, not {}",
                name.chars().count()
            ),
        );
    }
    if name.chars().any(char::is_control) {
        return refuse(
            Constraint::NoControlCharacters,
            "a participant name cannot contain a control character".to_owned(),
        );
    }
    if tx.agents().find(organization, name).await?.is_some() {
        return refuse(
            Constraint::NotAnAgentName,
            format!(
                "{name} is an Agent in this Organization, and only its own output is recorded under its name"
            ),
        );
    }

    Ok(name.to_owned())
}

/// A write that names nobody is the Operator's (ADR-0049); a name given is never replaced.
pub async fn named_or_operator(
    tx: &mut Tx<'_>,
    organization: &Organization,
    name: Option<&str>,
    operation: &'static str,
) -> Result<String> {
    match name {
        Some(name) => accepted(tx, organization, name, operation).await,
        None => {
            let operator = tx
                .operators()
                .current()
                .await?
                .ok_or_else(|| Reason::MissingOperator { operation }.concerning(FIELD))?;
            accepted(tx, organization, &operator.name, operation).await
        }
    }
}
