use jiff::Timestamp;
use kestrel_operator_types::SignInMethod;

use super::check::{Checked, Outcome, Provider};
use super::vocabulary::{Source, State, UseResult, UseSource};
use crate::domain::SubscriptionProfile;

/// Whether a revision's material authenticates, which says nothing of what a harness can do
/// with it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Authentication {
    pub state: State,
    pub source: Source,
    pub observed_at: Timestamp,
    pub provider_check: Option<Checked>,
}

impl Authentication {
    pub fn unchecked(source: Source, now: Timestamp) -> Self {
        Self {
            state: State::Unchecked,
            source,
            observed_at: now,
            provider_check: None,
        }
    }

    pub(super) fn checked(checked: Checked, now: Timestamp) -> Self {
        Self {
            state: match checked.outcome {
                Outcome::Accepted => State::CredentialAccepted,
                Outcome::Rejected | Outcome::Inconclusive | Outcome::Unavailable => {
                    State::Unchecked
                }
            },
            source: Source::ProviderCheck,
            observed_at: now,
            provider_check: Some(checked),
        }
    }
}

/// The latest result of one harness using one model with one revision; another model's result
/// never stands in for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelUse {
    pub harness: String,
    pub model: String,
    pub image: Option<String>,
    pub result: UseResult,
    pub source: UseSource,
    pub observed_at: Timestamp,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Holding {
    Profile(SubscriptionProfile),
    ProviderCredential(String),
}

#[derive(Debug, Clone)]
pub struct SignIn {
    pub harness: String,
    pub method: &'static SignInMethod,
    pub holding: Holding,
    pub revision: i64,
    /// The revision a harness's own refresh advanced to make this one.
    pub refreshed_from: Option<i64>,
    pub saved_at: Timestamp,
    pub authentication: Authentication,
    pub model_use: Vec<ModelUse>,
}

pub struct Saved {
    pub sign_in: SignIn,
    pub unavailable: Option<Provider>,
}
