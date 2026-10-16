pub mod credential;
pub mod github;
pub mod manifest;
pub mod post;
pub mod rotation;
pub mod webhook;

pub use rotation::{resume_webhook_secret_rotation, rotate_webhook_secret};

use anyhow::{Context as _, Result, bail};
use jiff::{SignedDuration, Timestamp};
use tracing::warn;

use crate::declined::{Again, Constraint, Declined, Next, Reason, Resource, Step};
use crate::domain::{
    Connection, Direction, Event, EventRecordId, GithubConnection, Integration, IntegrationKind,
    IntegrationState, Occurrence, SelectedIntegration, Session, Workspace,
};
use crate::integration::credential::App;
use crate::integration::github::{Github, Installed, Minted, Refused, Unproven};
use crate::store::Store;
use crate::store::integration::Recorded;

pub struct Registration<'a> {
    pub organization: &'a str,
    pub name: &'a str,
    pub carries: &'a [Direction],
    pub connecting: Connecting<'a>,
}

pub enum Connecting<'a> {
    /// Polled either way; a `signing_secret` also records a Delivery that reaches the webhook.
    Github {
        repository: &'a str,
        api: &'a str,
        app_id: i64,
        installation: i64,
        private_key: &'a str,
        interval: SignedDuration,
        signing_secret: Option<&'a str>,
    },
    Webhook {
        secret: &'a str,
    },
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Polled {
    pub seen: usize,
    pub recorded: usize,
}

pub async fn register(
    store: &Store,
    github: &Github,
    registration: Registration<'_>,
) -> Result<Integration> {
    if registration.carries.is_empty() {
        bail!(Declined::Unacceptable(
            "an integration carries something: name a direction it carries".to_owned()
        ));
    }

    let (connection, webhook_secret) = match registration.connecting {
        Connecting::Github {
            repository,
            api,
            app_id,
            installation,
            private_key,
            interval,
            signing_secret,
        } => {
            if interval <= SignedDuration::ZERO {
                bail!(Declined::Unacceptable(
                    "a poll interval is how long kestrel waits, and cannot be zero or negative"
                        .to_owned()
                ));
            }
            let repository = github::repository(repository)?;
            let credential = App::held(app_id, installation, private_key);
            let bot_login = github
                .app_bot_login(api, &credential)
                .await
                .context("learning the app's own identity")?;
            let repository_id = github
                .repository_id(api, &credential, &repository)
                .await
                .context("learning the repository's id")?;
            (
                Connection::Github(GithubConnection {
                    repository,
                    api: api.to_owned(),
                    credential,
                    bot_login,
                    interval,
                    repository_id,
                }),
                signing_secret,
            )
        }
        Connecting::Webhook { secret } => {
            if registration.carries.contains(&Direction::Outbound) {
                bail!(Declined::Unacceptable(
                    "a generic webhook carries events inbound only".to_owned()
                ));
            }
            (Connection::Webhook, Some(secret))
        }
    };
    if webhook_secret.is_some_and(str::is_empty) {
        bail!(Declined::Unacceptable(
            "a webhook secret with nothing in it is not one".to_owned()
        ));
    }

    let mut tx = store.begin().await?;
    let organization = tx.organizations().named(registration.organization).await?;
    let integration = tx
        .integrations()
        .register(
            &organization,
            registration.name,
            connection,
            registration.carries,
            webhook_secret,
        )
        .await?;
    tx.commit().await?;

    Ok(integration)
}

/// Identity — kind, API origin, repository, App and installation — has no field here because it
/// cannot change in place (ADR-0056).
#[derive(Debug, Default, Clone, Copy)]
pub struct Change<'a> {
    pub name: Option<&'a str>,
    pub carries: Option<&'a [Direction]>,
    pub interval: Option<SignedDuration>,
    pub revision: Option<i64>,
}

pub async fn integration(store: &Store, organization: &str, name: &str) -> Result<Integration> {
    let mut tx = store.read().await?;
    let organization = tx.organizations().named(organization).await?;

    tx.integrations().named(&organization, name).await
}

pub async fn change(
    store: &Store,
    organization: &str,
    name: &str,
    change: Change<'_>,
) -> Result<Integration> {
    maintained(
        store,
        organization,
        name,
        "change_integration",
        change.revision,
        |read| {
            let mut maintained = read.clone();
            let now = Timestamp::now();
            if let Some(name) = change.name {
                maintained.name = name.to_owned();
            }
            if let Some(carries) = change.carries {
                carried(read, carries)?;
                maintained.carries = [Direction::Inbound, Direction::Outbound]
                    .into_iter()
                    .filter(|direction| carries.contains(direction))
                    .collect();
            }
            if let Some(interval) = change.interval {
                let Connection::Github(github) = &mut maintained.connection else {
                    bail!(invalid(
                        "interval",
                        Constraint::Positive,
                        "a generic webhook is never polled, so it has no interval"
                    ));
                };
                if interval <= SignedDuration::ZERO {
                    bail!(invalid(
                        "interval",
                        Constraint::Positive,
                        "a poll interval is how long kestrel waits, and cannot be zero or negative"
                    ));
                }
                github.interval = interval;
                maintained.poll_due_at = maintained.poll_due_at.map(|due| due.min(now + interval));
            }
            let interval =
                |integration: &Integration| integration.github().ok().map(|github| github.interval);
            if maintained.name == read.name
                && maintained.carries == read.carries
                && interval(&maintained) == interval(read)
            {
                return Ok(None);
            }
            if maintained.polled() && !read.polled() {
                maintained.poll_due_at = Some(now);
                maintained.deliveries_read_from = Some(read.deliveries_read_from.unwrap_or(now));
                maintained.last_polled_at = Some(read.last_polled_at.unwrap_or(now));
            }
            Ok(Some(maintained))
        },
    )
    .await
}

pub async fn disable(store: &Store, organization: &str, name: &str) -> Result<Integration> {
    switched(store, organization, name, IntegrationState::Disabled).await
}

pub async fn enable(store: &Store, organization: &str, name: &str) -> Result<Integration> {
    switched(store, organization, name, IntegrationState::Enabled).await
}

pub const LEFT_UNSAID: &str = "its integration was retired, so nothing more is said through it";

/// Erases what it authenticates with on this side only: the App, its installation and its
/// GitHub keys are not kestrel's to remove (ADR-0056).
pub async fn retire(
    store: &Store,
    organization: &str,
    name: &str,
    revision: Option<i64>,
) -> Result<Integration> {
    let mut tx = store.begin().await?;
    let organization = tx.organizations().named(organization).await?;
    let read = tx.integrations().named(&organization, name).await?;
    if let Some(against) = revision.filter(|against| *against != read.revision) {
        bail!(revised(
            &read,
            &organization.name,
            "retire_integration",
            against,
            Next::inspect(Resource::Integration, read.name.clone()),
        ));
    }
    if read.retired() {
        return Ok(read);
    }
    let held = out_of_use(
        &Integration {
            state: IntegrationState::Retired,
            ..read.clone()
        },
        &organization.name,
        "fire_trigger",
    )
    .to_string();
    tx.integrations()
        .retire(&read, Timestamp::now(), LEFT_UNSAID, &held)
        .await?;
    let retired = tx.integrations().named(&organization, name).await?;
    tx.commit().await?;

    Ok(retired)
}

async fn switched(
    store: &Store,
    organization: &str,
    name: &str,
    to: IntegrationState,
) -> Result<Integration> {
    let operation = match to {
        IntegrationState::Enabled => "enable_integration",
        IntegrationState::Disabled => "disable_integration",
        IntegrationState::Retired => "retire_integration",
    };
    maintained(store, organization, name, operation, None, |read| {
        if read.state == to {
            return Ok(None);
        }
        let mut maintained = read.clone();
        maintained.state = to;
        maintained.disabled_at = maintained.disabled().then(Timestamp::now);
        if maintained.enabled() && maintained.polled() {
            maintained.poll_due_at = Some(Timestamp::now());
        }
        Ok(Some(maintained))
    })
    .await
}

async fn maintained(
    store: &Store,
    organization: &str,
    name: &str,
    operation: &'static str,
    against: Option<i64>,
    maintain: impl FnOnce(&Integration) -> Result<Option<Integration>>,
) -> Result<Integration> {
    let mut tx = store.begin().await?;
    let organization = tx.organizations().named(organization).await?;
    let read = tx.integrations().named(&organization, name).await?;
    if let Some(against) = against.filter(|against| *against != read.revision) {
        bail!(revised(
            &read,
            &organization.name,
            operation,
            against,
            Next::inspect(Resource::Integration, read.name.clone()),
        ));
    }
    if read.retired() {
        bail!(out_of_use(&read, &organization.name, operation));
    }
    let Some(maintained) = maintain(&read)? else {
        return Ok(read);
    };
    tx.integrations().maintain(&read, &maintained).await?;
    let maintained = tx
        .integrations()
        .named(&organization, &maintained.name)
        .await?;
    tx.commit().await?;

    Ok(maintained)
}

pub fn revised(
    read: &Integration,
    organization: &str,
    operation: &'static str,
    against: i64,
    next: Next,
) -> Reason {
    Reason::StateConflict {
        operation,
        resource: Resource::Integration,
        reference: read.name.clone(),
        organization: Some(organization.to_owned()),
        state: "revised",
        holding_session: None,
        next,
        message: format!(
            "the integration {} has changed since revision {against}; read it again and \
             decide on what it is now",
            read.name
        ),
    }
}

const REPLACE_PRIVATE_KEY: &str = "replace_integration_private_key";

/// GitHub is asked between the read and the write, so the write is fenced on the revision read.
pub async fn replace_private_key(
    store: &Store,
    github: &Github,
    organization: &str,
    name: &str,
    private_key: &str,
    against: Option<i64>,
) -> Result<Integration> {
    let read = integration(store, organization, name).await?;
    let again = || {
        Next::inspect(Resource::Integration, read.name.clone()).then(Step::ReplacePrivateKey {
            integration: read.name.clone(),
        })
    };
    if let Some(against) = against.filter(|against| *against != read.revision) {
        bail!(revised(
            &read,
            organization,
            REPLACE_PRIVATE_KEY,
            against,
            again()
        ));
    }
    let Connection::Github(connection) = &read.connection else {
        bail!(Reason::Forbidden {
            operation: REPLACE_PRIVATE_KEY,
            resource: Resource::Integration,
            reference: read.name.clone(),
            organization: Some(organization.to_owned()),
            constraint: "github_app",
            next: Next::inspect(Resource::Integration, read.name.clone()),
            message: format!(
                "the integration {} is a generic webhook, which presents no App private key",
                read.name
            ),
        });
    };
    // Exhaustive, so a state that gives its credentials up has to be refused here (ADR-0056).
    match read.state {
        IntegrationState::Enabled | IntegrationState::Disabled => {}
        IntegrationState::Retired => {
            bail!(out_of_use(&read, organization, REPLACE_PRIVATE_KEY));
        }
    }
    let replacement = App::held(
        connection.credential.id,
        connection.credential.installation,
        private_key,
    );
    if let Err(unproven) = github.proves(connection, &replacement).await {
        bail!(unproven_replacement(&read, organization, unproven, again()));
    }

    let mut tx = store.begin().await?;
    if !tx
        .integrations()
        .replace_private_key(&read, &replacement)
        .await?
    {
        bail!(revised(
            &read,
            organization,
            REPLACE_PRIVATE_KEY,
            read.revision,
            again()
        ));
    }
    let replaced = tx.integrations().with_id(read.id).await?;
    tx.commit().await?;

    Ok(replaced)
}

fn unproven_replacement(
    read: &Integration,
    organization: &str,
    unproven: Unproven,
    again: Next,
) -> Reason {
    const KEPT: &str = "the previous key is still in use";
    let rejected = |constraint, message: String| Reason::InvalidField {
        field: "private_key",
        operation: REPLACE_PRIVATE_KEY,
        constraint,
        allowed: None,
        message,
    };
    let unreachable = |state, message: String| Reason::StateConflict {
        operation: REPLACE_PRIVATE_KEY,
        resource: Resource::Integration,
        reference: read.name.clone(),
        organization: Some(organization.to_owned()),
        state,
        holding_session: None,
        next: again,
        message,
    };
    let unavailable = |retry_after_seconds, message: String| Reason::GithubUnavailable {
        operation: REPLACE_PRIVATE_KEY,
        integration: read.name.clone(),
        organization: organization.to_owned(),
        retry_after_seconds,
        again: Again::ReplacePrivateKey,
        message,
    };

    match unproven {
        Unproven::Unreadable => rejected(
            Constraint::RsaPrivateKey,
            format!("the replacement does not read as an RSA private key in PEM; {KEPT}"),
        ),
        Unproven::Rejected => rejected(
            Constraint::AcceptedByApp,
            format!(
                "github does not accept the replacement as a key of the App the integration {} \
                 signs as; {KEPT}",
                read.name
            ),
        ),
        Unproven::Installation => unreachable(
            "installation_unreachable",
            format!(
                "the replacement signs as the App, but the App's installation no longer answers \
                 for the integration {}; restore it on GitHub, then replace the key again; {KEPT}",
                read.name
            ),
        ),
        Unproven::Repository => unreachable(
            "repository_unreachable",
            format!(
                "the replacement signs as the App, but its installation no longer reaches the \
                 repository the integration {} watches; restore its access on GitHub, then \
                 replace the key again; {KEPT}",
                read.name
            ),
        ),
        Unproven::RateLimited { until } => unavailable(
            Some(until.duration_since(Timestamp::now()).as_secs().max(0)),
            format!(
                "github is rate limiting until {until}, so the replacement was not checked; {KEPT}"
            ),
        ),
        Unproven::Unanswered(error) => {
            warn!(
                integration = read.name,
                because = %error,
                "a replacement private key could not be checked"
            );
            unavailable(
                None,
                format!("github did not answer, so the replacement was not checked; {KEPT}"),
            )
        }
    }
}

/// Every change to an App's hook passes it, and its refusal names no other Organization's
/// records (ADR-0056).
pub async fn hook_maintainable<'a>(
    store: &Store,
    integration: &'a Integration,
    organization: &str,
    operation: &'static str,
) -> Result<&'a GithubConnection> {
    let forbidden = |constraint, message: String| Reason::Forbidden {
        operation,
        resource: Resource::Integration,
        reference: integration.name.clone(),
        organization: Some(organization.to_owned()),
        constraint,
        next: Next::inspect(Resource::Integration, integration.name.clone()),
        message,
    };
    let Connection::Github(connection) = &integration.connection else {
        bail!(forbidden(
            "github_app",
            format!(
                "the integration {} is a generic webhook, whose sender holds its secret",
                integration.name
            )
        ));
    };
    match integration.state {
        IntegrationState::Enabled | IntegrationState::Disabled => {}
        IntegrationState::Retired => bail!(out_of_use(integration, organization, operation)),
    }
    if store
        .read()
        .await?
        .integrations()
        .app_shared(integration)
        .await?
    {
        bail!(forbidden(
            "shared_app",
            format!(
                "another integration that is not retired signs as the same App as {}, perhaps in \
                 another organization; the App's webhook is theirs too, so it is not changed \
                 from here",
                integration.name
            )
        ));
    }

    Ok(connection)
}

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
                retry_after_seconds: Some(until.duration_since(Timestamp::now()).as_secs().max(0)),
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
fn same_repository(selected: &str, asked: &str) -> bool {
    if !asked.starts_with("https://") {
        return false;
    }
    match (
        github::named_repository(selected),
        github::named_repository(asked),
    ) {
        (Some(selected), Some(asked)) => selected.eq_ignore_ascii_case(&asked),
        _ => {
            let bare = |url: &str| {
                let url = url.trim_end_matches('/');
                url.strip_suffix(".git").unwrap_or(url).to_ascii_lowercase()
            };
            bare(selected) == bare(asked)
        }
    }
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
        .map_err(|unproven| unreached(&read, organization, OPERATION, unproven, Again::Read))?;

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
            Some(until.duration_since(Timestamp::now()).as_secs().max(0)),
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

fn carried(integration: &Integration, carries: &[Direction]) -> Result<()> {
    if carries.is_empty() {
        bail!(invalid(
            "carries",
            Constraint::Offered,
            "an integration carries something: name a direction it carries"
        ));
    }
    if integration.kind() == IntegrationKind::Webhook && carries.contains(&Direction::Outbound) {
        bail!(invalid(
            "carries",
            Constraint::Offered,
            "a generic webhook carries events inbound only"
        ));
    }
    Ok(())
}

fn invalid(field: &'static str, constraint: Constraint, message: &str) -> Reason {
    Reason::InvalidField {
        field,
        operation: "change_integration",
        allowed: matches!(constraint, Constraint::Offered)
            .then(|| vec!["inbound".to_owned(), "outbound".to_owned()]),
        constraint,
        message: message.to_owned(),
    }
}

/// A retired one names no repair: offering another Integration would choose its replacement.
pub fn out_of_use(
    integration: &Integration,
    organization: &str,
    operation: &'static str,
) -> Reason {
    let inspect = Next::inspect(Resource::Integration, integration.name.clone());
    let (next, message) = if integration.retired() {
        (
            inspect,
            format!(
                "the integration {} is retired: its credentials were erased, and it cannot be \
                 used, changed or enabled again",
                integration.name
            ),
        )
    } else {
        (
            inspect.then(Step::EnableIntegration {
                integration: integration.name.clone(),
            }),
            format!(
                "the integration {} is disabled; enable it to resume what waits on it",
                integration.name
            ),
        )
    };
    Reason::StateConflict {
        operation,
        resource: Resource::Integration,
        reference: integration.name.clone(),
        organization: Some(organization.to_owned()),
        state: integration.state.as_str(),
        holding_session: None,
        next,
        message,
    }
}

pub async fn integrations(store: &Store, organization: &str) -> Result<Vec<Integration>> {
    let mut tx = store.begin().await?;
    let organization = tx.organizations().named(organization).await?;

    tx.integrations().all(&organization).await
}

pub async fn acknowledge_event_refusal(
    store: &Store,
    organization: &str,
    name: &str,
) -> Result<()> {
    let mut tx = store.begin().await?;
    let organization = tx.organizations().named(organization).await?;
    let integration = tx.integrations().named(&organization, name).await?;
    tx.integrations()
        .acknowledge_event_refusal(&integration)
        .await?;
    tx.commit().await
}

pub async fn events(store: &Store, organization: &str, limit: usize) -> Result<Vec<Event>> {
    let mut tx = store.begin().await?;
    let organization = tx.organizations().named(organization).await?;

    tx.integrations().events(&organization, limit).await
}

pub async fn event(store: &Store, id: EventRecordId) -> Result<Event> {
    store.begin().await?.integrations().event(id).await
}

const RETENTION: SignedDuration = SignedDuration::from_hours(72);
/// Each poll reads back over the end of the last, by GitHub's clock, for a Delivery listed late.
const OVERLAP: SignedDuration = SignedDuration::from_mins(1);

/// A poll that cannot read every new Delivery records none of them and moves nothing (ADR-0011).
pub async fn poll(store: &Store, github: &Github, integration: &Integration) -> Result<Polled> {
    let interval = integration.github()?.interval;
    let (Some(from), Some(last_polled_at)) =
        (integration.deliveries_read_from, integration.last_polled_at)
    else {
        bail!("integration {} is not polled", integration.name);
    };
    let started = Timestamp::now();
    let read = read(store, github, integration, from).await;
    let mut tx = store.begin().await?;
    if !tx
        .integrations()
        .current(integration, Direction::Inbound)
        .await?
    {
        return Ok(Polled::default());
    }

    let read = match read {
        Ok(read) => read,
        Err(refused) => {
            warn!(
                integration = integration.name,
                because = %refused,
                "a poll came back with nothing"
            );
            tx.integrations()
                .poll_refused(integration, back_off(interval, &refused))
                .await?;
            tx.commit().await?;
            return Ok(Polled::default());
        }
    };
    let mut recorded = 0;
    for occurrence in &read.occurrences {
        match tx
            .integrations()
            .record_event(integration, occurrence)
            .await?
        {
            Recorded::Recorded => recorded += 1,
            Recorded::Already => {}
            Recorded::Refused { because } => {
                warn!(
                    integration = integration.name,
                    %because,
                    "an event was refused at ingest rather than stored"
                );
            }
        }
    }
    if read.ran_out && last_polled_at < started - RETENTION {
        tx.integrations()
            .deliveries_lost(
                integration,
                &read.source,
                &format!(
                    "no poll succeeded between {last_polled_at} and {started}, and GitHub keeps \
                     deliveries for three days, so some may have been lost"
                ),
            )
            .await?;
    }
    tx.integrations()
        .polled(
            integration,
            read.newest
                .map_or(from, |newest| from.max(newest - OVERLAP)),
            started,
            started + interval,
        )
        .await?;
    tx.commit().await?;

    Ok(Polled {
        seen: read.occurrences.len(),
        recorded,
    })
}

struct Read {
    source: String,
    occurrences: Vec<Occurrence>,
    newest: Option<Timestamp>,
    ran_out: bool,
}

/// A Delivery the webhook or an earlier poll already recorded has no payload fetched.
async fn read(
    store: &Store,
    github: &Github,
    integration: &Integration,
    from: Timestamp,
) -> Result<Read, Refused> {
    let listing = github.deliveries(integration, from).await?;
    let source = github::source(integration.github().map_err(Refused::Failed)?);
    let guids: Vec<&str> = listing
        .listed
        .iter()
        .map(|listed| listed.guid.as_str())
        .collect();
    let known = async {
        store
            .read()
            .await?
            .integrations()
            .known(integration, &source, &guids)
            .await
    }
    .await
    .map_err(Refused::Failed)?;
    let mut occurrences = Vec::new();
    for listed in &listing.listed {
        if !known.contains(&listed.guid) {
            occurrences.push(github.delivery(integration, listed).await?);
        }
    }

    Ok(Read {
        source,
        occurrences,
        newest: listing.newest,
        ran_out: listing.ran_out,
    })
}

/// A rate limit names the moment it lifts, and asking again before then spends a request on
/// another refusal; nothing is gained by polling more often than the interval either way.
fn back_off(interval: SignedDuration, refused: &Refused) -> Timestamp {
    let next = Timestamp::now() + interval;

    match refused {
        Refused::RateLimited { until } => next.max(*until),
        Refused::Failed(_) => next,
    }
}

#[cfg(test)]
mod tests {
    use anyhow::anyhow;

    use super::*;

    #[test]
    fn a_repository_is_the_same_whichever_form_git_or_gh_names_it_in() {
        let selected = "https://github.com/jtmthf/kestrel.git";

        for asked in [
            "https://github.com/jtmthf/kestrel.git",
            "https://github.com/jtmthf/kestrel",
            "https://github.com/JTMTHF/Kestrel/",
        ] {
            assert!(same_repository(selected, asked), "{asked}");
        }
        for asked in [
            "http://github.com/jtmthf/kestrel.git",
            "https://github.com/jtmthf/kestrel-fork.git",
            "https://example.com/jtmthf/kestrel.git",
            "git@github.com:jtmthf/kestrel.git",
        ] {
            assert!(!same_repository(selected, asked), "{asked}");
        }
    }

    #[test]
    fn a_rate_limit_defers_the_next_poll_to_the_moment_it_lifts() {
        let until = Timestamp::now() + SignedDuration::from_hours(1);

        assert_eq!(
            back_off(
                SignedDuration::from_secs(1),
                &Refused::RateLimited { until }
            ),
            until
        );
    }

    #[test]
    fn a_rate_limit_that_has_already_lifted_still_waits_the_interval_out() {
        let until = Timestamp::now() - SignedDuration::from_hours(1);

        assert!(
            back_off(
                SignedDuration::from_secs(60),
                &Refused::RateLimited { until }
            ) > Timestamp::now()
        );
    }

    #[test]
    fn a_failure_waits_the_interval_out_rather_than_hammering() {
        assert!(
            back_off(
                SignedDuration::from_secs(60),
                &Refused::Failed(anyhow!("connection refused"))
            ) > Timestamp::now() + SignedDuration::from_secs(50)
        );
    }
}
