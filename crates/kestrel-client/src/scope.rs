//! Scope is derived for every invocation and never stored (ADR-0016).

use std::fmt;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result, bail};
use kestrel_operator_types as wire;

use crate::api::ControlPlane;
use crate::exit::{Exit, Failed};
use crate::shell::quoted;
use crate::{BINARY, ORGANIZATION_VARIABLE};

const BINDING: &str = ".kestrel/organization";

pub struct Scope {
    pub organization: String,
    pub source: Source,
}

pub enum Source {
    Flag,
    Environment,
    Binding(PathBuf),
    OnlyOrganization,
}

impl fmt::Display for Source {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Source::Flag => formatter.write_str("--organization"),
            Source::Environment => formatter.write_str(ORGANIZATION_VARIABLE),
            Source::Binding(binding) => write!(formatter, "{}", binding.display()),
            Source::OnlyOrganization => formatter.write_str("only organization"),
        }
    }
}

pub enum Derived {
    Scope(Scope),
    Unnamed { existing: Vec<String> },
}

pub struct Scoping<'a> {
    api: &'a ControlPlane,
    named: Option<Scope>,
}

enum Binding {
    Scope(Scope),
    Empty(PathBuf),
    None,
}

impl<'a> Scoping<'a> {
    pub fn new(api: &'a ControlPlane, named: Option<Scope>) -> Self {
        Self { api, named }
    }

    pub async fn resolve(self) -> Result<Scope> {
        let api = self.api;
        match self.derive().await? {
            Derived::Scope(scope) => Ok(scope),
            Derived::Unnamed { existing } if existing.is_empty() => {
                Err(nothing_to_scope(api).await)
            }
            Derived::Unnamed { existing } => bail!(several(&existing)),
        }
    }

    pub async fn derive(self) -> Result<Derived> {
        if let Some(scope) = self.named {
            return Ok(Derived::Scope(scope));
        }
        match bound()? {
            Binding::Scope(scope) => return Ok(Derived::Scope(scope)),
            Binding::Empty(path) => {
                let existing = organizations(self.api).await?;
                let next = existing.first().map_or_else(
                    || format!("`{BINARY} organization declare default`, then `{BINARY} status --organization default`"),
                    |name| format!("`{BINARY} status --organization {}`", quoted(name)),
                );
                bail!(Failed::new(
                    Exit::Unresolved,
                    format!("{} binds no Organization; try {next}", path.display())
                ));
            }
            Binding::None => {}
        }

        let mut existing = organizations(self.api).await?;
        if existing.len() == 1 {
            return Ok(Derived::Scope(Scope {
                organization: existing.remove(0),
                source: Source::OnlyOrganization,
            }));
        }

        Ok(Derived::Unnamed { existing })
    }
}

pub async fn organizations(api: &ControlPlane) -> Result<Vec<String>> {
    let organizations: wire::ListOrganizationsResponse = api.get_as(&["organizations"]).await?;

    Ok(organizations
        .into_iter()
        .map(|organization| organization.name)
        .collect())
}

pub fn several(existing: &[String]) -> Failed {
    let choices = existing
        .iter()
        .map(|name| format!("`{BINARY} status --organization {}`", quoted(name)))
        .collect::<Vec<_>>()
        .join(" or ");
    Failed::new(
        Exit::Unresolved,
        format!(
            "no Organization is in scope and {} exist: {}; inspect the intended scope with {choices}, then pass its --organization flag",
            existing.len(),
            existing.join(", ")
        ),
    )
}

/// Naming the Operator declares their Organization, so with neither the gap is the name.
pub async fn nothing_to_scope(api: &ControlPlane) -> anyhow::Error {
    match api.get_as::<wire::Operator>(&["operator"]).await {
        Ok(_) => Failed::new(
            Exit::Unresolved,
            format!(
                "no Organization is in scope and none exists; declare one with \
                 `{BINARY} organization declare default`"
            ),
        )
        .into(),
        Err(refused) => refused,
    }
}

fn bound() -> Result<Binding> {
    let working = std::env::current_dir().context("reading the working directory")?;
    let Some(binding) = binding_for(&working) else {
        return Ok(Binding::None);
    };

    let organization = std::fs::read_to_string(&binding)
        .with_context(|| format!("reading {}", binding.display()))?
        .trim()
        .to_owned();
    if organization.is_empty() {
        return Ok(Binding::Empty(binding));
    }

    Ok(Binding::Scope(Scope {
        organization,
        source: Source::Binding(binding),
    }))
}

// The search stops at the repository root, so a binding in a home directory never becomes a
// remembered current Organization.
fn binding_for(working: &Path) -> Option<PathBuf> {
    let root = working
        .ancestors()
        .find(|directory| directory.join(".git").exists());

    for directory in working.ancestors() {
        let binding = directory.join(BINDING);
        if binding.is_file() {
            return Some(binding);
        }
        if root.is_none_or(|root| directory == root) {
            break;
        }
    }

    None
}
