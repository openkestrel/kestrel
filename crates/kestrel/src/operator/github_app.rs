use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Form, Json};
use kestrel_operator_types as wire;
use serde::Deserialize;

use crate::integration;
use crate::integration::github::Github;

use super::{ControlPlane, Refused, named};

#[derive(Deserialize)]
pub(super) struct GithubAppQuery {
    state: String,
    code: Option<String>,
    installation_id: Option<i64>,
    page: Option<u32>,
}

pub(super) async fn start_github_app(
    State(control_plane): State<ControlPlane>,
    Path(organization): Path<String>,
    registration: Result<Json<wire::GithubAppStart>, JsonRejection>,
) -> Result<(StatusCode, Json<wire::GithubAppStarted>), Refused> {
    let Json(registration) = registration?;
    named("start_integration_manifest", &registration.name)?;
    Ok((
        StatusCode::CREATED,
        Json(
            integration::manifest::start(
                &control_plane.store,
                &organization,
                registration,
                &control_plane.github_app_api,
            )
            .await?,
        ),
    ))
}

pub(super) fn setup_page(html: String) -> Response {
    const POLICY: &str = "default-src 'none'; form-action 'self' https://github.com; frame-ancestors 'none'; base-uri 'none'";
    let headers = [
        ("cache-control", "no-store"),
        ("referrer-policy", "no-referrer"),
        ("content-security-policy", POLICY),
    ];
    (headers, axum::response::Html(html)).into_response()
}

pub(super) async fn github_app_page(
    State(cp): State<ControlPlane>,
    Query(query): Query<GithubAppQuery>,
) -> Result<Response, Refused> {
    let github = Github::unfenced()?;
    if let Some(installation) = query.installation_id {
        integration::manifest::installed(&cp.store, &github, &query.state, installation).await?;
    }
    Ok(setup_page(
        integration::manifest::page(&cp.store, &github, &query.state, query.page).await?,
    ))
}

pub(super) async fn github_app_callback(
    State(cp): State<ControlPlane>,
    Query(query): Query<GithubAppQuery>,
) -> Result<Response, Refused> {
    let code = query
        .code
        .ok_or_else(|| Refused::Unprocessable("GitHub did not supply a manifest code".into()))?;
    Ok(setup_page(
        integration::manifest::callback(&cp.store, &Github::unfenced()?, &query.state, &code)
            .await?,
    ))
}

pub(super) async fn github_app_installed(
    State(cp): State<ControlPlane>,
    Query(query): Query<GithubAppQuery>,
) -> Result<Response, Refused> {
    let Some(installation) = query.installation_id else {
        return Ok(setup_page(
            integration::manifest::discovery_page(
                &cp.store,
                &Github::unfenced()?,
                &query.state,
                query.page,
            )
            .await?,
        ));
    };
    let status = integration::manifest::installed(
        &cp.store,
        &Github::unfenced()?,
        &query.state,
        installation,
    )
    .await?;
    let mut client = cp
        .client
        .join("/setup")
        .map_err(|_| Refused::Unprocessable("The Client URL cannot return to setup".into()))?;
    client
        .query_pairs_mut()
        .append_pair("organization", &status.organization)
        .append_pair("github_app_flow", &query.state);
    let mut response = setup_page(integration::manifest::installed_page(&query.state));
    *response.status_mut() = StatusCode::SEE_OTHER;
    response.headers_mut().insert(
        "location",
        client
            .as_str()
            .parse()
            .map_err(|_| Refused::Unprocessable("The Client URL is invalid".into()))?,
    );
    Ok(response)
}

#[derive(Deserialize)]
pub(super) struct GithubAppPages {
    page: Option<u32>,
    per_page: Option<u32>,
}

pub(super) async fn github_app_status(
    State(cp): State<ControlPlane>,
    Path(flow): Path<String>,
) -> Result<Json<wire::GithubAppFlow>, Refused> {
    Ok(Json(integration::manifest::status(&cp.store, &flow).await?))
}

pub(super) async fn github_app_cancel(
    State(cp): State<ControlPlane>,
    Path(flow): Path<String>,
) -> Result<Json<wire::GithubAppFlow>, Refused> {
    Ok(Json(integration::manifest::cancel(&cp.store, &flow).await?))
}

pub(super) async fn github_app_installations(
    State(cp): State<ControlPlane>,
    Path(flow): Path<String>,
    Query(pages): Query<GithubAppPages>,
) -> Result<Json<wire::GithubAppInstallations>, Refused> {
    Ok(Json(
        integration::manifest::installations(
            &cp.store,
            &Github::unfenced()?,
            &flow,
            pages.page,
            pages.per_page,
        )
        .await?,
    ))
}

pub(super) async fn github_app_repositories(
    State(cp): State<ControlPlane>,
    Path(flow): Path<String>,
    Query(pages): Query<GithubAppPages>,
) -> Result<Json<wire::GithubAppRepositories>, Refused> {
    Ok(Json(
        integration::manifest::repositories(
            &cp.store,
            &Github::unfenced()?,
            &flow,
            pages.page,
            pages.per_page,
        )
        .await?,
    ))
}

pub(super) async fn github_app_finish(
    State(cp): State<ControlPlane>,
    Path(flow): Path<String>,
    request: Result<Json<wire::GithubAppFinish>, JsonRejection>,
) -> Result<Json<wire::GithubAppFlow>, Refused> {
    let Json(request) = request?;
    Ok(Json(
        integration::manifest::finish(&cp.store, &Github::unfenced()?, &flow, &request.repository)
            .await?,
    ))
}

pub(super) async fn github_app_setup_finish(
    State(cp): State<ControlPlane>,
    Query(query): Query<GithubAppQuery>,
    Form(request): Form<wire::GithubAppFinish>,
) -> Result<Response, Refused> {
    integration::manifest::finish(
        &cp.store,
        &Github::unfenced()?,
        &query.state,
        &request.repository,
    )
    .await?;
    Ok(setup_page(
        integration::manifest::page(&cp.store, &Github::unfenced()?, &query.state, None).await?,
    ))
}
