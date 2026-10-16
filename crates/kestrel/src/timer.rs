//! The in-process wheel (ADR-0005). The schedule is never in process memory alone, so a
//! control plane that restarts finds every due time set before it existed and fires it.

use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use jiff::Timestamp;
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

use crate::domain::{Exit, SessionId};
use crate::follow_up;
use crate::integration::github::Github;
use crate::integration::post;
use crate::integration::{self, Polled};
use crate::pull_request;
use crate::store::Store;
use crate::trigger;
use crate::work;
use crate::workspace;

const SWEEP: Duration = Duration::from_millis(500);
const RETENTION_SWEEP: Duration = Duration::from_secs(60 * 60);

/// Starts the sweeps that consume Events now rather than at their next tick. It reaches only a
/// work role in the same process; across processes the sweep interval is the guarantee.
#[derive(Clone)]
pub struct Wake(Arc<watch::Sender<()>>);

impl Default for Wake {
    fn default() -> Self {
        Self(Arc::new(watch::Sender::new(())))
    }
}

impl Wake {
    pub fn wake(&self) {
        self.0.send_replace(());
    }
}

/// A pass runs one round of each sweep, and the live wheel repeats each round on its own cadence.
pub struct Sweeps {
    github: Github,
}

impl Sweeps {
    pub fn dialling_out(store: &Store) -> Result<Self> {
        Ok(Self {
            github: Github::dialling_out(store)?,
        })
    }

    /// In the order an Event moves through them, so what one round records the next consumes.
    pub async fn pass(&self, store: &Store, wake: &Wake, summaries: &crate::live_work::Summaries) {
        sweep_leases(store, summaries).await;
        expire_transcripts(store).await;
        polling(store, &self.github).await;
        elapsing(store, wake).await;
        firing(store, &self.github).await;
        following_up(store).await;
        learning_pull_requests(store, &self.github).await;
        sealing_idle_workspaces(store).await;
        posting(store, &self.github).await;
    }

    pub async fn run(
        &self,
        store: &Store,
        wake: &Wake,
        summaries: &crate::live_work::Summaries,
        shutdown: &CancellationToken,
    ) {
        let github = &self.github;
        let woken = || Some(wake.0.subscribe());

        // Beside the lease sweep rather than in it: a poll waits on GitHub, and a lease left
        // unswept for the length of an HTTP request is a Workspace wedged for that long.
        tokio::join!(
            repeating(shutdown, SWEEP, None, || sweep_leases(store, summaries)),
            repeating(shutdown, RETENTION_SWEEP, None, || expire_transcripts(
                store
            )),
            repeating(shutdown, SWEEP, None, || polling(store, github)),
            repeating(shutdown, SWEEP, None, || elapsing(store, wake)),
            repeating(shutdown, SWEEP, woken(), || firing(store, github)),
            repeating(shutdown, SWEEP, woken(), || following_up(store)),
            repeating(shutdown, SWEEP, woken(), || learning_pull_requests(
                store, github
            )),
            repeating(shutdown, SWEEP, None, || sealing_idle_workspaces(store)),
            repeating(shutdown, SWEEP, None, || posting(store, github)),
        );
    }
}

/// A round that fails is warned of inside it, so nothing here stops keeping time: the same due
/// times are still there to be found on the next round.
async fn repeating<Round: Future<Output = ()>>(
    shutdown: &CancellationToken,
    period: Duration,
    mut woken: Option<watch::Receiver<()>>,
    round: impl Fn() -> Round,
) {
    while !shutdown.is_cancelled() {
        round().await;
        match &mut woken {
            Some(woken) => tick_or_woken(shutdown, period, woken).await,
            None => tick(shutdown, period).await,
        }
    }
}

async fn following_up(store: &Store) {
    match follow_up::receive(store).await {
        Ok(received) => {
            for follow_up in received {
                info!(
                    event = %follow_up.event,
                    workspace = %follow_up.workspace,
                    session = ?follow_up.session,
                    "a follow-up was received"
                );
            }
        }
        Err(error) => warn!(%error, "a follow-up sweep found nothing it could do"),
    }
}

async fn learning_pull_requests(store: &Store, github: &Github) {
    match pull_request::learn(store, github).await {
        Ok(learned) => {
            for learned in learned {
                info!(
                    event = %learned.event,
                    workspace = ?learned.workspace,
                    "a pull request event was considered"
                );
            }
        }
        Err(error) => warn!(%error, "a pull request sweep found nothing it could do"),
    }
}

async fn elapsing(store: &Store, wake: &Wake) {
    match trigger::elapse(store, Timestamp::now()).await {
        Ok(minted) => {
            for occurrence in &minted {
                info!(source = occurrence.source, due = %occurrence.time, "a schedule elapsed");
            }
            if !minted.is_empty() {
                wake.wake();
            }
        }
        Err(error) => warn!(%error, "a schedule sweep found nothing it could do"),
    }
}

async fn sealing_idle_workspaces(store: &Store) {
    match workspace::seal_idle(store).await {
        Ok(sealed) => {
            for workspace in sealed {
                info!(workspace = %workspace.id, "an idle workspace sealed itself");
            }
        }
        Err(error) => warn!(%error, "an idle sweep found nothing it could do"),
    }
}

async fn sweep_leases(store: &Store, summaries: &crate::live_work::Summaries) {
    if let Err(error) = integration::manifest::expire(store).await {
        warn!(%error, "GitHub App setup expiry could not be swept");
    }
    match sweep(store, summaries).await {
        Ok(expired) => {
            for (session, exit) in expired {
                info!(%session, %exit, "a lease expired");
            }
        }
        Err(error) => warn!(%error, "a sweep found nothing it could do"),
    }
}

async fn polling(store: &Store, github: &Github) {
    if let Err(error) = poll(store, github).await {
        warn!(%error, "a poll found nothing it could do");
    }
}

/// A sweep of its own rather than the tail of the transaction that ends a Session: what a Session
/// ended as is durable the moment it ends, and saying so out loud is a request to somebody
/// else's system that may be refused, deferred and asked again without any of that reaching
/// the Session.
async fn posting(store: &Store, github: &Github) {
    if let Err(error) = post_due(store, github).await {
        warn!(%error, "a post found nothing it could do");
    }
}

/// Matching is its own sweep rather than the tail of a poll, so a control plane that stopped
/// between recording an Event and firing for it finds it on the way back up.
async fn firing(store: &Store, github: &Github) {
    match trigger::fire(store, github).await {
        Ok(fired) => {
            for firing in fired {
                match firing {
                    trigger::Fired::Opened {
                        event,
                        workspace,
                        session,
                    } => info!(%event, %workspace, %session, "a trigger fired"),
                    trigger::Fired::Fed {
                        event,
                        workspace,
                        session,
                    } => info!(%event, %workspace, ?session, "a trigger fed a workspace"),
                    trigger::Fired::Ignored {
                        event,
                        trigger,
                        correlation,
                    } => {
                        info!(%event, %trigger, %correlation, "a trigger ignored a correlation miss")
                    }
                    trigger::Fired::Failed {
                        event,
                        trigger,
                        because,
                    } => {
                        warn!(%event, %trigger, %because, "a trigger fired and opened nothing")
                    }
                    trigger::Fired::Held {
                        event,
                        trigger,
                        because,
                    } => {
                        info!(%event, %trigger, %because, "a trigger held work")
                    }
                    trigger::Fired::Canceled {
                        event,
                        trigger,
                        because,
                    } => {
                        info!(%event, %trigger, %because, "a trigger canceled held work")
                    }
                }
            }
        }
        Err(error) => warn!(%error, "a firing found nothing it could do"),
    }
}

async fn tick(shutdown: &CancellationToken, period: Duration) {
    tokio::select! {
        () = tokio::time::sleep(period) => {}
        () = shutdown.cancelled() => {}
    }
}

/// A wake that arrives mid-sweep is kept, so the Event it announced is not left to the tick.
async fn tick_or_woken(
    shutdown: &CancellationToken,
    period: Duration,
    woken: &mut watch::Receiver<()>,
) {
    tokio::select! {
        () = tokio::time::sleep(period) => {}
        _ = woken.changed() => {}
        () = shutdown.cancelled() => {}
    }
}

/// Every Session found is ended in the transaction that found it, so a heartbeat racing the sweep
/// either got there first — and its Session is not in this read — or waits for the write lock and
/// finds a Session that has ended. A lease that expires fails its Session and never re-dispatches
/// it: kestrel retries dispatch, never work.
async fn sweep(
    store: &Store,
    summaries: &crate::live_work::Summaries,
) -> Result<Vec<(SessionId, Exit)>> {
    let mut tx = store.begin().await?;
    let mut expired = Vec::new();
    let mut closed = Vec::new();

    let now = Timestamp::now();
    if let Some(gap) = tx.lease_sweep().record_pass(now).await? {
        warn!(start = %gap.start, end = %gap.end, "the control plane was not running");
        tx.workspaces().grace_leases(now + work::GRACE).await?;
    }
    let gap = tx.lease_sweep().gap().await?;
    for session in tx.workspaces().expired_leases(now).await? {
        let mut exit = work::expired_lease();
        if let Some(gap) = gap
            && session.lease_expires_at.is_some_and(|at| at >= gap.end)
        {
            let reached = if let Some(instance) = session.instance.as_deref() {
                tx.workspaces()
                    .supervisor(instance)
                    .await?
                    .and_then(|supervisor| supervisor.reached_at)
            } else {
                None
            };
            let because = if reached.is_some_and(|at| at > gap.end) {
                format!(
                    "the supervisor came back after the control plane was not running from {} to {}, no longer carrying the session",
                    gap.start, gap.end
                )
            } else {
                format!(
                    "the control plane was not running from {} to {}, and the supervisor did not reach it within 30 s after",
                    gap.start, gap.end
                )
            };
            exit = Exit::Failed { because };
        }
        work::close_lost_units(&mut tx, &session, summaries).await?;
        closed.push(session.clone());
        expired.push((session.id, work::ending(&mut tx, &session, exit).await?));
    }
    tx.commit().await?;

    for session in closed {
        summaries.clear_session(&session);
    }
    Ok(expired)
}

async fn post_due(store: &Store, github: &Github) -> Result<()> {
    let due = {
        let mut tx = store.begin().await?;
        tx.integrations().posts_due(Timestamp::now()).await?
    };

    for due in due {
        if let Some(comment) = post::post(store, github, &due).await? {
            info!(
                session = %due.session,
                turn = due.turn,
                comment,
                "what a session said reached the issue it came from"
            );
        }
    }

    Ok(())
}

/// One at a time, so the write lock is held for a poll's transaction rather than for its wait
/// on the network, and so an Integration whose poll runs long is not asked again underneath
/// the one in flight.
async fn poll(store: &Store, github: &Github) -> Result<()> {
    github.forget_unused().await?;
    let due = {
        let mut tx = store.begin().await?;
        tx.integrations().due(Timestamp::now()).await?
    };

    for integration in due {
        let Polled { seen, recorded } = integration::poll(store, github, &integration).await?;
        if recorded > 0 {
            info!(
                integration = integration.name,
                seen, recorded, "a poll recorded events"
            );
        }
    }

    Ok(())
}

pub async fn expire_transcript(store: &Store) -> Result<usize> {
    let mut tx = store.begin().await?;
    let expired = tx.log().expire(Timestamp::now()).await?;
    tx.commit().await?;
    Ok(expired)
}

async fn expire_transcripts(store: &Store) {
    match expire_transcript(store).await {
        Ok(expired) if expired > 0 => info!(expired, "Transcript entries expired"),
        Ok(_) => {}
        Err(error) => warn!(%error, "a retention sweep found nothing it could do"),
    }
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use super::*;

    #[tokio::test]
    async fn a_wake_ends_the_wait_before_the_tick() {
        let wake = Wake::default();
        let mut woken = wake.0.subscribe();
        let started = Instant::now();

        wake.wake();
        tick_or_woken(&CancellationToken::new(), SWEEP, &mut woken).await;

        assert!(started.elapsed() < SWEEP / 2);
    }

    #[tokio::test]
    async fn without_a_wake_the_tick_is_the_floor() {
        let wake = Wake::default();
        let mut woken = wake.0.subscribe();
        let started = Instant::now();

        tick_or_woken(&CancellationToken::new(), SWEEP, &mut woken).await;

        assert!(started.elapsed() >= SWEEP);
    }

    #[tokio::test]
    async fn one_wake_reaches_every_sweep_waiting_on_it() {
        let wake = Wake::default();
        let mut firing = wake.0.subscribe();
        let mut following_up = wake.0.subscribe();
        let shutdown = CancellationToken::new();
        let started = Instant::now();

        wake.wake();
        tick_or_woken(&shutdown, SWEEP, &mut firing).await;
        tick_or_woken(&shutdown, SWEEP, &mut following_up).await;

        assert!(started.elapsed() < SWEEP / 2);
    }
}
