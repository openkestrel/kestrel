use anyhow::{Result, bail};
use kestrel_operator_types::{SignInFill, SignInMethod, SignInMethodInput, SignInMethodOwnership};

use super::check::{Outcome, Provider, Providers};
use super::filled;
use super::profiles::operator_profile;
use super::record::{Authentication, Holding, Saved, SignIn};
use super::vocabulary::Source;
use crate::catalogue;
use crate::declined::{Constraint, Next, Reason, Resource};
use crate::domain::{Organization, SubscriptionProfile};
use crate::profile::Entry;
use crate::store::{Store, Tx};

pub const SAVE: &str = "save_sign_in";

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
        Some(checked) => Authentication::checked(checked, store.clock().now()),
        None => Authentication::unchecked(Source::Import, store.clock().now()),
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

/// Refuses before storage is touched, and never repeats any of the value in a refusal.
pub(super) fn structural(method: &SignInMethod, value: &str) -> Result<String> {
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
pub(super) fn chatgpt_login(value: &str) -> bool {
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
