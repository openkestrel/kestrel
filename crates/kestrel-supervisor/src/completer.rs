use jiff::Timestamp;
use std::collections::BTreeSet;

use agent_client_protocol::schema::v1::{ContentBlock, ContentChunk, SessionUpdate};

use crate::link::{Completion, Cost, PlanEntry, Report, TurnOutcome, Usage};

#[derive(Default)]
pub struct Completer {
    messages: Stream,
    thoughts: Stream,
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
}

impl Completer {
    pub fn begin(&mut self) {
        self.produced = false;
        self.ended = false;
    }

    pub fn update(&mut self, update: SessionUpdate, now: Timestamp) -> Completed {
        let mut completed = Completed::default();
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
            SessionUpdate::ToolCall(_) | SessionUpdate::ToolCallUpdate(_) => self.produced = true,
            SessionUpdate::UsageUpdate(usage) => completed.reports.push(Report::Used {
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
        completed
    }

    pub fn boundary(&mut self, outcome: TurnOutcome, now: Timestamp) -> Completed {
        let mut completed = Completed::default();
        self.messages
            .close(now, false, Some(outcome.clone()), &mut completed);
        self.thoughts
            .close(now, true, Some(outcome), &mut completed);
        self.ended = true;
        completed
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
        assert!(
            completer
                .update(SessionUpdate::ToolCall(ToolCall::new("tool", "read")), NOW)
                .reports
                .is_empty()
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
