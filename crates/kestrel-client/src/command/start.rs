use std::io::IsTerminal as _;

use anyhow::{Context as _, Result};
use clap::Args;
use kestrel_operator_types as wire;

use super::Running;
use super::input::given;
use crate::api::ControlPlane;
use crate::exit::{Exit, Failed};
use crate::output::Presentation;
use crate::scope::{self, Derived, Scoping};
use crate::{start, typed, view};

#[derive(Debug, Args)]
pub(crate) struct Start {
    /// The Brief the Workspace starts with; `@FILE` reads it from a file and `-` from standard
    /// input
    #[arg(long)]
    brief: String,
    /// A repository the work happens against; repeat for many. Without it, the clone's origin
    #[arg(long = "repository", value_name = "URL")]
    repositories: Vec<String>,
    /// The branch the work happens on. Without it, a Project's own, then origin's default,
    /// then the branch checked out
    #[arg(long)]
    branch: Option<String>,
    /// The Project, declared if missing. Without it, the one declaring the repositories, then
    /// the repository's name
    #[arg(long)]
    project: Option<String>,
    /// The Agent, declared if missing. Without it, the only one, then its harness's name
    #[arg(long)]
    agent: Option<String>,
    /// The Harness a declared Agent is driven by. Without it, the declared Agent's, then
    /// kestrel's default
    #[arg(long)]
    harness: Option<String>,
    /// The model a declared Agent works with. Without it, its Harness's default
    #[arg(long)]
    model: Option<String>,
    /// A Provider Credential for the Organization to hold, read from the environment variable
    /// of this name; repeat for many
    #[arg(long = "credential", value_name = "VARIABLE")]
    credentials: Vec<String>,
    /// Apply the plan without asking. A start asks only when standard input and standard error
    /// are both a terminal
    #[arg(long, short)]
    yes: bool,
}

pub(crate) async fn run(running: Running<'_>, start: Start) -> Result<()> {
    started(running.api, running.presentation, running.scoping, start).await
}

/// Every value is explained on stderr before anything changes, so what stdout carries is what
/// the start reached and nothing else.
async fn started(
    api: &ControlPlane,
    presentation: &Presentation,
    scoping: Scoping<'_>,
    mut start: Start,
) -> Result<()> {
    let brief = given(&start.brief)?;
    let mut clone =
        start::LocalClone::of(&std::env::current_dir().context("reading the working directory")?);
    let (named, existing) = match scoping.derive().await? {
        Derived::Scope(scope) => (Some(scope), Vec::new()),
        Derived::Unnamed { existing } if existing.is_empty() => {
            return Err(scope::nothing_to_scope(api).await);
        }
        Derived::Unnamed { existing } => (None, existing),
    };
    let organization =
        start::organization(named, &existing).map_err(|missing| incomplete(&[missing]))?;

    let declared = scope::organizations(api)
        .await?
        .contains(&organization.value);
    let (projects, agents, credentials) = if declared {
        let within = |records| ["organizations", &organization.value, records];
        tokio::try_join!(
            async { api.get_as(&within("projects")).await },
            async { api.get_as(&within("agents")).await },
            async { api.get_as(&within("credentials")).await },
        )?
    } else {
        (Vec::new(), Vec::new(), Vec::new())
    };
    let mut existing = start::Existing::read(declared, projects, agents, credentials);
    let names_a_project = start.project.as_ref().is_some_and(|name| {
        existing
            .projects
            .iter()
            .any(|declared| &declared.name == name)
    });
    // Planning matches declared Projects by address, so it compares what a declaration would hold.
    if !start.repositories.is_empty() {
        start.repositories = resolved(api, start.repositories).await?;
    } else if !names_a_project && let Some(origin) = clone.origin.take() {
        if declared {
            let declaring: wire::ListProjectsResponse = api
                .get_where_as(
                    &["organizations", &organization.value, "projects"],
                    &[("repository", &origin)],
                )
                .await?;
            existing.declaring = Some(declaring.into_iter().map(|record| record.name).collect());
        }
        clone.origin = resolved(api, vec![origin]).await?.pop();
    }

    let secrets = start::secrets(&start.credentials, |variable| std::env::var(variable).ok());
    let plan = start::plan(
        organization,
        start::Given {
            repositories: start.repositories,
            branch: start.branch,
            project: start.project,
            agent: start.agent,
            harness: start.harness,
            model: start.model,
            credentials: start.credentials,
        },
        &clone,
        &existing,
    );
    let (plan, secrets) = match (plan, secrets) {
        (Ok(plan), Ok(secrets)) => (plan, secrets),
        (plan, secrets) => {
            let mut missing = plan.err().unwrap_or_default();
            missing.extend(secrets.err().unwrap_or_default());
            return Err(incomplete(&missing));
        }
    };

    let explained = plan.explained();
    let column = explained
        .iter()
        .map(|row| row.what.len())
        .max()
        .unwrap_or_default();
    eprintln!("starting work with");
    for row in &explained {
        let (what, value, because, flag) = (row.what, &row.value, row.because, row.flag);
        if row.given {
            eprintln!("  {what:column$}  {value}  ({because})");
        } else {
            eprintln!("  {what:column$}  {value}  ({because}; {flag} overrides it)");
        }
    }

    if !start.yes && std::io::stdin().is_terminal() && std::io::stderr().is_terminal() {
        eprintln!("applying it will");
        for step in plan.applying(&existing) {
            eprintln!("  {step}");
        }
        if !confirmed()? {
            eprintln!("nothing was applied");
            return Ok(());
        }
    }

    let started: wire::Started = api
        .post_as(&["starts"], &plan.body(&brief, &secrets))
        .await?;
    for (kind, settled) in [
        ("organization", &started.organization),
        ("project", &started.project),
        ("agent", &started.agent),
    ] {
        if settled.created {
            eprintln!("declared the {kind} {}", settled.name);
        }
    }
    typed::show(presentation, &view::STARTED, &started)
}

async fn resolved(api: &ControlPlane, repositories: Vec<String>) -> Result<Vec<String>> {
    let resolution: wire::RepositoryResolution = api
        .post_as(
            &["repository-resolutions"],
            &wire::RepositoryResolutionRequest::new(repositories),
        )
        .await?;

    Ok(resolution
        .repositories
        .into_iter()
        .map(|repository| repository.address)
        .collect())
}

fn confirmed() -> Result<bool> {
    loop {
        eprint!("apply this plan? [Y/n] ");
        let mut answer = String::new();
        if std::io::stdin()
            .read_line(&mut answer)
            .context("reading the answer")?
            == 0
        {
            eprintln!();
            return Ok(false);
        }
        match answer.trim().to_lowercase().as_str() {
            "" | "y" | "yes" => return Ok(true),
            "n" | "no" => return Ok(false),
            _ => {}
        }
    }
}

fn incomplete(missing: &[start::Missing]) -> anyhow::Error {
    let flags: Vec<&str> = missing.iter().map(|missing| missing.flag).collect();
    let reasons: Vec<String> = missing
        .iter()
        .map(|missing| format!("{}: {}", missing.flag, missing.because))
        .collect();

    Failed::new(
        Exit::Usage,
        format!(
            "nothing says what to start with; pass {} ({})",
            flags.join(" and "),
            reasons.join("; ")
        ),
    )
    .into()
}
