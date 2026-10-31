pub(super) struct Secrets(Vec<String>);

impl Secrets {
    /// A login file is redacted as a whole and token by token, because a harness that logs one
    /// logs a token out of it rather than the file.
    pub(super) fn of(values: &[&str]) -> Self {
        let mut secrets = Vec::new();
        for value in values {
            secrets.push((*value).to_owned());
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(value) {
                leaves(&parsed, &mut secrets);
            }
        }
        secrets.retain(|secret| secret.len() >= 8);
        secrets.sort_by_key(|secret| std::cmp::Reverse(secret.len()));
        secrets.dedup();

        Self(secrets)
    }

    pub(super) fn redacted(&self, text: &str) -> String {
        self.0.iter().fold(text.to_owned(), |text, secret| {
            text.replace(secret, "[redacted]")
        })
    }
}

fn leaves(value: &serde_json::Value, into: &mut Vec<String>) {
    match value {
        serde_json::Value::String(leaf) => into.push(leaf.clone()),
        serde_json::Value::Array(items) => items.iter().for_each(|item| leaves(item, into)),
        serde_json::Value::Object(fields) => fields.values().for_each(|field| leaves(field, into)),
        _ => {}
    }
}

#[test]
fn a_held_secret_and_every_token_inside_one_are_redacted() {
    let login = r#"{"tokens":{"access_token":"eyJhbGciOiJSUzI1NiJ9.payload","refresh_token":"rt_abcdefghijkl"},"last_refresh":"2026"}"#;
    let secrets = Secrets::of(&["sk-go-0123456789", login]);

    let said = secrets.redacted(
        "key sk-go-0123456789 refused; refreshing with rt_abcdefghijkl; bearer eyJhbGciOiJSUzI1NiJ9.payload",
    );

    assert_eq!(
        said,
        "key [redacted] refused; refreshing with [redacted]; bearer [redacted]"
    );
}
