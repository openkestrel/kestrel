use jiff::{SignedDuration, Timestamp};

use super::github::Refused;

/// A rate limit names the moment it lifts, and asking again before then spends a request on
/// another refusal; nothing is gained by polling more often than the interval either way.
pub(super) fn back_off(interval: SignedDuration, refused: &Refused, now: Timestamp) -> Timestamp {
    let next = now + interval;

    match refused {
        Refused::RateLimited { until } => next.max(*until),
        Refused::Failed(_) => next,
    }
}
