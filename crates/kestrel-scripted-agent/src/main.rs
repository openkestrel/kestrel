//! A canned sequence over real stdio JSON-RPC, so the main suite drives kestrel's ACP client
//! over the wire rather than over a shim above it, with no network and no model spend.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::schema::v1::{
    AgentCapabilities, AuthMethod, AuthMethodAgent, AuthMethodTerminal, AvailableCommand,
    AvailableCommandInput, AvailableCommandsUpdate, CancelNotification, ConfigOptionUpdate,
    ContentBlock, ContentChunk, Cost, CurrentModeUpdate, InitializeRequest, InitializeResponse,
    LoadSessionRequest, LoadSessionResponse, MessageId, NewSessionRequest, NewSessionResponse,
    PermissionOption, PermissionOptionKind, Plan, PlanEntry, PlanEntryPriority, PlanEntryStatus,
    PromptCapabilities, PromptRequest, PromptResponse, RequestPermissionOutcome,
    RequestPermissionRequest, SessionConfigKind, SessionConfigOption, SessionConfigOptionCategory,
    SessionConfigSelect, SessionConfigSelectOption, SessionConfigValueId, SessionInfoUpdate,
    SessionMode, SessionModeState, SessionNotification, SessionUpdate,
    SetSessionConfigOptionRequest, SetSessionConfigOptionResponse, SetSessionModeRequest,
    SetSessionModeResponse, StopReason, TextContent, ToolCall, ToolCallStatus, ToolCallUpdate,
    ToolCallUpdateFields, UnstructuredCommandInput, UsageUpdate,
};
use agent_client_protocol::{Agent, Client, ConnectionTo, Error, Result, Stdio};
use clap::Parser;
use kestrel_scripted_agent::{
    BACKGROUND, BOOKKEEPING, BURSTED_SIZE, BURSTED_USAGE, CHATTER, CHATTERED_LINES,
    CHATTERED_MESSAGES, COMMAND, COMMAND_DESCRIPTION, COMMAND_HINT, CONFIDED, CUSTOM_CATEGORY,
    CUSTOM_OPTION, DEFAULT_MODEL, FIRST_MEMORY, LAST_MEMORY, LOGIN, MODE_OPTION, MUTTERED,
    OTHER_MODE, OTHER_MODEL, OTHER_THOUGHT_LEVEL, OVERLONG, REFRESHED, REPEATS, RESUMES_AFTER,
    SAID_WHILE_TRAILING, STARTING_MODE, STARTING_THOUGHT_LEVEL, SWITCHED_MODE, Script,
    THOUGHT_LEVEL_OPTION, TITLE, WRITTEN_WHILE_TRAILING, chattered, conversed,
};

const SESSION: &str = "scripted";
/// Long enough to kill a control plane and bring it back up under a turn that is in flight.
const LINGER: Duration = Duration::from_secs(3);
const TOOL_CALL: &str = "call-1";
const MODEL_OPTION: &str = "model";
const ALLOW_ONCE: &str = "allow-once";
/// Where `Revives` keeps its session, outside the checkout as a harness's own store would be.
const KEPT: &str = ".scripted-session";
const DIED: &str = ".scripted-session-died";
const PROMPT_SEPARATOR: char = '\u{1e}';
const VANISHING: Duration = Duration::from_millis(100);
const BURST_SETTLED: Duration = Duration::from_millis(1_000);
/// Between `BurstsUsage`'s updates: the burst still fits one usage window, but is long enough that
/// a heartbeat landing inside it would carry a value the window has not reported.
const BURST_SPACING: Duration = Duration::from_millis(200);
const BURST_PATIENCE: Duration = Duration::from_millis(2_500);

#[derive(Debug, Parser)]
#[command(name = "kestrel-scripted-agent", version)]
struct Cli {
    /// The sequence to play
    #[arg(long, value_enum, default_value = "speaks")]
    script: Script,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let script = Cli::parse().script;
    let prompted_so_far: Arc<Mutex<Vec<String>>> = Arc::default();
    let cancelled: Arc<AtomicBool> = Arc::default();
    let opened_against: Arc<Mutex<Option<PathBuf>>> = Arc::default();
    let located = Arc::clone(&opened_against);
    let options: Arc<Mutex<Vec<SessionConfigOption>>> = Arc::default();
    let offered_options = Arc::clone(&options);
    let loaded_options = Arc::clone(&options);
    let set_options = Arc::clone(&options);

    Agent
        .builder()
        .name("kestrel-scripted-agent")
        .on_receive_notification(
            {
                let cancelled = Arc::clone(&cancelled);
                async move |cancel: CancelNotification, _connection| {
                    if cancel.session_id.0.as_ref() == SESSION {
                        cancelled.store(true, Ordering::SeqCst);
                    }
                    Ok(())
                }
            },
            agent_client_protocol::on_receive_notification!(),
        )
        .on_receive_request(
            async move |initialize: InitializeRequest, responder, _connection| {
                if initialize.protocol_version != ProtocolVersion::V1 {
                    return responder.respond_with_error(Error::invalid_params().data(format!(
                        "this agent speaks ACP v1, and was initialized with {:?}",
                        initialize.protocol_version
                    )));
                }

                if script == Script::Predates {
                    return responder.respond(InitializeResponse::new(ProtocolVersion::V0));
                }

                if script == Script::Demands {
                    return responder.respond(
                        InitializeResponse::new(ProtocolVersion::V1).auth_methods(vec![
                            AuthMethod::Terminal(AuthMethodTerminal::new(
                                "terminal",
                                "Log in at a terminal",
                            )),
                        ]),
                    );
                }

                if script == Script::Insists {
                    return responder.respond(
                        InitializeResponse::new(ProtocolVersion::V1).auth_methods(vec![
                            AuthMethod::Agent(AuthMethodAgent::new(
                                "its-own",
                                "Log in as the agent asks",
                            )),
                        ]),
                    );
                }

                responder.respond(
                    InitializeResponse::new(ProtocolVersion::V1).agent_capabilities(
                        AgentCapabilities::new()
                            .prompt_capabilities(PromptCapabilities::new())
                            .load_session(script == Script::Revives),
                    ),
                )
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |new: NewSessionRequest, responder, _connection| {
                *opened_against
                    .lock()
                    .expect("where the session was opened should not be poisoned") = Some(new.cwd);
                if script == Script::Insists {
                    return responder.respond_with_error(Error::auth_required());
                }

                responder.respond(match script {
                    Script::Decides => NewSessionResponse::new(SESSION),
                    Script::LegacyModes | Script::LegacyModesKept => {
                        NewSessionResponse::new(SESSION).modes(legacy_modes())
                    }
                    _ => {
                        let offered = offered(DEFAULT_MODEL);
                        *offered_options
                            .lock()
                            .expect("the offered options should not be poisoned") = offered.clone();
                        NewSessionResponse::new(SESSION).config_options(offered)
                    }
                })
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |load: LoadSessionRequest, responder, connection| {
                if script != Script::Revives || load.session_id.0.as_ref() != SESSION {
                    return responder.respond_with_error(
                        Error::invalid_params().data("this agent kept no such session"),
                    );
                }

                let kept = kept();
                for (turn, prompted) in kept.iter().enumerate() {
                    update(
                        &connection,
                        SessionUpdate::UserMessageChunk(chunk(None, prompted)),
                    )?;
                    say(
                        &connection,
                        &format!("replayed-{turn}"),
                        &conversed(turn + 1, &kept[..turn]),
                    )?;
                }
                let offered = offered(DEFAULT_MODEL);
                *loaded_options
                    .lock()
                    .expect("the offered options should not be poisoned") = offered.clone();
                responder.respond(LoadSessionResponse::new().config_options(offered))
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |set: SetSessionConfigOptionRequest, responder, _connection| {
                if script == Script::RefusesOptions {
                    return responder.respond_with_error(
                        Error::internal_error()
                            .data("this scripted agent will not change its options"),
                    );
                }
                let Some(selected) = set.value.as_value_id() else {
                    return responder.respond_with_error(
                        Error::invalid_params().data("this agent's options are selections"),
                    );
                };

                {
                    let mut offered = set_options
                        .lock()
                        .expect("the offered options should not be poisoned");
                    let Some(option) = offered.iter_mut().find(|option| option.id == set.config_id)
                    else {
                        return responder.respond_with_error(
                            Error::invalid_params()
                                .data(format!("this agent offers no option {}", set.config_id.0)),
                        );
                    };
                    match &mut option.kind {
                        SessionConfigKind::Select(select) => {
                            select.current_value = selected.clone();
                        }
                        SessionConfigKind::Boolean(boolean) => {
                            boolean.current_value = selected.0.as_ref() == "true";
                        }
                        _ => {}
                    }
                }

                responder.respond(SetSessionConfigOptionResponse::new(
                    set_options
                        .lock()
                        .expect("the offered options should not be poisoned")
                        .clone(),
                ))
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |set: SetSessionModeRequest, responder, connection| {
                update(
                    &connection,
                    SessionUpdate::CurrentModeUpdate(CurrentModeUpdate::new(set.mode_id)),
                )?;
                responder.respond(SetSessionModeResponse::new())
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |prompt: PromptRequest, responder, connection| {
                let prompted_so_far = Arc::clone(&prompted_so_far);
                let cancelled = Arc::clone(&cancelled);
                // A cancel that arrived between turns belongs to the turn it was asked of, not
                // this one.
                cancelled.store(false, Ordering::SeqCst);
                let located = located
                    .lock()
                    .expect("where the session was opened should not be poisoned")
                    .clone();
                let prompted: String = prompt
                    .prompt
                    .iter()
                    .filter_map(|block| match block {
                        ContentBlock::Text(text) => Some(text.text.as_str()),
                        _ => None,
                    })
                    .collect();
                // The turn asks the client a question of its own, so it cannot run inside the
                // dispatch loop that would have to carry the answer.
                connection.clone().spawn(async move {
                    let earlier = {
                        let mut so_far = prompted_so_far
                            .lock()
                            .expect("what was prompted should not be poisoned");
                        let earlier = so_far.clone();
                        so_far.push(prompted.clone());
                        earlier
                    };
                    let stop = play(
                        script,
                        &prompted,
                        &earlier,
                        located.clone(),
                        &connection,
                        &cancelled,
                    )
                    .await?;
                    responder.respond(PromptResponse::new(stop))?;
                    if earlier.is_empty() {
                        trail(script, located, &connection).await?;
                    }
                    Ok(())
                })
            },
            agent_client_protocol::on_receive_request!(),
        )
        .connect_to(Stdio::new())
        .await
}

static TURN: AtomicUsize = AtomicUsize::new(0);

async fn play(
    script: Script,
    prompted: &str,
    earlier: &[String],
    located: Option<PathBuf>,
    connection: &ConnectionTo<Client>,
    cancelled: &AtomicBool,
) -> Result<StopReason> {
    TURN.store(earlier.len(), Ordering::Relaxed);
    let works_until_cancelled = match script {
        Script::WorksUntilCancelled => earlier.is_empty(),
        Script::AnswersThenWorksUntilCancelled => earlier.len() == 1,
        _ => false,
    };
    if works_until_cancelled {
        say(connection, "working", "working on it")?;
        // A second message closes the first, so the control plane has it on the record before
        // the cancel arrives.
        say(connection, "working-still", "still on it")?;
        update(
            connection,
            SessionUpdate::ToolCall(
                ToolCall::new(format!("{TOOL_CALL}-{}", earlier.len() + 1), "a long read")
                    .status(ToolCallStatus::InProgress),
            ),
        )?;
        while !cancelled.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        permission_cancelled(connection).await?;

        return Ok(StopReason::Cancelled);
    }
    if script == Script::IgnoresCancel {
        say(connection, "ignoring", "ignoring the cancel")?;
        say(connection, "ignoring-still", "still ignoring it")?;
        update(
            connection,
            SessionUpdate::ToolCall(
                ToolCall::new(format!("{TOOL_CALL}-ignored"), "a read nobody will answer")
                    .status(ToolCallStatus::InProgress),
            ),
        )?;
        return std::future::pending().await;
    }
    if script == Script::ReportsThenWaits {
        say(connection, "first", "first message")?;
        update(
            connection,
            SessionUpdate::AgentThoughtChunk(chunk(Some("thinking"), "thinking between messages")),
        )?;
        say(connection, "second", "still working")?;
        say(connection, "first", "late text")?;
        update(
            connection,
            SessionUpdate::UsageUpdate(UsageUpdate::new(12, 100)),
        )?;
        std::future::pending::<()>().await;
    }
    if script == Script::BurstsUsage {
        // Long enough that the notices the Session's start raised have settled first.
        tokio::time::sleep(BURST_SETTLED).await;
        for used in [
            BURSTED_USAGE / 4,
            BURSTED_USAGE / 2,
            BURSTED_USAGE * 3 / 4,
            BURSTED_USAGE,
        ] {
            update(
                connection,
                SessionUpdate::UsageUpdate(UsageUpdate::new(used, BURSTED_SIZE)),
            )?;
            tokio::time::sleep(BURST_SPACING).await;
        }
        // Long enough for the trailing-edge usage report to go up, and to watch for notices.
        tokio::time::sleep(BURST_PATIENCE).await;
        say(connection, "message-1", "spent")?;
        return Ok(StopReason::EndTurn);
    }
    if matches!(script, Script::CancelledText | Script::FailedText) {
        say(connection, "message", "observed message")?;
        update(
            connection,
            SessionUpdate::AgentThoughtChunk(chunk(Some("thought"), "observed thought")),
        )?;
        return Ok(if script == Script::CancelledText {
            StopReason::Cancelled
        } else {
            StopReason::MaxTokens
        });
    }
    if script == Script::Mutters {
        eprintln!("{MUTTERED}");
        eprintln!("{}", "a".repeat(OVERLONG));
    }
    if matches!(script, Script::Dawdles | Script::Mutters) {
        std::future::pending::<()>().await;
    }
    if script == Script::Chatters {
        for line in 1..=CHATTERED_LINES {
            eprintln!("git ran {line} of turn {}", earlier.len() + 1);
            tokio::time::sleep(CHATTER).await;
        }
        for message in 1..=CHATTERED_MESSAGES {
            say(
                connection,
                &format!("message-{message}"),
                &chattered(earlier.len() + 1, message),
            )?;
        }
        return Ok(StopReason::EndTurn);
    }
    if script == Script::Writes {
        tokio::time::sleep(Duration::from_secs(3)).await;
        let directory = located.as_ref().ok_or_else(Error::internal_error)?;
        std::fs::write(directory.join("written.txt"), "work in progress\n")
            .map_err(Error::into_internal_error)?;
        tokio::time::sleep(Duration::from_secs(5)).await;
        say(connection, "message-1", "wrote a file")?;
        return Ok(StopReason::EndTurn);
    }
    if script == Script::Lingers {
        tokio::time::sleep(LINGER).await;
    }
    if script == Script::Confides {
        say(connection, "message-1", &confided())?;
        return Ok(StopReason::EndTurn);
    }
    if script == Script::InspectsEnvironment {
        update(
            connection,
            SessionUpdate::ToolCall(
                ToolCall::new(TOOL_CALL, "env").status(ToolCallStatus::Completed),
            ),
        )?;
        let output = std::process::Command::new("env")
            .output()
            .map_err(Error::into_internal_error)?;
        if !output.status.success() {
            return Err(Error::internal_error());
        }
        let exposed: Vec<_> = String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter_map(|line| line.split_once('=').map(|(name, _)| name))
            .filter(|name| name.starts_with("KESTREL_"))
            .map(str::to_owned)
            .collect();
        say(
            connection,
            "message-1",
            &format!("KESTREL_ variables: {exposed:?}"),
        )?;
        return Ok(StopReason::EndTurn);
    }
    if script == Script::Refreshes {
        say(connection, "message-1", &refreshed())?;
        return Ok(StopReason::EndTurn);
    }
    if script == Script::Recalls {
        let remembers = prompted.contains(FIRST_MEMORY) && prompted.contains(LAST_MEMORY);
        let message = match remembers {
            true => "I remember the whole earlier context",
            false => "I forgot part of the earlier context",
        };
        say(connection, "message-1", message)?;
        return Ok(StopReason::EndTurn);
    }
    if script == Script::Locates {
        let said = match located {
            Some(directory) => directory.display().to_string(),
            None => "no session was opened".to_owned(),
        };
        say(connection, "message-1", &said)?;
        return Ok(StopReason::EndTurn);
    }
    if script == Script::Echoes {
        say(connection, "message-1", prompted)?;
        return Ok(StopReason::EndTurn);
    }
    if script == Script::Converses {
        say(
            connection,
            "message-1",
            &conversed(earlier.len() + 1, earlier),
        )?;
        return Ok(StopReason::EndTurn);
    }
    if script == Script::Refuses {
        say(connection, "message-1", "this is not work I will do")?;
        return Ok(StopReason::Refusal);
    }
    if script == Script::Silent {
        update(
            connection,
            SessionUpdate::AvailableCommandsUpdate(AvailableCommandsUpdate::new(Vec::new())),
        )?;
        update(
            connection,
            SessionUpdate::SessionInfoUpdate(SessionInfoUpdate::new().title("silent")),
        )?;
        update(
            connection,
            SessionUpdate::ConfigOptionUpdate(ConfigOptionUpdate::new(offered(DEFAULT_MODEL))),
        )?;
        return Ok(StopReason::EndTurn);
    }
    if script == Script::OversizedTool {
        update(
            connection,
            SessionUpdate::ToolCall(
                ToolCall::new(TOOL_CALL, "large read")
                    .raw_input(serde_json::json!({"path":"large.txt"})),
            ),
        )?;
        update(
            connection,
            SessionUpdate::ToolCallUpdate(ToolCallUpdate::new(
                TOOL_CALL,
                ToolCallUpdateFields::new()
                    .status(ToolCallStatus::Completed)
                    .raw_output(serde_json::json!({"text":"x".repeat(70 * 1024)})),
            )),
        )?;
        return Ok(StopReason::EndTurn);
    }
    if script == Script::ReconnectingTools {
        for id in ["settles-offline", "still-running"] {
            update(
                connection,
                SessionUpdate::ToolCall(ToolCall::new(id, id).status(ToolCallStatus::InProgress)),
            )?;
        }
        tokio::time::sleep(std::time::Duration::from_secs(4)).await;
        update(
            connection,
            SessionUpdate::ToolCallUpdate(ToolCallUpdate::new(
                "settles-offline",
                ToolCallUpdateFields::new().status(ToolCallStatus::Completed),
            )),
        )?;
        tokio::time::sleep(std::time::Duration::from_secs(8)).await;
        update(
            connection,
            SessionUpdate::ToolCallUpdate(ToolCallUpdate::new(
                "still-running",
                ToolCallUpdateFields::new().status(ToolCallStatus::Completed),
            )),
        )?;
        return Ok(StopReason::EndTurn);
    }
    if matches!(
        script,
        Script::AnswersThenWorks
            | Script::AnswersThenKeepsBooks
            | Script::ResumesAfterSettling
            | Script::AnswersThenWrites
    ) {
        say(connection, "message-1", "answered, with more to do")?;
        return Ok(StopReason::EndTurn);
    }
    if matches!(
        script,
        Script::AnswersWithAToolOpen | Script::AnswersWithAToolOpenThenExits
    ) && earlier.is_empty()
    {
        update(
            connection,
            SessionUpdate::ToolCall(
                ToolCall::new(TOOL_CALL, "background tests").status(ToolCallStatus::InProgress),
            ),
        )?;
        say(connection, "message-1", "waiting on the background tests")?;
        return Ok(StopReason::EndTurn);
    }
    if matches!(
        script,
        Script::SlowTool | Script::OpenToolCancelled | Script::OpenToolFailed
    ) {
        update(
            connection,
            SessionUpdate::ToolCall(
                ToolCall::new(TOOL_CALL, "slow read")
                    .status(ToolCallStatus::InProgress)
                    .raw_input(serde_json::json!({"path":"README.md"})),
            ),
        )?;
        match script {
            Script::OpenToolCancelled => return Ok(StopReason::Cancelled),
            Script::OpenToolFailed => return Ok(StopReason::MaxTokens),
            _ => {}
        }
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
        update(
            connection,
            SessionUpdate::ToolCallUpdate(ToolCallUpdate::new(
                TOOL_CALL,
                ToolCallUpdateFields::new()
                    .status(ToolCallStatus::Completed)
                    .raw_output(serde_json::json!({"text":"read result"})),
            )),
        )?;
        return Ok(StopReason::EndTurn);
    }
    if script == Script::Works {
        update(
            connection,
            SessionUpdate::ToolCall(
                ToolCall::new(TOOL_CALL, "read README.md").status(ToolCallStatus::Completed),
            ),
        )?;
        return Ok(StopReason::EndTurn);
    }
    if script == Script::Asks {
        permission_to_use_a_tool(connection).await?;
        return Ok(StopReason::EndTurn);
    }
    if script == Script::Revives {
        let kept = kept();
        if kept.len() == 1 && !std::path::Path::new(DIED).exists() {
            std::fs::write(DIED, "").map_err(Error::into_internal_error)?;
            std::process::exit(9);
        }
        keep(prompted)?;
        say(connection, "message-1", &conversed(kept.len() + 1, &kept))?;
        return Ok(StopReason::EndTurn);
    }
    if script == Script::Announces {
        // Long enough that a test can take the link down while the harness still has this to say.
        tokio::time::sleep(Duration::from_millis(800)).await;
        update(
            connection,
            SessionUpdate::SessionInfoUpdate(SessionInfoUpdate::new().title(TITLE)),
        )?;
        update(
            connection,
            SessionUpdate::AvailableCommandsUpdate(AvailableCommandsUpdate::new(vec![
                AvailableCommand::new(COMMAND, COMMAND_DESCRIPTION).input(
                    AvailableCommandInput::Unstructured(UnstructuredCommandInput::new(
                        COMMAND_HINT,
                    )),
                ),
                AvailableCommand::new("init", "Initialize the workspace"),
            ])),
        )?;
        update(
            connection,
            SessionUpdate::CurrentModeUpdate(CurrentModeUpdate::new(SWITCHED_MODE)),
        )?;
        say(connection, "message-1", "announced")?;
        return Ok(StopReason::EndTurn);
    }
    if script == Script::Repeats {
        for _ in 0..REPEATS {
            update(
                connection,
                SessionUpdate::ConfigOptionUpdate(ConfigOptionUpdate::new(offered(DEFAULT_MODEL))),
            )?;
        }
        say(connection, "message-1", "repeated")?;
        return Ok(StopReason::EndTurn);
    }
    if script == Script::SwitchesModel {
        update(
            connection,
            SessionUpdate::ConfigOptionUpdate(ConfigOptionUpdate::new(offered(OTHER_MODEL))),
        )?;
        say(connection, "message-1", "switched")?;
        return Ok(StopReason::EndTurn);
    }
    if script == Script::LegacyModes {
        update(
            connection,
            SessionUpdate::CurrentModeUpdate(CurrentModeUpdate::new(SWITCHED_MODE)),
        )?;
        say(connection, "message-1", "switched a legacy mode")?;
        return Ok(StopReason::EndTurn);
    }
    if script == Script::Vanishes {
        say(connection, "message-1", "answered, and about to vanish")?;
        tokio::spawn(async {
            tokio::time::sleep(VANISHING).await;
            std::process::exit(0);
        });
        return Ok(StopReason::EndTurn);
    }
    if script == Script::Lapses {
        if earlier.is_empty() {
            say(connection, "message-1", "the first turn is answered")?;
        }
        return Ok(StopReason::EndTurn);
    }

    update(
        connection,
        SessionUpdate::Plan(Plan::new(vec![PlanEntry::new(
            "read the issue",
            PlanEntryPriority::High,
            PlanEntryStatus::Pending,
        )])),
    )?;
    update(
        connection,
        SessionUpdate::AgentThoughtChunk(chunk(None, "the issue looks small")),
    )?;
    update(
        connection,
        SessionUpdate::ToolCall(
            ToolCall::new(TOOL_CALL, "read README.md").status(ToolCallStatus::Pending),
        ),
    )?;

    permission_to_use_a_tool(connection).await?;

    if script == Script::Dies {
        std::process::exit(9);
    }

    update(
        connection,
        SessionUpdate::ToolCallUpdate(ToolCallUpdate::new(
            TOOL_CALL,
            ToolCallUpdateFields::new().status(ToolCallStatus::Completed),
        )),
    )?;

    update(
        connection,
        SessionUpdate::AgentMessageChunk(chunk(Some("message-1"), "half of one message, ")),
    )?;
    update(
        connection,
        SessionUpdate::AgentMessageChunk(chunk(Some("message-1"), "and the other half")),
    )?;
    say(connection, "message-2", "a second message")?;

    update(
        connection,
        SessionUpdate::UsageUpdate(UsageUpdate::new(1_200, 200_000).cost(Cost::new(0.42, "USD"))),
    )?;

    Ok(StopReason::EndTurn)
}

/// What the agent does after answering its first turn, outside any turn.
async fn trail(
    script: Script,
    located: Option<PathBuf>,
    connection: &ConnectionTo<Client>,
) -> Result<()> {
    let background = format!("{TOOL_CALL}-background");
    let runs_in_the_background = async |after: Duration| -> Result<()> {
        tokio::time::sleep(after).await;
        update(
            connection,
            SessionUpdate::ToolCall(
                ToolCall::new(background.clone(), "background build")
                    .status(ToolCallStatus::InProgress),
            ),
        )?;
        tokio::time::sleep(BACKGROUND).await;
        update(
            connection,
            SessionUpdate::ToolCallUpdate(ToolCallUpdate::new(
                background.clone(),
                ToolCallUpdateFields::new().status(ToolCallStatus::Completed),
            )),
        )
    };
    match script {
        Script::AnswersThenWorks => {
            update(
                connection,
                SessionUpdate::AgentMessageChunk(chunk(None, SAID_WHILE_TRAILING)),
            )?;
            runs_in_the_background(Duration::ZERO).await
        }
        Script::AnswersWithAToolOpen => {
            tokio::time::sleep(BACKGROUND).await;
            update(
                connection,
                SessionUpdate::ToolCallUpdate(ToolCallUpdate::new(
                    TOOL_CALL,
                    ToolCallUpdateFields::new().status(ToolCallStatus::Completed),
                )),
            )
        }
        Script::AnswersWithAToolOpenThenExits => {
            tokio::time::sleep(VANISHING).await;
            std::process::exit(0);
        }
        Script::AnswersThenKeepsBooks => {
            let until = tokio::time::Instant::now() + BOOKKEEPING;
            let mut used = 0;
            while tokio::time::Instant::now() < until {
                used += 1;
                update(
                    connection,
                    SessionUpdate::UsageUpdate(UsageUpdate::new(used, 1_000)),
                )?;
                update(
                    connection,
                    SessionUpdate::ConfigOptionUpdate(ConfigOptionUpdate::new(offered(
                        DEFAULT_MODEL,
                    ))),
                )?;
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
            Ok(())
        }
        Script::ResumesAfterSettling => runs_in_the_background(RESUMES_AFTER).await,
        Script::AnswersThenWrites => {
            tokio::time::sleep(VANISHING).await;
            let directory = located.ok_or_else(Error::internal_error)?;
            std::fs::write(directory.join(WRITTEN_WHILE_TRAILING), "left behind\n")
                .map_err(Error::into_internal_error)?;
            update(
                connection,
                SessionUpdate::ToolCall(
                    ToolCall::new(background.clone(), "write a file")
                        .status(ToolCallStatus::Completed),
                ),
            )
        }
        _ => Ok(()),
    }
}

/// The only way a test sees where a credential reached: an agent saying what its own process
/// was spawned with.
fn confided() -> String {
    let mut reached: Vec<String> = std::env::vars()
        .filter(|(name, _)| name.starts_with(CONFIDED))
        .map(|(name, value)| format!("{name}={value}"))
        .collect();
    reached.sort();

    match reached.is_empty() {
        true => "nothing reached this agent".to_owned(),
        false => reached.join(" "),
    }
}

fn refreshed() -> String {
    let Some(home) = std::env::var_os("HOME") else {
        return "this agent has no home".to_owned();
    };
    let login = std::path::Path::new(&home).join(LOGIN);

    match std::fs::read_to_string(&login) {
        Ok(found) => match std::fs::write(&login, format!("{found}{REFRESHED}")) {
            Ok(()) => format!("logged in as {found}"),
            Err(error) => format!("logged in as {found}, and could not refresh it: {error}"),
        },
        Err(_) => "no login was found".to_owned(),
    }
}

fn kept() -> Vec<String> {
    std::fs::read_to_string(KEPT)
        .map(|kept| {
            kept.split(PROMPT_SEPARATOR)
                .filter(|prompted| !prompted.is_empty())
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn keep(prompted: &str) -> Result<()> {
    use std::io::Write as _;

    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(KEPT)
        .and_then(|mut kept| write!(kept, "{prompted}{PROMPT_SEPARATOR}"))
        .map_err(Error::into_internal_error)
}

fn say(connection: &ConnectionTo<Client>, message: &str, said: &str) -> Result<()> {
    update(
        connection,
        SessionUpdate::AgentMessageChunk(chunk(Some(message), said)),
    )
}

fn update(connection: &ConnectionTo<Client>, update: SessionUpdate) -> Result<()> {
    connection.send_notification(SessionNotification::new(SESSION, update))
}

fn chunk(message: Option<&str>, said: &str) -> ContentChunk {
    ContentChunk::new(ContentBlock::Text(TextContent::new(said))).message_id(message.map(|id| {
        MessageId::new(format!(
            "{}-{}-{id}",
            std::process::id(),
            TURN.load(Ordering::Relaxed)
        ))
    }))
}

fn models(current: impl Into<SessionConfigValueId>) -> SessionConfigOption {
    SessionConfigOption::new(
        MODEL_OPTION,
        "Model",
        SessionConfigKind::Select(SessionConfigSelect::new(
            current,
            vec![
                SessionConfigSelectOption::new(DEFAULT_MODEL, DEFAULT_MODEL),
                SessionConfigSelectOption::new(OTHER_MODEL, OTHER_MODEL),
            ],
        )),
    )
    .category(SessionConfigOptionCategory::Model)
}

fn offered(current: impl Into<SessionConfigValueId>) -> Vec<SessionConfigOption> {
    vec![
        models(current),
        SessionConfigOption::new(
            MODE_OPTION,
            "Mode",
            SessionConfigKind::Select(SessionConfigSelect::new(
                STARTING_MODE,
                vec![
                    SessionConfigSelectOption::new(STARTING_MODE, "Build"),
                    SessionConfigSelectOption::new(SWITCHED_MODE, "Plan"),
                ],
            )),
        )
        .category(SessionConfigOptionCategory::Mode),
        SessionConfigOption::new(
            THOUGHT_LEVEL_OPTION,
            "Thinking",
            SessionConfigKind::Select(SessionConfigSelect::new(
                STARTING_THOUGHT_LEVEL,
                vec![
                    SessionConfigSelectOption::new(STARTING_THOUGHT_LEVEL, "Low"),
                    SessionConfigSelectOption::new(OTHER_THOUGHT_LEVEL, "High"),
                ],
            )),
        )
        .category(SessionConfigOptionCategory::ThoughtLevel),
        SessionConfigOption::new(
            CUSTOM_OPTION,
            "Verbose",
            SessionConfigKind::Boolean(
                agent_client_protocol::schema::v1::SessionConfigBoolean::new(false),
            ),
        )
        .category(SessionConfigOptionCategory::Other(
            CUSTOM_CATEGORY.to_owned(),
        )),
    ]
}

fn legacy_modes() -> SessionModeState {
    SessionModeState::new(
        STARTING_MODE,
        vec![
            SessionMode::new(STARTING_MODE, "Build"),
            SessionMode::new(SWITCHED_MODE, "Plan"),
            SessionMode::new(OTHER_MODE, "Review"),
        ],
    )
}

/// Refuses to go on unless the client allows the call once, which is what makes a Session that
/// succeeded evidence that the round-trip completed.
async fn permission_to_use_a_tool(connection: &ConnectionTo<Client>) -> Result<()> {
    let outcome = connection
        .send_request(RequestPermissionRequest::new(
            SESSION,
            ToolCallUpdate::new(TOOL_CALL, ToolCallUpdateFields::new()),
            vec![allow_once(), reject_once()],
        ))
        .block_task()
        .await?
        .outcome;

    let RequestPermissionOutcome::Selected(selected) = outcome else {
        return Err(Error::internal_error()
            .data("the scripted agent was left without permission to proceed"));
    };
    if selected.option_id.0.as_ref() != ALLOW_ONCE {
        return Err(Error::internal_error().data(format!(
            "the scripted agent offered {ALLOW_ONCE} and was answered {}",
            selected.option_id.0
        )));
    }

    Ok(())
}

/// Refuses to go on unless the client answers `cancelled`, which is not a denial.
async fn permission_cancelled(connection: &ConnectionTo<Client>) -> Result<()> {
    let outcome = connection
        .send_request(RequestPermissionRequest::new(
            SESSION,
            ToolCallUpdate::new(TOOL_CALL, ToolCallUpdateFields::new()),
            vec![allow_once(), reject_once()],
        ))
        .block_task()
        .await?
        .outcome;

    if !matches!(outcome, RequestPermissionOutcome::Cancelled) {
        return Err(Error::internal_error()
            .data("a permission request on an interrupted turn was not answered cancelled"));
    }

    Ok(())
}

fn allow_once() -> PermissionOption {
    PermissionOption::new(ALLOW_ONCE, "Allow once", PermissionOptionKind::AllowOnce)
}

fn reject_once() -> PermissionOption {
    PermissionOption::new(
        "reject-once",
        "Reject once",
        PermissionOptionKind::RejectOnce,
    )
}
