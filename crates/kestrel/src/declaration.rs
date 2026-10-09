use anyhow::{Context as _, Result};
use serde::{Deserialize, Serialize};

use crate::declined::{Constraint, Reason};
use crate::domain::{Correlation, Declared, Fires, Templates, Trigger};
use crate::filter::Filter;
use crate::repository::{self, Purpose};
use crate::store::Store;
use crate::template::Template;
use crate::trigger::allowed;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Document {
    pub project: Project,
    pub agent: Agent,
    pub trigger: TriggerDeclaration,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Project {
    pub name: String,
    pub repositories: Vec<String>,
    pub branch: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Agent {
    pub name: String,
    pub harness: String,
    pub model: Option<String>,
    pub mode: Option<String>,
    pub thought_level: Option<String>,
}

impl Agent {
    pub fn declared(&self) -> Declared {
        Declared::named(Declared {
            model: self.model.clone(),
            mode: self.mode.clone(),
            thought_level: self.thought_level.clone(),
        })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TriggerDeclaration {
    pub name: String,
    pub filter: serde_json::Value,
    pub brief: String,
    pub branch: Option<String>,
    pub correlation: Option<String>,
    pub on_miss: Option<String>,
    pub on_open_workspace: Option<String>,
    pub project: String,
    pub agent: String,
    #[serde(default)]
    pub allows: Vec<String>,
    pub profile: Option<String>,
    pub model: Option<String>,
    pub mode: Option<String>,
    pub thought_level: Option<String>,
}

impl TriggerDeclaration {
    pub fn declared(&self) -> Declared {
        Declared::named(Declared {
            model: self.model.clone(),
            mode: self.mode.clone(),
            thought_level: self.thought_level.clone(),
        })
    }
}

#[derive(Serialize)]
pub struct Applied {
    pub declarations: Vec<Declaration>,
    pub admitting_outsiders: Vec<String>,
}

#[derive(Serialize)]
pub struct Declaration {
    pub kind: Kind,
    pub name: String,
    pub action: Action,
    pub differences: Vec<Difference>,
}

#[derive(Serialize)]
pub struct Difference {
    pub field: &'static str,
    pub was: Option<String>,
    pub becomes: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Project,
    Agent,
    Trigger,
}

#[derive(Serialize, Clone, Copy)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Add,
    Change,
    Unchanged,
}

pub enum ApplyMode {
    Apply,
    Preview,
}

struct ParsedTrigger {
    filter: Filter,
    templates: Templates,
}

struct Compared {
    action: Action,
    differences: Vec<Difference>,
}

pub async fn apply(
    store: &Store,
    organization: &str,
    document: &Document,
    mode: ApplyMode,
) -> Result<Applied> {
    let repositories = check_document(document, &mode)?;
    let parsed = parse_trigger(&document.trigger)?;
    let mut tx = store.begin().await?;
    let organization = tx.organizations().named(organization).await?;

    let projects = tx.projects().all(&organization).await?;
    let agents = tx.agents().all(&organization).await?;
    let triggers = tx.triggers().all(&organization).await?;
    let project_change = project_change(
        projects
            .iter()
            .find(|project| project.name == document.project.name),
        &document.project,
        &repositories,
    );
    let agent_change = agent_change(
        agents
            .iter()
            .find(|agent| agent.name == document.agent.name),
        &document.agent,
    );
    let trigger_change = trigger_change(
        triggers
            .iter()
            .find(|trigger| trigger.name == document.trigger.name),
        &document.trigger,
        &parsed,
    );
    let declarations = vec![
        Declaration {
            kind: Kind::Project,
            name: document.project.name.clone(),
            action: project_change.action,
            differences: project_change.differences,
        },
        Declaration {
            kind: Kind::Agent,
            name: document.agent.name.clone(),
            action: agent_change.action,
            differences: agent_change.differences,
        },
        Declaration {
            kind: Kind::Trigger,
            name: document.trigger.name.clone(),
            action: trigger_change.action,
            differences: trigger_change.differences,
        },
    ];

    let project = tx
        .projects()
        .declare(
            &organization,
            &document.project.name,
            &repositories,
            &document.project.branch,
        )
        .await?
        .record;
    let agent = tx
        .agents()
        .declare(
            &organization,
            &document.agent.name,
            &document.agent.harness,
            &document.agent.declared(),
        )
        .await?
        .record;
    let allows = allowed(&mut tx, &organization, &document.trigger.allows).await?;
    let profile = match &document.trigger.profile {
        Some(profile) => Some(tx.profiles().named(&organization, profile).await?),
        None => None,
    };
    let declared = document.trigger.declared();
    let fires = Fires::On(parsed.filter.clone());
    match declarations[2].action {
        Action::Add => {
            tx.triggers()
                .declare(
                    &organization,
                    &document.trigger.name,
                    &fires,
                    &parsed.templates,
                    &project,
                    &agent,
                    &declared,
                    &allows,
                    profile.as_ref(),
                    true,
                )
                .await?;
        }
        Action::Change => {
            let trigger = triggers
                .iter()
                .find(|trigger| trigger.name == document.trigger.name)
                .expect("a changed trigger was listed");
            tx.triggers()
                .redeclare(
                    trigger,
                    &fires,
                    &parsed.templates,
                    &project,
                    &agent,
                    &declared,
                    &allows,
                    profile.as_ref(),
                    true,
                )
                .await?;
        }
        Action::Unchanged => {}
    }

    if matches!(mode, ApplyMode::Apply) {
        tx.commit().await?;
    }

    Ok(Applied {
        declarations,
        admitting_outsiders: parsed
            .filter
            .admits_outsiders()
            .then(|| document.trigger.name.clone())
            .into_iter()
            .collect(),
    })
}

fn check_document(document: &Document, mode: &ApplyMode) -> Result<Vec<String>> {
    let operation = match mode {
        ApplyMode::Apply => "apply_declaration",
        ApplyMode::Preview => "preview_declaration",
    };
    let invalid = |field: &'static str, constraint: Constraint, message: String| {
        Err(Reason::InvalidField {
            field,
            operation,
            constraint,
            allowed: None,
            message,
        }
        .into())
    };

    for (field, name) in [
        ("project.name", &document.project.name),
        ("agent.name", &document.agent.name),
        ("trigger.name", &document.trigger.name),
    ] {
        if name.is_empty() {
            return invalid(
                field,
                Constraint::NonEmpty,
                "a declaration names each record".to_owned(),
            );
        }
    }
    let repositories = repository::addresses(repository::resolved(
        operation,
        "project.repositories",
        &document.project.repositories,
        Purpose::Declaration,
    )?);
    if document.project.branch.is_empty() {
        return invalid(
            "project.branch",
            Constraint::NonEmpty,
            "a project names the branch its work happens on".to_owned(),
        );
    }
    if document.agent.harness.is_empty() {
        return invalid(
            "agent.harness",
            Constraint::NonEmpty,
            "an agent names the harness that drives it".to_owned(),
        );
    }
    if document.trigger.project != document.project.name {
        return invalid(
            "trigger.project",
            Constraint::MatchesDeclared,
            format!(
                "the trigger names project {}, not the declared project {}",
                document.trigger.project, document.project.name
            ),
        );
    }
    if document.trigger.agent != document.agent.name {
        return invalid(
            "trigger.agent",
            Constraint::MatchesDeclared,
            format!(
                "the trigger names agent {}, not the declared agent {}",
                document.trigger.agent, document.agent.name
            ),
        );
    }
    Ok(repositories)
}

fn parse_trigger(declaration: &TriggerDeclaration) -> Result<ParsedTrigger> {
    let correlation = Correlation::parse(
        declaration.correlation.as_deref(),
        declaration.on_miss.as_deref(),
        declaration.on_open_workspace.as_deref(),
    )?;
    let templates = Templates {
        brief: declaration
            .brief
            .parse::<Template>()
            .context("a trigger brief")?,
        branch: declaration
            .branch
            .as_deref()
            .map(str::parse)
            .transpose()
            .context("a trigger branch")?,
        correlation,
    };

    Ok(ParsedTrigger {
        filter: Filter::from_json(&declaration.filter).context("a trigger filter")?,
        templates,
    })
}

fn project_change(
    project: Option<&crate::domain::Project>,
    declaration: &Project,
    repositories: &[String],
) -> Compared {
    let becomes = vec![
        ("repositories", Some(repositories.join("\n"))),
        ("branch", Some(declaration.branch.clone())),
    ];
    compared(
        project.map(|project| {
            vec![
                ("repositories", Some(project.repositories.join("\n"))),
                ("branch", Some(project.branch.clone())),
            ]
        }),
        becomes,
    )
}

fn agent_change(agent: Option<&crate::domain::Agent>, declaration: &Agent) -> Compared {
    let declared = declaration.declared();
    compared(
        agent.map(|agent| {
            vec![
                ("harness", Some(agent.harness.clone())),
                ("model", agent.declared.model.clone()),
                ("mode", agent.declared.mode.clone()),
                ("thought level", agent.declared.thought_level.clone()),
            ]
        }),
        vec![
            ("harness", Some(declaration.harness.clone())),
            ("model", declared.model),
            ("mode", declared.mode),
            ("thought level", declared.thought_level),
        ],
    )
}

fn trigger_change(
    trigger: Option<&Trigger>,
    declaration: &TriggerDeclaration,
    parsed: &ParsedTrigger,
) -> Compared {
    let mut allows = declaration.allows.clone();
    allows.sort();
    allows.dedup();
    compared(
        trigger.map(described_trigger),
        vec![
            ("matches", Some(parsed.filter.to_string())),
            ("project", Some(declaration.project.clone())),
            ("agent", Some(declaration.agent.clone())),
            ("allows", (!allows.is_empty()).then(|| allows.join(", "))),
            ("profile", declaration.profile.clone()),
            (
                "branch",
                parsed.templates.branch.as_ref().map(ToString::to_string),
            ),
            (
                "correlation",
                parsed
                    .templates
                    .correlation
                    .template()
                    .map(ToString::to_string),
            ),
            (
                "on miss",
                parsed
                    .templates
                    .correlation
                    .on_miss()
                    .map(|miss| miss.to_string()),
            ),
            (
                "on open workspace",
                Some(parsed.templates.correlation.on_open_workspace().to_string()),
            ),
            ("brief", Some(parsed.templates.brief.to_string())),
            ("model", declaration.model.clone()),
            ("mode", declaration.mode.clone()),
            ("thought level", declaration.thought_level.clone()),
        ],
    )
}

fn described_trigger(trigger: &Trigger) -> Vec<(&'static str, Option<String>)> {
    let mut allows = trigger
        .allows
        .iter()
        .map(|agent| agent.name.clone())
        .collect::<Vec<_>>();
    allows.sort();
    allows.dedup();
    vec![
        ("matches", Some(trigger.filter().to_string())),
        ("project", Some(trigger.project.name.clone())),
        ("agent", Some(trigger.agent.name.clone())),
        ("allows", (!allows.is_empty()).then(|| allows.join(", "))),
        (
            "profile",
            trigger.profile.as_ref().map(|profile| profile.name.clone()),
        ),
        (
            "branch",
            trigger.templates.branch.as_ref().map(ToString::to_string),
        ),
        (
            "correlation",
            trigger
                .templates
                .correlation
                .template()
                .map(ToString::to_string),
        ),
        (
            "on miss",
            trigger
                .templates
                .correlation
                .on_miss()
                .map(|miss| miss.to_string()),
        ),
        (
            "on open workspace",
            Some(
                trigger
                    .templates
                    .correlation
                    .on_open_workspace()
                    .to_string(),
            ),
        ),
        ("brief", Some(trigger.templates.brief.to_string())),
        ("model", trigger.declared.model.clone()),
        ("mode", trigger.declared.mode.clone()),
        ("thought level", trigger.declared.thought_level.clone()),
    ]
}

fn compared(
    was: Option<Vec<(&'static str, Option<String>)>>,
    becomes: Vec<(&'static str, Option<String>)>,
) -> Compared {
    let added = was.is_none();
    let differences: Vec<Difference> = was.map_or_else(
        || {
            becomes
                .iter()
                .map(|(field, becomes)| Difference {
                    field,
                    was: None,
                    becomes: becomes.clone(),
                })
                .collect()
        },
        |was| {
            was.into_iter()
                .zip(&becomes)
                .filter(|((_, was), (_, becomes))| was != becomes)
                .map(|((field, was), (_, becomes))| Difference {
                    field,
                    was,
                    becomes: becomes.clone(),
                })
                .collect()
        },
    );
    let action = if added {
        Action::Add
    } else if differences.is_empty() {
        Action::Unchanged
    } else {
        Action::Change
    };

    Compared {
        action,
        differences,
    }
}
