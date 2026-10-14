use axum::Json;
use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, State};
use kestrel_operator_types as wire;

use crate::declined::Reason;
use crate::domain::{self, WorkspaceId};
use crate::instance;
use crate::scheduling;

use super::{ControlPlane, Refused, diagnosis, resolved, wire_id};

pub(super) fn queue_standing(
    snapshot: &scheduling::Snapshot,
    workspace: WorkspaceId,
) -> Option<wire::QueueStanding> {
    if let Some(queued) = snapshot
        .queued
        .iter()
        .find(|queued| queued.session.workspace == workspace)
    {
        return Some(wire::QueueStanding {
            position: queued.position.map(|position| position as i64),
            reasons: reasons(queued.reasons.clone()),
            pending_since: None,
        });
    }
    if let Some(waiting) = snapshot
        .waiting
        .iter()
        .find(|waiting| waiting.session.workspace == workspace)
    {
        return Some(wire::QueueStanding {
            position: waiting.position.map(|position| position as i64),
            reasons: reasons(waiting.reasons.clone()),
            pending_since: waiting.pending_since.map(|since| since.to_string()),
        });
    }
    snapshot
        .unbriefed
        .iter()
        .find(|unbriefed| unbriefed.session.workspace == workspace)
        .map(|unbriefed| wire::QueueStanding {
            position: unbriefed.position.map(|position| position as i64),
            reasons: reasons(unbriefed.reasons.clone()),
            pending_since: unbriefed
                .brief_since
                .or(unbriefed.pending_since)
                .map(|since| since.to_string()),
        })
}

pub(super) async fn instances(
    State(control_plane): State<ControlPlane>,
    Path(organization): Path<String>,
) -> Result<Json<Vec<wire::HeldInstance>>, Refused> {
    let held = instance::held(&control_plane.store, &organization).await?;

    Ok(Json(
        held.into_iter()
            .map(|held| wire::HeldInstance {
                workspace: wire_id(&held.workspace),
                instance: held.instance,
                because: held.because,
            })
            .collect(),
    ))
}

pub(super) fn queue_reason(reason: scheduling::Reason) -> wire::QueueReason {
    match reason {
        scheduling::Reason::Dependencies(sessions) => {
            wire::QueueReason::QueueReasonDependencies(wire::QueueReasonDependencies {
                kind: serde_json::json!("dependencies"),
                sessions,
            })
        }
        scheduling::Reason::SubscriptionProfile(busy) => {
            let profile = busy.profile.clone();
            let session = busy.holder.session().map(str::to_owned);
            let (_, diagnostic) = diagnosis(Reason::SignInBusy(busy), None);
            wire::QueueReason::QueueReasonSubscriptionProfile(
                wire::QueueReasonSubscriptionProfile {
                    profile,
                    session,
                    diagnostic,
                    kind: serde_json::json!("subscription_profile"),
                },
            )
        }
        scheduling::Reason::InstanceArchiving(instance) => {
            wire::QueueReason::QueueReasonInstanceArchiving(wire::QueueReasonInstanceArchiving {
                kind: serde_json::json!("instance_archiving"),
                instance,
            })
        }
        scheduling::Reason::LiveInstanceLimit(limit) => {
            wire::QueueReason::QueueReasonLiveInstanceLimit(wire::QueueReasonLiveInstanceLimit {
                kind: serde_json::json!("live_instance_limit"),
                limit: limit as i64,
            })
        }
        scheduling::Reason::ActiveWorkSlots(limit) => {
            wire::QueueReason::QueueReasonActiveWorkSlots(wire::QueueReasonActiveWorkSlots {
                kind: serde_json::json!("active_work_slots"),
                limit: limit as i64,
            })
        }
        scheduling::Reason::Ahead {
            sessions,
            elsewhere,
        } => wire::QueueReason::QueueReasonAhead(wire::QueueReasonAhead {
            sessions,
            elsewhere: (elsewhere != 0).then_some(elsewhere as i64),
            kind: serde_json::json!("ahead"),
        }),
    }
}

pub(super) fn reasons(reasons: Vec<scheduling::Reason>) -> Vec<wire::QueueReason> {
    reasons.into_iter().map(queue_reason).collect()
}

pub(super) fn queue_record(snapshot: scheduling::Snapshot) -> wire::Queue {
    let recorded = snapshot.recorded.map(|recorded| wire::QueueWorkRole {
        active_work_slots: recorded.active_work_slots as i64,
        serialized_harnesses: recorded.serialized_harnesses,
        driver: recorded.driver,
    });

    wire::Queue {
        work_role: recorded,
        active_work: wire::ActiveWork {
            limit: snapshot.active_work.limit.map(|limit| limit as i64),
            occupied: snapshot.active_work.occupied as i64,
            occupants: snapshot
                .active_work
                .occupants
                .into_iter()
                .map(|session| wire::Occupant {
                    name: session.name,
                    workspace: wire_id(&session.workspace),
                    agent: session.agent.name,
                    phase: match session.state {
                        domain::SessionState::Trailing => wire::OccupantPhase::Trailing,
                        _ => wire::OccupantPhase::Working,
                    },
                    enqueued_at: session.enqueued_at.to_string(),
                })
                .collect(),
            elsewhere: snapshot.active_work.elsewhere as i64,
        },
        instances: wire::QueueInstances {
            limit: snapshot.instances.limit.map(|limit| limit as i64),
            count: snapshot.instances.count as i64,
            counted: snapshot.instances.counted,
        },
        queued: snapshot
            .queued
            .into_iter()
            .map(|queued| wire::QueuedSession {
                position: queued.position.map(|position| position as i64),
                name: queued.session.name,
                workspace: wire_id(&queued.session.workspace),
                agent: queued.session.agent.name,
                reasons: reasons(queued.reasons),
                enqueued_at: queued.session.enqueued_at.to_string(),
            })
            .collect(),
        waiting: snapshot
            .waiting
            .into_iter()
            .map(|waiting| wire::WaitingSession {
                position: waiting.position.map(|position| position as i64),
                name: waiting.session.name,
                workspace: wire_id(&waiting.session.workspace),
                agent: waiting.session.agent.name,
                pending_since: waiting.pending_since.map(|since| since.to_string()),
                reasons: reasons(waiting.reasons),
                enqueued_at: waiting.session.enqueued_at.to_string(),
            })
            .collect(),
        unbriefed: snapshot
            .unbriefed
            .into_iter()
            .map(|unbriefed| wire::UnbriefedSession {
                position: unbriefed.position.map(|position| position as i64),
                brief_since: unbriefed.brief_since.map(|since| since.to_string()),
                reasons: reasons(unbriefed.reasons),
                name: unbriefed.session.name,
                workspace: wire_id(&unbriefed.session.workspace),
                agent: unbriefed.session.agent.name,
                preparing: unbriefed
                    .session
                    .preparing
                    .map(|preparing| match preparing {
                        domain::Preparing::Provisioning => {
                            wire::UnbriefedSessionPreparing::Provisioning
                        }
                        domain::Preparing::Cloning => wire::UnbriefedSessionPreparing::Cloning,
                        domain::Preparing::StartingHarness => {
                            wire::UnbriefedSessionPreparing::StartingHarness
                        }
                        domain::Preparing::HarnessReady => {
                            wire::UnbriefedSessionPreparing::HarnessReady
                        }
                    }),
                pending_since: unbriefed.pending_since.map(|since| since.to_string()),
                enqueued_at: unbriefed.session.enqueued_at.to_string(),
            })
            .collect(),
    }
}

pub(super) async fn show_queue(
    State(control_plane): State<ControlPlane>,
    Path(organization): Path<String>,
) -> Result<Json<wire::Queue>, Refused> {
    let snapshot = scheduling::snapshot(&control_plane.store, &organization).await?;

    Ok(Json(queue_record(snapshot)))
}

pub(super) async fn release_instance(
    State(control_plane): State<ControlPlane>,
    Path((organization, workspace)): Path<(String, String)>,
    release: Result<Json<wire::InstanceRelease>, JsonRejection>,
) -> Result<Json<wire::ReleasedInstance>, Refused> {
    let Json(release) = release?;
    let workspace = resolved(&control_plane, &organization, &workspace).await?;
    let instance = instance::release(
        &control_plane.store,
        workspace.id,
        release
            .participant
            .as_ref()
            .and_then(|participant| participant.as_deref()),
    )
    .await?;

    Ok(Json(wire::ReleasedInstance { instance }))
}
