use std::collections::HashSet;
use std::io::Write as _;
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result, anyhow, bail};
use reqwest::{Client, StatusCode, Url, header};
use serde::Deserialize;
use serde_json::Value;

use crate::exit::{Exit, Failed};
use crate::output::Presentation;
use crate::sse::Events;
use crate::view;

/// How long a stream may stay unreachable before the read gives up on it.
const PATIENCE: Duration = Duration::from_secs(30);
const RETRY: Duration = Duration::from_millis(250);

#[derive(Deserialize)]
struct Refusal {
    message: String,
}

enum Cut {
    Refused(StatusCode, String),
    Lost(anyhow::Error),
    Failed(anyhow::Error),
}

impl From<reqwest::Error> for Cut {
    fn from(error: reqwest::Error) -> Self {
        Cut::Lost(error.into())
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

    loop {
        match streamed(stream, &mut cursor, &mut heard, &mut activities).await {
            Ok(()) => return Ok(cursor),
            Err(Cut::Refused(status, why)) => bail!(Failed::new(
                crate::api::refused(status),
                format!("the control plane refused the read: {why}")
            )),
            Err(Cut::Failed(error)) => return Err(error),
            Err(Cut::Lost(error)) if heard.elapsed() > PATIENCE => {
                return Err(error.context(Failed::new(
                    Exit::Unavailable,
                    format!("reading the transcript from {control_plane}"),
                )));
            }
            Err(Cut::Lost(_)) => tokio::time::sleep(RETRY).await,
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
    if status.is_client_error() {
        let why = response
            .json::<Refusal>()
            .await
            .map_or_else(|_| status.to_string(), |refusal| refusal.message);
        return Err(Cut::Refused(status, why));
    }
    if !status.is_success() {
        return Err(Cut::Lost(anyhow!("the control plane answered {status}")));
    }
    *heard = Instant::now();

    let mut renewal: Option<Renewal> = None;
    let mut events = Events::over(response);
    let mut stdout = std::io::stdout().lock();
    while let Some(event) = events.next().await? {
        *heard = Instant::now();
        match event.name.as_deref() {
            Some("entry") => {
                let entry = presented(&event.data, presentation).map_err(Cut::Failed)?;
                writeln!(stdout, "{entry}")
                    .and_then(|()| stdout.flush())
                    .map_err(|error| {
                        Cut::Failed(anyhow!(error).context("writing the transcript"))
                    })?;
                *cursor = event.id;
            }
            Some("activity") => {
                let activity: Value =
                    serde_json::from_str(&event.data).map_err(|error| Cut::Failed(error.into()))?;
                if let Some(line) = activities
                    .observe(activity, presentation)
                    .map_err(Cut::Failed)?
                {
                    writeln!(stdout, "{line}")
                        .and_then(|()| stdout.flush())
                        .map_err(|error| Cut::Failed(error.into()))?;
                }
                *cursor = event.id;
            }
            Some("cursor") => *cursor = event.id,
            Some("follower") => {
                let follower: Follower = serde_json::from_str(&event.data)
                    .map_err(|error| Cut::Failed(anyhow!(error).context("reading the follower")))?;
                // Replacing stops the renewal of a registration this stream no longer holds.
                let _previous = renewal.replace(Renewal::start(
                    client,
                    followers,
                    &follower.id,
                    follower.lease_seconds,
                ));
            }
            Some("end") => {
                if !selection.follow
                    && let Some(open) = activities.open.take()
                {
                    let line = activity_line(&open, presentation).map_err(Cut::Failed)?;
                    writeln!(stdout, "{line}")
                        .and_then(|()| stdout.flush())
                        .map_err(|error| Cut::Failed(error.into()))?;
                }
                return Ok(());
            }
            _ => {}
        }
    }

    Err(Cut::Lost(anyhow!("the stream closed before it ended")))
}

#[derive(Deserialize)]
struct Follower {
    id: String,
    lease_seconds: u64,
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

fn presented(data: &str, presentation: &Presentation) -> Result<String> {
    let entry: Value = serde_json::from_str(data)
        .context(Failed::new(Exit::Unavailable, "reading a transcript entry"))?;

    crate::output::line(presentation, &view::ENTRIES, &entry)
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
    open: Option<Value>,
}
impl Activities {
    fn observe(&mut self, activity: Value, presentation: &Presentation) -> Result<Option<String>> {
        let first = activity["first_seq"]
            .as_i64()
            .context("an Activity has no first seq")?;
        if activity["closed"] == true {
            self.open = None;
            if self.printed.insert(first) {
                return activity_line(&activity, presentation).map(Some);
            }
        } else if !self.printed.contains(&first) {
            self.open = Some(activity);
        }
        Ok(None)
    }
}
fn activity_line(activity: &Value, presentation: &Presentation) -> Result<String> {
    if matches!(presentation, Presentation::Json(_)) {
        return crate::output::line(presentation, &view::ENTRIES, activity);
    }
    let counts = &activity["counts"];
    Ok(format!(
        "activity  {}..{}  {} tools, {} failed, {} thoughts, {} plans, {} tombstones{}",
        activity["first_seq"],
        activity["last_seq"],
        counts["tool_calls"],
        counts["failed_calls"],
        counts["thoughts"],
        counts["plans"],
        counts["tombstones"],
        if activity["anomaly"] == true {
            "  interrupted or unresolved"
        } else {
            ""
        }
    ))
}
