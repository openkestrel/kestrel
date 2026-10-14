use std::collections::HashSet;
use std::io::Write;
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result, anyhow};
use kestrel_operator_types as wire;
use reqwest::{Client, StatusCode, Url, header};

use crate::diagnostic::{self, Request};
use crate::exit::{Exit, Failed};
use crate::output::Presentation;
use crate::sse::Events;
use crate::typed;
use crate::view;

/// How long a stream may stay unreachable before the read gives up on it.
const PATIENCE: Duration = Duration::from_secs(30);
const RETRY: Duration = Duration::from_millis(250);

const OPERATION: Request<'static> = Request {
    operation: "workspace transcript",
    write: false,
};

enum Cut {
    Refused(Failed),
    /// The server's own answer when it gave one, so giving up reports why rather than a lost line.
    Lost(Option<Failed>),
    Failed(anyhow::Error),
}

impl From<reqwest::Error> for Cut {
    fn from(_: reqwest::Error) -> Self {
        Cut::Lost(None)
    }
}

pub struct Selection<'a> {
    pub follow: bool,
    pub kinds: &'a str,
    pub summaries: bool,
    pub as_participant: Option<&'a str>,
}

#[derive(Clone, Copy)]
struct Stream<'a> {
    client: &'a Client,
    url: &'a Url,
    followers: &'a Url,
    selection: &'a Selection<'a>,
    presentation: &'a Presentation,
}

pub async fn read(
    control_plane: &Url,
    organization: &str,
    workspace: &str,
    from: Option<String>,
    selection: Selection<'_>,
    presentation: &Presentation,
) -> Result<Option<String>> {
    let client = Client::new();
    let url = transcript(control_plane, organization, workspace, &selection)?;
    let followers = followers(control_plane, organization, workspace)?;
    let stream = Stream {
        client: &client,
        url: &url,
        followers: &followers,
        selection: &selection,
        presentation,
    };
    let mut activities = Activities::default();
    let mut cursor = from;
    let mut heard = Instant::now();
    let mut answered = None;

    loop {
        match streamed(stream, &mut cursor, &mut heard, &mut activities).await {
            Ok(()) => return Ok(cursor),
            Err(Cut::Refused(failed)) => return Err(failed.into()),
            Err(Cut::Failed(error)) => return Err(error),
            Err(Cut::Lost(answer)) => {
                answered = answer.or(answered);
                if heard.elapsed() > PATIENCE {
                    return Err(answered
                        .unwrap_or_else(|| {
                            diagnostic::unreachable(control_plane.as_str(), OPERATION, true)
                        })
                        .into());
                }
                tokio::time::sleep(RETRY).await;
            }
        }
    }
}

async fn streamed(
    stream: Stream<'_>,
    cursor: &mut Option<String>,
    heard: &mut Instant,
    activities: &mut Activities,
) -> Result<(), Cut> {
    let Stream {
        client,
        url,
        followers,
        selection,
        presentation,
    } = stream;
    let mut request = client
        .get(url.clone())
        .header(header::ACCEPT, "text/event-stream");
    if let Some(cursor) = cursor {
        request = request.header("last-event-id", cursor.as_str());
    }

    let response = request.send().await?;
    let status = response.status();
    if !status.is_success() {
        let refusal = diagnostic::refusal(response, OPERATION).await;
        return Err(if status.is_client_error() {
            Cut::Refused(refusal)
        } else {
            Cut::Lost(Some(refusal))
        });
    }
    *heard = Instant::now();

    let mut renewal: Option<Renewal> = None;
    let mut events = Events::over(response);
    let mut stdout = std::io::stdout().lock();
    while let Some(event) = events.next().await? {
        *heard = Instant::now();
        // A name this Client does not know is a later event, and is passed over.
        let Some(decoded) = event
            .name
            .as_deref()
            .and_then(|name| wire::Event::from_sse(name, &event.data))
        else {
            continue;
        };
        let decoded = decoded
            .map_err(|error| Cut::Failed(anyhow!(error).context("reading the transcript")))?;
        match decoded {
            wire::Event::Recorded(entry) => {
                write_entry(&mut stdout, presentation, &entry)?;
                *cursor = event.id;
            }
            wire::Event::Activity(activity) => {
                if let Some(activity) = activities.observe(activity) {
                    write_activity(&mut stdout, presentation, &activity)?;
                }
                *cursor = event.id;
            }
            wire::Event::CursorEvent(_) => *cursor = event.id,
            wire::Event::FollowerEvent(follower) => {
                // Replacing stops the renewal of a registration this stream no longer holds.
                let _previous = renewal.replace(Renewal::start(
                    client,
                    followers,
                    &follower.id.to_string(),
                    follower.lease_seconds.try_into().unwrap_or(1),
                ));
            }
            wire::Event::End(_) => {
                if !selection.follow
                    && let Some(open) = activities.open.take()
                {
                    write_activity(&mut stdout, presentation, &open)?;
                }
                return Ok(());
            }
            wire::Event::TranscriptSessionState(state) => {
                write_transient(&mut stdout, presentation, &view::SESSION_STATE, &state)?;
            }
            wire::Event::Presence(presence) => {
                write_transient(&mut stdout, presentation, &view::PRESENCE, &presence)?;
            }
        }
    }

    Err(Cut::Lost(None))
}

fn write_entry(
    out: &mut impl Write,
    presentation: &Presentation,
    entry: &wire::Recorded,
) -> Result<(), Cut> {
    let wrote = typed::record(out, presentation, &view::ENTRIES, entry);
    written(out, wrote)
}

/// An Activity reaches stdout as a whole record when the invocation asked for JSON, and as one
/// prose line otherwise.
fn write_activity(
    out: &mut impl Write,
    presentation: &Presentation,
    activity: &wire::Activity,
) -> Result<(), Cut> {
    let wrote = if presentation.json() {
        crate::typed::document(out, presentation, activity)
    } else {
        writeln!(out, "{}", activity_line(activity)).map_err(anyhow::Error::from)
    };
    written(out, wrote)
}

/// A transient event reaches a person as its line; the complete answer keeps the records it
/// already carried, so JSON writes nothing for one.
fn write_transient<T: serde::Serialize>(
    out: &mut impl Write,
    presentation: &Presentation,
    columns: &typed::Columns<T>,
    event: &T,
) -> Result<(), Cut> {
    if presentation.json() {
        return Ok(());
    }
    let wrote = typed::record(out, presentation, columns, event);
    written(out, wrote)
}

fn written(out: &mut impl Write, wrote: Result<()>) -> Result<(), Cut> {
    wrote
        .and_then(|()| Ok(out.flush()?))
        .map_err(|error| Cut::Failed(error.context("writing the transcript")))
}

struct Renewal {
    task: tokio::task::JoinHandle<()>,
}

impl Renewal {
    fn start(client: &Client, followers: &Url, id: &str, lease_seconds: u64) -> Self {
        let mut url = followers.clone();
        url.path_segments_mut()
            .expect("the followers URL is a base for a path")
            .extend([id, "lease"]);
        // A third of the lease, so a dropped renewal still leaves two more before it passes.
        let every = Duration::from_secs((lease_seconds / 3).max(1));

        Self {
            task: tokio::spawn(renewing(client.clone(), url, every)),
        }
    }
}

impl Drop for Renewal {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn renewing(client: Client, url: Url, every: Duration) {
    loop {
        tokio::time::sleep(every).await;

        match client.post(url.clone()).send().await {
            Ok(response) if response.status() == StatusCode::NO_CONTENT => {}
            Ok(response) if response.status() == StatusCode::NOT_FOUND => return,
            _ => {}
        }
    }
}

fn transcript(
    control_plane: &Url,
    organization: &str,
    workspace: &str,
    selection: &Selection<'_>,
) -> Result<Url> {
    let mut url = workspace_path(control_plane, organization, workspace, "transcript")?;
    {
        let mut query = url.query_pairs_mut();
        query
            .append_pair("follow", if selection.follow { "true" } else { "false" })
            .append_pair("kinds", selection.kinds)
            .append_pair(
                "summaries",
                if selection.summaries { "true" } else { "false" },
            );
        if let Some(name) = selection.as_participant {
            query.append_pair("as", name);
        }
    }

    Ok(url)
}

fn followers(control_plane: &Url, organization: &str, workspace: &str) -> Result<Url> {
    workspace_path(control_plane, organization, workspace, "followers")
}

fn workspace_path(
    control_plane: &Url,
    organization: &str,
    workspace: &str,
    tail: &str,
) -> Result<Url> {
    let mut url = control_plane.clone();
    url.path_segments_mut()
        .map_err(|()| {
            Failed::new(
                Exit::Usage,
                format!("{control_plane} cannot be a base for a path"),
            )
        })
        .context("addressing the transcript")?
        .pop_if_empty()
        .extend([
            "operator",
            "organizations",
            organization,
            "workspaces",
            workspace,
            tail,
        ]);

    Ok(url)
}

#[derive(Default)]
struct Activities {
    printed: HashSet<i64>,
    open: Option<wire::Activity>,
}
impl Activities {
    fn observe(&mut self, activity: wire::Activity) -> Option<wire::Activity> {
        if activity.closed {
            self.open = None;
            if self.printed.insert(activity.first_seq) {
                return Some(activity);
            }
        } else if !self.printed.contains(&activity.first_seq) {
            self.open = Some(activity);
        }
        None
    }
}
fn activity_line(activity: &wire::Activity) -> String {
    let counts = &activity.counts;
    format!(
        "activity  {}..{}  {} tools, {} failed, {} thoughts, {} plans, {} tombstones{}",
        activity.first_seq,
        activity.last_seq,
        counts.tool_calls,
        counts.failed_calls,
        counts.thoughts,
        counts.plans,
        counts.tombstones,
        if activity.anomaly {
            "  interrupted or unresolved"
        } else {
            ""
        }
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn activity(first_seq: i64, closed: bool) -> wire::Activity {
        let data = serde_json::json!({
            "first_seq": first_seq, "last_seq": first_seq + 2, "closed": closed, "anomaly": true,
            "counts": { "tool_calls": 2, "failed_calls": 1, "thoughts": 0, "plans": 1, "tombstones": 0 },
            "latest": null, "started_at": null, "finished_at": null
        });
        match wire::Event::from_sse("activity", &data.to_string()) {
            Some(Ok(wire::Event::Activity(activity))) => activity,
            other => panic!("not an Activity: {other:?}"),
        }
    }

    #[test]
    fn a_closed_activity_is_printed_once_however_often_a_reconnect_replays_it() {
        let mut activities = Activities::default();

        assert!(activities.observe(activity(4, false)).is_none());
        assert_eq!(activities.open.as_ref().map(|open| open.first_seq), Some(4));
        assert_eq!(
            activities
                .observe(activity(4, true))
                .map(|closed| closed.first_seq),
            Some(4)
        );
        assert!(activities.open.is_none());
        assert!(activities.observe(activity(4, true)).is_none());
        assert!(activities.observe(activity(4, false)).is_none());
        assert!(activities.open.is_none());
    }

    #[test]
    fn an_activity_reads_as_one_line_of_its_counts() {
        assert_eq!(
            activity_line(&activity(4, true)),
            "activity  4..6  2 tools, 1 failed, 0 thoughts, 1 plans, 0 tombstones  interrupted or unresolved"
        );
    }

    fn session_state(patch: serde_json::Value) -> wire::TranscriptSessionState {
        let mut record = serde_json::json!({
            "message_buffering": false, "thought_buffering": false,
            "observation": { "availability": "unavailable", "last": null },
            "session_id": null, "tools": [], "units": [], "usage": null
        });
        record
            .as_object_mut()
            .unwrap()
            .extend(patch.as_object().unwrap().clone());
        serde_json::from_value(record).unwrap()
    }

    fn presence(patch: serde_json::Value) -> wire::Presence {
        let mut record = serde_json::json!({ "named": [], "anonymous": 0 });
        record
            .as_object_mut()
            .unwrap()
            .extend(patch.as_object().unwrap().clone());
        serde_json::from_value(record).unwrap()
    }

    #[test]
    fn a_session_state_and_a_presence_read_as_their_own_lines_a_person_can_see() {
        let state = session_state(serde_json::json!({
            "session_id": "00000000-0000-0000-0000-000000000001",
            "usage": { "context_size": 200000, "context_used": 1200, "cost": null },
        }));
        let presence = presence(serde_json::json!({ "named": ["Ada"], "anonymous": 2 }));
        let mut out = Vec::new();

        write_transient(
            &mut out,
            &Presentation::Delimited,
            &view::SESSION_STATE,
            &state,
        )
        .unwrap_or_else(|_| panic!("a session state"));
        write_transient(
            &mut out,
            &Presentation::Delimited,
            &view::PRESENCE,
            &presence,
        )
        .unwrap_or_else(|_| panic!("a presence"));

        assert_eq!(
            String::from_utf8(out).expect("utf-8"),
            "session state  00000000-0000-0000-0000-000000000001  unavailable  0 tools, 0 units  \
             1200 of 200000 context used\n\
             presence  Ada  2 anonymous\n"
        );
    }

    #[test]
    fn a_json_stream_keeps_only_the_records_it_already_carried() {
        let state = session_state(serde_json::json!({}));
        let presence = presence(serde_json::json!({}));
        let mut out = Vec::new();

        write_transient(
            &mut out,
            &Presentation::Json(None),
            &view::SESSION_STATE,
            &state,
        )
        .unwrap_or_else(|_| panic!("a session state"));
        write_transient(
            &mut out,
            &Presentation::Json(None),
            &view::PRESENCE,
            &presence,
        )
        .unwrap_or_else(|_| panic!("a presence"));

        assert!(out.is_empty(), "{}", String::from_utf8_lossy(&out));
    }
}
