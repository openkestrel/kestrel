use jiff::SignedDuration;
use kestrel::domain::{Direction, Event, EventRecordId, Integration};
use kestrel::integration::github::Github;
use kestrel::integration::{self, Connecting, Registration};

use super::super::{APP_ID, Consideration, INSTALLATION_ID, Kestrel, PRIVATE_KEY};

impl Kestrel {
    pub async fn register_integration(
        &self,
        organization: &str,
        name: &str,
        repository: &str,
        api: &str,
        carries: &[Direction],
        interval: SignedDuration,
    ) -> Integration {
        self.try_register_integration(organization, name, repository, api, carries, interval)
            .await
            .expect("the integration should register")
    }

    pub async fn try_register_integration(
        &self,
        organization: &str,
        name: &str,
        repository: &str,
        api: &str,
        carries: &[Direction],
        interval: SignedDuration,
    ) -> anyhow::Result<Integration> {
        integration::register(
            &self.store,
            &Github::dialling_out(&self.store)?,
            Registration {
                organization,
                name,
                carries,
                connecting: Connecting::Github {
                    repository,
                    api,
                    app_id: APP_ID,
                    installation: INSTALLATION_ID,
                    private_key: PRIVATE_KEY,
                    interval,
                    signing_secret: None,
                },
            },
        )
        .await
    }

    pub async fn register_signed_github(
        &self,
        organization: &str,
        name: &str,
        repository: &str,
        api: &str,
        signing_secret: &str,
    ) -> Integration {
        integration::register(
            &self.store,
            &Github::dialling_out(&self.store).expect("a github client"),
            Registration {
                organization,
                name,
                carries: &[Direction::Inbound, Direction::Outbound],
                connecting: Connecting::Github {
                    repository,
                    api,
                    app_id: APP_ID,
                    installation: INSTALLATION_ID,
                    private_key: PRIVATE_KEY,
                    interval: SignedDuration::from_millis(1),
                    signing_secret: Some(signing_secret),
                },
            },
        )
        .await
        .expect("the integration should register")
    }

    pub async fn register_webhook(
        &self,
        organization: &str,
        name: &str,
        secret: &str,
    ) -> Integration {
        integration::register(
            &self.store,
            &Github::dialling_out(&self.store).expect("a github client"),
            Registration {
                organization,
                name,
                carries: &[Direction::Inbound],
                connecting: Connecting::Webhook { secret },
            },
        )
        .await
        .expect("the webhook should register")
    }

    pub async fn integration(&self, organization: &str, name: &str) -> Integration {
        integration::integration(&self.store, organization, name)
            .await
            .expect("the integration should read")
    }

    pub async fn change_integration(
        &self,
        organization: &str,
        name: &str,
        change: integration::Change<'_>,
    ) -> anyhow::Result<Integration> {
        integration::change(&self.store, organization, name, change).await
    }

    pub async fn retire_integration(&self, organization: &str, name: &str) -> Integration {
        integration::retire(&self.store, organization, name, None)
            .await
            .expect("the integration should retire")
    }

    pub async fn try_enable_integration(
        &self,
        organization: &str,
        name: &str,
    ) -> anyhow::Result<Integration> {
        integration::enable(&self.store, organization, name).await
    }

    pub async fn authenticates_webhooks(&self, integration: &Integration) -> bool {
        let mut tx = self.store.read().await.expect("a read");
        tx.integrations()
            .verifier(integration)
            .await
            .expect("how it is verified should read")
            .is_some()
    }

    pub async fn replace_private_key(
        &self,
        organization: &str,
        name: &str,
        private_key: &str,
        revision: Option<i64>,
    ) -> anyhow::Result<Integration> {
        integration::replace_private_key(
            &self.store,
            &Github::unfenced()?,
            organization,
            name,
            private_key,
            revision,
        )
        .await
    }

    /// One client, so what it cached before a change is there to be refused after it.
    pub fn github(&self) -> Github {
        Github::dialling_out(&self.store).expect("a github client")
    }

    pub async fn disable_integration(&self, organization: &str, name: &str) -> Integration {
        integration::disable(&self.store, organization, name)
            .await
            .expect("the integration should disable")
    }

    pub async fn enable_integration(&self, organization: &str, name: &str) -> Integration {
        integration::enable(&self.store, organization, name)
            .await
            .expect("the integration should enable")
    }

    /// Polls with what was read of the Integration, however long ago that was.
    pub async fn poll_as(&self, integration: &Integration) -> integration::Polled {
        integration::poll(
            &self.store,
            &Github::dialling_out(&self.store).expect("a github client"),
            integration,
        )
        .await
        .expect("the poll should finish")
    }

    pub async fn integrations(&self, organization: &str) -> Vec<Integration> {
        integration::integrations(&self.store, organization)
            .await
            .expect("the integrations should list")
    }

    pub async fn acknowledge_event_refusal(&self, organization: &str, name: &str) {
        integration::acknowledge_event_refusal(&self.store, organization, name)
            .await
            .expect("the event refusal should be acknowledged");
    }

    pub async fn events(&self, organization: &str) -> Vec<Event> {
        integration::events(&self.store, organization, 100)
            .await
            .expect("the events should list")
    }

    pub async fn consideration(&self, event: EventRecordId) -> Option<Consideration> {
        let mut tx = self.store.read().await.expect("a read transaction");
        let considered = tx
            .pull_requests()
            .considered(event)
            .await
            .expect("the consideration should read")?;

        Some(Consideration {
            outcome: considered.outcome,
            candidates: considered
                .candidates
                .into_iter()
                .map(|(workspace, state)| (workspace, state.as_str().to_owned()))
                .collect(),
        })
    }
}
