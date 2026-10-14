use std::collections::HashMap;

use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::{BoxError, Json};
use futures_core::Stream;
use kestrel_operator_types as wire;
use tokio_stream::{StreamExt as _, StreamMap};
use tracing::warn;

use crate::declined::{self as declined, Locator, Reason, Resource};
use crate::domain::{self, Workspace, WorkspaceId, WorkspaceState};
use crate::fanout;
use crate::log::{self, Cursor, Window};
use crate::participant;
use crate::store::Store;
use crate::stream::{self, Emitted};

use super::{
    ControlPlane, Following, KEEP_ALIVE, POLL, Refused, diagnosed, malformed, resolved,
    running_tool, running_unit, session_observation, session_usage, wire_id,
};

// The Workspace's name, because a Client addresses every read by it.
pub(super) async fn changed(
    store: &Store,
    resource: fanout::Resource,
) -> anyhow::Result<wire::Change> {
    let mut tx = store.read().await?;
    Ok(match resource {
        fanout::Resource::Workspace(id) => wire::Change::ChangeWorkspace(wire::ChangeWorkspace {
            resource: "workspace".into(),
            id: wire_id(&id),
            workspace: tx.workspaces().get(id).await?.name,
        }),
        fanout::Resource::Session(id) => {
            let session = tx.workspaces().session(id).await?;
            wire::Change::ChangeSession(wire::ChangeSession {
                resource: "session".into(),
                id: wire_id(&id),
                workspace: tx.workspaces().get(session.workspace).await?.name,
            })
        }
        fanout::Resource::Queue => wire::Change::ChangeQueue(wire::ChangeQueue {
            resource: "queue".into(),
        }),
    })
}

pub(super) fn transcript_event(
    cursor: Option<Cursor>,
    event: wire::Event,
) -> serde_json::Result<Emitted> {
    Emitted::new(event.event_name(), cursor, &event)
}

pub(super) fn changes_event(event: wire::ChangesEvent) -> serde_json::Result<Emitted> {
    Emitted::new(event.event_name(), None, &event)
}

pub(super) fn refetch() -> wire::Refetch {
    serde_json::json!({})
}

pub(super) struct Read {
    page: log::TranscriptPage,
    session_state: ObservedSession,
    sealed: bool,
}

#[derive(Clone, PartialEq)]
pub(super) struct ObservedSession {
    session_id: Option<domain::SessionId>,
    state: crate::live_work::SessionState,
    observation: crate::live_work::Observation,
}

pub(super) fn transcript_session_state(observed: ObservedSession) -> wire::TranscriptSessionState {
    let ObservedSession {
        session_id,
        state,
        observation,
    } = observed;
    wire::TranscriptSessionState {
        session_id: session_id.map(|id| wire_id(&id)),
        tools: state.tools.into_iter().map(running_tool).collect(),
        units: state.units.into_iter().map(running_unit).collect(),
        message_buffering: state.message_buffering,
        thought_buffering: state.thought_buffering,
        usage: state.usage.map(session_usage),
        last_activity_at: state.last_activity_at.map(|at| at.to_string()),
        observation: session_observation(observation),
    }
}

pub(super) fn transcript_kind(kind: log::Kind) -> wire::TranscriptKind {
    match kind {
        log::Kind::SharedState => wire::TranscriptKind::SharedState,
        log::Kind::Narration => wire::TranscriptKind::Narration,
        log::Kind::Detail => wire::TranscriptKind::Detail,
    }
}

/// The stored body is the only copy that holds payload references, so it is decoded rather than
/// rebuilt from the domain entry.
pub(super) fn recorded(entry: log::StoredTranscriptEntry) -> serde_json::Result<wire::Recorded> {
    Ok(wire::Recorded {
        kind: transcript_kind(entry.kind),
        session_id: entry.session_id.map(|id| wire_id(&id)),
        seq: entry.seq,
        appended_at: entry.appended_at.to_string(),
        entry: serde_json::from_value(entry.entry)?,
    })
}

pub(super) fn activity(activity: log::Activity) -> wire::Activity {
    let count = |count: u64| -> i64 { count.try_into().expect("a count fits a wire integer") };
    wire::Activity {
        first_seq: activity.first_seq,
        last_seq: activity.last_seq,
        counts: wire::ActivityCounts {
            tool_calls: count(activity.counts.tool_calls),
            failed_calls: count(activity.counts.failed_calls),
            thoughts: count(activity.counts.thoughts),
            plans: count(activity.counts.plans),
            tombstones: count(activity.counts.tombstones),
        },
        latest: activity.latest.map(|latest| wire::ActivityLatest {
            kind: transcript_kind(latest.kind),
            title: latest.title,
            status: latest.status,
        }),
        started_at: activity.started_at.map(|at| at.to_string()),
        finished_at: activity.finished_at.map(|at| at.to_string()),
        anomaly: activity.anomaly,
        closed: activity.closed,
    }
}

pub(super) fn presence(presence: crate::presence::Presence) -> wire::Presence {
    wire::Presence {
        named: presence.named,
        anonymous: presence
            .anonymous
            .try_into()
            .expect("a follower count fits a wire integer"),
    }
}

pub(super) async fn transcript_payload(
    State(control_plane): State<ControlPlane>,
    Path((organization, workspace, payload)): Path<(String, String, String)>,
) -> Result<Response, Refused> {
    let workspace = resolved(&control_plane, &organization, &workspace).await?;
    let mut tx = control_plane.store.read().await?;
    let payload = match tx.log().payload(&workspace, &payload).await? {
        log::PayloadRead::Available(payload) => payload,
        log::PayloadRead::Gone => {
            return Err(diagnosed(
                Reason::Expired {
                    operation: "transcript_payload",
                    resource: Resource::TranscriptPayload,
                    reference: payload,
                    organization: Some(organization),
                    next: declined::Next::inspect(Resource::Workspace, workspace.id.to_string()),
                    message: "the Transcript payload has expired".to_owned(),
                },
                None,
            ));
        }
        log::PayloadRead::Missing => {
            return Err(diagnosed(
                Reason::MissingReference {
                    resource: Resource::TranscriptPayload,
                    reference: payload,
                    organization: Some(organization),
                    within: Some(Locator::new(Resource::Workspace, workspace.id.to_string())),
                    message: "no payload in this Workspace".to_owned(),
                },
                None,
            ));
        }
    };
    Ok((
        [(axum::http::header::CONTENT_TYPE, payload.media_type)],
        payload.content,
    )
        .into_response())
}

/// A stream that closes without an `end` event was cut off, and the reader resumes it from
/// the last id it was handed. A read that stays open past caught-up registers its follower and
/// carries who is following, transiently (ADR-0035).
pub(super) async fn transcript(
    State(control_plane): State<ControlPlane>,
    Path((organization, workspace)): Path<(String, String)>,
    Query(following): Query<Following>,
    headers: HeaderMap,
) -> Result<Sse<impl Stream<Item = Result<Event, BoxError>>>, Refused> {
    let workspace = resolved(&control_plane, &organization, &workspace).await?;
    let name = follower_name(&control_plane, &workspace, following.as_name.as_deref()).await?;
    let range = log::SeqRange {
        first_seq: following.first_seq,
        last_seq: following.last_seq,
    };
    range.validate()?;
    let transcribing = Transcribing {
        workspace: workspace.id,
        name,
        follow: following.follow.unwrap_or(true),
        kinds: kinds(following.kinds.as_deref())?,
        summaries: following.summaries.unwrap_or(true),
        range,
        from: last_event_id(&headers)?,
    };
    let read = transcribing.read(&control_plane).await?;

    Ok(per_resource(transcribed(control_plane, transcribing, read)))
}

pub(super) async fn follower_name(
    control_plane: &ControlPlane,
    workspace: &Workspace,
    name: Option<&str>,
) -> Result<Option<String>, Refused> {
    let Some(name) = name else {
        return Ok(None);
    };
    let mut tx = control_plane.store.read().await?;

    Ok(Some(
        participant::accepted(&mut tx, &workspace.organization, name, "transcript").await?,
    ))
}

pub(super) struct Transcribing {
    workspace: WorkspaceId,
    name: Option<String>,
    follow: bool,
    kinds: log::Kinds,
    summaries: bool,
    range: log::SeqRange,
    from: Option<Cursor>,
}

impl Transcribing {
    async fn read(&self, control_plane: &ControlPlane) -> Result<Read, Refused> {
        reading(
            control_plane,
            self.workspace,
            self.from,
            &self.kinds,
            self.summaries,
            self.range,
        )
        .await
    }
}

pub(super) fn kinds(kinds: Option<&str>) -> Result<log::Kinds, Refused> {
    kinds
        .map(str::parse::<log::Kinds>)
        .transpose()
        .map_err(|error| malformed(Some("kinds"), error.to_string()))
        .map(Option::unwrap_or_default)
}

pub(super) fn transcribed(
    control_plane: ControlPlane,
    transcribing: Transcribing,
    mut read: Read,
) -> stream::Events {
    let Transcribing {
        workspace,
        name,
        follow,
        kinds,
        summaries,
        range,
        from,
    } = transcribing;

    Box::pin(async_stream::try_stream! {
        let mut joined: Option<crate::presence::Joined> = None;
        let mut delivered = from;
        let mut session_state = read.session_state.clone();
        if follow {
            yield transcript_event(None, wire::Event::TranscriptSessionState(
                transcript_session_state(session_state.clone()),
            ))?;
        }
        loop {
            if follow && read.session_state != session_state {
                session_state = read.session_state.clone();
                yield transcript_event(None, wire::Event::TranscriptSessionState(
                    transcript_session_state(session_state.clone()),
                ))?;
            }
            let mut activities = read.page.activities.into_iter().peekable();
            for entry in read.page.entries {
                while activities.peek().is_some_and(|activity| activity.last_seq < entry.seq) {
                    let summary = activities.next().unwrap();
                    delivered = Some(Cursor::at(workspace, summary.last_seq));
                    yield transcript_event(delivered, wire::Event::Activity(activity(summary)))?;
                }
                delivered = Some(Cursor::at(workspace, entry.seq));
                yield transcript_event(delivered, wire::Event::Recorded(recorded(entry)?))?;
            }
            for summary in activities {
                delivered = Some(Cursor::at(workspace, summary.last_seq));
                yield transcript_event(delivered, wire::Event::Activity(activity(summary)))?;
            }
            if let Some(cursor) = read.page.cursor && delivered != Some(cursor) {
                delivered = Some(cursor);
                yield transcript_event(delivered, wire::Event::CursorEvent(cursor.to_string()))?;
            }
            if !read.page.more {
                let because = match (read.sealed, follow) {
                    (true, _) => Some(wire::EndBecause::Sealed),
                    (false, false) => Some(wire::EndBecause::CaughtUp),
                    (false, true) if range.last_seq.is_some() => Some(wire::EndBecause::CaughtUp),
                    (false, true) => None,
                };
                if let Some(because) = because {
                    yield transcript_event(None, wire::Event::End(wire::End { because }))?;
                    break;
                }

                if joined.is_none() {
                    let mut follower = control_plane.followers.join(workspace, name.clone());
                    yield transcript_event(None, wire::Event::FollowerEvent(wire::FollowerEvent {
                        id: wire_id(&follower.id),
                        lease_seconds: follower
                            .lease
                            .as_secs()
                            .try_into()
                            .expect("a lease fits a wire integer"),
                    }))?;
                    yield transcript_event(None, wire::Event::Presence(presence(follower.snapshot())))?;
                    joined = Some(follower);
                }

                let follower = joined.as_mut().expect("registered just above");
                if *follower.expiration.borrow() {
                    break;
                }
                if follower.presence.has_changed().unwrap_or(false) {
                    yield transcript_event(None, wire::Event::Presence(presence(follower.snapshot())))?;
                }

                tokio::select! {
                    () = tokio::time::sleep(POLL) => {}
                    () = control_plane.shutdown.cancelled() => break,
                }
            }

            read = reading(&control_plane, workspace, read.page.cursor, &kinds, summaries, range)
                .await
                .map_err(Refused::into_error)?;
        }
    })
}

/// An unknown or expired follower is a 404, never a fresh registration.
pub(super) async fn renew_follower(
    State(control_plane): State<ControlPlane>,
    Path((organization, workspace, id)): Path<(String, String, String)>,
) -> Result<StatusCode, Refused> {
    let workspace = resolved(&control_plane, &organization, &workspace).await?;
    let id = id
        .parse::<crate::presence::FollowerId>()
        .map_err(|_| Refused::NotFound("no such follower".to_owned()))?;

    if control_plane.followers.renew(workspace.id, id) {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(Refused::NotFound(
            "no such follower, or its lease has passed".to_owned(),
        ))
    }
}

pub(super) async fn changes(
    State(control_plane): State<ControlPlane>,
    Path(organization): Path<String>,
) -> Result<Sse<impl Stream<Item = Result<Event, BoxError>>>, Refused> {
    let notices = noticed(&control_plane, &organization).await?;

    Ok(per_resource(notices))
}

/// Every subscriber is told to refetch everything on connect, and again if it fell behind the
/// hub's buffer: a notice is a hint, so a missed one must never look like a quiet stream.
pub(super) async fn noticed(
    control_plane: &ControlPlane,
    organization: &str,
) -> Result<stream::Events, Refused> {
    let mut tx = control_plane.store.read().await?;
    let organization = tx.organizations().named(organization).await?;
    let mut subscription = control_plane.store.notices().subscribe(organization.id);
    drop(tx);
    let control_plane = control_plane.clone();

    Ok(Box::pin(async_stream::try_stream! {
        yield changes_event(wire::ChangesEvent::ChangesOpen(refetch()))?;
        loop {
            let watch = tokio::select! {
                () = control_plane.shutdown.cancelled() => break,
                watch = subscription.recv() => watch,
            };
            match watch {
                Some(fanout::Watch::Change(resource)) => {
                    match changed(&control_plane.store, resource).await {
                        Ok(changed) => yield changes_event(wire::ChangesEvent::Change(changed))?,
                        Err(_) => yield changes_event(wire::ChangesEvent::ChangesResync(refetch()))?,
                    }
                }
                Some(fanout::Watch::Resync) => {
                    yield changes_event(wire::ChangesEvent::ChangesResync(refetch()))?;
                }
                None => break,
            }
        }
    }))
}

pub(super) fn per_resource(
    mut events: stream::Events,
) -> Sse<impl Stream<Item = Result<Event, BoxError>>> {
    let framed = async_stream::try_stream! {
        while let Some(emitted) = events.next().await {
            let emitted = emitted?;
            let mut event = Event::default().event(emitted.event).json_data(&emitted.data)?;
            if let Some(cursor) = emitted.cursor {
                event = event.id(cursor.to_string());
            }
            yield event;
        }
    };

    Sse::new(framed).keep_alive(KeepAlive::new().interval(KEEP_ALIVE))
}

pub(super) async fn reserve_stream(
    State(control_plane): State<ControlPlane>,
) -> Result<(StatusCode, Json<wire::StreamReservation>), Refused> {
    let token = control_plane.streams.reserve().map_err(stream_refused)?;

    Ok((
        StatusCode::CREATED,
        Json(wire::StreamReservation { token: token.0 }),
    ))
}

pub(super) async fn open_stream(
    State(control_plane): State<ControlPlane>,
    Path(token): Path<String>,
) -> Result<Sse<impl Stream<Item = Result<Event, BoxError>>>, Refused> {
    let connected = control_plane
        .streams
        .connect(reservation(&token)?)
        .map_err(stream_refused)?;
    let shutdown = control_plane.shutdown.clone();

    let framed = async_stream::try_stream! {
        let mut subscriptions = StreamMap::new();
        let mut generations = HashMap::new();
        loop {
            let (id, emitted) = tokio::select! {
                () = shutdown.cancelled() => break,
                () = connected.changed() => {
                    let wanted = connected.wanted();
                    for subscribed in wanted.unstarted {
                        generations.insert(subscribed.id.clone(), subscribed.generation);
                        subscriptions.insert(subscribed.id, cut_off_unless_ended(subscribed.events));
                    }
                    generations.retain(|id, generation| {
                        let kept = wanted.held.get(id) == Some(generation);
                        if !kept {
                            subscriptions.remove(id);
                        }
                        kept
                    });
                    continue;
                }
                Some(next) = subscriptions.next(), if !subscriptions.is_empty() => next,
            };
            let emitted: Emitted = match emitted {
                Ok(emitted) => emitted,
                Err(error) => {
                    warn!(%error, subscription = id, "closing a reserved stream");
                    break;
                }
            };
            let ended = emitted.ends();
            yield Event::default().event(emitted.event).json_data(wire::StreamEvent {
                subscription: id.clone(),
                cursor: emitted.cursor.map(|cursor| cursor.to_string()),
                data: emitted.data,
            })?;
            if ended {
                subscriptions.remove(&id);
                if let Some(generation) = generations.remove(&id) {
                    connected.ended(&id, generation);
                }
            }
        }
    };

    Ok(Sse::new(framed).keep_alive(KeepAlive::new().interval(KEEP_ALIVE)))
}

/// A subscription cut off short of `end` closes its whole connection, so the tab resumes every
/// subscription from its cursors rather than missing one that went quiet.
pub(super) fn cut_off_unless_ended(mut events: stream::Events) -> stream::Events {
    Box::pin(async_stream::try_stream! {
        while let Some(emitted) = events.next().await {
            let emitted = emitted?;
            let ended = emitted.ends();
            yield emitted;
            if ended {
                return;
            }
        }
        Err::<(), BoxError>("a subscription was cut off".into())?;
    })
}

pub(super) async fn subscribe_stream(
    State(control_plane): State<ControlPlane>,
    Path((token, id)): Path<(String, String)>,
    subscription: Result<Json<wire::StreamSubscription>, JsonRejection>,
) -> Result<StatusCode, Refused> {
    let token = reservation(&token)?;
    stream::bounded_id(&id).map_err(stream_refused)?;
    if !control_plane.streams.known(token) {
        return Err(stream_refused(stream::Refusal::Unknown));
    }
    let Json(subscription) = subscription?;
    let events = match subscription.kind {
        wire::StreamSubscriptionKind::Notices => {
            noticed(&control_plane, &subscription.organization).await?
        }
        wire::StreamSubscriptionKind::Transcript => {
            let workspace = subscription.workspace.as_deref().ok_or_else(|| {
                malformed(
                    Some("workspace"),
                    "a transcript subscription names its Workspace",
                )
            })?;
            let workspace = resolved(&control_plane, &subscription.organization, workspace).await?;
            let name = follower_name(
                &control_plane,
                &workspace,
                subscription.participant.as_deref(),
            )
            .await?;
            let transcribing = Transcribing {
                workspace: workspace.id,
                name,
                follow: true,
                kinds: kinds(subscription.kinds.as_deref())?,
                summaries: true,
                range: log::SeqRange::default(),
                from: subscription
                    .after
                    .as_deref()
                    .map(str::parse)
                    .transpose()
                    .map_err(|error: anyhow::Error| malformed(Some("after"), error.to_string()))?,
            };
            let read = transcribing.read(&control_plane).await?;
            transcribed(control_plane.clone(), transcribing, read)
        }
    };
    control_plane
        .streams
        .subscribe(token, id, events)
        .map_err(stream_refused)?;

    Ok(StatusCode::NO_CONTENT)
}

pub(super) async fn unsubscribe_stream(
    State(control_plane): State<ControlPlane>,
    Path((token, id)): Path<(String, String)>,
) -> Result<StatusCode, Refused> {
    control_plane
        .streams
        .unsubscribe(reservation(&token)?, id)
        .map_err(stream_refused)?;

    Ok(StatusCode::NO_CONTENT)
}

pub(super) fn reservation(token: &str) -> Result<stream::Token, Refused> {
    token
        .parse()
        .map_err(|_| stream_refused(stream::Refusal::Unknown))
}

pub(super) fn stream_refused(refusal: stream::Refusal) -> Refused {
    match refusal {
        stream::Refusal::Unknown => Refused::NotFound(
            "no such stream reservation, or it has expired; reserve again".to_owned(),
        ),
        stream::Refusal::AlreadyConnected => {
            Refused::Conflict("the reserved stream is already open".to_owned())
        }
        stream::Refusal::TooManySubscriptions => Refused::Conflict(format!(
            "a reserved stream holds at most {} subscriptions",
            stream::MOST_SUBSCRIPTIONS
        )),
        stream::Refusal::IdOutOfBounds => {
            malformed(None, "a subscription id is between 1 and 64 characters")
        }
        stream::Refusal::TooManyReservations => Refused::Unavailable(anyhow::anyhow!(
            "{} streams are already reserved",
            stream::MOST_RESERVATIONS
        )),
    }
}

/// The state is read in the transaction the page is, so a Workspace sealed between the two
/// cannot end the stream short of its last entry.
pub(super) async fn reading(
    control_plane: &ControlPlane,
    id: WorkspaceId,
    from: Option<Cursor>,
    kinds: &log::Kinds,
    summaries: bool,
    range: log::SeqRange,
) -> Result<Read, Refused> {
    let mut tx = control_plane.store.read().await?;
    let workspace = tx
        .workspaces()
        .find(id)
        .await?
        .ok_or_else(|| Refused::NotFound("no workspace".to_owned()))?;
    let page = tx
        .log()
        .transcript_page(&workspace, from, Window::DEFAULT, kinds, summaries, range)
        .await?;

    let session = tx
        .workspaces()
        .unfinished_session(&workspace)
        .await?
        .map(|holding| holding.session);
    let (observation, state) = control_plane.live.summaries.observed(session.as_ref());
    let session_state = ObservedSession {
        session_id: session.as_ref().map(|session| session.id),
        state,
        observation,
    };
    Ok(Read {
        page,
        session_state,
        sealed: workspace.state == WorkspaceState::Sealed,
    })
}

pub(super) fn last_event_id(headers: &HeaderMap) -> Result<Option<Cursor>, Refused> {
    let Some(cursor) = headers.get("last-event-id") else {
        return Ok(None);
    };

    cursor
        .to_str()
        .map_err(|error| malformed(None, error.to_string()))?
        .parse()
        .map(Some)
        .map_err(|error: anyhow::Error| malformed(None, error.to_string()))
}
