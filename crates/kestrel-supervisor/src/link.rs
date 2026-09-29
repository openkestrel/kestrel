//! Server-sent events down, POST up, specified by `openapi/link.json` rather than shared as
//! types with the control plane that serves it.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use reqwest::{Client, Response, StatusCode, header};
use serde::{Deserialize, Serialize};

pub const CREDENTIALS: &str = "/link/sessions/{session}/credentials";
pub const INSTRUCTIONS: &str = "/link/sessions/{session}/instructions";
pub const REPORTS: &str = "/link/sessions/{session}/reports";

/// A session reaches the link as one path segment, whatever the Environment was handed.
const SEGMENT: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'~');

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Instruction {
    Start {
        checkout: Checkout,
        prompt: String,
    },
    /// The next turn, in the conversation the Session's first one opened.
    Prompt {
        prompt: String,
    },
    Stop,
    /// A control plane kestrel upgraded under a live Environment (ADR-0002) may send an
    /// instruction this supervisor predates; letting it past keeps the cursor moving.
    #[serde(other)]
    Unrecognized,
}

impl Instruction {
    pub const fn kind(&self) -> &'static str {
        match self {
            Instruction::Start { .. } => "start",
            Instruction::Prompt { .. } => "prompt",
            Instruction::Stop => "stop",
            Instruction::Unrecognized => "unrecognized",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Checkout {
    pub repositories: Vec<String>,
    pub base: String,
    pub branch: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Report {
    Connected { version: String },
    Heartbeat,
    Stderr { lines: Vec<String> },
    Started,
    Model { model: String },
    Said { message: String },
    Used { usage: Usage },
    Answered,
    Checkout { repositories: Vec<Observed> },
    Finished { exit: Exit },
}

impl Report {
    pub const fn kind(&self) -> &'static str {
        match self {
            Report::Connected { .. } => "connected",
            Report::Heartbeat => "heartbeat",
            Report::Stderr { .. } => "stderr",
            Report::Started => "started",
            Report::Model { .. } => "model",
            Report::Said { .. } => "said",
            Report::Used { .. } => "used",
            Report::Answered => "answered",
            Report::Checkout { .. } => "checkout",
            Report::Finished { .. } => "finished",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Observed {
    pub repository: String,
    #[serde(flatten)]
    pub git: Git,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "git", rename_all = "snake_case")]
pub enum Git {
    Read {
        branch: Option<String>,
        untracked: u64,
        uncommitted: u64,
        stashes: u64,
        unpushed: u64,
    },
    Unreadable {
        because: String,
    },
}

/// A report as it goes on the wire: the seq is what lets the control plane take it once
/// however many times a reconnect sends it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Reported<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seq: Option<i64>,
    #[serde(flatten)]
    pub report: &'a Report,
}

/// How the work this Environment was provisioned for went. Everything the control plane makes
/// of it is the control plane's business.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Exit {
    Succeeded,
    Failed { because: String },
}

/// What the agent has spent so far, cumulative rather than per turn.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Usage {
    pub context_used: u64,
    pub context_size: u64,
    pub cost: Option<Cost>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Cost {
    pub amount: f64,
    pub currency: String,
}

/// What the Harness is spawned with to reach a model: variables for its environment, and
/// a Subscription Profile's files for beneath its home.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Credentials {
    pub variables: BTreeMap<String, String>,
    #[serde(default)]
    pub files: BTreeMap<String, String>,
}

#[derive(Serialize)]
struct Refreshed<'a> {
    files: &'a BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Delivered {
    pub id: String,
    pub instruction: Instruction,
}

#[derive(Debug)]
pub enum Error {
    /// The link declined this Environment, or what it sent. Sending it again will not help.
    Refused(String),
    Lost(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Refused(why) | Error::Lost(why) => out.write_str(why),
        }
    }
}

impl std::error::Error for Error {}

impl From<reqwest::Error> for Error {
    fn from(error: reqwest::Error) -> Self {
        Error::Lost(error.to_string())
    }
}

pub struct Link {
    client: Client,
    base: String,
    session: String,
    credential: String,
    reached: Reached,
}

pub struct Instructions {
    response: Response,
    buffered: Vec<u8>,
    reached: Reached,
}

/// When the link last answered anything, so a supervisor nothing is answering can tell how long
/// it has been unreachable rather than reconnecting forever. Shared with a stream, because an
/// instruction arriving is an exchange with the link too.
#[derive(Clone)]
struct Reached(Arc<Mutex<Instant>>);

impl Reached {
    fn now() -> Self {
        Self(Arc::new(Mutex::new(Instant::now())))
    }

    fn touched(&self) {
        *self
            .0
            .lock()
            .expect("when the link was last reached should not be poisoned") = Instant::now();
    }

    fn elapsed(&self) -> Duration {
        self.0
            .lock()
            .expect("when the link was last reached should not be poisoned")
            .elapsed()
    }
}

impl Link {
    pub fn to(base: &str, session: &str, credential: &str) -> Self {
        Self {
            client: Client::new(),
            base: base.to_owned(),
            session: session.to_owned(),
            credential: credential.to_owned(),
            reached: Reached::now(),
        }
    }

    /// How long it has been since the link last answered, which is how long a supervisor has been
    /// cut off from its control plane.
    pub(crate) fn unreached_for(&self) -> Duration {
        self.reached.elapsed()
    }

    pub async fn report(&self, report: &Report, seq: Option<i64>) -> Result<(), Error> {
        let response = self
            .client
            .post(self.url(REPORTS))
            .bearer_auth(&self.credential)
            .json(&Reported { seq, report })
            .send()
            .await?;

        let response = refuse_if_declined(response).await?;
        if !response.status().is_success() {
            return Err(Error::Lost(format!(
                "the link answered {} to a report",
                response.status().as_u16()
            )));
        }
        self.reached.touched();

        Ok(())
    }

    pub async fn credentials(&self) -> Result<Credentials, Error> {
        let response = self
            .client
            .get(self.url(CREDENTIALS))
            .bearer_auth(&self.credential)
            .send()
            .await?;

        let response = refuse_if_declined(response).await?;
        if !response.status().is_success() {
            return Err(Error::Lost(format!(
                "the link answered {} to a request for this session's credentials",
                response.status().as_u16()
            )));
        }
        self.reached.touched();

        Ok(response.json().await?)
    }

    pub async fn refresh(&self, files: &BTreeMap<String, String>) -> Result<(), Error> {
        let response = self
            .client
            .patch(self.url(CREDENTIALS))
            .bearer_auth(&self.credential)
            .json(&Refreshed { files })
            .send()
            .await?;

        let response = refuse_if_declined(response).await?;
        if !response.status().is_success() {
            return Err(Error::Lost(format!(
                "the link answered {} to the logins this session refreshed",
                response.status().as_u16()
            )));
        }
        self.reached.touched();

        Ok(())
    }

    pub async fn open(&self, cursor: Option<&str>) -> Result<Instructions, Error> {
        let mut request = self
            .client
            .get(self.url(INSTRUCTIONS))
            .bearer_auth(&self.credential)
            .header(header::ACCEPT, "text/event-stream");
        if let Some(cursor) = cursor {
            request = request.header("last-event-id", cursor);
        }

        let response = refuse_if_declined(request.send().await?).await?;
        if !response.status().is_success() {
            return Err(Error::Lost(format!(
                "the link answered {} to a stream",
                response.status().as_u16()
            )));
        }
        self.reached.touched();

        Ok(Instructions {
            response,
            buffered: Vec::new(),
            reached: self.reached.clone(),
        })
    }

    fn url(&self, path: &str) -> String {
        let session = utf8_percent_encode(&self.session, SEGMENT).to_string();

        format!("{}{}", self.base, path.replace("{session}", &session))
    }
}

impl Instructions {
    pub async fn next(&mut self) -> Result<Option<Delivered>, Error> {
        loop {
            if let Some((id, data)) = self.take_frame() {
                let instruction = serde_json::from_str(&data).map_err(|error| {
                    Error::Lost(format!(
                        "the stream carried {data}, which is not an instruction: {error}"
                    ))
                })?;
                self.reached.touched();

                return Ok(Some(Delivered { id, instruction }));
            }

            match self.response.chunk().await? {
                Some(chunk) => self.buffered.extend_from_slice(&chunk),
                None => return Ok(None),
            }
        }
    }

    /// A frame is everything up to a blank line; the keep-alive is a frame with only a comment.
    fn take_frame(&mut self) -> Option<(String, String)> {
        loop {
            let end = self.buffered.windows(2).position(|pair| pair == b"\n\n")?;
            let frame: Vec<u8> = self.buffered.drain(..end + 2).collect();
            let frame = String::from_utf8_lossy(&frame);

            let mut id = None;
            let mut data = String::new();
            for line in frame.lines() {
                if let Some(carried) = line.strip_prefix("id:") {
                    id = Some(carried.trim().to_owned());
                } else if let Some(carried) = line.strip_prefix("data:") {
                    data.push_str(carried.trim());
                }
            }

            if let Some(id) = id
                && !data.is_empty()
            {
                return Some((id, data));
            }
        }
    }
}

async fn refuse_if_declined(response: Response) -> Result<Response, Error> {
    match response.status() {
        StatusCode::BAD_REQUEST
        | StatusCode::UNAUTHORIZED
        | StatusCode::FORBIDDEN
        | StatusCode::NOT_FOUND => Err(Error::Refused(response.text().await?)),
        _ => Ok(response),
    }
}
