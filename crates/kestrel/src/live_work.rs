use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use jiff::Timestamp;
use serde::{Deserialize, Serialize};

use crate::domain::Usage;
use crate::log::ToolStatus;

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

    /// Reports the repositories the Instance holds and says whether that changed what a reader
    /// sees: a report saying what the last one said raises nothing, and the time still moves.
    pub fn report(&self, instance: &str, repositories: Vec<Repository>) -> bool {
        let mut summaries = self.0.lock().unwrap();
        let Some(live) = summaries.get_mut(instance) else {
            return false;
        };
        let changed =
            live.summary.as_ref().map(|summary| &summary.repositories) != Some(&repositories);
        live.summary = Some(Summary {
            repositories,
            reported_at: Timestamp::now(),
        });

        changed
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
    pub tool_kind: String,
    pub status: ToolStatus,
    pub started_at: Timestamp,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunningUnit {
    pub id: String,
    pub kind: UnitKind,
    pub title: String,
    pub started_at: Timestamp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnitKind {
    BackgroundTask,
    Subagent,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SessionState {
    pub tools: Vec<RunningTool>,
    pub units: Vec<RunningUnit>,
    pub message_buffering: bool,
    pub thought_buffering: bool,
    /// Held in memory, never a row (ADR-0041).
    #[serde(default)]
    pub usage: Option<Usage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_activity_at: Option<Timestamp>,
}

impl Summaries {
    /// Reports a Session's live state and says whether a call or unit opened or settled, the only
    /// changes that raise a notice: an update to one already listed raises none (ADR-0037).
    pub fn report_session(&self, instance: &str, session: &str, state: SessionState) -> bool {
        let mut summaries = self.0.lock().unwrap();
        let Some(live) = summaries.get_mut(instance) else {
            return false;
        };
        let before = live
            .sessions
            .insert(session.to_owned(), state.clone())
            .unwrap_or_default();

        turned_over(&before.tools, &state.tools, |tool| tool.call_id.clone())
            || turned_over(&before.units, &state.units, |unit| unit.id.clone())
    }

    pub fn report_usage(&self, instance: &str, session: &str, usage: Usage) {
        if let Some(live) = self.0.lock().unwrap().get_mut(instance) {
            live.sessions.entry(session.to_owned()).or_default().usage = Some(usage);
        }
    }
    pub fn current_session(&self, session: &crate::domain::Session) -> SessionState {
        if crate::domain::SessionState::LIVE.contains(&session.state)
            && session
                .lease_expires_at
                .is_some_and(|at| at > Timestamp::now())
        {
            session
                .instance
                .as_deref()
                .map(|instance| self.session(instance, &session.id.to_string()))
                .unwrap_or_default()
        } else {
            SessionState::default()
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

/// Whether a call or unit opened or settled between two snapshots. An entry whose id is unchanged
/// is a progress update, whatever else about it moved, and raises nothing (ADR-0037).
fn turned_over<T>(before: &[T], after: &[T], id: impl Fn(&T) -> String) -> bool {
    let ids = |items: &[T]| items.iter().map(&id).collect::<HashSet<String>>();

    ids(before) != ids(after)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a_tool(title: &str) -> RunningTool {
        RunningTool {
            call_id: "call".to_owned(),
            title: title.to_owned(),
            tool_kind: "read".to_owned(),
            status: ToolStatus::InProgress,
            started_at: "2026-09-29T12:00:00Z".parse().unwrap(),
        }
    }

    fn a_unit() -> RunningUnit {
        RunningUnit {
            id: "task".to_owned(),
            kind: UnitKind::BackgroundTask,
            title: "tests".to_owned(),
            started_at: "2026-09-29T12:00:00Z".parse().unwrap(),
        }
    }

    #[test]
    fn a_call_or_unit_opening_or_settling_is_noticed_but_a_progress_update_is_not() {
        let summaries = Summaries::default();
        let _connection = summaries.connected("instance");

        assert!(summaries.report_session(
            "instance",
            "session",
            SessionState {
                tools: vec![a_tool("read a")],
                ..SessionState::default()
            }
        ));
        assert!(!summaries.report_session(
            "instance",
            "session",
            SessionState {
                tools: vec![a_tool("read b")],
                ..SessionState::default()
            }
        ));
        assert!(summaries.report_session("instance", "session", SessionState::default()));

        assert!(summaries.report_session(
            "instance",
            "session",
            SessionState {
                units: vec![a_unit()],
                ..SessionState::default()
            }
        ));
        assert!(summaries.report_session("instance", "session", SessionState::default()));
    }

    #[test]
    fn a_work_summary_that_says_what_the_last_one_did_is_not_noticed() {
        let summaries = Summaries::default();
        let _connection = summaries.connected("instance");
        let repository = || Repository {
            repository: "https://github.com/jtmthf/kestrel".to_owned(),
            git: Git::Read {
                branch: Some("main".to_owned()),
                changed: Changes {
                    files: 1,
                    added: 2,
                    removed: 3,
                },
                staged: Changes {
                    files: 0,
                    added: 0,
                    removed: 0,
                },
                committed: Commits {
                    commits: 0,
                    added: 0,
                    removed: 0,
                },
                pushed: None,
                untracked: 0,
                stashed: 0,
            },
        };

        assert!(summaries.report("instance", vec![repository()]));
        assert!(!summaries.report("instance", vec![repository()]));
    }
}
