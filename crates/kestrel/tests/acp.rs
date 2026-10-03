//! The supervisor driving a Harness over ACP (ADR-0007), against the scripted ACP agent
//! playing a canned sequence over real stdio JSON-RPC.

mod support;

use std::time::Duration;

use kestrel::domain::{Cost, Exit, Session, SessionId, SessionState, Usage, Workspace};
use kestrel_scripted_agent::{DEFAULT_MODEL, MUTTERED, OTHER_MODEL, OVERLONG};
use support::Kestrel;
use support::operator_log;
use support::repository;
use support::scripted_agent::{self, Script};
use support::supervisor::{self, Supervisor};

const PATIENCE: Duration = Duration::from_secs(30);
/// The Harness the fixture actually drives, so what a Session sees it advertise is recorded
/// against the name an Agent declared here names.
use support::HARNESS;

async fn a_workspace(kestrel: &Kestrel) -> Workspace {
    a_workspace_naming(kestrel, Some(OTHER_MODEL)).await
}

async fn a_workspace_naming(kestrel: &Kestrel, model: Option<&str>) -> Workspace {
    let organization = kestrel.declare_organization("acme").await;
    kestrel
        .declare_project(
            &organization,
            repository::NAME,
            &[repository::url().to_owned()],
            repository::BRANCH,
        )
        .await;
    kestrel
        .declare_agent(&organization, "builder", HARNESS, model)
        .await;

    kestrel
        .hold_provider_credential(
            &organization,
            support::PROVIDER_KEY,
            support::A_PROVIDER_KEY,
        )
        .await;

    kestrel.open_workspace("acme", "kestrel", "builder").await
}

/// A Session the work role has started a supervisor for, and is therefore past reading its Agent's
/// model.
async fn in_flight(kestrel: &Kestrel, session: SessionId) {
    let deadline = tokio::time::Instant::now() + PATIENCE;

    loop {
        if kestrel.session(session).await.supervisor.is_some() {
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the session {session} never reached a supervisor"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

async fn worked(script: Script) -> (Kestrel, Workspace, Session) {
    worked_naming(script, Some(OTHER_MODEL)).await
}

async fn worked_naming(script: Script, model: Option<&str>) -> (Kestrel, Workspace, Session) {
    let kestrel =
        Kestrel::dispatching_to(supervisor::binary(), &scripted_agent::playing(script)).await;
    let workspace = a_workspace_naming(&kestrel, model).await;
    let session = kestrel.enqueue_session(workspace.id).await;
    let ended = kestrel.after_one_turn(session.id).await;

    (kestrel, workspace, ended)
}

async fn transcript(kestrel: &Kestrel, workspace: &Workspace) -> Vec<String> {
    kestrel
        .transcript(workspace.id)
        .await
        .iter()
        .map(|entry| entry.entry.to_string())
        .collect()
}

#[tokio::test]
async fn what_the_agent_says_reaches_the_transcript_coalesced_by_the_message_it_belongs_to() {
    let (kestrel, workspace, session) = worked(Script::Speaks).await;

    assert_eq!(session.exit, Some(Exit::Succeeded));
    assert_eq!(
        transcript(&kestrel, &workspace)
            .await
            .into_iter()
            .filter(|entry| entry.starts_with("said  builder"))
            .collect::<Vec<_>>(),
        vec![
            "said  builder  half of one message, and the other half".to_owned(),
            "said  builder  a second message".to_owned(),
        ]
    );

    kestrel.teardown().await;
}

async fn operator_entries(
    kestrel: &Kestrel,
    workspace: &Workspace,
    kinds: Option<&str>,
) -> Vec<serde_json::Value> {
    let mut url = reqwest::Url::parse(&format!(
        "{}/operator/organizations/acme/workspaces/{}/transcript?follow=false",
        kestrel.operator(),
        workspace.id
    ))
    .unwrap();
    if let Some(kinds) = kinds {
        url.query_pairs_mut().append_pair("kinds", kinds);
    }
    let response = reqwest::Client::new().get(url).send().await.unwrap();
    assert!(response.status().is_success());
    response
        .text()
        .await
        .unwrap()
        .split("\n\n")
        .filter_map(|frame| {
            if !frame.contains("event: entry") {
                return None;
            }
            let data = frame
                .lines()
                .find_map(|line| line.strip_prefix("data: "))
                .unwrap();
            Some(serde_json::from_str(data).unwrap())
        })
        .collect()
}

#[tokio::test]
async fn an_agents_plan_reasoning_and_tools_reach_their_transcript_kinds() {
    let (kestrel, workspace, session) = worked(Script::Speaks).await;
    let shared = operator_entries(&kestrel, &workspace, None).await;
    assert!(shared.iter().all(|record| record["kind"] == "shared_state"));
    let all = operator_entries(&kestrel, &workspace, Some("detail,narration,shared_state")).await;
    let narration: Vec<_> = all
        .iter()
        .filter(|record| record["kind"] == "narration")
        .collect();
    assert_eq!(narration.len(), 2);
    assert_eq!(narration[0]["entry"]["type"], "plan");
    assert_eq!(
        narration[0]["entry"]["entries"][0]["content"],
        "read the issue"
    );
    assert_eq!(narration[1]["entry"]["text"], "the issue looks small");
    for record in all
        .iter()
        .filter(|record| record["entry"]["completion"].is_object())
    {
        assert_eq!(record["session_id"], session.id.to_string());
        assert_eq!(record["entry"]["session_id"], session.id.to_string());
        let completion = &record["entry"]["completion"];
        let start: jiff::Timestamp = completion["started_at"].as_str().unwrap().parse().unwrap();
        let finish: jiff::Timestamp = completion["finished_at"].as_str().unwrap().parse().unwrap();
        assert!(start <= finish);
    }
    assert!(
        all.windows(2)
            .all(|pair| pair[0]["seq"].as_i64() < pair[1]["seq"].as_i64())
    );
    let tools: Vec<_> = all
        .iter()
        .filter(|record| record["kind"] == "detail")
        .collect();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0]["entry"]["type"], "tool_call");
    assert_eq!(tools[0]["entry"]["call_id"], "call-1");
    assert_eq!(tools[0]["entry"]["title"], "read README.md");
    assert_eq!(tools[0]["entry"]["status"], "completed");
    kestrel.teardown().await;
}

#[tokio::test]
async fn what_the_agent_writes_to_stderr_reaches_the_operator_log_mid_session_and_no_transcript() {
    let log = operator_log::capturing();
    let kestrel = Kestrel::dispatching_to(
        supervisor::binary(),
        &scripted_agent::playing(Script::Mutters),
    )
    .await;
    let workspace = a_workspace(&kestrel).await;
    let session = kestrel.enqueue_session(workspace.id).await;

    let deadline = tokio::time::Instant::now() + PATIENCE;
    let relayed = loop {
        let relayed: Vec<String> = log
            .about(session.id)
            .into_iter()
            .filter(|line| line.contains("wrote to stderr"))
            .collect();
        if relayed.len() == 2 {
            break relayed;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the session {session} relayed {relayed:?} of what its agent wrote",
            session = session.id
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    };

    assert_eq!(
        kestrel.session(session.id).await.state,
        SessionState::Working
    );
    assert!(relayed[0].contains(MUTTERED), "{}", relayed[0]);
    assert!(
        relayed[1].contains("[truncated]") && relayed[1].len() < OVERLONG,
        "the overlong line was relayed {} bytes long",
        relayed[1].len()
    );
    let transcript = transcript(&kestrel, &workspace).await.join("\n");
    assert!(!transcript.contains(MUTTERED), "{transcript}");

    kestrel.teardown().await;
}

#[tokio::test]
async fn what_the_agent_used_is_recorded_on_the_session_and_reaches_no_transcript() {
    let (kestrel, workspace, session) = worked(Script::Speaks).await;

    assert_eq!(
        session.usage,
        Some(Usage {
            context_used: 1_200,
            context_size: 200_000,
            cost: Some(Cost {
                amount: 0.42,
                currency: "USD".to_owned(),
            }),
        })
    );
    let transcript = transcript(&kestrel, &workspace).await.join("\n");
    assert!(
        !transcript.contains("1200") && !transcript.contains("0.42"),
        "the transcript carries what the agent used:\n{transcript}"
    );

    kestrel.teardown().await;
}

/// The scripted agent refuses to go on unless it is allowed, so a Session that succeeded is a
/// round-trip that completed; the supervisor says which subject it decided about.
#[tokio::test]
async fn a_permission_request_is_answered_and_the_round_trip_is_observable() {
    let kestrel = Kestrel::boot().await;
    let workspace = a_workspace(&kestrel).await;
    let (session, on) = kestrel.dispatch_to_the_link(workspace.id).await;

    let mut supervisor = Supervisor::provision(&kestrel.link(), &on);
    supervisor.wait_until_it_says("reported connected").await;
    kestrel.start(&session, supervisor.harness()).await;
    supervisor.wait_until_it_says("reported answered").await;
    kestrel.stop_session(session.id).await;

    assert!(
        supervisor.said("allowed once  tool call call-1"),
        "the supervisor never said how it answered. it said:\n{}",
        supervisor.everything_it_said()
    );
    assert_eq!(
        kestrel.session(session.id).await.exit,
        Some(Exit::Succeeded),
        "the agent was not allowed to go on"
    );

    supervisor.lets_go_of(session.id).await;
    kestrel.teardown().await;
}

#[tokio::test]
async fn a_turn_that_stops_for_any_other_reason_fails_the_session() {
    let (kestrel, _, session) = worked(Script::Refuses).await;

    let Some(Exit::Failed { because }) = &session.exit else {
        panic!(
            "the session ended {:?}, and its agent refused",
            session.exit
        );
    };
    assert!(
        because.contains("refused"),
        "unhelpful exit status: {because}"
    );

    kestrel.teardown().await;
}

/// A prompt that never became work is not an answer: a turn with no message, narration or
/// detail fails the session rather than reporting itself answered.
#[tokio::test]
async fn a_turn_that_produced_nothing_fails_the_session_and_names_why() {
    let (kestrel, _, session) = worked(Script::Silent).await;

    let Some(Exit::Failed { because }) = &session.exit else {
        panic!(
            "the session ended {:?}, and its agent produced nothing",
            session.exit
        );
    };
    assert!(
        because.contains("answered the prompt with nothing"),
        "unhelpful exit status: {because}"
    );
    assert!(
        kestrel
            .turns(session.id)
            .await
            .iter()
            .all(|turn| turn.answered_at.is_none()),
        "a turn that produced nothing was recorded as answered"
    );

    kestrel.teardown().await;
}

/// Bookkeeping alone is not the agent working, but a tool call or a permission request is,
/// even when nothing is said.
#[tokio::test]
async fn a_turn_that_only_used_a_tool_or_asked_permission_is_answered() {
    for script in [Script::Works, Script::Asks] {
        let (kestrel, _, session) = worked(script).await;

        assert_eq!(session.exit, Some(Exit::Succeeded), "{script:?}");
        assert!(
            kestrel.turns(session.id).await[0].answered_at.is_some(),
            "{script:?}"
        );

        kestrel.teardown().await;
    }
}

#[tokio::test]
async fn an_agent_that_does_not_answer_acp_v1_fails_the_session_rather_than_being_prompted_anyway()
{
    let (kestrel, workspace, session) = worked(Script::Predates).await;

    let Some(Exit::Failed { because }) = &session.exit else {
        panic!(
            "the session ended {:?}, and its agent does not speak v1",
            session.exit
        );
    };
    assert!(because.contains("v1"), "unhelpful exit status: {because}");
    assert!(
        !transcript(&kestrel, &workspace)
            .await
            .iter()
            .any(|entry| entry.starts_with("said  builder")),
        "an agent that was never initialized said something"
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn an_agent_that_can_only_be_logged_into_at_a_terminal_fails_the_session_rather_than_hanging()
{
    let (kestrel, _, session) = worked(Script::Demands).await;

    let Some(Exit::Failed { because }) = &session.exit else {
        panic!(
            "the session ended {:?}, and nobody was at a terminal to log its agent in",
            session.exit
        );
    };
    assert!(
        because.contains("terminal"),
        "unhelpful exit status: {because}"
    );

    kestrel.teardown().await;
}

/// ACP offers a client no way to choose between the login methods an agent advertises, so the
/// one kestrel uses is configuration, and an agent that will not work without one it was not
/// given fails the Session rather than leaving it waiting at a login (ADR-0007).
#[tokio::test]
async fn an_agent_that_will_not_work_until_it_is_logged_in_fails_the_session_rather_than_hanging() {
    let (kestrel, _, session) = worked(Script::Insists).await;

    let Some(Exit::Failed { because }) = &session.exit else {
        panic!(
            "the session ended {:?}, and nothing had logged its agent in",
            session.exit
        );
    };
    assert!(
        because.contains("its-own"),
        "the session failed without naming what the agent offers to be logged in with: {because}"
    );

    kestrel.teardown().await;
}

/// A model reaches the agent through `session/set_config_option` rather than through what the
/// Environment was built with. What the agent was set to is on the Session, where nothing inside
/// the Environment has to be believed.
#[tokio::test]
async fn the_supervisor_sets_the_model_it_was_given() {
    let kestrel = Kestrel::boot().await;
    let workspace = a_workspace(&kestrel).await;
    let (session, on) = kestrel.dispatch_to_the_link(workspace.id).await;

    let mut supervisor =
        Supervisor::provision_selecting(&kestrel.link(), &on, Script::Speaks, OTHER_MODEL);
    supervisor.wait_until_it_says("reported connected").await;
    kestrel.start(&session, supervisor.harness()).await;
    supervisor.wait_until_it_says("reported answered").await;
    kestrel.stop_session(session.id).await;

    let ended = kestrel.session(session.id).await;
    assert_eq!(ended.exit, Some(Exit::Succeeded));
    assert_eq!(ended.worked_model.as_deref(), Some(OTHER_MODEL));

    supervisor.lets_go_of(session.id).await;
    kestrel.teardown().await;
}

#[tokio::test]
async fn a_session_without_a_model_uses_its_agents_model() {
    let (kestrel, _, session) = worked_naming(Script::Speaks, Some(OTHER_MODEL)).await;

    assert_eq!(session.agent.declared.model.as_deref(), Some(OTHER_MODEL));
    assert_eq!(session.worked_model.as_deref(), Some(OTHER_MODEL));

    kestrel.teardown().await;
}

/// The harness's default is the honest answer for a Session that names no model, and a Session that
/// could not say which model that was would leave an audit record that says nothing (ADR-0007).
#[tokio::test]
async fn a_session_and_its_agent_that_name_no_model_use_the_harness_default() {
    let (kestrel, _, session) = worked_naming(Script::Speaks, None).await;

    assert_eq!(session.exit, Some(Exit::Succeeded));
    assert_eq!(session.worked_model.as_deref(), Some(DEFAULT_MODEL));

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_session_that_names_no_model_skips_selection_when_its_harness_offers_none() {
    let (kestrel, _, session) = worked_naming(Script::Decides, None).await;

    assert_eq!(session.exit, Some(Exit::Succeeded));
    assert!(session.worked_model.is_none());

    kestrel.teardown().await;
}

#[tokio::test]
async fn two_sessions_in_one_workspace_can_drive_different_models() {
    let kestrel = Kestrel::dispatching_to(
        supervisor::binary(),
        &scripted_agent::playing(Script::Speaks),
    )
    .await;
    let workspace = a_workspace_naming(&kestrel, Some(OTHER_MODEL)).await;

    let built = kestrel
        .enqueue_session_naming(workspace.id, Some(OTHER_MODEL))
        .await;
    let built = kestrel.after_one_turn(built.id).await;
    let deadline = tokio::time::Instant::now() + PATIENCE;
    let reviewed = loop {
        match kestrel
            .try_enqueue_session_naming(workspace.id, Some(DEFAULT_MODEL))
            .await
        {
            Ok(session) => break session,
            Err(_) if tokio::time::Instant::now() < deadline => {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            Err(error) => panic!("the workspace never took its next session: {error}"),
        }
    };
    let reviewed = kestrel.after_one_turn(reviewed.id).await;

    assert_eq!(built.agent.declared.model.as_deref(), Some(OTHER_MODEL));
    assert_eq!(
        reviewed.agent.declared.model.as_deref(),
        Some(DEFAULT_MODEL)
    );
    assert_eq!(built.worked_model.as_deref(), Some(OTHER_MODEL));
    assert_eq!(reviewed.worked_model.as_deref(), Some(DEFAULT_MODEL));

    kestrel.teardown().await;
}

/// An Agent's model is configuration rather than a rebuild, and a Session already in flight was
/// handed the model it was dispatched with.
#[tokio::test]
async fn changing_an_agents_model_leaves_a_session_already_in_flight_on_the_one_it_started_on() {
    let kestrel = Kestrel::dispatching_to(
        supervisor::binary(),
        &scripted_agent::playing(Script::Lingers),
    )
    .await;
    let workspace = a_workspace_naming(&kestrel, Some(OTHER_MODEL)).await;
    let session = kestrel.enqueue_session(workspace.id).await;
    in_flight(&kestrel, session.id).await;

    kestrel
        .set_agent_model(&workspace.organization, "builder", Some(DEFAULT_MODEL))
        .await;
    assert_eq!(
        kestrel.session(session.id).await.state,
        SessionState::Working,
        "the session was over before its agent's model changed"
    );

    assert_eq!(
        kestrel
            .after_one_turn(session.id)
            .await
            .worked_model
            .as_deref(),
        Some(OTHER_MODEL)
    );

    kestrel.teardown().await;
}

/// Config options are optional and every agent ships a default, so a harness may let no client
/// choose a model at all. Running one on something other than what its Session named would leave
/// an audit record that lies, which is the worst of the three available outcomes (ADR-0007).
#[tokio::test]
async fn an_agent_that_lets_no_client_choose_a_model_fails_a_session_that_named_one() {
    let kestrel = Kestrel::dispatching_to(
        supervisor::binary(),
        &scripted_agent::playing(Script::Decides),
    )
    .await;
    let workspace = a_workspace_naming(&kestrel, None).await;
    let session = kestrel
        .enqueue_session_naming(workspace.id, Some(OTHER_MODEL))
        .await;
    let session = kestrel.after_one_turn(session.id).await;

    assert_eq!(session.agent.declared.model.as_deref(), Some(OTHER_MODEL));

    let Some(Exit::Failed { because }) = &session.exit else {
        panic!(
            "the session ended {:?}, and its harness offers no model to select",
            session.exit
        );
    };
    assert!(
        because.contains("this session named") && because.contains(OTHER_MODEL),
        "the session failed without explaining why its named model could not be selected: {because}"
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_model_the_agent_does_not_offer_fails_the_session_rather_than_falling_back_to_a_default()
{
    let (kestrel, _, session) =
        worked_naming(Script::Speaks, Some("a-model-no-agent-offers")).await;

    let Some(Exit::Failed { because }) = &session.exit else {
        panic!(
            "the session ended {:?}, and its session named a model the harness does not offer",
            session.exit
        );
    };
    assert!(
        because.contains("a-model-no-agent-offers"),
        "unhelpful exit status: {because}"
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn an_agent_that_dies_mid_turn_fails_the_session_rather_than_leaving_it_hanging() {
    let (kestrel, _, session) = worked(Script::Dies).await;

    assert!(
        matches!(session.exit, Some(Exit::Failed { .. })),
        "the session ended {:?}, and its agent died mid-turn",
        session.exit
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_completed_message_is_readable_before_a_slow_turn_answers() {
    let log = operator_log::capturing();
    let kestrel = Kestrel::dispatching_to(
        supervisor::binary(),
        &scripted_agent::playing(Script::ReportsThenWaits),
    )
    .await;
    let workspace = a_workspace(&kestrel).await;
    let session = kestrel.enqueue_session(workspace.id).await;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let entries = operator_entries(&kestrel, &workspace, None).await;
        if entries
            .iter()
            .any(|entry| entry["entry"]["message"] == "first message")
        {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the completed message stayed buffered"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let working = kestrel.session(session.id).await;
    assert_eq!(working.state, SessionState::Working);
    assert_eq!(working.worked_model.as_deref(), Some(OTHER_MODEL));
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        if log
            .about(session.id)
            .iter()
            .any(|line| line.contains("completed message"))
        {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the late update reached no diagnostic"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let all = operator_entries(&kestrel, &workspace, Some("shared_state,narration")).await;
    assert!(!serde_json::to_string(&all).unwrap().contains("late text"));
    let path = kestrel::operator::SESSION
        .replace("{organization}", "acme")
        .replace("{session}", &session.id.to_string());
    // The usage report is a transient one at the window's trailing edge, so it is waited for.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let shown: serde_json::Value = reqwest::Client::new()
            .get(format!("{}{path}", kestrel.operator()))
            .send()
            .await
            .expect("the operator boundary should answer")
            .json()
            .await
            .expect("the session as JSON");
        if shown["usage"]["context_used"] == 12 {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the late usage update did not reach the read: {shown}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    kestrel.teardown().await;
}

#[tokio::test]
async fn failed_and_cancelled_turns_keep_the_text_observed_before_the_boundary() {
    for (script, status) in [
        (Script::CancelledText, "cancelled"),
        (Script::FailedText, "failed"),
    ] {
        let (kestrel, workspace, session) = worked(script).await;
        assert!(matches!(session.exit, Some(Exit::Failed { .. })));
        let entries = operator_entries(&kestrel, &workspace, Some("shared_state,narration")).await;
        let completed: Vec<_> = entries
            .iter()
            .filter(|entry| entry["entry"]["completion"].is_object())
            .collect();
        assert_eq!(completed.len(), 2);
        assert_eq!(completed[0]["entry"]["message"], "observed message");
        assert_eq!(completed[1]["entry"]["text"], "observed thought");
        for entry in completed {
            assert_eq!(
                entry["entry"]["completion"]["turn_outcome"]["status"],
                status
            );
        }
        kestrel.teardown().await;
    }
}

#[tokio::test]
async fn a_running_tool_is_session_state_until_it_settles_into_detail() {
    let kestrel = Kestrel::dispatching_to(
        supervisor::binary(),
        &scripted_agent::playing(Script::SlowTool),
    )
    .await;
    let workspace = a_workspace(&kestrel).await;
    let session = kestrel.enqueue_session(workspace.id).await;
    let url = format!(
        "{}/operator/organizations/acme/sessions/{}",
        kestrel.operator(),
        session.id
    );
    let deadline = tokio::time::Instant::now() + PATIENCE;
    loop {
        let record: serde_json::Value = reqwest::get(&url).await.unwrap().json().await.unwrap();
        if record["tools"]
            .as_array()
            .is_some_and(|tools| !tools.is_empty())
        {
            assert_eq!(record["tools"][0]["title"], "slow read");
            assert_eq!(record["tools"][0]["status"], "in_progress");
            assert!(
                record["tools"][0]["started_at"]
                    .as_str()
                    .unwrap()
                    .parse::<jiff::Timestamp>()
                    .is_ok()
            );
            assert!(
                operator_entries(&kestrel, &workspace, Some("detail"))
                    .await
                    .is_empty()
            );
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "tool never appeared on its Session"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    kestrel.after_one_turn(session.id).await;
    let record: serde_json::Value = reqwest::get(&url).await.unwrap().json().await.unwrap();
    assert_eq!(record["tools"], serde_json::json!([]));
    let detail = operator_entries(&kestrel, &workspace, Some("detail")).await;
    assert_eq!(detail.len(), 1);
    assert_eq!(
        detail[0]["entry"]["input"],
        serde_json::json!({"path":"README.md"})
    );
    assert_eq!(
        detail[0]["entry"]["result"]["output"],
        serde_json::json!({"text":"read result"})
    );
    assert_eq!(detail[0]["entry"]["status"], "completed");
    kestrel.teardown().await;
}

#[tokio::test]
async fn open_calls_close_at_answer_cancel_and_failure_once() {
    for (script, reason) in [
        (Script::OpenToolAnswered, "unresolved"),
        (Script::OpenToolCancelled, "interrupted"),
        (Script::OpenToolFailed, "failed"),
    ] {
        let (kestrel, workspace, _) = worked(script).await;
        let detail = operator_entries(&kestrel, &workspace, Some("detail")).await;
        assert_eq!(
            detail.len(),
            1,
            "{script:?}: {}",
            serde_json::to_string(
                &operator_entries(&kestrel, &workspace, Some("shared_state,narration,detail"))
                    .await
            )
            .unwrap()
        );
        assert_eq!(detail[0]["entry"]["status"], "in_progress");
        assert_eq!(detail[0]["entry"]["closing_reason"], reason);
        let narration = reqwest::get(format!(
            "{}/operator/organizations/acme/workspaces/{}/transcript?follow=false&kinds=narration",
            kestrel.operator(),
            workspace.id
        ))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
        let activity: serde_json::Value = serde_json::from_str(
            narration
                .split("event: activity\n")
                .nth(1)
                .and_then(|frame| frame.lines().find_map(|line| line.strip_prefix("data: ")))
                .unwrap(),
        )
        .unwrap();
        assert_eq!(activity["counts"]["tool_calls"], 1, "{script:?}");
        assert_eq!(activity["counts"]["failed_calls"], 0, "{script:?}");
        kestrel.teardown().await;
    }
}

async fn session_tools(kestrel: &Kestrel, session: SessionId) -> serde_json::Value {
    reqwest::get(format!(
        "{}/operator/organizations/acme/sessions/{session}",
        kestrel.operator()
    ))
    .await
    .unwrap()
    .json::<serde_json::Value>()
    .await
    .unwrap()["tools"]
        .clone()
}

#[tokio::test]
async fn reconnect_restores_running_tools_and_omits_tools_that_settled_offline() {
    let kestrel = Kestrel::boot().await;
    let workspace = a_workspace(&kestrel).await;
    let (session, on) = kestrel.dispatch_to_the_link(workspace.id).await;
    let mut supervisor =
        Supervisor::provision_playing(&kestrel.link(), &on, Script::ReconnectingTools);
    kestrel.start(&session, supervisor.harness()).await;
    let deadline = tokio::time::Instant::now() + PATIENCE;
    while session_tools(&kestrel, session.id)
        .await
        .as_array()
        .unwrap()
        .len()
        != 2
    {
        assert!(tokio::time::Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let kestrel = kestrel.kill_and_restart().await;
    while session_tools(&kestrel, session.id)
        .await
        .as_array()
        .unwrap()
        .len()
        != 2
    {
        assert!(
            tokio::time::Instant::now() < deadline,
            "running snapshot was not restored"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let stopped = kestrel.kill().await;
    supervisor.wait_until_it_says("lost the link").await;
    tokio::time::sleep(Duration::from_secs(5)).await;
    let kestrel = stopped.restart().await;
    loop {
        let tools = session_tools(&kestrel, session.id).await;
        if tools.as_array().unwrap().len() == 1 {
            assert_eq!(tools[0]["call_id"], "still-running");
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "offline completion left stale tool state"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    kestrel.after_one_turn(session.id).await;
    assert_eq!(
        session_tools(&kestrel, session.id).await,
        serde_json::json!([])
    );
    let detail = operator_entries(&kestrel, &workspace, Some("detail")).await;
    assert_eq!(detail.len(), 2);
    assert!(
        detail
            .iter()
            .all(|entry| entry["entry"]["status"] == "completed")
    );
    supervisor.destroy();
    kestrel.teardown().await;
}

#[tokio::test]
async fn an_ended_session_never_shows_its_stale_running_tools() {
    let kestrel = Kestrel::dispatching_to(
        supervisor::binary(),
        &scripted_agent::playing(Script::SlowTool),
    )
    .await;
    let workspace = a_workspace(&kestrel).await;
    let session = kestrel.enqueue_session(workspace.id).await;
    let deadline = tokio::time::Instant::now() + PATIENCE;
    while session_tools(&kestrel, session.id)
        .await
        .as_array()
        .unwrap()
        .is_empty()
    {
        assert!(tokio::time::Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let response = reqwest::Client::new()
        .post(format!(
            "{}/operator/organizations/acme/sessions/{}/stop",
            kestrel.operator(),
            session.id
        ))
        .send()
        .await
        .unwrap();
    assert!(response.status().is_success());
    assert_eq!(
        session_tools(&kestrel, session.id).await,
        serde_json::json!([])
    );
    kestrel.teardown().await;
}

#[tokio::test]
async fn an_oversized_scripted_tool_result_is_referenced_and_fetchable() {
    let kestrel = Kestrel::boot().await;
    let workspace = a_workspace(&kestrel).await;
    let (session, on) = kestrel.dispatch_to_the_link(workspace.id).await;
    let supervisor = Supervisor::provision_playing(&kestrel.link(), &on, Script::OversizedTool);
    kestrel.start(&session, supervisor.harness()).await;
    kestrel.after_one_turn(session.id).await;
    let detail = operator_entries(&kestrel, &workspace, Some("detail")).await;
    assert_eq!(detail.len(), 1);
    assert_eq!(
        detail[0]["entry"]["input"],
        serde_json::json!({"path":"large.txt"})
    );
    let result = &detail[0]["entry"]["result"];
    assert!(result["bytes"].as_u64().unwrap() > 64 * 1024);
    assert_eq!(result["media_type"], "application/json");
    let linked: serde_json::Value = reqwest::Client::new()
        .get(format!(
            "{}/link/instances/{}/entries?kinds=detail",
            kestrel.link(),
            on.instance.replace('/', "%2F")
        ))
        .bearer_auth(on.credential.as_str())
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        linked["entries"][0]["entry"]["result"]["output"]["text"],
        "x".repeat(70 * 1024)
    );
    assert_eq!(
        linked["entries"][0]["entry"]["result"]["content"],
        serde_json::json!([])
    );
    assert!(linked["entries"][0]["entry"]["payload_fields"].is_null());
    let payload = result["payload_id"].as_str().unwrap();
    let response = reqwest::get(format!(
        "{}/operator/organizations/acme/workspaces/{}/transcript/payloads/{payload}",
        kestrel.operator(),
        workspace.id
    ))
    .await
    .unwrap();
    assert!(response.status().is_success());
    let fetched: serde_json::Value = response.json().await.unwrap();
    assert_eq!(fetched["output"]["text"], "x".repeat(70 * 1024));
    assert_eq!(fetched["content"], serde_json::json!([]));
    supervisor.destroy();
    kestrel.teardown().await;
}
