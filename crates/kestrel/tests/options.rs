//! What an Agent, a Trigger, `workspace open` and `session enqueue` declare for a Session's
//! Harness options, and how the supervisor sets them at setup (ADR-0041).

mod support;

use std::time::Duration;

use kestrel::domain::{
    Declared, Exit, Session, SessionId, SessionOption, SessionOptionKind, SessionOptionValue,
    SessionState, Workspace, WorkspaceId,
};
use kestrel::log::Entry;
use kestrel::operator;
use kestrel::work;
use kestrel_scripted_agent::{
    DEFAULT_MODEL, OTHER_MODE, OTHER_MODEL, OTHER_THOUGHT_LEVEL, STARTING_MODE,
    STARTING_THOUGHT_LEVEL, SWITCHED_MODE,
};
use reqwest::StatusCode;
use serde_json::{Value, json};
use support::link_client::{Link, Next};
use support::scripted_agent::{self, Script};
use support::supervisor;
use support::{HARNESS, Kestrel, OnTheLink, repository};

const PATIENCE: Duration = Duration::from_secs(30);
const LONG_ENOUGH_TO_BE_SURE: Duration = Duration::from_millis(500);

fn declared(model: Option<&str>, mode: Option<&str>, thought_level: Option<&str>) -> Declared {
    Declared {
        model: model.map(str::to_owned),
        mode: mode.map(str::to_owned),
        thought_level: thought_level.map(str::to_owned),
    }
}

fn current(session: &Session, category: &str) -> Option<String> {
    session
        .options
        .iter()
        .find(|option| option.is_category(category))
        .and_then(|option| option.current_value())
}

/// The value the harness ended up on for a category. The bookkeeping report is debounced, so it
/// is waited for rather than read once.
async fn until_current(kestrel: &Kestrel, session: SessionId, category: &str) -> String {
    let deadline = tokio::time::Instant::now() + PATIENCE;

    loop {
        let shown = kestrel.session(session).await;
        if let Some(value) = current(&shown, category) {
            return value;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the session {session} never reported a {category} option"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// The bookkeeping report is debounced, so a value the harness just moved to is waited for.
async fn until_current_is(kestrel: &Kestrel, session: SessionId, category: &str, value: &str) {
    let deadline = tokio::time::Instant::now() + PATIENCE;

    loop {
        let shown = kestrel.session(session).await;
        if current(&shown, category).as_deref() == Some(value) {
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the session {session} never reported {category} as {value}: {:?}",
            current(&shown, category)
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn failed(session: &Session) -> String {
    let Some(Exit::Failed { because }) = &session.exit else {
        panic!("the session did not fail: {:?}", session.exit);
    };

    because.clone()
}

async fn dispatching(script: Script) -> Kestrel {
    Kestrel::dispatching_to(supervisor::binary(), &scripted_agent::playing(script)).await
}

async fn a_workspace(kestrel: &Kestrel, agent: Declared) -> Workspace {
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
        .declare_agent_declaring(&organization, "builder", HARNESS, agent)
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

/// A Session's own value over the Agent's, category by category: the model the Agent named and
/// the mode the Session named, both set on the harness before its first prompt.
#[tokio::test]
async fn a_session_naming_a_mode_sets_it_over_the_agents_before_its_first_prompt() {
    let kestrel = dispatching(Script::Speaks).await;
    let workspace = a_workspace(
        &kestrel,
        declared(Some(OTHER_MODEL), Some(STARTING_MODE), None),
    )
    .await;
    let session = kestrel
        .enqueue_session_declaring(workspace.id, declared(None, Some(SWITCHED_MODE), None))
        .await;

    kestrel.answered(session.id, 1).await;
    assert_eq!(
        until_current(&kestrel, session.id, "model").await,
        OTHER_MODEL
    );
    assert_eq!(
        until_current(&kestrel, session.id, "mode").await,
        SWITCHED_MODE,
        "the Session's declared mode did not override the Agent's"
    );

    kestrel.stop_session(session.id).await;
    kestrel.teardown().await;
}

#[tokio::test]
async fn a_category_left_undeclared_stays_at_the_harness_default() {
    let kestrel = dispatching(Script::Speaks).await;
    let workspace = a_workspace(&kestrel, Declared::default()).await;
    let session = kestrel.enqueue_session(workspace.id).await;

    kestrel.answered(session.id, 1).await;
    assert_eq!(
        until_current(&kestrel, session.id, "model").await,
        DEFAULT_MODEL,
        "an undeclared model did not stay at the harness's default"
    );
    assert_eq!(
        until_current(&kestrel, session.id, "mode").await,
        STARTING_MODE,
        "an undeclared mode did not stay at the harness's default"
    );
    assert_eq!(
        until_current(&kestrel, session.id, "thought_level").await,
        STARTING_THOUGHT_LEVEL,
        "an undeclared thought level did not stay at the harness's default"
    );

    kestrel.stop_session(session.id).await;
    kestrel.teardown().await;
}

#[tokio::test]
async fn a_thought_level_the_harness_does_not_offer_fails_the_session() {
    let kestrel = dispatching(Script::Speaks).await;
    let workspace = a_workspace(&kestrel, declared(None, None, Some("deeper"))).await;
    let session = kestrel.enqueue_session(workspace.id).await;

    let session = kestrel.after_one_turn(session.id).await;

    let because = failed(&session);
    assert!(because.contains("thought_level"), "{because}");
    assert!(because.contains("deeper"), "{because}");

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_mode_a_harness_offers_no_way_to_set_fails_the_session() {
    let kestrel = dispatching(Script::Decides).await;
    let workspace = a_workspace(&kestrel, declared(None, Some(SWITCHED_MODE), None)).await;
    let session = kestrel.enqueue_session(workspace.id).await;

    let session = kestrel.after_one_turn(session.id).await;

    let because = failed(&session);
    assert!(because.contains("mode"), "{because}");
    assert!(because.contains(SWITCHED_MODE), "{because}");

    kestrel.teardown().await;
}

/// A harness that predates config options is set through `session/set_mode` instead, and its
/// synthesized Mode option reads the mode it was set to.
#[tokio::test]
async fn a_legacy_modes_harness_is_set_through_session_set_mode() {
    let kestrel = dispatching(Script::LegacyModesKept).await;
    let workspace = a_workspace(&kestrel, Declared::default()).await;
    let session = kestrel
        .enqueue_session_declaring(workspace.id, declared(None, Some(OTHER_MODE), None))
        .await;

    kestrel.answered(session.id, 1).await;
    assert_eq!(
        until_current(&kestrel, session.id, "mode").await,
        OTHER_MODE,
        "the legacy mode the Session declared was not set"
    );

    kestrel.stop_session(session.id).await;
    kestrel.teardown().await;
}

/// The scripted agent's process dies on the second prompt and a new one loads the session from
/// disk: recovery applies the declared mode again. Its load answer offers the harness's own
/// default, so a mode that is not applied again reads as that default.
#[tokio::test]
async fn a_recovered_harness_has_the_declared_mode_applied_again() {
    let kestrel = dispatching(Script::Revives).await;
    let workspace = a_workspace(&kestrel, Declared::default()).await;
    let session = kestrel
        .enqueue_session_declaring(workspace.id, declared(None, Some(SWITCHED_MODE), None))
        .await;

    kestrel.answered(session.id, 1).await;
    assert_eq!(
        until_current(&kestrel, session.id, "mode").await,
        SWITCHED_MODE
    );

    kestrel
        .post_while_busy(workspace.id, "operator", "the second thing to do")
        .await
        .expect("a waiting session takes the message as its next prompt");
    let answered = kestrel.answered(session.id, 2).await;

    assert_eq!(answered.state, SessionState::Waiting, "{:?}", answered.exit);
    assert_eq!(
        until_current(&kestrel, session.id, "mode").await,
        SWITCHED_MODE,
        "the recovered harness was not set to the declared mode again"
    );

    kestrel.stop_session(session.id).await;
    kestrel.teardown().await;
}

/// The scripted agent's model option as the control plane stores it, for a test standing in for
/// the supervisor.
fn model_option(current: &str) -> SessionOption {
    SessionOption {
        id: SessionOption::MODEL.to_owned(),
        name: "Model".to_owned(),
        description: None,
        category: Some(SessionOption::MODEL.to_owned()),
        kind: SessionOptionKind::Select {
            current: current.to_owned(),
            values: [DEFAULT_MODEL, OTHER_MODEL]
                .into_iter()
                .map(|value| SessionOptionValue {
                    value: value.to_owned(),
                    name: value.to_owned(),
                    description: None,
                })
                .collect(),
            groups: Vec::new(),
        },
    }
}

/// What the operator boundary answered to an option change.
async fn set_option(kestrel: &Kestrel, session: SessionId, body: Value) -> (StatusCode, Value) {
    let path = operator::SESSION_OPTIONS
        .replace("{organization}", "acme")
        .replace("{session}", &session.to_string());
    let response = reqwest::Client::new()
        .post(format!("{}{path}", kestrel.operator()))
        .json(&body)
        .send()
        .await
        .expect("the operator boundary should answer");
    let status = response.status();
    let answered = response.json().await.expect("a body");

    (status, answered)
}

async fn changed_entries(kestrel: &Kestrel, workspace: WorkspaceId) -> Vec<Entry> {
    kestrel
        .transcript(workspace)
        .await
        .into_iter()
        .filter_map(|entry| match entry.entry {
            Entry::OptionChanged { .. } => Some(entry.entry),
            _ => None,
        })
        .collect()
}

async fn until_changed(kestrel: &Kestrel, workspace: WorkspaceId) -> Entry {
    let deadline = tokio::time::Instant::now() + PATIENCE;

    loop {
        if let Some(entry) = changed_entries(kestrel, workspace).await.into_iter().next() {
            return entry;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the workspace {workspace} never recorded an option change"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

async fn until_state(kestrel: &Kestrel, session: SessionId, state: SessionState) -> Session {
    let deadline = tokio::time::Instant::now() + PATIENCE;

    loop {
        let shown = kestrel.session(session).await;
        if shown.state == state {
            return shown;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the session {} is {}, not {state}",
            shown.id,
            shown.state
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// A Session live on the link with the scripted options reported, driven by the test itself: the
/// phase a change needs, with no supervisor racing to answer it.
async fn live_on_the_link(kestrel: &Kestrel) -> (Session, OnTheLink) {
    let workspace = a_workspace(kestrel, Declared::default()).await;
    let (session, on) = kestrel.dispatch_to_the_link(workspace.id).await;
    kestrel.start_on_the_link(&session).await;
    kestrel.report_answered(&session, 1).await;
    kestrel
        .report_session_info(&session, &[model_option(DEFAULT_MODEL)])
        .await;

    let live = kestrel.session(session.id).await;
    assert_eq!(live.state, SessionState::Waiting, "{:?}", live.exit);
    assert!(!live.options.is_empty(), "the session reported no options");

    (live, on)
}

/// A change to a waiting Session is durable and ordered ahead of the prompt after it, so the next
/// Turn runs on the new model and the Transcript names the person, the option and both values.
#[tokio::test]
async fn changing_the_model_on_a_waiting_session_runs_the_next_turn_on_it() {
    let kestrel = dispatching(Script::Speaks).await;
    let workspace = a_workspace(&kestrel, Declared::default()).await;
    let session = kestrel.enqueue_session(workspace.id).await;

    kestrel.answered(session.id, 1).await;
    assert_eq!(
        until_current(&kestrel, session.id, "model").await,
        DEFAULT_MODEL
    );

    let (status, changed) = set_option(
        &kestrel,
        session.id,
        json!({"participant": "operator", "option": "model", "value": OTHER_MODEL}),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{changed}");
    assert_eq!(
        changed["changing_options"],
        json!([{
            "option": "model",
            "category": "model",
            "value": OTHER_MODEL,
            "participant": "operator",
        }]),
        "{changed}"
    );

    kestrel
        .post_while_busy(workspace.id, "operator", "keep going")
        .await
        .expect("a waiting session takes the message as its next prompt");
    kestrel.answered(session.id, 2).await;

    until_current_is(&kestrel, session.id, "model", OTHER_MODEL).await;
    let settled = kestrel.session(session.id).await;
    assert!(
        settled.changing_options.is_empty(),
        "{:?}",
        settled.changing_options
    );

    let entry = until_changed(&kestrel, workspace.id).await;
    let Entry::OptionChanged {
        participant,
        option,
        category,
        from,
        to,
        refused,
        ..
    } = entry
    else {
        unreachable!("until_changed only returns option changes")
    };
    assert_eq!(participant, "operator");
    assert_eq!(option, "model");
    assert_eq!(category, "model");
    assert_eq!(from.as_deref(), Some(DEFAULT_MODEL));
    assert_eq!(to.as_deref(), Some(OTHER_MODEL));
    assert!(refused.is_none(), "{refused:?}");

    kestrel.stop_session(session.id).await;
    kestrel.teardown().await;
}

/// A harness that offers only legacy `modes` has no Mode-category option to set, so a change to
/// its synthesized mode goes through `session/set_mode`.
#[tokio::test]
async fn changing_the_synthesized_mode_of_a_legacy_harness_goes_through_session_set_mode() {
    let kestrel = dispatching(Script::LegacyModesKept).await;
    let workspace = a_workspace(&kestrel, Declared::default()).await;
    let session = kestrel.enqueue_session(workspace.id).await;

    kestrel.answered(session.id, 1).await;
    assert_eq!(
        until_current(&kestrel, session.id, "mode").await,
        STARTING_MODE
    );

    let (status, _) = set_option(
        &kestrel,
        session.id,
        json!({"participant": "operator", "category": "mode", "value": OTHER_MODE}),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    until_current_is(&kestrel, session.id, "mode", OTHER_MODE).await;

    let Entry::OptionChanged { to, category, .. } = until_changed(&kestrel, workspace.id).await
    else {
        unreachable!("until_changed only returns option changes")
    };
    assert_eq!(category, "mode");
    assert_eq!(to.as_deref(), Some(OTHER_MODE));

    kestrel.stop_session(session.id).await;
    kestrel.teardown().await;
}

/// A queued Session has nothing to ask: the write sets its declared value at once, and the
/// harness applies it when the Session is set up.
#[tokio::test]
async fn changing_thought_level_on_a_queued_session_starts_it_with_that_value() {
    let kestrel = Kestrel::boot().await;
    let workspace = a_workspace(&kestrel, Declared::default()).await;
    let session = kestrel.enqueue_session(workspace.id).await;
    assert_eq!(session.state, SessionState::Queued);

    let (status, changed) = set_option(
        &kestrel,
        session.id,
        json!({
            "participant": "operator",
            "category": "thought_level",
            "value": OTHER_THOUGHT_LEVEL,
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{changed}");
    assert_eq!(changed["thought_level"], OTHER_THOUGHT_LEVEL);
    assert_eq!(changed["changing_options"], json!([]));

    let entry = until_changed(&kestrel, workspace.id).await;
    let Entry::OptionChanged {
        participant,
        option,
        category,
        from,
        to,
        refused,
        ..
    } = entry
    else {
        unreachable!("until_changed only returns option changes")
    };
    assert_eq!(participant, "operator");
    assert_eq!(option, "thought_level");
    assert_eq!(category, "thought_level");
    assert!(from.is_none(), "{from:?}");
    assert_eq!(to.as_deref(), Some(OTHER_THOUGHT_LEVEL));
    assert!(refused.is_none(), "{refused:?}");

    let stopped = kestrel.teardown().await;
    let kestrel = stopped
        .restart_with(
            supervisor::binary(),
            &scripted_agent::playing(Script::Speaks),
            2,
        )
        .await;
    kestrel.answered(session.id, 1).await;
    until_current_is(&kestrel, session.id, "thought_level", OTHER_THOUGHT_LEVEL).await;

    kestrel.stop_session(session.id).await;
    kestrel.teardown().await;
}

/// A Turn in flight is the harness's; the write is refused, naming the phase, and nothing
/// reaches the harness.
#[tokio::test]
async fn a_change_during_a_working_turn_is_refused_naming_working() {
    let kestrel = dispatching(Script::ReportsThenWaits).await;
    let workspace = a_workspace(&kestrel, Declared::default()).await;
    let session = kestrel.enqueue_session(workspace.id).await;

    until_state(&kestrel, session.id, SessionState::Working).await;
    let (status, refused) = set_option(
        &kestrel,
        session.id,
        json!({"participant": "operator", "option": "model", "value": OTHER_MODEL}),
    )
    .await;

    assert_eq!(status, StatusCode::CONFLICT, "{refused}");
    assert!(
        refused["message"]
            .as_str()
            .is_some_and(|message| message.contains("working")),
        "{refused}"
    );
    assert!(
        !matches!(
            kestrel.instruction(&session).await,
            kestrel::link::Instruction::SetOption { .. }
        ),
        "a refused change reached the harness"
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn an_unoffered_value_an_unknown_option_and_an_agents_name_are_refused() {
    let kestrel = dispatching(Script::Speaks).await;
    let workspace = a_workspace(&kestrel, Declared::default()).await;
    let session = kestrel.enqueue_session(workspace.id).await;
    kestrel.answered(session.id, 1).await;
    until_current(&kestrel, session.id, "model").await;

    let (status, refused) = set_option(
        &kestrel,
        session.id,
        json!({"participant": "operator", "option": "model", "value": "no-such-model"}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{refused}");
    assert_eq!(refused["field"], "value");

    let (status, refused) = set_option(
        &kestrel,
        session.id,
        json!({"participant": "operator", "option": "no-such-option", "value": OTHER_MODEL}),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{refused}");

    let (status, refused) = set_option(
        &kestrel,
        session.id,
        json!({"participant": "operator", "option": "model", "category": "model", "value": OTHER_MODEL}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{refused}");

    let (status, refused) = set_option(
        &kestrel,
        session.id,
        json!({"participant": "builder", "option": "model", "value": OTHER_MODEL}),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{refused}");
    assert_eq!(refused["field"], "participant");

    kestrel.stop_session(session.id).await;
    kestrel.teardown().await;
}

/// The harness has the last word: a change it refuses leaves the options where they were and the
/// Transcript records why.
#[tokio::test]
async fn a_harness_that_refuses_a_change_leaves_the_options_and_records_why() {
    let kestrel = dispatching(Script::RefusesOptions).await;
    let workspace = a_workspace(&kestrel, Declared::default()).await;
    let session = kestrel.enqueue_session(workspace.id).await;
    kestrel.answered(session.id, 1).await;
    until_current(&kestrel, session.id, "model").await;

    let (status, _) = set_option(
        &kestrel,
        session.id,
        json!({"participant": "operator", "option": "model", "value": OTHER_MODEL}),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);

    let entry = until_changed(&kestrel, workspace.id).await;
    let Entry::OptionChanged {
        from, to, refused, ..
    } = entry
    else {
        unreachable!("until_changed only returns option changes")
    };
    assert!(
        refused
            .as_deref()
            .is_some_and(|why| why.contains("will not change its options")),
        "{refused:?}"
    );
    assert!(to.is_none(), "{to:?}");
    assert_eq!(from.as_deref(), Some(DEFAULT_MODEL));

    let settled = kestrel.session(session.id).await;
    assert!(
        settled.changing_options.is_empty(),
        "{:?}",
        settled.changing_options
    );
    assert_eq!(
        current(&settled, "model").as_deref(),
        Some(DEFAULT_MODEL),
        "a refused change moved the option"
    );

    kestrel.stop_session(session.id).await;
    kestrel.teardown().await;
}

/// The instruction is durable: a change written while the supervisor's stream is down is handed
/// to it once, when it comes back with its cursor.
#[tokio::test]
async fn a_change_written_while_the_supervisor_is_disconnected_is_applied_once_after_it_reconnects()
{
    let kestrel = Kestrel::boot().await;
    let (session, on) = live_on_the_link(&kestrel).await;
    let link = Link::to(&kestrel.link());

    let mut first = link.open(&on.instance, &on.credential, None).await;
    let Next::Event(delivered) = first.next_within(PATIENCE).await else {
        panic!("the stream never delivered the instruction that was waiting on it");
    };
    assert_eq!(delivered.name.as_deref(), Some("start"));
    let cursor: i64 = delivered
        .id
        .expect("an instruction id")
        .parse()
        .expect("a cursor");
    drop(first);

    let (status, _) = set_option(
        &kestrel,
        session.id,
        json!({"participant": "operator", "option": "model", "value": OTHER_MODEL}),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);

    let mut again = link.open(&on.instance, &on.credential, Some(cursor)).await;
    let Next::Event(changed) = again.next_within(PATIENCE).await else {
        panic!("the reconnected stream was not handed the change");
    };
    assert_eq!(changed.name.as_deref(), Some("set_option"));
    let body: Value = serde_json::from_str(&changed.data).expect("the instruction body");
    assert_eq!(body["option"], "model");
    assert_eq!(body["value"], OTHER_MODEL);
    assert_eq!(body["participant"], "operator");

    let once: i64 = changed
        .id
        .expect("an instruction id")
        .parse()
        .expect("a cursor");
    let mut settled = link.open(&on.instance, &on.credential, Some(once)).await;
    assert!(
        matches!(
            settled.next_within(LONG_ENOUGH_TO_BE_SURE).await,
            Next::Quiet
        ),
        "reconnecting with the change's cursor was handed it again"
    );

    kestrel.teardown().await;
}

/// `changing_options` carries a change while the harness has not answered, and the answer stores
/// the new list and clears it.
#[tokio::test]
async fn changing_options_shows_while_a_change_is_pending_and_clears_when_it_settles() {
    let kestrel = Kestrel::boot().await;
    let (session, _) = live_on_the_link(&kestrel).await;

    let (status, changed) = set_option(
        &kestrel,
        session.id,
        json!({"participant": "operator", "option": "model", "value": OTHER_MODEL}),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{changed}");
    assert_eq!(
        changed["changing_options"],
        json!([{
            "option": "model",
            "category": "model",
            "value": OTHER_MODEL,
            "participant": "operator",
        }]),
        "{changed}"
    );

    kestrel
        .report_option_changed(
            &session,
            2,
            work::Report::OptionChanged {
                participant: "operator".to_owned(),
                option: "model".to_owned(),
                category: "model".to_owned(),
                from: Some(DEFAULT_MODEL.to_owned()),
                to: Some(OTHER_MODEL.to_owned()),
                refused: None,
                options: vec![model_option(OTHER_MODEL)],
            },
        )
        .await;

    let settled = kestrel.session(session.id).await;
    assert!(
        settled.changing_options.is_empty(),
        "{:?}",
        settled.changing_options
    );
    assert_eq!(current(&settled, "model").as_deref(), Some(OTHER_MODEL));

    let Entry::OptionChanged { to, .. } = until_changed(&kestrel, session.workspace).await else {
        unreachable!("until_changed only returns option changes")
    };
    assert_eq!(to.as_deref(), Some(OTHER_MODEL));

    kestrel.teardown().await;
}
