use axum::Json;
use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use jiff::SignedDuration;
use kestrel_operator_types as wire;
use serde::Deserialize;

use crate::declined::Constraint;
use crate::domain::{Connection, Direction, Integration, IntegrationKind, IntegrationState};
use crate::integration::github::{self, Github};
use crate::integration::{self, Connecting, Registration};

use super::{ControlPlane, Refused, diagnosis, invalid_field, named, open, wire_id};

/// Never the private key or a webhook secret: what an Integration presents stays behind the
/// boundary. The bot login is not a secret — it is the name GitHub already shows in public.
pub(super) fn integration_record(
    integration: Integration,
    organization: &str,
) -> wire::Integration {
    let diagnostic = (!integration.enabled()).then(|| {
        diagnosis(
            integration::out_of_use(&integration, organization, "use_integration"),
            None,
        )
        .1
    });
    let webhook_path = integration.webhook_path();
    let kind = match integration.kind() {
        IntegrationKind::Github => wire::IntegrationKind::Github,
        IntegrationKind::Webhook => wire::IntegrationKind::Webhook,
    };
    let received = integration.carries(Direction::Inbound) && !integration.retired();
    let (repository, bot_login, polled_every, webhook_path) = match integration.connection {
        Connection::Github(github) if !received => {
            (Some(github.repository), Some(github.bot_login), None, None)
        }
        Connection::Github(github) => (
            Some(github.repository),
            Some(github.bot_login),
            Some(format!("{:#}", github.interval)),
            Some(webhook_path),
        ),
        Connection::Webhook => (None, None, None, received.then_some(webhook_path)),
    };

    wire::Integration {
        id: wire_id(&integration.id),
        name: integration.name,
        kind,
        repository,
        bot_login,
        carries: integration
            .carries
            .into_iter()
            .map(wire_direction)
            .collect(),
        state: match integration.state {
            IntegrationState::Enabled => wire::IntegrationState::Enabled,
            IntegrationState::Disabled => wire::IntegrationState::Disabled,
            IntegrationState::Retired => wire::IntegrationState::Retired,
        },
        revision: integration.revision,
        disabled_at: integration.disabled_at.map(|at| at.to_string()),
        retired_at: integration.retired_at.map(|at| at.to_string()),
        canceled_posts: integration
            .canceled_posts
            .into_iter()
            .map(|post| wire::CanceledPost {
                session: wire_id(&post.session),
                turn: post.turn,
                canceled_at: post.canceled_at.to_string(),
                reason: post.because,
                attempted_at: post.attempted_at.map(|at| at.to_string()),
            })
            .collect(),
        polled_every,
        webhook_path,
        last_event_refusal: integration
            .last_event_refusal
            .map(|refusal| wire::EventRefusal {
                source: refusal.source,
                id: refusal.id,
                bytes: refusal
                    .bytes
                    .map(|bytes| i64::try_from(bytes).unwrap_or(i64::MAX)),
                reason: refusal.reason,
                observed_at: refusal.observed_at.to_string(),
            }),
        diagnostic,
    }
}

pub(super) const fn wire_direction(direction: Direction) -> wire::Direction {
    match direction {
        Direction::Inbound => wire::Direction::Inbound,
        Direction::Outbound => wire::Direction::Outbound,
    }
}

pub(super) const fn domain_direction(direction: &wire::Direction) -> Direction {
    match direction {
        wire::Direction::Inbound => Direction::Inbound,
        wire::Direction::Outbound => Direction::Outbound,
    }
}

pub(super) async fn integrations(
    State(control_plane): State<ControlPlane>,
    Path(organization): Path<String>,
) -> Result<Json<Vec<wire::Integration>>, Refused> {
    let integrations = integration::integrations(&control_plane.store, &organization).await?;

    Ok(Json(
        integrations
            .into_iter()
            .map(|integration| integration_record(integration, &organization))
            .collect(),
    ))
}

pub(super) async fn show_integration(
    State(control_plane): State<ControlPlane>,
    Path((organization, name)): Path<(String, String)>,
) -> Result<Json<wire::Integration>, Refused> {
    let integration = integration::integration(&control_plane.store, &organization, &name).await?;

    Ok(Json(integration_record(integration, &organization)))
}

/// What identifies the connection; naming one in a change is refused rather than ignored.
pub(super) const FIXED: [&str; 7] = [
    "organization",
    "kind",
    "api",
    "repository",
    "app_id",
    "installation",
    "id",
];

pub(super) async fn change_integration(
    State(control_plane): State<ControlPlane>,
    Path((organization, name)): Path<(String, String)>,
    change: Result<Json<serde_json::Value>, JsonRejection>,
) -> Result<Json<wire::Integration>, Refused> {
    let Json(change) = change?;
    if let Some(fixed) = FIXED.into_iter().find(|field| change.get(field).is_some()) {
        return Err(invalid_field(
            "change_integration",
            fixed,
            Constraint::Immutable,
            format!(
                "an integration's {fixed} identifies it and cannot change; register a new \
                 integration for a different one"
            ),
        ));
    }
    let change: wire::IntegrationChange = open(&change)?;
    if let Some(name) = &change.name {
        named("change_integration", name)?;
    }
    let interval = change
        .interval
        .as_deref()
        .map(str::parse::<SignedDuration>)
        .transpose()
        .map_err(|error| {
            invalid_field(
                "change_integration",
                "interval",
                Constraint::Positive,
                format!("an interval is a duration: {error}"),
            )
        })?;
    let carries: Option<Vec<Direction>> = change
        .carries
        .as_ref()
        .map(|carries| carries.iter().map(domain_direction).collect());
    let changed = integration::change(
        &control_plane.store,
        &organization,
        &name,
        integration::Change {
            name: change.name.as_deref(),
            carries: carries.as_deref(),
            interval,
            revision: change.revision,
        },
    )
    .await?;

    Ok(Json(integration_record(changed, &organization)))
}

#[derive(Deserialize)]
pub(super) struct RepositoriesPage {
    page: Option<u32>,
    per_page: Option<u32>,
}

pub(super) async fn integration_repositories(
    State(control_plane): State<ControlPlane>,
    Path((organization, name)): Path<(String, String)>,
    Query(asked): Query<RepositoriesPage>,
) -> Result<Json<wire::IntegrationRepositories>, Refused> {
    let reached = integration::repositories(
        &control_plane.store,
        &Github::unfenced()?,
        &organization,
        &name,
        asked.page,
        asked.per_page,
    )
    .await?;

    Ok(Json(wire::IntegrationRepositories {
        next_page: reached.next_page.map(i64::from),
        page: i64::from(reached.page),
        total: i64::try_from(reached.installed.total_count).unwrap_or(i64::MAX),
        repositories: reached
            .installed
            .repositories
            .into_iter()
            .map(|repository| wire::IntegrationRepository {
                address: github::https_address(&repository.full_name),
                watched: repository.full_name.eq_ignore_ascii_case(&reached.watched),
                repository: repository.full_name,
                default_branch: repository.default_branch,
                private: repository.private,
            })
            .collect(),
    }))
}

pub(super) async fn disable_integration(
    State(control_plane): State<ControlPlane>,
    Path((organization, name)): Path<(String, String)>,
) -> Result<Json<wire::Integration>, Refused> {
    let disabled = integration::disable(&control_plane.store, &organization, &name).await?;

    Ok(Json(integration_record(disabled, &organization)))
}

pub(super) async fn enable_integration(
    State(control_plane): State<ControlPlane>,
    Path((organization, name)): Path<(String, String)>,
) -> Result<Json<wire::Integration>, Refused> {
    let enabled = integration::enable(&control_plane.store, &organization, &name).await?;

    Ok(Json(integration_record(enabled, &organization)))
}

pub(super) async fn retire_integration(
    State(control_plane): State<ControlPlane>,
    Path((organization, name)): Path<(String, String)>,
    retirement: Result<Json<wire::IntegrationRetirement>, JsonRejection>,
) -> Result<Json<wire::Integration>, Refused> {
    let Json(retirement) = retirement?;
    let retired = integration::retire(
        &control_plane.store,
        &organization,
        &name,
        retirement.revision,
    )
    .await?;

    Ok(Json(integration_record(retired, &organization)))
}

pub(super) async fn replace_integration_private_key(
    State(control_plane): State<ControlPlane>,
    Path((organization, name)): Path<(String, String)>,
    replacement: Result<Json<wire::IntegrationPrivateKeyReplacement>, JsonRejection>,
) -> Result<Json<wire::Integration>, Refused> {
    let Json(replacement) = replacement?;
    let replaced = integration::replace_private_key(
        &control_plane.store,
        &Github::unfenced()?,
        &organization,
        &name,
        &replacement.private_key,
        replacement.revision,
    )
    .await?;

    Ok(Json(integration_record(replaced, &organization)))
}

pub(super) fn registration_name(registration: &wire::IntegrationRegistration) -> &str {
    match registration {
        wire::IntegrationRegistration::GithubRegistration(github) => &github.name,
        wire::IntegrationRegistration::WebhookRegistration(webhook) => &webhook.name,
    }
}

pub(super) fn registration_connecting(
    registration: &wire::IntegrationRegistration,
) -> Result<(&[wire::Direction], Connecting<'_>), Refused> {
    match registration {
        wire::IntegrationRegistration::GithubRegistration(github) => {
            let interval = github
                .interval
                .as_ref()
                .and_then(|interval| interval.as_deref());
            Ok((
                github
                    .carries
                    .as_deref()
                    .unwrap_or(&[wire::Direction::Inbound, wire::Direction::Outbound]),
                Connecting::Github {
                    repository: &github.repository,
                    api: github
                        .api
                        .as_ref()
                        .and_then(|api| api.as_deref())
                        .unwrap_or(github::API),
                    app_id: github.app_id,
                    installation: github.installation,
                    private_key: &github.private_key,
                    interval: interval
                        .map_or(Ok(SignedDuration::from_mins(1)), str::parse)
                        .map_err(|error| {
                            Refused::Unprocessable(format!("an interval is a duration: {error}"))
                        })?,
                    signing_secret: github
                        .webhook_secret
                        .as_ref()
                        .and_then(|secret| secret.as_deref()),
                },
            ))
        }
        wire::IntegrationRegistration::WebhookRegistration(webhook) => Ok((
            webhook
                .carries
                .as_deref()
                .unwrap_or(&[wire::Direction::Inbound]),
            Connecting::Webhook {
                secret: &webhook.secret,
            },
        )),
    }
}

pub(super) async fn register_integration(
    State(control_plane): State<ControlPlane>,
    Path(organization): Path<String>,
    registration: Result<Json<wire::IntegrationRegistration>, JsonRejection>,
) -> Result<(StatusCode, Json<wire::Integration>), Refused> {
    let Json(registration) = registration?;
    named("register_integration", registration_name(&registration))?;
    let (carries, connecting) = registration_connecting(&registration)?;
    let carries: Vec<Direction> = carries.iter().map(domain_direction).collect();
    let github = Github::unfenced()?;
    let registered = integration::register(
        &control_plane.store,
        &github,
        Registration {
            organization: &organization,
            name: registration_name(&registration),
            carries: &carries,
            connecting,
        },
    )
    .await?;

    Ok((
        StatusCode::CREATED,
        Json(integration_record(registered, &organization)),
    ))
}

pub(super) async fn acknowledge_event_refusal(
    State(control_plane): State<ControlPlane>,
    Path((organization, integration)): Path<(String, String)>,
) -> Result<StatusCode, Refused> {
    integration::acknowledge_event_refusal(&control_plane.store, &organization, &integration)
        .await?;

    Ok(StatusCode::NO_CONTENT)
}
