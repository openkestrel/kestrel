use std::fmt::Write as _;

use sha2::{Digest as _, Sha256};

/// The credential as the supervisor presents it. Never stored: `Store` keeps only its digest,
/// so a copy of the database is not a set of usable credentials.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    pub fn mint() -> Self {
        let mut bytes = [0u8; 32];
        getrandom::fill(&mut bytes).expect("the operating system should have entropy to spare");

        Self(hex(&bytes))
    }

    pub fn presented(token: &str) -> Self {
        Self(token.to_owned())
    }

    pub fn digest(&self) -> String {
        hex(&Sha256::digest(self.0.as_bytes()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut text, byte| {
        let _ = write!(text, "{byte:02x}");
        text
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_minted_secrets_differ() {
        assert_ne!(Secret::mint(), Secret::mint());
    }

    #[test]
    fn a_secrets_digest_is_not_the_secret() {
        let secret = Secret::mint();

        assert_ne!(secret.digest(), secret.as_str());
        assert_eq!(secret.digest(), Secret::presented(secret.as_str()).digest());
    }
}
