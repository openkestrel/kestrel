//! A Session whose Turn has answered trails while its agent keeps working, and is waiting only
//! once that work settles and the agent falls quiet (ADR-0040).

mod support;

use std::time::Duration;

use jiff::{SignedDuration, Timestamp};
use kestrel::domain::{Session, SessionId, SessionState, Workspace, WorkspaceState};
use kestrel::instance::{Git, Observed};
use kestrel::log::{ClosingReason, Entry, ToolStatus};
use kestrel::work::Occupied;
use kestrel_scripted_agent::{
    BACKGROUND, BACKGROUND_TASK, BOOKKEEPING, OTHER_MODEL, SAID_WHILE_TRAILING, TASK_RUNS,
    UNKNOWN_UPDATE,
};
use support::scripted_agent::{self, Script};
use support::supervisor::Supervisor;
use support::{HARNESS, Kestrel, QUIET_PERIOD, operator_log, repository, supervisor};

const PATIENCE: Duration = Duration::from_secs(30);

async fn a_workspace(kestrel: &Kestrel) -> Workspace {
    a_workspace_in(kestrel, None).await
}

async fn a_workspace_in(kestrel: &Kestrel, instance_limit: Option<usize>) -> Workspace {
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

    kestrel.open_workspace("acme", "kestrel", "builder").await
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
async fn a_call_still_open_when_trailing_ends_closes_unresolved_once() {
    let (kestrel, workspace, session) = playing(Script::AnswersWithAToolOpenThenExits).await;
    ended(&kestrel, session.id).await;

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
    listing_a_unit(&kestrel, session.id).await;

    let kestrel = kestrel.kill_and_restart().await;
    let read = listing_a_unit(&kestrel, session.id).await;
    assert_eq!(read["state"], "trailing");
    assert_eq!(read["units"][0]["title"], BACKGROUND_TASK);

    supervisor.destroy();
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
