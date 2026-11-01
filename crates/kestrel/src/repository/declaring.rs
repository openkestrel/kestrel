use anyhow::Result;

use super::selection::{Declaring, Selecting, bound};
use crate::declined::{Again, Constraint, Next, Reason, Resource};
use crate::domain::{Integration, Organization, Project, SelectedIntegration};
use crate::integration::{self, github::Github};
use crate::store::{Declared, Store, Tx};

struct Checked {
    repository: String,
    integration: Integration,
    /// False when the Project already held this binding on this branch as it was read.
    verified: bool,
}

pub struct Declarable {
    operation: &'static str,
    repositories: Vec<String>,
    branch: String,
    selected: Vec<Checked>,
}

/// GitHub's answer says the App reaches the repository and branch now, never that an Instance
/// will check them out (ADR-0057).
pub async fn declarable(
    store: &Store,
    github: &Github,
    declaring: &Declaring<'_>,
) -> Result<Declarable> {
    let Selecting {
        operation,
        organization,
        ..
    } = declaring.selecting;
    let bound = bound(store, &declaring.selecting).await?;
    let repositories = bound
        .iter()
        .map(|bound| bound.resolved.address.clone())
        .collect();
    let mut declarable = Declarable {
        operation,
        repositories,
        branch: declaring.branch.to_owned(),
        selected: Vec::new(),
    };
    if bound.iter().all(|bound| bound.integration.is_none()) {
        return Ok(declarable);
    }

    let held = {
        let mut tx = store.read().await?;
        let organization = tx.organizations().named(organization).await?;
        tx.projects().find(&organization, declaring.project).await?
    };
    for bound in bound {
        let Some(integration) = bound.integration else {
            continue;
        };
        let mut checked = Checked {
            repository: bound.resolved.address,
            integration,
            verified: false,
        };
        if !declarable.holds(held.as_ref(), &checked) {
            let connection = checked.integration.github()?;
            match github.reaches(connection, declaring.branch).await {
                Ok(true) => checked.verified = true,
                Ok(false) => {
                    return Err(Reason::InvalidField {
                        field: declaring.branch_field,
                        operation,
                        constraint: Constraint::BranchInRepository,
                        allowed: None,
                        message: format!(
                            "{} has no branch {} that the integration {} can see",
                            connection.repository, declaring.branch, checked.integration.name
                        ),
                    }
                    .into());
                }
                Err(unproven) => {
                    return Err(integration::unreached(
                        &checked.integration,
                        organization,
                        operation,
                        unproven,
                        Again::Request,
                        store.clock().now(),
                    )
                    .into());
                }
            }
        }
        declarable.selected.push(checked);
    }

    Ok(declarable)
}

impl Declarable {
    pub fn repositories(&self) -> &[String] {
        &self.repositories
    }

    pub fn branch(&self) -> &str {
        &self.branch
    }

    pub fn selected(&self) -> impl Iterator<Item = (&str, &str)> {
        self.selected.iter().map(|checked| {
            (
                checked.repository.as_str(),
                checked.integration.name.as_str(),
            )
        })
    }

    pub fn describes(&self, project: &Project) -> bool {
        project.repositories == self.repositories
            && project.branch == self.branch
            && project.integrations.len() == self.selected.len()
            && self
                .selected
                .iter()
                .all(|checked| self.holds(Some(project), checked))
    }

    fn holds(&self, project: Option<&Project>, checked: &Checked) -> bool {
        project.is_some_and(|project| {
            project.branch == self.branch
                && project.integrations.iter().any(|selected| {
                    selected.repository == checked.repository
                        && selected.integration == checked.integration.id
                })
        })
    }

    /// Whatever was checked is saved only if nothing it was checked against has moved since.
    pub async fn declare(
        &self,
        tx: &mut Tx<'_>,
        organization: &Organization,
        name: &str,
    ) -> Result<Declared<Project>> {
        let held = tx.projects().find(organization, name).await?;
        let mut integrations = Vec::with_capacity(self.selected.len());
        for checked in &self.selected {
            if !self.holds(held.as_ref(), checked) && !checked.verified {
                return Err(Reason::StateConflict {
                    operation: self.operation,
                    resource: Resource::Project,
                    reference: name.to_owned(),
                    organization: Some(organization.name.clone()),
                    state: "revised",
                    holding_session: None,
                    next: Next::inspect(Resource::Project, name),
                    message: format!(
                        "the project {name} changed while its repositories were being \
                         checked; read it again and declare what it should be now"
                    ),
                }
                .into());
            }
            let current = tx.integrations().with_id(checked.integration.id).await?;
            if current.revision != checked.integration.revision {
                return Err(integration::revised(
                    &current,
                    &organization.name,
                    self.operation,
                    checked.integration.revision,
                    Next::inspect(Resource::Integration, current.name.clone()),
                )
                .into());
            }
            integration::lending(&current, &organization.name, self.operation)?;
            integrations.push(SelectedIntegration {
                repository: checked.repository.clone(),
                integration: checked.integration.id,
                name: checked.integration.name.clone(),
            });
        }

        tx.projects()
            .declare(
                organization,
                name,
                &self.repositories,
                &integrations,
                &self.branch,
            )
            .await
    }
}
