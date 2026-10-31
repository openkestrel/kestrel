use serde_json::json;

use super::*;

fn harness(name: &str) -> wire::HarnessCatalogueEntry {
    let methods = match name {
        "claude" => json!([
            { "id": "claude-setup-token", "name": "Claude subscription", "kind": "subscription",
              "ownership": "operator", "input": "token", "relay": "claude-setup-token",
              "fills": { "kind": "variable", "variable": "CLAUDE_CODE_OAUTH_TOKEN" } },
            { "id": "anthropic-api-key", "name": "Anthropic API key", "kind": "key",
              "ownership": "organization", "input": "token",
              "fills": { "kind": "variable", "variable": "ANTHROPIC_API_KEY" } },
        ]),
        _ => json!([
            { "id": "codex-device-auth", "name": "Codex subscription", "kind": "subscription",
              "ownership": "operator", "input": "file", "relay": "codex-device-auth",
              "fills": { "kind": "file", "path": ".codex/auth.json" } },
            { "id": "openai-api-key", "name": "OpenAI API key", "kind": "key",
              "ownership": "organization", "input": "token",
              "fills": { "kind": "variable", "variable": "OPENAI_API_KEY" } },
        ]),
    };
    serde_json::from_value(json!({
        "name": name, "command": "acp", "availability": null, "sign_in_methods": methods,
    }))
    .expect("a catalogue entry")
}

fn picked(chosen: Result<Chosen<'_>, String>) -> String {
    match chosen {
        Ok(Chosen::Method(method)) => method.id.clone(),
        other => panic!("expected one method, got {other:?}"),
    }
}

#[test]
fn a_terminal_is_always_asked_when_no_method_is_named() {
    let claude = harness("claude");
    let codex = harness("codex");

    for (entry, mode, offered) in [
        (&claude, Mode::Any, 2),
        (&claude, Mode::Token, 2),
        (&codex, Mode::File, 1),
    ] {
        match chosen(entry, None, mode, true) {
            Ok(Chosen::Ask(candidates)) => assert_eq!(candidates.len(), offered),
            other => panic!("expected to ask, got {other:?}"),
        }
    }
}

#[test]
fn without_a_terminal_the_input_mode_must_leave_one_method() {
    let claude = harness("claude");
    let codex = harness("codex");

    assert_eq!(
        picked(chosen(&codex, None, Mode::File, false)),
        "codex-device-auth"
    );
    assert_eq!(
        picked(chosen(&codex, None, Mode::Token, false)),
        "openai-api-key"
    );
    for (entry, mode) in [
        (&claude, Mode::Any),
        (&claude, Mode::Token),
        (&codex, Mode::Any),
    ] {
        let refused = chosen(entry, None, mode, false).expect_err("ambiguous");
        assert!(refused.contains("--method"), "{refused}");
        assert!(refused.contains("held in your own Profile"), "{refused}");
        assert!(refused.contains("held by the Organization"), "{refused}");
    }
}

#[test]
fn a_named_method_must_take_the_input_given() {
    let claude = harness("claude");
    let codex = harness("codex");

    assert_eq!(
        picked(chosen(&claude, Some("anthropic-api-key"), Mode::Any, false)),
        "anthropic-api-key"
    );
    let refused =
        chosen(&codex, Some("codex-device-auth"), Mode::Token, false).expect_err("a file method");
    assert!(refused.contains("--file PATH"), "{refused}");
    let refused =
        chosen(&claude, Some("claude-setup-token"), Mode::File, false).expect_err("a token method");
    assert!(refused.contains("--token"), "{refused}");
    let refused = chosen(&claude, Some("nonsense"), Mode::Any, true).expect_err("unknown");
    assert!(refused.contains("claude-setup-token"), "{refused}");
    let refused = chosen(&claude, None, Mode::File, true).expect_err("no file method");
    assert!(refused.contains("login file"), "{refused}");
}
