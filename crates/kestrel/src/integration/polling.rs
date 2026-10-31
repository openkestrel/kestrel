use anyhow::{Result, bail};
use jiff::{SignedDuration, Timestamp};
use tracing::warn;

use crate::domain::{Direction, Integration, Occurrence};
use crate::store::Store;
use crate::store::integration::Recorded;

use super::back_off;
use super::github::{self, Github, Refused};

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Polled {
    pub seen: usize,
    pub recorded: usize,
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
    let started = store.clock().now();
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
                .poll_refused(
                    integration,
                    back_off(interval, &refused, store.clock().now()),
                )
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
