//! Each value of a start's plan, from what was given, then what is declared, then the clone.

use anyhow::Result;

use crate::declaration::{self, Prefix};
use crate::declined::{Candidate, Constraint, Reason, Resource};
use crate::domain::{Agent, Declared, Organization, Project};
use crate::repository::{self, Identity, Purpose, Resolved};
use crate::store::Tx;

use super::plan::{DEFAULT_HARNESS, OPERATION, Request, Source, Valued, read_from, valued};

pub(super) struct Resolving<'a> {
    pub request: &'a Request,
    pub missing: Vec<anyhow::Error>,
    pub conflicts: Vec<anyhow::Error>,
}

impl Resolving<'_> {
    pub(super) async fn organization(
        &mut self,
        tx: &mut Tx<'_>,
    ) -> Result<(Option<Valued<String>>, Option<Organization>)> {
        if let Some(named) = &self.request.organization {
            let found = tx.organizations().find(named).await?;
            return Ok((Some(valued(named.clone(), Source::Given)), found));
        }
        let mut all = tx.organizations().all().await?;
        match all.len() {
            1 => {
                let only = all.pop().expect("one organization");
                Ok((
                    Some(valued(only.name.clone(), Source::OnlyOrganization)),
                    Some(only),
                ))
            }
            0 => {
                self.missing.push(missing(
                    "organization",
                    "no organization exists; naming the operator declares one, or name one here",
                ));
                Ok((None, None))
            }
            several => {
                let names: Vec<&str> = all.iter().map(|found| found.name.as_str()).collect();
                let message = format!(
                    "{several} organizations exist and none was named: {}",
                    names.join(", ")
                );
                self.missing.push(
                    Reason::AmbiguousReference {
                        resource: Resource::Organization,
                        reference: String::new(),
                        organization: None,
                        message,
                        candidates: all
                            .into_iter()
                            .map(|found| Candidate {
                                id: found.id.to_string(),
                                name: found.name,
                            })
                            .collect(),
                    }
                    .concerning("organization")
                    .into(),
                );
                Ok((None, None))
            }
        }
    }

    pub(super) fn repositories(
        &mut self,
        given: Option<Vec<Resolved>>,
        clone: Option<&Resolved>,
        projects: &[Project],
    ) -> Result<Option<Valued<Vec<Resolved>>>> {
        if let Some(given) = given {
            return Ok(Some(valued(given, Source::Given)));
        }
        let named = self
            .request
            .project
            .as_ref()
            .and_then(|name| projects.iter().find(|found| &found.name == name));
        if let Some(named) = named {
            return Ok(Some(read_from(
                declared(&named.repositories)?,
                Source::Project,
                &named.name,
            )));
        }
        let Some(clone) = clone else {
            self.missing.push(missing(
                "repositories",
                "no repository is given, and no project or clone names one",
            ));
            return Ok(None);
        };
        // A Project named here that is not declared yet is declared against the clone.
        if self.request.project.is_some() {
            return Ok(Some(valued(vec![clone.clone()], Source::CloneOrigin)));
        }
        let declaring: Vec<&Project> = projects
            .iter()
            .filter(|found| {
                found
                    .repositories
                    .first()
                    .is_some_and(|first| Identity::same(first, &clone.address))
            })
            .collect();
        match declaring.as_slice() {
            [one] => Ok(Some(read_from(
                declared(&one.repositories)?,
                Source::CloneOrigin,
                &one.name,
            ))),
            [] => Ok(Some(valued(vec![clone.clone()], Source::CloneOrigin))),
            several => {
                let names: Vec<&str> = several.iter().map(|found| found.name.as_str()).collect();
                self.missing.push(
                    Reason::AmbiguousReference {
                        resource: Resource::Project,
                        reference: clone.address.clone(),
                        organization: None,
                        message: format!(
                            "the projects {} each begin with the clone's origin {}",
                            names.join(", "),
                            clone.address
                        ),
                        candidates: several
                            .iter()
                            .map(|found| Candidate {
                                id: found.id.to_string(),
                                name: found.name.clone(),
                            })
                            .collect(),
                    }
                    .concerning("project")
                    .into(),
                );
                Ok(Some(valued(vec![clone.clone()], Source::CloneOrigin)))
            }
        }
    }

    pub(super) fn project(
        &mut self,
        repositories: Option<&Valued<Vec<Resolved>>>,
        projects: &[Project],
    ) -> Option<Valued<String>> {
        if let Some(named) = &self.request.project {
            return Some(valued(named.clone(), Source::Given));
        }
        let repositories = repositories?;
        if let (Source::CloneOrigin, Some(record)) = (repositories.source, &repositories.record) {
            return Some(read_from(record.clone(), Source::CloneOrigin, record));
        }
        if self.missing_project() {
            return None;
        }
        let wanted = addresses(&repositories.value);
        if let Some(found) = projects.iter().find(|found| found.repositories == wanted) {
            return Some(read_from(found.name.clone(), Source::Project, &found.name));
        }
        let name = repositories.value.first()?.directory.clone();
        if projects.iter().any(|found| found.name == name) {
            return self.missing_project_named(&name);
        }
        Some(valued(name, Source::NamedForRepository))
    }

    fn missing_project_named(&mut self, name: &str) -> Option<Valued<String>> {
        self.missing.push(
            Reason::InvalidField {
                field: "project",
                operation: OPERATION,
                constraint: Constraint::Immutable,
                allowed: None,
                message: format!(
                    "the project {name} is declared against other repositories, and a start \
                     changes no declaration; name another project"
                ),
            }
            .into(),
        );
        None
    }

    fn missing_project(&self) -> bool {
        self.missing.iter().any(|missing| {
            missing
                .downcast_ref::<crate::declined::Concerning>()
                .is_some_and(|concerning| concerning.field == "project")
        })
    }

    pub(super) fn branch(&mut self, declared: Option<&Project>) -> Option<Valued<String>> {
        if let Some(branch) = &self.request.branch {
            return Some(valued(branch.clone(), Source::Given));
        }
        if let Some(found) = declared {
            return Some(read_from(
                found.branch.clone(),
                Source::Project,
                &found.name,
            ));
        }
        let clone = self.request.clone.as_ref();
        let named = |branch: &Option<String>| branch.clone().filter(|branch| !branch.is_empty());
        if let Some(branch) = clone.and_then(|clone| named(&clone.default_branch)) {
            return Some(valued(branch, Source::CloneDefaultBranch));
        }
        if let Some(branch) = clone.and_then(|clone| named(&clone.checked_out)) {
            return Some(valued(branch, Source::CloneCheckedOut));
        }
        self.missing.push(missing(
            "branch",
            "no branch is given, and no project or clone names one to start each workspace from",
        ));
        None
    }

    pub(super) fn agent(
        &mut self,
        agents: &[Agent],
        signed_in: Option<String>,
    ) -> (Valued<String>, Valued<String>, Valued<Option<String>>) {
        let request = self.request;
        let harness_or_model = request.harness.is_some() || request.model.is_some();
        let named = match (&request.agent, agents) {
            (Some(agent), _) => Some(valued(agent.clone(), Source::Given)),
            (None, [only]) if !harness_or_model => {
                Some(valued(only.name.clone(), Source::OnlyAgent))
            }
            _ => None,
        };
        let find = |name: &str| agents.iter().find(|found| found.name == name);
        let found = named.as_ref().and_then(|named| find(&named.value));
        let harness = match (&request.harness, found) {
            (Some(harness), _) => valued(harness.clone(), Source::Given),
            (None, Some(found)) => read_from(found.harness.clone(), Source::Agent, &found.name),
            (None, None) => match signed_in {
                Some(harness) => valued(harness, Source::SavedSignIn),
                None => valued(DEFAULT_HARNESS.to_owned(), Source::Default),
            },
        };
        let agent = named.unwrap_or_else(|| valued(harness.value.clone(), Source::NamedForHarness));
        let found = find(&agent.value);
        let model = match (&request.model, found) {
            (Some(model), _) => valued(Some(model.clone()), Source::Given),
            (None, Some(found)) => {
                read_from(found.declared.model.clone(), Source::Agent, &found.name)
            }
            (None, None) => valued(None, Source::Default),
        };

        if let Some(found) = found {
            let declared = Declared {
                model: model.value.clone(),
                ..found.declared.clone()
            };
            if let Err(conflict) =
                declaration::start_agent_conflict(found, &harness.value, &declared)
            {
                if agent.source == Source::Given {
                    self.conflicts
                        .push(declaration::scoped(Prefix::Agent, OPERATION, conflict));
                } else {
                    self.missing.push(
                        Reason::InvalidField {
                            field: "agent",
                            operation: OPERATION,
                            constraint: Constraint::Immutable,
                            allowed: None,
                            message: format!(
                                "the agent {} is declared differently, and a start changes no \
                                 declaration; name another agent",
                                found.name
                            ),
                        }
                        .into(),
                    );
                }
            }
        }

        (agent, harness, model)
    }
}

pub(super) fn missing(field: &'static str, message: &str) -> anyhow::Error {
    Reason::InvalidField {
        field,
        operation: OPERATION,
        constraint: Constraint::NonEmpty,
        allowed: None,
        message: message.to_owned(),
    }
    .into()
}

fn declared(repositories: &[String]) -> Result<Vec<Resolved>> {
    repository::resolved(
        OPERATION,
        "repositories",
        repositories,
        Purpose::Declaration,
    )
}

pub(super) fn addresses(resolved: &[Resolved]) -> Vec<String> {
    resolved
        .iter()
        .map(|resolved| resolved.address.clone())
        .collect()
}
