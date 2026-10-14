use kestrel::domain::{Organization, SubscriptionProfile};
use kestrel::profile::{self, Contents};
use kestrel::provider::{self, Held};
use kestrel::sign_in::credential_use::{self, CredentialUse};

use super::super::{Kestrel, SERIALIZED};

impl Kestrel {
    /// Declares an Organization of the same name when none exists, as naming does.
    pub async fn name_operator(&self, name: &str) {
        kestrel::operator_identity::name(&self.store, name)
            .await
            .expect("the operator should be named");
    }

    pub async fn hold_provider_credential(
        &self,
        organization: &Organization,
        variable: &str,
        secret: &str,
    ) {
        provider::hold(&self.store, &organization.name, variable, secret)
            .await
            .expect("the provider credential should be held");
    }

    pub async fn provider_credentials_held(&self, organization: &Organization) -> Vec<Held> {
        provider::held(&self.store, &organization.name)
            .await
            .expect("what the organization holds should list")
    }

    pub async fn declare_profile(
        &self,
        organization: &str,
        name: &str,
        owner: &str,
    ) -> anyhow::Result<SubscriptionProfile> {
        profile::declare(&self.store, organization, name, Some(owner))
            .await
            .map(|declared| declared.record)
    }

    pub async fn hold_in_profile(
        &self,
        organization: &str,
        name: &str,
        entry: &profile::Entry,
        login: &str,
    ) {
        profile::hold(&self.store, organization, name, entry, login)
            .await
            .expect("the login should be held");
    }

    pub async fn profiles(
        &self,
        organization: &str,
    ) -> Vec<(SubscriptionProfile, Vec<profile::Held>)> {
        profile::profiles(&self.store, organization)
            .await
            .expect("the profiles should list")
    }

    /// What a dispatching work role records at its start, for a test that claims by hand.
    pub async fn record_serialized_harnesses(&self) {
        let mut tx = self.store.begin().await.expect("a transaction");
        tx.queue()
            .record(1, &[SERIALIZED.to_owned()], "local-exec")
            .await
            .expect("the work role should be recorded");
        tx.commit().await.expect("the record should commit");
    }

    pub async fn acquire_credential_use(
        &self,
        profile: &str,
        holder: &str,
    ) -> anyhow::Result<CredentialUse> {
        let mut tx = self.store.begin().await?;
        let held = credential_use::acquire(&mut tx, "acme", profile, SERIALIZED, holder).await?;
        tx.commit().await?;
        Ok(held)
    }

    pub async fn release_credential_use(&self, held: &CredentialUse) {
        credential_use::release(&self.store, held)
            .await
            .expect("the login should be released");
    }

    /// What the next Session spawned with the profile would be handed.
    pub async fn profile_contents(&self, profile: &SubscriptionProfile) -> Contents {
        profile::contents(&self.store, profile)
            .await
            .expect("the profile should open")
    }
}
