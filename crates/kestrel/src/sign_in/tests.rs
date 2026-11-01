use super::save::chatgpt_login;

#[test]
fn a_chatgpt_login_carries_the_tokens_codex_reads() {
    let login = r#"{"OPENAI_API_KEY":null,"tokens":{"id_token":"a","access_token":"b","refresh_token":"c","account_id":"d"},"last_refresh":"2026-01-01T00:00:00Z"}"#;
    assert!(chatgpt_login(login));

    let declared = r#"{"auth_mode":"chatgpt","tokens":{"id_token":"a","access_token":"b","refresh_token":"c"}}"#;
    assert!(chatgpt_login(declared));
}

#[test]
fn an_api_key_login_or_a_partial_one_is_not_a_subscription() {
    for refused in [
        r#"{"OPENAI_API_KEY":"sk-proj-abc"}"#,
        r#"{"auth_mode":"apikey","OPENAI_API_KEY":"sk","tokens":{"id_token":"a","access_token":"b","refresh_token":"c"}}"#,
        r#"{"tokens":{"id_token":"a","access_token":"b"}}"#,
        r#"{"tokens":{"id_token":"a","access_token":"","refresh_token":"c"}}"#,
        r#"["tokens"]"#,
        "not json",
    ] {
        assert!(!chatgpt_login(refused), "{refused} was accepted");
    }
}
