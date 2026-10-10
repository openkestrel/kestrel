//! What a Session's harness is handed at its spawn, and what a failure, a working Turn or a
//! refreshed login may then say about the revision handed over and no other.

use std::collections::BTreeMap;

use anyhow::{Result, bail};
use kestrel_operator_types::{SignInMethod, SignInMethodOwnership};
use tracing::info;

use super::{Source, State, UseResult, filled};
use crate::catalogue;
use crate::declined::{Constraint, Next, Reason, Resource};
use crate::domain::{Organization, Session, SubscriptionProfile, Supplied};
use crate::profile::{Contents, Entry, Kind};
use crate::store::{Store, Tx};

/// Refuses a method the harness does not offer, or one whose material nothing this work would
/// be handed holds; the Operator's Profile never stands in for the one a Workspace names.
pub(crate) async fn selected(
    tx: &mut Tx<'_>,
    operation: &'static str,
    organization: &Organization,
    harness: &str,
    profile: Option<&SubscriptionProfile>,
    method: &str,
) -> Result<&'static SignInMethod> {
    let offered = catalogue::sign_in_method(operation, harness, method)
        .map_err(|error| concerning_the_method(error, harness))?;
    let entry = filled(offered)?;

    let (resource, reference, held) = match offered.ownership {
        SignInMethodOwnership::Organization => (
            Resource::ProviderCredential,
            entry.name.clone(),
            tx.organizations()
                .provider_credential_revision(organization.id, &entry.name)
                .await?
                .is_some(),
        ),
        SignInMethodOwnership::Operator => {
            let Some(profile) = profile else {
                bail!(Reason::InvalidField {
                    field: "sign_in_method",
                    operation,
                    constraint: Constraint::RequiresProfile,
                    allowed: None,
                    message: format!(
                        "{} is held in a Subscription Profile, and this work names none",
                        offered.name
                    ),
                });
            };
            (
                Resource::SubscriptionProfile,
                profile.name.clone(),
                tx.profiles().revision_of(profile, &entry).await?.is_some(),
            )
        }
    };
    if !held {
        bail!(Reason::StateConflict {
            operation,
            resource,
            reference: reference.clone(),
            organization: Some(organization.name.clone()),
            state: "sign_in_not_saved",
            holding_session: None,
            next: Next::inspect(resource, reference.clone()),
            message: format!(
                "no {} sign-in for {harness} is saved in the {} {reference}",
                offered.name,
                resource.noun()
            ),
        });
    }

    Ok(offered)
}

fn concerning_the_method(error: anyhow::Error, harness: &str) -> anyhow::Error {
    match error.downcast::<Reason>() {
        Ok(Reason::InvalidField {
            operation,
            constraint,
            allowed,
            field,
            message,
        }) => Reason::InvalidField {
            field: "sign_in_method",
            operation,
            constraint,
            // A harness outside the catalogue offers no method to choose instead.
            allowed: allowed.filter(|_| field == "method"),
            message: match field {
                "method" => message,
                _ => format!("the harness {harness} has no guided Sign-in Methods to select"),
            },
        }
        .into(),
        Ok(reason) => reason.into(),
        Err(error) => error,
    }
}

/// The catalogued method of `harness` an entry fills, if it is one of its authentication slots.
pub(crate) fn slot(harness: &str, entry: &Entry) -> Option<&'static SignInMethod> {
    catalogue::harnesses()
        .iter()
        .filter(|row| row.name == harness)
        .flat_map(|row| &row.sign_in_methods)
        .find(|method| filled(method).is_ok_and(|fills| fills == *entry))
}

/// Decrypts what the Session's harness is spawned with and records the revision of each value in
/// the same transaction, so material replaced since the enqueue is attributed as supplied.
pub async fn hand_off(store: &Store, session: &Session) -> Result<Contents> {
    let mut tx = store.begin().await?;
    let workspace = tx.workspaces().get(session.workspace).await?;
    let harness = &session.agent.harness;
    let chosen = session
        .sign_in_method
        .as_deref()
        .and_then(|method| catalogue::sign_in_method("hand_off", harness, method).ok());
    // A selected method leaves the harness nothing else to authenticate with, so a refusal
    // cannot be answered by another credential working.
    let competes = |entry: &Entry| {
        chosen.is_some_and(|chosen| slot(harness, entry).is_some_and(|slot| slot.id != chosen.id))
    };

    let mut handed = Contents::default();
    let mut supplied: BTreeMap<(Kind, String), (Option<&SubscriptionProfile>, i64)> =
        BTreeMap::new();

    let mut credentials = tx
        .organizations()
        .provider_credentials(session.organization)
        .await?;
    for held in tx
        .organizations()
        .provider_credentials_held(session.organization)
        .await?
    {
        let entry = Entry {
            kind: Kind::Variable,
            name: held.variable,
        };
        if competes(&entry) {
            continue;
        }
        if let Some(secret) = credentials.remove(&entry.name) {
            handed.variables.insert(entry.name.clone(), secret);
            supplied.insert((entry.kind, entry.name), (None, held.revision));
        }
    }

    if let Some(profile) = &workspace.profile {
        let mut contents = tx.profiles().contents(profile).await?;
        for held in tx.profiles().held(profile).await? {
            if competes(&held.entry) {
                continue;
            }
            let (kept, into) = match held.entry.kind {
                Kind::Variable => (&mut contents.variables, &mut handed.variables),
                Kind::File => (&mut contents.files, &mut handed.files),
            };
            if let Some(secret) = kept.remove(&held.entry.name) {
                into.insert(held.entry.name.clone(), secret);
                supplied.insert(
                    (held.entry.kind, held.entry.name),
                    (Some(profile), held.revision),
                );
            }
        }
    }

    tx.sign_ins().record_supplied(session, &supplied).await?;
    tx.commit().await?;

    info!(
        session = %session.id,
        sign_in_method = session.sign_in_method.as_deref().unwrap_or("none selected"),
        variables = handed.variables.keys().cloned().collect::<Vec<_>>().join(", "),
        files = handed.files.keys().cloned().collect::<Vec<_>>().join(", "),
        "an instance took the credentials its session needs"
    );

    Ok(handed)
}

/// Writes back only a file still at the revision this Session was handed, and moves the
/// Session's handoff on to what it wrote so its next refresh is not read as stale.
pub async fn refresh(
    store: &Store,
    session: &Session,
    profile: &SubscriptionProfile,
    files: &BTreeMap<String, String>,
) -> Result<Vec<String>> {
    let mut tx = store.begin().await?;
    let mut refreshed = Vec::new();

    for (path, contents) in files {
        let entry = Entry {
            kind: Kind::File,
            name: path.clone(),
        };
        let Some(handed) = tx.sign_ins().supplied_revision(session, &entry).await? else {
            continue;
        };
        match tx
            .profiles()
            .refresh_file(profile, &entry, handed, contents)
            .await?
        {
            Some(revision) => {
                tx.sign_ins()
                    .advance_supplied(session, &entry, revision)
                    .await?;
                refreshed.push(path.clone());
            }
            None => info!(
                session = %session.id,
                profile = %profile.name,
                file = %path,
                handed,
                "a refreshed login was not kept: newer material replaced the revision it began from"
            ),
        }
    }
    tx.commit().await?;

    Ok(refreshed)
}

/// Which sign-in a Session's evidence is about.
#[derive(Debug, Clone)]
pub enum Attribution {
    /// The method selected, or the only catalogued login its harness was handed.
    Established(&'static SignInMethod, Supplied),
    /// Several logins the harness could have authenticated with, and nothing says which it did.
    Uncertain(Vec<(&'static SignInMethod, Supplied)>),
    Unattributed,
}

pub fn attribution(session: &Session) -> Attribution {
    let harness = &session.agent.harness;
    let mut candidates: Vec<(&'static SignInMethod, Supplied)> = session
        .supplied
        .iter()
        .filter_map(|supplied| {
            let entry = Entry {
                kind: supplied.kind,
                name: supplied.name.clone(),
            };
            slot(harness, &entry).map(|method| (method, supplied.clone()))
        })
        .collect();
    candidates.sort_by(|(a, _), (b, _)| a.id.cmp(&b.id));

    match (&session.sign_in_method, candidates.len()) {
        (Some(selected), _) => candidates
            .into_iter()
            .find(|(method, _)| method.id == *selected)
            .map_or(Attribution::Unattributed, |(method, supplied)| {
                Attribution::Established(method, supplied)
            }),
        (None, 0) => Attribution::Unattributed,
        (None, 1) => {
            let (method, supplied) = candidates.remove(0);
            Attribution::Established(method, supplied)
        }
        (None, _) => Attribution::Uncertain(candidates),
    }
}

/// The revision evidence may be written against: one sign-in, and still what is held.
fn evidenced(session: &Session) -> Option<i64> {
    match attribution(session) {
        Attribution::Established(_, supplied) if supplied.current => Some(supplied.revision),
        _ => None,
    }
}

fn model(session: &Session) -> Option<&str> {
    session
        .worked_model
        .as_deref()
        .or(session.agent.declared.model.as_deref())
}

pub(crate) async fn refused(tx: &mut Tx<'_>, session: &Session) -> Result<()> {
    let Some(revision) = evidenced(session) else {
        return Ok(());
    };
    tx.sign_ins()
        .observe(revision, State::AuthenticationFailed, Source::Session)
        .await?;
    if let Some(model) = model(session) {
        tx.sign_ins()
            .record_use(
                revision,
                &session.agent.harness,
                model,
                UseResult::AuthenticationFailed,
            )
            .await?;
    }

    Ok(())
}

pub(crate) async fn worked(tx: &mut Tx<'_>, session: &Session) -> Result<()> {
    let (Some(revision), Some(model)) = (evidenced(session), model(session)) else {
        return Ok(());
    };
    tx.sign_ins().withdraw_session_failure(revision).await?;
    tx.sign_ins()
        .record_use(revision, &session.agent.harness, model, UseResult::Worked)
        .await
}
