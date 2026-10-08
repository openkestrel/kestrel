use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;
use tracing::warn;

use crate::domain::Usage;
use crate::log::ToolStatus;
use crate::store::Store;
use crate::store::workspace::Linked;

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

    pub fn report(&self, instance: &str, summary: Summary) {
        if let Some(live) = self.0.lock().unwrap().get_mut(instance) {
            live.summary = Some(summary);
        }
    }

    pub fn get(&self, instance: &str) -> Option<Summary> {
        self.0.lock().unwrap().get(instance)?.summary.clone()
    }
}

/// Newest per Instance.
#[derive(Clone, Default)]
pub struct Unrecorded(Arc<(Mutex<Pending>, Notify)>);

type Pending = HashMap<Linked, Summary>;

impl Unrecorded {
    pub fn report(&self, reporter: Linked, summary: Summary) {
        let (pending, wake) = &*self.0;
        pending.lock().unwrap().insert(reporter, summary);
        wake.notify_one();
    }

    /// The only writer, so a report never waits on the write lock: the supervisor waits on each
    /// report before it takes its next read, and a stalled report would time that read out.
    pub async fn record(&self, store: &Store, stop: CancellationToken) {
        let (pending, wake) = &*self.0;
        loop {
            let stopping = tokio::select! {
                () = wake.notified() => false,
                () = stop.cancelled() => true,
            };
            let taken = std::mem::take(&mut *pending.lock().unwrap());
            if let Err(error) = record_reports(store, &taken).await {
                warn!(
                    error = format!("{error:#}"),
                    reports = taken.len(),
                    "the work reports could not be recorded"
                );
                {
                    let mut pending = pending.lock().unwrap();
                    for (reporter, summary) in taken {
                        pending.entry(reporter).or_insert(summary);
                    }
                }
                if !stopping {
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                    wake.notify_one();
                }
            }
            if stopping {
                return;
            }
        }
    }
}

async fn record_reports(store: &Store, taken: &Pending) -> anyhow::Result<()> {
    if taken.is_empty() {
        return Ok(());
    }
    let mut tx = store.begin().await?;
    for (reporter, summary) in taken {
        tx.workspaces()
            .record_work_report(reporter, summary)
            .await?;
    }
    tx.commit().await
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
                live.summary = None;
                live.sessions
                    .retain(|_, state| !state.tools.is_empty() || !state.units.is_empty());
                if live.sessions.is_empty() {
                    summaries.remove(&self.instance);
                }
            }
        }
    }
}

#[derive(Serialize)]
pub struct Work {
    #[serde(flatten)]
    pub current: Current,
    pub last_report: LastReport,
    /// Only `Received`, each history for its own Instance.
    pub earlier_reports: Vec<LastReport>,
}

#[derive(Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Current {
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

#[derive(Serialize)]
#[serde(tag = "report", rename_all = "snake_case")]
pub enum LastReport {
    None,
    Received {
        instance: String,
        current_instance: bool,
        #[serde(flatten)]
        summary: Summary,
    },
}

pub async fn read(
    store: &Store,
    summaries: &Summaries,
    organization: &str,
    reference: &str,
) -> anyhow::Result<Work> {
    let mut tx = store.read().await?;
    let organization = tx.organizations().named(organization).await?;
    let workspace = tx.workspaces().resolved(&organization, reference).await?;
    let instance = tx.workspaces().instance(workspace.id).await?;
    let mut reports = tx
        .workspaces()
        .work_reports(workspace.id)
        .await?
        .into_iter()
        .map(|report| LastReport::Received {
            current_instance: instance.as_ref() == Some(&report.instance),
            instance: report.instance,
            summary: report.summary,
        });
    let last_report = reports.next().unwrap_or(LastReport::None);
    let earlier_reports = reports.collect();
    let Some(instance) = instance else {
        return Ok(Work {
            current: Current::NoInstance {
                branch: workspace.checkout.branch,
                pull_request: None,
            },
            last_report,
            earlier_reports,
        });
    };
    let on_the_link = tx
        .workspaces()
        .supervisor(&instance)
        .await?
        .and_then(|supervisor| supervisor.reached_at)
        .is_some_and(|reached| Timestamp::now().duration_since(reached) < crate::link::ON_THE_LINK);
    let current = match summaries.get(&instance) {
        Some(summary) if on_the_link => Current::Reported { summary },
        _ => Current::NotAnswering {
            message: "the Instance isn't answering",
        },
    };
    Ok(Work {
        current,
        last_report,
        earlier_reports,
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

    pub fn clear_session(&self, session: &crate::domain::Session) {
        let Some(instance) = session.instance.as_deref() else {
            return;
        };
        let mut summaries = self.0.lock().unwrap();
        if let Some(live) = summaries.get_mut(instance) {
            live.sessions.remove(&session.id.to_string());
            if live.streams == 0 && live.sessions.is_empty() {
                summaries.remove(instance);
            }
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
}
