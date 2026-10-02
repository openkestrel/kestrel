use jiff::Timestamp;
use std::collections::{BTreeMap, BTreeSet};

use agent_client_protocol::schema::v1::{
    ContentBlock, ContentChunk, SessionUpdate, ToolCall, ToolCallStatus,
};

use crate::link::{
    ClosingReason, Completion, Cost, PlanEntry, Report, ToolStatus, TurnOutcome, Usage,
};

#[derive(Default)]
pub struct Completer {
    messages: Stream,
    thoughts: Stream,
    tools: BTreeMap<String, (ToolCall, Timestamp)>,
    completed_tools: BTreeSet<String>,
    pub produced: bool,
    ended: bool,
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
        self.ended = false;
    }

    pub fn update(&mut self, update: SessionUpdate, now: Timestamp) -> Completed {
        let mut completed = Completed::default();
        let before = self.snapshot();
        match update {
            SessionUpdate::AgentMessageChunk(chunk) => {
                if self.ended {
                    completed
                        .diagnostics
                        .push("kestrel: a message arrived after its turn ended".to_owned());
                } else {
                    self.produced |= self.messages.chunk(chunk, now, false, &mut completed);
                }
            }
            SessionUpdate::AgentThoughtChunk(chunk) => {
                if self.ended {
                    completed
                        .diagnostics
                        .push("kestrel: a thought arrived after its turn ended".to_owned());
                } else {
                    self.produced |= self.thoughts.chunk(chunk, now, true, &mut completed);
                }
            }
            SessionUpdate::Plan(plan) => {
                if self.ended {
                    completed
                        .diagnostics
                        .push("kestrel: a plan arrived after its turn ended".to_owned());
                } else {
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
                let id = call.tool_call_id.0.to_string();
                if self.ended || self.completed_tools.contains(&id) || self.tools.contains_key(&id)
                {
                    completed.diagnostics.push(format!(
                        "kestrel: a late or duplicate tool start arrived for {id}"
                    ));
                } else {
                    self.produced = true;
                    self.tools.insert(id.clone(), (call, now));
                    self.settle(&id, now, None, &mut completed);
                }
            }
            SessionUpdate::ToolCallUpdate(update) => {
                let id = update.tool_call_id.0.to_string();
                if let Some((call, _)) = self.tools.get_mut(&id) {
                    call.update(update.fields);
                    self.settle(&id, now, None, &mut completed);
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
        let after = self.snapshot();
        if before != after {
            completed.state = Some(after);
        }
        completed
    }

    pub fn boundary(&mut self, outcome: TurnOutcome, now: Timestamp) -> Completed {
        let mut completed = Completed::default();
        self.messages
            .close(now, false, Some(outcome.clone()), &mut completed);
        self.thoughts
            .close(now, true, Some(outcome.clone()), &mut completed);
        for id in self.tools.keys().cloned().collect::<Vec<_>>() {
            self.settle(&id, now, Some(outcome.clone()), &mut completed);
        }
        completed.state = Some(self.snapshot());
        self.ended = true;
        completed
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
            message_buffering: self.messages.open.is_some(),
            thought_buffering: self.thoughts.open.is_some(),
            // Filled in when the state goes up the link, which is what holds the latest usage.
            usage: None,
        }
    }

    fn settle(
        &mut self,
        id: &str,
        now: Timestamp,
        outcome: Option<TurnOutcome>,
        completed: &mut Completed,
    ) {
        let Some((call, _)) = self.tools.get(id) else {
            return;
        };
        if outcome.is_none()
            && !matches!(
                call.status,
                ToolCallStatus::Completed | ToolCallStatus::Failed
            )
        {
            return;
        }
        let (call, started_at) = self.tools.remove(id).unwrap();
        self.completed_tools.insert(id.to_owned());
        let closing_reason = outcome.as_ref().map(|outcome| match outcome {
            TurnOutcome::Cancelled => ClosingReason::Interrupted,
            TurnOutcome::Failed { .. } => ClosingReason::Failed,
            TurnOutcome::Answered { .. } => ClosingReason::Unresolved,
        });
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
    fn open_tools_close_once_at_each_turn_boundary_and_late_updates_are_diagnostics() {
        use agent_client_protocol::schema::v1::{ToolCallUpdate, ToolCallUpdateFields};
        for (outcome, reason) in [
            (TurnOutcome::Cancelled, "interrupted"),
            (
                TurnOutcome::Failed {
                    because: "lost".to_owned(),
                },
                "failed",
            ),
            (
                TurnOutcome::Answered {
                    stop_reason: "end_turn".to_owned(),
                },
                "unresolved",
            ),
        ] {
            let mut completer = Completer::default();
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
