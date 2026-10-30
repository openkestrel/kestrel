use anyhow::{Result, bail};
use jiff::Timestamp;

use crate::declined::{Again, Constraint, Next, Reason, Resource, Step};
use crate::domain::Integration;
use crate::integration::github::{DeliveryResult, Github, Hook, Unset};
use crate::integration::{hook_maintainable, integration, manifest, revised, rotation};
use crate::store::Store;

pub const SET_URL: &str = "set_integration_webhook_url";
pub const READ: &str = "read_integration_webhook";

pub fn public_base(value: &str) -> Result<String> {
    manifest::base(value, false, SET_URL)
}

pub async fn set_url(
    store: &Store,
    github: &Github,
    organization: &str,
    name: &str,
    base: Option<&str>,
    against: Option<i64>,
) -> Result<Integration> {
    let base = public_base(base.ok_or_else(|| Reason::InvalidField {
        field: "base",
        operation: SET_URL,
        constraint: Constraint::NonEmpty,
        allowed: None,
        message: "Supply a public HTTPS base or configure KESTREL_WEBHOOK_BASE on the control plane; polling continues without one.".into(),
    })?)?;
    let _attempt = store
        .hook_updates
        .try_lock()
        .map_err(|_| Reason::StateConflict {
            operation: SET_URL,
            resource: Resource::Integration,
            reference: name.into(),
            organization: Some(organization.into()),
            state: "hook_url_update_in_flight",
            holding_session: None,
            next: Next::inspect(Resource::Integration, name),
            message: "A hook URL request is still in flight; inspect the hook before retrying."
                .into(),
        })?;
    let mut tx = store.begin().await?;
    let org = tx.organizations().named(organization).await?;
    let read = tx.integrations().named(&org, name).await?;
    if let Some(against) = against.filter(|against| *against != read.revision) {
        bail!(revised(
            &read,
            organization,
            SET_URL,
            against,
            Next::inspect(Resource::Integration, read.name.clone())
        ));
    }
    hook_maintainable(store, &read, organization, SET_URL).await?;
    if let Some(rotation) = &read.rotation {
        bail!(rotation::unrotated(
            &read,
            organization,
            SET_URL,
            rotation,
            store.clock().now(),
        ));
    }
    if !tx.integrations().signs_webhooks(&read).await? {
        bail!(Reason::Forbidden {
            operation: SET_URL,
            resource: Resource::Integration,
            reference: read.name.clone(),
            organization: Some(organization.into()),
            constraint: "signing_secret",
            next: Next::inspect(Resource::Integration, read.name.clone()),
            message: "This Integration holds no webhook signing secret; polling continues.".into(),
        });
    }
    let url = format!("{base}{}", read.webhook_path());
    match tx.integrations().pending_hook_url(&read).await? {
        Some(pending) if pending != url => bail!(unfinished(&read, organization, SET_URL)),
        Some(_) => {}
        None => {
            tx.integrations().start_hook_url(&read, &url).await?;
            tx.integrations().maintain(&read, &read).await?;
        }
    }
    let reserved = tx.integrations().with_id(read.id).await?;
    tx.commit().await?;

    let current = integration(store, organization, name).await?;
    if current.revision != reserved.revision {
        bail!(revised(
            &current,
            organization,
            SET_URL,
            reserved.revision,
            Next::inspect(Resource::Integration, current.name.clone())
        ));
    }
    let connection = hook_maintainable(store, &current, organization, SET_URL).await?;
    github
        .set_hook_url(&connection.api, &connection.credential, &url)
        .await
        .map_err(|error| {
            unavailable(&current, organization, SET_URL, error, store.clock().now())
        })?;

    let mut tx = store.begin().await?;
    let changed = tx.integrations().with_id(read.id).await?;
    tx.integrations().finish_hook_url(&changed).await?;
    tx.commit().await?;
    if changed.retired() {
        bail!(super::out_of_use(&changed, organization, SET_URL));
    }
    if changed.revision != reserved.revision {
        bail!(revised(
            &changed,
            organization,
            SET_URL,
            reserved.revision,
            Next::inspect(Resource::Integration, changed.name.clone())
        ));
    }
    Ok(changed)
}

pub async fn status(
    store: &Store,
    github: &Github,
    organization: &str,
    name: &str,
) -> Result<(Hook, Vec<DeliveryResult>, Option<String>)> {
    let read = integration(store, organization, name).await?;
    let connection = read.github().map_err(|_| Reason::Forbidden {
        operation: READ,
        resource: Resource::Integration,
        reference: read.name.clone(),
        organization: Some(organization.into()),
        constraint: "github_app",
        next: Next::inspect(Resource::Integration, read.name.clone()),
        message: "A generic webhook has no GitHub App Delivery log.".into(),
    })?;
    if read.retired() {
        bail!(super::out_of_use(&read, organization, READ));
    }
    let hook = github
        .hook(connection)
        .await
        .map_err(|error| unavailable(&read, organization, READ, error, store.clock().now()))?;
    let deliveries = github
        .recent_deliveries(connection)
        .await
        .map_err(|error| unavailable(&read, organization, READ, error, store.clock().now()))?;
    let current = integration(store, organization, name).await?;
    if current.revision != read.revision {
        bail!(revised(
            &current,
            organization,
            READ,
            read.revision,
            Next::inspect(Resource::Integration, current.name.clone())
        ));
    }
    let pending_url = store
        .read()
        .await?
        .integrations()
        .pending_hook_url(&current)
        .await?;
    Ok((
        hook,
        deliveries
            .into_iter()
            .filter(|delivery| {
                delivery.installation_id == Some(connection.credential.installation)
                    && delivery.repository_id == Some(connection.repository_id)
            })
            .collect(),
        pending_url,
    ))
}

fn unavailable(
    read: &Integration,
    organization: &str,
    operation: &'static str,
    error: Unset,
    now: Timestamp,
) -> Reason {
    let unavailable = |retry_after_seconds, message: &str| Reason::GithubUnavailable {
        operation,
        integration: read.name.clone(),
        organization: organization.into(),
        retry_after_seconds,
        again: Again::Read,
        message: message.into(),
    };
    let mut next = Next::inspect(Resource::Integration, read.name.clone());
    let (state, message) = match error {
        Unset::RateLimited { until } => {
            return unavailable(
                Some(until.duration_since(now).as_secs().max(1)),
                "GitHub is rate limiting hook requests; inspect the hook after the wait. Polling continues.",
            );
        }
        Unset::Rejected => {
            next = next.then(Step::ReplacePrivateKey {
                integration: read.name.clone(),
            });
            (
                "credentials_rejected",
                "GitHub does not accept the App private key; replace it and inspect the hook. Polling remains configured.",
            )
        }
        Unset::Refused(_) => (
            "hook_refused",
            "GitHub refused the hook request; inspect the App's webhook configuration and access. Polling continues.",
        ),
        Unset::Unanswered(_) if operation == READ => {
            return unavailable(
                None,
                "GitHub did not answer the hook read; retry the read. Polling continues.",
            );
        }
        Unset::Unanswered(_) => (
            "hook_unconfirmed",
            "GitHub did not confirm the hook request. A URL change may have landed; inspect the App hook before retrying. Polling continues.",
        ),
    };
    Reason::StateConflict {
        operation,
        resource: Resource::Integration,
        reference: read.name.clone(),
        organization: Some(organization.into()),
        state,
        holding_session: None,
        next,
        message: message.into(),
    }
}

pub fn unfinished(read: &Integration, organization: &str, operation: &'static str) -> Reason {
    Reason::StateConflict {
        operation,
        resource: Resource::Integration,
        reference: read.name.clone(),
        organization: Some(organization.into()),
        state: "hook_url_update_pending",
        holding_session: None,
        next: Next::inspect(Resource::Integration, read.name.clone()),
        message: "An App hook URL update is unfinished; read webhook status and retry set-url with the same base to finish it before changing the URL or secret.".into(),
    }
}
