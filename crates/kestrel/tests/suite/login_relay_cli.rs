//! `kestrel login` through a vendor relay: Claude's code at a terminal, Codex's device code
//! without one, against scripted vendor commands rather than live accounts.

use std::os::unix::fs::PermissionsExt as _;

use serde_json::Value;
use tempfile::TempDir;

use crate::support;

use support::Kestrel;
use support::client::{self, Client, Invocation};

const CODE: &str = "private-authorization-code";
const TOKEN: &str = "sk-ant-oat01-scripted-subscription-secret";
const URL: &str = "https://claude.com/cai/oauth/authorize?client_id=client&redirect_uri=https%3A%2F%2Fplatform.claude.com%2Foauth%2Fcode%2Fcallback&response_type=code&scope=user%3Ainference&code_challenge=abcdefghijklmnopqrstuvwxyz0123456789ABCDEFGHI&state=public-state";
const DEVICE_URL: &str = "https://auth.openai.com/codex/device";
const DEVICE_CODE: &str = "0K88-JZ4OG";
const ACCESS: &str = "scripted-chatgpt-access-secret";
const LOGIN: &str = r#"{"auth_mode":"chatgpt","OPENAI_API_KEY":null,"tokens":{"id_token":"scripted-id","access_token":"scripted-chatgpt-access-secret","refresh_token":"scripted-refresh","account_id":"account"},"last_refresh":"2026-10-16T00:00:00Z"}"#;

fn vendor(name: &str, script: impl FnOnce(&str) -> String) -> TempDir {
    let scripts = TempDir::new().expect("a directory for the vendor");
    let path = scripts.path().join(name);
    std::fs::write(&path, script(&scripts.path().display().to_string()))
        .expect("the vendor writes");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
        .expect("the vendor runs");
    scripts
}

/// A pinned-format `claude setup-token` that prints its link, takes the code and prints a token.
fn claude() -> TempDir {
    vendor("claude", |_| {
        format!(
            r#"#!/bin/sh
[ "$1" = setup-token ] || exit 2
stty -echo
printf '\033[2J\033[HSign in\r\n%s\r\n\r\nPaste code here if prompted > ' '{URL}'
IFS= read -r code
[ "$code" = '{CODE}' ] || exit 1
printf '\033[2J\033[HYour token:\r\n{TOKEN}\r\n\r\nPress Enter to continue\r\n'
IFS= read -r continuation
exit 0
"#
        )
    })
}

/// A pinned-format `codex login --device-auth` that waits for `approved` beside it.
fn codex() -> TempDir {
    vendor("codex", |directory| {
        format!(
            r#"#!/bin/sh
[ "$*" = 'login -c cli_auth_credentials_store=file --device-auth' ] || exit 2
printf 'Follow these steps to sign in with ChatGPT using device code authorization:\r\n\r\n'
printf '1. Open this link in your browser and sign in to your account\r\n   \033[94m{DEVICE_URL}\033[0m\r\n\r\n'
printf '2. Enter this one-time code \033[90m(expires in 15 minutes)\033[0m\r\n   \033[94m{DEVICE_CODE}\033[0m\r\n\r\n'
while [ ! -f '{directory}/approved' ]; do sleep 0.02; done
mkdir -p "$HOME/.codex" && printf '%s' '{LOGIN}' > "$HOME/.codex/auth.json"
exit 0
"#
        )
    })
}

async fn booted(scripts: &TempDir) -> Kestrel {
    let kestrel = Kestrel::dispatching_with_programs(scripts.path()).await;
    kestrel.name_operator("ada").await;
    kestrel
}

#[tokio::test]
async fn at_a_terminal_claude_shows_its_link_alone_on_a_line_and_takes_the_code_unechoed() {
    let scripts = claude();
    let kestrel = booted(&scripts).await;

    let shown = client::ran_on_a_terminal_by(
        &kestrel,
        &["login", "claude"],
        Invocation::default()
            .after_prompt("Choose 1-2: ")
            .answering("Paste the code", &format!("{CODE}\n")),
        "1\n",
    )
    .await;

    assert!(shown.status.success(), "{}", shown.said);
    assert!(shown.lines().contains(&URL), "{}", shown.said);
    for secret in [CODE, TOKEN] {
        assert!(!shown.said.contains(secret), "{}", shown.said);
    }
    assert!(!shown.said.contains('\u{1b}'), "{}", shown.said);
    assert!(
        shown.said.contains("serves only its owner"),
        "{}",
        shown.said
    );
    assert!(
        shown.said.contains("login_completed (relay)"),
        "{}",
        shown.said
    );
    assert!(shown.said.contains("none recorded"), "{}", shown.said);

    kestrel.teardown().await;
}

#[tokio::test]
async fn without_a_terminal_the_claude_relay_is_refused_before_it_starts() {
    let scripts = claude();
    let kestrel = booted(&scripts).await;

    let refused = client::ran_by(
        &kestrel,
        &["login", "claude", "--method", "claude-setup-token"],
        Invocation::default(),
    )
    .await;

    assert_eq!(refused.status.code(), Some(2), "{}", refused.err);
    assert!(refused.err.contains("--token"), "{}", refused.err);
    assert!(kestrel.database().relays().await.is_empty());

    kestrel.teardown().await;
}

#[tokio::test]
async fn without_a_terminal_codex_prints_its_link_and_code_and_waits_for_approval() {
    let scripts = codex();
    let kestrel = booted(&scripts).await;
    let operator = kestrel.operator();
    let client = Client::spawn(
        &operator,
        &["login", "codex", "--method", "codex-device-auth", "--json"],
    );

    let client = tokio::task::spawn_blocking(move || {
        client.until_err_says(DEVICE_CODE);
        client
    })
    .await
    .expect("the client shows the code");
    let id = kestrel.database().relays().await.remove(0);
    let waiting: Value = reqwest::get(format!(
        "{operator}/operator/organizations/ada/sign-ins/codex/codex-device-auth/relay/{id}"
    ))
    .await
    .expect("the relay reads")
    .json()
    .await
    .expect("a relay");
    assert_eq!(waiting["progress"]["state"], "awaiting_browser");
    std::fs::write(scripts.path().join("approved"), "").expect("approval");
    let mut client = client;
    let finished = tokio::task::spawn_blocking(move || client.finish())
        .await
        .expect("the client finishes");

    assert!(
        finished.status.success(),
        "{:?} {:?}\n{}",
        finished.status,
        finished.out,
        finished.err
    );
    let lines: Vec<&str> = finished.err.lines().collect();
    assert!(lines.contains(&DEVICE_URL), "{}", finished.err);
    assert!(lines.contains(&DEVICE_CODE), "{}", finished.err);
    assert!(!finished.err.contains(ACCESS), "{}", finished.err);
    let saved = finished.json();
    assert!(!saved.to_string().contains(ACCESS), "{saved}");
    assert_eq!(saved["holding"]["fills"]["path"], ".codex/auth.json");
    assert_eq!(saved["authentication"]["state"], "login_completed");

    kestrel.teardown().await;
}
