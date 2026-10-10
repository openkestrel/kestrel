use std::collections::HashMap;
use std::fmt;
use std::sync::Mutex;
use std::time::Duration;

use anyhow::{Context as _, Result, anyhow, bail};
use jiff::{SignedDuration, Timestamp};
use reqwest::header::{HeaderMap, HeaderValue};
use reqwest::{Response, StatusCode};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::declined::Declined;
use crate::domain::{GithubConnection, Integration, IntegrationId, Occurrence};
use crate::integration::credential::App;
use crate::readiness::{Delegation, Readiness, WorkState};
use crate::store::Store;

pub const API: &str = "https://api.github.com";

/// GitHub reports a label coming off an issue as an `unlabeled` event carrying that same
/// label, so a trigger matching on the type and label alone would fire on both.
pub const LABELLED: &str = "com.github.issues.labeled";
pub const COMMENTED: &str = "com.github.issue_comment.created";
pub const PULL_REQUEST_OPENED: &str = "com.github.pull_request.opened";
pub const PULL_REQUEST_REOPENED: &str = "com.github.pull_request.reopened";
pub const PULL_REQUEST_CLOSED: &str = "com.github.pull_request.closed";
pub const PULL_REQUEST_SYNCHRONIZE: &str = "com.github.pull_request.synchronize";

/// A comment that opens with it is a command to kestrel rather than a remark to the Workspace.
pub const MENTION: &str = "@kestrel";
const AGENT: &str = "agent=";

/// Every outcome comment carries it, so a post that never learned whether its comment
/// landed can recognise that it already did. It never marks what may be heard: kestrel knows
/// its own voice by author (ADR-0028).
pub const MARKER: &str = "<!-- kestrel session ";

const VERSION: &str = "2022-11-28";
const PER_PAGE: usize = 100;
const REQUEST: Duration = Duration::from_secs(30);

const PAGES: usize = 10;

/// Why a poll came back with nothing rather than with no events. Neither moves where the
/// Integration reads from, so the next poll covers the same window again.
#[derive(Debug)]
pub enum Refused {
    RateLimited { until: Timestamp },
    Failed(anyhow::Error),
}

impl fmt::Display for Refused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Refused::RateLimited { until } => write!(f, "github is rate limiting until {until}"),
            Refused::Failed(error) => write!(f, "{error}"),
        }
    }
}

/// A redelivery is listed again under the same `guid`.
#[derive(Debug, Deserialize)]
pub struct Listed {
    pub id: i64,
    pub guid: String,
    pub delivered_at: Timestamp,
    pub installation_id: Option<i64>,
    pub repository_id: Option<i64>,
}

pub struct Listing {
    /// Oldest first, one per `guid`, only this Integration's repository's.
    pub listed: Vec<Listed>,
    /// By GitHub's clock, whoever's Delivery it was.
    pub newest: Option<Timestamp>,
    /// The log ended before reaching where the poll started reading.
    pub ran_out: bool,
}

/// An installation token this process has already minted, good until another request needs
/// one closer to its real expiry than `EXPIRY_MARGIN` allows.
struct Minted {
    token: String,
    expires_at: Timestamp,
}

/// How much slack a cached installation token keeps before its real expiry: minted fresh
/// rather than risking it expiring between a request going out and GitHub answering it.
const EXPIRY_MARGIN: i64 = 60;

pub struct Github {
    client: reqwest::Client,
    /// Installation tokens this process has minted, by Integration: GitHub charges nothing
    /// extra for reusing one, and letting each expire unused would mint one per request.
    /// Keyed by revision too: a token minted before a change is never handed out after it.
    tokens: Mutex<HashMap<(IntegrationId, i64), Minted>>,
    fence: Option<Store>,
}

impl Github {
    pub async fn convert_manifest(
        &self,
        api: &str,
        code: &str,
    ) -> Result<super::manifest::CreatedApp> {
        let response = self
            .client
            .post(format!(
                "{}/app-manifests/{code}/conversions",
                api.trim_end_matches('/')
            ))
            .header("accept", "application/vnd.github+json")
            .header("x-github-api-version", VERSION)
            .send()
            .await
            .map_err(|_| anyhow!("the GitHub manifest exchange could not be reached"))?;
        if !response.status().is_success() {
            bail!(
                "GitHub refused the manifest exchange ({})",
                response.status()
            );
        }
        response
            .json()
            .await
            .map_err(|_| anyhow!("GitHub returned an invalid App configuration"))
    }

    pub async fn repository_installation(
        &self,
        api: &str,
        app: &App,
        repository: &str,
    ) -> Result<i64> {
        #[derive(Deserialize)]
        struct Installation {
            id: i64,
            app_id: i64,
        }
        let response = self
            .as_app(
                reqwest::Method::GET,
                app,
                &format!(
                    "{}/repos/{repository}/installation",
                    api.trim_end_matches('/')
                ),
            )?
            .send()
            .await
            .context("checking the App installation")?;
        if !response.status().is_success() {
            bail!(Declined::Unacceptable(
                "install the App on the requested repository before finishing setup".into()
            ));
        }
        let installation: Installation = response
            .json()
            .await
            .context("reading the App installation")?;
        if installation.app_id != app.id || installation.id <= 0 {
            bail!(Declined::Unacceptable(
                "the repository installation does not belong to this App".into()
            ));
        }
        Ok(installation.id)
    }

    /// Refuses a token to work begun before its Integration was disabled or changed.
    pub fn dialling_out(store: &Store) -> Result<Self> {
        Ok(Self {
            fence: Some(store.clone()),
            ..Self::unfenced()?
        })
    }

    /// For calls made with a credential rather than a saved Integration, such as registration.
    pub fn unfenced() -> Result<Self> {
        Ok(Self {
            client: reqwest::Client::builder()
                .timeout(REQUEST)
                .user_agent(concat!("kestrel/", env!("CARGO_PKG_VERSION")))
                .build()
                .context("building the client kestrel polls github with")?,
            tokens: Mutex::new(HashMap::new()),
            fence: None,
        })
    }

    async fn still_current(&self, integration: &Integration) -> Result<(), Refused> {
        let Some(store) = &self.fence else {
            return Ok(());
        };
        let unchanged = async {
            store
                .read()
                .await?
                .integrations()
                .unchanged(integration)
                .await
        }
        .await
        .map_err(Refused::Failed)?;
        if !unchanged {
            self.tokens
                .lock()
                .expect("the token cache is not poisoned")
                .retain(|(id, revision), _| {
                    *id != integration.id || *revision > integration.revision
                });
            return Err(Refused::Failed(anyhow!(
                "the integration {} was disabled, retired or changed while this was under way",
                integration.name
            )));
        }
        Ok(())
    }

    /// The installation token this Integration currently presents: the one already cached,
    /// unless it is close enough to expiry that a request starting now might outlive it.
    async fn token(
        &self,
        integration: &Integration,
        github: &GithubConnection,
    ) -> Result<String, Refused> {
        self.still_current(integration).await?;
        let fresh_enough = Timestamp::now() + SignedDuration::from_secs(EXPIRY_MARGIN);
        if let Some(minted) = self
            .tokens
            .lock()
            .expect("the token cache is not poisoned")
            .get(&(integration.id, integration.revision))
            && minted.expires_at > fresh_enough
        {
            return Ok(minted.token.clone());
        }

        let minted = self.mint(&github.api, &github.credential).await?;
        self.still_current(integration).await?;
        let token = minted.token.clone();
        self.tokens
            .lock()
            .expect("the token cache is not poisoned")
            .insert((integration.id, integration.revision), minted);

        Ok(token)
    }

    /// Exchanges a freshly signed App JWT for an installation token, scoped to exactly the
    /// installation the Integration was registered against.
    async fn mint(&self, api: &str, app: &App) -> Result<Minted, Refused> {
        let response = self
            .as_app(
                reqwest::Method::POST,
                app,
                &format!(
                    "{}/app/installations/{}/access_tokens",
                    api.trim_end_matches('/'),
                    app.installation
                ),
            )
            .map_err(Refused::Failed)?
            .send()
            .await
            .map_err(|error| {
                Refused::Failed(anyhow!(
                    "an installation token could not be minted: {error}"
                ))
            })?;

        let minted: MintedToken = answered(response, "an installation token").await?;
        Ok(Minted {
            token: minted.token,
            expires_at: minted.expires_at,
        })
    }

    /// What GitHub calls the App's own identity: its bot account's login, `<slug>[bot]`,
    /// learned once at registration so later work can recognise what the Integration itself
    /// said. The only caller of this signs no cached token, since no Integration exists yet to
    /// cache one against.
    pub async fn app_bot_login(&self, api: &str, app: &App) -> Result<String> {
        let response = self
            .as_app(
                reqwest::Method::GET,
                app,
                &format!("{}/app", api.trim_end_matches('/')),
            )?
            .send()
            .await
            .context("the app's own identity could not be read")?;

        let app_record: AppRecord = answered(response, "the app's own identity")
            .await
            .map_err(|refused| anyhow!("{refused}"))?;

        Ok(format!("{}[bot]", app_record.slug))
    }

    /// GitHub's id for the repository, which is how its Delivery log names it.
    pub async fn repository_id(&self, api: &str, app: &App, repository: &str) -> Result<i64> {
        #[derive(Deserialize)]
        struct Named {
            id: i64,
        }

        let token = self
            .mint(api, app)
            .await
            .map_err(|refused| anyhow!("{refused}"))?
            .token;
        let response = self
            .client
            .get(format!("{}/repos/{repository}", api.trim_end_matches('/')))
            .header("accept", "application/vnd.github+json")
            .header("x-github-api-version", VERSION)
            .bearer_auth(token)
            .send()
            .await
            .with_context(|| format!("{repository} could not be read"))?;
        let named: Named = answered(response, repository)
            .await
            .map_err(|refused| anyhow!("{refused}"))?;

        Ok(named.id)
    }

    /// GitHub lists the log newest first, across every installation and repository the App has.
    pub async fn deliveries(
        &self,
        integration: &Integration,
        from: Timestamp,
    ) -> Result<Listing, Refused> {
        self.still_current(integration).await?;
        let github = integration.github().map_err(Refused::Failed)?;
        let mut url = format!(
            "{}/app/hook/deliveries?per_page={PER_PAGE}",
            github.api.trim_end_matches('/')
        );
        let mut newest = None;
        let mut newest_first: Vec<Listed> = Vec::new();

        loop {
            let response = self
                .as_app(reqwest::Method::GET, &github.credential, &url)
                .map_err(Refused::Failed)?
                .send()
                .await
                .map_err(|error| {
                    Refused::Failed(anyhow!("the app's deliveries could not be listed: {error}"))
                })?;
            let next = next_page(response.headers());
            let page: Vec<Listed> = answered(response, "the app's deliveries").await?;
            newest = newest.or_else(|| page.iter().map(|listed| listed.delivered_at).max());
            let reached = page.iter().any(|listed| listed.delivered_at < from);
            newest_first.extend(page.into_iter().filter(|listed| {
                listed.delivered_at >= from
                    && listed.installation_id == Some(github.credential.installation)
                    && listed.repository_id == Some(github.repository_id)
            }));
            match next {
                Some(next) if !reached => url = next,
                _ => {
                    let mut guids = std::collections::HashSet::new();
                    newest_first.reverse();
                    newest_first.retain(|listed| guids.insert(listed.guid.clone()));
                    newest_first.sort_by_key(|listed| (listed.delivered_at, listed.id));
                    return Ok(Listing {
                        listed: newest_first,
                        newest,
                        ran_out: !reached,
                    });
                }
            }
        }
    }

    pub async fn delivery(
        &self,
        integration: &Integration,
        listed: &Listed,
    ) -> Result<Occurrence, Refused> {
        #[derive(Deserialize)]
        struct Delivered {
            event: String,
            request: Request,
        }
        #[derive(Deserialize)]
        struct Request {
            payload: Option<serde_json::Value>,
        }

        self.still_current(integration).await?;
        let github = integration.github().map_err(Refused::Failed)?;
        let asked_for = format!("the delivery {}", listed.guid);
        let response = self
            .as_app(
                reqwest::Method::GET,
                &github.credential,
                &format!(
                    "{}/app/hook/deliveries/{}",
                    github.api.trim_end_matches('/'),
                    listed.id
                ),
            )
            .map_err(Refused::Failed)?
            .send()
            .await
            .map_err(|error| Refused::Failed(anyhow!("{asked_for} could not be read: {error}")))?;
        let delivery: Delivered = answered(response, &asked_for).await?;
        let payload = delivery
            .request
            .payload
            .ok_or_else(|| Refused::Failed(anyhow!("{asked_for} carries no payload")))?;

        delivered(
            github,
            &delivery.event,
            &listed.guid,
            payload,
            listed.delivered_at,
        )
        .map_err(Refused::Failed)
    }

    fn as_app(
        &self,
        method: reqwest::Method,
        app: &App,
        url: &str,
    ) -> Result<reqwest::RequestBuilder> {
        Ok(self
            .client
            .request(method, url)
            .header("accept", "application/vnd.github+json")
            .header("x-github-api-version", VERSION)
            .bearer_auth(app_jwt(app)?))
    }

    /// The comment kestrel leaves on the issue the work came from. What comes back is where
    /// it landed, so a delivery that is asked again recognises its own comment.
    pub async fn comment(
        &self,
        integration: &Integration,
        subject: i64,
        body: &str,
    ) -> Result<Comment, Refused> {
        let github = integration.github().map_err(Refused::Failed)?;
        let repository = repository(&github.repository).map_err(Refused::Failed)?;
        let response = self
            .request(
                reqwest::Method::POST,
                integration,
                &format!("repos/{repository}/issues/{subject}/comments"),
            )
            .await?
            .json(&Body { body })
            .send()
            .await
            .map_err(|error| {
                Refused::Failed(anyhow!(
                    "{repository}#{subject} could not be commented on: {error}"
                ))
            })?;

        answered(response, &format!("a comment on {repository}#{subject}")).await
    }

    /// Whether a comment carrying `marker` is already on the issue. `since` bounds the read to
    /// the window an attempt could have landed in, so this is one page rather than a walk back
    /// through everything an issue has ever collected.
    pub async fn comment_carrying(
        &self,
        integration: &Integration,
        subject: i64,
        marker: &str,
        since: Timestamp,
    ) -> Result<Option<Comment>, Refused> {
        let github = integration.github().map_err(Refused::Failed)?;
        let repository = repository(&github.repository).map_err(Refused::Failed)?;
        let response = self
            .request(
                reqwest::Method::GET,
                integration,
                &format!(
                    "repos/{repository}/issues/{subject}/comments?per_page={PER_PAGE}&since={since}"
                ),
            )
            .await?
            .send()
            .await
            .map_err(|error| {
                Refused::Failed(anyhow!(
                    "the comments on {repository}#{subject} could not be read: {error}"
                ))
            })?;

        let comments: Vec<Comment> =
            answered(response, &format!("the comments on {repository}#{subject}")).await?;

        Ok(comments
            .into_iter()
            .find(|comment| comment.body.contains(marker)))
    }

    pub async fn issue(
        &self,
        integration: &Integration,
        number: i64,
    ) -> Result<serde_json::Value, Refused> {
        let github = integration.github().map_err(Refused::Failed)?;
        let repository = repository(&github.repository).map_err(Refused::Failed)?;
        let response = self
            .request(
                reqwest::Method::GET,
                integration,
                &format!("repos/{repository}/issues/{number}"),
            )
            .await?
            .send()
            .await
            .map_err(|error| {
                Refused::Failed(anyhow!("{repository}#{number} could not be read: {error}"))
            })?;

        answered(response, &format!("{repository}#{number}")).await
    }

    pub async fn pull_request(
        &self,
        integration: &Integration,
        number: i64,
    ) -> Result<serde_json::Value, Refused> {
        let github = integration.github().map_err(Refused::Failed)?;
        let repository = repository(&github.repository).map_err(Refused::Failed)?;
        let response = self
            .request(
                reqwest::Method::GET,
                integration,
                &format!("repos/{repository}/pulls/{number}"),
            )
            .await?
            .send()
            .await
            .map_err(|error| {
                Refused::Failed(anyhow!(
                    "pull request {number} on {repository} could not be read: {error}"
                ))
            })?;

        answered(response, &format!("pull request {number} on {repository}")).await
    }

    pub async fn readiness(
        &self,
        integration: &Integration,
        occurrence: &Occurrence,
    ) -> Result<Readiness, Refused> {
        let github = integration.github().map_err(Refused::Failed)?;
        let number = EventData::new(occurrence)
            .subject_issue()
            .ok_or_else(|| Refused::Failed(anyhow!("the event names no GitHub issue")))?;
        let work_item = format!("https://github.com/{}/issues/{number}", github.repository);
        let issue = self.issue(integration, number).await?;
        if issue.get("number").and_then(serde_json::Value::as_i64) != Some(number) {
            return Err(Refused::Failed(anyhow!(
                "github returned a different issue for {work_item}"
            )));
        }
        let state = match issue.get("state").and_then(serde_json::Value::as_str) {
            Some("open") => WorkState::Open,
            Some("closed") => WorkState::Terminal,
            _ => WorkState::Unknown,
        };
        let assigned = issue
            .get("assignees")
            .and_then(serde_json::Value::as_array)
            .map(|assignees| {
                assignees.iter().any(|assignee| {
                    assignee
                        .get("login")
                        .and_then(serde_json::Value::as_str)
                        .is_some_and(|login| {
                            login.eq_ignore_ascii_case(MENTION.trim_start_matches('@'))
                        })
                })
            });

        let mut evidence = vec![work_item.clone()];
        let command = EventData::new(occurrence).command();
        let live_command = if command.is_some() && assigned != Some(true) {
            let id = occurrence
                .data
                .get("comment")
                .and_then(|comment| comment.get("id"))
                .and_then(serde_json::Value::as_i64);
            match id {
                Some(id) => {
                    let response = self
                        .request(
                            reqwest::Method::GET,
                            integration,
                            &format!(
                                "repos/{}/issues/comments/{id}",
                                repository(&github.repository).map_err(Refused::Failed)?
                            ),
                        )
                        .await?
                        .send()
                        .await
                        .map_err(|error| {
                            Refused::Failed(anyhow!(
                                "the command on {work_item} could not be read: {error}"
                            ))
                        })?;
                    let comment: serde_json::Value =
                        answered(response, &format!("the command on {work_item}")).await?;
                    evidence.push(format!("{work_item}#issuecomment-{id}"));
                    let original = EventData::new(occurrence);
                    let actor = original.actor();
                    let mut current = occurrence.clone();
                    current.data = comment;
                    let current_actor = current
                        .data
                        .get("user")
                        .and_then(|user| user.get("login"))
                        .and_then(serde_json::Value::as_str);
                    let same_issue = current
                        .data
                        .get("issue_url")
                        .and_then(serde_json::Value::as_str)
                        .is_some_and(|url| url.ends_with(&format!("/issues/{number}")));
                    Some(
                        actor
                            .zip(current_actor)
                            .is_some_and(|(before, now)| before.eq_ignore_ascii_case(now))
                            && same_issue
                            && EventData::new(&current).command() == command,
                    )
                }
                None => None,
            }
        } else {
            Some(false)
        };
        let delegation = match (assigned, live_command) {
            (Some(true), _) | (_, Some(true)) => Delegation::Current,
            (Some(false), Some(false)) => Delegation::Absent,
            _ => Delegation::Unknown,
        };

        let repository = repository(&github.repository).map_err(Refused::Failed)?;
        let mut unresolved_blockers = Vec::new();
        for page in 1..=PAGES {
            let response = self
                .request(
                    reqwest::Method::GET,
                    integration,
                    &format!("repos/{repository}/issues/{number}/dependencies/blocked_by?per_page={PER_PAGE}&page={page}"),
                )
                .await?
                .send()
                .await
                .map_err(|error| Refused::Failed(anyhow!("the blockers on {work_item} could not be read: {error}")))?;
            let blockers: Vec<serde_json::Value> =
                answered(response, &format!("the blockers on {work_item}")).await?;
            for blocker in &blockers {
                let url = blocker
                    .get("html_url")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| {
                        Refused::Failed(anyhow!("a blocker on {work_item} has no URL"))
                    })?;
                evidence.push(url.to_owned());
                match blocker.get("state").and_then(serde_json::Value::as_str) {
                    Some("open") => unresolved_blockers.push(url.to_owned()),
                    Some("closed") => {}
                    _ => {
                        return Err(Refused::Failed(anyhow!(
                            "a blocker on {work_item} has unknown state"
                        )));
                    }
                }
            }
            if blockers.len() < PER_PAGE {
                return Ok(Readiness {
                    work_item,
                    state,
                    delegation,
                    unresolved_blockers,
                    evidence,
                });
            }
        }
        Err(Refused::Failed(anyhow!(
            "{work_item} has more blockers than one readiness check reads"
        )))
    }

    async fn request(
        &self,
        method: reqwest::Method,
        integration: &Integration,
        path: &str,
    ) -> Result<reqwest::RequestBuilder, Refused> {
        let github = integration.github().map_err(Refused::Failed)?;
        let token = self.token(integration, github).await?;

        Ok(self
            .client
            .request(
                method,
                format!("{}/{path}", github.api.trim_end_matches('/')),
            )
            .header("accept", "application/vnd.github+json")
            .header("x-github-api-version", VERSION)
            .bearer_auth(token))
    }
}

#[derive(Serialize)]
struct AppClaims {
    iat: i64,
    exp: i64,
    iss: i64,
}

/// Signed fresh for every mint: GitHub accepts an App JWT for ten minutes at most, well short
/// of the hour an installation token lasts, so nothing is gained by caching the JWT itself.
fn app_jwt(app: &App) -> Result<String> {
    let now = Timestamp::now().as_second();
    let claims = AppClaims {
        // A few seconds behind the control plane's own clock, as GitHub's docs ask, so a
        // JWT is never rejected for arriving at a moment that, to GitHub, is still its past.
        iat: now - 60,
        exp: now + 540,
        iss: app.id,
    };
    let private_key = app
        .private_key()
        .context("the github app's private key was erased when its integration was retired")?;
    let key = jsonwebtoken::EncodingKey::from_rsa_pem(private_key.as_bytes())
        .context("a github app's private key does not read as PEM")?;

    jsonwebtoken::encode(
        &jsonwebtoken::Header::new(jsonwebtoken::Algorithm::RS256),
        &claims,
        &key,
    )
    .context("a github app jwt could not be signed")
}

#[derive(Deserialize)]
struct MintedToken {
    token: String,
    expires_at: Timestamp,
}

#[derive(Deserialize)]
struct AppRecord {
    slug: String,
}

async fn answered<T: DeserializeOwned>(response: Response, asked_for: &str) -> Result<T, Refused> {
    let status = response.status();
    if let Some(until) = rate_limited(status, response.headers()) {
        return Err(Refused::RateLimited { until });
    }
    if !status.is_success() {
        return Err(Refused::Failed(anyhow!(
            "github answered {status} for {asked_for}"
        )));
    }

    response.json().await.map_err(|error| {
        Refused::Failed(anyhow!(
            "github's account of {asked_for} could not be read: {error}"
        ))
    })
}

/// Both of the ways GitHub says to come back later: the reset moment on an exhausted quota,
/// and the seconds a secondary limit asks for.
fn rate_limited(status: StatusCode, headers: &HeaderMap) -> Option<Timestamp> {
    if status != StatusCode::FORBIDDEN && status != StatusCode::TOO_MANY_REQUESTS {
        return None;
    }

    if let Some(seconds) = number(headers.get("retry-after")) {
        return Some(Timestamp::now() + jiff::SignedDuration::from_secs(seconds.max(0)));
    }
    if number(headers.get("x-ratelimit-remaining")) == Some(0) {
        return number(headers.get("x-ratelimit-reset"))
            .and_then(|reset| Timestamp::from_second(reset).ok());
    }

    None
}

fn number(header: Option<&HeaderValue>) -> Option<i64> {
    header?.to_str().ok()?.trim().parse().ok()
}

fn next_page(headers: &HeaderMap) -> Option<String> {
    headers
        .get("link")?
        .to_str()
        .ok()?
        .split(',')
        .find(|link| link.contains("rel=\"next\""))?
        .split(';')
        .next()?
        .trim()
        .strip_prefix('<')?
        .strip_suffix('>')
        .map(str::to_owned)
}

pub struct EventData<'a> {
    occurrence: &'a Occurrence,
}

impl<'a> EventData<'a> {
    pub fn new(occurrence: &'a Occurrence) -> Self {
        Self { occurrence }
    }

    pub fn actor(&self) -> Option<&'a str> {
        self.field(&["actor", "login"])
            .or_else(|| self.field(&["user", "login"]))
            .or_else(|| self.field(&["sender", "login"]))
            .or_else(|| self.field(&["comment", "user", "login"]))
            .and_then(serde_json::Value::as_str)
    }

    /// GitHub's word for the author's standing in the repository, where an Event carries one.
    pub fn association(&self) -> Option<&str> {
        self.field(&["author_association"])
            .or_else(|| self.field(&["comment", "author_association"]))
            .or_else(|| self.field(&["issue", "author_association"]))
            .and_then(serde_json::Value::as_str)
    }

    pub fn label(&self) -> Option<&str> {
        self.field(&["label", "name"])
            .and_then(serde_json::Value::as_str)
    }

    pub fn labels(&self) -> impl Iterator<Item = &str> {
        self.field(&["issue", "labels"])
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|label| label.get("name").and_then(serde_json::Value::as_str))
    }

    pub fn title(&self) -> Option<&str> {
        self.field(&["issue", "title"])
            .and_then(serde_json::Value::as_str)
    }

    pub fn url(&self) -> Option<&str> {
        self.field(&["html_url"])
            .or_else(|| self.field(&["issue", "html_url"]))
            .and_then(serde_json::Value::as_str)
    }

    pub fn message(&self) -> Option<&'a str> {
        self.field(&["body"])
            .or_else(|| self.field(&["comment", "body"]))
            .and_then(serde_json::Value::as_str)
    }

    /// Exactly what a filter's `prefix` on the body sees, so the two never disagree about a command.
    pub fn command(&self) -> Option<Command<'a>> {
        if self.occurrence.r#type != COMMENTED {
            return None;
        }
        let rest = self.message()?.strip_prefix(MENTION)?;
        if rest.starts_with(|character: char| !character.is_whitespace()) {
            return None;
        }

        let rest = rest.trim_start();
        let (agent, rest) = match rest.strip_prefix(AGENT) {
            Some(named) => {
                let end = named.find(char::is_whitespace).unwrap_or(named.len());
                (Some(&named[..end]), named[end..].trim_start())
            }
            None => (None, rest),
        };
        let instruction = rest.trim_end();

        Some(Command {
            agent,
            instruction: (!instruction.is_empty()).then_some(instruction),
        })
    }

    pub fn subject_issue(&self) -> Option<i64> {
        self.occurrence
            .subject
            .as_deref()
            .and_then(|subject| subject.strip_prefix('#'))
            .and_then(|number| number.parse().ok())
    }

    fn field(&self, path: &[&str]) -> Option<&'a serde_json::Value> {
        let mut at = &self.occurrence.data;
        for part in path {
            at = at.get(*part)?;
        }
        Some(at)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Command<'a> {
    pub agent: Option<&'a str>,
    pub instruction: Option<&'a str>,
}

/// Whether the Event records what the Integration's own identity said, by author rather than
/// by provenance markers. Its voice feeds no Session and fires no Trigger, whatever a Trigger's
/// filter admits (ADR-0028).
pub fn authored_by_own(integration: &Integration, occurrence: &Occurrence) -> bool {
    let Ok(github) = integration.github() else {
        return false;
    };

    EventData::new(occurrence)
        .actor()
        .is_some_and(|actor| actor.eq_ignore_ascii_case(&github.bot_login))
}

/// What an operator's dispatch of `issue` mints: about the issue, as anything GitHub said of it is.
pub fn dispatched(
    github: &GithubConnection,
    r#type: &str,
    issue: i64,
    data: serde_json::Value,
) -> Occurrence {
    Occurrence {
        id: uuid::Uuid::now_v7().to_string(),
        source: source(github),
        specversion: "1.0".to_owned(),
        r#type: r#type.to_owned(),
        subject: Some(format!("#{issue}")),
        time: Timestamp::now(),
        data,
    }
}

/// Its `guid` is the Event's id whether it arrived by webhook or was read from the Delivery log.
pub fn delivered(
    github: &GithubConnection,
    event: &str,
    guid: &str,
    payload: serde_json::Value,
    time: Timestamp,
) -> Result<Occurrence> {
    if let Some(named) = payload
        .get("repository")
        .and_then(|repository| repository.get("full_name"))
        .and_then(serde_json::Value::as_str)
        && !named.eq_ignore_ascii_case(&github.repository)
    {
        bail!(
            "the delivery is about {named}, and this integration watches {}",
            github.repository
        );
    }
    let action = payload.get("action").and_then(serde_json::Value::as_str);
    let subject = payload
        .get("issue")
        .or_else(|| payload.get("pull_request"))
        .and_then(|issue| issue.get("number"))
        .and_then(serde_json::Value::as_i64)
        .map(|number| format!("#{number}"));

    Ok(Occurrence {
        id: guid.to_owned(),
        source: source(github),
        specversion: "1.0".to_owned(),
        r#type: match action {
            Some(action) => format!("com.github.{event}.{action}"),
            None => format!("com.github.{event}"),
        },
        subject,
        time,
        data: payload,
    })
}

/// The external resource the event is about (ADR-0011): the repository, never the
/// integration, so an event dedups identically however kestrel learned it.
pub fn source(github: &GithubConnection) -> String {
    format!("https://github.com/{}", github.repository)
}

/// `owner/name`, checked here because it is pasted into a URL rather than sent as a parameter.
pub fn repository(repository: &str) -> Result<String> {
    let unnamed = || {
        Declined::Unacceptable(format!(
            "{repository} is not a github repository: name it owner/name"
        ))
    };
    let (owner, name) = repository.split_once('/').ok_or_else(unnamed)?;

    for part in [owner, name] {
        if part.is_empty()
            || !part
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || "._-".contains(character))
        {
            bail!(unnamed());
        }
    }

    Ok(format!("{owner}/{name}"))
}

pub fn names(url: &str, repository: &str) -> bool {
    named_repository(url).is_some_and(|named| named.eq_ignore_ascii_case(repository))
}

/// The `owner/name` a github.com URL names, in any of the forms git clones it by.
pub fn named_repository(url: &str) -> Option<String> {
    let named = [
        "https://github.com/",
        "http://github.com/",
        "ssh://git@github.com/",
        "git@github.com:",
        "git://github.com/",
    ]
    .iter()
    .find_map(|prefix| {
        url.get(..prefix.len())
            .filter(|scheme_and_host| scheme_and_host.eq_ignore_ascii_case(prefix))
            .map(|_| &url[prefix.len()..])
    })?
    .trim_end_matches('/');

    repository(named.strip_suffix(".git").unwrap_or(named)).ok()
}

/// One comment as GitHub reports it, and what it answers a newly posted one with.
#[derive(Debug, Deserialize)]
pub struct Comment {
    pub html_url: String,
    #[serde(default)]
    body: String,
}

#[derive(Serialize)]
struct Body<'a> {
    body: &'a str,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for (name, value) in pairs {
            headers.insert(
                reqwest::header::HeaderName::from_bytes(name.as_bytes()).expect("a header name"),
                HeaderValue::from_str(value).expect("a header value"),
            );
        }

        headers
    }

    #[test]
    fn an_exhausted_quota_says_when_it_resets() {
        let until = rate_limited(
            StatusCode::FORBIDDEN,
            &headers(&[
                ("x-ratelimit-remaining", "0"),
                ("x-ratelimit-reset", "1789000000"),
            ]),
        );

        assert_eq!(until, Timestamp::from_second(1_789_000_000).ok());
    }

    #[test]
    fn a_secondary_limit_asks_for_seconds_rather_than_a_moment() {
        let until = rate_limited(
            StatusCode::TOO_MANY_REQUESTS,
            &headers(&[("retry-after", "60")]),
        )
        .expect("a retry-after is a rate limit");

        assert!(until > Timestamp::now() + jiff::SignedDuration::from_secs(50));
    }

    #[test]
    fn a_forbidden_answer_with_quota_left_is_a_failure_rather_than_a_rate_limit() {
        assert_eq!(
            rate_limited(
                StatusCode::FORBIDDEN,
                &headers(&[("x-ratelimit-remaining", "4999")])
            ),
            None
        );
    }

    #[test]
    fn a_repository_is_named_by_any_url_git_clones_it_by() {
        for url in [
            "https://github.com/jtmthf/kestrel",
            "https://github.com/jtmthf/kestrel.git",
            "https://github.com/jtmthf/kestrel/",
            "git@github.com:jtmthf/kestrel.git",
            "ssh://git@github.com/jtmthf/kestrel",
            "HTTPS://GitHub.com/jtmthf/kestrel",
        ] {
            assert_eq!(
                named_repository(url).as_deref(),
                Some("jtmthf/kestrel"),
                "{url}"
            );
        }
        assert_eq!(named_repository("/tmp/jtmthf/kestrel"), None);
        assert_eq!(named_repository("https://gitlab.com/jtmthf/kestrel"), None);
        assert_eq!(
            named_repository("https://github.com/jtmthf/kestrel/pull/7"),
            None
        );
    }

    #[test]
    fn a_repository_is_owner_and_name() {
        assert_eq!(repository("jtmthf/kestrel").unwrap(), "jtmthf/kestrel");
        assert!(repository("kestrel").is_err());
        assert!(repository("jtmthf/kestrel/issues").is_err());
        assert!(repository("../../secrets").is_err());
        assert!(repository("jtmthf/").is_err());
    }

    fn watching() -> GithubConnection {
        GithubConnection {
            repository: "jtmthf/kestrel".to_owned(),
            api: API.to_owned(),
            credential: App::held(1, 2, "not a pem, but nothing here signs with it"),
            bot_login: "kestrel[bot]".to_owned(),
            interval: jiff::SignedDuration::from_secs(60),
            repository_id: 1,
        }
    }

    #[test]
    fn a_delivery_is_named_in_the_webhook_vocabulary() {
        let payload = serde_json::json!({
            "action": "labeled",
            "label": { "name": "ready-for-agent" },
            "issue": { "number": 43 },
            "repository": { "full_name": "jtmthf/kestrel" },
            "sender": { "login": "jtmthf" }
        });

        let at: Timestamp = "2026-10-06T02:48:04.272Z".parse().unwrap();
        let occurrence = delivered(&watching(), "issues", "d-1", payload, at).unwrap();

        assert_eq!(occurrence.r#type, LABELLED);
        assert_eq!(occurrence.id, "d-1");
        assert_eq!(occurrence.time, at);
        assert_eq!(occurrence.source, "https://github.com/jtmthf/kestrel");
        assert_eq!(occurrence.subject.as_deref(), Some("#43"));
        assert_eq!(EventData::new(&occurrence).actor(), Some("jtmthf"));
    }

    #[test]
    fn a_comment_is_identified_by_its_delivery_as_every_event_is() {
        let payload = serde_json::json!({
            "action": "created",
            "comment": { "id": 99, "body": "please add a test" },
            "issue": { "number": 43 }
        });

        let occurrence = delivered(
            &watching(),
            "issue_comment",
            "d-2",
            payload,
            Timestamp::now(),
        )
        .unwrap();

        assert_eq!(occurrence.r#type, COMMENTED);
        assert_eq!(occurrence.id, "d-2");
    }

    #[test]
    fn a_comment_kestrel_left_is_recorded_like_any_other() {
        let payload = serde_json::json!({
            "action": "created",
            "comment": { "id": 99, "body": "done\n<!-- kestrel session 1 -->" },
            "issue": { "number": 43 }
        });

        let occurrence = delivered(
            &watching(),
            "issue_comment",
            "d-3",
            payload,
            Timestamp::now(),
        )
        .unwrap();

        assert_eq!(occurrence.r#type, COMMENTED);
    }

    #[test]
    fn the_integration_recognises_its_own_voice_by_author() {
        let mut watched = watching();
        watched.bot_login = "kestrel[bot]".to_owned();
        let integration = Integration {
            id: crate::domain::IntegrationId::generate(),
            organization: crate::domain::OrganizationId::generate(),
            name: "github".to_owned(),
            connection: crate::domain::Connection::Github(watched),
            carries: vec![crate::domain::Direction::Inbound],
            state: crate::domain::IntegrationState::Enabled,
            revision: 1,
            disabled_at: None,
            retired_at: None,
            canceled_posts: Vec::new(),
            poll_due_at: None,
            deliveries_read_from: None,
            last_polled_at: None,
            last_event_refusal: None,
        };
        for shape in [
            serde_json::json!({ "actor": { "login": "Kestrel[Bot]" } }),
            serde_json::json!({ "user": { "login": "Kestrel[Bot]" } }),
            serde_json::json!({ "sender": { "login": "Kestrel[Bot]" } }),
            serde_json::json!({ "comment": { "user": { "login": "Kestrel[Bot]" } } }),
        ] {
            let mut occurrence = commented("done as the work asked");
            occurrence.data = shape.clone();

            assert!(
                authored_by_own(&integration, &occurrence),
                "the shape {shape} was not recognised"
            );
        }

        assert!(!authored_by_own(
            &integration,
            &commented("said by someone else")
        ));
    }

    fn commented(body: &str) -> Occurrence {
        delivered(
            &watching(),
            "issue_comment",
            "d-5",
            serde_json::json!({
                "action": "created",
                "comment": {
                    "id": 5,
                    "body": body,
                    "user": { "login": "jtmthf" },
                    "author_association": "MEMBER",
                },
                "issue": { "number": 43, "author_association": "NONE" },
                "sender": { "login": "jtmthf" },
            }),
            Timestamp::now(),
        )
        .expect("a comment")
    }

    #[test]
    fn a_comment_is_judged_by_its_author_rather_than_the_issues() {
        let occurrence = commented("@kestrel go");

        assert_eq!(EventData::new(&occurrence).association(), Some("MEMBER"));
    }

    fn command(body: &str) -> Option<(Option<String>, Option<String>)> {
        let occurrence = commented(body);
        EventData::new(&occurrence).command().map(|command| {
            (
                command.agent.map(str::to_owned),
                command.instruction.map(str::to_owned),
            )
        })
    }

    #[test]
    fn a_comment_opening_with_the_mention_is_a_command() {
        let some = |text: &str| Some(text.to_owned());

        assert_eq!(command("@kestrel"), Some((None, None)));
        assert_eq!(
            command("@kestrel /implement\nthen open a PR\n"),
            Some((None, some("/implement\nthen open a PR")))
        );
        assert_eq!(
            command("@kestrel agent=codex $tdd the parser"),
            Some((some("codex"), some("$tdd the parser")))
        );
        assert_eq!(
            command("@kestrel agent=claude"),
            Some((some("claude"), None))
        );
    }

    #[test]
    fn a_passing_mention_commands_nothing() {
        assert_eq!(command("thanks @kestrel"), None);
        assert_eq!(command("@kestrels are birds"), None);
        assert_eq!(command("@kestrel-bot can you look?"), None);
        assert_eq!(command(" @kestrel go"), None);
        assert_eq!(command("@Kestrel go"), None);
        assert_eq!(command("please add a test"), None);
    }

    #[test]
    fn only_a_comment_can_be_a_command() {
        let mut occurrence = commented("@kestrel /implement");
        occurrence.r#type = LABELLED.to_owned();

        assert_eq!(EventData::new(&occurrence).command(), None);
    }

    #[test]
    fn a_delivery_about_another_repository_is_refused() {
        let payload = serde_json::json!({
            "action": "labeled",
            "repository": { "full_name": "someone/else" }
        });

        assert!(delivered(&watching(), "issues", "d-4", payload, Timestamp::now()).is_err());
    }

    #[test]
    fn the_next_page_of_a_cursor_list_is_read_from_its_link() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "link",
            HeaderValue::from_static(
                "<https://api.github.com/app/hook/deliveries?per_page=100&cursor=v1_12077215967>; rel=\"next\"",
            ),
        );

        assert_eq!(
            next_page(&headers).as_deref(),
            Some("https://api.github.com/app/hook/deliveries?per_page=100&cursor=v1_12077215967")
        );
        assert_eq!(next_page(&HeaderMap::new()), None);
    }
}
