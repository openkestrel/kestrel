//! A notice says only what changed, never what it changed to (ADR-0035).

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::broadcast;
use tokio::time::Instant;

use crate::domain::{OrganizationId, Session, SessionId, Workspace, WorkspaceId};

const COALESCE: Duration = Duration::from_millis(250);
/// How many notices one subscriber may lag before it is told to resync instead.
const BUFFER: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Resource {
    Workspace(WorkspaceId),
    Session(SessionId),
    Queue,
}

/// Collected as a transaction goes, so a dropped or rolled-back one hands the hub nothing.
#[derive(Default)]
pub struct Touched {
    resources: Vec<(OrganizationId, Resource)>,
    queues_everywhere: bool,
}

impl Touched {
    pub fn workspace(&mut self, workspace: &Workspace) {
        self.workspace_id(workspace.organization.id, workspace.id);
    }

    pub fn workspace_id(&mut self, organization: OrganizationId, workspace: WorkspaceId) {
        self.resources
            .push((organization, Resource::Workspace(workspace)));
    }

    pub fn session(&mut self, session: &Session) {
        self.session_id(session.organization, session.id);
    }

    pub fn session_id(&mut self, organization: OrganizationId, session: SessionId) {
        self.resources
            .push((organization, Resource::Session(session)));
    }

    pub fn queue(&mut self, organization: OrganizationId) {
        self.resources.push((organization, Resource::Queue));
        self.queues_everywhere = true;
    }

    pub fn every_queue(&mut self) {
        self.queues_everywhere = true;
    }

    pub fn is_empty(&self) -> bool {
        self.resources.is_empty() && !self.queues_everywhere
    }
}

pub enum Watch {
    Change(Resource),
    Resync,
}

pub struct Subscription {
    receiver: broadcast::Receiver<Resource>,
}

impl Subscription {
    pub async fn recv(&mut self) -> Option<Watch> {
        match self.receiver.recv().await {
            Ok(resource) => Some(Watch::Change(resource)),
            Err(broadcast::error::RecvError::Lagged(_)) => Some(Watch::Resync),
            Err(broadcast::error::RecvError::Closed) => None,
        }
    }
}

/// One process runs one topology, so a subscriber reaches the same hub its writes do.
#[derive(Clone, Default)]
pub struct Notices {
    hub: Arc<Hub>,
}

#[derive(Default)]
struct Hub {
    state: Mutex<State>,
    flushing: AtomicBool,
}

#[derive(Default)]
struct State {
    senders: HashMap<OrganizationId, broadcast::Sender<Resource>>,
    pending: HashMap<(OrganizationId, Resource), Instant>,
}

impl Notices {
    pub fn subscribe(&self, organization: OrganizationId) -> Subscription {
        let mut state = self.hub.state.lock().unwrap();
        let sender = state
            .senders
            .entry(organization)
            .or_insert_with(|| broadcast::channel(BUFFER).0);

        Subscription {
            receiver: sender.subscribe(),
        }
    }

    /// Never called before the transaction committed: a refused or rolled-back write has nothing
    /// to announce.
    pub fn publish(&self, touched: Touched) {
        if touched.is_empty() {
            return;
        }
        let at = Instant::now() + COALESCE;

        let start = {
            let mut state = self.hub.state.lock().unwrap();
            for (organization, resource) in touched.resources {
                state.pending.entry((organization, resource)).or_insert(at);
            }
            if touched.queues_everywhere {
                for organization in state.senders.keys().copied().collect::<Vec<_>>() {
                    state
                        .pending
                        .entry((organization, Resource::Queue))
                        .or_insert(at);
                }
            }
            !self.hub.flushing.swap(true, Ordering::SeqCst)
        };

        if start {
            tokio::spawn(flush(self.clone()));
        }
    }
}

async fn flush(notices: Notices) {
    loop {
        let deadline = notices
            .hub
            .state
            .lock()
            .unwrap()
            .pending
            .values()
            .min()
            .copied();
        let Some(deadline) = deadline else {
            let state = notices.hub.state.lock().unwrap();
            if state.pending.is_empty() {
                notices.hub.flushing.store(false, Ordering::SeqCst);
                return;
            }
            continue;
        };

        tokio::time::sleep_until(deadline).await;

        let mut state = notices.hub.state.lock().unwrap();
        let now = Instant::now();
        let due = state
            .pending
            .iter()
            .filter(|(_, at)| **at <= now)
            .map(|(key, _)| *key)
            .collect::<Vec<_>>();
        for key in &due {
            state.pending.remove(key);
        }
        for (organization, resource) in due {
            if let Some(sender) = state.senders.get(&organization) {
                let _ = sender.send(resource);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn notices() -> (Notices, OrganizationId, Subscription) {
        let notices = Notices::default();
        let organization = OrganizationId::generate();
        let subscription = notices.subscribe(organization);

        (notices, organization, subscription)
    }

    fn drain(events: &mut Vec<Watch>, subscription: &mut Subscription) {
        while let Ok(resource) = subscription.receiver.try_recv() {
            events.push(Watch::Change(resource));
        }
    }

    #[tokio::test(start_paused = true)]
    async fn twenty_changes_to_one_session_coalesce_into_at_most_two_notices() {
        let (notices, organization, mut subscription) = notices();
        let session = SessionId::generate();

        for _ in 0..20 {
            let mut touched = Touched::default();
            touched.session_id(organization, session);
            notices.publish(touched);
        }
        tokio::time::sleep(COALESCE * 2).await;

        let mut events = Vec::new();
        drain(&mut events, &mut subscription);
        let session_notices = events
            .iter()
            .filter(|event| matches!(event, Watch::Change(Resource::Session(id)) if *id == session))
            .count();
        assert!(
            (1..=2).contains(&session_notices),
            "twenty changes became {session_notices} notices"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_subscriber_that_falls_behind_is_told_to_resync() {
        let (notices, organization, mut subscription) = notices();

        let mut touched = Touched::default();
        for _ in 0..=BUFFER {
            touched.session_id(organization, SessionId::generate());
        }
        notices.publish(touched);
        tokio::time::sleep(COALESCE * 2).await;

        assert!(matches!(subscription.recv().await, Some(Watch::Resync)));
        assert!(matches!(
            subscription.recv().await,
            Some(Watch::Change(Resource::Session(_)))
        ));
    }

    #[tokio::test(start_paused = true)]
    async fn a_session_notice_stays_in_the_organization_that_changed() {
        let notices = Notices::default();
        let ours = OrganizationId::generate();
        let theirs = OrganizationId::generate();
        let mut ours = notices.subscribe(ours);

        let mut touched = Touched::default();
        touched.session_id(theirs, SessionId::generate());
        notices.publish(touched);
        tokio::time::sleep(COALESCE * 2).await;

        assert!(ours.receiver.try_recv().is_err());
    }

    #[tokio::test(start_paused = true)]
    async fn a_touched_queue_with_no_organization_reaches_every_subscriber() {
        let notices = Notices::default();
        let mut ours = notices.subscribe(OrganizationId::generate());

        let mut touched = Touched::default();
        touched.every_queue();
        notices.publish(touched);
        tokio::time::sleep(COALESCE * 2).await;

        assert!(matches!(
            ours.recv().await,
            Some(Watch::Change(Resource::Queue))
        ));
    }
}
