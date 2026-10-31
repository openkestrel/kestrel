use anyhow::{Result, bail};
use jiff::{SignedDuration, Timestamp};
use tracing::warn;

use crate::declined::{Again, Constraint, Next, Reason, Resource, Step};
use crate::domain::{
    Connection, Direction, GithubConnection, Integration, IntegrationKind, IntegrationState,
};
use crate::store::Store;

use super::credential::App;
use super::github::{Github, Unproven};
use super::{hook, integration, out_of_use, revised};

/// Identity — kind, API origin, repository, App and installation — has no field here because it
/// cannot change in place (ADR-0056).
#[derive(Debug, Default, Clone, Copy)]
pub struct Change<'a> {
    pub name: Option<&'a str>,
    pub carries: Option<&'a [Direction]>,
    pub interval: Option<SignedDuration>,
    pub revision: Option<i64>,
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
            let now = store.clock().now();
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
        .retire(&read, store.clock().now(), LEFT_UNSAID, &held)
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
        maintained.disabled_at = maintained.disabled().then(|| store.clock().now());
        if maintained.enabled() && maintained.polled() {
            maintained.poll_due_at = Some(store.clock().now());
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
        bail!(unproven_replacement(
            &read,
            organization,
            unproven,
            again(),
            store.clock().now(),
        ));
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
    now: Timestamp,
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
            Some(until.duration_since(now).as_secs().max(0)),
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

    if operation != hook::SET_URL
        && store
            .read()
            .await?
            .integrations()
            .pending_hook_url(integration)
            .await?
            .is_some()
    {
        bail!(hook::unfinished(integration, organization, operation));
    }

    Ok(connection)
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
