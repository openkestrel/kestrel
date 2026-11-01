use anyhow::Result;
use jiff::Timestamp;
use kestrel_operator_types::{SignInFill, SignInMethodOwnership};

use super::filled;
use super::profiles::operator_profiles;
use super::record::{Holding, SignIn};
use crate::catalogue;
use crate::domain::{Organization, SubscriptionProfile};
use crate::store::{Store, Tx};

/// Every catalogued method with material saved, an aliased key once per harness offering it.
pub async fn saved(store: &Store, organization: &str) -> Result<Vec<SignIn>> {
    let mut tx = store.read().await?;
    let organization = tx.organizations().named(organization).await?;
    let profiles = operator_profiles(&mut tx, &organization).await?;

    saved_in(&mut tx, &organization, profiles).await
}

pub(crate) async fn saved_in(
    tx: &mut Tx<'_>,
    organization: &Organization,
    holding: Vec<SubscriptionProfile>,
) -> Result<Vec<SignIn>> {
    let credentials = tx
        .organizations()
        .provider_credentials_held(organization.id)
        .await?;
    let mut profiles = Vec::new();
    for profile in holding {
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
