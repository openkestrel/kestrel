use anyhow::{Context as _, Result, anyhow, bail};
use tracing::warn;

use crate::domain::{
    Event, EventRecordId, Integration, PullRequest, PullRequestState, Workspace, WorkspaceId,
    WorkspaceState,
};
use crate::integration::github::{self, Github};
use crate::log::Entry;
use crate::store::Store;
use crate::store::Tx;
use crate::store::pull_request::Considered;

const AT_A_TIME: usize = 32;
const LEARNED: &[&str] = &[
    github::PULL_REQUEST_OPENED,
    github::PULL_REQUEST_REOPENED,
    github::PULL_REQUEST_CLOSED,
    github::PULL_REQUEST_SYNCHRONIZE,
];

pub struct Learned {
    pub event: EventRecordId,
    pub workspace: Option<WorkspaceId>,
}

/// Independent of the Triggers: learning a pull request is never a Firing, so it prompts no
/// Session, while a Trigger declared for the same Event still fires for it.
pub async fn learn(store: &Store, github: &Github) -> Result<Vec<Learned>> {
    let events = {
        let mut tx = store.read().await?;
        tx.pull_requests().unconsidered(LEARNED, AT_A_TIME).await?
    };

    let mut learned = Vec::with_capacity(events.len());
    for event in events {
        learned.push(learning(store, github, &event).await?);
    }

    Ok(learned)
}

async fn learning(store: &Store, github: &Github, event: &Event) -> Result<Learned> {
    let observed = match Observed::of(event) {
        Ok(observed) => Some(observed),
        Err(error) => {
            warn!(event = %event.record_id, %error, "a pull request event describes no pull request");
            None
        }
    };
    let reconciled = match &observed {
        Some(observed) => reconciling(store, github, event, observed)
            .await
            .unwrap_or_else(|error| {
                warn!(event = %event.record_id, %error, "a tied pull request event could not be settled from its repository");
                None
            }),
        None => None,
    };

    let mut tx = store.begin().await?;
    let reconciled = match reconciled {
        Some((integration, reconciled)) => tx
            .integrations()
            .unchanged(&integration)
            .await?
            .then_some(reconciled),
        None => None,
    };
    let matched = match &observed {
        Some(observed) => matching(&mut tx, event, observed).await?,
        None => Vec::new(),
    };
    let open = matched
        .iter()
        .filter(|(workspace, _)| workspace.state == WorkspaceState::Open)
        .collect::<Vec<_>>();
    let sealed = matched
        .iter()
        .any(|(workspace, _)| workspace.state == WorkspaceState::Sealed);

    let considered = match (observed.as_ref(), open.as_slice()) {
        (Some(observed), [(workspace, repository)]) => {
            let action = observed_action(event);
            let learned = observed
                .clone()
                .into_pull_request(repository.clone(), event.record_id);
            if tx
                .pull_requests()
                .observe(workspace, &learned, &action)
                .await?
            {
                tx.log()
                    .append(
                        workspace,
                        Entry::PullRequest {
                            event: event.record_id,
                            repository: learned.repository.clone(),
                            number: learned.number,
                            url: learned.url.clone(),
                            title: learned.title.clone(),
                            action,
                            state: learned.state,
                        },
                    )
                    .await?;
                match &reconciled {
                    Some(reconciled) => tx.pull_requests().reconcile(workspace, reconciled).await?,
                    None => tx.pull_requests().learn(workspace, &learned).await?,
                }
            }
            Considered::Attached(workspace.id)
        }
        (Some(_), []) if sealed => Considered::Sealed,
        (Some(_), []) => Considered::Unmatched,
        (Some(_), _) => Considered::Ambiguous,
        (None, _) => Considered::Unmatched,
    };
    let candidates = matched
        .iter()
        .map(|(workspace, _)| (workspace.id, workspace.state))
        .collect::<Vec<_>>();
    tx.pull_requests()
        .consider(event, &considered, &candidates)
        .await?;
    tx.commit().await?;

    Ok(Learned {
        event: event.record_id,
        workspace: match considered {
            Considered::Attached(workspace) => Some(workspace),
            Considered::Sealed | Considered::Unmatched | Considered::Ambiguous => None,
        },
    })
}

async fn matching(
    tx: &mut Tx<'_>,
    event: &Event,
    observed: &Observed,
) -> Result<Vec<(Workspace, String)>> {
    let mut matched = Vec::new();
    for workspace in tx
        .pull_requests()
        .on_branch(event.organization, &observed.head_branch)
        .await?
    {
        let repository = workspace
            .checkout
            .repositories
            .iter()
            .find(|url| github::names(url, &observed.head_repository))
            .cloned();
        if let Some(repository) = repository {
            matched.push((workspace, repository));
        }
    }

    Ok(matched)
}

/// Two deliveries that tie on source freshness but disagree are settled by the repository itself,
/// never by which arrived first.
async fn reconciling(
    store: &Store,
    github: &Github,
    event: &Event,
    observed: &Observed,
) -> Result<Option<(Integration, PullRequest)>> {
    let Some(integration) = event.integration else {
        return Ok(None);
    };

    let mut read = store.read().await?;
    let matched = matching(&mut read, event, observed).await?;
    let open = matched
        .iter()
        .filter(|(workspace, _)| workspace.state == WorkspaceState::Open)
        .collect::<Vec<_>>();
    let [(workspace, repository)] = open.as_slice() else {
        return Ok(None);
    };
    let Some(held) = read
        .pull_requests()
        .value(workspace.id, &observed.url)
        .await?
    else {
        return Ok(None);
    };
    if held.updated_at != observed.updated_at || !observed.conflicts_with(&held) {
        return Ok(None);
    }
    let integration = read.integrations().with_id(integration).await?;
    drop(read);
    if integration.disabled() {
        return Ok(None);
    }

    let pull_request = github
        .pull_request(&integration, observed.number)
        .await
        .map_err(|refused| {
            anyhow!(
                "the pull request {} could not be read back: {refused}",
                observed.url
            )
        })?;
    let reconciled = Observed::parse(&pull_request)?;
    if reconciled.number != observed.number || reconciled.url != observed.url {
        bail!(
            "github answered with pull request {} at {}, not {}",
            reconciled.number,
            reconciled.url,
            observed.url
        );
    }

    Ok(Some((
        integration,
        reconciled.into_pull_request(repository.clone(), event.record_id),
    )))
}

fn observed_action(event: &Event) -> String {
    event
        .occurrence
        .r#type
        .rsplit('.')
        .next()
        .unwrap_or_default()
        .to_owned()
}

#[derive(Clone)]
struct Observed {
    head_repository: String,
    head_branch: String,
    head_revision: String,
    number: i64,
    url: String,
    title: String,
    state: PullRequestState,
    updated_at: jiff::Timestamp,
}

impl Observed {
    fn of(event: &Event) -> Result<Self> {
        let pull_request = event
            .occurrence
            .data
            .get("pull_request")
            .context("the event carries no pull request")?;

        Self::parse(pull_request)
    }

    fn parse(pull_request: &serde_json::Value) -> Result<Self> {
        let text = |path: &[&str]| -> Result<String> {
            path.iter()
                .try_fold(pull_request, |at, step| at.get(*step))
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
                .with_context(|| format!("the pull request has no {}", path.join(".")))
        };
        let merged = pull_request
            .get("merged")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);

        Ok(Self {
            head_repository: text(&["head", "repo", "full_name"])?,
            head_branch: text(&["head", "ref"])?,
            head_revision: text(&["head", "sha"])?,
            number: pull_request
                .get("number")
                .and_then(serde_json::Value::as_i64)
                .context("the pull request has no number")?,
            url: text(&["html_url"])?,
            title: text(&["title"])?,
            state: match (text(&["state"])?.as_str(), merged) {
                (_, true) => PullRequestState::Merged,
                ("open", false) => PullRequestState::Open,
                _ => PullRequestState::Closed,
            },
            updated_at: text(&["updated_at"])?
                .parse()
                .context("the pull request's updated_at is not a time")?,
        })
    }

    fn conflicts_with(&self, held: &PullRequest) -> bool {
        held.state != self.state || held.head_revision != self.head_revision
    }

    fn into_pull_request(self, repository: String, event: EventRecordId) -> PullRequest {
        PullRequest {
            repository,
            number: self.number,
            url: self.url,
            title: self.title,
            state: self.state,
            head_branch: self.head_branch,
            head_revision: self.head_revision,
            updated_at: self.updated_at,
            event,
        }
    }
}

/// `known` is `None` when no Integration could deliver them, which is not knowing there are none.
/// One learned is known however it arrived: a fork's pull request is delivered by the watched base.
pub struct Availability {
    pub repository: String,
    pub known: Option<Vec<PullRequest>>,
}

pub async fn availability(store: &Store, workspace: &Workspace) -> Result<Vec<Availability>> {
    let mut tx = store.read().await?;
    let watched = tx.pull_requests().watched(&workspace.organization).await?;
    let learned = tx.pull_requests().of(workspace.id).await?;

    Ok(workspace
        .checkout
        .repositories
        .iter()
        .map(|repository| {
            let known: Vec<_> = learned
                .iter()
                .filter(|pull_request| &pull_request.repository == repository)
                .cloned()
                .collect();
            Availability {
                repository: repository.clone(),
                known: (!known.is_empty()
                    || watched
                        .iter()
                        .any(|watching| github::names(repository, watching)))
                .then_some(known),
            }
        })
        .collect())
}
