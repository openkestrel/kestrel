use std::net::SocketAddr;
use std::time::Duration;

use anyhow::{Context as _, Result};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

use crate::capability::{Capability, Images};
use crate::cli::Role;
use crate::integration::webhook;
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

pub struct Listening {
    link: TcpListener,
    operator: TcpListener,
    bound: Listen,
    store: Store,
    wake: Wake,
    follow_lease: Duration,
    images: Images,
    pub(crate) live: crate::live::Live,
}

impl Listening {
    pub fn bound(&self) -> Listen {
        self.bound
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
    wake: Wake,
    follow_lease: Duration,
) -> Result<Listening> {
    let link = TcpListener::bind(listen.link)
        .await
        .with_context(|| format!("listening for the link on {}", listen.link))?;
    let operator = TcpListener::bind(listen.operator)
        .await
        .with_context(|| format!("listening for operators on {}", listen.operator))?;
    let bound = Listen {
        link: link.local_addr()?,
        operator: operator.local_addr()?,
    };

    Ok(Listening {
        link,
        operator,
        bound,
        store,
        wake,
        follow_lease,
        images: Images::default(),
        live: crate::live::Live::default(),
    })
}

pub async fn run(listening: Listening, shutdown: CancellationToken) -> Result<()> {
    let Listening {
        link: link_listener,
        operator: operator_listener,
        bound,
        store,
        wake,
        follow_lease,
        images,
        live,
    } = listening;

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
    let followers = crate::presence::Followers::new(follow_lease);
    let link_router = link::router(store.clone(), shutdown.clone(), live.clone())
        .merge(webhook::router(store.clone(), wake));
    let streams = crate::stream::Streams::new(follow_lease);
    let operator_router =
        operator::router(store, shutdown.clone(), live, followers, streams, images);

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
