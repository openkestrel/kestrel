//! A GitHub App's webhook secret changes in two systems, so its rotation is a durable operation
//! rather than one write: the replacement is saved first, verifies beside the secret it replaces,
//! and becomes the only one once GitHub acknowledges setting it (ADR-0056).

use anyhow::{Result, bail};
use jiff::Timestamp;
use tracing::warn;

use crate::declined::{Again, Next, Reason, Resource, Step};
use crate::domain::{Integration, Rotation, RotationFailure};
use crate::integration::github::{Github, Unset};
use crate::integration::{hook_maintainable, integration, out_of_use, revised};
use crate::link::credential::Secret;
use crate::store::Store;

pub const ROTATE: &str = "rotate_integration_webhook_secret";
pub const RESUME: &str = "resume_integration_webhook_secret_rotation";

pub async fn rotate_webhook_secret(
    store: &Store,
    github: &Github,
    organization: &str,
    name: &str,
    against: Option<i64>,
) -> Result<Integration> {
    let read = integration(store, organization, name).await?;
    if let Some(against) = against.filter(|against| *against != read.revision) {
        bail!(revised(
            &read,
            organization,
            ROTATE,
            against,
            Next::inspect(Resource::Integration, read.name.clone()),
        ));
    }
    hook_maintainable(store, &read, organization, ROTATE).await?;
    if read.rotation.is_some() {
        bail!(under_way(&read, organization, ROTATE));
    }
    if !store
        .read()
        .await?
        .integrations()
        .signs_webhooks(&read)
        .await?
    {
        bail!(Reason::Forbidden {
            operation: ROTATE,
            resource: Resource::Integration,
            reference: read.name.clone(),
            organization: Some(organization.to_owned()),
            constraint: "signing_secret",
            next: Next::inspect(Resource::Integration, read.name.clone()),
            message: format!(
                "the integration {} holds no webhook secret, so there is none to rotate; the \
                 App's webhook may sign for someone else",
                read.name
            ),
        });
    }

    let mut tx = store.begin().await?;
    let started = tx
        .integrations()
        .start_rotation(&read, Secret::mint().as_str(), Timestamp::now())
        .await?;
    let current = tx.integrations().with_id(read.id).await?;
    tx.commit().await?;
    if !started {
        bail!(if current.rotation.is_some() {
            under_way(&current, organization, ROTATE)
        } else {
            revised(
                &read,
                organization,
                ROTATE,
                read.revision,
                Next::inspect(Resource::Integration, read.name.clone()),
            )
        });
    }

    attempt(store, github, organization, &current, ROTATE).await
}

/// Sets the same saved replacement again: a masked read-back cannot show whether GitHub took it.
pub async fn resume_webhook_secret_rotation(
    store: &Store,
    github: &Github,
    organization: &str,
    name: &str,
) -> Result<Integration> {
    let read = integration(store, organization, name).await?;
    hook_maintainable(store, &read, organization, RESUME).await?;
    if read.rotation.is_none() {
        return Ok(read);
    }

    attempt(store, github, organization, &read, RESUME).await
}

async fn attempt(
    store: &Store,
    github: &Github,
    organization: &str,
    read: &Integration,
    operation: &'static str,
) -> Result<Integration> {
    let connection = read.github()?;
    let mut tx = store.begin().await?;
    let Some(replacement) = tx.integrations().replacement(read).await? else {
        drop(tx);
        return ended(store, organization, read, operation).await;
    };
    tx.integrations()
        .attempting_rotation(read, &replacement, Timestamp::now())
        .await?;
    tx.commit().await?;

    let unset = github
        .set_hook_secret(&connection.api, &connection.credential, &replacement.secret)
        .await;
    let mut tx = store.begin().await?;
    match unset {
        Ok(()) => {
            tx.integrations().rotated(read, &replacement).await?;
            tx.commit().await?;
            ended(store, organization, read, operation).await
        }
        Err(unset) => {
            let failure = failed(read, unset);
            let recorded = tx
                .integrations()
                .rotation_failed(read, &replacement, failure)
                .await?;
            tx.commit().await?;
            if !recorded {
                return ended(store, organization, read, operation).await;
            }
            bail!(unrotated(
                read,
                organization,
                operation,
                &Rotation {
                    started_at: read
                        .rotation
                        .as_ref()
                        .map_or_else(Timestamp::now, |rotation| rotation.started_at),
                    attempted_at: Some(Timestamp::now()),
                    failure: Some(failure),
                },
            ))
        }
    }
}

/// Finished here or elsewhere; retirement ends one by erasing what it would have set.
async fn ended(
    store: &Store,
    organization: &str,
    read: &Integration,
    operation: &'static str,
) -> Result<Integration> {
    let current = store.read().await?.integrations().with_id(read.id).await?;
    if current.retired() {
        bail!(out_of_use(&current, organization, operation));
    }
    Ok(current)
}

fn failed(read: &Integration, unset: Unset) -> RotationFailure {
    match unset {
        Unset::Rejected => RotationFailure::CredentialsRejected,
        Unset::Refused(status) => {
            warn!(
                integration = read.name,
                %status,
                "github refused to change the App's webhook secret"
            );
            RotationFailure::Refused
        }
        Unset::RateLimited { until } => RotationFailure::RateLimited { until },
        Unset::Unanswered(error) => {
            warn!(
                integration = read.name,
                because = %error,
                "github did not acknowledge a new App webhook secret"
            );
            RotationFailure::Unanswered
        }
    }
}

fn resume(integration: &Integration) -> Step {
    Step::ResumeWebhookSecretRotation {
        integration: integration.name.clone(),
    }
}

fn under_way(read: &Integration, organization: &str, operation: &'static str) -> Reason {
    Reason::StateConflict {
        operation,
        resource: Resource::Integration,
        reference: read.name.clone(),
        organization: Some(organization.to_owned()),
        state: "rotating_webhook_secret",
        holding_session: None,
        next: Next::inspect(Resource::Integration, read.name.clone()).then(resume(read)),
        message: format!(
            "the webhook secret of integration {} is already being rotated; resume that rotation \
             rather than starting another",
            read.name
        ),
    }
}

/// What an unfinished rotation says, whether it is read or has just been refused.
pub fn unrotated(
    integration: &Integration,
    organization: &str,
    operation: &'static str,
    rotation: &Rotation,
) -> Reason {
    const BOTH: &str = "the previous and the replacement secret both verify until it finishes";
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
        again: Again::ResumeWebhookSecretRotation,
        message,
    };
    let name = &integration.name;

    match (rotation.failure, rotation.attempted_at) {
        (None, None) => conflict(
            "rotating_webhook_secret",
            inspect().then(resume(integration)),
            format!(
                "a new webhook secret for integration {name} was saved at {} but github has not \
                 been asked to sign with it; resume the rotation; {BOTH}",
                rotation.started_at
            ),
        ),
        (None, Some(attempted_at)) => conflict(
            "rotating_webhook_secret",
            inspect().then(resume(integration)),
            format!(
                "github was asked at {attempted_at} to sign integration {name}'s webhooks with a \
                 new secret and never said whether it did; resume the rotation to set the same \
                 secret again; {BOTH}"
            ),
        ),
        (Some(RotationFailure::CredentialsRejected), _) => conflict(
            "credentials_rejected",
            inspect()
                .then(Step::ReplacePrivateKey {
                    integration: name.clone(),
                })
                .then(resume(integration)),
            format!(
                "github does not accept the private key integration {name} signs as its App \
                 with, so the webhook secret was not changed; replace the key, then resume the \
                 rotation; {BOTH}"
            ),
        ),
        (Some(RotationFailure::Refused), _) => conflict(
            "hook_refused",
            inspect().then(resume(integration)),
            format!(
                "github refused to change the webhook secret of the App integration {name} signs \
                 as; check the App's webhook on GitHub, then resume the rotation; {BOTH}"
            ),
        ),
        (Some(RotationFailure::RateLimited { until }), _) => unavailable(
            Some(until.duration_since(Timestamp::now()).as_secs().max(0)),
            format!(
                "github is rate limiting until {until}, so integration {name}'s webhook secret \
                 was not changed; resume the rotation then; {BOTH}"
            ),
        ),
        (Some(RotationFailure::Unanswered), _) => unavailable(
            None,
            format!(
                "github did not acknowledge integration {name}'s new webhook secret; resume the \
                 rotation to set the same secret again; {BOTH}"
            ),
        ),
    }
}
