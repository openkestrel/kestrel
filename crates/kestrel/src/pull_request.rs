use anyhow::{Context as _, Result};
use tracing::warn;

use crate::domain::{Event, EventRecordId, PullRequest, PullRequestState, Workspace, WorkspaceId};
use crate::integration::github;
use crate::log::Entry;
use crate::store::Store;
use crate::store::pull_request::Considered;

const AT_A_TIME: usize = 32;
const LEARNED: &[&str] = &[github::PULL_REQUEST_OPENED];

pub struct Learned {
    pub event: EventRecordId,
    pub workspace: Option<WorkspaceId>,
}

/// Independent of the Triggers: learning a pull request is never a Firing, so it prompts no
/// Session, while a Trigger declared for the same Event still fires for it.
pub async fn learn(store: &Store) -> Result<Vec<Learned>> {
    let events = {
        let mut tx = store.read().await?;
        tx.pull_requests().unconsidered(LEARNED, AT_A_TIME).await?
    };

    let mut learned = Vec::with_capacity(events.len());
    for event in events {
        learned.push(learning(store, &event).await?);
    }

    Ok(learned)
}

async fn learning(store: &Store, event: &Event) -> Result<Learned> {
    let mut tx = store.begin().await?;
    let observed = match Observed::of(event) {
        Ok(observed) => Some(observed),
        Err(error) => {
            warn!(event = %event.record_id, %error, "a pull request event describes no pull request");
            None
        }
    };

    let mut matching = Vec::new();
    if let Some(observed) = &observed {
        for workspace in tx
            .pull_requests()
            .open_on_branch(event.organization, &observed.head_branch)
            .await?
        {
            if let Some(repository) = workspace
                .checkout
                .repositories
                .iter()
                .find(|url| github::names(url, &observed.head_repository))
            {
                let repository = repository.clone();
                matching.push((workspace, repository));
            }
        }
    }

    let considered = match (observed, <[_; 1]>::try_from(matching)) {
        (Some(observed), Ok([(workspace, repository)])) => {
            let learned = observed.into_pull_request(repository, event.record_id);
            tx.log()
                .append(
                    &workspace,
                    Entry::PullRequest {
                        event: event.record_id,
                        repository: learned.repository.clone(),
                        number: learned.number,
                        url: learned.url.clone(),
                        title: learned.title.clone(),
                        action: observed_action(event),
                        state: learned.state,
                    },
                )
                .await?;
            tx.pull_requests().learn(&workspace, &learned).await?;
            Considered::Attached(workspace.id)
        }
        (Some(_), Err(matching)) if !matching.is_empty() => Considered::Ambiguous,
        _ => Considered::Unmatched,
    };
    tx.pull_requests().consider(event, &considered).await?;
    tx.commit().await?;

    Ok(Learned {
        event: event.record_id,
        workspace: match considered {
            Considered::Attached(workspace) => Some(workspace),
            Considered::Unmatched | Considered::Ambiguous => None,
        },
    })
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
