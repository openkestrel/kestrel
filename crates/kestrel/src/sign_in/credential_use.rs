//! A Profile's login lent to something that is not a Session, under the rule that already
//! serializes Sessions sharing one on a harness that rotates it.

use anyhow::{Result, bail};

use crate::declined::{Next, Reason, Resource};
use crate::domain::SubscriptionProfileId;
use crate::store::Store;
use crate::workspace;

pub const ACQUIRE: &str = "use_sign_in";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CredentialUse {
    pub profile: SubscriptionProfileId,
    pub harness: String,
    pub holder: String,
}

/// Refused while a Session or another holder is using the login, never queued behind it. The
/// caller releases only after it has handed back whatever the harness refreshed.
pub async fn acquire(
    store: &Store,
    organization: &str,
    profile: &str,
    harness: &str,
    holder: &str,
) -> Result<CredentialUse> {
    let mut tx = store.begin().await?;
    let organization = tx.organizations().named(organization).await?;
    let profile = tx.profiles().named(&organization, profile).await?;
    let serialized = tx
        .queue()
        .recorded()
        .await?
        .is_some_and(|recorded| recorded.serialized_harnesses.iter().any(|s| s == harness));

    let busy = |holding: Option<String>, next: Next, by: String| Reason::StateConflict {
        operation: ACQUIRE,
        resource: Resource::SubscriptionProfile,
        reference: profile.name.clone(),
        organization: Some(organization.name.clone()),
        state: "credential_in_use",
        holding_session: holding,
        next,
        message: format!(
            "the {harness} login in the subscription profile {} is in use by {by}",
            profile.name
        ),
    };
    if serialized && let Some(holding) = tx.sign_ins().session_using(&profile, harness).await? {
        let holding = tx.workspaces().session(holding).await?;
        let session = holding.id.to_string();
        bail!(busy(
            Some(session.clone()),
            workspace::stoppable(Next::inspect(Resource::Session, &session), &holding),
            format!("the session {session}"),
        ));
    }
    if let Some(holder) = tx.sign_ins().holder(&profile, harness).await? {
        bail!(busy(
            None,
            Next::inspect(Resource::SubscriptionProfile, profile.name.clone()),
            holder,
        ));
    }

    let held = CredentialUse {
        profile: profile.id,
        harness: harness.to_owned(),
        holder: holder.to_owned(),
    };
    tx.sign_ins().hold(&profile, &held).await?;
    tx.commit().await?;

    Ok(held)
}

pub async fn release(store: &Store, held: &CredentialUse) -> Result<()> {
    let mut tx = store.begin().await?;
    tx.sign_ins().release(held).await?;
    tx.commit().await
}

/// Nothing that held a login outlives the process that was using it.
pub async fn release_all(store: &Store) -> Result<()> {
    let mut tx = store.begin().await?;
    tx.sign_ins().release_all().await?;
    tx.commit().await
}
