//! A catalogued Sign-in Method's material, held where its kind says (ADR-0046), with what each
//! revision of it is known to do. Saving never spends model usage.

pub mod check;
pub mod credential_use;
pub mod handoff;

use std::str::FromStr;

use anyhow::{Result, bail};
use jiff::Timestamp;
use kestrel_operator_types::{SignInFill, SignInMethod, SignInMethodInput, SignInMethodOwnership};

use crate::catalogue;
use crate::declined::{Constraint, Next, Reason, Resource};
use crate::domain::{Organization, SubscriptionProfile};
use crate::profile::Entry;
use crate::store::{Store, Tx};
use check::{Checked, Outcome, Provider, Providers};

pub const SAVE: &str = "save_sign_in";

macro_rules! vocabulary {
    ($name:ident { $($variant:ident => $text:literal),+ $(,)? }) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum $name {
            $($variant),+
        }

        impl $name {
            pub const fn as_str(self) -> &'static str {
                match self {
                    $($name::$variant => $text),+
                }
            }
        }

        impl FromStr for $name {
            type Err = anyhow::Error;

            fn from_str(text: &str) -> Result<Self> {
                match text {
                    $($text => Ok($name::$variant),)+
                    other => bail!("{other} is not a {}", stringify!($name)),
                }
            }
        }
    };
}

vocabulary!(State {
    Unchecked => "unchecked",
    LoginCompleted => "login_completed",
    CredentialAccepted => "credential_accepted",
    AuthenticationFailed => "authentication_failed",
    Expired => "expired",
    NotCovered => "not_covered",
});

vocabulary!(Source {
    Import => "import",
    ProviderCheck => "provider_check",
    Relay => "relay",
    GenericWrite => "generic_write",
    Refresh => "refresh",
    Session => "session",
});

vocabulary!(UseResult {
    Worked => "worked",
    AuthenticationFailed => "authentication_failed",
    NotCovered => "not_covered",
});

vocabulary!(UseSource {
    ModelTest => "model_test",
    Session => "session",
});

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
    pub fn unchecked(source: Source) -> Self {
        Self {
            state: State::Unchecked,
            source,
            observed_at: Timestamp::now(),
            provider_check: None,
        }
    }

    fn checked(checked: Checked) -> Self {
        Self {
            state: match checked.outcome {
                Outcome::Accepted => State::CredentialAccepted,
                Outcome::Rejected | Outcome::Inconclusive | Outcome::Unavailable => {
                    State::Unchecked
                }
            },
            source: Source::ProviderCheck,
            observed_at: Timestamp::now(),
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

/// Where a method's material goes, resolved before anything is asked of a provider and again at
/// commit, when an Operator Profile still missing is declared.
enum Destination {
    Profile(Option<SubscriptionProfile>, Entry),
    ProviderCredential(Organization, String),
}

pub async fn save(
    store: &Store,
    providers: &Providers,
    organization: &str,
    harness: &str,
    method: &str,
    value: &str,
) -> Result<Saved> {
    let offered = catalogue::sign_in_method(SAVE, harness, method)?;
    let material = structural(offered, value)?;

    let examined = {
        let mut tx = store.read().await?;
        let destination = destination(&mut tx, organization, harness, offered, false).await?;
        revision(&mut tx, &destination).await?
    };

    let checked = match Provider::checking(&offered.id) {
        Some(provider) => Some(providers.check(provider, &material).await),
        None => None,
    };
    if let Some(checked) = checked.as_ref().filter(|c| c.outcome == Outcome::Rejected) {
        bail!(Reason::CredentialRejected {
            operation: SAVE,
            organization: organization.to_owned(),
            harness: harness.to_owned(),
            method: offered.id.clone(),
            provider: checked.provider.as_str(),
            status: checked.status,
            provider_error: checked.provider_error.clone(),
            message: format!(
                "{} did not accept this key, so whatever was saved before is kept",
                checked.provider.as_str()
            ),
        });
    }
    let unavailable = checked
        .as_ref()
        .filter(|checked| checked.outcome == Outcome::Unavailable)
        .map(|checked| checked.provider);
    let authentication = match checked {
        Some(checked) => Authentication::checked(checked),
        None => Authentication::unchecked(Source::Import),
    };

    let mut tx = store.begin().await?;
    let destination = destination(&mut tx, organization, harness, offered, true).await?;
    if revision(&mut tx, &destination).await? != examined {
        let (resource, reference) = match &destination {
            Destination::Profile(profile, _) => (
                Resource::SubscriptionProfile,
                profile
                    .as_ref()
                    .map_or_else(String::new, |p| p.name.clone()),
            ),
            Destination::ProviderCredential(_, variable) => {
                (Resource::ProviderCredential, variable.clone())
            }
        };
        bail!(Reason::StateConflict {
            operation: SAVE,
            resource,
            reference: reference.clone(),
            organization: Some(organization.to_owned()),
            state: "replaced",
            holding_session: None,
            next: Next::inspect(resource, reference),
            message: format!(
                "the {} sign-in for {harness} was replaced while this one was being checked, \
                 and the replacement stands",
                offered.id
            ),
        });
    }

    let (holding, revision, saved_at) = match destination {
        Destination::Profile(profile, entry) => {
            let profile = profile.expect("declared at commit");
            let held = tx
                .profiles()
                .hold(&profile, &entry, &material, &authentication)
                .await?;
            (Holding::Profile(profile), held.revision, held.set_at)
        }
        Destination::ProviderCredential(organization, variable) => {
            let held = tx
                .organizations()
                .hold_provider_credential(organization.id, &variable, &material, &authentication)
                .await?;
            (
                Holding::ProviderCredential(variable),
                held.revision,
                held.set_at,
            )
        }
    };
    tx.commit().await?;

    Ok(Saved {
        sign_in: SignIn {
            harness: harness.to_owned(),
            method: offered,
            holding,
            revision,
            refreshed_from: None,
            saved_at,
            authentication,
            model_use: Vec::new(),
        },
        unavailable,
    })
}

/// Every catalogued method with material saved, an aliased key once per harness offering it.
pub async fn saved(store: &Store, organization: &str) -> Result<Vec<SignIn>> {
    let mut tx = store.read().await?;
    let organization = tx.organizations().named(organization).await?;
    let credentials = tx
        .organizations()
        .provider_credentials_held(organization.id)
        .await?;
    let mut profiles = Vec::new();
    for profile in operator_profiles(&mut tx, &organization).await? {
        let held = tx.profiles().held(&profile).await?;
        profiles.push((profile, held));
    }

    let mut saved = Vec::new();
    for row in catalogue::harnesses() {
        for method in &row.sign_in_methods {
            let found: Vec<(Holding, i64, Timestamp)> = match method.ownership {
                SignInMethodOwnership::Organization => {
                    let SignInFill::SignInVariableFill(fill) = &method.fills else {
                        continue;
                    };
                    credentials
                        .iter()
                        .filter(|held| held.variable == fill.variable)
                        .map(|held| {
                            (
                                Holding::ProviderCredential(held.variable.clone()),
                                held.revision,
                                held.set_at,
                            )
                        })
                        .collect()
                }
                SignInMethodOwnership::Operator => {
                    let entry = filled(method)?;
                    profiles
                        .iter()
                        .flat_map(|(profile, held)| {
                            held.iter().filter(|held| held.entry == entry).map(|held| {
                                (
                                    Holding::Profile(profile.clone()),
                                    held.revision,
                                    held.set_at,
                                )
                            })
                        })
                        .collect()
                }
            };
            for (holding, revision, saved_at) in found {
                saved.push(SignIn {
                    harness: row.name.clone(),
                    method,
                    holding,
                    revision,
                    refreshed_from: tx.sign_ins().refreshed_from(revision).await?,
                    saved_at,
                    authentication: tx.sign_ins().authentication(revision).await?,
                    model_use: tx.sign_ins().model_use(revision, &row.name).await?,
                });
            }
        }
    }

    Ok(saved)
}

/// Refuses before storage is touched, and never repeats any of the value in a refusal.
fn structural(method: &SignInMethod, value: &str) -> Result<String> {
    match method.input {
        SignInMethodInput::Token => {
            let token = value.trim();
            if token.is_empty() {
                bail!(Reason::InvalidField {
                    field: "value",
                    operation: SAVE,
                    constraint: Constraint::NonEmpty,
                    allowed: None,
                    message: format!("{} needs a token, and none was given", method.name),
                });
            }
            if !token.chars().all(|character| character.is_ascii_graphic()) {
                bail!(Reason::InvalidField {
                    field: "value",
                    operation: SAVE,
                    constraint: Constraint::SingleToken,
                    allowed: None,
                    message: format!(
                        "{} takes one token, without spaces or line breaks inside it",
                        method.name
                    ),
                });
            }
            Ok(token.to_owned())
        }
        SignInMethodInput::File => {
            if value.trim().is_empty() {
                bail!(Reason::InvalidField {
                    field: "value",
                    operation: SAVE,
                    constraint: Constraint::NonEmpty,
                    allowed: None,
                    message: format!("{} needs a login file, and it was empty", method.name),
                });
            }
            if !chatgpt_login(value) {
                bail!(Reason::InvalidField {
                    field: "value",
                    operation: SAVE,
                    constraint: Constraint::ChatgptLoginFile,
                    allowed: None,
                    message: format!(
                        "{} takes the auth.json a ChatGPT sign-in writes, holding its id, access \
                         and refresh tokens; this file is not one",
                        method.name
                    ),
                });
            }
            Ok(value.to_owned())
        }
    }
}

/// Codex's ChatGPT login, judged by the fields the pinned Codex reads and not by whether the
/// tokens look current. An API-key-only file is not a subscription.
fn chatgpt_login(value: &str) -> bool {
    let Ok(serde_json::Value::Object(login)) = serde_json::from_str(value) else {
        return false;
    };
    if login
        .get("auth_mode")
        .is_some_and(|mode| mode.as_str() != Some("chatgpt"))
    {
        return false;
    }
    let Some(tokens) = login.get("tokens").and_then(serde_json::Value::as_object) else {
        return false;
    };

    ["id_token", "access_token", "refresh_token"]
        .iter()
        .all(|field| {
            tokens
                .get(*field)
                .and_then(serde_json::Value::as_str)
                .is_some_and(|token| !token.is_empty())
        })
}

pub(crate) fn filled(method: &SignInMethod) -> Result<Entry> {
    match &method.fills {
        SignInFill::SignInVariableFill(fill) => Entry::variable(&fill.variable),
        SignInFill::SignInFileFill(fill) => Entry::file(&fill.path),
    }
}

async fn destination(
    tx: &mut Tx<'_>,
    organization: &str,
    harness: &str,
    method: &SignInMethod,
    declaring: bool,
) -> Result<Destination> {
    let organization = tx.organizations().named(organization).await?;

    match (&method.ownership, &method.fills) {
        (SignInMethodOwnership::Organization, SignInFill::SignInVariableFill(fill)) => Ok(
            Destination::ProviderCredential(organization, fill.variable.clone()),
        ),
        (SignInMethodOwnership::Organization, SignInFill::SignInFileFill(_)) => {
            bail!("an Organization holds Provider Credentials, never a login file")
        }
        (SignInMethodOwnership::Operator, _) => {
            let entry = filled(method)?;
            let profile = operator_profile(tx, &organization, harness, declaring).await?;
            Ok(Destination::Profile(profile, entry))
        }
    }
}

async fn revision(tx: &mut Tx<'_>, destination: &Destination) -> Result<Option<i64>> {
    match destination {
        Destination::Profile(None, _) => Ok(None),
        Destination::Profile(Some(profile), entry) => {
            tx.profiles().revision_of(profile, entry).await
        }
        Destination::ProviderCredential(organization, variable) => {
            tx.organizations()
                .provider_credential_revision(organization.id, variable)
                .await
        }
    }
}

async fn operator_profiles(
    tx: &mut Tx<'_>,
    organization: &Organization,
) -> Result<Vec<SubscriptionProfile>> {
    let Some(operator) = tx.operators().current().await? else {
        return Ok(Vec::new());
    };

    Ok(tx
        .profiles()
        .all(organization)
        .await?
        .into_iter()
        .filter(|profile| profile.owner_operator == Some(operator.id))
        .collect())
}

/// The Operator's one Profile, or the one of theirs already holding this harness's subscription;
/// a free-text owner's Profile is never chosen, and neither is one of several arbitrarily.
async fn operator_profile(
    tx: &mut Tx<'_>,
    organization: &Organization,
    harness: &str,
    declaring: bool,
) -> Result<Option<SubscriptionProfile>> {
    let operator = tx
        .operators()
        .current()
        .await?
        .ok_or(Reason::MissingOperator { operation: SAVE })?;
    let mut owned = operator_profiles(tx, organization).await?;

    match owned.len() {
        0 if declaring => Ok(Some(
            tx.profiles()
                .declare(
                    organization,
                    &operator.name,
                    &operator.name,
                    Some(operator.id),
                )
                .await?
                .record,
        )),
        0 => Ok(None),
        1 => Ok(owned.pop()),
        _ => {
            let subscriptions = catalogue::harnesses()
                .iter()
                .filter(|row| row.name == harness)
                .flat_map(|row| &row.sign_in_methods)
                .filter(|method| method.ownership == SignInMethodOwnership::Operator)
                .map(filled)
                .collect::<Result<Vec<_>>>()?;
            let mut holding = Vec::new();
            for profile in &owned {
                let held = tx.profiles().held(profile).await?;
                if held.iter().any(|held| subscriptions.contains(&held.entry)) {
                    holding.push(profile.clone());
                }
            }
            if holding.len() == 1 {
                return Ok(holding.pop());
            }

            let names: Vec<&str> = owned.iter().map(|profile| profile.name.as_str()).collect();
            bail!(Reason::StateConflict {
                operation: SAVE,
                resource: Resource::SubscriptionProfile,
                reference: owned[0].name.clone(),
                organization: Some(organization.name.clone()),
                state: "several_operator_profiles",
                holding_session: None,
                next: Next::inspect(Resource::SubscriptionProfile, owned[0].name.clone()),
                message: format!(
                    "{} owns several Subscription Profiles in {} ({}), and none alone holds a \
                     {harness} subscription to replace; hold it in one by name instead",
                    operator.name,
                    organization.name,
                    names.join(", ")
                ),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
