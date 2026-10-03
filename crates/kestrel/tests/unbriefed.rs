mod support;

use std::time::Duration;

use jiff::{SignedDuration, Timestamp};
use kestrel::domain::{
    Exit, Preparing, Session, SessionId, SessionState, Workspace, WorkspaceState,
};
use kestrel::log::{BriefSource, Entry};
use kestrel::queue::Reason;
use kestrel::work::Occupied;
use support::repository;
use support::scripted_agent::{self, Script};
use support::supervisor;
use support::{A_PROVIDER_KEY, HARNESS, Kestrel, PROVIDER_KEY};

const PATIENCE: Duration = Duration::from_secs(30);

async fn until(
    kestrel: &Kestrel,
    session: SessionId,
    what: &str,
    ready: impl Fn(&Session) -> bool,
) -> Session {
    let deadline = tokio::time::Instant::now() + PATIENCE;

    loop {
        let session = kestrel.session(session).await;
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

async fn sealed_by_the_sweep(kestrel: &Kestrel, workspace: Workspace) -> Workspace {
    let deadline = tokio::time::Instant::now() + PATIENCE;

    loop {
        let shown = kestrel.show_workspace(workspace.id).await;
        if shown.state == WorkspaceState::Sealed {
            return shown;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the workspace {} was last active at {} and never sealed itself",
            shown.id,
            shown.last_active_at
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
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

async fn reasons(kestrel: &Kestrel, session: &Session) -> Vec<Reason> {
    kestrel
        .queue("acme")
        .await
        .queued
        .into_iter()
        .find(|queued| queued.session.id == session.id)
        .expect("the session is queued")
        .reasons
}

#[tokio::test]
async fn with_every_slot_occupied_an_unbriefed_workspace_still_reaches_harness_ready() {
    let kestrel = Kestrel::dispatching_harnesses_up_to(
        supervisor::binary(),
        &[(HARNESS, &scripted_agent::playing(Script::ReportsThenWaits))],
        1,
    )
    .await;
    let busy = a_workspace(&kestrel).await;
    let unbriefed = a_workspace(&kestrel).await;

    let working = kestrel.enqueue_session(busy.id).await;
    let working = until(
        &kestrel,
        working.id,
        "its slot's session working",
        |session| session.state == SessionState::Working,
    )
    .await;
    assert_eq!(kestrel.queue("acme").await.active_work.occupied, 1);

    let queued = kestrel
        .enqueue_session_with_nothing_posted(unbriefed.id)
        .await;
    let ready = until(
        &kestrel,
        queued.id,
        "harness ready without a slot",
        |session| session.preparing == Some(Preparing::HarnessReady),
    )
    .await;

    assert_eq!(ready.state, SessionState::Unbriefed);
    assert!(ready.instance.is_some(), "it has an Instance of its own");
    assert!(
        kestrel.turns(ready.id).await.is_empty(),
        "no Turn may be prompted before the Brief"
    );
    assert_eq!(
        kestrel.session(working.id).await.state,
        SessionState::Working,
        "the slot is still the working session's"
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn an_unbriefed_workspace_is_refused_at_the_live_instance_limit() {
    let kestrel = Kestrel::boot().await;
    let organization = kestrel.declare_limited_organization("acme", 1).await;
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
    let active = kestrel.open_workspace("acme", "kestrel", "builder").await;
    let waiting = kestrel.open_workspace("acme", "kestrel", "builder").await;

    let first = kestrel.enqueue_session(active.id).await;
    let claimed = kestrel
        .occupy_session()
        .await
        .expect("the first session should claim");
    assert_eq!(claimed.id, first.id);
    kestrel.executes_on(&claimed, "active").await;

    let second = kestrel
        .enqueue_session_with_nothing_posted(waiting.id)
        .await;
    assert!(
        kestrel.occupy_session().await.is_none(),
        "an unbriefed Session still cannot be admitted past the live Instance limit"
    );
    let second = kestrel.session(second.id).await;
    assert_eq!(second.state, SessionState::Queued);
    assert_eq!(
        reasons(&kestrel, &second).await,
        [Reason::LiveInstanceLimit(1)]
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn an_unbriefed_workspace_dispatches_past_a_serialized_profile() {
    let kestrel = Kestrel::boot().await;
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
        .declare_agent(&organization, "builder", support::SERIALIZED, None)
        .await;
    kestrel
        .declare_profile("acme", "jack", "Jack")
        .await
        .expect("the profile should declare");
    let holding = kestrel
        .open_workspace_with("acme", "kestrel", "builder", "jack")
        .await;
    let unbriefed = kestrel
        .open_workspace_with("acme", "kestrel", "builder", "jack")
        .await;

    let first = kestrel.enqueue_session(holding.id).await;
    let claimed = kestrel
        .occupy_session()
        .await
        .expect("the first session should claim");
    assert_eq!(claimed.id, first.id);
    kestrel.executes_on(&claimed, "profile-holder").await;

    let second = kestrel
        .enqueue_session_with_nothing_posted(unbriefed.id)
        .await;
    let claimed = kestrel
        .occupy_session()
        .await
        .expect("an unbriefed session takes no turn, so the profile does not hold it");
    assert_eq!(claimed.id, second.id);
    assert_eq!(claimed.state, SessionState::Unbriefed);

    kestrel.teardown().await;
}

#[tokio::test]
async fn an_unbriefed_session_is_starting_its_harness_once_its_checkout_is_reported() {
    let kestrel =
        Kestrel::dispatching_harnesses_up_to(supervisor::binary(), &[(HARNESS, "sleep 600")], 1)
            .await;
    let workspace = a_workspace(&kestrel).await;

    let queued = kestrel
        .enqueue_session_with_nothing_posted(workspace.id)
        .await;
    let starting = until(&kestrel, queued.id, "starting its harness", |session| {
        session.preparing == Some(Preparing::StartingHarness)
    })
    .await;

    assert_eq!(starting.state, SessionState::Unbriefed);

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_spawn_failure_ends_an_unbriefed_session_failed() {
    let kestrel = Kestrel::dispatching_harnesses_up_to(
        supervisor::binary(),
        &[(HARNESS, "kestrel-no-such-harness-binary")],
        1,
    )
    .await;
    let workspace = a_workspace(&kestrel).await;

    let queued = kestrel
        .enqueue_session_with_nothing_posted(workspace.id)
        .await;
    let ended = until(&kestrel, queued.id, "it ended", |session| {
        session.state == SessionState::Ended
    })
    .await;

    assert!(
        matches!(ended.exit, Some(Exit::Failed { .. })),
        "a harness that cannot spawn fails the session: {ended:?}"
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn an_unbriefed_start_opens_the_conversation_reports_ready_and_prompts_nothing() {
    let kestrel = Kestrel::dispatching(supervisor::binary()).await;
    let workspace = a_workspace(&kestrel).await;

    let queued = kestrel
        .enqueue_session_with_nothing_posted(workspace.id)
        .await;
    let ready = until(&kestrel, queued.id, "harness ready", |session| {
        session.preparing == Some(Preparing::HarnessReady)
    })
    .await;

    assert!(kestrel.turns(ready.id).await.is_empty());
    let transcript = kestrel.transcript(workspace.id).await;
    assert!(
        transcript
            .iter()
            .all(|recorded| !matches!(recorded.entry, Entry::SessionStarted { .. })),
        "an unbriefed Session never starts a turn: {transcript:?}"
    );
    assert!(
        transcript
            .iter()
            .all(|recorded| !matches!(&recorded.entry, Entry::Said { participant, .. } if participant == "builder")),
        "the scripted agent says nothing until it is prompted: {transcript:?}"
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_post_to_a_ready_unbriefed_session_becomes_its_brief_and_the_agent_echoes_it() {
    let kestrel = Kestrel::dispatching_harnesses_up_to(
        supervisor::binary(),
        &[
            (HARNESS, &scripted_agent::playing(Script::Echoes)),
            (
                support::SERIALIZED,
                &scripted_agent::playing(Script::ReportsThenWaits),
            ),
        ],
        1,
    )
    .await;
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
        .declare_agent(&organization, "builder", HARNESS, None)
        .await;
    kestrel
        .declare_agent(&organization, "holder", support::SERIALIZED, None)
        .await;
    kestrel
        .hold_provider_credential(&organization, PROVIDER_KEY, A_PROVIDER_KEY)
        .await;

    let holding = kestrel.open_workspace("acme", "kestrel", "holder").await;
    let held = kestrel.enqueue_session(holding.id).await;
    let held = until(&kestrel, held.id, "the holder working", |session| {
        session.state == SessionState::Working
    })
    .await;
    assert_eq!(kestrel.queue("acme").await.active_work.occupied, 1);

    let unbriefed = kestrel.open_workspace("acme", "kestrel", "builder").await;
    let queued = kestrel
        .enqueue_session_with_nothing_posted(unbriefed.id)
        .await;
    until(&kestrel, queued.id, "harness ready", |session| {
        session.preparing == Some(Preparing::HarnessReady)
    })
    .await;

    let brief = "Reply with the single word ready.";
    let posted = kestrel.post(unbriefed.id, "alice", brief).await;
    assert_eq!(posted.id, queued.id);
    let transcript = kestrel.transcript(unbriefed.id).await;
    let written = transcript
        .iter()
        .position(|recorded| {
            matches!(&recorded.entry, Entry::Brief { source: BriefSource::Operator { participant: Some(by) }, brief: held } if by == "alice" && held == brief)
        })
        .expect("the post should record the Brief at once");
    assert!(written > 0);
    assert_eq!(
        transcript[written - 1].entry,
        Entry::ParticipantJoined {
            participant: "alice".to_owned(),
        }
    );

    assert!(kestrel.turns(queued.id).await.is_empty());
    assert_eq!(
        kestrel.session(queued.id).await.state,
        SessionState::Unbriefed
    );

    kestrel.complete_session(&held).await;
    until(&kestrel, queued.id, "its turn answered", |session| {
        session.state == SessionState::Waiting
    })
    .await;
    let echoed = kestrel
        .transcript(unbriefed.id)
        .await
        .into_iter()
        .find_map(|recorded| match recorded.entry {
            Entry::Said {
                participant,
                message,
                ..
            } if participant == "builder" => Some(message),
            _ => None,
        })
        .expect("the agent should echo what it was prompted with");
    assert_eq!(echoed, brief, "the Brief is the whole prompt, verbatim");

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_message_posted_before_the_harness_is_ready_becomes_the_brief_at_ready() {
    let kestrel = Kestrel::boot().await;
    let workspace = a_workspace(&kestrel).await;
    let queued = kestrel
        .enqueue_session_with_nothing_posted(workspace.id)
        .await;
    let session = kestrel
        .claim_session()
        .await
        .expect("the unbriefed session should claim");
    assert_eq!(session.id, queued.id);
    assert_eq!(session.preparing, Some(Preparing::Provisioning));

    let posted = kestrel.post(workspace.id, "alice", "what I want").await;
    assert_eq!(posted.id, session.id);
    assert!(kestrel.has_pending_messages(workspace.id).await);
    assert!(
        kestrel
            .transcript(workspace.id)
            .await
            .iter()
            .all(|recorded| !matches!(recorded.entry, Entry::Brief { .. })),
        "nothing is a Brief until the harness is ready"
    );

    kestrel.executes_on(&session, "local-exec/fixture").await;
    kestrel.report_ready(&session).await;

    let transcript = kestrel.transcript(workspace.id).await;
    let written = transcript
        .iter()
        .position(|recorded| {
            matches!(&recorded.entry, Entry::Brief { source: BriefSource::Operator { participant: Some(by) }, brief } if by == "alice" && brief == "what I want")
        })
        .expect("the held message should become the Brief at ready");
    assert!(written > 0);
    assert_eq!(
        transcript[written - 1].entry,
        Entry::ParticipantJoined {
            participant: "alice".to_owned(),
        }
    );
    assert!(!kestrel.has_pending_messages(workspace.id).await);

    let prompted = match kestrel.occupy_up_to(2).await {
        Some(Occupied::Resumed(prompted)) => prompted,
        _ => panic!("the Brief should take the free slot"),
    };
    assert_eq!(prompted.id, session.id);
    assert_eq!(
        kestrel.session(session.id).await.state,
        SessionState::Working
    );
    assert_eq!(kestrel.turns(session.id).await.len(), 1);

    kestrel.teardown().await;
}

#[tokio::test]
async fn the_first_turn_waits_behind_older_held_input() {
    let kestrel = Kestrel::boot().await;
    let older = a_workspace(&kestrel).await;
    let newer = a_workspace(&kestrel).await;

    let first = kestrel.enqueue_session(older.id).await;
    let claimed = kestrel
        .occupy_session()
        .await
        .expect("the older session should claim");
    assert_eq!(claimed.id, first.id);
    let waiting = kestrel.waits_after_its_first_turn(&claimed).await;
    assert_eq!(waiting.state, SessionState::Waiting);
    let held = kestrel.post(older.id, "operator", "older input").await;
    assert_eq!(held.id, first.id);

    let queued = kestrel.enqueue_session_with_nothing_posted(newer.id).await;
    let session = kestrel
        .claim_session()
        .await
        .expect("the unbriefed session should claim");
    assert_eq!(session.id, queued.id);
    kestrel.executes_on(&session, "newer-instance").await;
    kestrel.report_ready(&session).await;
    let posted = kestrel.post(newer.id, "alice", "the brief").await;
    assert_eq!(posted.id, queued.id);

    let prompted = match kestrel.occupy_up_to(2).await {
        Some(Occupied::Resumed(prompted)) => prompted,
        _ => panic!("the older input should take the free slot"),
    };
    assert_eq!(prompted.id, first.id);
    assert!(
        kestrel.turns(queued.id).await.is_empty(),
        "the Brief waits behind input that asked first"
    );

    kestrel.complete_session(&waiting).await;
    let prompted = match kestrel.occupy_up_to(2).await {
        Some(Occupied::Resumed(prompted)) => prompted,
        _ => panic!("the Brief should take the slot next"),
    };
    assert_eq!(prompted.id, queued.id);
    assert_eq!(kestrel.turns(queued.id).await.len(), 1);

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_serialized_profile_holds_the_first_turn_not_the_dispatch() {
    let kestrel = Kestrel::boot().await;
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
        .declare_agent(&organization, "builder", support::SERIALIZED, None)
        .await;
    kestrel
        .declare_profile("acme", "jack", "Jack")
        .await
        .expect("the profile should declare");
    let holding = kestrel
        .open_workspace_with("acme", "kestrel", "builder", "jack")
        .await;
    let unbriefed = kestrel
        .open_workspace_with("acme", "kestrel", "builder", "jack")
        .await;

    let first = kestrel.enqueue_session(holding.id).await;
    let occupied = kestrel
        .occupy_session()
        .await
        .expect("the holder should claim");
    assert_eq!(occupied.id, first.id);
    kestrel.executes_on(&occupied, "profile-holder").await;

    let queued = kestrel
        .enqueue_session_with_nothing_posted(unbriefed.id)
        .await;
    let session = kestrel
        .occupy_session()
        .await
        .expect("an unbriefed session dispatches past the profile");
    assert_eq!(session.id, queued.id);
    assert_eq!(session.state, SessionState::Unbriefed);
    kestrel.executes_on(&session, "unbriefed-instance").await;
    kestrel.report_ready(&session).await;
    kestrel.post(unbriefed.id, "alice", "the brief").await;

    assert!(
        kestrel.occupy_up_to(2).await.is_none(),
        "the profile holds the first Turn, not the dispatch"
    );
    assert!(kestrel.turns(queued.id).await.is_empty());

    kestrel.complete_session(&occupied).await;
    let prompted = match kestrel.occupy_up_to(2).await {
        Some(Occupied::Resumed(prompted)) => prompted,
        _ => panic!("the first Turn should take the slot once the profile is free"),
    };
    assert_eq!(prompted.id, queued.id);
    assert_eq!(kestrel.turns(queued.id).await.len(), 1);

    kestrel.teardown().await;
}

#[tokio::test]
async fn posts_after_the_brief_are_held_and_drain_into_the_next_turn() {
    let kestrel = Kestrel::boot().await;
    let workspace = a_workspace(&kestrel).await;
    let queued = kestrel
        .enqueue_session_with_nothing_posted(workspace.id)
        .await;
    let session = kestrel
        .claim_session()
        .await
        .expect("the unbriefed session should claim");
    assert_eq!(session.id, queued.id);
    kestrel.executes_on(&session, "local-exec/fixture").await;
    kestrel.report_ready(&session).await;
    kestrel.post(workspace.id, "alice", "the brief").await;

    let held = kestrel
        .post_while_busy(workspace.id, "operator", "and one more thing")
        .await;
    assert_eq!(held.map(|held| held.id), Some(session.id));
    assert!(kestrel.has_pending_messages(workspace.id).await);

    let prompted = match kestrel.occupy_up_to(2).await {
        Some(Occupied::Resumed(prompted)) => prompted,
        _ => panic!("the Brief should take the free slot"),
    };
    assert_eq!(prompted.id, session.id);
    assert_eq!(kestrel.turns(session.id).await.len(), 1);

    kestrel.report_answered(&session, 1).await;
    assert_eq!(
        kestrel.session(session.id).await.state,
        SessionState::Working,
        "the held message should follow at the answer, on the slot the Session holds"
    );
    assert_eq!(kestrel.turns(session.id).await.len(), 2);
    assert!(!kestrel.has_pending_messages(workspace.id).await);

    kestrel.teardown().await;
}

#[tokio::test]
async fn an_idle_unbriefed_workspace_seals_after_the_window() {
    let kestrel = Kestrel::dispatching(supervisor::binary()).await;
    let workspace = a_workspace(&kestrel).await;

    let queued = kestrel
        .enqueue_session_with_nothing_posted(workspace.id)
        .await;
    let ready = until(&kestrel, queued.id, "harness ready", |session| {
        session.preparing == Some(Preparing::HarnessReady)
    })
    .await;
    assert!(ready.instance.is_some());

    kestrel
        .last_active(
            &workspace,
            Timestamp::now() - SignedDuration::from_hours(25),
        )
        .await;
    let sealed = sealed_by_the_sweep(&kestrel, workspace).await;

    assert_eq!(sealed.state, WorkspaceState::Sealed);
    assert_eq!(
        kestrel.session(ready.id).await.state,
        SessionState::Ended,
        "sealing let the unbriefed Session go with its Instance"
    );

    kestrel.teardown().await;
}
