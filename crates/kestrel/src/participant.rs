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
    let name = writable(name, FIELD, operation)?;
    if tx.agents().find(organization, name).await?.is_some() {
        bail!(Reason::InvalidField {
            field: FIELD,
            operation,
            constraint: Constraint::NotAnAgentName,
            allowed: None,
            message: format!(
                "{name} is an Agent in this Organization, and only its own output is recorded under its name"
            ),
        });
    }

    Ok(name.to_owned())
}

/// The part of the rule no Organization decides, which an Operator's name is held to as well.
pub fn writable<'a>(
    name: &'a str,
    field: &'static str,
    operation: &'static str,
) -> Result<&'a str> {
    let name = name.trim();
    let refuse = |constraint: Constraint, message: String| -> Result<&'a str> {
        bail!(Reason::InvalidField {
            field,
            operation,
            constraint,
            allowed: None,
            message,
        })
    };

    if name.is_empty() {
        return refuse(Constraint::NonEmpty, format!("a {field} cannot be empty"));
    }
    if name.chars().count() > LONGEST {
        return refuse(
            Constraint::MaxLength,
            format!(
                "a {field} is at most {LONGEST} characters, not {}",
                name.chars().count()
            ),
        );
    }
    if name.chars().any(char::is_control) {
        return refuse(
            Constraint::NoControlCharacters,
            format!("a {field} cannot contain a control character"),
        );
    }

    Ok(name)
}

/// The Operator as an author, where one is named.
pub async fn operator(
    tx: &mut Tx<'_>,
    organization: &Organization,
    operation: &'static str,
) -> Result<Option<String>> {
    match tx.operators().current().await? {
        Some(operator) => Ok(Some(
            accepted(tx, organization, &operator.name, operation).await?,
        )),
        None => Ok(None),
    }
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
        None => operator(tx, organization, operation).await?.ok_or_else(|| {
            Reason::MissingOperator { operation }
                .concerning(FIELD)
                .into()
        }),
    }
}
