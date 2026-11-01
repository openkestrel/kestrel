//! The credential a poll presents: the App's own JWT rather than an installation token, and an
//! installation token replaced before it nears expiry.

use jiff::SignedDuration;
use kestrel::domain::{Connection, GithubConnection, Integration, IntegrationId, OrganizationId};
use kestrel::integration::credential::App;
use kestrel::integration::github::Github;

use crate::support::events::{BOTH, REPOSITORY, eagerly, labelled_ready, recorded, watching};
use crate::support::github_stub::{self, GithubStub, INSTALLATION_TOKEN};
use crate::support::{APP_ID, INSTALLATION_ID, Kestrel, PRIVATE_KEY};

/// The Delivery log is the App's own, so the poll presents the App's JWT, not an installation token.
#[tokio::test]
async fn the_poll_presents_the_apps_own_credential() {
    let stub = GithubStub::start();
    stub.deliver(labelled_ready(43));
    let kestrel = Kestrel::boot().await;
    watching(&kestrel, &stub, BOTH).await;

    recorded(&kestrel, 1).await;

    for request in stub
        .requests()
        .iter()
        .filter(|request| request.url.starts_with("/app/hook/deliveries"))
    {
        let authorization = request
            .headers
            .iter()
            .find(|(name, _)| name == "authorization")
            .map(|(_, value)| value.as_str())
            .expect("the poll should authenticate");
        let jwt = authorization
            .strip_prefix("Bearer ")
            .expect("a bearer credential");
        assert_eq!(jwt.split('.').count(), 3, "{jwt} is not a JWT");
        assert!(!authorization.contains(INSTALLATION_TOKEN));
    }

    kestrel.teardown().await;
}

/// An installation token lasts an hour; kestrel never keeps reading on one minted that long
/// ago. Rather than waiting out a real hour, the stub mints one good for less than
/// `EXPIRY_MARGIN`, so it already reads as stale by the time anything asks for it again.
#[tokio::test]
async fn an_installation_token_nearing_expiry_is_replaced_before_it_is_used_again() {
    let stub = GithubStub::start();
    stub.script_answer(
        "POST",
        "/access_tokens",
        github_stub::minted_token("first-installation-token", SignedDuration::from_secs(30)),
    );
    stub.script_answer(
        "POST",
        "/access_tokens",
        github_stub::minted_token("second-installation-token", SignedDuration::from_hours(1)),
    );
    let integration = Integration {
        id: IntegrationId::generate(),
        organization: OrganizationId::generate(),
        name: "github".to_owned(),
        connection: Connection::Github(GithubConnection {
            repository: REPOSITORY.to_owned(),
            api: stub.base_url(),
            credential: App::held(APP_ID, INSTALLATION_ID, PRIVATE_KEY),
            bot_login: "kestrel[bot]".to_owned(),
            interval: eagerly(),
            repository_id: github_stub::REPOSITORY_ID,
        }),
        carries: BOTH.to_vec(),
        state: kestrel::domain::IntegrationState::Enabled,
        revision: 1,
        disabled_at: None,
        retired_at: None,
        canceled_posts: Vec::new(),
        poll_due_at: None,
        deliveries_read_from: None,
        last_polled_at: None,
        last_event_refusal: None,
        rotation: None,
    };
    let github = Github::unfenced(&kestrel::timer::Clock::System).expect("the GitHub client");

    for _ in 0..2 {
        github
            .issue(&integration, 43)
            .await
            .expect("the issue should read");
    }

    let authorizations: Vec<String> = stub
        .requests()
        .iter()
        .filter(|request| request.url.ends_with("/issues/43"))
        .filter_map(|request| {
            request
                .headers
                .iter()
                .find(|(name, _)| name == "authorization")
        })
        .map(|(_, value)| value.clone())
        .collect();

    assert_eq!(
        authorizations,
        vec![
            "Bearer first-installation-token",
            "Bearer second-installation-token"
        ],
        "a token nearing expiry was not replaced before its next use"
    );
}
