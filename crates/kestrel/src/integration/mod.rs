pub mod credential;
pub mod github;
pub mod hook;
pub mod manifest;
pub mod post;
pub mod rotation;
pub mod webhook;

pub use rotation::{resume_webhook_secret_rotation, rotate_webhook_secret};

mod maintenance;
mod polling;
mod refusal;
mod repository;
mod retry;

pub use maintenance::{
    Change, LEFT_UNSAID, change, disable, enable, hook_maintainable, replace_private_key, retire,
};
pub use polling::{Polled, poll};
pub use refusal::{out_of_use, revised};
pub use repository::{
    Lent, REPOSITORIES_PER_PAGE, Reached, lend, lending, repositories, unreached,
};

use retry::back_off;

use anyhow::{Context as _, Result, bail};
use jiff::SignedDuration;

use crate::declined::Declined;
use crate::domain::{Connection, Direction, Event, EventRecordId, GithubConnection, Integration};
use crate::store::Store;

use self::credential::App;
use self::github::Github;

pub struct Registration<'a> {
    pub organization: &'a str,
    pub name: &'a str,
    pub carries: &'a [Direction],
    pub connecting: Connecting<'a>,
}

pub enum Connecting<'a> {
    /// Polled either way; a `signing_secret` also records a Delivery that reaches the webhook.
    Github {
        repository: &'a str,
        api: &'a str,
        app_id: i64,
        installation: i64,
        private_key: &'a str,
        interval: SignedDuration,
        signing_secret: Option<&'a str>,
    },
    Webhook {
        secret: &'a str,
    },
}

pub async fn register(
    store: &Store,
    github: &Github,
    registration: Registration<'_>,
) -> Result<Integration> {
    if registration.carries.is_empty() {
        bail!(Declined::Unacceptable(
            "an integration carries something: name a direction it carries".to_owned()
        ));
    }

    let (connection, webhook_secret) = match registration.connecting {
        Connecting::Github {
            repository,
            api,
            app_id,
            installation,
            private_key,
            interval,
            signing_secret,
        } => {
            if interval <= SignedDuration::ZERO {
                bail!(Declined::Unacceptable(
                    "a poll interval is how long kestrel waits, and cannot be zero or negative"
                        .to_owned()
                ));
            }
            let repository = github::repository(repository)?;
            let credential = App::held(app_id, installation, private_key);
            let bot_login = github
                .app_bot_login(api, &credential)
                .await
                .context("learning the app's own identity")?;
            let repository_id = github
                .repository_id(api, &credential, &repository)
                .await
                .context("learning the repository's id")?;
            (
                Connection::Github(GithubConnection {
                    repository,
                    api: api.to_owned(),
                    credential,
                    bot_login,
                    interval,
                    repository_id,
                }),
                signing_secret,
            )
        }
        Connecting::Webhook { secret } => {
            if registration.carries.contains(&Direction::Outbound) {
                bail!(Declined::Unacceptable(
                    "a generic webhook carries events inbound only".to_owned()
                ));
            }
            (Connection::Webhook, Some(secret))
        }
    };
    if webhook_secret.is_some_and(str::is_empty) {
        bail!(Declined::Unacceptable(
            "a webhook secret with nothing in it is not one".to_owned()
        ));
    }

    let mut tx = store.begin().await?;
    let organization = tx.organizations().named(registration.organization).await?;
    let integration = tx
        .integrations()
        .register(
            &organization,
            registration.name,
            connection,
            registration.carries,
            webhook_secret,
        )
        .await?;
    tx.commit().await?;

    Ok(integration)
}

pub async fn integration(store: &Store, organization: &str, name: &str) -> Result<Integration> {
    let mut tx = store.read().await?;
    let organization = tx.organizations().named(organization).await?;

    tx.integrations().named(&organization, name).await
}

pub async fn integrations(store: &Store, organization: &str) -> Result<Vec<Integration>> {
    let mut tx = store.begin().await?;
    let organization = tx.organizations().named(organization).await?;

    tx.integrations().all(&organization).await
}

pub async fn acknowledge_event_refusal(
    store: &Store,
    organization: &str,
    name: &str,
) -> Result<()> {
    let mut tx = store.begin().await?;
    let organization = tx.organizations().named(organization).await?;
    let integration = tx.integrations().named(&organization, name).await?;
    tx.integrations()
        .acknowledge_event_refusal(&integration)
        .await?;
    tx.commit().await
}

pub async fn events(store: &Store, organization: &str, limit: usize) -> Result<Vec<Event>> {
    let mut tx = store.begin().await?;
    let organization = tx.organizations().named(organization).await?;

    tx.integrations().events(&organization, limit).await
}

pub async fn event(store: &Store, id: EventRecordId) -> Result<Event> {
    store.begin().await?.integrations().event(id).await
}

#[cfg(test)]
mod tests;
