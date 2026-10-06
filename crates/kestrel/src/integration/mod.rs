pub mod credential;
pub mod github;
pub mod manifest;
pub mod post;
pub mod webhook;

use anyhow::{Context as _, Result, bail};
use jiff::{SignedDuration, Timestamp};
use tracing::warn;

use crate::declined::Declined;
use crate::domain::{
    Connection, Direction, Event, EventRecordId, GithubConnection, Integration, Occurrence,
};
use crate::integration::credential::App;
use crate::integration::github::{Github, Refused};
use crate::store::Store;
use crate::store::integration::Recorded;

pub struct Registration<'a> {
    pub organization: &'a str,
    pub name: &'a str,
    pub carries: &'a [Direction],
    pub connecting: Connecting<'a>,
}

pub enum Connecting<'a> {
    /// Polled whether or not it has a `signing_secret`; with one, a Delivery GitHub can
    /// reach kestrel with is also recorded on arrival.
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
            (
                Connection::Github(GithubConnection {
                    repository,
                    api: api.to_owned(),
                    credential,
                    bot_login,
                    interval,
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

/// GitHub keeps a Delivery for three days: one older than that which no poll read is gone.
const RETENTION: SignedDuration = SignedDuration::from_hours(72);
/// A Delivery is listed a few seconds after it is made, so each poll reads back over the end
/// of the last one; what it reads again it recognises rather than records twice.
const LISTING_LAG: SignedDuration = SignedDuration::from_mins(1);

/// One poll of one Integration: its App's Deliveries since it last read them. Every Event the poll
/// read and where the next one starts are written in one transaction, and a poll that cannot read
/// every new Delivery records none of them and starts from the same place next time (ADR-0011).
pub async fn poll(store: &Store, github: &Github, integration: &Integration) -> Result<Polled> {
    let interval = integration.github()?.interval;
    let from = integration
        .deliveries_read_from
        .context("a polled integration knows where its reading starts")?;
    let started = Timestamp::now();
    let read = read(store, github, integration, from).await;
    let mut tx = store.begin().await?;

    let (occurrences, ran_out) = match read {
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
    for occurrence in &occurrences {
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
    if ran_out && from < started - RETENTION {
        let github = integration.github()?;
        tx.integrations()
            .deliveries_lost(
                integration,
                &github::source(github),
                &format!(
                    "GitHub keeps deliveries for three days, and those made after {from} that no \
                     poll read are gone"
                ),
            )
            .await?;
    }
    tx.integrations()
        .polled(
            integration,
            from.max(started - LISTING_LAG),
            started + interval,
        )
        .await?;
    tx.commit().await?;

    Ok(Polled {
        seen: occurrences.len(),
        recorded,
    })
}

/// Only a Delivery not yet recorded has its payload fetched: one that reached the webhook, or an
/// earlier poll, already has its Event.
async fn read(
    store: &Store,
    github: &Github,
    integration: &Integration,
    from: Timestamp,
) -> Result<(Vec<Occurrence>, bool), Refused> {
    let listing = github.deliveries(integration, from).await?;
    let source = github::source(integration.github().map_err(Refused::Failed)?);
    let mut occurrences = Vec::new();

    for listed in &listing.listed {
        let known = async {
            store
                .read()
                .await?
                .integrations()
                .knows(integration, &source, &listed.guid)
                .await
        }
        .await
        .map_err(Refused::Failed)?;
        if known {
            continue;
        }
        if let Some(occurrence) = github.delivery(integration, listed).await? {
            occurrences.push(occurrence);
        }
    }

    Ok((occurrences, listing.ran_out))
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
