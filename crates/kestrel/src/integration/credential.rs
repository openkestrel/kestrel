use std::fmt;

/// The Organization's GitHub App: what kestrel signs an installation-token request with, in
/// place of a token kestrel merely presents. Kept rather than digested — a request to GitHub
/// needs it again every time the cached installation token lapses — so it carries no `Display`
/// and a `Debug` that redacts, and reaching the key itself is spelled out at the two calls that
/// need it: sealing it at registration, and signing a JWT with it at each mint.
#[derive(Clone, PartialEq, Eq)]
pub struct App {
    pub id: i64,
    pub installation: i64,
    key: Option<String>,
}

impl App {
    pub fn held(id: i64, installation: i64, private_key: &str) -> Self {
        Self {
            id,
            installation,
            key: Some(private_key.to_owned()),
        }
    }

    pub fn erased(id: i64, installation: i64) -> Self {
        Self {
            id,
            installation,
            key: None,
        }
    }

    pub fn private_key(&self) -> Option<&str> {
        self.key.as_deref()
    }
}

impl fmt::Debug for App {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "App {{ id: {}, installation: {}, key: redacted }}",
            self.id, self.installation
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_apps_private_key_does_not_appear_in_what_is_logged_of_it() {
        let app = App::held(
            1,
            2,
            "-----BEGIN RSA PRIVATE KEY-----\nnotinalog\n-----END---",
        );

        assert!(!format!("{app:?}").contains("notinalog"));
    }
}
