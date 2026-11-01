use std::io::IsTerminal as _;

use anyhow::{Context as _, Result};
use clap::Args;
use kestrel_operator_types as wire;

use super::Running;
use super::input::given;
use crate::api::ControlPlane;
use crate::diagnostic::{self, Invocation};
use crate::exit::{Exit, Failed};
use crate::output::Presentation;
use crate::scope::{Derived, Scoping, Source};
use crate::{start, typed, view};

#[derive(Debug, Args)]
pub(crate) struct Start {
    /// The Brief: the initial prompt that tells the Session what to do. `@FILE` reads it from a
    /// file and `-` from standard input
    #[arg(long)]
    brief: Option<String>,
    /// A repository the work happens against; repeat for many. Without it, the Project's, then
    /// the clone's origin
    #[arg(long = "repository", value_name = "URL")]
    repositories: Vec<String>,
    /// The base branch each Workspace's own branch starts from. Without it, the Project's,
    /// then origin's default, then the branch checked out
    #[arg(long)]
    branch: Option<String>,
    /// The Project, declared if missing. Without it, the one declaring the repositories, then
    /// one named for the repository
    #[arg(long)]
    project: Option<String>,
    /// The Agent, declared if missing. Without it, the only one, then one named for its harness
    #[arg(long)]
    agent: Option<String>,
    /// The Harness a declared Agent is driven by. Without it, the Agent's, then the one a saved
    /// sign-in serves, then kestrel's default
    #[arg(long)]
    harness: Option<String>,
    /// The model a declared Agent works with. Without it, its Harness's default
    #[arg(long)]
    model: Option<String>,
    /// The Subscription Profile the Session uses. Without it, the Operator's own
    #[arg(long)]
    profile: Option<String>,
    /// The saved Sign-in Method the Session is handed, withholding the harness's others
    #[arg(long = "sign-in", value_name = "METHOD")]
    sign_in: Option<String>,
    /// A Provider Credential for the Organization to hold, read from the environment variable
    /// of this name; repeat for many
    #[arg(long = "credential", value_name = "VARIABLE")]
    credentials: Vec<String>,
    /// Apply the reviewed plan. Without a terminal nothing is applied without it; it never
    /// supplies a missing value
    #[arg(long, short)]
    yes: bool,
}

pub(crate) async fn run(running: Running<'_>, start: Start) -> Result<()> {
    started(
        running.api,
        running.presentation,
        running.scoping,
        running.invocation,
        start,
    )
    .await
}

/// The plan is reviewed on stderr before anything changes, so stdout carries only what the
/// start reached.
async fn started(
    api: &ControlPlane,
    presentation: &Presentation,
    scoping: Scoping<'_>,
    invocation: &Invocation,
    start: Start,
) -> Result<()> {
    let brief = start.brief.as_deref().map(given).transpose()?;
    let clone =
        start::LocalClone::of(&std::env::current_dir().context("reading the working directory")?);
    // The only Organization is the control plane's to find, so only a named one is sent.
    let organization = match scoping.derive().await? {
        Derived::Scope(scope) if !matches!(scope.source, Source::OnlyOrganization) => {
            Some(scope.organization)
        }
        Derived::Scope(_) | Derived::Unnamed { .. } => None,
    };
    let secrets = start::secrets(&start.credentials, |variable| std::env::var(variable).ok());

    let plan = wire::StartPlan {
        organization,
        project: start.project,
        repositories: (!start.repositories.is_empty()).then_some(start.repositories),
        integrations: None,
        branch: start.branch,
        agent: start.agent,
        harness: start.harness,
        model: start.model,
        profile: start.profile,
        sign_in_method: start.sign_in,
        brief: brief.clone(),
        credentials: (!start.credentials.is_empty()).then(|| {
            start
                .credentials
                .iter()
                .map(|variable| wire::StartCredential {
                    variable: variable.clone(),
                    secret: None,
                })
                .collect()
        }),
        clone: clone.sent(),
    };
    let preview: wire::StartPreview = api.inspect_as(&["starts", "preview"], &plan).await?;

    for line in start::review(&preview) {
        eprintln!("{line}");
    }
    for warning in &preview.warnings {
        eprintln!("warning: {}", diagnostic::inline(warning, invocation));
    }
    let (brief, secrets) = ready(&preview, &clone, brief, secrets, invocation)?;

    if !start.yes {
        if !(std::io::stdin().is_terminal() && std::io::stderr().is_terminal()) {
            return Err(Failed::new(
                Exit::Usage,
                "nothing was applied: review the plan above and run the same command with --yes \
                 to apply it",
            )
            .into());
        }
        if !confirmed()? {
            eprintln!("nothing was applied");
            return Ok(());
        }
    }

    // A refused or lost start is reported, never sent again.
    let started: wire::Started = api
        .post_as(
            &["starts"],
            &start::applied(&preview, &plan, brief, secrets),
        )
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
    if started.completed_initial_setup {
        eprintln!("initial setup is complete");
    }
    typed::show(presentation, &view::STARTED, &started)
}

/// A gap outranks a missing input, which outranks an inspection that failed; each names every
/// other it found. A gap exits as its kind says: a setup prerequisite 78, a conflict 4.
fn ready(
    preview: &wire::StartPreview,
    clone: &start::LocalClone,
    brief: Option<String>,
    secrets: Result<Vec<(String, String)>, Vec<String>>,
    invocation: &Invocation,
) -> Result<(String, Vec<(String, String)>)> {
    if let Some((first, rest)) = preview.gaps.split_first() {
        for gap in rest.iter().chain(&preview.missing) {
            eprintln!("also: {}", diagnostic::inline(gap, invocation));
        }
        return Err(diagnostic::carried(first.clone()).into());
    }

    let mut missing: Vec<(String, String)> = preview
        .missing
        .iter()
        .map(|missing| {
            let field = diagnostic::field(missing).unwrap_or_default();
            let mut because = diagnostic::message(missing).to_owned();
            if field == "repositories"
                && let Some(silent) = clone.silent()
            {
                because = format!("{because}: {silent}");
            }
            (start::flag(field), because)
        })
        .collect();
    let secrets = secrets.unwrap_or_else(|unset| {
        missing.extend(
            unset
                .into_iter()
                .map(|because| ("--credential".to_owned(), because)),
        );
        Vec::new()
    });
    if !missing.is_empty() {
        let mut flags: Vec<&str> = missing.iter().map(|(flag, _)| flag.as_str()).collect();
        flags.dedup();
        let reasons: Vec<String> = missing
            .iter()
            .map(|(flag, because)| format!("{flag}: {because}"))
            .collect();
        return Err(Failed::new(
            Exit::Usage,
            format!(
                "the plan still needs {} ({})",
                flags.join(" and "),
                reasons.join("; ")
            ),
        )
        .into());
    }

    if let Some(unavailable) = preview.unavailable.first()
        && preview.outcome == wire::StartPreviewOutcome::Unavailable
    {
        return Err(Failed::diagnosed(Exit::Unavailable, unavailable.clone()).into());
    }
    match brief {
        Some(brief) => Ok((brief, secrets)),
        None => Err(Failed::new(Exit::Usage, "the plan still needs --brief").into()),
    }
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
