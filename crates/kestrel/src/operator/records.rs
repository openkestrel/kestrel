use kestrel_operator_types as wire;

use crate::domain::{self, HeldMessage, Session, StartedBy, Workspace};
use crate::store::Store;
use crate::{instance, pull_request, work, workspace};

use super::{diagnosed_failure, selected, shown_option, supplied_material, wire_id};

pub(super) fn session_state(value: domain::SessionState) -> wire::SessionState {
    match value {
        domain::SessionState::Queued => wire::SessionState::Queued,
        domain::SessionState::Working => wire::SessionState::Working,
        domain::SessionState::Trailing => wire::SessionState::Trailing,
        domain::SessionState::Waiting => wire::SessionState::Waiting,
        domain::SessionState::Unbriefed => wire::SessionState::Unbriefed,
        domain::SessionState::Ended => wire::SessionState::Ended,
        domain::SessionState::Unreachable => wire::SessionState::Unreachable,
    }
}

pub(super) fn unfinished_state(value: domain::SessionState) -> wire::UnfinishedSessionState {
    match value {
        domain::SessionState::Queued => wire::UnfinishedSessionState::Queued,
        domain::SessionState::Working => wire::UnfinishedSessionState::Working,
        domain::SessionState::Trailing => wire::UnfinishedSessionState::Trailing,
        domain::SessionState::Waiting => wire::UnfinishedSessionState::Waiting,
        domain::SessionState::Unbriefed => wire::UnfinishedSessionState::Unbriefed,
        domain::SessionState::Ended => wire::UnfinishedSessionState::Ended,
        domain::SessionState::Unreachable => wire::UnfinishedSessionState::Unreachable,
    }
}

pub(super) fn session_preparing(value: domain::Preparing) -> wire::SessionPreparing {
    match value {
        domain::Preparing::Provisioning => wire::SessionPreparing::Provisioning,
        domain::Preparing::Cloning => wire::SessionPreparing::Cloning,
        domain::Preparing::StartingHarness => wire::SessionPreparing::StartingHarness,
        domain::Preparing::HarnessReady => wire::SessionPreparing::HarnessReady,
    }
}

pub(super) fn unfinished_preparing(value: domain::Preparing) -> wire::UnfinishedSessionPreparing {
    match value {
        domain::Preparing::Provisioning => wire::UnfinishedSessionPreparing::Provisioning,
        domain::Preparing::Cloning => wire::UnfinishedSessionPreparing::Cloning,
        domain::Preparing::StartingHarness => wire::UnfinishedSessionPreparing::StartingHarness,
        domain::Preparing::HarnessReady => wire::UnfinishedSessionPreparing::HarnessReady,
    }
}

pub(crate) fn held_message(message: HeldMessage) -> wire::HeldMessage {
    wire::HeldMessage {
        id: message.id,
        participant: message.participant,
        message: message.message,
        posted_at: message.posted_at.to_string(),
        edited_at: message.edited_at.map(|at| at.to_string()),
    }
}

pub(super) fn pull_request_availability(
    value: pull_request::Availability,
) -> wire::PullRequestAvailability {
    wire::PullRequestAvailability {
        repository: value.repository,
        availability: if value.known.is_some() {
            wire::PullRequestAvailabilityAvailability::Available
        } else {
            wire::PullRequestAvailabilityAvailability::Unavailable
        },
        known: value.known.map(|known| {
            known
                .into_iter()
                .map(|pr| wire::PullRequest {
                    repository: pr.repository,
                    number: pr.number,
                    url: pr.url,
                    title: pr.title,
                    state: match pr.state {
                        domain::PullRequestState::Open => wire::PullRequestState::Open,
                        domain::PullRequestState::Closed => wire::PullRequestState::Closed,
                        domain::PullRequestState::Merged => wire::PullRequestState::Merged,
                    },
                    head_branch: pr.head_branch,
                    head_revision: pr.head_revision,
                    updated_at: pr.updated_at.to_string(),
                    event: wire_id(&pr.event),
                })
                .collect()
        }),
    }
}

pub(crate) fn session_exit(exit: domain::Exit) -> wire::Exit {
    match exit {
        domain::Exit::Succeeded => wire::Exit {
            status: wire::ExitStatus::Succeeded,
            because: None,
        },
        domain::Exit::Failed { because } => wire::Exit {
            status: wire::ExitStatus::Failed,
            because: Some(because),
        },
    }
}

pub(crate) fn session_usage(usage: domain::Usage) -> wire::Usage {
    wire::Usage {
        context_used: usage
            .context_used
            .try_into()
            .expect("context usage fits a wire integer"),
        context_size: usage
            .context_size
            .try_into()
            .expect("context size fits a wire integer"),
        cost: usage.cost.map(|cost| wire::Cost {
            amount: cost.amount,
            currency: cost.currency,
        }),
    }
}

pub(crate) fn running_tool(tool: crate::live_work::RunningTool) -> wire::RunningTool {
    wire::RunningTool {
        call_id: tool.call_id,
        title: tool.title,
        tool_kind: tool.tool_kind,
        status: match tool.status {
            crate::log::ToolStatus::Pending => "pending",
            crate::log::ToolStatus::InProgress => "in_progress",
            crate::log::ToolStatus::Completed => "completed",
            crate::log::ToolStatus::Failed => "failed",
        }
        .to_owned(),
        started_at: tool.started_at.to_string(),
    }
}

pub(crate) fn running_unit(unit: crate::live_work::RunningUnit) -> wire::RunningUnit {
    wire::RunningUnit {
        id: unit.id,
        title: unit.title,
        started_at: unit.started_at.to_string(),
        kind: match unit.kind {
            crate::live_work::UnitKind::BackgroundTask => wire::RunningUnitKind::BackgroundTask,
            crate::live_work::UnitKind::Subagent => wire::RunningUnitKind::Subagent,
        },
    }
}

pub(crate) fn session_observation(
    observation: crate::live_work::Observation,
) -> wire::SessionObservation {
    match observation {
        crate::live_work::Observation::Current { observed_at } => {
            wire::SessionObservation::SessionObservationCurrent(wire::SessionObservationCurrent {
                availability: "current".into(),
                observed_at: observed_at.to_string(),
            })
        }
        crate::live_work::Observation::Unavailable { last } => {
            wire::SessionObservation::SessionObservationUnavailable(
                wire::SessionObservationUnavailable {
                    availability: "unavailable".into(),
                    last: last.map(|last| wire::LastObservation {
                        observed_at: last.observed_at.to_string(),
                        tools: last.tools.into_iter().map(running_tool).collect(),
                        units: last.units.into_iter().map(running_unit).collect(),
                    }),
                },
            )
        }
    }
}

pub(crate) fn workspace_work(work: crate::live_work::Work) -> wire::WorkspaceWork {
    let crate::live_work::Work {
        current,
        last_report,
        earlier_reports,
    } = work;
    let earlier = work_earlier_reports(earlier_reports);
    match current {
        crate::live_work::Current::Reported { summary } => {
            wire::WorkspaceWork::WorkReported(wire::WorkReported {
                state: serde_json::json!("reported"),
                repositories: summary
                    .repositories
                    .into_iter()
                    .map(work_repository)
                    .collect(),
                reported_at: summary.reported_at.to_string(),
                last_report: work_last_report(last_report),
                earlier_reports: earlier,
            })
        }
        crate::live_work::Current::NoInstance {
            branch,
            pull_request,
        } => wire::WorkspaceWork::WorkNoInstance(wire::WorkNoInstance {
            state: serde_json::json!("no_instance"),
            branch,
            pull_request,
            last_report: work_last_report(last_report),
            earlier_reports: earlier,
        }),
        crate::live_work::Current::NotAnswering { message } => {
            wire::WorkspaceWork::WorkNotAnswering(wire::WorkNotAnswering {
                state: serde_json::json!("not_answering"),
                message: message.to_owned(),
                last_report: work_last_report(last_report),
                earlier_reports: earlier,
            })
        }
    }
}

pub(super) fn work_last_report(report: crate::live_work::LastReport) -> wire::WorkLastReport {
    match report {
        crate::live_work::LastReport::None => {
            wire::WorkLastReport::WorkNoReport(wire::WorkNoReport {
                report: serde_json::json!("none"),
            })
        }
        crate::live_work::LastReport::Received {
            instance,
            current_instance,
            summary,
        } => wire::WorkLastReport::WorkInstanceReport(work_instance_report(
            instance,
            current_instance,
            summary,
        )),
    }
}

/// Every earlier report is one an Instance sent: `live_work::read` collects them from stored rows.
pub(super) fn work_earlier_reports(
    reports: Vec<crate::live_work::LastReport>,
) -> Vec<wire::WorkInstanceReport> {
    reports
        .into_iter()
        .filter_map(|report| match report {
            crate::live_work::LastReport::Received {
                instance,
                current_instance,
                summary,
            } => Some(work_instance_report(instance, current_instance, summary)),
            crate::live_work::LastReport::None => None,
        })
        .collect()
}

pub(super) fn work_instance_report(
    instance: String,
    current_instance: bool,
    summary: crate::live_work::Summary,
) -> wire::WorkInstanceReport {
    wire::WorkInstanceReport {
        report: serde_json::json!("received"),
        instance,
        current_instance,
        repositories: summary
            .repositories
            .into_iter()
            .map(work_repository)
            .collect(),
        reported_at: summary.reported_at.to_string(),
    }
}

pub(super) fn work_repository(repository: crate::live_work::Repository) -> wire::WorkRepository {
    let crate::live_work::Repository { repository, git } = repository;
    match git {
        crate::live_work::Git::Read {
            branch,
            changed,
            staged,
            committed,
            pushed,
            untracked,
            stashed,
        } => wire::WorkRepository::WorkRepositoryRead(wire::WorkRepositoryRead {
            repository,
            git: serde_json::json!("read"),
            branch,
            changed: work_changes(changed),
            staged: work_changes(staged),
            committed: work_commits(committed),
            pushed,
            untracked: wire_count(untracked),
            stashed: wire_count(stashed),
        }),
        crate::live_work::Git::Unreadable { because } => {
            wire::WorkRepository::WorkRepositoryUnreadable(wire::WorkRepositoryUnreadable {
                repository,
                git: serde_json::json!("unreadable"),
                because,
            })
        }
    }
}

pub(super) fn work_changes(changes: crate::live_work::Changes) -> wire::WorkChanges {
    wire::WorkChanges {
        files: wire_count(changes.files),
        added: wire_count(changes.added),
        removed: wire_count(changes.removed),
    }
}

pub(super) fn work_commits(commits: crate::live_work::Commits) -> wire::WorkCommits {
    wire::WorkCommits {
        commits: wire_count(commits.commits),
        added: wire_count(commits.added),
        removed: wire_count(commits.removed),
    }
}

pub(super) fn wire_count(counted: u64) -> i64 {
    counted.try_into().expect("a count fits a wire integer")
}

pub(crate) async fn workspace_record(
    store: &Store,
    workspace: Workspace,
) -> anyhow::Result<wire::Workspace> {
    let continued_by = workspace::continuations(store, workspace.id)
        .await?
        .into_iter()
        .map(|workspace| workspace.to_string())
        .collect();
    let instance = work::instance(store, workspace.id).await?;
    let held = instance::held_by(store, workspace.id)
        .await?
        .map(|held| held.because);
    let pull_requests = pull_request::availability(store, &workspace)
        .await?
        .into_iter()
        .map(pull_request_availability)
        .collect();
    let unfinished_session = workspace::unfinished(store, workspace.id)
        .await?
        .map(|session| wire::UnfinishedSession {
            id: wire_id(&session.id),
            name: session.name,
            state: unfinished_state(session.state),
            preparing: session.preparing.map(unfinished_preparing),
        });
    let held_messages = workspace::held_messages(store, workspace.id)
        .await?
        .into_iter()
        .map(held_message)
        .collect();

    Ok(wire::Workspace {
        id: wire_id(&workspace.id),
        name: workspace.name,
        organization: workspace.organization.name,
        project: workspace.project.name,
        opened_with: workspace.opened_with.name,
        profile: workspace.profile.map(|profile| profile.name),
        checkout: wire::Checkout {
            base: workspace.checkout.base,
            branch: workspace.checkout.branch,
            repositories: workspace.checkout.repositories,
        },
        integrations: Some(selected(workspace.integrations)),
        instance,
        held,
        correlation: workspace.correlation,
        state: match workspace.state {
            domain::WorkspaceState::Open => wire::WorkspaceState::Open,
            domain::WorkspaceState::Sealed => wire::WorkspaceState::Sealed,
        },
        opened_at: workspace.opened_at.to_string(),
        last_active_at: workspace.last_active_at.to_string(),
        sealed_at: workspace.sealed_at.map(|at| at.to_string()),
        continues: workspace.continues.map(|workspace| wire_id(&workspace)),
        started_by: workspace.started_by.map(|started| match started {
            StartedBy::Event(event) => event.to_string(),
            StartedBy::Participant(participant) => participant,
        }),
        continued_by,
        held_messages,
        pull_requests,
        unfinished_session,
    })
}

pub(crate) fn session_record(
    session: Session,
    organization: &str,
    summaries: &crate::live_work::Summaries,
) -> wire::Session {
    let (observation, state) = summaries.observed(Some(&session));
    let trailing = session.state == domain::SessionState::Trailing;
    let harness = session.agent.harness.clone();
    let diagnostic = session
        .evidence
        .clone()
        .map(|evidence| diagnosed_failure(&session, organization, evidence));
    let supplied = session
        .supplied
        .iter()
        .map(|supplied| supplied_material(&harness, supplied))
        .collect();
    let options = session
        .options
        .into_iter()
        .map(|option| shown_option(option, &harness))
        .collect();
    wire::Session {
        id: wire_id(&session.id),
        name: session.name,
        workspace: wire_id(&session.workspace),
        state: session_state(session.state),
        preparing: session.preparing.map(session_preparing),
        exit: session.exit.map(session_exit),
        outcome_message: session.outcome_message,
        diagnostic,
        instance: session.instance,
        supervisor: session.supervisor,
        agent: session.agent.name,
        harness,
        model: session.agent.declared.model,
        mode: session.agent.declared.mode,
        thought_level: session.agent.declared.thought_level,
        worked_model: session.worked_model,
        sign_in_method: session.sign_in_method,
        supplied,
        title: session.title,
        options,
        changing_options: session
            .changing_options
            .into_iter()
            .map(|change| wire::ChangingOption {
                category: change.category,
                option: change.option,
                participant: change.participant,
                value: change.value,
            })
            .collect(),
        commands: session
            .commands
            .into_iter()
            .map(|command| wire::SessionCommand {
                name: command.name,
                description: command.description,
                input_hint: command.input_hint,
            })
            .collect(),
        interrupting: session.interrupting.map(|interrupt| wire::Interrupting {
            participant: interrupt.participant,
            requested_at: interrupt.requested_at.to_string(),
        }),
        enqueued_at: session.enqueued_at.to_string(),
        started_at: session.started_at.map(|at| at.to_string()),
        ended_at: session.ended_at.map(|at| at.to_string()),
        lease_expires_at: session.lease_expires_at.map(|at| at.to_string()),
        connected_at: session
            .connected
            .as_ref()
            .map(|connected| connected.at.to_string()),
        supervisor_version: session.connected.map(|connected| connected.version),
        // The live figure is the Turn in flight; the recorded one is the last Turn answered.
        usage: state.usage.or(session.usage).map(session_usage),
        depends_on: session
            .depends_on
            .into_iter()
            .map(|blocker| wire::SessionReference {
                id: blocker.id.to_string(),
                name: blocker.name,
            })
            .collect(),
        tools: state.tools.into_iter().map(running_tool).collect(),
        units: state.units.into_iter().map(running_unit).collect(),
        message_buffering: state.message_buffering,
        thought_buffering: state.thought_buffering,
        last_activity_at: state
            .last_activity_at
            .filter(|_| trailing)
            .map(|at| at.to_string()),
        observation: session_observation(observation),
    }
}
