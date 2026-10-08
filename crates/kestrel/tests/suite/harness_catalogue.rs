use crate::support::Kestrel;
use reqwest::StatusCode;
use serde_json::Value;

async fn catalogue(kestrel: &Kestrel) -> Value {
    let response = reqwest::get(format!("{}/operator/harnesses", kestrel.operator()))
        .await
        .expect("the operator boundary answers");
    assert_eq!(response.status(), StatusCode::OK);
    response.json().await.expect("catalogue metadata")
}

#[tokio::test]
async fn guided_methods_name_their_inputs_and_owners_without_setup() {
    let kestrel = Kestrel::boot().await;
    let rows = catalogue(&kestrel).await;
    let rows = rows.as_array().expect("harness rows");
    assert_eq!(rows.len(), 3);
    let method = |harness: &str, id: &str| {
        rows.iter()
            .find(|row| row["name"] == harness)
            .expect("a product harness")["sign_in_methods"]
            .as_array()
            .expect("guided methods")
            .iter()
            .find(|method| method["id"] == id)
            .expect("the supported sign-in method")
    };
    let expected = [
        (
            "claude",
            "claude-setup-token",
            "subscription",
            "operator",
            "token",
            "variable",
            "CLAUDE_CODE_OAUTH_TOKEN",
        ),
        (
            "claude",
            "anthropic-api-key",
            "key",
            "organization",
            "token",
            "variable",
            "ANTHROPIC_API_KEY",
        ),
        (
            "codex",
            "codex-device-auth",
            "subscription",
            "operator",
            "file",
            "file",
            ".codex/auth.json",
        ),
        (
            "codex",
            "openai-api-key",
            "key",
            "organization",
            "token",
            "variable",
            "OPENAI_API_KEY",
        ),
        (
            "opencode",
            "opencode-go-zen",
            "subscription",
            "operator",
            "token",
            "variable",
            "OPENCODE_API_KEY",
        ),
        (
            "opencode",
            "anthropic-api-key",
            "key",
            "organization",
            "token",
            "variable",
            "ANTHROPIC_API_KEY",
        ),
        (
            "opencode",
            "openai-api-key",
            "key",
            "organization",
            "token",
            "variable",
            "OPENAI_API_KEY",
        ),
    ];
    assert_eq!(
        rows.iter()
            .map(|row| row["sign_in_methods"].as_array().unwrap().len())
            .sum::<usize>(),
        7
    );
    for (harness, id, kind, ownership, input, target, destination) in expected {
        let method = method(harness, id);
        assert_eq!(method["kind"], kind);
        assert_eq!(method["ownership"], ownership);
        assert_eq!(method["input"], input);
        assert_eq!(method["fills"]["kind"], target);
        assert_eq!(
            method["fills"][if target == "file" { "path" } else { "variable" }],
            destination
        );
        assert!(!method["name"].as_str().unwrap().is_empty());
    }
    assert_eq!(
        method("claude", "claude-setup-token")["relay"],
        "claude-setup-token"
    );
    assert_eq!(
        method("codex", "codex-device-auth")["relay"],
        "codex-device-auth"
    );
    assert_eq!(
        method("opencode", "opencode-go-zen")["console_url"],
        "https://opencode.ai/auth"
    );
    for row in rows {
        assert!(row.get("available").is_none());
        for method in row["sign_in_methods"].as_array().unwrap() {
            if method["kind"] == "key" || method["id"] == "opencode-go-zen" {
                assert!(method.get("relay").is_none());
            }
        }
    }
}

#[tokio::test]
async fn method_selection_refuses_unknown_harnesses_and_unsupported_combinations() {
    let kestrel = Kestrel::boot().await;
    for (harness, method, field, allowed) in [
        (
            "custom",
            "openai-api-key",
            "harness",
            vec!["opencode", "claude", "codex"],
        ),
        (
            "codex",
            "anthropic-api-key",
            "method",
            vec!["codex-device-auth", "openai-api-key"],
        ),
        (
            "claude",
            "unknown",
            "method",
            vec!["claude-setup-token", "anthropic-api-key"],
        ),
    ] {
        let response = reqwest::get(format!(
            "{}/operator/harnesses/{harness}/sign-in-methods/{method}",
            kestrel.operator()
        ))
        .await
        .expect("the operator boundary answers");
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let body: Value = response.json().await.expect("a typed diagnostic");
        assert_eq!(body["kind"], "invalid_field");
        assert_eq!(body["field"], field);
        assert_eq!(body["context"]["constraint"], "offered");
        assert_eq!(
            body["context"]["allowed_values"],
            serde_json::json!(allowed)
        );
        assert_eq!(body["next_steps"][0]["action"], "correct_field");
        assert_eq!(body["next_steps"][0]["operation"], "show_sign_in_method");
        assert_eq!(body["next_steps"][0]["field"], field);
        let _: kestrel_operator_types::Diagnostic =
            serde_json::from_value(body).expect("the authored diagnostic shape");
    }
    let response = reqwest::get(format!(
        "{}/operator/harnesses/codex/sign-in-methods/codex-device-auth",
        kestrel.operator()
    ))
    .await
    .expect("the operator boundary answers");
    assert_eq!(response.status(), StatusCode::OK);
    let method: Value = response.json().await.expect("a supported method");
    assert_eq!(method["id"], "codex-device-auth");
    assert_eq!(method["fills"]["path"], ".codex/auth.json");
    let _: kestrel_operator_types::SignInMethod =
        serde_json::from_value(method).expect("the authored method shape");
}

#[tokio::test]
async fn dispatch_defaults_match_catalogue_commands_and_overrides_remain_authoritative() {
    use clap::Parser as _;
    use kestrel::cli::Cli;

    let kestrel = Kestrel::boot().await;
    let rows: Vec<kestrel_operator_types::HarnessCatalogueEntry> =
        serde_json::from_value(catalogue(&kestrel).await).expect("the generated catalogue shape");
    let configured = |args: &[&str]| {
        Cli::try_parse_from(args)
            .expect("valid control-plane configuration")
            .dispatch("127.0.0.1:7717".parse().unwrap())
            .expect("dispatch configuration")
            .harnesses
    };
    let defaults = configured(&["kestrel-control-plane"]);
    for (name, command) in [
        ("opencode", "opencode acp --print-logs"),
        ("claude", "claude-agent-acp"),
        ("codex", "codex-acp"),
    ] {
        let row = rows
            .iter()
            .find(|row| row.name == name)
            .expect("a product harness");
        assert_eq!(row.command, command);
        let spawned = defaults
            .iter()
            .find(|spawned| spawned.name == name)
            .expect("a default command");
        assert_eq!(spawned.command, row.command);
    }
    let overrides = configured(&[
        "kestrel-control-plane",
        "--harness-command",
        "claude=custom-claude --acp",
        "--harness-command",
        "bespoke=custom-agent --acp",
    ]);
    assert_eq!(overrides.len(), 2);
    assert_eq!(overrides[0].name, "claude");
    assert_eq!(overrides[0].command, "custom-claude --acp");
    assert_eq!(overrides[1].name, "bespoke");
    assert_eq!(overrides[1].command, "custom-agent --acp");
    assert!(rows.iter().all(|row| row.name != "bespoke"));
}
