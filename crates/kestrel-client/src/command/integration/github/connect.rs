//! `integration github connect`: start the GitHub App manifest flow, show how to reach it, and
//! wait on the flow's own status until it completes, is cancelled, expires or fails.

use std::collections::HashSet;
use std::io::IsTerminal as _;
use std::time::Duration;

use anyhow::{Result, bail};
use clap::Args;
use kestrel_operator_types as wire;
use reqwest::Url;
use serde_json::json;

use super::reach::{self, Surroundings};
use crate::BINARY;
use crate::api::ControlPlane;
use crate::command::Running;
use crate::command::relay::transient;
use crate::diagnostic::Invocation;
use crate::exit::{Exit, Failed};
use crate::shell::quoted;
use crate::{typed, view};

const POLL: Duration = Duration::from_secs(1);

#[derive(Debug, Args)]
#[command(after_help = "\
The setup URL is always printed to stderr on a line of its own. At a terminal with a display it \
is also opened in a browser (BROWSER names the opener); over SSH, or without a display, the \
command prints an `ssh -L` tunnel to run on the machine with the browser, marked as a template \
when the SSH destination is unknown. The browser creates and installs the App; this command only \
waits, and never asks for input. Without --repository the repository is chosen in the browser \
after installation. Ctrl-C stops waiting and leaves the setup open until it expires; --resume \
waits on it again. The connected record goes to stdout.")]
pub(crate) struct Connect {
    /// The name the Integration is referred to by
    #[arg(default_value = "github")]
    name: String,
    /// The repository to connect once the App is installed, as owner/name; omit it to choose one
    /// in the browser
    #[arg(long, value_name = "OWNER/NAME")]
    repository: Option<String>,
    /// Wait on a setup already started, by the flow its resume command names
    #[arg(
        long,
        value_name = "FLOW",
        conflicts_with_all = ["name", "app_organization", "callback_base", "webhook_base"]
    )]
    resume: Option<String>,
    /// Create the private App under this GitHub organization instead of your personal account
    #[arg(long)]
    app_organization: Option<String>,
    /// Browser-reachable loopback origin (defaults to the Client URL the control plane names)
    #[arg(long)]
    callback_base: Option<String>,
    /// Public HTTPS origin of the webhook listener; omit to use the deployment setting
    #[arg(long)]
    webhook_base: Option<String>,
    #[arg(long, env = "KESTREL_GITHUB_API", hide = true)]
    api: Option<String>,
}

pub(super) async fn run(running: Running<'_>, connect: Connect) -> Result<()> {
    let Running {
        api,
        presentation,
        scoping,
        invocation,
        ..
    } = running;
    let organization = scoping.resolve().await?.organization;
    let scope = scope(&organization, invocation);
    let flow = match &connect.resume {
        Some(flow) => flow.clone(),
        None => started(api, &organization, &connect).await?,
    };
    let read: wire::GithubAppFlow = api.get_as(&["github-app", "flows", &flow]).await?;
    eprintln!(
        "Waiting for GitHub setup to finish. Ctrl-C stops waiting; the setup stays open until \
         {}. Resume with:",
        read.expires_at
    );
    let repository = connect.repository.as_deref();
    eprintln!("  {}", resumed(&flow, repository, &scope));
    let connected = waited(api, read, repository).await?;

    typed::show(presentation, &view::GITHUB_CONNECTED, &connected)
}

async fn started(api: &ControlPlane, organization: &str, connect: &Connect) -> Result<String> {
    let client = match &connect.callback_base {
        Some(base) => base.clone(),
        None => {
            let read: wire::Readiness = api
                .get_where_as(&["readiness"], &[("organization", organization)])
                .await?;
            read.client_url
        }
    };
    let started: wire::GithubAppStarted = api
        .post_as(
            &["organizations", organization, "github-app"],
            &wire::GithubAppStart {
                name: connect.name.clone(),
                callback_base: client.clone(),
                webhook_base: connect.webhook_base.clone().map(Some),
                api: connect
                    .api
                    .as_ref()
                    .map(|api| serde_json::from_value(json!(api)))
                    .transpose()?,
                app_organization: connect.app_organization.clone().map(Some),
            },
        )
        .await?;
    reach::shown(
        &started.url,
        &reach::reach(&Surroundings::read(), &Url::parse(&started.url)?),
        std::io::stderr().is_terminal(),
    );
    Ok(started.flow)
}

/// The flags that keep a suggested command on this one's Organization and control plane.
fn scope(organization: &str, invocation: &Invocation) -> String {
    let mut scope = format!(" --organization {}", quoted(organization));
    if invocation.elsewhere {
        scope.push_str(&format!(
            " --control-plane {}",
            quoted(&invocation.control_plane)
        ));
    }
    scope
}

fn resumed(flow: &str, repository: Option<&str>, scope: &str) -> String {
    let repository = repository.map_or_else(String::new, |repository| {
        format!(" --repository {}", quoted(repository))
    });
    format!(
        "{BINARY} integration github connect --resume {}{repository}{scope}",
        quoted(flow)
    )
}

async fn waited(
    api: &ControlPlane,
    read: wire::GithubAppFlow,
    repository: Option<&str>,
) -> Result<wire::GithubAppFlow> {
    use wire::GithubAppFlowPhase as P;
    let path = ["github-app", "flows", read.flow.as_str()].map(str::to_owned);
    let path: Vec<&str> = path.iter().map(String::as_str).collect();
    let finish = [path.as_slice(), &["finish"]].concat();
    let mut said = HashSet::new();
    let mut first = |phase: &'static str| said.insert(phase);
    let mut lost = false;
    let mut read = Some(read);
    loop {
        let flow = match read.take() {
            Some(flow) => flow,
            None => match api.get_as::<wire::GithubAppFlow>(&path).await {
                Ok(flow) => {
                    if std::mem::take(&mut lost) {
                        eprintln!("Reached the control plane again.");
                    }
                    flow
                }
                Err(error) if transient(&error) => {
                    if !std::mem::replace(&mut lost, true) {
                        eprintln!("Lost the control plane; still waiting on the setup…");
                    }
                    tokio::time::sleep(POLL).await;
                    continue;
                }
                Err(error) => return Err(error),
            },
        };
        match flow.phase {
            P::Ready => {}
            P::Exchanging | P::Converted => {
                if first("created") {
                    eprintln!(
                        "GitHub created the App. Install it on the account that owns the \
                         repository."
                    );
                }
            }
            P::SelectRepository => match repository {
                Some(repository) => {
                    if first("finishing") {
                        eprintln!("The App is installed. Connecting {repository}…");
                    }
                    let chosen = wire::GithubAppFinish {
                        repository: repository.to_owned(),
                    };
                    match api.post_as::<wire::GithubAppFlow>(&finish, &chosen).await {
                        Ok(done) if done.phase == P::Completed => return Ok(done),
                        Ok(_) => {}
                        Err(error) if transient(&error) => {}
                        Err(error) => return Err(error),
                    }
                }
                None => {
                    if first("choosing") {
                        eprintln!("The App is installed. Choose its repository in the browser.");
                    }
                }
            },
            P::Completed => return Ok(flow),
            P::Cancelled => bail!(ended(&flow, "was cancelled")),
            P::Expired => bail!(ended(&flow, "expired")),
            P::ConversionFailed => bail!(ended(&flow, "could not create the App")),
        }
        tokio::time::sleep(POLL).await;
    }
}

/// Built from the flow's own state, so a script reads the same diagnostic any refusal carries.
fn ended(flow: &wire::GithubAppFlow, how: &str) -> Failed {
    let message = format!(
        "The GitHub setup for the Integration {} {how}, and connected nothing. {}",
        flow.name, flow.guidance
    );
    let next_steps = vec![wire::Action::ConnectGithubAction(
        wire::ConnectGithubAction {
            action: json!("connect_github"),
            organization: Some(flow.organization.clone()),
        },
    )];
    let reference = flow.name.clone();
    let operation = "connect_github".to_owned();
    let organization = Some(flow.organization.clone());
    let diagnostic = if flow.phase == wire::GithubAppFlowPhase::Expired {
        wire::Diagnostic::ExpiredResourceDiagnostic(wire::ExpiredResourceDiagnostic {
            kind: json!("expired_resource"),
            message,
            field: None,
            context: wire::ExpiredResourceContext {
                operation,
                organization,
                reference,
                resource: wire::Resource::Integration,
            },
            next_steps,
        })
    } else {
        wire::Diagnostic::StateConflictDiagnostic(wire::StateConflictDiagnostic {
            kind: json!("state_conflict"),
            message,
            field: None,
            context: wire::StateConflictContext {
                holding_session: None,
                operation,
                organization,
                reference,
                resource: wire::Resource::Integration,
                state: flow.phase.as_str().to_owned(),
            },
            next_steps,
        })
    };
    Failed::diagnosed(Exit::Rejected, diagnostic)
}
