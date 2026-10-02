mod support;

use std::time::Duration;

use kestrel::domain::{Session, SessionId, SessionState, Workspace, WorkspaceId};
use kestrel::link::Instruction;
use kestrel::log::{ClosingReason, Entry};
use support::repository;
use support::scripted_agent::{self, Script};
use support::supervisor;
use support::{A_PROVIDER_KEY, HARNESS, Kestrel, PROVIDER_KEY};

const PATIENCE: Duration = Duration::from_secs(30);

async fn until_session(
    kestrel: &Kestrel,
    id: SessionId,
    what: &str,
    ready: impl Fn(&Session) -> bool,
) -> Session {
    let deadline = tokio::time::Instant::now() + PATIENCE;

    loop {
        let session = kestrel.session(id).await;
        if ready(&session) {
            return session;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the session {} is {} with the exit status {:?}, and never {what}",
            session.id,
            session.state,
            session.exit
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

async fn until_entries(
    kestrel: &Kestrel,
    workspace: WorkspaceId,
    what: &str,
    ready: impl Fn(&[Entry]) -> bool,
) -> Vec<Entry> {
    let deadline = tokio::time::Instant::now() + PATIENCE;

    loop {
        let entries = entries(kestrel, workspace).await;
        if ready(&entries) {
            return entries;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the workspace {workspace} never {what}: {entries:?}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

async fn entries(kestrel: &Kestrel, workspace: WorkspaceId) -> Vec<Entry> {
    kestrel.every_entry(workspace).await
}

async fn a_workspace(kestrel: &Kestrel) -> Workspace {
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
        .declare_agent(
            &organization,
            "builder",
            HARNESS,
            Some(kestrel_scripted_agent::OTHER_MODEL),
        )
        .await;
    kestrel
        .hold_provider_credential(&organization, PROVIDER_KEY, A_PROVIDER_KEY)
        .await;

    kestrel.open_workspace("acme", "kestrel", "builder").await
}

/// A Session the control plane believes is mid-Turn, without a harness behind it.
async fn a_working_session(kestrel: &Kestrel) -> Session {
    let workspace = a_workspace(kestrel).await;
    let session = kestrel.dispatch_session(workspace.id).await;
    kestrel.on_the_link(&session).await;
    kestrel.start(&session, support::harness()).await;

    session
}

fn said(entries: &[Entry], message: &str) -> bool {
    entries
        .iter()
        .any(|entry| matches!(entry, Entry::Said { message: said, .. } if said == message))
}

#[tokio::test]
async fn an_interrupt_records_who_asked_and_a_second_one_sends_nothing_more() {
    let kestrel = Kestrel::boot().await;
    let session = a_working_session(&kestrel).await;

    let interrupted = kestrel
        .interrupt(session.id, "alice")
        .await
        .expect("a working turn should interrupt");
    assert_eq!(interrupted.state, SessionState::Working);
    let interrupting = interrupted
        .interrupting
        .expect("the request is on the Session while it is in flight");
    assert_eq!(interrupting.participant, "alice");

    let again = kestrel
        .interrupt(session.id, "bob")
        .await
        .expect("a second interrupt is taken, not refused");
    assert_eq!(
        again
            .interrupting
            .map(|interrupting| interrupting.participant),
        Some("alice".to_owned()),
        "the first request stands"
    );

    let instructions = kestrel.instructions(&session).await;
    assert_eq!(
        instructions
            .iter()
            .filter(|instruction| matches!(instruction, Instruction::Interrupt { .. }))
            .count(),
        1,
        "a double interrupt sends one cancel: {instructions:?}"
    );

    for (participant, because) in [
        ("builder", "is an Agent in this Organization"),
        ("", "names its participant"),
    ] {
        let refusal = kestrel
            .interrupt(session.id, participant)
            .await
            .expect_err("the name rule applies");
        assert!(refusal.to_string().contains(because), "{refusal}");
    }

    kestrel.teardown().await;
}

#[tokio::test]
async fn an_interrupt_is_refused_for_queued_waiting_and_ended_sessions() {
    let kestrel = Kestrel::boot().await;

    let waiting = {
        let workspace = a_workspace(&kestrel).await;
        let session = kestrel.dispatch_session(workspace.id).await;
        kestrel.waits_after_its_first_turn(&session).await
    };
    let ended = {
        let workspace = a_workspace(&kestrel).await;
        let session = kestrel.dispatch_session(workspace.id).await;
        let waiting = kestrel.waits_after_its_first_turn(&session).await;
        kestrel.stop_session(waiting.id).await;
        kestrel.session(waiting.id).await
    };
    let queued = {
        let workspace = a_workspace(&kestrel).await;
        kestrel
            .enqueue_session_with_nothing_posted(workspace.id)
            .await
    };

    for (session, phase) in [(queued, "queued"), (waiting, "waiting"), (ended, "ended")] {
        let before = kestrel.session(session.id).await;
        let sent = kestrel.instructions(&session).await;
        let refusal = kestrel
            .interrupt(session.id, "alice")
            .await
            .expect_err("only a working turn can be interrupted");
        assert!(
            refusal.to_string().contains(phase),
            "the refusal names the phase {phase}: {refusal}"
        );
        let after = kestrel.session(session.id).await;
        assert_eq!(
            after.state, before.state,
            "a refused interrupt changes nothing"
        );
        assert!(after.interrupting.is_none());
        assert_eq!(
            kestrel.instructions(&session).await,
            sent,
            "a refused interrupt sends nothing"
        );
    }

    kestrel.teardown().await;
}

#[tokio::test]
async fn an_interrupt_is_refused_for_unbriefed_and_unreachable_sessions() {
    let kestrel = Kestrel::dispatching_to(
        supervisor::binary(),
        &scripted_agent::playing(Script::ReportsThenWaits),
    )
    .await;
    let workspace = a_workspace(&kestrel).await;
    let blocker = kestrel.dispatch_session(workspace.id).await;
    let unbriefed_workspace = a_workspace(&kestrel).await;
    let unbriefed = kestrel
        .enqueue_session_with_nothing_posted(unbriefed_workspace.id)
        .await;
    let unbriefed = until_session(&kestrel, unbriefed.id, "unbriefed", |session| {
        session.state == SessionState::Unbriefed
    })
    .await;

    let dependent_workspace = a_workspace(&kestrel).await;
    let dependent = kestrel
        .enqueue_blocked(dependent_workspace.id, &blocker)
        .await;
    kestrel.fail_session(&blocker, "the blocker failed").await;
    let unreachable = until_session(&kestrel, dependent.id, "unreachable", |session| {
        session.state == SessionState::Unreachable
    })
    .await;

    for (session, phase) in [(unbriefed, "unbriefed"), (unreachable, "unreachable")] {
        let refusal = kestrel
            .interrupt(session.id, "alice")
            .await
            .expect_err("only a working turn can be interrupted");
        assert!(
            refusal.to_string().contains(phase),
            "the refusal names the phase {phase}: {refusal}"
        );
        assert!(kestrel.session(session.id).await.interrupting.is_none());
    }

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_turn_that_answers_before_the_cancel_lands_stays_answered() {
    let kestrel = Kestrel::boot().await;
    let session = a_working_session(&kestrel).await;
    let workspace = session.workspace;

    kestrel
        .interrupt(session.id, "alice")
        .await
        .expect("a working turn should interrupt");
    kestrel.report_answered(&session, 1).await;

    let after = kestrel.session(session.id).await;
    assert_eq!(after.state, SessionState::Waiting);
    assert!(
        after.interrupting.is_none(),
        "a turn that answered clears the interrupt with nothing written"
    );
    assert!(
        !entries(&kestrel, workspace)
            .await
            .iter()
            .any(|entry| matches!(entry, Entry::TurnInterrupted { .. })),
        "the turn answered as it would have"
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_session_that_ends_before_the_cancel_lands_no_longer_reads_as_interrupting() {
    let kestrel = Kestrel::boot().await;

    let stopped = a_working_session(&kestrel).await;
    kestrel
        .interrupt(stopped.id, "alice")
        .await
        .expect("a working turn should interrupt");
    kestrel.stop_session(stopped.id).await;

    let failed = a_working_session(&kestrel).await;
    kestrel
        .interrupt(failed.id, "alice")
        .await
        .expect("a working turn should interrupt");
    kestrel.fail_session(&failed, "the harness crashed").await;

    for (session, how) in [(stopped, "stopped"), (failed, "failed")] {
        let after = kestrel.session(session.id).await;
        assert_eq!(after.state, SessionState::Ended);
        assert!(
            after.interrupting.is_none(),
            "a {how} Session is not still interrupting: {:?}",
            after.interrupting
        );
    }

    kestrel.teardown().await;
}

#[tokio::test]
async fn an_interrupted_turn_that_produced_nothing_leaves_the_session_unfailed() {
    let kestrel = Kestrel::boot().await;
    let session = a_working_session(&kestrel).await;

    kestrel
        .interrupt(session.id, "alice")
        .await
        .expect("a working turn should interrupt");
    kestrel.report_interrupted(&session).await;

    let after = kestrel.session(session.id).await;
    assert_eq!(after.state, SessionState::Waiting);
    assert!(
        after.exit.is_none(),
        "an interrupted turn that produced nothing never fails its Session: {:?}",
        after.exit
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn an_interrupted_report_takes_held_messages_at_once_on_the_slot_it_held() {
    let kestrel = Kestrel::boot().await;
    let session = a_working_session(&kestrel).await;
    let workspace = session.workspace;
    let held = kestrel
        .posted_while_busy(workspace, "alice", "the next thing")
        .await
        .held_message
        .expect("a mid-turn message is held");

    kestrel
        .interrupt(session.id, "alice")
        .await
        .expect("a working turn should interrupt");
    kestrel.report_interrupted(&session).await;

    let after = kestrel.session(session.id).await;
    assert_eq!(
        after.state,
        SessionState::Working,
        "the slot the interrupted turn held goes straight to the next turn"
    );
    assert!(after.interrupting.is_none());

    let turns = kestrel.turns(session.id).await;
    assert_eq!(turns.len(), 2, "the held message is the next Turn at once");
    assert!(turns[1].answered_at.is_none());
    assert!(
        entries(&kestrel, workspace).await.iter().any(|entry| {
            entry
                == &Entry::Messages {
                    messages: vec![kestrel::log::Message {
                        participant: "alice".to_owned(),
                        message: "the next thing".to_owned(),
                    }],
                }
        }),
        "the held message was taken"
    );
    assert!(
        kestrel
            .instructions(&session)
            .await
            .iter()
            .any(|instruction| {
                matches!(instruction, Instruction::Prompt { prompt, .. } if prompt == "the next thing")
            }),
        "the held message reached the agent"
    );
    assert!(
        entries(&kestrel, workspace).await.iter().any(|entry| {
            matches!(
                entry,
                Entry::TurnInterrupted { session: said, participant }
                    if *said == session.id && participant == "alice"
            )
        }),
        "the interruption is on the record, naming who asked"
    );
    assert!(
        kestrel.held_messages(workspace).await.is_empty(),
        "the held message is no longer held: {}",
        held.id
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn an_interrupted_scripted_turn_records_who_asked_and_keeps_what_it_said() {
    let kestrel = Kestrel::dispatching_to(
        supervisor::binary(),
        &scripted_agent::playing(Script::WorksUntilCancelled),
    )
    .await;
    let workspace = a_workspace(&kestrel).await;
    let session = kestrel.enqueue_session(workspace.id).await;
    until_entries(&kestrel, workspace.id, "the agent working", |entries| {
        said(entries, "working on it")
    })
    .await;

    kestrel
        .interrupt(session.id, "operator")
        .await
        .expect("a working turn should interrupt");
    let waiting = until_session(&kestrel, session.id, "waiting", |session| {
        session.state == SessionState::Waiting
    })
    .await;
    assert!(waiting.exit.is_none(), "an interrupted turn never fails");
    assert!(waiting.interrupting.is_none());

    let entries = until_entries(&kestrel, workspace.id, "the interruption", |entries| {
        entries
            .iter()
            .any(|entry| matches!(entry, Entry::TurnInterrupted { .. }))
    })
    .await;
    assert!(
        said(&entries, "working on it"),
        "what the agent said before the interrupt stays: {entries:?}"
    );
    assert!(
        entries.iter().any(|entry| matches!(
            entry,
            Entry::ToolCall {
                closing_reason: Some(ClosingReason::Interrupted),
                ..
            }
        )),
        "the open tool call closed interrupted: {entries:?}"
    );
    assert!(
        entries.iter().any(|entry| matches!(
            entry,
            Entry::TurnInterrupted { session: said, participant }
                if *said == session.id && participant == "operator"
        )),
        "the interruption names who asked: {entries:?}"
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn an_interrupted_turn_answers_a_permission_request_cancelled() {
    // The script asks for permission only after the cancel arrives, and fails its conversation
    // unless the answer is `cancelled`: a Waiting Session is the proof the round trip completed.
    let kestrel = Kestrel::dispatching_to(
        supervisor::binary(),
        &scripted_agent::playing(Script::WorksUntilCancelled),
    )
    .await;
    let workspace = a_workspace(&kestrel).await;
    let session = kestrel.enqueue_session(workspace.id).await;
    until_entries(&kestrel, workspace.id, "the agent working", |entries| {
        said(entries, "working on it")
    })
    .await;

    kestrel
        .interrupt(session.id, "operator")
        .await
        .expect("a working turn should interrupt");
    let waiting = until_session(&kestrel, session.id, "waiting", |session| {
        session.state == SessionState::Waiting
    })
    .await;

    assert!(waiting.exit.is_none(), "{:?}", waiting.exit);
    kestrel.teardown().await;
}

#[tokio::test]
async fn with_nothing_held_the_session_waits_and_the_next_post_goes_on() {
    let kestrel = Kestrel::dispatching_to(
        supervisor::binary(),
        &scripted_agent::playing(Script::WorksUntilCancelled),
    )
    .await;
    let workspace = a_workspace(&kestrel).await;
    let session = kestrel.enqueue_session(workspace.id).await;
    until_entries(&kestrel, workspace.id, "the agent working", |entries| {
        said(entries, "working on it")
    })
    .await;

    kestrel
        .interrupt(session.id, "operator")
        .await
        .expect("a working turn should interrupt");
    until_session(&kestrel, session.id, "waiting", |session| {
        session.state == SessionState::Waiting
    })
    .await;

    kestrel.post(workspace.id, "operator", "carry on").await;
    let answered = kestrel.answered(session.id, 2).await;

    assert_eq!(answered.id, session.id, "the same Session goes on");
    assert_eq!(
        kestrel.sessions(workspace.id).await.len(),
        1,
        "the interrupt started no Session of its own"
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_held_message_becomes_the_next_turn_at_once() {
    let kestrel = Kestrel::dispatching_to(
        supervisor::binary(),
        &scripted_agent::playing(Script::WorksUntilCancelled),
    )
    .await;
    let workspace = a_workspace(&kestrel).await;
    let session = kestrel.enqueue_session(workspace.id).await;
    until_entries(&kestrel, workspace.id, "the agent working", |entries| {
        said(entries, "working on it")
    })
    .await;
    let held = kestrel
        .posted_while_busy(workspace.id, "operator", "and then this")
        .await
        .held_message
        .expect("a mid-turn message is held");

    kestrel
        .interrupt(session.id, "operator")
        .await
        .expect("a working turn should interrupt");
    let answered = kestrel.answered(session.id, 2).await;

    assert_eq!(answered.id, session.id);
    assert_eq!(kestrel.sessions(workspace.id).await.len(), 1);
    assert!(
        kestrel
            .instructions(&session)
            .await
            .iter()
            .any(|instruction| {
                matches!(instruction, Instruction::Prompt { prompt, .. } if prompt == "and then this")
            }),
        "the held message went to the agent at once: {}",
        held.id
    );
    assert!(kestrel.held_messages(workspace.id).await.is_empty());

    kestrel.teardown().await;
}

#[tokio::test]
async fn an_interrupt_for_a_turn_that_already_answered_leaves_the_next_turn_working() {
    let kestrel = Kestrel::dispatching_to(
        supervisor::binary(),
        &scripted_agent::playing(Script::AnswersThenWorksUntilCancelled),
    )
    .await;
    let workspace = a_workspace(&kestrel).await;
    let session = kestrel.enqueue_session(workspace.id).await;
    kestrel.answered(session.id, 1).await;

    kestrel.post(workspace.id, "operator", "carry on").await;
    until_entries(&kestrel, workspace.id, "the agent working", |entries| {
        said(entries, "working on it")
    })
    .await;
    kestrel
        .instruct(&session, Instruction::Interrupt { turn: 1 })
        .await;

    let deadline = tokio::time::Instant::now() + Duration::from_secs(1);
    while tokio::time::Instant::now() < deadline {
        let session = kestrel.session(session.id).await;
        assert_eq!(
            session.state,
            SessionState::Working,
            "a cancel naming the answered turn left the next one alone"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    kestrel
        .interrupt(session.id, "operator")
        .await
        .expect("the second turn is still working");
    until_entries(&kestrel, workspace.id, "the interruption", |entries| {
        entries.iter().any(|entry| {
            matches!(
                entry,
                Entry::TurnInterrupted { participant, .. } if participant == "operator"
            )
        })
    })
    .await;

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_harness_that_ignores_the_cancel_ends_the_session_as_lost_continuity() {
    let kestrel = Kestrel::dispatching_with_a_quick_interrupt(
        supervisor::binary(),
        &scripted_agent::playing(Script::IgnoresCancel),
        Duration::from_secs(1),
    )
    .await;
    let workspace = a_workspace(&kestrel).await;
    let session = kestrel.enqueue_session(workspace.id).await;
    until_entries(&kestrel, workspace.id, "the agent working", |entries| {
        said(entries, "ignoring the cancel")
    })
    .await;

    kestrel
        .interrupt(session.id, "operator")
        .await
        .expect("a working turn should interrupt");
    let ended = until_session(&kestrel, session.id, "ended", |session| {
        session.state == SessionState::Ended
    })
    .await;

    let because = match ended.exit {
        Some(kestrel::domain::Exit::Failed { because }) => because,
        other => panic!("an interrupted harness that never answered failed: {other:?}"),
    };
    assert!(
        because.contains("ACP continuity"),
        "the failure names the lost continuity: {because}"
    );
    assert!(
        entries(&kestrel, workspace.id)
            .await
            .iter()
            .any(|entry| matches!(
                entry,
                Entry::ToolCall {
                    closing_reason: Some(ClosingReason::Unresolved),
                    ..
                }
            )),
        "the open unit closed unresolved"
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn an_interrupt_while_the_supervisor_is_off_the_link_is_delivered_on_reconnect() {
    let kestrel = Kestrel::dispatching_to(
        supervisor::binary(),
        &scripted_agent::playing(Script::WorksUntilCancelled),
    )
    .await;
    let workspace = a_workspace(&kestrel).await;
    let session = kestrel.enqueue_session(workspace.id).await;
    until_entries(&kestrel, workspace.id, "the agent working", |entries| {
        said(entries, "working on it")
    })
    .await;

    let kestrel = kestrel.kill_and_restart().await;
    kestrel
        .interrupt(session.id, "operator")
        .await
        .expect("the session is still working after the restart");
    let waiting = until_session(&kestrel, session.id, "waiting", |session| {
        session.state == SessionState::Waiting
    })
    .await;
    assert!(waiting.interrupting.is_none());
    assert!(
        entries(&kestrel, workspace.id)
            .await
            .iter()
            .any(|entry| matches!(entry, Entry::TurnInterrupted { .. })),
        "the replayed interrupt landed"
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_cancelled_turn_kestrel_did_not_ask_for_still_ends_the_session() {
    let kestrel = Kestrel::dispatching_to(
        supervisor::binary(),
        &scripted_agent::playing(Script::OpenToolCancelled),
    )
    .await;
    let workspace = a_workspace(&kestrel).await;
    let session = kestrel.enqueue_session(workspace.id).await;

    let ended = until_session(&kestrel, session.id, "ended", |session| {
        session.state == SessionState::Ended
    })
    .await;
    assert!(
        matches!(ended.exit, Some(kestrel::domain::Exit::Failed { .. })),
        "a cancel kestrel did not send ends the Session: {:?}",
        ended.exit
    );
    assert!(ended.interrupting.is_none());

    let entries = until_entries(&kestrel, workspace.id, "the Session ended", |entries| {
        entries.iter().any(
            |entry| matches!(entry, Entry::SessionEnded { session: ended, .. } if *ended == session.id),
        )
    })
    .await;
    assert!(
        !entries
            .iter()
            .any(|entry| matches!(entry, Entry::TurnInterrupted { .. })),
        "nobody interrupted this turn: {entries:?}"
    );
    assert!(
        entries.iter().any(|entry| matches!(
            entry,
            Entry::ToolCall {
                closing_reason: Some(ClosingReason::Interrupted),
                completion: kestrel::log::Completion {
                    turn_outcome: Some(kestrel::log::TurnOutcome::Cancelled),
                    ..
                },
                ..
            }
        )),
        "the open tool call closed with the cancelled turn: {entries:?}"
    );

    kestrel.teardown().await;
}
