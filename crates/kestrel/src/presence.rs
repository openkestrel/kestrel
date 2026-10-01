//! Who is following a Workspace's Transcript right now. Memory only: presence never enters the
//! Transcript or a change notice, and nothing reads it to decide anything (ADR-0035).

use std::collections::HashMap;
use std::fmt;
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use tokio::sync::watch;
use tokio::time::Instant;
use uuid::Uuid;

use crate::domain::WorkspaceId;

/// How long a follower's lease lasts before it must renew.
pub const LEASE: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub struct FollowerId(Uuid);

impl FollowerId {
    pub fn generate() -> Self {
        Self(Uuid::now_v7())
    }
}

impl fmt::Display for FollowerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl FromStr for FollowerId {
    type Err = uuid::Error;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        Ok(Self(text.parse()?))
    }
}

/// The whole current set, sent as one snapshot: a named follow displays once per name, and
/// unnamed follows are counted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Presence {
    pub named: Vec<String>,
    pub anonymous: usize,
}

#[derive(Clone)]
pub struct Followers {
    hub: Arc<Hub>,
}

struct Hub {
    lease: Duration,
    workspaces: Mutex<HashMap<WorkspaceId, Watching>>,
    sweeping: AtomicBool,
}

struct Watching {
    presence: watch::Sender<Presence>,
    followers: HashMap<FollowerId, Follower>,
}

struct Follower {
    name: Option<String>,
    deadline: Instant,
    expired: watch::Sender<bool>,
}

impl Followers {
    pub fn new(lease: Duration) -> Self {
        Self {
            hub: Arc::new(Hub {
                lease,
                workspaces: Mutex::new(HashMap::new()),
                sweeping: AtomicBool::new(false),
            }),
        }
    }

    /// Registers a follower of the Workspace and hands back its id, its lease, presence as it
    /// changes, word of its own expiry, and a guard whose drop removes it.
    pub fn join(&self, workspace: WorkspaceId, name: Option<String>) -> Joined {
        let now = Instant::now();
        let (id, presence, expiration, start) = {
            let mut workspaces = self.hub.workspaces.lock().unwrap();
            let watching = workspaces.entry(workspace).or_insert_with(|| Watching {
                presence: watch::channel(Presence {
                    named: Vec::new(),
                    anonymous: 0,
                })
                .0,
                followers: HashMap::new(),
            });

            let id = FollowerId::generate();
            let (expired, expiration) = watch::channel(false);
            watching.followers.insert(
                id,
                Follower {
                    name,
                    deadline: now + self.hub.lease,
                    expired,
                },
            );
            watching
                .presence
                .send_replace(snapshot(&watching.followers));
            let presence = watching.presence.subscribe();
            let start = !self.hub.sweeping.swap(true, Ordering::SeqCst);

            (id, presence, expiration, start)
        };

        if start {
            tokio::spawn(sweep(self.clone()));
        }

        Joined {
            id,
            lease: self.hub.lease,
            presence,
            expiration,
            hub: self.hub.clone(),
            workspace,
        }
    }

    /// Extends a known, unexpired follower's lease to a full period from now. `false` for one
    /// that is unknown or whose lease has already passed.
    pub fn renew(&self, workspace: WorkspaceId, id: FollowerId) -> bool {
        let now = Instant::now();
        let mut workspaces = self.hub.workspaces.lock().unwrap();
        let Some(watching) = workspaces.get_mut(&workspace) else {
            return false;
        };
        let Some(follower) = watching.followers.get_mut(&id) else {
            return false;
        };
        if follower.deadline <= now {
            return false;
        }

        follower.deadline = now + self.hub.lease;

        true
    }
}

/// One registered follower's view. Dropping it leaves at once, however the stream ended.
pub struct Joined {
    pub id: FollowerId,
    pub lease: Duration,
    pub presence: watch::Receiver<Presence>,
    /// Set when the lease expired and the follower was removed: its stream has nothing left.
    pub expiration: watch::Receiver<bool>,
    hub: Arc<Hub>,
    workspace: WorkspaceId,
}

impl Joined {
    /// The snapshot as it stands, marked seen so the next change is a real update.
    pub fn snapshot(&mut self) -> Presence {
        self.presence.borrow_and_update().clone()
    }
}

impl Drop for Joined {
    fn drop(&mut self) {
        let mut workspaces = self.hub.workspaces.lock().unwrap();
        let Some(watching) = workspaces.get_mut(&self.workspace) else {
            return;
        };
        if watching.followers.remove(&self.id).is_some() {
            watching
                .presence
                .send_replace(snapshot(&watching.followers));
        }
        if watching.followers.is_empty() {
            workspaces.remove(&self.workspace);
        }
    }
}

async fn sweep(followers: Followers) {
    loop {
        let deadline = followers
            .hub
            .workspaces
            .lock()
            .unwrap()
            .values()
            .flat_map(|watching| {
                watching
                    .followers
                    .values()
                    .map(|follower| follower.deadline)
            })
            .min();
        let Some(deadline) = deadline else {
            let workspaces = followers.hub.workspaces.lock().unwrap();
            if workspaces
                .values()
                .all(|watching| watching.followers.is_empty())
            {
                followers.hub.sweeping.store(false, Ordering::SeqCst);
                return;
            }
            continue;
        };

        tokio::time::sleep_until(deadline).await;

        let now = Instant::now();
        let mut workspaces = followers.hub.workspaces.lock().unwrap();
        for watching in workspaces.values_mut() {
            let expired: Vec<FollowerId> = watching
                .followers
                .iter()
                .filter(|(_, follower)| follower.deadline <= now)
                .map(|(id, _)| *id)
                .collect();
            for id in &expired {
                if let Some(follower) = watching.followers.remove(id) {
                    follower.expired.send_replace(true);
                }
            }
            if !expired.is_empty() {
                watching
                    .presence
                    .send_replace(snapshot(&watching.followers));
            }
        }
        workspaces.retain(|_, watching| !watching.followers.is_empty());
    }
}

fn snapshot(followers: &HashMap<FollowerId, Follower>) -> Presence {
    let mut named: Vec<String> = followers
        .values()
        .filter_map(|follower| follower.name.clone())
        .collect();
    named.sort();
    named.dedup();

    Presence {
        named,
        anonymous: followers
            .values()
            .filter(|follower| follower.name.is_none())
            .count(),
    }
}
