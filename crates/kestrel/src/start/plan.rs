//! A start's plan, resolved from what a Client gave and what exists. Resolving writes nothing,
//! asks no provider or GitHub, and holds no secret it could return.

use anyhow::Result;
use kestrel_operator_types::{SignInFill, SignInMethodOwnership};

use crate::capability::Images;
use crate::catalogue;
use crate::declaration::{self, Prefix};
use crate::declined::Reason;
use crate::domain::Organization;
use crate::repository::{self, Purpose, Resolved, Selecting, Selection};
use crate::start_readiness::{self, Context, Gap, Readiness};
use crate::store::{Store, Tx};
use crate::workspace;

use super::resolving::{Resolving, addresses, missing};

/// The operation a Client corrects a missing or conflicting input for.
pub const OPERATION: &str = "start";
pub(super) const DEFAULT_HARNESS: &str = "opencode";

#[derive(Debug, Default, Clone)]
pub struct Request {
    pub organization: Option<String>,
    pub project: Option<String>,
    pub repositories: Vec<String>,
    pub integrations: Vec<Selection>,
    pub branch: Option<String>,
    pub agent: Option<String>,
    pub harness: Option<String>,
    pub model: Option<String>,
    pub profile: Option<String>,
    pub sign_in_method: Option<String>,
    pub brief: Option<String>,
    pub credentials: Vec<Credential>,
    pub clone: Option<LocalClone>,
}

#[derive(Debug, Clone)]
pub struct Credential {
    pub variable: String,
    pub secret: Option<String>,
}

#[derive(Debug, Clone)]
pub struct LocalClone {
    pub repository: String,
    pub default_branch: Option<String>,
    pub checked_out: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Given,
    CloneOrigin,
    CloneDefaultBranch,
    CloneCheckedOut,
    OnlyOrganization,
    Project,
    OnlyAgent,
    Agent,
    NamedForRepository,
    NamedForHarness,
    SavedSignIn,
    OperatorProfile,
    Default,
}

#[derive(Debug, Clone)]
pub struct Valued<T> {
    pub value: T,
    pub source: Source,
    /// The declared record the value was read from.
    pub record: Option<String>,
}

pub(super) fn valued<T>(value: T, source: Source) -> Valued<T> {
    Valued {
        value,
        source,
        record: None,
    }
}

pub(super) fn read_from<T>(value: T, source: Source, record: &str) -> Valued<T> {
    Valued {
        value,
        source,
        record: Some(record.to_owned()),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Organization,
    Project,
    Agent,
    ProviderCredential,
    Workspace,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Declare,
    Reuse,
    Hold,
    Replace,
    Open,
}

#[derive(Debug)]
pub struct Step {
    pub kind: Kind,
    pub name: Option<String>,
    pub action: Action,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Ready,
    Incomplete,
    Blocked,
    Unavailable,
}

#[derive(Debug)]
pub struct Preview {
    pub organization: Option<Valued<String>>,
    pub project: Option<Valued<String>>,
    pub repositories: Option<Valued<Vec<Resolved>>>,
    /// The Integration each repository selects, by name.
    pub integrations: Vec<Selection>,
    pub branch: Option<Valued<String>>,
    pub agent: Valued<String>,
    pub harness: Valued<String>,
    pub model: Valued<Option<String>>,
    pub profile: Valued<Option<String>>,
    pub sign_in_method: Valued<Option<String>>,
    pub declarations: Vec<Step>,
    /// The path's own: its gaps are only those a start does not satisfy itself.
    pub readiness: Readiness,
    pub missing: Vec<anyhow::Error>,
    pub conflicts: Vec<anyhow::Error>,
}

impl Preview {
    pub fn outcome(&self) -> Outcome {
        if !self.readiness.gaps.is_empty() || !self.conflicts.is_empty() {
            Outcome::Blocked
        } else if !self.missing.is_empty() {
            Outcome::Incomplete
        } else if self.readiness.uninspectable.is_some() {
            Outcome::Unavailable
        } else {
            Outcome::Ready
        }
    }
}

impl Request {
    fn named(&self) -> Self {
        let named = |value: &Option<String>| value.clone().filter(|value| !value.trim().is_empty());
        Self {
            organization: named(&self.organization),
            project: named(&self.project),
            branch: named(&self.branch),
            agent: named(&self.agent),
            harness: named(&self.harness),
            model: named(&self.model),
            profile: named(&self.profile),
            sign_in_method: named(&self.sign_in_method),
            brief: named(&self.brief),
            ..self.clone()
        }
    }
}

/// Everything a start of `request` would resolve now, with every missing input and gap that can
/// be established rather than the first.
pub async fn preview(store: &Store, images: &Images, request: &Request) -> Result<Preview> {
    let request = request.named();
    start_readiness::offered(&Context {
        harness: request.harness.clone(),
        sign_in_method: request.sign_in_method.clone(),
        ..Context::default()
    })?;
    // A repository no declaration could hold is malformed input, not a gap.
    let given = if request.repositories.is_empty() {
        None
    } else {
        Some(
            repository::resolution(
                store,
                &Selecting {
                    operation: OPERATION,
                    repositories_field: "repositories",
                    integrations_field: "integrations",
                    organization: request.organization.as_deref().unwrap_or_default(),
                    repositories: &request.repositories,
                    selections: &request.integrations,
                },
                Purpose::Declaration,
            )
            .await?
            .into_iter()
            .map(|bound| bound.resolved)
            .collect::<Vec<_>>(),
        )
    };
    let clone = match &request.clone {
        Some(clone) => repository::resolved(
            OPERATION,
            "clone.repository",
            std::slice::from_ref(&clone.repository),
            Purpose::Declaration,
        )?
        .pop(),
        None => None,
    };
    // Before the transaction, so a slow daemon holds no read open.
    let capability = images.read().await;

    let mut tx = store.read().await?;
    let mut resolving = Resolving {
        request: &request,
        missing: Vec::new(),
        conflicts: Vec::new(),
    };
    let (organization, record) = resolving.organization(&mut tx).await?;
    let (projects, agents, held) = match &record {
        Some(organization) => (
            tx.projects().all(organization).await?,
            tx.agents().all(organization).await?,
            tx.organizations()
                .provider_credentials_held(organization.id)
                .await?
                .into_iter()
                .map(|held| held.variable)
                .collect(),
        ),
        None => (Vec::new(), Vec::new(), Vec::<String>::new()),
    };

    let repositories = resolving.repositories(given, clone.as_ref(), &projects)?;
    let project = resolving.project(repositories.as_ref(), &projects);
    let declared_project = project
        .as_ref()
        .and_then(|project| projects.iter().find(|found| found.name == project.value));
    let branch = resolving.branch(declared_project);
    let integrations = if !request.integrations.is_empty() {
        request.integrations.clone()
    } else {
        declared_project
            .filter(|found| {
                repositories.as_ref().is_some_and(|repositories| {
                    addresses(&repositories.value) == found.repositories
                })
            })
            .map(|found| {
                found
                    .integrations
                    .iter()
                    .map(|selected| Selection {
                        repository: selected.repository.clone(),
                        integration: selected.name.clone(),
                    })
                    .collect()
            })
            .unwrap_or_default()
    };
    if let (Some(found), Some(repositories), Some(branch)) =
        (declared_project, &repositories, &branch)
    {
        let selected = integrations
            .iter()
            .map(|selected| (selected.repository.as_str(), selected.integration.as_str()))
            .collect();
        if let Err(conflict) = declaration::start_project_conflict(
            found,
            &addresses(&repositories.value),
            selected,
            &branch.value,
        ) {
            resolving
                .conflicts
                .push(declaration::scoped(Prefix::Project, OPERATION, conflict));
        }
    }

    // Any path first, so the one harness a usable sign-in serves can name the Agent.
    let any = start_readiness::assessed(
        &mut tx,
        &capability,
        record.clone(),
        &Context {
            profile: request.profile.clone(),
            sign_in_method: request.sign_in_method.clone(),
            ..Context::default()
        },
    )
    .await?;
    let (agent, harness, model) = resolving.agent(&agents, any.selected.harness);
    let path = start_readiness::assessed(
        &mut tx,
        &capability,
        record.clone(),
        &Context {
            harness: Some(harness.value.clone()),
            model: model.value.clone(),
            profile: request.profile.clone(),
            sign_in_method: request.sign_in_method.clone(),
            ..Context::default()
        },
    )
    .await?;
    let (profile, sign_in_method) = selection(&mut tx, &request, record.as_ref(), &harness).await?;
    if request.brief.is_none() {
        resolving.missing.push(missing(
            "brief",
            "a start carries a Brief, the initial prompt that tells the Session what to do",
        ));
    }

    let declarations = steps(&Declaring {
        request: &request,
        organization: organization.as_ref(),
        declared: record.is_some(),
        project: project.as_ref(),
        project_declared: declared_project.is_some(),
        agent: &agent,
        agent_declared: agents.iter().any(|found| found.name == agent.value),
        held: &held,
    });
    let readiness = satisfied(path, &request, &harness.value, organization.is_some());

    Ok(Preview {
        organization,
        project,
        repositories,
        integrations,
        branch,
        agent,
        harness,
        model,
        profile,
        sign_in_method,
        declarations,
        readiness,
        missing: resolving.missing,
        conflicts: resolving.conflicts,
    })
}

/// The Profile and method an open naming neither would take: the producer's own choice.
async fn selection(
    tx: &mut Tx<'_>,
    request: &Request,
    organization: Option<&Organization>,
    harness: &Valued<String>,
) -> Result<(Valued<Option<String>>, Valued<Option<String>>)> {
    let given_method = request
        .sign_in_method
        .clone()
        .map(|method| valued(Some(method), Source::Given));
    if let Some(profile) = &request.profile {
        return Ok((
            valued(Some(profile.clone()), Source::Given),
            given_method.unwrap_or_else(|| valued(None, Source::Default)),
        ));
    }
    let chosen = match organization {
        Some(organization) => {
            match workspace::operators_own(
                tx,
                OPERATION,
                organization,
                &harness.value,
                request.sign_in_method.as_deref(),
            )
            .await
            {
                Ok((profile, method)) => Some((profile, method.map(str::to_owned))),
                // Readiness reports the refusal as the path's gap.
                Err(error) if error.downcast_ref::<Reason>().is_some() => None,
                Err(error) => return Err(error),
            }
        }
        None => None,
    };
    let (profile, method) = chosen.unwrap_or((None, None));
    let profile = match profile {
        Some(profile) => valued(Some(profile.name), Source::OperatorProfile),
        None => valued(None, Source::Default),
    };
    let method = given_method.unwrap_or_else(|| match method {
        Some(method) => valued(Some(method), Source::OperatorProfile),
        None => valued(None, Source::Default),
    });

    Ok((profile, method))
}

struct Declaring<'a> {
    request: &'a Request,
    organization: Option<&'a Valued<String>>,
    declared: bool,
    project: Option<&'a Valued<String>>,
    project_declared: bool,
    agent: &'a Valued<String>,
    agent_declared: bool,
    held: &'a [String],
}

fn steps(plan: &Declaring<'_>) -> Vec<Step> {
    let reused = |declared: bool| {
        if declared {
            Action::Reuse
        } else {
            Action::Declare
        }
    };
    let mut steps = Vec::new();
    if let Some(organization) = plan.organization {
        steps.push(Step {
            kind: Kind::Organization,
            name: Some(organization.value.clone()),
            action: reused(plan.declared),
        });
    }
    if let Some(project) = plan.project {
        steps.push(Step {
            kind: Kind::Project,
            name: Some(project.value.clone()),
            action: reused(plan.project_declared),
        });
    }
    steps.push(Step {
        kind: Kind::Agent,
        name: Some(plan.agent.value.clone()),
        action: reused(plan.agent_declared),
    });
    for credential in &plan.request.credentials {
        steps.push(Step {
            kind: Kind::ProviderCredential,
            name: Some(credential.variable.clone()),
            action: if plan.held.contains(&credential.variable) {
                Action::Replace
            } else {
                Action::Hold
            },
        });
    }
    steps.push(Step {
        kind: Kind::Workspace,
        name: None,
        action: Action::Open,
    });

    steps
}

/// A start declares its own Project, so readiness's repository gaps are its own inputs; a key the
/// plan holds for the harness is the sign-in readiness found missing.
fn satisfied(
    mut readiness: Readiness,
    request: &Request,
    harness: &str,
    organization: bool,
) -> Readiness {
    readiness.gaps.retain(|gap| match gap {
        Gap::NoRepository | Gap::NoProject => false,
        // Where nothing names the Organization, nothing establishes what it holds.
        Gap::NoSignIn { .. } => {
            organization
                && !request
                    .credentials
                    .iter()
                    .any(|credential| fills(harness, &credential.variable))
        }
        Gap::Refused(_) | Gap::NeedsAttention(_) => true,
    });
    readiness
}

fn fills(harness: &str, variable: &str) -> bool {
    catalogue::harnesses()
        .iter()
        .filter(|row| row.name == harness)
        .flat_map(|row| &row.sign_in_methods)
        .any(|method| {
            method.ownership == SignInMethodOwnership::Organization
                && matches!(&method.fills, SignInFill::SignInVariableFill(fill) if fill.variable == variable)
        })
}
