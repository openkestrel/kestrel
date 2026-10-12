use anyhow::{Result, bail};
use jiff::{SignedDuration, Timestamp};
use kestrel_operator_types as wire;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::declined::{Again, Constraint, Declined, Reason};
use crate::domain::{Connection, Direction, GithubConnection, IntegrationId};
use crate::integration::{
    credential::App,
    github::{self, Github, Unproven},
};
use crate::link::credential::Secret;
use crate::store::{Store, app_flow::Held};

pub const PAGE: &str = "/operator/github-app/setup";
pub const CALLBACK: &str = "/operator/github-app/callback";
pub const INSTALLED: &str = "/operator/github-app/installed";
pub const FLOW: &str = "/operator/github-app/flows/{flow}";
pub const INSTALLATIONS: &str = "/operator/github-app/flows/{flow}/installations";
pub const REPOSITORIES: &str = "/operator/github-app/flows/{flow}/repositories";
pub const FINISH: &str = "/operator/github-app/flows/{flow}/finish";
const UNREACHABLE: &str = "https://unreachable.invalid";
const CLEANUP: &str = "Delete or uninstall an abandoned App manually in GitHub settings; kestrel never removes it remotely.";
use wire::GithubAppFlowPhase as Phase;

#[derive(Deserialize, Serialize)]
struct Flow {
    app_name: String,
    organization: String,
    registration: wire::GithubAppStart,
    integration: IntegrationId,
    app: Option<CreatedApp>,
}

#[derive(Deserialize, Serialize)]
pub struct CreatedApp {
    pub id: i64,
    pub pem: String,
    pub webhook_secret: String,
    pub slug: String,
}

fn invalid(field: &'static str, message: impl Into<String>) -> Reason {
    Reason::InvalidField {
        field,
        operation: "github_app_setup",
        constraint: Constraint::WellFormed,
        allowed: None,
        message: message.into(),
    }
}

pub async fn start(
    store: &Store,
    organization: &str,
    mut registration: wire::GithubAppStart,
) -> Result<wire::GithubAppStarted> {
    if registration
        .app_organization
        .as_ref()
        .and_then(|owner| owner.as_ref())
        .is_some_and(|name| {
            name.is_empty() || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
    {
        bail!(invalid(
            "app_organization",
            "The GitHub organization must be a login."
        ));
    }
    registration.callback_base = base(&registration.callback_base, true)?;
    registration.webhook_base = registration
        .webhook_base
        .flatten()
        .as_deref()
        .map(|url| base(url, false))
        .transpose()?
        .map(Some);
    let mut tx = store.begin().await?;
    let organization_record = tx.organizations().named(organization).await?;
    if tx
        .integrations()
        .all(&organization_record)
        .await?
        .iter()
        .any(|i| i.name == registration.name)
    {
        bail!(Declined::Taken(
            "an integration already has that name".into()
        ));
    }
    let state = Secret::mint();
    let url = format!(
        "{}{PAGE}?state={}",
        registration.callback_base,
        state.as_str()
    );
    let status = wire::GithubAppFlow {
        flow: state.as_str().into(),
        phase: Phase::Ready,
        organization: organization.into(),
        name: registration.name.clone(),
        expires_at: format!("{:.9}", Timestamp::now() + SignedDuration::from_hours(1)),
        installation_id: None,
        integration: None,
        repository: None,
        guidance: format!(
            "Create and install the App, then explicitly select a repository. {CLEANUP}"
        ),
    };
    let flow = Flow {
        app_name: format!("kestrel-{}", &Secret::mint().as_str()[..24]),
        organization: organization.into(),
        registration,
        integration: IntegrationId::generate(),
        app: None,
    };
    tx.app_flows()
        .create(&serde_json::to_string(&flow)?, &status)
        .await?;
    tx.commit().await?;
    Ok(wire::GithubAppStarted {
        url,
        flow: state.as_str().into(),
    })
}

async fn held(store: &Store, state: &str) -> Result<Held> {
    let mut tx = store.begin().await?;
    tx.app_flows().expire(Some(state)).await?;
    let held = tx.app_flows().read(state).await;
    tx.commit().await?;
    held
}

pub async fn status(store: &Store, state: &str) -> Result<wire::GithubAppFlow> {
    Ok(held(store, state).await?.status)
}

fn configuration(held: &Held, phases: &[Phase]) -> Result<Flow> {
    if !phases.contains(&held.status.phase) {
        bail!(invalid(
            "flow",
            format!(
                "This setup phase cannot perform that step. {}",
                held.status.guidance
            )
        ));
    }
    Ok(serde_json::from_str(
        held.configuration
            .as_deref()
            .ok_or_else(|| invalid("flow", "Start GitHub App setup again."))?,
    )?)
}

fn app(flow: &Flow, installation: i64) -> Result<App> {
    let app = flow
        .app
        .as_ref()
        .ok_or_else(|| invalid("flow", "Create the App before discovering installations."))?;
    Ok(App::held(app.id, installation, &app.pem))
}

fn api(flow: &Flow) -> &str {
    flow.registration
        .api
        .as_ref()
        .and_then(|api| api.as_deref())
        .unwrap_or(github::API)
}

fn unproven(flow: &Flow, operation: &'static str, again: Again, error: Unproven) -> Reason {
    match error {
        Unproven::RateLimited { until } => Reason::GithubUnavailable {
            operation,
            integration: flow.registration.name.clone(),
            organization: flow.organization.clone(),
            retry_after_seconds: Some(until.duration_since(Timestamp::now()).as_secs().max(1)),
            again,
            message: "GitHub is limiting setup requests; retry after the wait.".into(),
        },
        Unproven::Unanswered(_) => Reason::GithubUnavailable {
            operation,
            integration: flow.registration.name.clone(),
            organization: flow.organization.clone(),
            retry_after_seconds: None,
            again,
            message: "GitHub could not answer the setup check; retry. Nothing was saved.".into(),
        },
        _ => invalid(
            "flow",
            format!(
                "The App cannot access that installation or repository. Check its installation access and retry. {CLEANUP}"
            ),
        ),
    }
}

fn pagination(page: Option<u32>, per_page: Option<u32>) -> Result<(u32, u32)> {
    let page = page.unwrap_or(1);
    let per_page = per_page.unwrap_or(30).min(100);
    if page == 0 || page == u32::MAX || per_page == 0 {
        bail!(invalid(
            "page",
            "Pages and page sizes must be positive, bounded counts."
        ));
    }
    Ok((page, per_page))
}

pub async fn installations(
    store: &Store,
    github: &Github,
    state: &str,
    page: Option<u32>,
    per_page: Option<u32>,
) -> Result<wire::GithubAppInstallations> {
    let flow = configuration(
        &held(store, state).await?,
        &[Phase::Converted, Phase::SelectRepository],
    )?;
    let (page, per_page) = pagination(page, per_page)?;
    let credential = app(&flow, 0)?;
    let installed = github
        .installations(api(&flow), &credential, page, per_page)
        .await
        .map_err(|error| unproven(&flow, "list_github_app_installations", Again::Read, error))?;
    if installed
        .iter()
        .any(|installation| installation.app_id != credential.id || installation.id <= 0)
    {
        bail!(invalid(
            "installation_id",
            "GitHub returned an installation belonging to another App."
        ));
    }
    Ok(wire::GithubAppInstallations {
        page: page.into(),
        next_page: (installed.len() == per_page as usize).then(|| i64::from(page) + 1),
        installations: installed
            .into_iter()
            .map(|installation| wire::GithubAppInstallation {
                id: installation.id,
                account: installation.account.login,
            })
            .collect(),
    })
}

pub async fn installed(
    store: &Store,
    github: &Github,
    state: &str,
    installation: i64,
) -> Result<wire::GithubAppFlow> {
    let read = held(store, state).await?;
    let flow = configuration(&read, &[Phase::Converted, Phase::SelectRepository])?;
    github
        .installation(api(&flow), &app(&flow, 0)?, installation)
        .await
        .map_err(|error| unproven(&flow, "github_app_installed", Again::Request, error))?;
    let mut tx = store.begin().await?;
    let current = tx.app_flows().read(state).await?;
    configuration(&current, &[Phase::Converted, Phase::SelectRepository])?;
    if current.status.expires_at.parse::<Timestamp>()? <= Timestamp::now() {
        bail!(invalid("flow", "Setup expired; start again."));
    }
    let expected = current.status.phase.clone();
    let mut status = current.status;
    status.phase = Phase::SelectRepository;
    status.installation_id = Some(installation);
    status.guidance = "Select one accessible repository explicitly, then finish setup. Retry repository reads if GitHub is unavailable.".into();
    tx.app_flows()
        .save(expected, current.configuration.as_deref(), &status)
        .await?;
    tx.commit().await?;
    Ok(status)
}

pub async fn repositories(
    store: &Store,
    github: &Github,
    state: &str,
    page: Option<u32>,
    per_page: Option<u32>,
) -> Result<wire::GithubAppRepositories> {
    let read = held(store, state).await?;
    let flow = configuration(&read, &[Phase::SelectRepository])?;
    let credential = app(
        &flow,
        read.status
            .installation_id
            .expect("selection holds an installation"),
    )?;
    let (page, per_page) = pagination(page, per_page)?;
    github
        .installation(api(&flow), &credential, credential.installation)
        .await
        .map_err(|error| unproven(&flow, "list_github_app_repositories", Again::Read, error))?;
    let installed = github
        .installation_repositories(api(&flow), &credential, page, per_page)
        .await
        .map_err(|error| unproven(&flow, "list_github_app_repositories", Again::Read, error))?;
    Ok(wire::GithubAppRepositories {
        page: page.into(),
        total: i64::try_from(installed.total_count)?,
        next_page: (u64::from(page) * u64::from(per_page) < installed.total_count)
            .then(|| i64::from(page) + 1),
        repositories: installed
            .repositories
            .into_iter()
            .map(|repository| wire::GithubAppRepository {
                id: repository.id,
                repository: repository.full_name,
                default_branch: repository.default_branch,
                private: repository.private,
            })
            .collect(),
    })
}

fn completed(status: wire::GithubAppFlow, repository: &str) -> Result<wire::GithubAppFlow> {
    if status.repository.as_deref() != Some(repository) {
        bail!(invalid(
            "repository",
            "Setup already completed for another repository."
        ));
    }
    Ok(status)
}

pub async fn finish(
    store: &Store,
    github: &Github,
    state: &str,
    repository: &str,
) -> Result<wire::GithubAppFlow> {
    let repository = github::repository(repository)?;
    let read = held(store, state).await?;
    if read.status.phase == Phase::Completed {
        return completed(read.status, &repository);
    }
    let flow = configuration(&read, &[Phase::SelectRepository])?;
    let credential = app(
        &flow,
        read.status
            .installation_id
            .expect("selection holds an installation"),
    )?;
    let installation = github
        .repository_installation(api(&flow), &credential, &repository)
        .await
        .map_err(|error| unproven(&flow, "finish_github_app_flow", Again::Request, error))?;
    if installation != credential.installation {
        bail!(invalid(
            "repository",
            "The selected repository belongs to a different installation."
        ));
    }
    let named = github
        .installation_repository(api(&flow), &credential, &repository)
        .await
        .map_err(|error| unproven(&flow, "finish_github_app_flow", Again::Request, error))?;
    if named.id <= 0
        || !named.full_name.eq_ignore_ascii_case(&repository)
        || named.default_branch.is_empty()
    {
        bail!(invalid(
            "repository",
            "GitHub could not establish the selected repository and its default branch."
        ));
    }
    let mut tx = store.begin().await?;
    let current = tx.app_flows().read(state).await?;
    if current.status.phase == Phase::Completed {
        return completed(current.status, &repository);
    }
    configuration(&current, &[Phase::SelectRepository])?;
    if current.status.installation_id != read.status.installation_id
        || current.status.expires_at.parse::<Timestamp>()? <= Timestamp::now()
    {
        bail!(invalid(
            "flow",
            "Setup expired or its installation changed; inspect setup before retrying."
        ));
    }
    let organization = tx.organizations().named(&flow.organization).await?;
    let integration = tx
        .integrations()
        .register_with_id(
            flow.integration,
            &organization,
            &flow.registration.name,
            Connection::Github(GithubConnection {
                repository: repository.clone(),
                api: api(&flow).into(),
                credential,
                bot_login: format!(
                    "{}[bot]",
                    flow.app.as_ref().expect("selection holds the App").slug
                ),
                interval: SignedDuration::from_mins(1),
                repository_id: named.id,
            }),
            &[Direction::Inbound, Direction::Outbound],
            Some(
                &flow
                    .app
                    .as_ref()
                    .expect("selection holds the App")
                    .webhook_secret,
            ),
        )
        .await?;
    let mut status = current.status;
    status.phase = Phase::Completed;
    status.integration = Some(integration.id.to_string());
    status.repository = Some(repository);
    status.guidance =
        "The GitHub Integration is ready. Temporary App setup credentials have been erased.".into();
    tx.app_flows()
        .save(Phase::SelectRepository, None, &status)
        .await?;
    tx.commit().await?;
    Ok(status)
}

pub async fn cancel(store: &Store, state: &str) -> Result<wire::GithubAppFlow> {
    let mut tx = store.begin().await?;
    tx.app_flows().expire(Some(state)).await?;
    let mut status = tx.app_flows().read(state).await?.status;
    if !matches!(
        status.phase,
        Phase::Completed | Phase::Expired | Phase::Cancelled
    ) {
        let expected = status.phase.clone();
        status.phase = Phase::Cancelled;
        status.guidance =
            format!("Local setup cancelled and temporary credentials erased. {CLEANUP}");
        tx.app_flows().save(expected, None, &status).await?;
    }
    tx.commit().await?;
    Ok(status)
}

pub async fn expire(store: &Store) -> Result<()> {
    let mut tx = store.begin().await?;
    tx.app_flows().expire(None).await?;
    tx.commit().await?;
    Ok(())
}

pub async fn callback(store: &Store, github: &Github, state: &str, code: &str) -> Result<String> {
    if code.is_empty() || !code.bytes().all(|b| b.is_ascii_alphanumeric()) {
        bail!(invalid("code", "The GitHub manifest code is invalid."));
    }
    held(store, state).await?;
    let mut tx = store.begin().await?;
    let read = tx.app_flows().read(state).await?;
    let mut flow = configuration(&read, &[Phase::Ready])?;
    let mut status = read.status;
    status.phase = Phase::Exchanging;
    status.guidance = format!(
        "Conversion may be in progress or its result uncertain. If it cannot complete, cancel and start again. {CLEANUP}"
    );
    tx.app_flows()
        .save(Phase::Ready, read.configuration.as_deref(), &status)
        .await?;
    tx.commit().await?;
    let converted = github.convert_manifest(api(&flow), code).await;
    let app = match converted {
        Ok(app)
            if app.id > 0
                && !app.pem.is_empty()
                && !app.webhook_secret.is_empty()
                && !app.slug.is_empty()
                && app
                    .slug
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-') =>
        {
            app
        }
        _ => {
            let mut tx = store.begin().await?;
            let current = tx.app_flows().read(state).await?;
            if current.status.phase == Phase::Exchanging {
                status.phase = Phase::ConversionFailed;
                status.guidance = format!(
                    "One-time App conversion failed or its outcome is uncertain. Cancel and restart setup with a new App. {CLEANUP}"
                );
                tx.app_flows()
                    .save(Phase::Exchanging, None, &status)
                    .await?;
            }
            tx.commit().await?;
            bail!(invalid("flow", status.guidance));
        }
    };
    flow.app = Some(app);
    let mut tx = store.begin().await?;
    let current = tx.app_flows().read(state).await?;
    configuration(&current, &[Phase::Exchanging])?;
    if status.expires_at.parse::<Timestamp>()? <= Timestamp::now() {
        bail!(invalid("flow", "Setup expired; start again."));
    }
    status.phase = Phase::Converted;
    status.guidance =
        "Install the App, then discover its installations and explicitly select a repository."
            .into();
    tx.app_flows()
        .save(
            Phase::Exchanging,
            Some(&serde_json::to_string(&flow)?),
            &status,
        )
        .await?;
    tx.commit().await?;
    install_page(&flow, state)
}

fn install_page(flow: &Flow, state: &str) -> Result<String> {
    let install = format!(
        "https://github.com/apps/{}/installations/new",
        flow.app
            .as_ref()
            .expect("a converted flow holds the App")
            .slug
    );
    Ok(format!(
        "<!doctype html><title>Install kestrel's GitHub App</title><h1>Install the App</h1><p><a href=\"{}\">Install the App</a> on the repositories you want to make accessible.</p><p>Then <a href=\"{}{}?state={}\">discover installations and choose one repository</a>.</p>",
        escape(&install),
        escape(&flow.registration.callback_base),
        INSTALLED,
        escape(state)
    ))
}

pub async fn page(
    store: &Store,
    github: &Github,
    state: &str,
    page: Option<u32>,
) -> Result<String> {
    let read = held(store, state).await?;
    match read.status.phase {
        Phase::Ready => manifest_page(&configuration(&read, &[Phase::Ready])?, state),
        Phase::Converted => install_page(&configuration(&read, &[Phase::Converted])?, state),
        Phase::SelectRepository => {
            let listed = repositories(store, github, state, page, None).await?;
            let mut html = format!(
                "<!doctype html><title>Choose a repository</title><h1>Choose one repository</h1><form method=\"post\" action=\"{PAGE}?state={}\">",
                escape(state)
            );
            for repository in listed.repositories {
                html.push_str(&format!("<p><label><input required type=\"radio\" name=\"repository\" value=\"{}\">{} (default branch: {})</label></p>", escape(&repository.repository), escape(&repository.repository), escape(&repository.default_branch)));
            }
            html.push_str("<button>Register Integration for selected repository</button></form>");
            if listed.total == 0 {
                html.push_str("<p>No repositories are accessible. Update the App installation in GitHub and reload to retry.</p>");
            }
            if let Some(next) = listed.next_page {
                html.push_str(&format!(
                    "<a href=\"{PAGE}?state={}&amp;page={next}\">Next page</a>",
                    escape(state)
                ));
            }
            Ok(html)
        }
        _ => Ok(format!(
            "<!doctype html><title>GitHub App setup</title><h1>GitHub App setup</h1><p>{}</p>",
            escape(&read.status.guidance)
        )),
    }
}

pub async fn discovery_page(
    store: &Store,
    github: &Github,
    state: &str,
    page: Option<u32>,
) -> Result<String> {
    let listed = installations(store, github, state, page, None).await?;
    let mut html =
        "<!doctype html><title>Choose an installation</title><h1>Choose an App installation</h1>"
            .to_owned();
    if listed.installations.is_empty() {
        html.push_str(
            "<p>No installation is accessible yet. Install the App and reload to retry.</p>",
        );
    }
    for installation in listed.installations {
        html.push_str(&format!(
            "<p><a href=\"{PAGE}?state={}&amp;installation_id={}\">{}</a></p>",
            escape(state),
            installation.id,
            escape(&installation.account)
        ));
    }
    if let Some(next) = listed.next_page {
        html.push_str(&format!(
            "<a href=\"{INSTALLED}?state={}&amp;page={next}\">Next page</a>",
            escape(state)
        ));
    }
    Ok(html)
}
fn base(value: &str, loopback: bool) -> Result<String> {
    let url = reqwest::Url::parse(value).map_err(|_| {
        invalid(
            "callback_base",
            "A callback or webhook base must be an absolute URL.",
        )
    })?;
    let local = url.host_str().is_some_and(|host| {
        host == "localhost"
            || host
                .trim_matches(['[', ']'])
                .parse::<std::net::IpAddr>()
                .is_ok_and(|ip| ip.is_loopback())
    });
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path() != "/"
        || (loopback && !local)
        || (!loopback && (local || url.scheme() != "https"))
    {
        bail!(invalid(
            "callback_base",
            "The callback base must be a loopback HTTP(S) origin; the webhook base must be a public HTTPS origin."
        ));
    }
    Ok(url.as_str().trim_end_matches('/').into())
}

fn manifest_page(flow: &Flow, state: &str) -> Result<String> {
    let base = &flow.registration.callback_base;
    // GitHub refuses a loopback hook URL, but never resolves a public one.
    let webhook_base = flow
        .registration
        .webhook_base
        .as_ref()
        .and_then(|base| base.as_deref())
        .unwrap_or(UNREACHABLE);
    let hook =
        json!({"url": format!("{webhook_base}/webhooks/{}", flow.integration), "active": true});
    let action = flow
        .registration
        .app_organization
        .as_ref()
        .and_then(|owner| owner.as_ref())
        .map_or_else(
            || "https://github.com/settings/apps/new".to_owned(),
            |owner| format!("https://github.com/organizations/{owner}/settings/apps/new"),
        );
    let manifest = json!({
        "name": flow.app_name,
        "url": "https://github.com/openkestrel/kestrel",
        "public": false,
        "redirect_url": format!("{base}{CALLBACK}"),
        "setup_url": format!("{base}{INSTALLED}?state={state}"),
        "hook_attributes": hook,
        "default_permissions": {"contents": "write", "issues": "write", "pull_requests": "write", "metadata": "read"},
        "default_events": ["issues", "issue_comment", "pull_request"]
    });
    Ok(format!(
        "<!doctype html><title>Create kestrel's GitHub App</title><h1>Create kestrel's GitHub App</h1><form method=\"post\" action=\"{}?state={}\"><input type=\"hidden\" name=\"manifest\" value=\"{}\"><button>Create GitHub App</button></form>",
        escape(&action),
        escape(state),
        escape(&manifest.to_string())
    ))
}

fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
