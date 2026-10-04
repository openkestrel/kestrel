use anyhow::{Result, bail};
use jiff::{SignedDuration, Timestamp};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::declined::Declined;
use crate::domain::{Connection, Direction, GithubConnection, Integration, IntegrationId};
use crate::integration::{
    credential::App,
    github::{self, Github},
};
use crate::link::credential::Secret;
use crate::store::Store;

pub const PAGE: &str = "/operator/github-app/setup";
pub const CALLBACK: &str = "/operator/github-app/callback";
pub const INSTALLED: &str = "/operator/github-app/installed";

#[derive(Deserialize, Serialize)]
pub struct Start {
    pub name: String,
    pub repository: String,
    pub callback_base: String,
    pub webhook_base: Option<String>,
    pub api: Option<String>,
    pub app_organization: Option<String>,
}

#[derive(Deserialize, Serialize)]
struct Flow {
    organization: String,
    registration: Start,
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

pub async fn start(store: &Store, organization: &str, mut registration: Start) -> Result<Value> {
    if registration.app_organization.as_ref().is_some_and(|name| {
        name.is_empty() || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
    }) {
        bail!(Declined::Unacceptable(
            "the GitHub organization must be a login".into()
        ));
    }
    registration.repository = github::repository(&registration.repository)?;
    registration.callback_base = base(&registration.callback_base, true)?;
    registration.webhook_base = registration
        .webhook_base
        .as_deref()
        .map(|url| base(url, false))
        .transpose()?;
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
    let flow = Flow {
        organization: organization.into(),
        registration,
        integration: IntegrationId::generate(),
        app: None,
    };
    tx.app_flows()
        .create(
            state.as_str(),
            &serde_json::to_string(&flow)?,
            Timestamp::now() + SignedDuration::from_hours(1),
        )
        .await?;
    tx.commit().await?;
    Ok(json!({"url": url}))
}

fn base(value: &str, loopback: bool) -> Result<String> {
    let url = reqwest::Url::parse(value).map_err(|_| {
        Declined::Unacceptable("a callback or webhook base must be an absolute URL".into())
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
        bail!(Declined::Unacceptable("the callback base must be a loopback HTTP(S) origin; the webhook base must be a public HTTPS origin".into()));
    }
    Ok(url.as_str().trim_end_matches('/').into())
}

pub async fn page(store: &Store, state: &str) -> Result<String> {
    let flow: Flow =
        serde_json::from_str(&store.read().await?.app_flows().read(state, "ready").await?)?;
    let base = &flow.registration.callback_base;
    let hook = flow.registration.webhook_base.as_ref().map_or(
        json!({"active": false}),
        |base| json!({"url": format!("{base}/webhooks/{}", flow.integration), "active": true}),
    );
    let action = flow.registration.app_organization.as_ref().map_or_else(
        || "https://github.com/settings/apps/new".to_owned(),
        |owner| format!("https://github.com/organizations/{owner}/settings/apps/new"),
    );
    let manifest = json!({
        "name": format!("kestrel-{}", flow.registration.name),
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

pub async fn callback(store: &Store, github: &Github, state: &str, code: &str) -> Result<String> {
    if code.is_empty() || !code.bytes().all(|b| b.is_ascii_alphanumeric()) {
        bail!(Declined::Unacceptable(
            "the GitHub manifest code is invalid".into()
        ));
    }
    let mut tx = store.begin().await?;
    let mut flow: Flow = serde_json::from_str(&tx.app_flows().claim(state).await?)?;
    tx.commit().await?;
    let app = github
        .convert_manifest(
            flow.registration.api.as_deref().unwrap_or(github::API),
            code,
        )
        .await?;
    if app.id <= 0
        || app.pem.is_empty()
        || app.webhook_secret.is_empty()
        || app.slug.is_empty()
        || !app
            .slug
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
    {
        bail!("GitHub returned an incomplete App configuration");
    }
    let install = format!("https://github.com/apps/{}/installations/new", app.slug);
    flow.app = Some(app);
    let mut tx = store.begin().await?;
    tx.app_flows()
        .converted(state, &serde_json::to_string(&flow)?)
        .await?;
    tx.commit().await?;
    let finish = format!(
        "{}{INSTALLED}?state={state}",
        flow.registration.callback_base
    );
    Ok(format!(
        "<!doctype html><title>Install kestrel's GitHub App</title><h1>Install the App</h1><p><a href=\"{}\">Install on {}</a>, selecting this repository. GitHub will return here after installation.</p><p>If it does not, <a href=\"{}\">finish setup after installing</a>.</p>",
        escape(&install),
        escape(&flow.registration.repository),
        escape(&finish)
    ))
}

pub async fn installed(store: &Store, github: &Github, state: &str) -> Result<Integration> {
    let flow: Flow = serde_json::from_str(
        &store
            .read()
            .await?
            .app_flows()
            .read(state, "converted")
            .await?,
    )?;
    let app = flow.app.as_ref().expect("a converted flow holds the app");
    let api = flow.registration.api.as_deref().unwrap_or(github::API);
    let mut credential = App::held(app.id, 0, &app.pem);
    credential.installation = github
        .repository_installation(api, &credential, &flow.registration.repository)
        .await?;
    let mut tx = store.begin().await?;
    tx.app_flows().read(state, "converted").await?;
    let organization = tx.organizations().named(&flow.organization).await?;
    let integration = tx
        .integrations()
        .register_with_id(
            flow.integration,
            &organization,
            &flow.registration.name,
            Connection::Github(GithubConnection {
                repository: flow.registration.repository,
                api: api.into(),
                credential,
                bot_login: format!("{}[bot]", app.slug),
                interval: SignedDuration::from_mins(1),
                signed: flow.registration.webhook_base.is_some(),
            }),
            &[Direction::Inbound, Direction::Outbound],
            Some(app.webhook_secret.as_str()),
        )
        .await?;
    tx.app_flows().remove(state).await?;
    tx.commit().await?;
    Ok(integration)
}

fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
