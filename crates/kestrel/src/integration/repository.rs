use anyhow::{Result, bail};
use jiff::Timestamp;
use tracing::warn;

use crate::declined::{Again, Constraint, Next, Reason, Resource, Step};
use crate::domain::{
    Connection, Direction, GithubConnection, Integration, SelectedIntegration, Session, Workspace,
};
use crate::repository::Identity;
use crate::store::Store;

use super::github::{Github, Installed, Minted, Refused, Unproven};
use super::{integration, out_of_use};

/// What lends a repository its authority: an enabled GitHub App that carries outbound, which is
/// what a checkout, a push and a pull request are (ADR-0056).
pub fn lending<'a>(
    integration: &'a Integration,
    organization: &str,
    operation: &'static str,
) -> Result<&'a GithubConnection> {
    let connection = reading(integration, organization, operation)?;
    if !integration.carries(Direction::Outbound) {
        bail!(Reason::StateConflict {
            operation,
            resource: Resource::Integration,
            reference: integration.name.clone(),
            organization: Some(organization.to_owned()),
            state: "outbound_not_carried",
            holding_session: None,
            next: Next::inspect(Resource::Integration, integration.name.clone()),
            message: format!(
                "the integration {} carries nothing outbound, so it lends a repository no \
                 authority; change what it carries to include outbound",
                integration.name
            ),
        });
    }

    Ok(connection)
}

fn reading<'a>(
    integration: &'a Integration,
    organization: &str,
    operation: &'static str,
) -> Result<&'a GithubConnection> {
    let Connection::Github(connection) = &integration.connection else {
        bail!(Reason::Forbidden {
            operation,
            resource: Resource::Integration,
            reference: integration.name.clone(),
            organization: Some(organization.to_owned()),
            constraint: "github_app",
            next: Next::inspect(Resource::Integration, integration.name.clone()),
            message: format!(
                "the integration {} is a generic webhook, which reaches no repository",
                integration.name
            ),
        });
    };
    if !integration.enabled() {
        bail!(out_of_use(integration, organization, operation));
    }

    Ok(connection)
}

/// What a Session's Instance is handed when its `gh` or `git` asks to reach a repository
/// (ADR-0028).
#[derive(Debug)]
pub enum Lent {
    Token(Minted),
    /// Its Workspace fixed no Integration for the repository, and none is looked for elsewhere.
    Unbound,
    Refused(Reason),
    Unavailable {
        retry_after_seconds: Option<i64>,
        message: String,
    },
}

const REACH_GITHUB: &str = "reach_github";
/// Each attempt after the first follows maintenance that landed while a token was being minted.
const LENDING_ATTEMPTS: usize = 3;

/// Lent only as the Integration the Workspace fixed when it opened, read by id and as it is now:
/// never the Project's current binding, an Event's Integration or another that names the
/// repository (ADR-0056).
pub async fn lend(
    store: &Store,
    github: &Github,
    session: &Session,
    repository: Option<&str>,
) -> Result<Lent> {
    for _ in 0..LENDING_ATTEMPTS {
        let (organization, integration) = {
            let mut tx = store.read().await?;
            let workspace = tx.workspaces().get(session.workspace).await?;
            let selected = match selected(&workspace, repository) {
                Ok(Some(selected)) => selected.integration,
                Ok(None) => return Ok(Lent::Unbound),
                Err(refused) => return Ok(Lent::Refused(*refused)),
            };
            (
                workspace.organization.name,
                tx.integrations().with_id(selected).await?,
            )
        };
        let connection = match lending(&integration, &organization, REACH_GITHUB) {
            Ok(connection) => connection,
            Err(refused) => return refused.downcast().map(Lent::Refused),
        };
        let refused = match github.minted(&integration, connection).await {
            Ok(minted) => return Ok(Lent::Token(minted)),
            Err(refused) => refused,
        };
        let now = store
            .read()
            .await?
            .integrations()
            .with_id(integration.id)
            .await?;
        if now.revision != integration.revision || now.state != integration.state {
            continue;
        }

        return Ok(match refused {
            Refused::RateLimited { until } => Lent::Unavailable {
                retry_after_seconds: Some(
                    until.duration_since(store.clock().now()).as_secs().max(0),
                ),
                message: format!(
                    "github is rate limiting the integration {} until {until}",
                    integration.name
                ),
            },
            Refused::Failed(error) => {
                warn!(
                    integration = integration.name,
                    because = %error,
                    "a token could not be minted for a session"
                );
                Lent::Unavailable {
                    retry_after_seconds: None,
                    message: format!(
                        "github did not mint a token for the integration {}",
                        integration.name
                    ),
                }
            }
        });
    }

    Ok(Lent::Unavailable {
        retry_after_seconds: Some(1),
        message: "the integration kept changing while a token was minted for it".to_owned(),
    })
}

/// A request naming no repository is lent the only one the Workspace selected, if it has one.
fn selected<'a>(
    workspace: &'a Workspace,
    repository: Option<&str>,
) -> Result<Option<&'a SelectedIntegration>, Box<Reason>> {
    let Some(asked) = repository else {
        return match workspace.integrations.as_slice() {
            [] => Ok(None),
            [only] => Ok(Some(only)),
            several => Err(Box::new(Reason::InvalidField {
                field: "repository",
                operation: REACH_GITHUB,
                constraint: Constraint::Offered,
                allowed: Some(
                    several
                        .iter()
                        .map(|selected| selected.repository.clone())
                        .collect(),
                ),
                message: format!(
                    "the workspace {} reaches each of its repositories as its own integration; \
                     name the repository to reach",
                    workspace.id
                ),
            })),
        };
    };

    Ok(workspace
        .integrations
        .iter()
        .find(|selected| same_repository(&selected.repository, asked)))
}

/// Only over HTTPS, so a token is never handed to a request that would send it in the clear.
pub(super) fn same_repository(selected: &str, asked: &str) -> bool {
    asked.starts_with("https://") && Identity::same(selected, asked)
}

pub struct Reached {
    pub watched: String,
    pub page: u32,
    pub next_page: Option<u32>,
    pub installed: Installed,
}

pub const REPOSITORIES_PER_PAGE: u32 = 30;
const REPOSITORIES_PER_PAGE_AT_MOST: u32 = 100;

pub async fn repositories(
    store: &Store,
    github: &Github,
    organization: &str,
    name: &str,
    page: Option<u32>,
    per_page: Option<u32>,
) -> Result<Reached> {
    const OPERATION: &str = "list_integration_repositories";
    let counted = |field, value: Option<u32>, otherwise| match value {
        Some(0) => Err(anyhow::Error::from(Reason::InvalidField {
            field,
            operation: OPERATION,
            constraint: Constraint::Positive,
            allowed: None,
            message: format!("{field} counts from 1"),
        })),
        value => Ok(value.unwrap_or(otherwise)),
    };
    let page = counted("page", page, 1)?;
    let per_page =
        counted("per_page", per_page, REPOSITORIES_PER_PAGE)?.min(REPOSITORIES_PER_PAGE_AT_MOST);
    let read = integration(store, organization, name).await?;
    let connection = reading(&read, organization, OPERATION)?;
    let installed = github
        .installed_repositories(connection, page, per_page)
        .await
        .map_err(|unproven| {
            unreached(
                &read,
                organization,
                OPERATION,
                unproven,
                Again::Read,
                store.clock().now(),
            )
        })?;

    Ok(Reached {
        watched: connection.repository.clone(),
        page,
        next_page: (u64::from(page) * u64::from(per_page) < installed.total_count)
            .then(|| page + 1),
        installed,
    })
}

/// Nothing here establishes that a repository does not exist (ADR-0057).
pub fn unreached(
    integration: &Integration,
    organization: &str,
    operation: &'static str,
    unproven: Unproven,
    again: Again,
    now: Timestamp,
) -> Reason {
    let inspect = || Next::inspect(Resource::Integration, integration.name.clone());
    let conflict = |state, next, message: String| Reason::StateConflict {
        operation,
        resource: Resource::Integration,
        reference: integration.name.clone(),
        organization: Some(organization.to_owned()),
        state,
        holding_session: None,
        next,
        message,
    };
    let unavailable = |retry_after_seconds, message: String| Reason::GithubUnavailable {
        operation,
        integration: integration.name.clone(),
        organization: organization.to_owned(),
        retry_after_seconds,
        again,
        message,
    };
    let repository = integration.github().map_or_else(
        |_| "its repository".to_owned(),
        |github| github.repository.clone(),
    );

    match unproven {
        Unproven::Unreadable | Unproven::Rejected => conflict(
            "credentials_rejected",
            inspect().then(Step::ReplacePrivateKey {
                integration: integration.name.clone(),
            }),
            format!(
                "github does not accept the private key the integration {} signs with; replace \
                 it, then try again",
                integration.name
            ),
        ),
        Unproven::Installation => conflict(
            "installation_unreachable",
            inspect(),
            format!(
                "the App's installation does not answer for the integration {}; restore it on \
                 GitHub, then try again",
                integration.name
            ),
        ),
        Unproven::Repository => conflict(
            "repository_unreachable",
            inspect(),
            format!(
                "the integration {} does not reach {repository} through its App's installation, \
                 which does not establish whether the repository exists; give the installation \
                 access to it on GitHub, then try again",
                integration.name
            ),
        ),
        Unproven::RateLimited { until } => unavailable(
            Some(until.duration_since(now).as_secs().max(0)),
            format!("github is rate limiting until {until}, so {repository} was not checked"),
        ),
        Unproven::Unanswered(error) => {
            warn!(
                integration = integration.name,
                because = %error,
                "github could not be read through an integration"
            );
            unavailable(
                None,
                format!("github did not answer, so {repository} was not checked"),
            )
        }
    }
}
