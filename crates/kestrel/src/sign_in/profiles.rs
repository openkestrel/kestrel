use anyhow::{Result, bail};
use kestrel_operator_types::{SignInMethod, SignInMethodOwnership};

use super::SAVE;
use super::filled;
use crate::catalogue;
use crate::declined::{Next, Reason, Resource};
use crate::domain::{Organization, SubscriptionProfile};
use crate::store::Tx;

pub(crate) async fn operator_profiles(
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
pub(super) async fn operator_profile(
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
            let mut holding = holding_a_subscription(tx, &owned, harness).await?;
            if holding.len() == 1 {
                return Ok(holding.pop().map(|(profile, _)| profile));
            }

            bail!(several_operator_profiles(
                SAVE,
                organization,
                &owned,
                format!(
                    "{} owns several Subscription Profiles in {} ({}), and none alone holds a \
                     {harness} subscription to replace; hold it in one by name instead",
                    operator.name,
                    organization.name,
                    names(&owned)
                ),
            ));
        }
    }
}

pub(crate) async fn holding_a_subscription(
    tx: &mut Tx<'_>,
    profiles: &[SubscriptionProfile],
    harness: &str,
) -> Result<Vec<(SubscriptionProfile, Vec<&'static SignInMethod>)>> {
    let subscriptions = catalogue::harnesses()
        .iter()
        .filter(|row| row.name == harness)
        .flat_map(|row| &row.sign_in_methods)
        .filter(|method| method.ownership == SignInMethodOwnership::Operator)
        .map(|method| Ok((method, filled(method)?)))
        .collect::<Result<Vec<_>>>()?;
    let mut holding = Vec::new();
    for profile in profiles {
        let held = tx.profiles().held(profile).await?;
        let methods: Vec<&'static SignInMethod> = subscriptions
            .iter()
            .filter(|(_, entry)| held.iter().any(|held| held.entry == *entry))
            .map(|(method, _)| *method)
            .collect();
        if !methods.is_empty() {
            holding.push((profile.clone(), methods));
        }
    }

    Ok(holding)
}

pub(crate) fn names(profiles: &[SubscriptionProfile]) -> String {
    profiles
        .iter()
        .map(|profile| profile.name.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

pub(crate) fn several_operator_profiles(
    operation: &'static str,
    organization: &Organization,
    among: &[SubscriptionProfile],
    message: String,
) -> Reason {
    Reason::StateConflict {
        operation,
        resource: Resource::SubscriptionProfile,
        reference: among[0].name.clone(),
        organization: Some(organization.name.clone()),
        state: "several_operator_profiles",
        holding_session: None,
        next: Next::inspect(Resource::SubscriptionProfile, among[0].name.clone()),
        message,
    }
}
