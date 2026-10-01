//! kestrel as an ACP client (ADR-0007): no contract of kestrel's, and no branch on which
//! Harness is on the other end of one.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::str::FromStr as _;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use agent_client_protocol::schema::MaybeUndefined;
use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::schema::v1::{
    AgentCapabilities, AuthMethod, AuthenticateRequest, AvailableCommand, AvailableCommandInput,
    CancelNotification, ContentBlock, ErrorCode, InitializeRequest, InitializeResponse,
    LoadSessionRequest, NewSessionRequest, PromptRequest, RequestPermissionOutcome,
    RequestPermissionRequest, RequestPermissionResponse, ResumeSessionRequest,
    SelectedPermissionOutcome, SessionConfigId, SessionConfigKind, SessionConfigOption,
    SessionConfigOptionCategory, SessionConfigSelect, SessionConfigSelectOption,
    SessionConfigSelectOptions, SessionConfigValueId, SessionId, SessionModeId, SessionModeState,
    SessionNotification, SessionUpdate, SetSessionConfigOptionRequest, SetSessionModeRequest,
    StopReason, TextContent,
};
use agent_client_protocol::{
    AcpAgent, AcpAgentConfig, Client, ConnectionTo, Error, LineDirection,
    is_incoming_transport_closed,
};

use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::completer::{Completed, Completer};
use crate::link::{
    Report, SessionCommand, SessionInfo, SessionOption, SessionOptionGroup, SessionOptionKind,
    SessionOptionValue, TurnOutcome,
};
use crate::permission::{self, Subject};

/// What this Environment was configured to drive, what the Session asks of it, and where what the
/// agent writes to stderr goes. Which Harness is on the other end is the configuration's
/// business, never this module's.
#[derive(Debug, Clone)]
pub struct Harness {
    pub command: String,
    /// The ACP authentication method to log the agent in with, for an agent that requires one.
    pub auth: Option<String>,
    /// The model the Session named, if it named one.
    pub model: Option<String>,
    /// The mode the Session named, if it named one.
    pub mode: Option<String>,
    /// The thought level the Session named, if it named one.
    pub thought_level: Option<String>,
    /// How long an interrupted Turn is given to answer its cancel before the harness is ended.
    pub interrupt_deadline: Duration,
    pub stderr: mpsc::UnboundedSender<String>,
}

/// Long enough for any log line worth reading, and short enough that a runaway one costs the
/// link little.
const LINE_LIMIT: usize = 4 * 1024;

/// One turn's account. `failed` says why the conversation is over; without it, the agent
/// answered and waits for another prompt.
pub struct Worked {
    pub allowed: Vec<Subject>,
    pub on: Option<On>,
    pub failed: Option<String>,
}

/// Which model the agent works the turn on — the one its Session named, or the one the harness
/// defaults to when it named none.
pub struct On {
    pub model: String,
}

/// The categories a Session declared, each a harness value id (ADR-0041). A category named none
/// for stays at whatever the harness already offers.
#[derive(Debug, Default, Clone, Copy)]
struct Declared<'a> {
    model: Option<&'a str>,
    mode: Option<&'a str>,
    thought_level: Option<&'a str>,
}

impl Harness {
    fn declared(&self) -> Declared<'_> {
        Declared {
            model: self.model.as_deref(),
            mode: self.mode.as_deref(),
            thought_level: self.thought_level.as_deref(),
        }
    }
}

/// What a Session's declaration asks the agent to set, decided before anything is sent: one
/// entry per category that differs from what the harness already offers.
struct Planned {
    choices: Vec<Choice>,
    /// The model the harness is on, whether or not this Session named one, for the report.
    on: Option<On>,
}

enum Choice {
    /// A config option the harness categorized as this.
    Option { id: SessionConfigId, value: String },
    /// A legacy mode, which has no Mode-category option to set.
    Mode(String),
}

/// Long enough for an agent between turns to see its connection close; one mid-turn is cut off.
const CLOSING: Duration = Duration::from_millis(500);

/// The channels one conversation's task listens on: the next prompt, and a request to cancel the
/// Turn in flight.
struct Channels {
    prompts: mpsc::UnboundedReceiver<String>,
    interrupts: mpsc::UnboundedReceiver<()>,
}

/// One ACP conversation for the whole Session, held apart from the link so that losing the link
/// loses nothing of it (ADR-0024). Dropping it kills the agent.
pub struct Conversation {
    prompts: mpsc::UnboundedSender<String>,
    interrupts: mpsc::UnboundedSender<()>,
    turns: mpsc::UnboundedReceiver<ConversationEvent>,
    task: JoinHandle<()>,
}

impl Conversation {
    /// `provider` reaches the agent's own process and nothing else: not this one's
    /// environment, not a file, and not ACP, which carries no credentials (ADR-0007).
    pub fn open(
        harness: &Harness,
        provider: BTreeMap<String, String>,
        first: Option<String>,
        root: PathBuf,
    ) -> Self {
        let (prompts, prompted) = mpsc::unbounded_channel();
        let (interrupts, interrupted) = mpsc::unbounded_channel();
        let (answered, turns) = mpsc::unbounded_channel();
        if let Some(first) = first {
            prompts
                .send(first)
                .expect("the conversation has not started, so nothing has hung up on it");
        }
        let task = tokio::spawn(conversing(
            harness.clone(),
            provider,
            root,
            Channels {
                prompts: prompted,
                interrupts: interrupted,
            },
            answered,
        ));

        Self {
            prompts,
            interrupts,
            turns,
            task,
        }
    }

    pub fn prompt(&self, prompt: String) {
        // A conversation that is over says so as its last turn, which is where that is heard.
        let _ = self.prompts.send(prompt);
    }

    /// Asks the agent to cancel the Turn in flight; the conversation goes on.
    pub fn interrupt(&self) {
        let _ = self.interrupts.send(());
    }

    /// Cancel-safe, so a caller may stop waiting on it and come back.
    pub fn try_next(&mut self) -> Option<ConversationEvent> {
        self.turns.try_recv().ok()
    }

    pub async fn next(&mut self) -> ConversationEvent {
        self.turns.recv().await.unwrap_or_else(|| {
            ConversationEvent::Worked(Worked {
                allowed: Vec::new(),
                on: None,
                failed: Some("the agent conversation ended unannounced".to_owned()),
            })
        })
    }

    /// Waited out, because the agent runs in a process group of its own that only its
    /// connection closing kills, and an exit that beats that leaves the agent behind.
    pub async fn end(mut self) {
        let (hung_up, _) = mpsc::unbounded_channel();
        drop(std::mem::replace(&mut self.prompts, hung_up));
        if tokio::time::timeout(CLOSING, &mut self.task).await.is_err() {
            self.task.abort();
            let _ = (&mut self.task).await;
        }
    }
}

impl Drop for Conversation {
    fn drop(&mut self) {
        self.task.abort();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Recovery {
    Resume,
    Load,
}

impl Recovery {
    /// Resume is preferred because it goes on without replaying the conversation back.
    fn offered(capabilities: &AgentCapabilities) -> Option<Self> {
        if capabilities.session_capabilities.resume.is_some() {
            return Some(Self::Resume);
        }

        capabilities.load_session.then_some(Self::Load)
    }
}

#[derive(Default)]
struct Continuity {
    conversed: Option<SessionId>,
    recovery: Option<Recovery>,
    in_flight: Option<String>,
    /// Cleared by an answered turn, so an agent that dies as soon as it is brought back is not
    /// brought back forever.
    recovered: bool,
}

impl Continuity {
    fn opened(&mut self, conversed: SessionId, recovery: Option<Recovery>) {
        self.conversed = Some(conversed);
        self.recovery = recovery;
    }

    fn answered(&mut self) {
        self.in_flight = None;
        self.recovered = false;
    }

    /// Never `session/new` again: a conversation standing in for the lost one would be passed
    /// off as its continuation (ADR-0024).
    fn recovering(&mut self, lost: String) -> Result<(), String> {
        if self.conversed.is_none() {
            return Err(lost);
        }
        if self.recovery.is_none() {
            return Err(format!(
                "the agent's process was lost ({lost}), and its harness cannot resume the \
                 conversation it held"
            ));
        }
        if self.recovered {
            return Err(format!(
                "the agent's process was lost again ({lost}), with no turn answered since it \
                 was brought back"
            ));
        }
        self.recovered = true;

        Ok(())
    }
}

enum Ended {
    HungUp,
    Over(String),
    Lost(String),
}

/// Everything that can go wrong here ends the conversation, and is its last turn.
async fn conversing(
    harness: Harness,
    provider: BTreeMap<String, String>,
    root: PathBuf,
    mut channels: Channels,
    turns: mpsc::UnboundedSender<ConversationEvent>,
) {
    let heard = Arc::new(Mutex::new(Hearing {
        completer: Completer::default(),
        allowed: Vec::new(),
        on: None,
        interrupting: false,
        reports: turns.clone(),
        diagnostics: harness.stderr.clone(),
        replaying: false,
        info: Held::default(),
        announced: None,
    }));
    let mut continuity = Continuity::default();

    let because = loop {
        let lost = match living(
            &harness,
            &provider,
            &root,
            &mut channels,
            &turns,
            &heard,
            &mut continuity,
        )
        .await
        {
            Ok(Ended::HungUp) => return,
            Ok(Ended::Over(because)) => break because,
            Ok(Ended::Lost(because)) => because,
            Err(error) => described(&error),
        };
        match continuity.recovering(lost.clone()) {
            Ok(()) => {
                let _ = harness.stderr.send(format!(
                    "kestrel: the agent's process was lost ({lost}); bringing it back into its \
                     conversation"
                ));
            }
            Err(because) => break because,
        }
    };
    let _ = turns.send(ConversationEvent::Worked(
        heard
            .lock()
            .expect("the observation lock")
            .worked(Some(because)),
    ));
}

/// An `Err` is the connection itself failing, which is a loss.
async fn living(
    harness: &Harness,
    provider: &BTreeMap<String, String>,
    root: &Path,
    channels: &mut Channels,
    turns: &mpsc::UnboundedSender<ConversationEvent>,
    heard: &Arc<Mutex<Hearing>>,
    continuity: &mut Continuity,
) -> Result<Ended, Error> {
    let spawn = match AcpAgent::from_str(&harness.command) {
        Ok(spawn) => spawn,
        Err(error) => {
            return Ok(Ended::Over(format!(
                "the harness {:?} could not be spawned: {error}",
                harness.command
            )));
        }
    };
    let stderr = harness.stderr.clone();
    let config = spawn.into_config();
    let inherited = std::env::vars_os()
        .filter_map(|(name, _)| name.into_string().ok())
        .chain(config.environment().keys().cloned())
        .filter(|name| name.starts_with("KESTREL_"))
        .collect::<BTreeSet<_>>();
    // ACP's launcher cannot remove inherited variables, so env removes them before execing the harness.
    let mut clean = AcpAgentConfig::new("env");
    for name in inherited {
        clean = clean.arg("-u").arg(name);
    }
    let clean = clean
        .arg(config.command().to_string_lossy().into_owned())
        .args(config.arguments().iter().cloned())
        .envs(config.environment().clone())
        .envs(provider.clone());
    let spawn = AcpAgent::new(clean).with_debug(move |line, direction| {
        if direction == LineDirection::Stderr {
            let _ = stderr.send(bounded(line));
        }
    });

    Client
        .builder()
        .name("kestrel")
        .on_receive_notification(
            {
                let heard = Arc::clone(heard);
                async move |notification: SessionNotification, _connection| {
                    heard
                        .lock()
                        .expect("what the agent said should not be poisoned")
                        .update(notification.update);
                    Ok(())
                }
            },
            agent_client_protocol::on_receive_notification!(),
        )
        .on_receive_request(
            {
                let heard = Arc::clone(heard);
                async move |request: RequestPermissionRequest, responder, _connection| {
                    let mut heard = heard
                        .lock()
                        .expect("what the agent said should not be poisoned");
                    heard.completer.produced = true;
                    // A request that arrives while the Turn is being cancelled is answered
                    // `cancelled`, which is not a denial: the agent is stopping anyway.
                    let outcome = if heard.interrupting {
                        RequestPermissionOutcome::Cancelled
                    } else {
                        match permission::allow_once(&request.options) {
                            Some(option) => {
                                heard.allowed.push(Subject::from(&request));
                                RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(
                                    option,
                                ))
                            }
                            None => RequestPermissionOutcome::Cancelled,
                        }
                    };
                    drop(heard);

                    responder.respond(RequestPermissionResponse::new(outcome))
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .connect_with(
            spawn,
            async |connection: ConnectionTo<agent_client_protocol::Agent>| {
                let conversed = match (continuity.conversed.clone(), continuity.recovery) {
                    (Some(conversed), Some(recovery)) => {
                        if let Err(error) =
                            recover(&connection, harness, root, heard, &conversed, recovery).await
                        {
                            return Ok(ended(&error));
                        }
                        let _ = turns.send(ConversationEvent::Ready);
                        conversed
                    }
                    _ => match set_up(&connection, harness, root, heard).await {
                        Ok((conversed, recovery)) => {
                            continuity.opened(conversed.clone(), recovery);
                            let _ = turns.send(ConversationEvent::Ready);
                            conversed
                        }
                        Err(error) => return Ok(ended(&error)),
                    },
                };

                loop {
                    let prompt = match continuity.in_flight.clone() {
                        Some(prompt) => prompt,
                        None => tokio::select! {
                            prompt = channels.prompts.recv() => match prompt {
                                Some(prompt) => prompt,
                                None => return Ok(Ended::HungUp),
                            },
                            () = connection.incoming_closed() => {
                                return Ok(Ended::Lost(
                                    "it closed its connection between turns".to_owned(),
                                ));
                            }
                        },
                    };
                    continuity.in_flight = Some(prompt.clone());
                    heard
                        .lock()
                        .expect("the observation lock")
                        .completer
                        .begin();
                    let mut prompting = Box::pin(
                        connection
                            .send_request(PromptRequest::new(
                                conversed.clone(),
                                vec![ContentBlock::Text(TextContent::new(prompt))],
                            ))
                            .block_task(),
                    );
                    let mut cancelled = false;
                    let mut give_up_at = None;
                    let answered = loop {
                        tokio::select! {
                            answered = prompting.as_mut() => break answered,
                            Some(()) = channels.interrupts.recv(), if !cancelled => {
                                connection
                                    .send_notification(CancelNotification::new(conversed.clone()))?;
                                heard.lock().expect("the observation lock").interrupting = true;
                                cancelled = true;
                                give_up_at = Some(tokio::time::Instant::now() + harness.interrupt_deadline);
                            }
                            () = until(give_up_at) => {
                                let mut heard = heard.lock().expect("the observation lock");
                                heard.interrupting = false;
                                // `Answered` is the boundary that closes open units
                                // `unresolved`; the exit below says why the Turn never came back.
                                let completed = heard.completer.boundary(
                                    TurnOutcome::Answered {
                                        stop_reason: "cancelled".to_owned(),
                                    },
                                    jiff::Timestamp::now(),
                                );
                                heard.emit(completed);
                                drop(heard);
                                return Ok(Ended::Over(format!(
                                    "the agent did not answer the interrupt within {:?}, and its ACP \
                                     continuity is lost",
                                    harness.interrupt_deadline
                                )));
                            }
                        }
                    };
                    // The Turn is over: nothing queued after it belongs to it.
                    while channels.interrupts.try_recv().is_ok() {}
                    heard.lock().expect("the observation lock").interrupting = false;
                    let answered = match answered {
                        Ok(answered) => answered,
                        Err(error) => return Ok(ended(&error)),
                    };
                    continuity.answered();

                    if let Some(because) = stopped_short(answered.stop_reason) {
                        if answered.stop_reason == StopReason::Cancelled {
                            let mut heard = heard.lock().expect("the observation lock");
                            let completed = heard
                                .completer
                                .boundary(TurnOutcome::Cancelled, jiff::Timestamp::now());
                            heard.emit(completed);
                            if cancelled {
                                drop(heard);
                                // kestrel asked for this one: the conversation and the Instance
                                // stay, and the Turn is reported interrupted rather than ended.
                                let _ = turns.send(ConversationEvent::Interrupted);
                                continue;
                            }
                        }
                        return Ok(Ended::Over(because));
                    }
                    let mut this_turn = heard.lock().expect("the observation lock");
                    let failed = (!this_turn.completer.produced)
                        .then(|| "the agent answered the prompt with nothing".to_owned());
                    let worked = this_turn.worked(failed);
                    drop(this_turn);
                    if turns.send(ConversationEvent::Worked(worked)).is_err() {
                        return Ok(Ended::HungUp);
                    }
                }
            },
        )
        .await
}

fn ended(error: &Error) -> Ended {
    match is_incoming_transport_closed(error) {
        true => Ended::Lost(described(error)),
        false => Ended::Over(error.to_string()),
    }
}

/// What the agent's end of a lost connection said, without the library's own bookkeeping
/// around it.
fn described(error: &Error) -> String {
    let data = error.data.as_ref();
    data.and_then(|data| data.get("data"))
        .or(data)
        .and_then(serde_json::Value::as_str)
        .map_or_else(|| error.to_string(), str::to_owned)
}

async fn initialized(
    connection: &ConnectionTo<agent_client_protocol::Agent>,
    auth: Option<&str>,
) -> Result<InitializeResponse, Error> {
    let initialized = connection
        .send_request(InitializeRequest::new(ProtocolVersion::V1))
        .block_task()
        .await?;
    if initialized.protocol_version != ProtocolVersion::V1 {
        return Err(Error::internal_error().data(format!(
            "kestrel speaks ACP v1, and this agent answered v{}",
            initialized.protocol_version
        )));
    }
    if needs_a_human_at_a_terminal(&initialized.auth_methods) {
        return Err(Error::internal_error().data(
            "this agent authenticates only at an interactive terminal, and nobody is at one",
        ));
    }

    if let Some(method) = auth {
        if !initialized
            .auth_methods
            .iter()
            .any(|offered| offered.id().0.as_ref() == method)
        {
            return Err(Error::internal_error().data(format!(
                "kestrel is configured to log this agent in with {method:?}, and it offers {}",
                offered(&initialized.auth_methods)
            )));
        }
        connection
            .send_request(AuthenticateRequest::new(method.to_owned()))
            .block_task()
            .await?;
    }

    Ok(initialized)
}

/// Everything kestrel asks of an agent before its first prompt, in the order ACP has a client
/// ask it.
async fn set_up(
    connection: &ConnectionTo<agent_client_protocol::Agent>,
    harness: &Harness,
    root: &Path,
    heard: &Mutex<Hearing>,
) -> Result<(SessionId, Option<Recovery>), Error> {
    let initialized = initialized(connection, harness.auth.as_deref()).await?;

    let set_up = connection
        .send_request(NewSessionRequest::new(root))
        .block_task()
        .await
        .map_err(|error| unlogged_in(error, &initialized.auth_methods))?;

    let selected = select(
        connection,
        &set_up.session_id,
        set_up.config_options.as_deref(),
        set_up.modes.as_ref(),
        &harness.declared(),
    )
    .await?;
    let mut heard = heard.lock().expect("the observation lock");
    heard.hold(selected.options.or(set_up.config_options), set_up.modes);
    heard.model(selected.on);
    heard.mode(selected.mode);

    Ok((
        set_up.session_id,
        Recovery::offered(&initialized.agent_capabilities),
    ))
}

// Loaded history was already reported and must not be sent again.
async fn recover(
    connection: &ConnectionTo<agent_client_protocol::Agent>,
    harness: &Harness,
    root: &Path,
    heard: &Mutex<Hearing>,
    conversed: &SessionId,
    recovery: Recovery,
) -> Result<(), Error> {
    initialized(connection, harness.auth.as_deref()).await?;
    {
        let mut heard = heard.lock().expect("the observation lock");
        let completed = heard.completer.boundary(
            TurnOutcome::Failed {
                because: "the harness connection was lost".to_owned(),
            },
            jiff::Timestamp::now(),
        );
        heard.emit(completed);
        heard.replaying = true;
    }

    let (config_options, modes) = match recovery {
        Recovery::Resume => {
            let resumed = connection
                .send_request(ResumeSessionRequest::new(conversed.clone(), root))
                .block_task()
                .await?;
            (resumed.config_options, resumed.modes)
        }
        Recovery::Load => {
            let loaded = connection
                .send_request(LoadSessionRequest::new(conversed.clone(), root))
                .block_task()
                .await?;
            (loaded.config_options, loaded.modes)
        }
    };
    {
        let mut heard = heard
            .lock()
            .expect("what the agent said should not be poisoned");
        heard.replaying = false;
    }

    let selected = select(
        connection,
        conversed,
        config_options.as_deref(),
        modes.as_ref(),
        &harness.declared(),
    )
    .await?;
    let mut heard = heard.lock().expect("the observation lock");
    heard.hold(selected.options.or(config_options), modes);
    heard.mode(selected.mode);

    Ok(())
}

/// What a `session/new`, `session/load` or `session/resume` led to once the declared categories,
/// if any, were selected: the agent's state and the list its answer carried.
struct Selected {
    on: Option<On>,
    options: Option<Vec<SessionConfigOption>>,
    /// A legacy mode set through `session/set_mode`.
    mode: Option<SessionModeId>,
}

async fn select(
    connection: &ConnectionTo<agent_client_protocol::Agent>,
    conversed: &SessionId,
    offered: Option<&[SessionConfigOption]>,
    modes: Option<&SessionModeState>,
    declared: &Declared<'_>,
) -> Result<Selected, Error> {
    let planned = planned(offered.unwrap_or_default(), modes, declared)?;
    let mut options = None;
    let mut mode = None;

    for choice in planned.choices {
        match choice {
            Choice::Option { id, value, .. } => {
                let answered = connection
                    .send_request(SetSessionConfigOptionRequest::new(
                        conversed.clone(),
                        id,
                        SessionConfigValueId::new(value),
                    ))
                    .block_task()
                    .await?;
                options = Some(answered.config_options);
            }
            Choice::Mode(value) => {
                connection
                    .send_request(SetSessionModeRequest::new(
                        conversed.clone(),
                        SessionModeId::new(value.clone()),
                    ))
                    .block_task()
                    .await?;
                mode = Some(SessionModeId::new(value));
            }
        }
    }

    Ok(Selected {
        on: planned.on,
        options,
        mode,
    })
}

/// What each declared category asks of the agent: the option the harness categorized as it, or
/// a legacy mode among `modes`. A category the harness offers no way to set fails rather than
/// running on something the Session did not name.
fn planned(
    offered: &[SessionConfigOption],
    modes: Option<&SessionModeState>,
    declared: &Declared<'_>,
) -> Result<Planned, Error> {
    let mut choices = Vec::new();

    // The model the harness is on is reported whether or not this Session named one.
    let mut on = None;
    if let Some((id, select)) = selectable(offered, "model") {
        on = Some(On {
            model: select.current_value.0.to_string(),
        });
        if let Some(named) = declared.model {
            check_offered("model", named, &select.options)?;
            if select.current_value.0.as_ref() != named {
                on = Some(On {
                    model: named.to_owned(),
                });
                choices.push(Choice::Option {
                    id: id.clone(),
                    value: named.to_owned(),
                });
            }
        }
    } else if let Some(named) = declared.model {
        return Err(no_option("model", named));
    }

    for (category, named) in [
        ("mode", declared.mode),
        ("thought_level", declared.thought_level),
    ] {
        let Some(named) = named else {
            continue;
        };
        if let Some((id, select)) = selectable(offered, category) {
            check_offered(category, named, &select.options)?;
            if select.current_value.0.as_ref() != named {
                choices.push(Choice::Option {
                    id: id.clone(),
                    value: named.to_owned(),
                });
            }
        } else if category == "mode" {
            if modes.is_some_and(|modes| {
                modes
                    .available_modes
                    .iter()
                    .any(|mode| mode.id.0.as_ref() == named)
            }) {
                choices.push(Choice::Mode(named.to_owned()));
            } else {
                return Err(no_option(category, named));
            }
        } else {
            return Err(no_option(category, named));
        }
    }

    Ok(Planned { choices, on })
}

/// The option a harness categorized as this, when it is one a client can select.
fn selectable<'o>(
    offered: &'o [SessionConfigOption],
    category: &str,
) -> Option<(&'o SessionConfigId, &'o SessionConfigSelect)> {
    offered
        .iter()
        .find(|option| {
            option
                .category
                .as_ref()
                .and_then(category_of)
                .is_some_and(|categorized| categorized == category)
        })
        .and_then(|option| match &option.kind {
            SessionConfigKind::Select(select) => Some((&option.id, select)),
            _ => None,
        })
}

fn category_of(category: &SessionConfigOptionCategory) -> Option<&'static str> {
    match category {
        SessionConfigOptionCategory::Model => Some("model"),
        SessionConfigOptionCategory::Mode => Some(MODE),
        SessionConfigOptionCategory::ThoughtLevel => Some("thought_level"),
        _ => None,
    }
}

/// Config options are optional and every agent ships a default (ADR-0007), so a harness may
/// offer no way to set a category. One whose Session named a value for it then fails rather than
/// quietly running on something else.
fn no_option(category: &str, named: &str) -> Error {
    Error::internal_error().data(format!(
        "this agent lets no client select a {category}, and this session named {named}"
    ))
}

fn check_offered(
    category: &str,
    named: &str,
    options: &SessionConfigSelectOptions,
) -> Result<(), Error> {
    let offered: Vec<String> = selectable_values(options)
        .map(|value| value.to_string())
        .collect();
    if offered.iter().any(|value| value == named) {
        return Ok(());
    }

    Err(Error::internal_error().data(format!(
        "this agent does not offer the {category} {named}, which this session named"
    )))
}

/// ACP's `terminal` method launches an interactive process for someone to log in at, so an
/// agent offering nothing else cannot be driven headlessly and is refused here rather than
/// prompted and left waiting (ADR-0007).
fn needs_a_human_at_a_terminal(offered: &[AuthMethod]) -> bool {
    !offered.is_empty()
        && offered
            .iter()
            .all(|method| matches!(method, AuthMethod::Terminal(_)))
}

/// ACP requires the agent to be logged in before `session/new`, and offers no way to tell which of
/// several methods a client with nobody at a keyboard should pick, so the method is
/// configuration and an agent that needs one kestrel was not given fails here.
fn unlogged_in(error: Error, offered_methods: &[AuthMethod]) -> Error {
    if error.code != ErrorCode::AuthRequired {
        return error;
    }

    Error::internal_error().data(format!(
        "this agent must be logged in before it answers session/new, and kestrel was configured \
         with no method to log it in with. it offers {}",
        offered(offered_methods)
    ))
}

fn offered(methods: &[AuthMethod]) -> String {
    if methods.is_empty() {
        return "none".to_owned();
    }

    methods
        .iter()
        .map(|method| method.id().0.to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

fn selectable_values(options: &SessionConfigSelectOptions) -> impl Iterator<Item = Arc<str>> {
    let values: Vec<Arc<str>> = match options {
        SessionConfigSelectOptions::Ungrouped(options) => options
            .iter()
            .map(|option| option.value.0.clone())
            .collect(),
        SessionConfigSelectOptions::Grouped(groups) => groups
            .iter()
            .flat_map(|group| group.options.iter().map(|option| option.value.0.clone()))
            .collect(),
        _ => Vec::new(),
    };

    values.into_iter()
}

fn bounded(line: &str) -> String {
    if line.len() <= LINE_LIMIT {
        return line.to_owned();
    }

    let mut end = LINE_LIMIT;
    while !line.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}… [truncated]", &line[..end])
}

/// Waits until a deadline that a Turn being cancelled armed, or forever while it is not.
async fn until(deadline: Option<tokio::time::Instant>) {
    match deadline {
        Some(at) => tokio::time::sleep_until(at).await,
        None => std::future::pending().await,
    }
}

/// Why a turn that stopped for anything but ending it ends the conversation too.
fn stopped_short(stop: StopReason) -> Option<String> {
    let because = match stop {
        StopReason::EndTurn => return None,
        StopReason::MaxTokens => "the agent ran out of tokens".to_owned(),
        StopReason::MaxTurnRequests => "the agent ran out of requests".to_owned(),
        StopReason::Refusal => "the agent refused the work".to_owned(),
        StopReason::Cancelled => "the agent was cancelled".to_owned(),
        other => format!("the agent stopped for a reason kestrel does not know: {other:?}"),
    };

    Some(because)
}

pub enum ConversationEvent {
    Report(Report),
    Worked(Worked),
    /// The working Turn was cancelled at kestrel's request: the conversation stays open.
    Interrupted,
    State(Report),
    /// The conversation is open, with no Turn started: what an unbriefed Session waits for.
    Ready,
}

struct Hearing {
    completer: Completer,
    allowed: Vec<Subject>,
    on: Option<On>,
    /// A cancel is in flight: permission requests are answered `cancelled` until the Turn
    /// returns.
    interrupting: bool,
    reports: mpsc::UnboundedSender<ConversationEvent>,
    diagnostics: mpsc::UnboundedSender<String>,
    replaying: bool,
    /// What the harness has said about the Session beyond its turns (ADR-0041), and the last
    /// whole state this supervisor put on the link, so an unchanged resend is dropped here.
    info: Held,
    announced: Option<SessionInfo>,
}

/// The bookkeeping a harness keeps current for the Session's whole life.
#[derive(Default)]
struct Held {
    title: Option<String>,
    options: Vec<SessionConfigOption>,
    modes: Option<SessionModeState>,
    current_mode: Option<SessionModeId>,
    commands: Vec<AvailableCommand>,
}

impl Held {
    /// The whole state as the link carries it, with a Mode option synthesized from legacy `modes`
    /// for a harness that offers none of its own (ADR-0041).
    fn snapshot(&self) -> SessionInfo {
        let mut options: Vec<SessionOption> = self.options.iter().map(option).collect();
        let mode = self.current_mode.clone().or_else(|| {
            self.modes
                .as_ref()
                .map(|modes| modes.current_mode_id.clone())
        });
        match options.iter_mut().find(|option| option.is_category(MODE)) {
            Some(option) => set_current(option, mode),
            None => {
                if let Some(current) = mode {
                    options.push(synthesized_mode(&current, self.modes.as_ref()));
                }
            }
        }

        SessionInfo {
            title: self.title.clone(),
            options,
            commands: self.commands.iter().map(command).collect(),
        }
    }
}

const MODE: &str = "mode";

fn option(option: &SessionConfigOption) -> SessionOption {
    SessionOption {
        id: option.id.0.to_string(),
        name: option.name.clone(),
        description: option.description.clone(),
        category: option.category.as_ref().map(category),
        kind: match &option.kind {
            SessionConfigKind::Select(select) => SessionOptionKind::Select {
                current: select.current_value.0.to_string(),
                values: values(&select.options),
                groups: groups(&select.options),
            },
            SessionConfigKind::Boolean(boolean) => SessionOptionKind::Boolean {
                current: boolean.current_value,
            },
            _ => SessionOptionKind::Select {
                current: "unknown".to_owned(),
                values: Vec::new(),
                groups: Vec::new(),
            },
        },
    }
}

/// A select's flat offered values, empty when the harness grouped them instead.
fn values(options: &SessionConfigSelectOptions) -> Vec<SessionOptionValue> {
    match options {
        SessionConfigSelectOptions::Ungrouped(options) => options.iter().map(value).collect(),
        _ => Vec::new(),
    }
}

fn value(value: &SessionConfigSelectOption) -> SessionOptionValue {
    SessionOptionValue {
        value: value.value.0.to_string(),
        name: value.name.clone(),
        description: value.description.clone(),
    }
}

fn groups(options: &SessionConfigSelectOptions) -> Vec<SessionOptionGroup> {
    match options {
        SessionConfigSelectOptions::Grouped(groups) => groups
            .iter()
            .map(|group| SessionOptionGroup {
                group: group.group.0.to_string(),
                name: group.name.clone(),
                values: group.options.iter().map(value).collect(),
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn category(category: &SessionConfigOptionCategory) -> String {
    match category {
        SessionConfigOptionCategory::Mode => "mode".to_owned(),
        SessionConfigOptionCategory::Model => "model".to_owned(),
        SessionConfigOptionCategory::ModelConfig => "model_config".to_owned(),
        SessionConfigOptionCategory::ThoughtLevel => "thought_level".to_owned(),
        SessionConfigOptionCategory::Other(other) => other.clone(),
        _ => "unknown".to_owned(),
    }
}

fn set_current(option: &mut SessionOption, current: Option<SessionModeId>) {
    let (Some(current), SessionOptionKind::Select { current: held, .. }) =
        (current, &mut option.kind)
    else {
        return;
    };

    *held = current.0.to_string();
}

fn synthesized_mode(current: &SessionModeId, modes: Option<&SessionModeState>) -> SessionOption {
    SessionOption {
        id: MODE.to_owned(),
        name: "Mode".to_owned(),
        description: None,
        category: Some(MODE.to_owned()),
        kind: SessionOptionKind::Select {
            current: current.0.to_string(),
            values: modes
                .map(|modes| {
                    modes
                        .available_modes
                        .iter()
                        .map(|mode| SessionOptionValue {
                            value: mode.id.0.to_string(),
                            name: mode.name.clone(),
                            description: mode.description.clone(),
                        })
                        .collect()
                })
                .unwrap_or_default(),
            groups: Vec::new(),
        },
    }
}

fn command(command: &AvailableCommand) -> SessionCommand {
    SessionCommand {
        name: command.name.clone(),
        description: command.description.clone(),
        input_hint: match &command.input {
            Some(AvailableCommandInput::Unstructured(input)) => Some(input.hint.clone()),
            _ => None,
        },
    }
}

impl Hearing {
    fn emit(&self, completed: Completed) {
        if let Some(state) = completed.state {
            let _ = self.reports.send(ConversationEvent::State(state));
        }
        for report in completed.reports {
            let _ = self.reports.send(ConversationEvent::Report(report));
        }
        for diagnostic in completed.diagnostics {
            let _ = self.diagnostics.send(diagnostic);
        }
    }
    fn model(&mut self, on: Option<On>) {
        if let Some(on) = &on {
            let _ = self.reports.send(ConversationEvent::Report(Report::Model {
                model: on.model.clone(),
            }));
        }
        self.on = on;
    }
    /// A legacy mode set through `session/set_mode`, kept as the mode the Session is on.
    fn mode(&mut self, mode: Option<SessionModeId>) {
        if let Some(mode) = mode {
            self.info.current_mode = Some(mode);
            self.announce();
        }
    }
    /// Takes the options a `session/new`, `session/load`, `session/resume` or a
    /// `session/set_config_option` answer carried, when it carried any, and says the whole state
    /// up the link if it differs from what was last said.
    fn hold(&mut self, options: Option<Vec<SessionConfigOption>>, modes: Option<SessionModeState>) {
        if let Some(options) = options {
            self.info.options = options;
        }
        if let Some(modes) = modes {
            self.info.modes = Some(modes);
        }
        self.announce();
    }
    fn announce(&mut self) {
        let info = self.info.snapshot();
        if self.announced.as_ref() == Some(&info) {
            return;
        }
        self.announced = Some(info.clone());
        let _ = self
            .reports
            .send(ConversationEvent::Report(Report::SessionInfo(info)));
    }
    fn update(&mut self, update: SessionUpdate) {
        if self.replaying {
            return;
        }
        match &update {
            SessionUpdate::ConfigOptionUpdate(update) => {
                self.info.options = update.config_options.clone();
            }
            SessionUpdate::SessionInfoUpdate(update) => match &update.title {
                MaybeUndefined::Value(title) => self.info.title = Some(title.clone()),
                MaybeUndefined::Null => self.info.title = None,
                MaybeUndefined::Undefined => {}
            },
            SessionUpdate::AvailableCommandsUpdate(update) => {
                self.info.commands = update.available_commands.clone();
            }
            SessionUpdate::CurrentModeUpdate(update) => {
                self.info.current_mode = Some(update.current_mode_id.clone());
            }
            _ => {}
        }
        self.announce();
        let completed = self.completer.update(update, jiff::Timestamp::now());
        self.emit(completed);
    }
    fn worked(&mut self, failed: Option<String>) -> Worked {
        let outcome = match &failed {
            Some(because) => TurnOutcome::Failed {
                because: because.clone(),
            },
            None => TurnOutcome::Answered {
                stop_reason: "end_turn".to_owned(),
            },
        };
        let completed = self.completer.boundary(outcome, jiff::Timestamp::now());
        self.emit(completed);
        Worked {
            allowed: std::mem::take(&mut self.allowed),
            on: self.on.take(),
            failed,
        }
    }
}

#[cfg(test)]
mod tests {
    use agent_client_protocol::schema::v1::{
        AuthMethodAgent, AuthMethodTerminal, ConfigOptionUpdate, CurrentModeUpdate,
        SessionCapabilities, SessionConfigSelect, SessionConfigSelectGroup,
        SessionConfigSelectOption, SessionInfoUpdate, SessionMode, SessionResumeCapabilities,
    };

    use super::*;

    #[test]
    fn an_agent_offering_only_a_terminal_to_log_in_at_cannot_be_driven() {
        assert!(needs_a_human_at_a_terminal(&[AuthMethod::Terminal(
            AuthMethodTerminal::new("terminal", "Log in at a terminal")
        )]));
    }

    #[test]
    fn an_agent_offering_something_else_as_well_can_be() {
        assert!(!needs_a_human_at_a_terminal(&[
            AuthMethod::Terminal(AuthMethodTerminal::new("terminal", "Log in at a terminal")),
            AuthMethod::Agent(AuthMethodAgent::new("its-own", "Log in as the agent asks")),
        ]));
    }

    #[test]
    fn an_agent_that_offers_nothing_needs_nothing() {
        assert!(!needs_a_human_at_a_terminal(&[]));
    }

    fn models(offered: &[&'static str]) -> Vec<SessionConfigOption> {
        vec![
            SessionConfigOption::new(
                "reasoning",
                "Reasoning",
                SessionConfigKind::Select(SessionConfigSelect::new(
                    "low",
                    vec![SessionConfigSelectOption::new("low", "Low")],
                )),
            ),
            SessionConfigOption::new(
                "model",
                "Model",
                SessionConfigKind::Select(SessionConfigSelect::new(
                    offered[0],
                    offered
                        .iter()
                        .map(|model| SessionConfigSelectOption::new(*model, *model))
                        .collect::<Vec<_>>(),
                )),
            )
            .category(SessionConfigOptionCategory::Model),
        ]
    }

    fn a_mode_option(current: &str) -> SessionConfigOption {
        SessionConfigOption::new(
            "mode",
            "Mode",
            SessionConfigKind::Select(SessionConfigSelect::new(
                current.to_owned(),
                vec![
                    SessionConfigSelectOption::new("build", "Build"),
                    SessionConfigSelectOption::new("plan", "Plan"),
                ],
            )),
        )
        .category(SessionConfigOptionCategory::Mode)
    }

    /// A `Hearing` with nothing but the bookkeeping state under test.
    fn hearing() -> (Hearing, mpsc::UnboundedReceiver<ConversationEvent>) {
        let (reports, heard) = mpsc::unbounded_channel();
        (
            Hearing {
                completer: Completer::default(),
                allowed: Vec::new(),
                on: None,
                interrupting: false,
                reports,
                diagnostics: mpsc::unbounded_channel().0,
                replaying: false,
                info: Held::default(),
                announced: None,
            },
            heard,
        )
    }

    fn session_infos(heard: &mut mpsc::UnboundedReceiver<ConversationEvent>) -> Vec<SessionInfo> {
        let mut infos = Vec::new();
        while let Ok(event) = heard.try_recv() {
            if let ConversationEvent::Report(Report::SessionInfo(info)) = event {
                infos.push(info);
            }
        }

        infos
    }

    #[test]
    fn twenty_identical_config_option_updates_are_announced_once() {
        let (mut hearing, mut heard) = hearing();
        let offered = models(&["fast", "thorough"]);

        for _ in 0..20 {
            hearing.update(SessionUpdate::ConfigOptionUpdate(ConfigOptionUpdate::new(
                offered.clone(),
            )));
        }

        assert_eq!(session_infos(&mut heard).len(), 1);
        assert!(!hearing.completer.produced);
    }

    #[test]
    fn an_option_list_that_changes_is_announced_again() {
        let (mut hearing, mut heard) = hearing();

        hearing.update(SessionUpdate::ConfigOptionUpdate(ConfigOptionUpdate::new(
            models(&["fast"]),
        )));
        hearing.update(SessionUpdate::ConfigOptionUpdate(ConfigOptionUpdate::new(
            models(&["thorough"]),
        )));

        let said = session_infos(&mut heard);
        assert_eq!(said.len(), 2);
        assert!(said[1].options[1].is_category("model"));
    }

    #[test]
    fn a_harness_offering_only_legacy_modes_reports_a_synthesized_mode_option() {
        let (mut hearing, _) = hearing();

        hearing.hold(
            None,
            Some(SessionModeState::new(
                "build",
                vec![
                    SessionMode::new("build", "Build"),
                    SessionMode::new("plan", "Plan"),
                ],
            )),
        );

        let mode = hearing
            .info
            .snapshot()
            .options
            .into_iter()
            .find(|option| option.is_category("mode"))
            .expect("a synthesized mode option");
        assert_eq!(mode.id, "mode");
        assert_eq!(mode.name, "Mode");
        match mode.kind {
            SessionOptionKind::Select {
                current, values, ..
            } => {
                assert_eq!(current, "build");
                assert_eq!(
                    values
                        .iter()
                        .map(|value| value.value.as_str())
                        .collect::<Vec<_>>(),
                    ["build", "plan"]
                );
            }
            other => panic!("a mode option is a select, not {other:?}"),
        }
    }

    #[test]
    fn a_current_mode_update_sets_the_mode_options_current_value() {
        let (mut hearing, _) = hearing();
        hearing.hold(Some(vec![a_mode_option("build")]), None);

        hearing.update(SessionUpdate::CurrentModeUpdate(CurrentModeUpdate::new(
            "plan",
        )));

        let mode = hearing
            .info
            .snapshot()
            .options
            .into_iter()
            .find(|option| option.is_category("mode"))
            .expect("the mode option");
        assert!(matches!(
            mode.kind,
            SessionOptionKind::Select { current, .. } if current == "plan"
        ));
    }

    #[test]
    fn a_title_is_set_and_cleared_by_session_info_updates() {
        let (mut hearing, _) = hearing();

        hearing.update(SessionUpdate::SessionInfoUpdate(
            SessionInfoUpdate::new().title("a conversation"),
        ));
        assert_eq!(
            hearing.info.snapshot().title.as_deref(),
            Some("a conversation")
        );

        hearing.update(SessionUpdate::SessionInfoUpdate(
            SessionInfoUpdate::new().title(None::<String>),
        ));
        assert!(hearing.info.snapshot().title.is_none());
    }

    #[test]
    fn a_grouped_select_is_reported_with_its_groups() {
        let offered = vec![
            SessionConfigOption::new(
                "model",
                "Model",
                SessionConfigKind::Select(SessionConfigSelect::new(
                    "fast",
                    vec![SessionConfigSelectGroup::new(
                        "theirs",
                        "Theirs",
                        vec![SessionConfigSelectOption::new("fast", "Fast")],
                    )],
                )),
            )
            .category(SessionConfigOptionCategory::Model),
        ];
        let (mut hearing, _) = hearing();

        hearing.hold(Some(offered), None);

        let model = &hearing.info.snapshot().options[0];
        match &model.kind {
            SessionOptionKind::Select { values, groups, .. } => {
                assert!(values.is_empty());
                assert_eq!(groups[0].group, "theirs");
                assert_eq!(groups[0].values[0].value, "fast");
            }
            other => panic!("a model option is a select, not {other:?}"),
        }
    }

    #[test]
    fn the_model_a_session_named_is_set_through_the_option_the_agent_categorized_as_one() {
        let planned = planned(
            &models(&["fast", "thorough"]),
            None,
            &Declared {
                model: Some("thorough"),
                ..Declared::default()
            },
        )
        .expect("the model should be selectable");

        match planned.choices.as_slice() {
            [Choice::Option { id, value }] => {
                assert_eq!(id.0.as_ref(), "model");
                assert_eq!(value, "thorough");
            }
            other => panic!("expected one option to set, got {}", other.len()),
        }
        assert_eq!(planned.on.expect("the model it is on").model, "thorough");
    }

    #[test]
    fn a_session_that_named_no_model_sets_nothing_and_is_on_what_the_agent_already_was() {
        let planned = planned(&models(&["fast", "thorough"]), None, &Declared::default())
            .expect("naming no model should not fail");

        assert!(planned.choices.is_empty());
        assert_eq!(planned.on.expect("the model it is on").model, "fast");
    }

    #[test]
    fn a_category_already_at_the_declared_value_is_not_set() {
        let planned = planned(
            &models(&["fast", "thorough"]),
            None,
            &Declared {
                model: Some("fast"),
                ..Declared::default()
            },
        )
        .expect("the model should be selectable");

        assert!(planned.choices.is_empty());
        assert_eq!(planned.on.expect("the model it is on").model, "fast");
    }

    #[test]
    fn a_model_an_agent_does_not_offer_is_refused_rather_than_swapped_for_one_it_does() {
        let refused = planned(
            &models(&["fast"]),
            None,
            &Declared {
                model: Some("thorough"),
                ..Declared::default()
            },
        )
        .err()
        .expect("a model the agent does not offer");

        assert!(
            refused
                .data
                .is_some_and(|why| why.to_string().contains("thorough"))
        );
    }

    #[test]
    fn an_agent_that_lets_no_client_select_a_model_fails_a_session_that_named_one() {
        let refused = planned(
            &[],
            None,
            &Declared {
                model: Some("thorough"),
                ..Declared::default()
            },
        )
        .err()
        .expect("an agent with no model to select");

        assert!(
            refused
                .data
                .is_some_and(|why| why.to_string().contains("thorough"))
        );
    }

    #[test]
    fn an_agent_that_lets_no_client_select_a_model_works_a_session_that_named_none() {
        assert!(
            planned(&[], None, &Declared::default())
                .expect("naming no model should not fail")
                .choices
                .is_empty()
        );
    }

    #[test]
    fn a_declared_mode_is_set_through_the_option_the_agent_categorized_as_one() {
        let planned = planned(
            &[a_mode_option("build")],
            None,
            &Declared {
                mode: Some("plan"),
                ..Declared::default()
            },
        )
        .expect("the mode should be selectable");

        match planned.choices.as_slice() {
            [Choice::Option { id, value }] => {
                assert_eq!(id.0.as_ref(), "mode");
                assert_eq!(value, "plan");
            }
            other => panic!("expected one option to set, got {}", other.len()),
        }
    }

    #[test]
    fn a_declared_mode_matching_a_legacy_mode_is_set_through_session_set_mode() {
        let legacy = SessionModeState::new(
            "build",
            vec![
                SessionMode::new("build", "Build"),
                SessionMode::new("plan", "Plan"),
            ],
        );
        let planned = planned(
            &[],
            Some(&legacy),
            &Declared {
                mode: Some("plan"),
                ..Declared::default()
            },
        )
        .expect("a legacy mode the harness offers");

        match planned.choices.as_slice() {
            [Choice::Mode(value)] => assert_eq!(value, "plan"),
            other => panic!("expected one mode to set, got {}", other.len()),
        }
    }

    #[test]
    fn a_mode_a_harness_offers_no_way_to_set_fails_naming_the_category_and_the_value() {
        let refused = planned(
            &[],
            None,
            &Declared {
                mode: Some("plan"),
                ..Declared::default()
            },
        )
        .err()
        .expect("a mode the harness offers no way to set");

        let why = refused.data.expect("a reason").to_string();
        assert!(why.contains("mode"), "{why}");
        assert!(why.contains("plan"), "{why}");

        let refused = planned(
            &[a_mode_option("build")],
            None,
            &Declared {
                mode: Some("review"),
                ..Declared::default()
            },
        )
        .err()
        .expect("a mode the harness does not offer");

        let why = refused.data.expect("a reason").to_string();
        assert!(why.contains("mode"), "{why}");
        assert!(why.contains("review"), "{why}");
    }

    #[test]
    fn a_thought_level_the_harness_does_not_offer_fails_naming_the_category_and_the_value() {
        let refused = planned(
            &models(&["fast"]),
            None,
            &Declared {
                thought_level: Some("high"),
                ..Declared::default()
            },
        )
        .err()
        .expect("a thought level the harness does not offer");

        let why = refused.data.expect("a reason").to_string();
        assert!(why.contains("thought_level"), "{why}");
        assert!(why.contains("high"), "{why}");
    }

    #[test]
    fn a_declared_thought_level_the_harness_offers_is_set() {
        let offered = vec![
            SessionConfigOption::new(
                "thinking",
                "Thinking",
                SessionConfigKind::Select(SessionConfigSelect::new(
                    "low",
                    vec![
                        SessionConfigSelectOption::new("low", "Low"),
                        SessionConfigSelectOption::new("high", "High"),
                    ],
                )),
            )
            .category(SessionConfigOptionCategory::ThoughtLevel),
        ];
        let planned = planned(
            &offered,
            None,
            &Declared {
                thought_level: Some("high"),
                ..Declared::default()
            },
        )
        .expect("a thought level the harness offers");

        match planned.choices.as_slice() {
            [Choice::Option { id, value }] => {
                assert_eq!(id.0.as_ref(), "thinking");
                assert_eq!(value, "high");
            }
            other => panic!("expected one option to set, got {}", other.len()),
        }
    }

    #[test]
    fn a_grouped_selection_is_searched_the_same_as_a_flat_one() {
        let grouped = vec![
            SessionConfigOption::new(
                "model",
                "Model",
                SessionConfigKind::Select(SessionConfigSelect::new(
                    "fast",
                    vec![SessionConfigSelectGroup::new(
                        "theirs",
                        "Theirs",
                        vec![SessionConfigSelectOption::new("thorough", "Thorough")],
                    )],
                )),
            )
            .category(SessionConfigOptionCategory::Model),
        ];

        assert!(
            planned(
                &grouped,
                None,
                &Declared {
                    model: Some("thorough"),
                    ..Declared::default()
                },
            )
            .is_ok()
        );
    }

    #[test]
    fn an_agent_that_must_be_logged_in_says_what_it_offers_to_be_logged_in_with() {
        let offered = [AuthMethod::Agent(AuthMethodAgent::new(
            "its-own",
            "Log in as the agent asks",
        ))];

        let refused = unlogged_in(Error::auth_required(), &offered);

        assert!(
            refused
                .data
                .is_some_and(|why| why.to_string().contains("its-own"))
        );
    }

    #[test]
    fn an_error_that_is_not_about_being_logged_in_is_carried_as_it_came() {
        let refused = unlogged_in(Error::invalid_params().data("no"), &[]);

        assert_eq!(refused.code, ErrorCode::InvalidParams);
    }

    #[test]
    fn a_line_within_the_limit_is_carried_whole() {
        assert_eq!(
            bounded("level=INFO message=init"),
            "level=INFO message=init"
        );
    }

    #[test]
    fn an_overlong_line_is_cut_short_and_says_so() {
        let overlong = "é".repeat(LINE_LIMIT);

        let carried = bounded(&overlong);

        assert!(carried.starts_with(&"é".repeat(LINE_LIMIT / 2)));
        assert!(carried.ends_with("… [truncated]"));
        assert!(carried.len() < LINE_LIMIT + 64);
    }

    #[test]
    fn ending_the_turn_is_the_only_stop_reason_the_conversation_goes_on_after() {
        assert_eq!(stopped_short(StopReason::EndTurn), None);

        for stop in [
            StopReason::MaxTokens,
            StopReason::MaxTurnRequests,
            StopReason::Refusal,
            StopReason::Cancelled,
        ] {
            assert!(stopped_short(stop).is_some(), "{stop:?}");
        }
    }

    #[test]
    fn an_agent_that_can_both_resume_and_load_a_session_is_resumed() {
        let capabilities = AgentCapabilities::new()
            .load_session(true)
            .session_capabilities(
                SessionCapabilities::new().resume(SessionResumeCapabilities::new()),
            );

        assert_eq!(Recovery::offered(&capabilities), Some(Recovery::Resume));
    }

    #[test]
    fn an_agent_that_can_only_load_a_session_is_loaded() {
        let capabilities = AgentCapabilities::new().load_session(true);

        assert_eq!(Recovery::offered(&capabilities), Some(Recovery::Load));
    }

    #[test]
    fn an_agent_that_can_do_neither_cannot_be_recovered() {
        assert_eq!(Recovery::offered(&AgentCapabilities::new()), None);
    }

    fn conversing_with(recovery: Option<Recovery>) -> Continuity {
        Continuity {
            conversed: Some(SessionId::new("a-conversation")),
            recovery,
            ..Continuity::default()
        }
    }

    #[test]
    fn a_process_lost_before_it_opened_a_session_has_nothing_to_recover() {
        let mut continuity = Continuity {
            recovery: Some(Recovery::Load),
            ..Continuity::default()
        };

        assert_eq!(
            continuity.recovering("it exited".to_owned()),
            Err("it exited".to_owned())
        );
    }

    #[test]
    fn a_harness_that_cannot_resume_a_session_ends_the_conversation_saying_so() {
        let refused = conversing_with(None)
            .recovering("it exited".to_owned())
            .expect_err("nothing to recover with");

        assert!(
            refused.contains("it exited") && refused.contains("cannot resume"),
            "{refused}"
        );
    }

    #[test]
    fn an_agent_lost_again_before_it_answers_is_not_brought_back_twice() {
        let mut continuity = conversing_with(Some(Recovery::Load));
        assert_eq!(continuity.recovering("it exited".to_owned()), Ok(()));

        let refused = continuity
            .recovering("it exited".to_owned())
            .expect_err("a second loss without an answer between");

        assert!(refused.contains("again"), "{refused}");
    }
}
