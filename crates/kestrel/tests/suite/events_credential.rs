//! The credential a poll presents: the App's own JWT rather than an installation token, and an
//! installation token replaced before it nears expiry.

use std::time::Duration;

use jiff::SignedDuration;
use kestrel::domain::Direction;

use crate::support::Kestrel;
use crate::support::events::{BOTH, labelled_ready, watching};
use crate::support::github_stub::{GithubStub, INSTALLATION_TOKEN, ScriptedResponse};

/// The Delivery log is the App's own, so the poll presents the App's JWT, not an installation token.
#[tokio::test]
async fn the_poll_presents_the_apps_own_credential() {
    let stub = GithubStub::start();
    stub.deliver(labelled_ready(43));
    let kestrel = Kestrel::stepped().await;
    watching(&kestrel, &stub, BOTH).await;

    kestrel.work_pass().await;
    assert_eq!(kestrel.events("acme").await.len(), 1);

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

#[tokio::test]
async fn an_installation_token_nearing_expiry_is_replaced_before_it_is_used_again() {
    let stub = GithubStub::start();
    let kestrel = Kestrel::stepped().await;
    watching(&kestrel, &stub, &[Direction::Outbound]).await;
    let integration = kestrel.integrations("acme").await.remove(0);
    for token in ["first-installation-token", "second-installation-token"] {
        stub.script_answer(
            "POST",
            "/access_tokens",
            ScriptedResponse::ok(
                serde_json::json!({
                    "token": token,
                    "expires_at": (kestrel.now() + SignedDuration::from_hours(1)).to_string(),
                })
                .to_string(),
            ),
        );
    }
    let github = kestrel.github();
    github
        .issue(&integration, 43)
        .await
        .expect("the issue should read");
    github
        .issue(&integration, 43)
        .await
        .expect("the cached token should work");
    kestrel.away_for(Duration::from_secs(59 * 60 + 30));
    github
        .issue(&integration, 43)
        .await
        .expect("the issue should read after renewal");

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
            "Bearer first-installation-token",
            "Bearer second-installation-token"
        ],
        "a token nearing expiry was not replaced before its next use"
    );
    kestrel.teardown().await;
}
