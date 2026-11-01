//! A catalogued Sign-in Method's material, held where its kind says (ADR-0046), with what each
//! revision of it is known to do. Saving never spends model usage.

pub mod check;
pub mod credential_use;
pub mod handoff;
pub mod model_test;
pub mod relay;

mod profiles;
mod record;
mod save;
mod saved;
#[cfg(test)]
mod tests;
mod vocabulary;

pub use record::{Authentication, Holding, ModelUse, Saved, SignIn};
pub use save::{SAVE, save};
pub use saved::saved;
pub use vocabulary::{Source, State, TestOutcome, TestState, UseResult, UseSource};

pub(crate) use profiles::{
    holding_a_subscription, names, operator_profiles, several_operator_profiles,
};
pub(crate) use saved::saved_in;

use anyhow::Result;
use kestrel_operator_types::{SignInFill, SignInMethod};

use crate::profile::Entry;

pub(crate) fn filled(method: &SignInMethod) -> Result<Entry> {
    match &method.fills {
        SignInFill::SignInVariableFill(fill) => Entry::variable(&fill.variable),
        SignInFill::SignInFileFill(fill) => Entry::file(&fill.path),
    }
}
