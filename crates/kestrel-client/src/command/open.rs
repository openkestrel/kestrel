use std::io::{IsTerminal as _, Write};

use anyhow::{Context as _, Result, bail};
use kestrel_operator_types as wire;
use reqwest::Url;
use serde::Serialize;

use super::Running;
use crate::browser::{self, Reach, Surroundings};
use crate::scope::{self, Derived};
use crate::typed;

#[derive(Serialize)]
struct Opened {
    url: String,
    workspace: Option<wire::Workspace>,
}

pub(crate) async fn run(running: Running<'_>, reference: Option<String>) -> Result<()> {
    let Running {
        api,
        scoping,
        presentation,
        url: operator,
        ..
    } = running;
    let organization = match scoping.derive().await? {
        Derived::Scope(scope) => Some(scope.organization),
        Derived::Unnamed { existing } if existing.is_empty() && reference.is_none() => None,
        Derived::Unnamed { existing } if existing.is_empty() => {
            return Err(scope::nothing_to_scope(api).await);
        }
        Derived::Unnamed { existing } => bail!(scope::several(&existing)),
    };
    let query: Vec<_> = organization
        .iter()
        .map(|name| ("organization", name.as_str()))
        .collect();
    let read: wire::Readiness = api.get_where_as(&["readiness"], &query).await?;
    let workspace = match (organization.as_deref(), reference.as_deref()) {
        (Some(organization), Some(reference)) => Some(
            api.get_as::<wire::Workspace>(&[
                "organizations",
                organization,
                "workspaces",
                reference,
            ])
            .await?,
        ),
        (Some(organization), None) => {
            let workspaces: wire::ListWorkspacesResponse = api
                .get_as(&["organizations", organization, "workspaces"])
                .await?;
            if workspaces.is_empty() {
                None
            } else {
                Some(
                    api.get_as::<wire::Workspace>(&[
                        "organizations",
                        organization,
                        "workspaces",
                        "latest",
                    ])
                    .await?,
                )
            }
        }
        _ => None,
    };
    let mut url: Url = read
        .client_url
        .parse()
        .context("reading the configured Client URL")?;
    if let Some(workspace) = &workspace {
        url.path_segments_mut()
            .map_err(|()| anyhow::anyhow!("the Client URL cannot hold a route"))?
            .pop_if_empty()
            .extend([
                "organizations",
                &workspace.organization,
                "workspaces",
                &workspace.id.to_string(),
            ]);
    }
    let opened = Opened {
        url: url.to_string(),
        workspace,
    };
    if presentation.json() {
        return typed::whole(presentation, &opened);
    }
    let mut out = std::io::stdout().lock();
    writeln!(out, "{}", opened.url)?;
    out.flush()?;
    drop(out);
    let reach = browser::reach(&Surroundings::read(), &url, Some(operator));
    match reach {
        Reach::Here if std::io::stdout().is_terminal() => browser::opened(&opened.url),
        Reach::Here => {}
        reach => browser::access(reach),
    }
    Ok(())
}
