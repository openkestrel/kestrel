use jiff::{SignedDuration, Timestamp};
use std::collections::{BTreeMap, BTreeSet};

use agent_client_protocol::schema::v1::{
    ContentBlock, ContentChunk, SessionUpdate, ToolCall, ToolCallStatus,
};

use crate::link::{
    ClosingReason, Completion, Cost, PlanEntry, Report, RunningUnit, ToolStatus, TurnOutcome,
    UnitKind, Usage,
};

#[derive(Default)]
pub struct Completer {
    messages: Stream,
    thoughts: Stream,
    tools: BTreeMap<String, (ToolCall, Timestamp)>,
    completed_tools: BTreeSet<String>,
    units: BTreeMap<String, RunningUnit>,
    pub produced: bool,
    phase: Phase,
    last_activity: Option<Timestamp>,
    reported_activity: Option<Timestamp>,
}

#[derive(Default, Clone, Copy, PartialEq, Eq)]
enum Phase {
    #[default]
    Unprompted,
    Turn,
    Trailing,
    Between,
}

/// How often a trailing Session's last activity alone is worth a new snapshot.
const ACTIVITY_GRAIN: SignedDuration = SignedDuration::from_secs(1);

pub enum Settling {
    Settled(Completed),
    /// Asked again then; a Completer that is not trailing is asked again a quiet period on, since
    /// activity may start it trailing in between.
    Until(Timestamp),
}

pub enum UnitChange {
    Opened {
        id: String,
        kind: UnitKind,
        title: String,
    },
    Progressed {
        id: String,
    },
    Settled {
        id: String,
    },
}

#[derive(Default)]
struct Stream {
    open: Option<Text>,
    completed: BTreeSet<String>,
}

struct Text {
    id: Option<String>,
    text: String,
    started_at: Timestamp,
}

#[derive(Default)]
pub struct Completed {
    pub reports: Vec<Report>,
    pub diagnostics: Vec<String>,
    pub state: Option<Report>,
}

impl Completer {
    pub fn begin(&mut self) {
        self.produced = false;
        self.phase = Phase::Turn;
        self.last_activity = None;
    }

    pub fn update(&mut self, update: SessionUpdate, now: Timestamp) -> Completed {
        let mut completed = Completed::default();
        let before = self.snapshot();
        match update {
            SessionUpdate::AgentMessageChunk(chunk) => {
                self.active(now);
                self.produced |= self.messages.chunk(chunk, now, false, &mut completed);
            }
            SessionUpdate::AgentThoughtChunk(chunk) => {
                self.active(now);
                self.produced |= self.thoughts.chunk(chunk, now, true, &mut completed);
            }
            SessionUpdate::Plan(plan) => {
                self.active(now);
                {
                    self.produced = true;
                    completed.reports.push(Report::Plan {
                        entries: plan
                            .entries
                            .into_iter()
                            .map(|entry| PlanEntry {
                                content: entry.content,
                                priority: serde_json::to_value(entry.priority)
                                    .expect("a plan priority")
                                    .as_str()
                                    .expect("a priority string")
                                    .to_owned(),
                                status: serde_json::to_value(entry.status)
                                    .expect("a plan status")
                                    .as_str()
                                    .expect("a status string")
                                    .to_owned(),
                            })
                            .collect(),
                        completion: Completion::at(now),
                    });
                }
            }
            SessionUpdate::ToolCall(call) => {
                self.active(now);
                let id = call.tool_call_id.0.to_string();
                if self.completed_tools.contains(&id) || self.tools.contains_key(&id) {
                    completed.diagnostics.push(format!(
                        "kestrel: a late or duplicate tool start arrived for {id}"
                    ));
                } else {
                    self.produced = true;
                    self.tools.insert(id.clone(), (call, now));
                    self.settle_tool(&id, now, None, false, &mut completed);
                }
            }
            SessionUpdate::ToolCallUpdate(update) => {
                self.active(now);
                let id = update.tool_call_id.0.to_string();
                if let Some((call, _)) = self.tools.get_mut(&id) {
                    call.update(update.fields);
                    self.settle_tool(&id, now, None, false, &mut completed);
                } else {
                    completed.diagnostics.push(format!(
                        "kestrel: an update arrived for unknown or completed tool {id}"
                    ));
                }
            }
            SessionUpdate::UsageUpdate(usage) => completed.reports.push(Report::Usage {
                usage: Usage {
                    context_used: usage.used,
                    context_size: usage.size,
                    cost: usage.cost.map(|cost| Cost {
                        amount: cost.amount,
                        currency: cost.currency,
                    }),
                },
            }),
            _ => {}
        }
        self.restate(&before, &mut completed);
        completed
    }

    /// A settled unit adds no entry: whatever it produced arrived as updates of its own.
    pub fn unit(&mut self, change: UnitChange, now: Timestamp) -> Completed {
        let mut completed = Completed::default();
        let before = self.snapshot();
        self.active(now);
        match change {
            UnitChange::Opened { id, kind, title } => {
                if self.units.contains_key(&id) {
                    completed
                        .diagnostics
                        .push(format!("kestrel: a duplicate start arrived for unit {id}"));
                } else {
                    self.units.insert(
                        id.clone(),
                        RunningUnit {
                            id,
                            kind,
                            title,
                            started_at: now,
                        },
                    );
                }
            }
            UnitChange::Progressed { id } if !self.units.contains_key(&id) => {
                completed.diagnostics.push(format!(
                    "kestrel: progress arrived for unknown or settled unit {id}"
                ))
            }
            UnitChange::Progressed { .. } => {}
            UnitChange::Settled { id } => {
                if self.units.remove(&id).is_none() {
                    completed.diagnostics.push(format!(
                        "kestrel: an end arrived for unknown or settled unit {id}"
                    ));
                }
            }
        }
        self.restate(&before, &mut completed);
        completed
    }

    fn restate(&mut self, before: &Report, completed: &mut Completed) {
        let grain_passed = matches!(
            (self.last_activity, self.reported_activity),
            (Some(last), Some(reported)) if last.duration_since(reported) >= ACTIVITY_GRAIN
        );
        if *before != self.snapshot() || grain_passed {
            completed.state = Some(self.reported());
        }
    }

    fn active(&mut self, now: Timestamp) {
        if matches!(self.phase, Phase::Trailing | Phase::Between) {
            self.phase = Phase::Trailing;
            self.last_activity = Some(now);
        }
    }

    pub fn settle(&mut self, now: Timestamp, quiet: SignedDuration) -> Settling {
        let Some(last) = self.last_activity.filter(|_| self.phase == Phase::Trailing) else {
            return Settling::Until(now + quiet);
        };
        if !self.tools.is_empty() || !self.units.is_empty() {
            return Settling::Until(now + quiet);
        }
        if now < last + quiet {
            return Settling::Until(last + quiet);
        }
        let mut completed = Completed::default();
        self.messages.close(now, false, None, &mut completed);
        self.thoughts.close(now, true, None, &mut completed);
        for id in self.tools.keys().cloned().collect::<Vec<_>>() {
            self.settle_tool(&id, now, None, true, &mut completed);
        }
        self.phase = Phase::Between;
        self.last_activity = None;
        completed.state = Some(self.reported());

        Settling::Settled(completed)
    }

    /// An answer leaves open calls and units running and starts the Session trailing; a call
    /// still open when trailing ends is closed `unresolved`, whatever ended it. Any other
    /// boundary drops open units, since it leaves the Session waiting or ended.
    pub fn boundary(&mut self, outcome: TurnOutcome, now: Timestamp) -> Completed {
        let mut completed = Completed::default();
        self.messages
            .close(now, false, Some(outcome.clone()), &mut completed);
        self.thoughts
            .close(now, true, Some(outcome.clone()), &mut completed);
        let trailing = self.phase == Phase::Trailing;
        if !matches!(outcome, TurnOutcome::Answered { .. }) || trailing {
            for id in self.tools.keys().cloned().collect::<Vec<_>>() {
                self.settle_tool(&id, now, Some(outcome.clone()), trailing, &mut completed);
            }
        }
        if !matches!(outcome, TurnOutcome::Answered { .. }) {
            self.units.clear();
        }
        match outcome {
            TurnOutcome::Answered { .. } => {
                self.phase = Phase::Trailing;
                self.last_activity = Some(now);
            }
            _ if trailing => self.last_activity = Some(now),
            _ => self.phase = Phase::Between,
        }
        completed.state = Some(self.reported());
        completed
    }

    fn reported(&mut self) -> Report {
        self.reported_activity = self.last_activity;
        self.snapshot()
    }

    pub fn snapshot(&self) -> Report {
        Report::SessionState {
            tools: self
                .tools
                .iter()
                .map(|(id, (call, started_at))| crate::link::RunningTool {
                    call_id: id.clone(),
                    title: call.title.clone(),
                    status: serde_json::to_value(call.status)
                        .unwrap()
                        .as_str()
                        .unwrap()
                        .to_owned(),
                    started_at: *started_at,
                })
                .collect(),
            units: self.units.values().cloned().collect(),
            message_buffering: self.messages.open.is_some(),
            thought_buffering: self.thoughts.open.is_some(),
            // Filled in when the state goes up the link, which is what holds the latest usage.
            usage: None,
            last_activity_at: self.last_activity.filter(|_| self.phase == Phase::Trailing),
        }
    }

    fn settle_tool(
        &mut self,
        id: &str,
        now: Timestamp,
        outcome: Option<TurnOutcome>,
        unresolved: bool,
        completed: &mut Completed,
    ) {
        let Some((call, _)) = self.tools.get(id) else {
            return;
        };
        if !unresolved
            && outcome.is_none()
            && !matches!(
                call.status,
                ToolCallStatus::Completed | ToolCallStatus::Failed
            )
        {
            return;
        }
        let (call, started_at) = self.tools.remove(id).unwrap();
        self.completed_tools.insert(id.to_owned());
        let closing_reason = match (&outcome, unresolved) {
            (_, true) => Some(ClosingReason::Unresolved),
            (Some(TurnOutcome::Cancelled), false) => Some(ClosingReason::Interrupted),
            (Some(TurnOutcome::Failed { .. }), false) => Some(ClosingReason::Failed),
            (Some(TurnOutcome::Answered { .. }), false) => Some(ClosingReason::Unresolved),
            (None, false) => None,
        };
        let status = match call.status {
            ToolCallStatus::InProgress => ToolStatus::InProgress,
            ToolCallStatus::Completed => ToolStatus::Completed,
            ToolCallStatus::Failed => ToolStatus::Failed,
            _ => ToolStatus::Pending,
        };
        completed.reports.push(Report::ToolCall {
            call_id: id.to_owned(),
            title: call.title,
            tool_kind: serde_json::to_value(call.kind)
                .unwrap()
                .as_str()
                .unwrap()
                .to_owned(),
            status,
            input: call.raw_input.unwrap_or(serde_json::Value::Null),
            result: Box::new(serde_json::json!({"content":call.content,"output":call.raw_output})),
            closing_reason,
            completion: Completion {
                started_at,
                finished_at: now,
                turn_outcome: outcome,
            },
        });
    }
}

impl Stream {
    fn chunk(
        &mut self,
        chunk: ContentChunk,
        now: Timestamp,
        thought: bool,
        completed: &mut Completed,
    ) -> bool {
        let id = chunk.message_id.as_ref().map(|id| id.0.to_string());
        if id.as_ref().is_some_and(|id| self.completed.contains(id)) {
            completed.diagnostics.push(format!(
                "kestrel: an update arrived for completed {} {}",
                if thought { "thought" } else { "message" },
                id.as_deref().unwrap_or_default()
            ));
            return false;
        }
        if self.open.as_ref().is_some_and(|open| open.id != id) {
            self.close(now, thought, None, completed);
        }
        let ContentBlock::Text(text) = chunk.content else {
            return true;
        };
        match &mut self.open {
            Some(open) => open.text.push_str(&text.text),
            None => {
                self.open = Some(Text {
                    id: id.clone(),
                    text: text.text,
                    started_at: now,
                })
            }
        }
        if id.is_none() {
            self.close(now, thought, None, completed);
        }
        true
    }

    fn close(
        &mut self,
        now: Timestamp,
        thought: bool,
        turn_outcome: Option<TurnOutcome>,
        completed: &mut Completed,
    ) {
        let Some(open) = self.open.take() else {
            return;
        };
        if let Some(id) = open.id {
            self.completed.insert(id);
        }
        let completion = Completion {
            started_at: open.started_at,
            finished_at: now,
            turn_outcome,
        };
        completed.reports.push(if thought {
            Report::Thought {
                text: open.text,
                completion,
            }
        } else {
            Report::Said {
                message: open.text,
                completion,
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_client_protocol::schema::v1::{ContentBlock, ContentChunk, MessageId, TextContent};

    use jiff::SignedDuration;

    const NOW: Timestamp = Timestamp::constant(1_790_683_200, 0);

    fn chunk(id: Option<&str>, text: &str) -> ContentChunk {
        ContentChunk::new(ContentBlock::Text(TextContent::new(text)))
            .message_id(id.map(MessageId::new))
    }

    #[test]
    fn tool_updates_replace_current_state_and_preserve_call_content_until_completion() {
        use agent_client_protocol::schema::v1::{ToolCallUpdate, ToolCallUpdateFields};
        let mut completer = Completer::default();
        completer.update(
            SessionUpdate::AgentMessageChunk(chunk(Some("message"), "buffering")),
            NOW,
        );
        completer.update(
            SessionUpdate::AgentThoughtChunk(chunk(Some("thought"), "thinking")),
            NOW,
        );
        completer.update(
            SessionUpdate::ToolCall(
                ToolCall::new("call", "pending read").raw_input(serde_json::json!({"path":"a"})),
            ),
            NOW,
        );
        let changed = completer.update(
            SessionUpdate::ToolCallUpdate(ToolCallUpdate::new(
                "call",
                ToolCallUpdateFields::new()
                    .title("reading a")
                    .status(ToolCallStatus::InProgress),
            )),
            NOW,
        );
        assert!(changed.reports.is_empty());
        let state = serde_json::to_value(changed.state.unwrap()).unwrap();
        assert_eq!(state["tools"][0]["title"], "reading a");
        assert_eq!(state["tools"][0]["status"], "in_progress");
        assert_eq!(state["message_buffering"], true);
        assert_eq!(state["thought_buffering"], true);
        let update = serde_json::from_value(serde_json::json!({"toolCallId":"call", "status":"failed", "content":[{"type":"content","content":{"type":"text","text":"read failed"}}]})).unwrap();
        let settled = completer.update(SessionUpdate::ToolCallUpdate(update), NOW);
        let report = serde_json::to_value(&settled.reports[0]).unwrap();
        assert_eq!(report["title"], "reading a");
        assert_eq!(report["status"], "failed");
        assert_eq!(report["input"], serde_json::json!({"path":"a"}));
        assert_eq!(
            report["result"]["content"][0]["content"]["text"],
            "read failed"
        );
        assert_eq!(report["closing_reason"], serde_json::Value::Null);
    }

    #[test]
    fn open_tools_close_once_at_a_cancelled_or_failed_boundary_and_late_updates_are_diagnostics() {
        use agent_client_protocol::schema::v1::{ToolCallUpdate, ToolCallUpdateFields};
        for (outcome, reason) in [
            (TurnOutcome::Cancelled, "interrupted"),
            (
                TurnOutcome::Failed {
                    because: "lost".to_owned(),
                },
                "failed",
            ),
        ] {
            let mut completer = Completer::default();
            completer.begin();
            let opened =
                completer.update(SessionUpdate::ToolCall(ToolCall::new("call", "read")), NOW);
            let snapshot = serde_json::to_value(opened.state.unwrap()).unwrap();
            assert_eq!(snapshot["tools"][0]["status"], "pending");
            assert_eq!(snapshot["tools"][0]["started_at"], NOW.to_string());
            let settled = completer.boundary(outcome.clone(), NOW);
            assert_eq!(settled.reports.len(), 1);
            let report = serde_json::to_value(&settled.reports[0]).unwrap();
            assert_eq!(report["status"], "pending");
            assert_eq!(report["closing_reason"], reason);
            assert_eq!(
                report["completion"]["turn_outcome"],
                serde_json::to_value(&outcome).unwrap()
            );
            assert_eq!(
                serde_json::to_value(settled.state.unwrap()).unwrap()["tools"],
                serde_json::json!([])
            );
            assert!(completer.boundary(outcome, NOW).reports.is_empty());
            completer.begin();
            let late = completer.update(
                SessionUpdate::ToolCallUpdate(ToolCallUpdate::new(
                    "call",
                    ToolCallUpdateFields::new().status(ToolCallStatus::Completed),
                )),
                NOW,
            );
            assert!(late.reports.is_empty());
            assert_eq!(late.diagnostics.len(), 1);
            assert!(!completer.produced);
        }
    }

    const QUIET: SignedDuration = SignedDuration::from_secs(30);

    fn at(seconds: i64) -> Timestamp {
        NOW + SignedDuration::from_secs(seconds)
    }

    fn answered() -> TurnOutcome {
        TurnOutcome::Answered {
            stop_reason: "end_turn".to_owned(),
        }
    }

    fn settled(settling: Settling) -> Completed {
        match settling {
            Settling::Settled(completed) => completed,
            Settling::Until(at) => panic!("still trailing until {at}"),
        }
    }

    fn until(settling: Settling) -> Timestamp {
        match settling {
            Settling::Until(at) => at,
            Settling::Settled(_) => panic!("settled"),
        }
    }

    #[test]
    fn an_answer_closes_its_text_and_leaves_open_calls_running_while_the_session_trails() {
        let mut completer = Completer::default();
        completer.begin();
        completer.update(SessionUpdate::ToolCall(ToolCall::new("call", "tests")), NOW);
        completer.update(
            SessionUpdate::AgentMessageChunk(chunk(Some("message"), "waiting on tests")),
            NOW,
        );
        let answer = completer.boundary(answered(), at(1));
        assert_eq!(
            answer.reports,
            vec![Report::Said {
                message: "waiting on tests".to_owned(),
                completion: Completion {
                    started_at: NOW,
                    finished_at: at(1),
                    turn_outcome: Some(answered()),
                },
            }]
        );
        let state = serde_json::to_value(answer.state.unwrap()).unwrap();
        assert_eq!(state["tools"][0]["call_id"], "call");
        assert_eq!(state["last_activity_at"], at(1).to_string());
        assert_eq!(until(completer.settle(at(100), QUIET)), at(100) + QUIET);
    }

    #[test]
    fn activity_restarts_the_quiet_period_and_bookkeeping_does_not() {
        use agent_client_protocol::schema::v1::{
            AvailableCommandsUpdate, Plan, ToolCallUpdate, ToolCallUpdateFields, UsageUpdate,
        };
        let mut completer = Completer::default();
        completer.begin();
        completer.boundary(answered(), NOW);
        assert_eq!(until(completer.settle(at(10), QUIET)), at(30));
        completer.update(SessionUpdate::UsageUpdate(UsageUpdate::new(1, 2)), at(20));
        completer.update(
            SessionUpdate::AvailableCommandsUpdate(AvailableCommandsUpdate::new(Vec::new())),
            at(25),
        );
        assert_eq!(until(completer.settle(at(29), QUIET)), at(30));

        for activity in [
            SessionUpdate::AgentMessageChunk(chunk(None, "said")),
            SessionUpdate::AgentThoughtChunk(chunk(None, "thought")),
            SessionUpdate::Plan(Plan::new(Vec::new())),
            SessionUpdate::ToolCall(
                ToolCall::new("call", "read").status(ToolCallStatus::Completed),
            ),
            SessionUpdate::ToolCallUpdate(ToolCallUpdate::new(
                "open",
                ToolCallUpdateFields::new().status(ToolCallStatus::Completed),
            )),
        ] {
            let mut completer = Completer::default();
            completer.begin();
            completer.update(SessionUpdate::ToolCall(ToolCall::new("open", "read")), NOW);
            completer.boundary(answered(), NOW);
            if !matches!(activity, SessionUpdate::ToolCallUpdate(_)) {
                completer.update(
                    SessionUpdate::ToolCallUpdate(ToolCallUpdate::new(
                        "open",
                        ToolCallUpdateFields::new().status(ToolCallStatus::Completed),
                    )),
                    NOW,
                );
            }
            completer.update(activity, at(31));
            assert_eq!(until(completer.settle(at(31), QUIET)), at(61));
        }
    }

    #[test]
    fn trailing_ends_once_every_open_call_has_settled_and_the_quiet_period_has_passed() {
        use agent_client_protocol::schema::v1::{ToolCallUpdate, ToolCallUpdateFields};
        let mut completer = Completer::default();
        completer.begin();
        completer.update(SessionUpdate::ToolCall(ToolCall::new("call", "tests")), NOW);
        completer.boundary(answered(), NOW);
        assert!(matches!(
            completer.settle(at(600), QUIET),
            Settling::Until(_)
        ));
        let done = completer.update(
            SessionUpdate::ToolCallUpdate(ToolCallUpdate::new(
                "call",
                ToolCallUpdateFields::new().status(ToolCallStatus::Completed),
            )),
            at(700),
        );
        let report = serde_json::to_value(&done.reports[0]).unwrap();
        assert_eq!(report["status"], "completed");
        assert_eq!(report["closing_reason"], serde_json::Value::Null);
        assert_eq!(until(completer.settle(at(710), QUIET)), at(730));
        let ended = settled(completer.settle(at(730), QUIET));
        assert!(ended.reports.is_empty());
        let state = serde_json::to_value(ended.state.unwrap()).unwrap();
        assert_eq!(state["tools"], serde_json::json!([]));
        assert_eq!(state.get("last_activity_at"), None);
    }

    #[test]
    fn the_end_of_trailing_closes_buffered_text_as_a_boundary() {
        let mut completer = Completer::default();
        completer.begin();
        completer.boundary(answered(), NOW);
        completer.update(
            SessionUpdate::AgentMessageChunk(chunk(Some("late"), "pushed")),
            at(1),
        );
        completer.update(
            SessionUpdate::AgentThoughtChunk(chunk(Some("musing"), "CI next")),
            at(2),
        );
        let ended = settled(completer.settle(at(32), QUIET));
        assert_eq!(
            ended.reports,
            vec![
                Report::Said {
                    message: "pushed".to_owned(),
                    completion: Completion {
                        started_at: at(1),
                        finished_at: at(32),
                        turn_outcome: None,
                    },
                },
                Report::Thought {
                    text: "CI next".to_owned(),
                    completion: Completion {
                        started_at: at(2),
                        finished_at: at(32),
                        turn_outcome: None,
                    },
                },
            ]
        );
    }

    #[test]
    fn a_call_open_when_trailing_ends_closes_unresolved_once() {
        use agent_client_protocol::schema::v1::{ToolCallUpdate, ToolCallUpdateFields};
        let mut completer = Completer::default();
        completer.begin();
        completer.update(SessionUpdate::ToolCall(ToolCall::new("call", "tests")), NOW);
        completer.boundary(answered(), NOW);
        let lost = TurnOutcome::Failed {
            because: "the harness exited".to_owned(),
        };
        let ended = completer.boundary(lost.clone(), at(5));
        assert_eq!(ended.reports.len(), 1);
        let report = serde_json::to_value(&ended.reports[0]).unwrap();
        assert_eq!(report["closing_reason"], "unresolved");
        assert!(completer.boundary(lost, at(6)).reports.is_empty());
        let late = completer.update(
            SessionUpdate::ToolCallUpdate(ToolCallUpdate::new(
                "call",
                ToolCallUpdateFields::new().status(ToolCallStatus::Completed),
            )),
            at(7),
        );
        assert!(late.reports.is_empty());
        assert_eq!(late.diagnostics.len(), 1);
    }

    #[test]
    fn activity_after_trailing_ended_trails_again() {
        let mut completer = Completer::default();
        completer.begin();
        completer.boundary(answered(), NOW);
        settled(completer.settle(at(30), QUIET));
        let resumed = completer.update(SessionUpdate::ToolCall(ToolCall::new("ci", "gh")), at(90));
        let state = serde_json::to_value(resumed.state.unwrap()).unwrap();
        assert_eq!(state["last_activity_at"], at(90).to_string());
        assert!(matches!(
            completer.settle(at(500), QUIET),
            Settling::Until(_)
        ));
    }

    fn opened(id: &str) -> UnitChange {
        UnitChange::Opened {
            id: id.to_owned(),
            kind: UnitKind::BackgroundTask,
            title: "background tests".to_owned(),
        }
    }

    #[test]
    fn an_open_unit_keeps_the_session_trailing_however_long_it_is_silent() {
        let mut completer = Completer::default();
        completer.begin();
        completer.unit(opened("task"), NOW);
        let answer = completer.boundary(answered(), at(1));
        let state = serde_json::to_value(answer.state.unwrap()).unwrap();
        assert_eq!(
            state["units"],
            serde_json::json!([{
                "id": "task",
                "kind": "background_task",
                "title": "background tests",
                "started_at": NOW.to_string(),
            }])
        );
        assert!(matches!(
            completer.settle(at(600), QUIET),
            Settling::Until(_)
        ));
        let done = completer.unit(
            UnitChange::Settled {
                id: "task".to_owned(),
            },
            at(700),
        );
        assert!(done.reports.is_empty());
        assert_eq!(
            serde_json::to_value(done.state.unwrap()).unwrap()["units"],
            serde_json::json!([])
        );
        assert_eq!(until(completer.settle(at(710), QUIET)), at(730));
        settled(completer.settle(at(730), QUIET));
    }

    #[test]
    fn every_unit_change_is_activity() {
        let mut completer = Completer::default();
        completer.begin();
        completer.boundary(answered(), NOW);
        settled(completer.settle(at(30), QUIET));
        let resumed = completer.unit(opened("task"), at(40));
        let state = serde_json::to_value(resumed.state.unwrap()).unwrap();
        assert_eq!(state["last_activity_at"], at(40).to_string());
        completer.unit(
            UnitChange::Progressed {
                id: "task".to_owned(),
            },
            at(50),
        );
        completer.unit(
            UnitChange::Settled {
                id: "task".to_owned(),
            },
            at(60),
        );
        assert_eq!(until(completer.settle(at(61), QUIET)), at(90));
        completer.unit(
            UnitChange::Progressed {
                id: "task".to_owned(),
            },
            at(70),
        );
        assert_eq!(until(completer.settle(at(71), QUIET)), at(100));
    }

    #[test]
    fn a_change_to_a_unit_never_opened_or_already_settled_is_a_diagnostic() {
        let mut completer = Completer::default();
        completer.begin();
        completer.unit(opened("task"), NOW);
        assert_eq!(completer.unit(opened("task"), NOW).diagnostics.len(), 1);
        completer.unit(
            UnitChange::Settled {
                id: "task".to_owned(),
            },
            NOW,
        );
        for change in [
            UnitChange::Progressed {
                id: "task".to_owned(),
            },
            UnitChange::Settled {
                id: "task".to_owned(),
            },
        ] {
            let late = completer.unit(change, NOW);
            assert!(late.reports.is_empty());
            assert_eq!(late.diagnostics.len(), 1);
        }
    }

    #[test]
    fn a_unit_carries_over_into_the_next_turn_and_a_lost_harness_drops_it() {
        let mut completer = Completer::default();
        completer.begin();
        completer.unit(opened("task"), NOW);
        completer.boundary(answered(), NOW);
        completer.begin();
        assert_eq!(
            serde_json::to_value(completer.snapshot()).unwrap()["units"][0]["id"],
            "task"
        );
        let lost = completer.boundary(
            TurnOutcome::Failed {
                because: "the harness exited".to_owned(),
            },
            at(5),
        );
        assert!(lost.reports.is_empty());
        assert_eq!(
            serde_json::to_value(lost.state.unwrap()).unwrap()["units"],
            serde_json::json!([])
        );
    }

    #[test]
    fn a_cancelled_turn_does_not_trail() {
        let mut completer = Completer::default();
        completer.begin();
        let cancelled = completer.boundary(TurnOutcome::Cancelled, NOW);
        let state = serde_json::to_value(cancelled.state.unwrap()).unwrap();
        assert_eq!(state.get("last_activity_at"), None);
        assert_eq!(until(completer.settle(at(60), QUIET)), at(60) + QUIET);
    }

    #[test]
    fn a_tool_settles_once_with_its_input_result_and_times() {
        use agent_client_protocol::schema::v1::{
            ToolCall, ToolCallStatus, ToolCallUpdate, ToolCallUpdateFields,
        };
        let mut completer = Completer::default();
        let opened = completer.update(
            SessionUpdate::ToolCall(
                ToolCall::new("call", "read").raw_input(serde_json::json!({"path":"a"})),
            ),
            NOW,
        );
        assert!(opened.reports.is_empty());
        let later = NOW + jiff::SignedDuration::from_secs(2);
        let update = SessionUpdate::ToolCallUpdate(ToolCallUpdate::new(
            "call",
            ToolCallUpdateFields::new()
                .status(ToolCallStatus::Completed)
                .raw_output(serde_json::json!({"text":"result"})),
        ));
        let settled = completer.update(update.clone(), later);
        assert_eq!(settled.reports.len(), 1);
        let entry = serde_json::to_value(&settled.reports[0]).unwrap();
        assert_eq!(entry["kind"], "tool_call");
        assert_eq!(entry["input"], serde_json::json!({"path":"a"}));
        assert_eq!(
            entry["result"]["output"],
            serde_json::json!({"text":"result"})
        );
        assert_eq!(entry["completion"]["started_at"], NOW.to_string());
        assert_eq!(entry["completion"]["finished_at"], later.to_string());
        assert_eq!(completer.update(update, later).diagnostics.len(), 1);
    }

    #[test]
    fn an_idless_chunk_completes_immediately() {
        let mut completer = Completer::default();
        let reports = completer.update(SessionUpdate::AgentMessageChunk(chunk(None, "hello")), NOW);
        assert_eq!(reports.reports.len(), 1);
        assert_eq!(
            serde_json::to_value(&reports.reports[0]).unwrap()["message"],
            "hello"
        );
    }
    #[test]
    fn completed_ids_cannot_leak_into_the_next_turn() {
        let mut completer = Completer::default();
        completer.update(
            SessionUpdate::AgentMessageChunk(chunk(Some("one"), "first")),
            NOW,
        );
        completer.boundary(TurnOutcome::Cancelled, NOW);
        completer.begin();
        let late = completer.update(
            SessionUpdate::AgentMessageChunk(chunk(Some("one"), "late")),
            NOW,
        );
        assert!(late.reports.is_empty());
        assert_eq!(late.diagnostics.len(), 1);
        assert!(!completer.produced);
        assert!(
            completer
                .boundary(TurnOutcome::Cancelled, NOW)
                .reports
                .is_empty()
        );
    }
    #[test]
    fn messages_and_thoughts_complete_independently_around_interleaved_work() {
        use agent_client_protocol::schema::v1::{Plan, ToolCall};
        let mut completer = Completer::default();
        assert!(
            completer
                .update(
                    SessionUpdate::AgentMessageChunk(chunk(Some("message"), "half ")),
                    NOW
                )
                .reports
                .is_empty()
        );
        assert!(
            completer
                .update(
                    SessionUpdate::AgentThoughtChunk(chunk(Some("thought"), "think ")),
                    NOW
                )
                .reports
                .is_empty()
        );
        assert_eq!(
            completer
                .update(
                    SessionUpdate::ToolCall(
                        ToolCall::new("tool", "read").status(ToolCallStatus::Completed)
                    ),
                    NOW
                )
                .reports
                .len(),
            1
        );
        assert_eq!(
            completer
                .update(SessionUpdate::Plan(Plan::new(Vec::new())), NOW)
                .reports,
            vec![Report::Plan {
                entries: Vec::new(),
                completion: Completion::at(NOW)
            }]
        );
        assert!(
            completer
                .update(
                    SessionUpdate::AgentMessageChunk(chunk(Some("message"), "and half")),
                    NOW
                )
                .reports
                .is_empty()
        );
        assert!(
            completer
                .update(
                    SessionUpdate::AgentThoughtChunk(chunk(Some("thought"), "again")),
                    NOW
                )
                .reports
                .is_empty()
        );
        let later: Timestamp = "2026-09-29T12:00:02Z".parse().unwrap();
        let message = completer.update(
            SessionUpdate::AgentMessageChunk(chunk(Some("next"), "next")),
            later,
        );
        assert_eq!(
            message.reports,
            vec![Report::Said {
                message: "half and half".to_owned(),
                completion: Completion {
                    started_at: NOW,
                    finished_at: later,
                    turn_outcome: None
                }
            }]
        );
        let outcome = TurnOutcome::Answered {
            stop_reason: "end_turn".to_owned(),
        };
        let boundary = completer.boundary(outcome.clone(), later);
        assert_eq!(
            boundary.reports,
            vec![
                Report::Said {
                    message: "next".to_owned(),
                    completion: Completion {
                        started_at: later,
                        finished_at: later,
                        turn_outcome: Some(outcome.clone())
                    }
                },
                Report::Thought {
                    text: "think again".to_owned(),
                    completion: Completion {
                        started_at: NOW,
                        finished_at: later,
                        turn_outcome: Some(outcome)
                    }
                },
            ]
        );
    }

    #[test]
    fn failed_and_cancelled_boundaries_keep_observed_text_once() {
        for outcome in [
            TurnOutcome::Cancelled,
            TurnOutcome::Failed {
                because: "lost".to_owned(),
            },
        ] {
            let mut completer = Completer::default();
            completer.update(
                SessionUpdate::AgentMessageChunk(chunk(Some("message"), "observed")),
                NOW,
            );
            completer.update(
                SessionUpdate::AgentThoughtChunk(chunk(Some("thought"), "reasoning")),
                NOW,
            );
            let completed = completer.boundary(outcome.clone(), NOW);
            assert_eq!(completed.reports.len(), 2);
            for report in completed.reports {
                assert_eq!(
                    serde_json::to_value(report).unwrap()["completion"]["turn_outcome"],
                    serde_json::to_value(&outcome).unwrap()
                );
            }
            assert!(completer.boundary(outcome.clone(), NOW).reports.is_empty());
            let late = completer.update(
                SessionUpdate::AgentThoughtChunk(chunk(Some("thought"), "late")),
                NOW,
            );
            assert!(late.reports.is_empty());
            assert_eq!(late.diagnostics.len(), 1);
        }
    }

    #[test]
    fn bookkeeping_does_not_make_a_turn_produced() {
        use agent_client_protocol::schema::v1::{
            AvailableCommandsUpdate, ConfigOptionUpdate, CurrentModeUpdate, SessionModeId,
            UsageUpdate,
        };
        let mut completer = Completer::default();
        for update in [
            SessionUpdate::UsageUpdate(UsageUpdate::new(12, 100)),
            SessionUpdate::AvailableCommandsUpdate(AvailableCommandsUpdate::new(Vec::new())),
            SessionUpdate::CurrentModeUpdate(CurrentModeUpdate::new(SessionModeId::new("build"))),
            SessionUpdate::ConfigOptionUpdate(ConfigOptionUpdate::new(Vec::new())),
        ] {
            completer.update(update, NOW);
        }
        assert!(!completer.produced);
    }

    #[test]
    fn each_plan_replacement_and_idless_thought_is_one_completed_unit() {
        use agent_client_protocol::schema::v1::{
            Plan, PlanEntry as AcpPlanEntry, PlanEntryPriority, PlanEntryStatus,
        };
        let mut completer = Completer::default();
        for status in [PlanEntryStatus::Pending, PlanEntryStatus::Completed] {
            let snapshot = completer.update(
                SessionUpdate::Plan(Plan::new(vec![AcpPlanEntry::new(
                    "read",
                    PlanEntryPriority::High,
                    status,
                )])),
                NOW,
            );
            assert_eq!(snapshot.reports.len(), 1);
            assert!(
                matches!(&snapshot.reports[0], Report::Plan { entries, completion } if entries.len() == 1 && completion.turn_outcome.is_none())
            );
        }
        for text in ["first thought", "second thought"] {
            assert_eq!(
                completer
                    .update(SessionUpdate::AgentThoughtChunk(chunk(None, text)), NOW)
                    .reports,
                vec![Report::Thought {
                    text: text.to_owned(),
                    completion: Completion::at(NOW)
                }]
            );
        }
        assert!(completer.produced);
    }
}
