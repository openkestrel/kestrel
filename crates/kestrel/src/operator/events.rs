use axum::Json;
use axum::extract::{Path, Query, State};
use kestrel_operator_types as wire;
use serde::Deserialize;

use crate::domain::{self, EventRecordId};
use crate::integration;
use crate::store::Store;
use crate::trigger;

use super::{ControlPlane, EVENTS_LISTED, Refused, no_event};

#[derive(Deserialize)]
pub(super) struct Limited {
    limit: Option<usize>,
}

pub(super) async fn event_record(
    store: &Store,
    event: domain::Event,
) -> Result<wire::EventRecord, Refused> {
    let mut firings = Vec::new();
    for firing in trigger::firings(store, event.record_id).await? {
        firings.push(wire::Firing {
            outcome: match firing.outcome.as_str() {
                "opened" => wire::FiringOutcome::Opened,
                "fed" => wire::FiringOutcome::Fed,
                "ignored" => wire::FiringOutcome::Ignored,
                "failed" => wire::FiringOutcome::Failed,
                "held" => wire::FiringOutcome::Held,
                "canceled" => wire::FiringOutcome::Canceled,
                outcome => return Err(anyhow::anyhow!("a firing ended {outcome}").into()),
            },
            trigger: firing.trigger,
            workspace: firing.workspace.map(|workspace| wire_id(&workspace)),
            failure: firing.failure,
            worked_ahead: firing.worked_ahead,
        });
    }

    Ok(wire::EventRecord {
        record: wire_id(&event.record_id),
        organization: wire_id(&event.organization),
        integration: event.integration.map(|integration| wire_id(&integration)),
        recorded_at: event.recorded_at.to_string(),
        event: wire::CloudEvent {
            id: event.occurrence.id,
            source: event.occurrence.source,
            specversion: event.occurrence.specversion,
            r#type: event.occurrence.r#type,
            subject: event.occurrence.subject,
            time: event.occurrence.time.to_string(),
            data: event.occurrence.data,
        },
        firings,
    })
}

pub(super) fn wire_id(id: &impl ToString) -> uuid::Uuid {
    id.to_string().parse().expect("an identifier is a UUID")
}

pub(super) async fn events(
    State(control_plane): State<ControlPlane>,
    Path(organization): Path<String>,
    Query(limited): Query<Limited>,
) -> Result<Json<wire::ListEventsResponse>, Refused> {
    let events = integration::events(
        &control_plane.store,
        &organization,
        limited.limit.unwrap_or(EVENTS_LISTED),
    )
    .await?;
    let mut records = Vec::new();
    for event in events {
        records.push(event_record(&control_plane.store, event).await?);
    }

    Ok(Json(records))
}

pub(super) async fn event(
    State(control_plane): State<ControlPlane>,
    Path(record): Path<String>,
) -> Result<Json<wire::EventRecord>, Refused> {
    let record: EventRecordId = record.parse().map_err(|_| no_event(&record, None))?;
    let event = integration::event(&control_plane.store, record).await?;

    Ok(Json(event_record(&control_plane.store, event).await?))
}
