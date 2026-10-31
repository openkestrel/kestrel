//! Subscription Profiles: a person's login, sealed beside the Provider Credentials, reaching only
//! the Sessions of Workspaces that name it, and outliving every Instance it is written into.

use crate::support;

use kestrel::domain::Exit;
use kestrel_scripted_agent::{LOGIN, REFRESHED, Script};
use reqwest::StatusCode;
use support::link_client::Link;
use support::profile::{
    ALEXS_KEY, FIRST_LOGIN, JACKS_KEY, SUBSCRIPTION_KEY, a_profile, declared, finished, login_file,
    playing, subscription_key, worked,
};
use support::{A_PROVIDER_KEY, Kestrel, PROVIDER_KEY, SERIALIZED, repository};

#[tokio::test]
async fn a_session_reaches_a_model_with_its_workspaces_profile_and_no_provider_account() {
    let kestrel = playing(Script::Confides).await;
    declared(&kestrel, "opencode").await;
    a_profile(&kestrel, "jack", "Jack", &subscription_key(), JACKS_KEY).await;
    let workspace = kestrel
        .open_workspace_with("acme", repository::NAME, "builder", "jack")
        .await;

    let (session, said) = worked(&kestrel, &workspace).await;

    assert_eq!(session.exit, Some(Exit::Succeeded), "{said}");
    assert!(
        said.contains(&format!("{SUBSCRIPTION_KEY}={JACKS_KEY}")),
        "the profile never reached the agent: {said}"
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn one_persons_profile_reaches_no_workspace_that_does_not_name_it() {
    let kestrel = playing(Script::Confides).await;
    declared(&kestrel, "opencode").await;
    a_profile(&kestrel, "jack", "Jack", &subscription_key(), JACKS_KEY).await;
    a_profile(&kestrel, "alex", "Alex", &subscription_key(), ALEXS_KEY).await;
    let organization = kestrel.organizations().await.remove(0);
    kestrel
        .hold_provider_credential(&organization, PROVIDER_KEY, A_PROVIDER_KEY)
        .await;

    let alexs = kestrel
        .open_workspace_with("acme", repository::NAME, "builder", "alex")
        .await;
    let nobodys = kestrel
        .open_workspace("acme", repository::NAME, "builder")
        .await;
    let (_, alex_said) = worked(&kestrel, &alexs).await;
    let (_, nobody_said) = worked(&kestrel, &nobodys).await;

    assert!(alex_said.contains(ALEXS_KEY), "{alex_said}");
    assert!(
        !alex_said.contains(JACKS_KEY),
        "another person's profile reached this session: {alex_said}"
    );
    assert!(
        !nobody_said.contains(JACKS_KEY) && !nobody_said.contains(ALEXS_KEY),
        "a workspace naming no profile was spawned with one: {nobody_said}"
    );

    kestrel.teardown().await;
}

/// The scripted agent rewrites its login the way a harness refreshes one, and a second Workspace
/// is a fresh Instance: what it finds is what the first Session handed back.
#[tokio::test]
async fn a_login_refreshed_on_one_instance_is_the_one_the_next_instance_starts_from() {
    let kestrel = playing(Script::Refreshes).await;
    declared(&kestrel, "opencode").await;
    a_profile(&kestrel, "jack", "Jack", &login_file(), FIRST_LOGIN).await;

    let first = kestrel
        .open_workspace_with("acme", repository::NAME, "builder", "jack")
        .await;
    let (session, said) = worked(&kestrel, &first).await;
    assert_eq!(session.exit, Some(Exit::Succeeded), "{said}");
    assert!(
        said.contains(&format!("logged in as {FIRST_LOGIN}")),
        "{said}"
    );

    let second = kestrel
        .open_workspace_with("acme", repository::NAME, "builder", "jack")
        .await;
    let (next, said) = worked(&kestrel, &second).await;

    assert_ne!(
        next.instance, session.instance,
        "the second workspace reused an instance"
    );
    assert!(
        said.contains(&format!("logged in as {FIRST_LOGIN}{REFRESHED}")),
        "the refreshed login did not reach the next instance: {said}"
    );

    kestrel.teardown().await;
}

/// An Instance outlives its Session and holds on to what was written into it, so the login is
/// taken back out as the Session ends.
#[tokio::test]
async fn an_instance_holds_no_login_once_its_session_has_ended() {
    let kestrel = playing(Script::Refreshes).await;
    declared(&kestrel, "opencode").await;
    a_profile(&kestrel, "jack", "Jack", &login_file(), FIRST_LOGIN).await;
    let workspace = kestrel
        .open_workspace_with("acme", repository::NAME, "builder", "jack")
        .await;

    let (session, said) = worked(&kestrel, &workspace).await;

    assert_eq!(session.exit, Some(Exit::Succeeded), "{said}");
    let instance = session
        .instance
        .as_deref()
        .and_then(|instance| instance.strip_prefix("local-exec/"))
        .expect("a local instance");
    let home = std::env::temp_dir().join(format!("{instance}.home"));
    assert!(home.is_dir(), "the instance has no home of its own");
    assert!(
        !home.join(LOGIN).exists(),
        "the login was left on the instance after its session"
    );

    kestrel.teardown().await;
}

/// The login reaches whichever Agent's Session uses it, and leaves the Instance with that Session.
#[tokio::test]
async fn an_instance_holds_no_login_once_another_agents_session_on_it_has_ended() {
    let kestrel = playing(Script::Refreshes).await;
    declared(&kestrel, "opencode").await;
    let organization = kestrel.organizations().await.remove(0);
    kestrel
        .declare_agent(&organization, "reviewer", "opencode", None)
        .await;
    a_profile(&kestrel, "jack", "Jack", &login_file(), FIRST_LOGIN).await;
    let workspace = kestrel
        .open_workspace_with("acme", repository::NAME, "builder", "jack")
        .await;
    let (built, _) = worked(&kestrel, &workspace).await;

    let review = kestrel.enqueue_session_as(workspace.id, "reviewer").await;
    let (reviewed, said) = finished(&kestrel, &workspace, review).await;

    assert_eq!(reviewed.exit, Some(Exit::Succeeded), "{said}");
    assert_eq!(reviewed.agent.name, "reviewer");
    assert_eq!(reviewed.instance, built.instance);
    assert!(
        said.contains(&format!("logged in as {FIRST_LOGIN}{REFRESHED}")),
        "the login never reached the second agent's session: {said}"
    );
    let instance = reviewed
        .instance
        .as_deref()
        .and_then(|instance| instance.strip_prefix("local-exec/"))
        .expect("a local instance");
    let home = std::env::temp_dir().join(format!("{instance}.home"));
    assert!(
        !home.join(LOGIN).exists(),
        "the login was left on the instance after the second agent's session"
    );

    kestrel.teardown().await;
}

/// Nothing of what the profile holds is said anywhere a Workspace is read from.
#[tokio::test]
async fn a_session_spawned_with_a_profile_records_it_nowhere() {
    let kestrel = playing(Script::Speaks).await;
    declared(&kestrel, "opencode").await;
    a_profile(&kestrel, "jack", "Jack", &subscription_key(), JACKS_KEY).await;
    kestrel
        .hold_in_profile("acme", "jack", &login_file(), FIRST_LOGIN)
        .await;
    let workspace = kestrel
        .open_workspace_with("acme", repository::NAME, "builder", "jack")
        .await;

    let (session, said) = worked(&kestrel, &workspace).await;

    assert_eq!(session.exit, Some(Exit::Succeeded), "{said}");
    for secret in [JACKS_KEY, FIRST_LOGIN] {
        assert!(!said.contains(secret), "{said}");
        assert!(!format!("{session:?}").contains(secret));
        assert!(!format!("{workspace:?}").contains(secret));
    }

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_profile_is_sealed_beside_the_database_and_listed_by_name_never_by_value() {
    let kestrel = Kestrel::boot().await;
    declared(&kestrel, "opencode").await;
    a_profile(&kestrel, "jack", "Jack", &subscription_key(), JACKS_KEY).await;
    kestrel
        .hold_in_profile("acme", "jack", &login_file(), FIRST_LOGIN)
        .await;

    let listed = kestrel.profiles("acme").await;

    let [(profile, held)] = &listed[..] else {
        panic!("the organization lists {listed:?}");
    };
    assert_eq!(profile.owner, "Jack");
    assert_eq!(
        held.iter()
            .map(|held| held.entry.to_string())
            .collect::<Vec<_>>(),
        [
            format!("file {LOGIN}"),
            format!("variable {SUBSCRIPTION_KEY}")
        ]
    );
    assert!(!format!("{listed:?}").contains(JACKS_KEY));
    for kept in ["kestrel.db", "kestrel.db-wal"] {
        let Ok(written) = std::fs::read(kestrel.data_dir().join(kept)) else {
            continue;
        };
        for secret in [JACKS_KEY, FIRST_LOGIN] {
            assert!(
                !written
                    .windows(secret.len())
                    .any(|window| window == secret.as_bytes()),
                "a copy of {kept} is a usable login"
            );
        }
    }

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_profile_never_changes_hands() {
    let kestrel = Kestrel::boot().await;
    declared(&kestrel, "opencode").await;
    kestrel
        .declare_profile("acme", "jack", "Jack")
        .await
        .expect("the profile should declare");

    let taken = kestrel.declare_profile("acme", "jack", "Alex").await;

    assert!(
        taken
            .expect_err("the profile changed hands")
            .to_string()
            .contains("belongs to Jack")
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_session_whose_profile_holds_no_login_fails_before_an_instance() {
    let kestrel = playing(Script::Confides).await;
    declared(&kestrel, "opencode").await;
    kestrel
        .declare_profile("acme", "jack", "Jack")
        .await
        .expect("the profile should declare");
    let workspace = kestrel
        .open_workspace_with("acme", repository::NAME, "builder", "jack")
        .await;

    let (session, _) = worked(&kestrel, &workspace).await;

    let Some(Exit::Failed { because }) = &session.exit else {
        panic!(
            "the session ended {:?} on a profile holding nothing",
            session.exit
        );
    };
    assert!(because.contains("holds no login"), "{because}");
    assert!(session.instance.is_none());

    kestrel.teardown().await;
}

/// Two copies of one rotating login race to refresh it, so the second Session waits for the first.
#[tokio::test]
async fn sessions_on_a_serialized_harness_sharing_a_profile_are_dispatched_one_at_a_time() {
    let kestrel = Kestrel::boot().await;
    declared(&kestrel, SERIALIZED).await;
    let organization = kestrel.organizations().await.remove(0);
    kestrel
        .declare_agent(&organization, "reviewer", "claude", None)
        .await;
    a_profile(&kestrel, "jack", "Jack", &login_file(), FIRST_LOGIN).await;
    a_profile(&kestrel, "alex", "Alex", &login_file(), FIRST_LOGIN).await;
    let open = |agent: &'static str, profile: &'static str| {
        kestrel.open_workspace_with("acme", repository::NAME, agent, profile)
    };
    let (jacks, jacks_again, alexs, jacks_other_harness) = (
        open("builder", "jack").await,
        open("builder", "jack").await,
        open("builder", "alex").await,
        open("reviewer", "jack").await,
    );
    for workspace in [&jacks, &jacks_again, &alexs, &jacks_other_harness] {
        kestrel.enqueue_session(workspace.id).await;
    }

    let mut claimed = Vec::new();
    while let Some(next) = kestrel.claim_session().await {
        claimed.push(next);
    }

    let workspaces: Vec<_> = claimed.iter().map(|session| session.workspace).collect();
    assert_eq!(workspaces, [jacks.id, alexs.id, jacks_other_harness.id]);

    kestrel.complete_session(&claimed[0]).await;
    assert_eq!(
        kestrel.claim_session().await.map(|next| next.workspace),
        Some(jacks_again.id)
    );

    kestrel.teardown().await;
}

/// A trailing agent still uses its subscription, so it holds the Profile as a working one does.
#[tokio::test]
async fn a_trailing_session_holds_its_serialized_profile_until_it_settles() {
    let kestrel = Kestrel::boot().await;
    declared(&kestrel, SERIALIZED).await;
    a_profile(&kestrel, "jack", "Jack", &login_file(), FIRST_LOGIN).await;
    let first = kestrel
        .open_workspace_with("acme", repository::NAME, "builder", "jack")
        .await;
    let second = kestrel
        .open_workspace_with("acme", repository::NAME, "builder", "jack")
        .await;
    kestrel.enqueue_session(first.id).await;
    let trailing = kestrel.claim_session().await.expect("the first claims");
    kestrel.on_the_link(&trailing).await;
    kestrel.start_on_the_link(&trailing).await;
    kestrel.report_answered(&trailing, 1).await;
    kestrel.enqueue_session(second.id).await;

    assert_eq!(
        kestrel.claim_session().await.map(|next| next.workspace),
        None
    );

    kestrel.report_settled(&trailing, 2).await;
    assert_eq!(
        kestrel.claim_session().await.map(|next| next.workspace),
        Some(second.id)
    );

    kestrel.teardown().await;
}

/// A Session can hand back a refreshed login and never add one the person did not put there.
#[tokio::test]
async fn a_session_refreshes_only_the_files_its_profile_already_holds() {
    let kestrel = Kestrel::boot().await;
    declared(&kestrel, "opencode").await;
    a_profile(&kestrel, "jack", "Jack", &login_file(), FIRST_LOGIN).await;
    let workspace = kestrel
        .open_workspace_with("acme", repository::NAME, "builder", "jack")
        .await;
    let (session, on) = kestrel.dispatch_to_the_link(workspace.id).await;
    let link = Link::to(&kestrel.link());
    let handed = link
        .credentials(&on.instance, session.id, Some(&on.credential))
        .await;
    assert_eq!(handed.status(), StatusCode::OK);

    let answered = link
        .refresh(
            &on.instance,
            session.id,
            &on.credential,
            &[
                (LOGIN, "a-refreshed-login"),
                (".ssh/id_ed25519", "smuggled"),
            ],
        )
        .await;

    assert_eq!(answered.status(), StatusCode::NO_CONTENT);
    let profile = workspace.profile.expect("the workspace names a profile");
    let contents = kestrel.profile_contents(&profile).await;
    assert_eq!(
        contents.files.into_iter().collect::<Vec<_>>(),
        [(LOGIN.to_owned(), "a-refreshed-login".to_owned())]
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_session_that_has_ended_refreshes_nothing() {
    let kestrel = Kestrel::boot().await;
    declared(&kestrel, "opencode").await;
    a_profile(&kestrel, "jack", "Jack", &login_file(), FIRST_LOGIN).await;
    let workspace = kestrel
        .open_workspace_with("acme", repository::NAME, "builder", "jack")
        .await;
    let (session, on) = kestrel.dispatch_to_the_link(workspace.id).await;
    kestrel.complete_session(&session).await;

    let answered = Link::to(&kestrel.link())
        .refresh(
            &on.instance,
            session.id,
            &on.credential,
            &[(LOGIN, "too-late")],
        )
        .await;

    assert_eq!(answered.status(), StatusCode::GONE);
    let profile = workspace.profile.expect("the workspace names a profile");
    assert_eq!(
        kestrel.profile_contents(&profile).await.files[LOGIN],
        FIRST_LOGIN
    );

    kestrel.teardown().await;
}
