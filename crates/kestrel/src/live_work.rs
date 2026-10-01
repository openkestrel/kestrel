use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use jiff::Timestamp;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Repository {
    pub repository: String,
    #[serde(flatten)]
    pub git: Git,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "git", rename_all = "snake_case")]
pub enum Git {
    Read {
        branch: Option<String>,
        changed: Changes,
        staged: Changes,
        committed: Commits,
        pushed: Option<String>,
        untracked: u64,
        stashed: u64,
    },
    Unreadable {
        because: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Changes {
    pub files: u64,
    pub added: u64,
    pub removed: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Commits {
    pub commits: u64,
    pub added: u64,
    pub removed: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Summary {
    pub repositories: Vec<Repository>,
    pub reported_at: Timestamp,
}

#[derive(Default)]
struct Live {
    streams: usize,
    summary: Option<Summary>,
    sessions: HashMap<String, SessionState>,
}

#[derive(Clone, Default)]
pub struct Summaries(Arc<Mutex<HashMap<String, Live>>>);

impl Summaries {
    pub fn connected(&self, instance: &str) -> Connection {
        self.0
            .lock()
            .unwrap()
            .entry(instance.to_owned())
            .or_default()
            .streams += 1;
        Connection {
            summaries: self.clone(),
            instance: instance.to_owned(),
        }
    }

    pub fn report(&self, instance: &str, repositories: Vec<Repository>) {
        if let Some(live) = self.0.lock().unwrap().get_mut(instance) {
            live.summary = Some(Summary {
                repositories,
                reported_at: Timestamp::now(),
            });
        }
    }

    pub fn get(&self, instance: &str) -> Option<Summary> {
        self.0.lock().unwrap().get(instance)?.summary.clone()
    }
}

pub struct Connection {
    summaries: Summaries,
    instance: String,
}

impl Drop for Connection {
    fn drop(&mut self) {
        let mut summaries = self.summaries.0.lock().unwrap();
        if let Some(live) = summaries.get_mut(&self.instance) {
            live.streams -= 1;
            if live.streams == 0 {
                summaries.remove(&self.instance);
            }
        }
    }
}

#[derive(Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Work {
    Reported {
        #[serde(flatten)]
        summary: Summary,
    },
    NoInstance {
        branch: String,
        pull_request: Option<String>,
    },
    NotAnswering {
        message: &'static str,
    },
}

pub async fn read(
    store: &crate::store::Store,
    summaries: &Summaries,
    organization: &str,
    reference: &str,
) -> anyhow::Result<Work> {
    let mut tx = store.read().await?;
    let organization = tx.organizations().named(organization).await?;
    let workspace = tx.workspaces().resolved(&organization, reference).await?;
    let Some(instance) = tx.workspaces().instance(workspace.id).await? else {
        return Ok(Work::NoInstance {
            branch: workspace.checkout.branch,
            pull_request: None,
        });
    };
    let on_the_link = tx
        .workspaces()
        .supervisor(&instance)
        .await?
        .and_then(|supervisor| supervisor.reached_at)
        .is_some_and(|reached| Timestamp::now().duration_since(reached) < crate::link::ON_THE_LINK);
    if on_the_link && let Some(summary) = summaries.get(&instance) {
        return Ok(Work::Reported { summary });
    }
    Ok(Work::NotAnswering {
        message: "the Instance isn't answering",
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunningTool {
    pub call_id: String,
    pub title: String,
    pub status: String,
    pub started_at: Timestamp,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionState {
    pub tools: Vec<RunningTool>,
    pub message_buffering: bool,
    pub thought_buffering: bool,
}

impl Summaries {
    pub fn report_session(&self, instance: &str, session: &str, state: SessionState) {
        if let Some(live) = self.0.lock().unwrap().get_mut(instance) {
            live.sessions.insert(session.to_owned(), state);
        }
    }
    pub fn session(&self, instance: &str, session: &str) -> SessionState {
        self.0
            .lock()
            .unwrap()
            .get(instance)
            .and_then(|live| live.sessions.get(session))
            .cloned()
            .unwrap_or_default()
    }
}
