use std::net::{SocketAddr, TcpListener as StdListener};
use std::time::Duration;

use anyhow::{Context as _, Result};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};
use url::Url;

use crate::capability::{Capability, Images};
use crate::cli::Role;
use crate::integration::webhook;
use crate::sign_in::check::Providers;
use crate::store::Store;
use crate::timer::Wake;
use crate::{link, operator};

/// Two listeners rather than one, so exposing the operator boundary never exposes the link
/// (ADR-0015).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Listen {
    pub link: SocketAddr,
    pub operator: SocketAddr,
}

/// Two sockets bound before a role serves them, so both ports stay held across whatever stops
/// serving and nothing else can take them.
pub struct Listeners {
    link: StdListener,
    operator: StdListener,
}

impl Listeners {
    pub fn bind(listen: Listen) -> Result<Self> {
        let link = StdListener::bind(listen.link)
            .with_context(|| format!("listening for the link on {}", listen.link))?;
        let operator = StdListener::bind(listen.operator)
            .with_context(|| format!("listening for operators on {}", listen.operator))?;
        link.set_nonblocking(true)
            .context("the link's socket should not block")?;
        operator
            .set_nonblocking(true)
            .context("the operator's socket should not block")?;

        Ok(Self { link, operator })
    }

    pub fn bound(&self) -> Result<Listen> {
        Ok(Listen {
            link: self.link.local_addr()?,
            operator: self.operator.local_addr()?,
        })
    }

    /// Another handle on the same sockets, so serving them releases neither port when it stops.
    pub fn try_clone(&self) -> Result<Self> {
        Ok(Self {
            link: self
                .link
                .try_clone()
                .context("the link's socket should clone")?,
            operator: self
                .operator
                .try_clone()
                .context("the operator's socket should clone")?,
        })
    }

    /// A handle for a role to serve, leaving these bound so neither port is released.
    pub fn serving(&self) -> Result<(TcpListener, TcpListener)> {
        let listeners = self.try_clone()?;
        Ok((
            TcpListener::from_std(listeners.link).context("the link's socket should serve")?,
            TcpListener::from_std(listeners.operator)
                .context("the operator's socket should serve")?,
        ))
    }
}

pub struct Listening {
    listeners: Listeners,
    bound: Listen,
    client: Url,
    store: Store,
    wake: Wake,
    follow_lease: Duration,
    providers: Providers,
    github_app_api: String,
    webhook_base: Option<String>,
    images: Images,
    pub(crate) live: crate::live::Live,
}

impl Listening {
    /// Serves sockets the caller already bound, so a restart comes back on the address everything
    /// already reached it at.
    pub fn on(
        store: Store,
        listeners: Listeners,
        client: Url,
        wake: Wake,
        follow_lease: Duration,
    ) -> Result<Self> {
        let bound = listeners.bound()?;
        let live = crate::live::Live::on(store.clock().clone());

        Ok(Self {
            listeners,
            bound,
            client,
            store,
            wake,
            follow_lease,
            providers: Providers::default(),
            github_app_api: crate::integration::github::API.into(),
            webhook_base: None,
            images: Images::default(),
            live,
        })
    }

    #[must_use]
    pub fn with_webhook_base(mut self, base: Option<String>) -> Self {
        self.webhook_base = base;
        self
    }

    pub fn bound(&self) -> Listen {
        self.bound
    }

    #[must_use]
    pub fn checking_sign_ins_with(mut self, providers: Providers) -> Self {
        self.providers = providers;
        self
    }

    #[must_use]
    pub fn creating_github_apps_at(mut self, api: &str) -> Self {
        self.github_app_api = api.into();
        self
    }

    #[must_use]
    pub fn inspecting(mut self, images: Images) -> Self {
        self.images = images;
        self
    }
}

/// Binding before the role starts is what lets a caller that asked for port 0 learn which
/// port it got.
pub async fn bind(
    store: Store,
    listen: Listen,
    client: Url,
    wake: Wake,
    follow_lease: Duration,
) -> Result<Listening> {
    Listening::on(store, Listeners::bind(listen)?, client, wake, follow_lease)
}

pub async fn run(listening: Listening, shutdown: CancellationToken) -> Result<()> {
    let Listening {
        listeners,
        bound,
        client,
        store,
        wake,
        follow_lease,
        providers,
        github_app_api,
        webhook_base,
        images,
        live,
    } = listening;
    let (link_listener, operator_listener) = listeners.serving()?;

    info!(role = %Role::Serve, link = %bound.link, operator = %bound.operator, "role started");
    if !bound.operator.ip().is_loopback() {
        warn!(
            operator = %bound.operator,
            "the operator boundary authenticates nobody, and is listening beyond loopback"
        );
    }

    tokio::spawn(said_at_startup(images.clone()));
    let stop_recording = CancellationToken::new();
    let recording = tokio::spawn({
        let unrecorded = live.unrecorded.clone();
        let store = store.clone();
        let stop = stop_recording.clone();
        async move { unrecorded.record(&store, stop).await }
    });
    let followers = crate::presence::Followers::new(follow_lease, store.clock().clone());
    let link_router = link::router(
        store.clone(),
        crate::integration::github::Github::dialling_out(&store)?,
        shutdown.clone(),
        live.clone(),
    )
    .merge(webhook::router(store.clone(), wake));
    let streams = crate::stream::Streams::new(follow_lease);
    let operator_router = operator::router(
        store,
        shutdown.clone(),
        live,
        followers,
        streams,
        images,
        client,
        providers,
        github_app_api,
        webhook_base,
    );

    let serving_link = axum::serve(link_listener, link_router)
        .with_graceful_shutdown(shutdown.clone().cancelled_owned());
    let serving_operators = axum::serve(operator_listener, operator_router)
        .with_graceful_shutdown(shutdown.cancelled_owned());
    let (link_served, operators_served) =
        tokio::join!(serving_link.into_future(), serving_operators.into_future());
    stop_recording.cancel();
    recording.await.context("recording work reports")?;
    link_served.context("serving the link and the webhooks")?;
    operators_served.context("serving operators")?;
    info!(role = %Role::Serve, "role stopped");

    Ok(())
}

/// Said once, so an image that cannot be inspected is in the log before anyone asks; the control
/// plane serves either way.
async fn said_at_startup(images: Images) {
    match images.read().await {
        Capability::Unchecked => {}
        Capability::Inspected {
            image,
            identity,
            harnesses,
        } => info!(%image, %identity, ?harnesses, "the image declares its harnesses"),
        Capability::Unavailable { image, cause } => warn!(
            %image,
            "{}",
            crate::capability::unavailable(&image, &cause, None)
        ),
    }
}

pub(crate) const BUSY_RETRY_AFTER_SECONDS: i64 = 1;

/// A busy refusal says when to ask again, so a caller can tell it from one that asking again
/// will not fix.
pub(crate) fn refusal(status: StatusCode, busy: bool, body: impl IntoResponse) -> Response {
    if busy {
        (
            status,
            [(header::RETRY_AFTER, BUSY_RETRY_AFTER_SECONDS.to_string())],
            body,
        )
            .into_response()
    } else {
        (status, body).into_response()
    }
}
