//! A Session reaches GitHub as the Integration its Workspace fixed when it opened, asking the link
//! for a token at each use (ADR-0028, ADR-0056).

use crate::support;

use std::time::Duration;

use jiff::SignedDuration;
use kestrel::domain::{Direction, Session};
use kestrel::integration::Change;
use kestrel::log::Entry;
use kestrel::operator;
use kestrel_scripted_agent::Script;
use reqwest::StatusCode;
use serde_json::{Value, json};
use support::github_stub::{self, GithubStub, ScriptedResponse};
use support::link_client::Link;
use support::{Kestrel, OnTheLink, scripted_agent};

const REPOSITORY: &str = "jtmthf/kestrel";
const HTTPS: &str = "https://github.com/jtmthf/kestrel.git";
const TOOLS: &str = "jtmthf/tools";
const TOOLS_HTTPS: &str = "https://github.com/jtmthf/tools.git";
const BOTH: &[Direction] = &[Direction::Inbound, Direction::Outbound];
const MINTED: &str = "/access_tokens";

async fn posted(kestrel: &Kestrel, path: &str, body: &Value) -> (StatusCode, Value) {
    let response = reqwest::Client::new()
        .post(format!(
            "{}{}",
            kestrel.operator(),
            path.replace("{organization}", "acme")
        ))
        .json(body)
        .send()
        .await
        .expect("the operator boundary should answer");
    let status = response.status();

    (status, response.json().await.unwrap_or(Value::Null))
}

async fn connect(kestrel: &Kestrel, stub: &GithubStub, name: &str, repository: &str) {
    kestrel
        .register_integration(
            "acme",
            name,
            repository,
            &stub.base_url(),
            BOTH,
            SignedDuration::from_hours(1),
        )
        .await;
}

fn has_main(stub: &GithubStub) {
    stub.script_answer(
        "GET",
        "/branches/main",
        ScriptedResponse::ok(json!({ "name": "main" }).to_string()),
    );
}

async fn declare(kestrel: &Kestrel, stub: &GithubStub, selected: &[(&str, &str)]) {
    for _ in selected {
        has_main(stub);
    }
    let (status, project) = posted(
        kestrel,
        operator::PROJECTS,
        &json!({
            "name": "web",
            "repositories": selected.iter().map(|(repository, _)| repository).collect::<Vec<_>>(),
            "integrations": selected
                .iter()
                .map(|(repository, integration)| {
                    json!({ "repository": repository, "integration": integration })
                })
                .collect::<Vec<_>>(),
            "branch": "main",
        }),
    )
    .await;
    assert!(status.is_success(), "{project}");
}

/// The test plays the supervisor, so the Session is carried without a checkout or a harness.
async fn carried(
    kestrel: &Kestrel,
    stub: &GithubStub,
    selected: &[(&str, &str)],
) -> (Session, OnTheLink) {
    kestrel.declare_organization("acme").await;
    for (repository, integration) in selected {
        connect(kestrel, stub, integration, repository).await;
    }
    let (status, agent) = posted(
        kestrel,
        operator::AGENTS,
        &json!({ "name": "builder", "harness": "opencode" }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{agent}");
    declare(kestrel, stub, selected).await;
    let workspace = kestrel.open_workspace("acme", "web", "builder").await;

    kestrel.dispatch_to_the_link(workspace.id).await
}

async fn asked(
    kestrel: &Kestrel,
    session: &Session,
    on: &OnTheLink,
    repository: Option<&str>,
) -> (StatusCode, Value) {
    let response = Link::to(&kestrel.link())
        .github_token(&on.instance, session.id, repository, &on.credential)
        .await;
    let status = response.status();

    (status, response.json().await.unwrap_or(Value::Null))
}

fn mints(stub: &GithubStub) -> Vec<String> {
    stub.requests()
        .into_iter()
        .filter(|request| request.method == "POST" && request.url.contains(MINTED))
        .map(|request| request.body)
        .collect()
}

#[tokio::test]
async fn a_session_is_lent_a_token_for_its_repository_alone_and_never_the_apps_key() {
    let log = support::operator_log::capturing();
    let stub = GithubStub::start();
    let kestrel = Kestrel::boot().await;
    let (session, on) = carried(&kestrel, &stub, &[(REPOSITORY, "github")]).await;

    let (status, lent) = asked(&kestrel, &session, &on, Some(HTTPS)).await;

    assert_eq!(status, StatusCode::OK, "{lent}");
    assert_eq!(lent["token"], github_stub::INSTALLATION_TOKEN);
    let mut fields: Vec<_> = lent.as_object().expect("an object").keys().collect();
    fields.sort();
    assert_eq!(fields, ["expires_at", "token"]);
    let scoped: Value = serde_json::from_str(mints(&stub).last().expect("a mint")).expect("json");
    assert_eq!(
        scoped,
        json!({ "repository_ids": [github_stub::REPOSITORY_ID] })
    );
    let (status, unnamed) = asked(&kestrel, &session, &on, None).await;
    assert_eq!(status, StatusCode::OK, "{unnamed}");
    assert_eq!(
        log.containing(github_stub::INSTALLATION_TOKEN),
        Vec::<String>::new()
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_repository_the_workspace_selected_nothing_for_is_lent_nothing_and_nothing_else_is_asked()
{
    let stub = GithubStub::start();
    let kestrel = Kestrel::boot().await;
    let (session, on) = carried(&kestrel, &stub, &[(REPOSITORY, "github")]).await;
    connect(&kestrel, &stub, "tools", TOOLS).await;
    let before = mints(&stub).len();

    for unbound in [
        TOOLS_HTTPS,
        "http://github.com/jtmthf/kestrel.git",
        "https://example.com/jtmthf/kestrel.git",
    ] {
        let (status, lent) = asked(&kestrel, &session, &on, Some(unbound)).await;
        assert_eq!(status, StatusCode::NO_CONTENT, "{unbound}: {lent}");
    }
    assert_eq!(mints(&stub).len(), before);

    kestrel.teardown().await;
}

#[tokio::test]
async fn each_selected_repository_is_lent_its_own_integration_and_an_unnamed_one_is_refused() {
    let stub = GithubStub::start();
    let kestrel = Kestrel::boot().await;
    let (session, on) = carried(&kestrel, &stub, &[(REPOSITORY, "github"), (TOOLS, "tools")]).await;

    let (status, unnamed) = asked(&kestrel, &session, &on, None).await;
    assert_eq!(status, StatusCode::CONFLICT, "{unnamed}");
    assert!(
        unnamed["message"]
            .as_str()
            .is_some_and(|message| message.contains("name the repository")),
        "{unnamed}"
    );
    kestrel.disable_integration("acme", "tools").await;
    let (status, lent) = asked(&kestrel, &session, &on, Some(HTTPS)).await;
    assert_eq!(status, StatusCode::OK, "{lent}");
    let (status, refused) = asked(&kestrel, &session, &on, Some(TOOLS_HTTPS)).await;
    assert_eq!(status, StatusCode::CONFLICT, "{refused}");
    assert!(
        refused["message"]
            .as_str()
            .is_some_and(|message| message.contains("tools is disabled")),
        "{refused}"
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn an_integration_out_of_use_lends_nothing_in_place_of_another_and_its_session_goes_on() {
    let stub = GithubStub::start();
    let kestrel = Kestrel::boot().await;
    let (session, on) = carried(&kestrel, &stub, &[(REPOSITORY, "github")]).await;
    connect(&kestrel, &stub, "replacement", REPOSITORY).await;
    declare(&kestrel, &stub, &[(REPOSITORY, "replacement")]).await;
    let refused = async |kestrel: &Kestrel, saying: &str| {
        let (status, refused) = asked(kestrel, &session, &on, Some(HTTPS)).await;
        assert_eq!(status, StatusCode::CONFLICT, "{refused}");
        assert!(
            refused["message"]
                .as_str()
                .is_some_and(|message| message.contains(saying)),
            "{refused}"
        );
    };

    kestrel.disable_integration("acme", "github").await;
    refused(&kestrel, "disabled").await;
    kestrel.enable_integration("acme", "github").await;
    let (status, lent) = asked(&kestrel, &session, &on, Some(HTTPS)).await;
    assert_eq!(status, StatusCode::OK, "{lent}");
    kestrel
        .change_integration(
            "acme",
            "github",
            Change {
                carries: Some(&[Direction::Inbound]),
                ..Change::default()
            },
        )
        .await
        .expect("the integration should change");
    refused(&kestrel, "outbound").await;
    kestrel.retire_integration("acme", "github").await;
    refused(&kestrel, "retired").await;

    assert_ne!(
        kestrel.session(session.id).await.state,
        kestrel::domain::SessionState::Ended
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_session_that_has_ended_is_lent_nothing() {
    let stub = GithubStub::start();
    let kestrel = Kestrel::boot().await;
    let (session, on) = carried(&kestrel, &stub, &[(REPOSITORY, "github")]).await;
    kestrel.complete_session(&session).await;

    let (status, gone) = asked(&kestrel, &session, &on, Some(HTTPS)).await;

    assert_eq!(status, StatusCode::GONE, "{gone}");

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_token_about_to_lapse_is_minted_again_for_the_next_use() {
    let stub = GithubStub::start();
    let kestrel = Kestrel::boot().await;
    let (session, on) = carried(&kestrel, &stub, &[(REPOSITORY, "github")]).await;
    for token in ["ghs_first", "ghs_second", "ghs_third"] {
        stub.script_answer(
            "POST",
            MINTED,
            github_stub::minted_token(token, SignedDuration::from_secs(30)),
        );
    }

    let mut lent = Vec::new();
    for _ in 0..3 {
        let (status, token) = asked(&kestrel, &session, &on, Some(HTTPS)).await;
        assert_eq!(status, StatusCode::OK, "{token}");
        lent.push(token["token"].clone());
    }

    assert_eq!(lent, ["ghs_first", "ghs_second", "ghs_third"]);

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_replaced_key_is_never_lent_a_token_minted_under_the_last_revision() {
    let stub = GithubStub::start();
    let kestrel = Kestrel::boot().await;
    let (session, on) = carried(&kestrel, &stub, &[(REPOSITORY, "github")]).await;
    stub.script_answer(
        "POST",
        MINTED,
        github_stub::minted_token("ghs_before", SignedDuration::from_hours(1)),
    );
    let (_, before) = asked(&kestrel, &session, &on, Some(HTTPS)).await;
    assert_eq!(before["token"], "ghs_before");

    kestrel
        .replace_private_key("acme", "github", support::REPLACEMENT_PRIVATE_KEY, None)
        .await
        .expect("a key github accepts should replace the last");
    let (status, after) = asked(&kestrel, &session, &on, Some(HTTPS)).await;
    assert_eq!(status, StatusCode::OK, "{after}");
    assert_ne!(after["token"], "ghs_before");

    stub.script_answer(
        "POST",
        MINTED,
        github_stub::minted_token("ghs_stale", SignedDuration::from_hours(1))
            .after(Duration::from_secs(1)),
    );
    let racing = {
        let link = Link::to(&kestrel.link());
        let (instance, credential, session) =
            (on.instance.clone(), on.credential.clone(), session.id);
        tokio::spawn(async move {
            let response = link
                .github_token(&instance, session, Some(HTTPS), &credential)
                .await;
            (
                response.status(),
                response.json::<Value>().await.unwrap_or(Value::Null),
            )
        })
    };
    tokio::time::sleep(Duration::from_millis(300)).await;
    kestrel
        .replace_private_key("acme", "github", support::PRIVATE_KEY, None)
        .await
        .expect("the first key should replace its replacement");
    let (status, raced) = racing.await.expect("the request should finish");

    assert_eq!(status, StatusCode::OK, "{raced}");
    assert_ne!(raced["token"], "ghs_stale", "the stale revision was lent");

    kestrel.teardown().await;
}

/// The agent's own `git` and `gh`, through the real supervisor: each use is lent a token, and
/// nothing the agent can read afterwards holds one.
#[tokio::test]
async fn an_agents_git_and_gh_are_lent_a_token_at_each_use_that_nothing_it_keeps_holds() {
    let stub = GithubStub::start();
    let held = tempfile::TempDir::new().expect("a directory");
    let real = held.path().join("real-gh");
    let bin = held.path().join("bin");
    std::fs::create_dir(&bin).expect("a directory");
    let executable = |path: &std::path::Path, contents: String| {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::write(path, contents).expect("a script");
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
            .expect("an executable");
    };
    executable(
        &real,
        "#!/bin/sh\nif [ -n \"$GH_TOKEN\" ]; then echo \"gh ran with a token of ${#GH_TOKEN} \
         characters\"; else echo 'gh ran with no token'; fi\n"
            .to_owned(),
    );
    executable(
        &bin.join("gh"),
        format!(
            "#!/bin/sh\nexec '{}' gh '{}' \"$@\"\n",
            support::supervisor::binary().display(),
            real.display()
        ),
    );
    let environment = support::environment::Environment::executing(&format!(
        "export GIT_CONFIG_COUNT=1\n\
         export GIT_CONFIG_KEY_0='url.{}.insteadOf'\n\
         export GIT_CONFIG_VALUE_0='{HTTPS}'\n\
         export PATH='{}':\"$PATH\"\n\
         exec '{}'",
        support::repository::url(),
        bin.display(),
        support::supervisor::binary().display(),
    ));
    let kestrel =
        Kestrel::dispatching_to(environment.path(), &scripted_agent::playing(Script::Runs)).await;
    let organization = kestrel.declare_organization("acme").await;
    connect(&kestrel, &stub, "github", REPOSITORY).await;
    kestrel
        .hold_provider_credential(
            &organization,
            support::PROVIDER_KEY,
            support::A_PROVIDER_KEY,
        )
        .await;
    let (status, agent) = posted(
        &kestrel,
        operator::AGENTS,
        &json!({ "name": "builder", "harness": "opencode" }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{agent}");
    declare(&kestrel, &stub, &[(REPOSITORY, "github")]).await;
    let brief = "printf 'protocol=https\\nhost=github.com\\npath=jtmthf/kestrel.git\\n\\n' \
                 | git credential fill | sed -n 's/^password=.*/a password was handed over/p'; \
                 gh pr list; \
                 if env | grep -q kestrel_should_never_say; then echo a token is in the environment; \
                 else echo no token is in the environment; fi";

    let (status, opened) = posted(
        &kestrel,
        operator::WORKSPACES,
        &json!({ "project": "web", "agent": "builder", "brief": brief }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{opened}");
    let session: kestrel::domain::SessionId = opened["session"]["id"]
        .as_str()
        .expect("a session")
        .parse()
        .expect("a session id");
    let answered = kestrel.answered(session, 1).await;
    assert_ne!(
        answered.state,
        kestrel::domain::SessionState::Ended,
        "{answered:?}"
    );
    let transcript = kestrel.every_entry(answered.workspace).await;
    let said: Vec<_> = transcript
        .iter()
        .filter_map(|entry| match entry {
            Entry::Said { message, .. } => Some(message.as_str()),
            _ => None,
        })
        .collect();

    assert_eq!(
        said,
        [format!(
            "a password was handed over\ngh ran with a token of {} characters\n\
             no token is in the environment\n",
            github_stub::INSTALLATION_TOKEN.len()
        )]
    );
    assert!(
        !format!("{transcript:?}").contains(github_stub::INSTALLATION_TOKEN),
        "the token reached the transcript"
    );
    let scoped = json!({ "repository_ids": [github_stub::REPOSITORY_ID] }).to_string();
    assert!(mints(&stub).contains(&scoped));

    kestrel.teardown().await;
}
