//! Server-sent events down, POST up, specified by `openapi/link.json` rather than shared as
//! types with the control plane that serves it.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use reqwest::{Client, Response, StatusCode, header};
use serde::{Deserialize, Serialize};

pub const ANSWERS: &str = "/link/instances/{instance}/answers/{request}";
pub const CREDENTIALS: &str = "/link/instances/{instance}/credentials";
pub const INSTRUCTIONS: &str = "/link/instances/{instance}/instructions";
pub const REPORTS: &str = "/link/instances/{instance}/reports";

/// An Instance's name carries a `/`, and reaches the link as one path segment.
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
        harness: Harness,
    },
    /// Opens the conversation and prompts nothing: the Session waits unbriefed for its first
    /// message (ADR-0038).
    Unbriefed {
        checkout: Checkout,
        harness: Harness,
    },
    /// The next turn, in the conversation the Session's first one opened.
    Prompt {
        prompt: String,
    },
    /// Changes one of the Session's harness options before its next prompt (ADR-0041).
    SetOption {
        option: String,
        value: String,
        participant: String,
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
            Instruction::Unbriefed { .. } => "unbriefed",
            Instruction::Prompt { .. } => "prompt",
            Instruction::SetOption { .. } => "set_option",
            Instruction::Stop => "stop",
            Instruction::Unrecognized => "unrecognized",
        }
    }
}

/// A read of the Instance's checkouts. It carries no event id, so a reconnect never replays it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Asked {
    pub request: String,
    #[serde(flatten)]
    pub read: Read,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "read", rename_all = "snake_case")]
pub enum Read {
    Files {
        #[serde(default)]
        path: Option<String>,
    },
    File {
        path: String,
        #[serde(default)]
        raw: bool,
    },
    Changes {
        scope: String,
        paths: Vec<String>,
    },
    Commits,
    Stashes,
    #[serde(other)]
    Unrecognized,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "answer", rename_all = "snake_case")]
pub enum Answer {
    Listing {
        path: String,
        entries: Vec<Entry>,
        total: u64,
        truncated: bool,
    },
    Text {
        path: String,
        text: String,
    },
    Changes {
        repositories: Vec<RepositoryDiff>,
    },
    Commits {
        repositories: Vec<RepositoryText>,
    },
    Stashes {
        repositories: Vec<RepositoryText>,
    },
    Refused {
        message: String,
    },
    Missing {
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RepositoryDiff {
    pub repository: String,
    pub diff: String,
    pub files: Vec<FileStat>,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FileStat {
    pub path: String,
    pub added: Option<u64>,
    pub removed: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RepositoryText {
    pub repository: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Entry {
    pub name: String,
    pub kind: EntryKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git: Option<Tracking>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryKind {
    File,
    Directory,
    Symlink,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Tracking {
    Tracked,
    Untracked,
    Ignored,
}

pub enum AnswerBody {
    Json(Answer),
    Raw(reqwest::Body),
}

/// What to spawn for the Session, which may be another Agent's than the last Session's, and
/// what the Session declared for the harness's options.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Harness {
    pub command: String,
    #[serde(default)]
    pub auth: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub mode: Option<String>,
    #[serde(default)]
    pub thought_level: Option<String>,
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
    Connected {
        version: String,
    },
    Heartbeat,
    Work {
        repositories: Vec<WorkRepository>,
    },
    Stderr {
        lines: Vec<String>,
    },
    /// The harness is up and its conversation open, with no Turn started.
    Ready,
    Started,
    Model {
        model: String,
    },
    Said {
        message: String,
        completion: Completion,
    },
    Thought {
        text: String,
        completion: Completion,
    },
    Plan {
        entries: Vec<PlanEntry>,
        completion: Completion,
    },
    ToolCall {
        call_id: String,
        title: String,
        tool_kind: String,
        status: String,
        input: serde_json::Value,
        result: Box<serde_json::Value>,
        closing_reason: Option<String>,
        completion: Completion,
    },
    SessionState {
        tools: Vec<RunningTool>,
        message_buffering: bool,
        thought_buffering: bool,
    },
    Used {
        usage: Usage,
    },
    /// The Session's whole bookkeeping state, unnumbered and idempotent: a change is said once,
    /// and the whole state is said again after a reconnect (ADR-0041).
    SessionInfo(SessionInfo),
    /// The harness answered a person's option change, with the whole list it left or why it
    /// refused (ADR-0041).
    OptionChanged {
        participant: String,
        option: String,
        category: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        from: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        to: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        refused: Option<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        options: Vec<SessionOption>,
    },
    Answered,
    Checkout {
        repositories: Vec<Observed>,
    },
    Finished {
        exit: Exit,
    },
}

impl Report {
    pub const fn kind(&self) -> &'static str {
        match self {
            Report::Connected { .. } => "connected",
            Report::Heartbeat => "heartbeat",
            Report::Work { .. } => "work",
            Report::Stderr { .. } => "stderr",
            Report::Ready => "ready",
            Report::Started => "started",
            Report::Model { .. } => "model",
            Report::Said { .. } => "said",
            Report::Thought { .. } => "thought",
            Report::Plan { .. } => "plan",
            Report::ToolCall { .. } => "tool_call",
            Report::SessionState { .. } => "session_state",
            Report::Used { .. } => "used",
            Report::SessionInfo { .. } => "session_info",
            Report::OptionChanged { .. } => "option_changed",
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WorkRepository {
    pub repository: String,
    #[serde(flatten)]
    pub git: WorkGit,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "git", rename_all = "snake_case")]
pub enum WorkGit {
    Read {
        branch: Option<String>,
        changed: Changes,
        staged: Changes,
        committed: Commits,
        pushed: Option<String>,
        untracked: u64,
        stashed: u64,
    },
    Unreadable {
        because: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Changes {
    pub files: u64,
    pub added: u64,
    pub removed: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Commits {
    pub commits: u64,
    pub added: u64,
    pub removed: u64,
}

/// A report as it goes on the wire: the seq is what lets the control plane take it once
/// however many times a reconnect sends it, counted within the Session it names.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Reported<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session: Option<&'a str>,
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

/// One config option the harness offers, whole: its current value and every value it offers
/// (ADR-0041). A legacy harness that offers only `modes` is reported through a synthesized
/// option of the `mode` category, so a reader sees one shape.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionInfo {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub options: Vec<SessionOption>,
    #[serde(default)]
    pub commands: Vec<SessionCommand>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionOption {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(flatten)]
    pub kind: SessionOptionKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SessionOptionKind {
    Select {
        current: String,
        #[serde(default)]
        values: Vec<SessionOptionValue>,
        #[serde(default)]
        groups: Vec<SessionOptionGroup>,
    },
    Boolean {
        current: bool,
    },
}

impl SessionOption {
    pub fn is_category(&self, category: &str) -> bool {
        self.category.as_deref() == Some(category)
    }

    /// What the option is set to now.
    pub fn current_value(&self) -> Option<String> {
        match &self.kind {
            SessionOptionKind::Select { current, .. } => Some(current.clone()),
            SessionOptionKind::Boolean { current } => Some(current.to_string()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionOptionValue {
    pub value: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionOptionGroup {
    pub group: String,
    pub name: String,
    pub values: Vec<SessionOptionValue>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionCommand {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub input_hint: Option<String>,
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
pub enum Down {
    Instruction(Delivered),
    Read(Asked),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Delivered {
    pub id: String,
    pub session: String,
    pub instruction: Instruction,
}

#[derive(Deserialize)]
struct Carried {
    session: String,
    #[serde(flatten)]
    instruction: Instruction,
}

#[derive(Debug)]
pub enum Error {
    /// The link declined this Instance, or what it sent. Sending it again will not help.
    Refused(String),
    /// The link takes nothing more about the Session, which is let go; the Instance stays on it.
    Session(String),
    Lost(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Refused(why) | Error::Session(why) | Error::Lost(why) => out.write_str(why),
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
    instance: String,
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
    pub fn to(base: &str, instance: &str, credential: &str) -> Self {
        Self {
            client: Client::new(),
            base: base.to_owned(),
            instance: instance.to_owned(),
            credential: credential.to_owned(),
            reached: Reached::now(),
        }
    }

    /// How long it has been since the link last answered, which is how long a supervisor has been
    /// cut off from its control plane.
    pub(crate) fn unreached_for(&self) -> Duration {
        self.reached.elapsed()
    }

    pub async fn connected(&self) -> Result<Option<Checkout>, Error> {
        let response = self
            .post_report(
                &Report::Connected {
                    version: env!("CARGO_PKG_VERSION").to_owned(),
                },
                None,
                None,
            )
            .await?;
        let body = response.bytes().await?;
        if body.is_empty() {
            return Ok(None);
        }
        serde_json::from_slice(&body)
            .map(Some)
            .map_err(|error| Error::Lost(error.to_string()))
    }

    pub async fn report(
        &self,
        report: &Report,
        session: Option<&str>,
        seq: Option<i64>,
    ) -> Result<(), Error> {
        self.post_report(report, session, seq).await?;
        Ok(())
    }

    async fn post_report(
        &self,
        report: &Report,
        session: Option<&str>,
        seq: Option<i64>,
    ) -> Result<Response, Error> {
        let response = self
            .client
            .post(self.url(REPORTS))
            .bearer_auth(&self.credential)
            .json(&Reported {
                session,
                seq,
                report,
            })
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

        Ok(response)
    }

    pub async fn credentials(&self, session: &str) -> Result<Credentials, Error> {
        let response = self
            .client
            .get(self.about(CREDENTIALS, session))
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

    pub async fn refresh(
        &self,
        session: &str,
        files: &BTreeMap<String, String>,
    ) -> Result<(), Error> {
        let response = self
            .client
            .patch(self.about(CREDENTIALS, session))
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

    pub async fn answer(&self, request: &str, body: AnswerBody) -> Result<(), Error> {
        let request = utf8_percent_encode(request, SEGMENT).to_string();
        let sending = self
            .client
            .post(self.url(ANSWERS).replace("{request}", &request))
            .bearer_auth(&self.credential);
        let sending = match body {
            AnswerBody::Json(answer) => sending.json(&answer),
            AnswerBody::Raw(raw) => sending
                .header(header::CONTENT_TYPE, "application/octet-stream")
                .body(raw),
        };

        let response = refuse_if_declined(sending.send().await?).await?;
        if !response.status().is_success() {
            return Err(Error::Lost(format!(
                "the link answered {} to the answer to a read",
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
        let instance = utf8_percent_encode(&self.instance, SEGMENT).to_string();

        format!("{}{}", self.base, path.replace("{instance}", &instance))
    }

    fn about(&self, path: &str, session: &str) -> String {
        let session = utf8_percent_encode(session, SEGMENT);

        format!("{}?session={session}", self.url(path))
    }
}

impl Instructions {
    pub async fn next(&mut self) -> Result<Option<Down>, Error> {
        loop {
            if let Some((id, data)) = self.take_frame() {
                let Some(id) = id else {
                    let asked = serde_json::from_str(&data).map_err(|error| {
                        Error::Lost(format!(
                            "the stream carried {data}, which is not a read: {error}"
                        ))
                    })?;
                    self.reached.touched();
                    return Ok(Some(Down::Read(asked)));
                };
                let Carried {
                    session,
                    instruction,
                } = serde_json::from_str(&data).map_err(|error| {
                    Error::Lost(format!(
                        "the stream carried {data}, which is not an instruction: {error}"
                    ))
                })?;
                self.reached.touched();

                return Ok(Some(Down::Instruction(Delivered {
                    id,
                    session,
                    instruction,
                })));
            }

            match self.response.chunk().await? {
                Some(chunk) => self.buffered.extend_from_slice(&chunk),
                None => return Ok(None),
            }
        }
    }

    /// A frame is everything up to a blank line; the keep-alive is a frame with only a comment.
    fn take_frame(&mut self) -> Option<(Option<String>, String)> {
        loop {
            let end = self.buffered.windows(2).position(|pair| pair == b"\n\n")?;
            let frame: Vec<u8> = self.buffered.drain(..end + 2).collect();
            let frame = String::from_utf8_lossy(&frame);

            let mut id = None;
            let mut event = None;
            let mut data = String::new();
            for line in frame.lines() {
                if let Some(carried) = line.strip_prefix("id:") {
                    id = Some(carried.trim().to_owned());
                } else if let Some(carried) = line.strip_prefix("event:") {
                    event = Some(carried.trim().to_owned());
                } else if let Some(carried) = line.strip_prefix("data:") {
                    data.push_str(carried.trim());
                }
            }

            if data.is_empty() {
                continue;
            }
            if id.is_some() || event.as_deref() == Some("read") {
                return Some((id, data));
            }
        }
    }
}

async fn refuse_if_declined(response: Response) -> Result<Response, Error> {
    match response.status() {
        StatusCode::GONE | StatusCode::BAD_REQUEST => Err(Error::Session(response.text().await?)),
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN | StatusCode::NOT_FOUND => {
            Err(Error::Refused(response.text().await?))
        }
        _ => Ok(response),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Completion {
    pub started_at: jiff::Timestamp,
    pub finished_at: jiff::Timestamp,
    pub turn_outcome: Option<TurnOutcome>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PlanEntry {
    pub content: String,
    pub priority: String,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum TurnOutcome {
    Answered { stop_reason: String },
    Cancelled,
    Failed { because: String },
}

impl Completion {
    pub fn at(now: jiff::Timestamp) -> Self {
        Self {
            started_at: now,
            finished_at: now,
            turn_outcome: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct RunningTool {
    pub call_id: String,
    pub title: String,
    pub status: String,
    pub started_at: jiff::Timestamp,
}
