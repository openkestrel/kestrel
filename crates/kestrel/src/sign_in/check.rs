use std::time::Duration;

use reqwest::StatusCode;
use serde::{Deserialize, Serialize};

const DEADLINE: Duration = Duration::from_secs(10);
const ERROR_BODY_LIMIT: usize = 64 * 1024;
const ANTHROPIC_VERSION: &str = "2023-06-01";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    Anthropic,
    #[serde(rename = "openai")]
    OpenAi,
}

impl Provider {
    pub const fn as_str(self) -> &'static str {
        match self {
            Provider::Anthropic => "anthropic",
            Provider::OpenAi => "openai",
        }
    }

    /// The catalogued methods whose provider documents an authenticated read that spends no model
    /// usage; every other method saves unchecked.
    pub fn checking(method: &str) -> Option<Self> {
        match method {
            "anthropic-api-key" => Some(Provider::Anthropic),
            "openai-api-key" => Some(Provider::OpenAi),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Accepted,
    Rejected,
    /// Answered, but about permission, credit or something else that is not whether the
    /// credential authenticates.
    Inconclusive,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Checked {
    pub provider: Provider,
    pub outcome: Outcome,
    pub status: Option<u16>,
    pub provider_error: Option<String>,
}

#[derive(Clone)]
pub struct Providers {
    anthropic: String,
    openai: String,
    client: reqwest::Client,
}

impl Default for Providers {
    fn default() -> Self {
        Self::at(
            "https://api.anthropic.com",
            "https://api.openai.com",
            DEADLINE,
        )
    }
}

impl Providers {
    pub fn at(anthropic: &str, openai: &str, deadline: Duration) -> Self {
        Self {
            anthropic: anthropic.trim_end_matches('/').to_owned(),
            openai: openai.trim_end_matches('/').to_owned(),
            client: reqwest::Client::builder()
                .timeout(deadline)
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .expect("an HTTP client with a timeout"),
        }
    }

    pub async fn check(&self, provider: Provider, key: &str) -> Checked {
        let request = match provider {
            Provider::Anthropic => self
                .client
                .get(format!("{}/v1/models?limit=1", self.anthropic))
                .header("x-api-key", key)
                .header("anthropic-version", ANTHROPIC_VERSION),
            Provider::OpenAi => self
                .client
                .get(format!("{}/v1/models", self.openai))
                .bearer_auth(key),
        };

        let Ok(mut response) = request.send().await else {
            return Checked {
                provider,
                outcome: Outcome::Unavailable,
                status: None,
                provider_error: None,
            };
        };
        let status = response.status();
        let outcome = judged(status);
        let provider_error = if status.is_success() {
            None
        } else {
            let mut body = Vec::new();
            while let Ok(Some(chunk)) = response.chunk().await {
                body.extend_from_slice(&chunk);
                if body.len() > ERROR_BODY_LIMIT {
                    break;
                }
            }
            error_type(&body)
        };

        Checked {
            provider,
            outcome,
            status: Some(status.as_u16()),
            provider_error,
        }
    }
}

/// Only 401 rejects: a provider may refuse its model list for permission, credit or quota while
/// the key still authenticates.
fn judged(status: StatusCode) -> Outcome {
    if status.is_success() {
        Outcome::Accepted
    } else if status == StatusCode::UNAUTHORIZED {
        Outcome::Rejected
    } else if status.is_client_error()
        && status != StatusCode::REQUEST_TIMEOUT
        && status != StatusCode::TOO_MANY_REQUESTS
    {
        Outcome::Inconclusive
    } else {
        Outcome::Unavailable
    }
}

fn error_type(body: &[u8]) -> Option<String> {
    let body: serde_json::Value = serde_json::from_slice(body).ok()?;
    let error = body.get("error")?;
    let named = error
        .get("code")
        .and_then(serde_json::Value::as_str)
        .or_else(|| error.get("type").and_then(serde_json::Value::as_str))?;
    let token = !named.is_empty()
        && named.len() <= 64
        && named
            .chars()
            .all(|character| character.is_ascii_lowercase() || character == '_');

    token.then(|| named.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_an_authentication_refusal_rejects_a_key() {
        assert_eq!(judged(StatusCode::OK), Outcome::Accepted);
        assert_eq!(judged(StatusCode::UNAUTHORIZED), Outcome::Rejected);
        for inconclusive in [
            StatusCode::PAYMENT_REQUIRED,
            StatusCode::FORBIDDEN,
            StatusCode::NOT_FOUND,
        ] {
            assert_eq!(
                judged(inconclusive),
                Outcome::Inconclusive,
                "{inconclusive}"
            );
        }
        for unavailable in [408, 429, 500, 502, 503, 529] {
            let status = StatusCode::from_u16(unavailable).unwrap();
            assert_eq!(judged(status), Outcome::Unavailable, "{status}");
        }
    }

    #[test]
    fn a_providers_error_message_is_never_kept() {
        let openai = br#"{"error":{"message":"Incorrect API key provided: sk-abc***wxyz.","type":"invalid_request_error","code":"invalid_api_key"}}"#;
        assert_eq!(error_type(openai).as_deref(), Some("invalid_api_key"));

        let anthropic = br#"{"type":"error","error":{"type":"authentication_error","message":"invalid x-api-key"}}"#;
        assert_eq!(
            error_type(anthropic).as_deref(),
            Some("authentication_error")
        );

        let prose = br#"{"error":{"type":"the key sk-ant-123 is wrong"}}"#;
        assert_eq!(error_type(prose), None);
        assert_eq!(error_type(b"<html>bad gateway</html>"), None);
    }
}
