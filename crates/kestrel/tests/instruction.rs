//! The control plane, not the supervisor, decides a Session's first Turn: the Workspace's
//! unfollowed Brief when there is one, otherwise the message or messages that started this
//! Session verbatim, with nothing improvised when it can find neither (#314).

mod support;

use kestrel::log::Entry;
use support::scripted_agent::Script;
use support::supervisor::Supervisor;
use support::{A_PROVIDER_KEY, Kestrel, PROVIDER_KEY};

async fn a_workspace(kestrel: &Kestrel) -> kestrel::domain::Workspace {
    let organization = kestrel.declare_organization("acme").await;
    kestrel
        .declare_project(&organization, "kestrel", &[], "main")
        .await;
    kestrel
        .declare_agent(&organization, "builder", "opencode", None)
        .await;
    kestrel
        .hold_provider_credential(&organization, PROVIDER_KEY, A_PROVIDER_KEY)
        .await;
    kestrel.open_workspace("acme", "kestrel", "builder").await
}

/// What the agent was prompted with, which the echoing agent says back.
async fn prompted(kestrel: &Kestrel) -> String {
    let claimed = kestrel
        .claim_session()
        .await
        .expect("a session should be queued to claim");
    let on = kestrel.on_the_link(&claimed).await;
    let mut supervisor = Supervisor::provision_playing(&kestrel.link(), &on, Script::Echoes);
    supervisor.wait_until_it_says("reported connected").await;
    kestrel.start(&claimed, supervisor.harness()).await;
    supervisor.wait_until_it_says("reported answered").await;
    kestrel.stop_session(claimed.id).await;
    supervisor.lets_go_of(claimed.id).await;

    kestrel
        .transcript(claimed.workspace)
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
        .expect("the agent should say what it was prompted with")
}

#[tokio::test]
async fn a_workspace_opened_without_a_brief_starts_on_the_message_posted_to_it_alone() {
    let kestrel = Kestrel::boot().await;
    let workspace = a_workspace(&kestrel).await;
    kestrel
        .post(
            workspace.id,
            "operator",
            "Reply with the single word ready.",
        )
        .await;

    let prompt = prompted(&kestrel).await;

    assert_eq!(prompt, "Reply with the single word ready.");
    assert!(
        !prompt.contains("Do the work this environment was provisioned for"),
        "the control plane's instruction should replace the supervisor's improvised one: {prompt}"
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_session_with_neither_a_brief_nor_a_starting_message_fails_rather_than_starting() {
    let kestrel = Kestrel::boot().await;
    let workspace = a_workspace(&kestrel).await;
    let session = kestrel
        .enqueue_session_with_nothing_posted(workspace.id)
        .await;

    let refusal = kestrel
        .try_start(&session, support::harness())
        .await
        .expect_err("a session with no instruction cannot start");

    assert!(
        refusal.to_string().contains("no instruction"),
        "unhelpful refusal: {refusal}"
    );
    assert!(
        kestrel
            .transcript(workspace.id)
            .await
            .iter()
            .all(|recorded| !matches!(recorded.entry, Entry::SessionStarted { .. })),
        "a session with no instruction should never be recorded as started"
    );

    kestrel.teardown().await;
}
