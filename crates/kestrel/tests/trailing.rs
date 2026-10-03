//! A Session whose Turn has answered trails while its agent keeps working, and is waiting only
//! once that work settles and the agent falls quiet (ADR-0040).

mod support;

use std::time::Duration;

use jiff::{SignedDuration, Timestamp};
use kestrel::domain::{
    Direction, Exit, OnOpenWorkspace, Session, SessionId, SessionState, Workspace, WorkspaceState,
};
use kestrel::instance::{Git, Observed};
use kestrel::link::Instruction;
use kestrel::log::{ClosingReason, Entry, ToolStatus};
use kestrel::scheduling::Occupied;
use kestrel_scripted_agent::{
    BACKGROUND, BACKGROUND_TASK, BOOKKEEPING, CHILD_SAID_IN_TURN, CHILD_SAID_WHILE_TRAILING,
    CHILD_TITLE, OTHER_MODEL, SAID_BY_SUBAGENT, SAID_WHILE_TRAILING, SUBAGENT, SUBAGENT_CALL,
    TASK_RUNS, UNKNOWN_UPDATE,
};
use support::github_stub::{self, GithubStub};
use support::scripted_agent::{self, Script};
use support::supervisor::Supervisor;
use support::{
    HARNESS, Kestrel, QUIET_PERIOD, labelled_on, operator_log, repository, supervisor, templates,
};

const PATIENCE: Duration = Duration::from_secs(30);

async fn a_workspace(kestrel: &Kestrel) -> Workspace {
    a_workspace_in(kestrel, None).await
}

async fn a_workspace_in(kestrel: &Kestrel, instance_limit: Option<usize>) -> Workspace {
    an_organization(kestrel, instance_limit).await;

    kestrel.open_workspace("acme", "kestrel", "builder").await
}

async fn an_organization(kestrel: &Kestrel, instance_limit: Option<usize>) {
    let organization = match instance_limit {
        Some(limit) => kestrel.declare_limited_organization("acme", limit).await,
        None => kestrel.declare_organization("acme").await,
    };
    kestrel
        .declare_project(
            &organization,
            repository::NAME,
            &[repository::url().to_owned()],
            repository::BRANCH,
        )
        .await;
    kestrel
        .declare_agent(&organization, "builder", HARNESS, Some(OTHER_MODEL))
        .await;
    kestrel
        .hold_provider_credential(
            &organization,
            support::PROVIDER_KEY,
            support::A_PROVIDER_KEY,
        )
        .await;
}

async fn playing(script: Script) -> (Kestrel, Workspace, Session) {
    playing_command(&scripted_agent::playing(script)).await
}

/// Played by an agent that sees none of the declarations kestrel makes at `initialize`.
async fn playing_unaware(script: Script) -> (Kestrel, Workspace, Session) {
    playing_command(&format!("{} --unaware", scripted_agent::playing(script))).await
}

async fn playing_command(command: &str) -> (Kestrel, Workspace, Session) {
    let kestrel = Kestrel::dispatching_to(supervisor::binary(), command).await;
    let workspace = a_workspace(&kestrel).await;
    let session = kestrel.enqueue_session(workspace.id).await;

    (kestrel, workspace, session)
}

async fn session_read(kestrel: &Kestrel, session: SessionId) -> serde_json::Value {
    reqwest::get(format!(
        "{}/operator/organizations/acme/sessions/{session}",
        kestrel.operator()
    ))
    .await
    .unwrap()
    .json()
    .await
    .unwrap()
}

async fn tool_calls(kestrel: &Kestrel, workspace: &Workspace) -> Vec<Entry> {
    kestrel
        .every_entry(workspace.id)
        .await
        .into_iter()
        .filter(|entry| matches!(entry, Entry::ToolCall { .. }))
        .collect()
}

async fn ended(kestrel: &Kestrel, session: SessionId) -> Session {
    let deadline = tokio::time::Instant::now() + PATIENCE;
    loop {
        let session = kestrel.session(session).await;
        if session.state == SessionState::Ended {
            return session;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the session {} is {} and never ended",
            session.id,
            session.state
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn position(entries: &[Entry], what: &str, found: impl Fn(&Entry) -> bool) -> usize {
    entries
        .iter()
        .position(found)
        .unwrap_or_else(|| panic!("no {what} in {entries:#?}"))
}

#[tokio::test]
async fn output_after_an_answer_trails_it_and_lands_before_the_next_prompt() {
    let (kestrel, workspace, session) = playing(Script::AnswersThenWorks).await;
    kestrel.answering(session.id, 1).await;

    let deadline = tokio::time::Instant::now() + PATIENCE;
    loop {
        let read = session_read(&kestrel, session.id).await;
        if read["tools"]
            .as_array()
            .is_some_and(|tools| !tools.is_empty())
        {
            assert_eq!(read["state"], "trailing");
            assert_eq!(read["tools"][0]["title"], "background build");
            assert!(
                read["last_activity_at"]
                    .as_str()
                    .is_some_and(|at| at.parse::<Timestamp>().is_ok()),
                "a trailing Session says when its agent was last active: {read}"
            );
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the work after the answer never showed"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    let waiting = kestrel.answered(session.id, 1).await;
    assert_eq!(waiting.state, SessionState::Waiting);
    assert_eq!(
        session_read(&kestrel, session.id).await["last_activity_at"],
        serde_json::Value::Null
    );
    kestrel
        .post(workspace.id, "jack", "and the next thing")
        .await;
    kestrel.answering(session.id, 2).await;

    let entries = kestrel.every_entry(workspace.id).await;
    let answer = position(
        &entries,
        "answer",
        |entry| matches!(entry, Entry::Said { message, .. } if message == "answered, with more to do"),
    );
    let late = position(
        &entries,
        "message after the answer",
        |entry| matches!(entry, Entry::Said { message, .. } if message == SAID_WHILE_TRAILING),
    );
    let background = position(&entries, "background call", |entry| {
        matches!(
            entry,
            Entry::ToolCall { title, status: ToolStatus::Completed, closing_reason: None, .. }
                if title == "background build"
        )
    });
    let prompt = position(&entries, "next prompt", |entry| {
        matches!(entry, Entry::Messages { .. })
    });
    assert!(
        answer < late && late < prompt && answer < background && background < prompt,
        "{entries:#?}"
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_call_open_at_the_answer_keeps_its_session_trailing_until_it_settles_and_its_agent_falls_quiet()
 {
    let (kestrel, workspace, session) = playing(Script::AnswersWithAToolOpen).await;
    kestrel.answering(session.id, 1).await;

    let deadline = tokio::time::Instant::now() + PATIENCE;
    let settled_at = loop {
        let state = kestrel.session(session.id).await.state;
        if !tool_calls(&kestrel, &workspace).await.is_empty() {
            break tokio::time::Instant::now();
        }
        assert_eq!(state, SessionState::Trailing, "the call is still running");
        assert!(tokio::time::Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(20)).await;
    };
    let waiting = kestrel.answered(session.id, 1).await;
    assert_eq!(waiting.state, SessionState::Waiting);
    assert!(
        settled_at.elapsed() >= QUIET_PERIOD / 2,
        "the Session was waiting {:?} after its call settled",
        settled_at.elapsed()
    );

    let calls = tool_calls(&kestrel, &workspace).await;
    assert!(
        matches!(
            calls.as_slice(),
            [Entry::ToolCall {
                status: ToolStatus::Completed,
                closing_reason: None,
                ..
            }]
        ),
        "{calls:#?}"
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn an_agent_that_answers_and_falls_silent_is_waiting_once_the_quiet_period_passes() {
    let (kestrel, workspace, session) = playing(Script::Speaks).await;
    assert_eq!(
        kestrel.answering(session.id, 1).await.state,
        SessionState::Trailing
    );

    assert_eq!(
        kestrel.answered(session.id, 1).await.state,
        SessionState::Waiting
    );
    let unresolved: Vec<_> = tool_calls(&kestrel, &workspace)
        .await
        .into_iter()
        .filter(|entry| {
            matches!(
                entry,
                Entry::ToolCall {
                    closing_reason: Some(ClosingReason::Unresolved),
                    ..
                }
            )
        })
        .collect();
    assert!(unresolved.is_empty(), "{unresolved:#?}");

    kestrel.teardown().await;
}

#[tokio::test]
async fn an_agent_that_exits_cleanly_while_trailing_ends_its_session_as_a_waiting_one_would() {
    let (kestrel, workspace, session) = playing(Script::AnswersWithAToolOpenThenExits).await;
    let ended = ended(&kestrel, session.id).await;

    let Some(Exit::Failed { because }) = &ended.exit else {
        panic!("the session ended {:?}, and its agent exited", ended.exit);
    };
    assert!(
        because.contains("process was lost") && because.contains("cannot resume"),
        "{because}"
    );

    let calls = tool_calls(&kestrel, &workspace).await;
    assert!(
        matches!(
            calls.as_slice(),
            [Entry::ToolCall {
                status: ToolStatus::InProgress,
                closing_reason: Some(ClosingReason::Unresolved),
                ..
            }]
        ),
        "{calls:#?}"
    );

    kestrel.teardown().await;
}

/// The Session read once it is `state` with a call running.
async fn running(kestrel: &Kestrel, session: SessionId, state: &str) -> serde_json::Value {
    let deadline = tokio::time::Instant::now() + PATIENCE;
    loop {
        let read = session_read(kestrel, session).await;
        if read["state"] == state
            && read["tools"]
                .as_array()
                .is_some_and(|tools| !tools.is_empty())
        {
            return read;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the session never read {state} with a call running: {read}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

async fn stopped(kestrel: &Kestrel, session: SessionId) {
    let response = reqwest::Client::new()
        .post(format!(
            "{}/operator/organizations/acme/sessions/{session}/stop",
            kestrel.operator()
        ))
        .send()
        .await
        .unwrap();
    assert!(response.status().is_success(), "{response:?}");
}

#[tokio::test]
async fn a_call_open_when_the_next_turn_starts_runs_on_into_it_and_settles_there() {
    let (kestrel, workspace, session) = playing(Script::CarriesAToolOver).await;
    running(&kestrel, session.id, "trailing").await;

    kestrel
        .post_while_busy(workspace.id, "jack", "and the next thing")
        .await;
    let read = running(&kestrel, session.id, "working").await;
    assert_eq!(read["tools"][0]["title"], "background tests");
    kestrel.answering(session.id, 2).await;

    let entries = kestrel.every_entry(workspace.id).await;
    let prompt = position(&entries, "next prompt", |entry| {
        matches!(entry, Entry::Messages { .. })
    });
    let settled = position(&entries, "carried call", |entry| {
        matches!(
            entry,
            Entry::ToolCall { title, status: ToolStatus::Completed, closing_reason: None, .. }
                if title == "background tests"
        )
    });
    assert!(prompt < settled, "{entries:#?}");
    assert_eq!(tool_calls(&kestrel, &workspace).await.len(), 1);

    kestrel.stop_session(session.id).await;
    kestrel.teardown().await;
}

#[tokio::test]
async fn stopping_a_trailing_session_succeeds_and_closes_its_open_calls_interrupted() {
    let (kestrel, workspace, session) = playing(Script::CarriesAToolOver).await;
    running(&kestrel, session.id, "trailing").await;

    stopped(&kestrel, session.id).await;

    assert_eq!(
        kestrel.session(session.id).await.exit,
        Some(Exit::Succeeded)
    );
    assert_eq!(
        session_read(&kestrel, session.id).await["tools"],
        serde_json::json!([])
    );
    let calls = tool_calls(&kestrel, &workspace).await;
    assert!(
        matches!(
            calls.as_slice(),
            [Entry::ToolCall {
                title,
                tool_kind,
                status: ToolStatus::InProgress,
                closing_reason: Some(ClosingReason::Interrupted),
                result,
                ..
            }] if title == "background tests" && tool_kind == "execute" && result.is_null()
        ),
        "{calls:#?}"
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn stopping_a_working_session_records_its_open_call_interrupted() {
    let (kestrel, workspace, session) = playing(Script::WorksUntilCancelled).await;
    running(&kestrel, session.id, "working").await;

    stopped(&kestrel, session.id).await;

    assert!(matches!(
        kestrel.session(session.id).await.exit,
        Some(Exit::Failed { .. })
    ));
    let calls = tool_calls(&kestrel, &workspace).await;
    assert!(
        matches!(
            calls.as_slice(),
            [Entry::ToolCall {
                title,
                closing_reason: Some(ClosingReason::Interrupted),
                ..
            }] if title == "a long read"
        ),
        "{calls:#?}"
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn bookkeeping_after_an_answer_does_not_keep_its_session_trailing() {
    let (kestrel, _, session) = playing(Script::AnswersThenKeepsBooks).await;
    kestrel.answering(session.id, 1).await;

    let waiting = kestrel
        .answered_within(session.id, 1, BOOKKEEPING - QUIET_PERIOD * 2)
        .await;
    assert_eq!(waiting.state, SessionState::Waiting);

    kestrel.teardown().await;
}

#[tokio::test]
async fn activity_after_its_work_settled_trails_a_waiting_session_again() {
    let (kestrel, workspace, session) = playing(Script::ResumesAfterSettling).await;
    assert_eq!(
        kestrel.answered(session.id, 1).await.state,
        SessionState::Waiting
    );

    let deadline = tokio::time::Instant::now() + PATIENCE;
    while kestrel.session(session.id).await.state != SessionState::Trailing {
        assert!(
            tokio::time::Instant::now() < deadline,
            "the waiting Session never trailed again"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    let waiting = kestrel
        .answered_within(session.id, 1, BACKGROUND + PATIENCE)
        .await;
    assert_eq!(waiting.state, SessionState::Waiting);
    assert_eq!(tool_calls(&kestrel, &workspace).await.len(), 1);

    kestrel.teardown().await;
}

async fn listing_a_unit(kestrel: &Kestrel, session: SessionId) -> serde_json::Value {
    let deadline = tokio::time::Instant::now() + PATIENCE;
    loop {
        let read = session_read(kestrel, session).await;
        if read["units"]
            .as_array()
            .is_some_and(|units| !units.is_empty())
        {
            return read;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the Session never listed a unit: {read}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test]
async fn a_silent_background_task_keeps_its_session_trailing_until_it_settles() {
    let (kestrel, _workspace, session) = playing(Script::AnswersWithABackgroundTask).await;
    kestrel.answering(session.id, 1).await;

    let read = listing_a_unit(&kestrel, session.id).await;
    assert_eq!(read["state"], "trailing");
    let unit = &read["units"][0];
    assert_eq!(unit["kind"], "background_task");
    assert_eq!(unit["title"], BACKGROUND_TASK);
    assert!(
        unit["started_at"]
            .as_str()
            .is_some_and(|at| at.parse::<Timestamp>().is_ok()),
        "a unit says when it started: {unit}"
    );

    tokio::time::sleep(QUIET_PERIOD * 2).await;
    let silent = session_read(&kestrel, session.id).await;
    assert_eq!(silent["state"], "trailing");
    assert_eq!(silent["units"][0]["title"], BACKGROUND_TASK);

    let waiting = kestrel.answered(session.id, 1).await;
    assert_eq!(waiting.state, SessionState::Waiting);
    assert_eq!(
        session_read(&kestrel, session.id).await["units"],
        serde_json::json!([])
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn an_agent_that_did_not_see_the_declaration_trails_by_the_baseline() {
    let (kestrel, _workspace, session) = playing_unaware(Script::AnswersWithABackgroundTask).await;
    kestrel.answering(session.id, 1).await;
    let answered = tokio::time::Instant::now();

    loop {
        let read = session_read(&kestrel, session.id).await;
        assert_eq!(read["units"], serde_json::json!([]), "{read}");
        if read["state"] != "trailing" {
            assert_eq!(read["state"], "waiting");
            break;
        }
        assert!(
            answered.elapsed() < TASK_RUNS,
            "the Session trailed as long as a task it was never told of"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_session_update_kestrel_does_not_know_is_a_diagnostic_and_the_session_carries_on() {
    let log = operator_log::capturing();
    let (kestrel, workspace, session) = playing_unaware(Script::AnswersWithABackgroundTask).await;

    let deadline = tokio::time::Instant::now() + PATIENCE;
    while !log
        .about(session.id)
        .iter()
        .any(|line| line.contains(UNKNOWN_UPDATE))
    {
        assert!(
            tokio::time::Instant::now() < deadline,
            "the unknown update reached no diagnostic"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(
        kestrel.answered(session.id, 1).await.state,
        SessionState::Waiting
    );
    kestrel.post(workspace.id, "jack", "and again").await;
    assert_eq!(
        kestrel.answered(session.id, 2).await.state,
        SessionState::Waiting
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_running_background_task_is_listed_again_after_the_supervisor_reconnects() {
    let kestrel = Kestrel::boot().await;
    let workspace = a_workspace(&kestrel).await;
    let (session, on) = kestrel.dispatch_to_the_link(workspace.id).await;
    let supervisor =
        Supervisor::provision_playing(&kestrel.link(), &on, Script::AnswersWithABackgroundTask);
    kestrel.start(&session, supervisor.harness()).await;
    kestrel.answering(session.id, 1).await;
    listing_a_unit(&kestrel, session.id).await;

    let kestrel = kestrel.kill_and_restart().await;
    let read = listing_a_unit(&kestrel, session.id).await;
    assert_eq!(read["state"], "trailing");
    assert_eq!(read["units"][0]["title"], BACKGROUND_TASK);
    assert!(tool_calls(&kestrel, &workspace).await.is_empty());

    supervisor.destroy();
    kestrel.teardown().await;
}

#[tokio::test]
async fn a_subagents_tool_calls_and_messages_reach_the_parent_transcript_as_before_the_declaration()
{
    for declared in [false, true] {
        let (kestrel, workspace, session) = match declared {
            false => playing_unaware(Script::AnswersWithSubagents).await,
            true => playing(Script::AnswersWithSubagents).await,
        };
        assert_eq!(
            kestrel.answered(session.id, 1).await.state,
            SessionState::Waiting
        );

        let entries = kestrel.every_entry(workspace.id).await;
        position(&entries, "subagent's call", |entry| {
            matches!(
                entry,
                Entry::ToolCall { title, status: ToolStatus::Completed, closing_reason: None, session_id, .. }
                    if title == SUBAGENT_CALL && *session_id == session.id
            )
        });
        position(
            &entries,
            "subagent's message",
            |entry| matches!(entry, Entry::Said { message, session_id, .. } if message == SAID_BY_SUBAGENT && *session_id == Some(session.id)),
        );

        kestrel.teardown().await;
    }
}

#[tokio::test]
async fn two_subagents_using_one_tool_call_id_produce_two_detail_entries() {
    let (kestrel, workspace, session) = playing(Script::AnswersWithSubagents).await;
    kestrel.answered(session.id, 1).await;

    let calls: Vec<String> = tool_calls(&kestrel, &workspace)
        .await
        .into_iter()
        .filter_map(|entry| match entry {
            Entry::ToolCall { title, call_id, .. } if title == SUBAGENT_CALL => Some(call_id),
            _ => None,
        })
        .collect();
    assert_eq!(calls.len(), 2, "{calls:?}");
    assert_ne!(calls[0], calls[1]);

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_silent_subagent_keeps_its_session_trailing_until_it_finishes() {
    let (kestrel, _workspace, session) = playing(Script::AnswersWithSubagents).await;
    kestrel.answering(session.id, 1).await;

    let read = listing_a_unit(&kestrel, session.id).await;
    assert_eq!(read["state"], "trailing");
    let unit = &read["units"][0];
    assert_eq!(read["units"].as_array().unwrap().len(), 1, "{read}");
    assert_eq!(unit["kind"], "subagent");
    assert_eq!(unit["title"], SUBAGENT);

    tokio::time::sleep(QUIET_PERIOD * 2).await;
    let silent = session_read(&kestrel, session.id).await;
    assert_eq!(silent["state"], "trailing");
    assert_eq!(silent["units"][0]["kind"], "subagent");

    let waiting = kestrel.answered(session.id, 1).await;
    assert_eq!(waiting.state, SessionState::Waiting);
    assert_eq!(
        session_read(&kestrel, session.id).await["units"],
        serde_json::json!([])
    );

    kestrel.teardown().await;
}

#[cfg(unix)]
#[tokio::test]
async fn the_unpublished_work_hold_reads_the_checkout_taken_when_trailing_ends() {
    let (kestrel, workspace, session) = playing(Script::AnswersThenWrites).await;
    kestrel.answered(session.id, 1).await;
    kestrel.stop_session(session.id).await;

    let held = kestrel.held_instances("acme").await;
    assert_eq!(
        held.len(),
        1,
        "the work written while trailing holds nothing"
    );
    assert_eq!(held[0].workspace, workspace.id);
    assert!(
        held[0].because.ends_with("1 untracked file"),
        "{}",
        held[0].because
    );

    kestrel.teardown().await;
}

fn clean_checkout() -> Vec<Observed> {
    vec![Observed {
        repository: repository::url().to_owned(),
        git: Git::Read {
            branch: Some(repository::BRANCH.to_owned()),
            untracked: 0,
            uncommitted: 0,
            stashes: 0,
            unpushed: 0,
        },
    }]
}

/// Driven by the test itself through the reports a supervisor makes: a clean checkout, then the
/// answer.
async fn trailing(kestrel: &Kestrel, workspace: &Workspace, instance: &str) -> Session {
    let queued = kestrel.enqueue_session(workspace.id).await;
    answered_on(kestrel, &queued, instance).await
}

async fn answered_on(kestrel: &Kestrel, queued: &Session, instance: &str) -> Session {
    let session = kestrel
        .occupy_session()
        .await
        .expect("the session should claim");
    assert_eq!(session.id, queued.id);
    kestrel.executes_on(&session, instance).await;
    kestrel.start_on_the_link(&session).await;
    kestrel.report_checkout(&session, clean_checkout()).await;
    kestrel.report_answered(&session, 2).await;

    let trailing = kestrel.session(session.id).await;
    assert_eq!(trailing.state, SessionState::Trailing);
    trailing
}

#[tokio::test]
async fn a_trailing_session_holds_its_active_work_slot_until_its_work_settles() {
    let kestrel = Kestrel::boot().await;
    let first = a_workspace(&kestrel).await;
    let second = kestrel.open_workspace("acme", "kestrel", "builder").await;
    let session = trailing(&kestrel, &first, "trailing").await;

    let next = kestrel.enqueue_session(second.id).await;
    assert!(kestrel.occupy_up_to(1).await.is_none());

    kestrel.report_settled(&session, 3).await;
    assert_eq!(
        kestrel.session(session.id).await.state,
        SessionState::Waiting
    );
    match kestrel.occupy_up_to(1).await {
        Some(Occupied::Claimed(claimed)) => assert_eq!(claimed.id, next.id),
        _ => panic!("the slot the trailing Session held should be free"),
    }

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_message_to_a_trailing_session_starts_its_turn_at_once_with_every_slot_occupied() {
    let kestrel = Kestrel::boot().await;
    let first = a_workspace(&kestrel).await;
    let second = kestrel.open_workspace("acme", "kestrel", "builder").await;
    let session = trailing(&kestrel, &first, "trailing").await;
    kestrel.enqueue_session(second.id).await;
    assert!(kestrel.occupy_up_to(1).await.is_none());

    let posted = kestrel
        .posted_while_busy(first.id, "jack", "and the next thing")
        .await;

    assert!(posted.held_message.is_none(), "{posted:?}");
    assert_eq!(
        kestrel.session(session.id).await.state,
        SessionState::Working
    );
    assert_eq!(kestrel.turns(session.id).await.len(), 2);
    assert!(
        matches!(
            kestrel.instruction(&session).await,
            Instruction::Prompt { turn: 2, prompt } if prompt == "and the next thing"
        ),
        "the message is the next Turn's prompt"
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_firing_that_continues_a_trailing_session_starts_its_turn_at_once() {
    const REPOSITORY: &str = "jtmthf/kestrel";
    const EVENTS: &str = "/issues/events?";
    let correlated = |name: &str, label: &str| {
        (
            name.to_owned(),
            labelled_on(REPOSITORY, label),
            templates(
                support::BRIEF,
                None,
                Some("{{ event.source }}{{ event.subject }}"),
            ),
        )
    };
    let stub = GithubStub::start();
    stub.script_answer(
        "GET",
        EVENTS,
        github_stub::page(&[github_stub::labelled(7, 43, "ready-for-agent")]),
    );
    let kestrel = Kestrel::boot().await;
    an_organization(&kestrel, None).await;
    for (name, filter, templates) in [
        correlated("ready", "ready-for-agent"),
        correlated("ci", "ci-failed"),
    ] {
        kestrel
            .declare_correlated_trigger(
                "acme",
                &name,
                &filter,
                "builder",
                &[],
                &templates,
                OnOpenWorkspace::Continue,
            )
            .await;
    }
    kestrel
        .register_integration(
            "acme",
            "github",
            REPOSITORY,
            &stub.base_url(),
            &[Direction::Inbound],
            SignedDuration::from_millis(1),
        )
        .await;
    let deadline = tokio::time::Instant::now() + PATIENCE;
    let queued = loop {
        if let Some(workspace) = kestrel.workspaces("acme").await.first()
            && let Some(queued) = kestrel.sessions(workspace.id).await.into_iter().next()
        {
            break queued;
        }
        assert!(tokio::time::Instant::now() < deadline, "nothing opened");
        tokio::time::sleep(Duration::from_millis(20)).await;
    };
    let session = answered_on(&kestrel, &queued, "trailing").await;

    stub.script_answer(
        "GET",
        EVENTS,
        github_stub::page(&[github_stub::labelled(8, 43, "ci-failed")]),
    );

    while kestrel.session(session.id).await.state != SessionState::Working {
        assert!(
            tokio::time::Instant::now() < deadline,
            "the firing never continued the trailing Session"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(kestrel.turns(session.id).await.len(), 2);
    assert!(matches!(
        kestrel.instruction(&session).await,
        Instruction::Prompt { turn: 2, .. }
    ));

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_trailing_session_refuses_seal_and_the_idle_sweep() {
    let kestrel = Kestrel::boot().await;
    let workspace = a_workspace(&kestrel).await;
    let session = trailing(&kestrel, &workspace, "trailing").await;

    let refusal = kestrel
        .try_seal_workspace(workspace.id)
        .await
        .expect_err("a trailing Session is in flight");
    assert!(
        refusal.to_string().contains(&session.id.to_string()),
        "{refusal}"
    );

    kestrel
        .last_active(
            &workspace,
            Timestamp::now() - SignedDuration::from_hours(25),
        )
        .await;
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert_eq!(
        kestrel.show_workspace(workspace.id).await.state,
        WorkspaceState::Open,
        "the idle sweep sealed a Workspace whose Session trails"
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn cap_reclamation_never_archives_a_trailing_sessions_instance() {
    let kestrel = Kestrel::boot().await;
    let first = a_workspace_in(&kestrel, Some(1)).await;
    let arriving = kestrel.open_workspace("acme", "kestrel", "builder").await;
    let session = trailing(&kestrel, &first, "trailing").await;
    kestrel
        .last_active(&first, Timestamp::now() - SignedDuration::from_hours(2))
        .await;

    kestrel.enqueue_session(arriving.id).await;
    assert!(kestrel.occupy_session().await.is_none());
    assert!(kestrel.instances_to_archive().await.is_empty());
    assert_eq!(
        kestrel.instance(first.id).await.as_deref(),
        Some("trailing")
    );

    kestrel.report_settled(&session, 3).await;
    assert!(kestrel.occupy_session().await.is_none());
    assert_eq!(kestrel.instances_to_archive().await, ["trailing"]);

    kestrel.teardown().await;
}

#[test]
fn the_published_documents_describe_trailing() {
    let document = |name: &str| -> serde_json::Value {
        serde_json::from_str(
            &std::fs::read_to_string(support::crate_root().join("../../openapi").join(name))
                .expect("a readable openapi document"),
        )
        .expect("valid json")
    };
    let operator = document("operator.json");
    let session = &operator["components"]["schemas"]["Session"];
    assert!(
        session["properties"]["state"]["enum"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!("trailing"))
    );
    assert!(
        session["required"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!("last_activity_at"))
    );

    let link = document("link.json");
    let schemas = &link["components"]["schemas"];
    assert_eq!(
        schemas["Report"]["discriminator"]["mapping"]["settled"],
        "#/components/schemas/Settled"
    );
    assert_eq!(
        schemas["SessionState"]["properties"]["last_activity_at"]["format"],
        "date-time"
    );
    for document in [&operator, &link] {
        assert_eq!(
            document["components"]["schemas"]["RunningUnit"]["properties"]["kind"]["enum"],
            serde_json::json!(["background_task", "subagent"])
        );
    }
    assert!(
        session["required"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!("units"))
    );
}

#[tokio::test]
async fn an_opencode_childs_output_in_a_turn_reaches_its_parents_transcript() {
    let (kestrel, workspace, session) = playing(Script::RunsAnOpenCodeChild).await;
    kestrel.answering(session.id, 1).await;

    let entries = kestrel.every_entry(workspace.id).await;
    let read = position(&entries, "the child's tool call", |entry| {
        matches!(
            entry,
            Entry::ToolCall { call_id, title, status: ToolStatus::Completed, .. }
                if call_id == "child-1:call-1" && *title == format!("{CHILD_TITLE}: read")
        )
    });
    let said = position(
        &entries,
        "the child's message",
        |entry| matches!(entry, Entry::Said { message, .. } if message == CHILD_SAID_IN_TURN),
    );
    let answer = position(
        &entries,
        "answer",
        |entry| matches!(entry, Entry::Said { message, .. } if message == "the child is still looking"),
    );
    assert!(read < answer && said < answer, "{entries:#?}");

    kestrel.teardown().await;
}

#[tokio::test]
async fn an_opencode_childs_output_after_the_answer_lands_before_the_next_prompt() {
    let (kestrel, workspace, session) = playing(Script::RunsAnOpenCodeChild).await;
    kestrel.answered(session.id, 1).await;
    kestrel
        .post(workspace.id, "jack", "and the next thing")
        .await;
    kestrel.answering(session.id, 2).await;

    let entries = kestrel.every_entry(workspace.id).await;
    let answer = position(
        &entries,
        "answer",
        |entry| matches!(entry, Entry::Said { message, .. } if message == "the child is still looking"),
    );
    let late = position(
        &entries,
        "the child's message after the answer",
        |entry| matches!(entry, Entry::Said { message, .. } if message == CHILD_SAID_WHILE_TRAILING),
    );
    let search = position(&entries, "the child's call after the answer", |entry| {
        matches!(
            entry,
            Entry::ToolCall { call_id, status: ToolStatus::Completed, .. }
                if call_id == "child-1:call-1-background"
        )
    });
    let prompt = position(&entries, "next prompt", |entry| {
        matches!(entry, Entry::Messages { .. })
    });
    assert!(
        answer < late && late < prompt && answer < search && search < prompt,
        "{entries:#?}"
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_running_opencode_child_keeps_its_session_trailing_as_a_subagent_unit_until_it_completes()
{
    let (kestrel, _workspace, session) = playing(Script::RunsAnOpenCodeChild).await;
    kestrel.answering(session.id, 1).await;

    let read = listing_a_unit(&kestrel, session.id).await;
    assert_eq!(read["state"], "trailing");
    let unit = &read["units"][0];
    assert_eq!(unit["kind"], "subagent");
    assert_eq!(unit["title"], CHILD_TITLE);

    tokio::time::sleep(QUIET_PERIOD * 2).await;
    let silent = session_read(&kestrel, session.id).await;
    assert_eq!(silent["state"], "trailing");
    assert_eq!(silent["units"][0]["kind"], "subagent");

    let waiting = kestrel.answered(session.id, 1).await;
    assert_eq!(waiting.state, SessionState::Waiting);
    assert_eq!(
        session_read(&kestrel, session.id).await["units"],
        serde_json::json!([])
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_lapsed_supervisor_closes_its_open_tool_unresolved() {
    let kestrel = Kestrel::boot().await;
    let workspace = a_workspace(&kestrel).await;
    let (session, on) = kestrel.dispatch_to_the_link(workspace.id).await;
    let supervisor = Supervisor::provision_playing(&kestrel.link(), &on, Script::CarriesAToolOver);
    kestrel.start(&session, supervisor.harness()).await;
    running(&kestrel, session.id, "trailing").await;
    supervisor.destroy();
    kestrel
        .lease_until(&session, Timestamp::now() - SignedDuration::from_secs(1))
        .await;
    ended(&kestrel, session.id).await;
    let calls = tool_calls(&kestrel, &workspace).await;
    assert!(
        matches!(calls.as_slice(), [Entry::ToolCall {
        title, closing_reason: Some(ClosingReason::Unresolved), result, ..
    }] if title == "background tests" && result.is_null()),
        "{calls:#?}"
    );
    let read = session_read(&kestrel, session.id).await;
    assert_eq!(read["tools"], serde_json::json!([]));
    assert_eq!(read["units"], serde_json::json!([]));
    kestrel.teardown().await;
}

#[tokio::test]
async fn a_lapsed_supervisor_closes_its_open_adapter_units_unresolved() {
    for (script, kind, title) in [
        (
            Script::AnswersWithABackgroundTask,
            "background_task",
            BACKGROUND_TASK,
        ),
        (Script::AnswersWithSubagents, "subagent", SUBAGENT),
    ] {
        let kestrel = Kestrel::boot().await;
        let workspace = a_workspace(&kestrel).await;
        let (session, on) = kestrel.dispatch_to_the_link(workspace.id).await;
        let supervisor = Supervisor::provision_playing(&kestrel.link(), &on, script);
        kestrel.start(&session, supervisor.harness()).await;
        let before = listing_a_unit(&kestrel, session.id).await;
        let unit = &before["units"][0];
        supervisor.destroy();
        kestrel
            .lease_until(&session, Timestamp::now() - SignedDuration::from_secs(1))
            .await;
        ended(&kestrel, session.id).await;
        let calls = tool_calls(&kestrel, &workspace).await;
        let unresolved: Vec<_> = calls
            .iter()
            .filter(|entry| {
                matches!(
                    entry,
                    Entry::ToolCall {
                        closing_reason: Some(ClosingReason::Unresolved),
                        ..
                    }
                )
            })
            .collect();
        assert!(
            matches!(unresolved.as_slice(), [Entry::ToolCall {
            call_id, title: recorded_title, tool_kind, result, completion, ..
        }] if call_id == &format!("unit/{}", unit["id"].as_str().unwrap())
            && recorded_title == title && tool_kind == kind && result.is_null()
            && completion.started_at.to_string() == unit["started_at"].as_str().unwrap()),
            "{calls:#?}"
        );
        let read = session_read(&kestrel, session.id).await;
        assert_eq!(read["tools"], serde_json::json!([]));
        assert_eq!(read["units"], serde_json::json!([]));
        kestrel.teardown().await;
    }
}

#[tokio::test]
async fn a_supervisor_that_exits_without_reporting_closes_its_open_work_unresolved() {
    for script in [Script::CarriesAToolOver, Script::AnswersWithABackgroundTask] {
        let (kestrel, workspace, session) = playing(script).await;
        if script == Script::CarriesAToolOver {
            running(&kestrel, session.id, "trailing").await;
        } else {
            listing_a_unit(&kestrel, session.id).await;
        }
        let instance = kestrel.session(session.id).await.instance.unwrap();
        kestrel::compute::Driver::LocalExec(kestrel::compute::LocalExec::running(
            supervisor::binary(),
        ))
        .destroy_named(&instance)
        .unwrap();
        let session = ended(&kestrel, session.id).await;
        assert!(
            matches!(session.exit, Some(Exit::Failed { because }) if because.contains("supervisor exited"))
        );
        let calls = tool_calls(&kestrel, &workspace).await;
        assert!(
            matches!(
                calls.as_slice(),
                [Entry::ToolCall {
                    closing_reason: Some(ClosingReason::Unresolved),
                    ..
                }]
            ),
            "{calls:#?}"
        );
        let read = session_read(&kestrel, session.id).await;
        assert_eq!(read["tools"], serde_json::json!([]));
        assert_eq!(read["units"], serde_json::json!([]));
        kestrel.teardown().await;
    }
}

#[tokio::test]
async fn supervisor_loss_does_not_record_an_already_recorded_call_again() {
    use kestrel::live_work::{RunningTool, SessionState as LiveSession};
    use kestrel::work::{Report, Reported};
    use support::link_client::Link;

    let kestrel = Kestrel::boot().await;
    let workspace = a_workspace(&kestrel).await;
    let (session, on) = kestrel.dispatch_to_the_link(workspace.id).await;
    let supervisor =
        Supervisor::provision_playing(&kestrel.link(), &on, Script::AnswersWithSubagents);
    kestrel.start(&session, supervisor.harness()).await;
    kestrel.answering(session.id, 1).await;
    let before = tool_calls(&kestrel, &workspace).await;
    let Entry::ToolCall {
        call_id,
        title,
        tool_kind,
        status,
        completion,
        ..
    } = &before[0]
    else {
        panic!("the scripted subagent recorded no tool call");
    };
    let stale = RunningTool {
        call_id: call_id.clone(),
        title: title.clone(),
        tool_kind: tool_kind.clone(),
        status: *status,
        started_at: completion.started_at,
    };
    let link = Link::to(&kestrel.link());
    let _stream = link.open(&on.instance, &on.credential, None).await;
    supervisor.destroy();
    let state = LiveSession {
        tools: vec![stale],
        ..LiveSession::default()
    };
    let response = link
        .report(
            &on.instance,
            Some(&on.credential),
            &Reported {
                session: Some(session.id),
                seq: None,
                report: Report::SessionState {
                    tools: state.tools,
                    units: state.units,
                    message_buffering: false,
                    thought_buffering: false,
                    usage: None,
                    last_activity_at: None,
                },
            },
        )
        .await;
    assert_eq!(response.status(), reqwest::StatusCode::ACCEPTED);
    kestrel
        .lease_until(&session, Timestamp::now() - SignedDuration::from_secs(1))
        .await;
    ended(&kestrel, session.id).await;
    assert_eq!(
        serde_json::to_value(tool_calls(&kestrel, &workspace).await).unwrap(),
        serde_json::to_value(before).unwrap()
    );
    assert_eq!(
        session_read(&kestrel, session.id).await["tools"],
        serde_json::json!([])
    );
    drop(_stream);
    kestrel.teardown().await;
}

#[tokio::test]
async fn a_supervisor_reconnecting_within_its_lease_keeps_its_tool_open() {
    let kestrel = Kestrel::boot().await;
    let workspace = a_workspace(&kestrel).await;
    let (session, on) = kestrel.dispatch_to_the_link(workspace.id).await;
    let supervisor = Supervisor::provision_playing(&kestrel.link(), &on, Script::CarriesAToolOver);
    kestrel.start(&session, supervisor.harness()).await;
    let before = running(&kestrel, session.id, "trailing").await;
    let kestrel = kestrel.kill_and_restart().await;
    let after = running(&kestrel, session.id, "trailing").await;
    assert_eq!(before["tools"], after["tools"]);
    assert!(tool_calls(&kestrel, &workspace).await.is_empty());
    supervisor.destroy();
    kestrel.teardown().await;
}
