//! What an Agent, a Trigger, `workspace open` and `session enqueue` declare for a Session's
//! Harness options, and how the supervisor sets them at setup (ADR-0041).

mod support;

use std::time::Duration;

use kestrel::domain::{Declared, Exit, Session, SessionId, SessionState, Workspace};
use kestrel_scripted_agent::{
    DEFAULT_MODEL, OTHER_MODE, OTHER_MODEL, STARTING_MODE, SWITCHED_MODE,
};
use support::scripted_agent::{self, Script};
use support::supervisor;
use support::{HARNESS, Kestrel, repository};

const PATIENCE: Duration = Duration::from_secs(30);

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
    let shown = kestrel.session(session.id).await;
    assert!(
        current(&shown, "thought_level").is_none(),
        "a category the harness offers nothing for appeared in its options"
    );

    kestrel.stop_session(session.id).await;
    kestrel.teardown().await;
}

#[tokio::test]
async fn a_thought_level_the_harness_does_not_offer_fails_the_session() {
    let kestrel = dispatching(Script::Speaks).await;
    let workspace = a_workspace(&kestrel, declared(None, None, Some("high"))).await;
    let session = kestrel.enqueue_session(workspace.id).await;

    let session = kestrel.after_one_turn(session.id).await;

    let because = failed(&session);
    assert!(because.contains("thought_level"), "{because}");
    assert!(because.contains("high"), "{because}");

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
