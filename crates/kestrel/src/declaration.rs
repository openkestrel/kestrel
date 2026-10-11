use std::fmt;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::capability::Images;
use crate::cron::Cron;
use crate::declined::{Concerning, Constraint, Reason};
use crate::domain::{
    Correlation, Declared, Fires, FiringBudget, Organization, Schedule, Templates, Trigger,
};
use crate::filter::Filter;
use crate::integration::github::Github;
use crate::repository::{self, Declarable, Declaring, Selecting};
use crate::store::{Declared as DeclaredRecord, Store, Tx};
use crate::template::Template;

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
    #[serde(default)]
    pub integrations: Vec<repository::Selection>,
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

/// The one change vocabulary every declaration path answers in: what was declared, and what
/// writing it did.
#[derive(Debug, Clone, Serialize)]
pub struct Declaration {
    pub kind: Kind,
    pub name: String,
    pub action: Action,
    pub differences: Vec<Difference>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Difference {
    pub field: &'static str,
    pub was: Option<String>,
    pub becomes: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Project,
    Agent,
    Trigger,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Add,
    Change,
    Unchanged,
    Remove,
}

pub enum ApplyMode {
    Apply,
    Preview,
}

impl ApplyMode {
    const fn operation(&self) -> &'static str {
        match self {
            ApplyMode::Apply => "apply_declaration",
            ApplyMode::Preview => "preview_declaration",
        }
    }
}

/// A declaration refusal before it knows its route: the record-relative field, the constraint
/// it broke and why. The caller stamps its operation and prefixes the field.
#[derive(Debug)]
pub struct Invalid {
    pub field: &'static str,
    pub constraint: Constraint,
    pub message: String,
}

impl fmt::Display for Invalid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Invalid {}

pub fn invalid(
    field: &'static str,
    constraint: Constraint,
    message: impl Into<String>,
) -> anyhow::Error {
    Invalid {
        field,
        constraint,
        message: message.into(),
    }
    .into()
}

/// Where a refused field lives: the single-record routes name it bare, the document and a
/// start prefix it with the record.
#[derive(Debug, Clone, Copy)]
pub enum Prefix {
    Root,
    Project,
    Agent,
    Trigger,
}

pub fn prefixed(prefix: Prefix, field: &'static str) -> &'static str {
    match (prefix, field) {
        (Prefix::Root, field) => field,
        (Prefix::Project, "name") => "project.name",
        (Prefix::Project, "repositories") => "project.repositories",
        (Prefix::Project, "integrations") => "project.integrations",
        (Prefix::Project, "branch") => "project.branch",
        (Prefix::Agent, "name") => "agent.name",
        (Prefix::Agent, "harness") => "agent.harness",
        (Prefix::Agent, "model") => "agent.model",
        (Prefix::Agent, "mode") => "agent.mode",
        (Prefix::Agent, "thought level") => "agent.thought level",
        (Prefix::Trigger, "name") => "trigger.name",
        (Prefix::Trigger, "filter") => "trigger.filter",
        (Prefix::Trigger, "every") => "trigger.every",
        (Prefix::Trigger, "cron") => "trigger.cron",
        (Prefix::Trigger, "zone") => "trigger.zone",
        (Prefix::Trigger, "brief") => "trigger.brief",
        (Prefix::Trigger, "branch") => "trigger.branch",
        (Prefix::Trigger, "correlation") => "trigger.correlation",
        (Prefix::Trigger, "on_miss") => "trigger.on_miss",
        (Prefix::Trigger, "on_open_workspace") => "trigger.on_open_workspace",
        (Prefix::Trigger, "project") => "trigger.project",
        (Prefix::Trigger, "agent") => "trigger.agent",
        (Prefix::Trigger, "allows") => "trigger.allows",
        (Prefix::Trigger, "profile") => "trigger.profile",
        (Prefix::Trigger, "model") => "trigger.model",
        (Prefix::Trigger, "mode") => "trigger.mode",
        (Prefix::Trigger, "thought level") => "trigger.thought level",
        (_, field) => field,
    }
}

/// Gives a module error its route: stamps the caller's operation onto an [`Invalid`], prefixes
/// the record-relative field of a reason the module raised with it, and leaves the rest alone.
pub fn scoped(prefix: Prefix, operation: &'static str, error: anyhow::Error) -> anyhow::Error {
    let error = match error.downcast::<Invalid>() {
        Ok(invalid) => {
            return Reason::InvalidField {
                field: prefixed(prefix, invalid.field),
                operation,
                constraint: invalid.constraint,
                allowed: None,
                message: invalid.message,
            }
            .into();
        }
        Err(error) => error,
    };
    let error = match error.downcast::<Reason>() {
        Ok(Reason::InvalidField {
            field,
            constraint,
            allowed,
            message,
            ..
        }) => {
            return Reason::InvalidField {
                field: prefixed(prefix, field),
                operation,
                constraint,
                allowed,
                message,
            }
            .into();
        }
        Ok(reason) => return reason.into(),
        Err(error) => error,
    };
    match error.downcast::<Concerning>() {
        Ok(concerning) => Concerning {
            field: prefixed(prefix, concerning.field),
            reason: concerning.reason,
        }
        .into(),
        Err(error) => error,
    }
}

#[derive(Debug, Clone, Copy)]
pub enum OnExisting {
    Change,
    Refuse,
}

/// What declared a Trigger: flags on the single-record route, or a file on the document and
/// applied-Triggers paths.
#[derive(Debug, Clone, Copy)]
pub enum Provenance {
    Flags,
    File,
}

impl Provenance {
    const fn applied(self) -> bool {
        match self {
            Provenance::Flags => false,
            Provenance::File => true,
        }
    }
}

pub struct CheckedProject {
    pub name: String,
    pub declarable: Declarable,
}

pub struct CheckedAgent {
    pub name: String,
    pub harness: String,
    pub declared: Declared,
}

pub struct CheckedTrigger {
    pub name: String,
    pub fires: Fires,
    pub templates: Templates,
    pub project: String,
    pub agent: String,
    pub declared: Declared,
    pub allows: Vec<String>,
    pub profile: Option<String>,
}

pub struct TriggerProposal {
    pub name: String,
    pub fires: Fires,
    pub templates: Templates,
    pub project: String,
    pub agent: String,
    pub declared: Declared,
    pub allows: Vec<String>,
    pub profile: Option<String>,
}

impl CheckedTrigger {
    pub fn new(proposal: TriggerProposal) -> Self {
        Self {
            name: proposal.name,
            fires: proposal.fires,
            templates: proposal.templates,
            project: proposal.project,
            agent: proposal.agent,
            declared: Declared::named(proposal.declared),
            allows: proposal.allows,
            profile: proposal.profile,
        }
    }
}

/// The schedule half of a Trigger declaration: exactly one of a filter, an interval or a cron
/// expression read in a zone.
pub struct ScheduleForms<'a> {
    pub filter: Option<&'a serde_json::Value>,
    pub every: Option<&'a str>,
    pub cron: Option<&'a str>,
    pub zone: Option<&'a str>,
}

pub struct TemplateFields<'a> {
    pub brief: &'a str,
    pub branch: Option<&'a str>,
    pub correlation: Option<&'a str>,
    pub on_miss: Option<&'a str>,
    pub on_open_workspace: Option<&'a str>,
}

pub fn check_name(name: &str) -> Result<()> {
    if name.is_empty() {
        return Err(invalid(
            "name",
            Constraint::NonEmpty,
            "a declaration names each record",
        ));
    }
    Ok(())
}

pub fn check_branch(branch: &str) -> Result<()> {
    if branch.is_empty() {
        return Err(invalid(
            "branch",
            Constraint::NonEmpty,
            "a project names the branch its work happens on",
        ));
    }
    Ok(())
}

pub fn check_harness(harness: &str) -> Result<()> {
    if harness.is_empty() {
        return Err(invalid(
            "harness",
            Constraint::NonEmpty,
            "an agent names the harness that drives it",
        ));
    }
    Ok(())
}

/// The caller's operation is forwarded to the repository checks; the module's own rules carry
/// no operation.
pub async fn check_project(
    store: &Store,
    github: &Github,
    organization: &str,
    project: &Project,
    operation: &'static str,
) -> Result<CheckedProject> {
    check_name(&project.name)?;
    check_branch(&project.branch)?;
    let declarable = repository::declarable(
        store,
        github,
        &Declaring {
            selecting: Selecting {
                operation,
                repositories_field: "repositories",
                integrations_field: "integrations",
                organization,
                repositories: &project.repositories,
                selections: &project.integrations,
            },
            branch_field: "branch",
            project: &project.name,
            branch: &project.branch,
        },
    )
    .await?;
    Ok(CheckedProject {
        name: project.name.clone(),
        declarable,
    })
}

pub async fn check_agent(
    images: &Images,
    name: &str,
    harness: &str,
    declared: &Declared,
    operation: &'static str,
) -> Result<CheckedAgent> {
    check_name(name)?;
    check_harness(harness)?;
    images.admit(harness, operation, "harness").await?;
    Ok(CheckedAgent {
        name: name.to_owned(),
        harness: harness.to_owned(),
        declared: Declared::named(declared.clone()),
    })
}

/// A document Trigger names the Project and Agent the same document declares.
pub fn check_trigger_links(
    trigger_project: &str,
    trigger_agent: &str,
    project: &str,
    agent: &str,
) -> Result<()> {
    if trigger_project != project {
        return Err(invalid(
            "project",
            Constraint::MatchesDeclared,
            format!(
                "the trigger names project {trigger_project}, not the declared project {project}"
            ),
        ));
    }
    if trigger_agent != agent {
        return Err(invalid(
            "agent",
            Constraint::MatchesDeclared,
            format!("the trigger names agent {trigger_agent}, not the declared agent {agent}"),
        ));
    }
    Ok(())
}

/// The one Trigger parser: a filter, an interval or a cron expression, and only one, with the
/// firing-budget check wherever a schedule arrives.
pub fn parse_trigger(
    forms: &ScheduleForms<'_>,
    fields: &TemplateFields<'_>,
) -> Result<(Fires, Templates)> {
    let fires = parse_fires(forms)?;
    check_budget(&fires)?;
    Ok((fires.clone(), parse_templates(fields)?))
}

/// The document and the applied-Triggers file declare event Triggers only; a schedule in their
/// shape is refused by their schema, and what arrives here meets the same template rules.
pub fn parse_filter_trigger(
    filter: &serde_json::Value,
    fields: &TemplateFields<'_>,
) -> Result<(Filter, Templates)> {
    let filter = Filter::from_json(filter).map_err(|error| {
        invalid(
            "filter",
            Constraint::WellFormed,
            format!("a trigger filter: {error}"),
        )
    })?;
    Ok((filter, parse_templates(fields)?))
}

fn parse_fires(forms: &ScheduleForms<'_>) -> Result<Fires> {
    match (forms.filter, forms.every, forms.cron, forms.zone) {
        (Some(filter), None, None, None) => {
            Ok(Fires::On(Filter::from_json(filter).map_err(|error| {
                invalid(
                    "filter",
                    Constraint::WellFormed,
                    format!("a trigger filter: {error}"),
                )
            })?))
        }
        (None, Some(every), None, None) => Ok(Fires::Scheduled(Schedule::Every(
            every.parse().map_err(|error| {
                invalid(
                    "every",
                    Constraint::WellFormed,
                    format!("a trigger schedule is a duration: {error}"),
                )
            })?,
        ))),
        (None, None, Some(cron), Some(zone)) => Ok(Fires::Scheduled(Schedule::Cron(
            Cron::new(cron, zone).map_err(|error| {
                invalid(
                    "cron",
                    Constraint::WellFormed,
                    format!("a trigger schedule: {error:#}"),
                )
            })?,
        ))),
        (None, None, Some(_), None) => Err(invalid(
            "zone",
            Constraint::WellFormed,
            "a cron expression declares the time zone it is read in",
        )),
        (_, _, None, Some(_)) => Err(invalid(
            "zone",
            Constraint::WellFormed,
            "a time zone is declared only with a cron expression",
        )),
        _ => Err(invalid(
            "filter",
            Constraint::WellFormed,
            "a trigger declares a filter, an interval or a cron expression, and only one",
        )),
    }
}

fn parse_templates(fields: &TemplateFields<'_>) -> Result<Templates> {
    // Each present value parses alone first, so a syntax error names its own field; the combined
    // parse below then enforces only the pairing rules.
    if let Some(correlation) = fields.correlation {
        correlation.parse::<Template>().map_err(|error| {
            invalid(
                "correlation",
                Constraint::WellFormed,
                format!("a trigger correlation: {error}"),
            )
        })?;
    }
    if let Some(on_miss) = fields.on_miss {
        on_miss
            .parse::<crate::domain::CorrelationMiss>()
            .map_err(|error| {
                invalid(
                    "on_miss",
                    Constraint::WellFormed,
                    format!("a trigger on_miss: {error}"),
                )
            })?;
    }
    if let Some(on_open_workspace) = fields.on_open_workspace {
        on_open_workspace
            .parse::<crate::domain::OnOpenWorkspace>()
            .map_err(|error| {
                invalid(
                    "on_open_workspace",
                    Constraint::WellFormed,
                    format!("a trigger on_open_workspace: {error}"),
                )
            })?;
    }
    let correlation =
        Correlation::parse(fields.correlation, fields.on_miss, fields.on_open_workspace).map_err(
            |error| invalid("correlation", Constraint::WellFormed, format!("{error:#}")),
        )?;
    Ok(Templates {
        brief: fields.brief.parse::<Template>().map_err(|error| {
            invalid(
                "brief",
                Constraint::WellFormed,
                format!("a trigger brief: {error}"),
            )
        })?,
        branch: fields.branch.map(str::parse).transpose().map_err(|error| {
            invalid(
                "branch",
                Constraint::WellFormed,
                format!("a trigger branch: {error}"),
            )
        })?,
        correlation,
    })
}

pub fn check_budget(fires: &Fires) -> Result<()> {
    let Fires::Scheduled(schedule) = fires else {
        return Ok(());
    };
    let budget = FiringBudget::default();
    let allowed = budget.window / i32::try_from(budget.limit.get())?;
    let fastest = schedule.fastest();
    if fastest < allowed {
        let (field, pace) = match schedule {
            Schedule::Every(_) => ("every", format!("every {fastest:#}")),
            Schedule::Cron(_) => ("cron", format!("as often as every {fastest:#}")),
        };
        return Err(invalid(
            field,
            Constraint::WellFormed,
            format!(
                "a trigger firing {pace} would exhaust its budget of {} firings in {:#}: fire at \
                 most every {allowed:#}",
                budget.limit, budget.window
            ),
        ));
    }
    Ok(())
}

pub fn project_change(
    existing: Option<&crate::domain::Project>,
    checked: &CheckedProject,
) -> Declaration {
    let selected = |mut selected: Vec<(&str, &str)>| {
        selected.sort();
        (!selected.is_empty()).then(|| {
            selected
                .iter()
                .map(|(repository, integration)| format!("{repository} as {integration}"))
                .collect::<Vec<_>>()
                .join("\n")
        })
    };
    let becomes = vec![
        (
            "repositories",
            Some(checked.declarable.repositories().join("\n")),
        ),
        (
            "integrations",
            selected(checked.declarable.selected().collect()),
        ),
        ("branch", Some(checked.declarable.branch().to_owned())),
    ];
    let (action, differences) = compared(
        existing.map(|project| {
            vec![
                ("repositories", Some(project.repositories.join("\n"))),
                (
                    "integrations",
                    selected(
                        project
                            .integrations
                            .iter()
                            .map(|selected| (selected.repository.as_str(), selected.name.as_str()))
                            .collect(),
                    ),
                ),
                ("branch", Some(project.branch.clone())),
            ]
        }),
        becomes,
    );
    Declaration {
        kind: Kind::Project,
        name: checked.name.clone(),
        action,
        differences,
    }
}

pub fn agent_change(
    existing: Option<&crate::domain::Agent>,
    checked: &CheckedAgent,
) -> Declaration {
    let (action, differences) = compared(
        existing.map(|agent| {
            vec![
                ("harness", Some(agent.harness.clone())),
                ("model", agent.declared.model.clone()),
                ("mode", agent.declared.mode.clone()),
                ("thought level", agent.declared.thought_level.clone()),
            ]
        }),
        vec![
            ("harness", Some(checked.harness.clone())),
            ("model", checked.declared.model.clone()),
            ("mode", checked.declared.mode.clone()),
            ("thought level", checked.declared.thought_level.clone()),
        ],
    );
    Declaration {
        kind: Kind::Agent,
        name: checked.name.clone(),
        action,
        differences,
    }
}

pub fn trigger_change(existing: Option<&Trigger>, checked: &CheckedTrigger) -> Declaration {
    let mut allows = checked.allows.clone();
    allows.sort();
    allows.dedup();
    let becomes = vec![
        ("matches", Some(checked.fires.to_string())),
        ("project", Some(checked.project.clone())),
        ("agent", Some(checked.agent.clone())),
        ("allows", (!allows.is_empty()).then(|| allows.join(", "))),
        ("profile", checked.profile.clone()),
        (
            "branch",
            checked.templates.branch.as_ref().map(ToString::to_string),
        ),
        (
            "correlation",
            checked
                .templates
                .correlation
                .template()
                .map(ToString::to_string),
        ),
        (
            "on miss",
            checked
                .templates
                .correlation
                .on_miss()
                .map(|miss| miss.to_string()),
        ),
        (
            "on open workspace",
            Some(
                checked
                    .templates
                    .correlation
                    .on_open_workspace()
                    .to_string(),
            ),
        ),
        ("brief", Some(checked.templates.brief.to_string())),
        ("model", checked.declared.model.clone()),
        ("mode", checked.declared.mode.clone()),
        ("thought level", checked.declared.thought_level.clone()),
    ];
    let (action, differences) = compared(existing.map(described_trigger), becomes);
    Declaration {
        kind: Kind::Trigger,
        name: checked.name.clone(),
        action,
        differences,
    }
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
        ("matches", Some(trigger.fires.to_string())),
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
) -> (Action, Vec<Difference>) {
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
    (action, differences)
}

/// A start refuses what would change a declaration: the module's difference, reported on its
/// first differing field so the caller prefixes it.
pub fn refuse_on_change(change: &Declaration, message: String) -> Result<()> {
    if matches!(change.action, Action::Add | Action::Unchanged) {
        return Ok(());
    }
    let field = change
        .differences
        .first()
        .map(|difference| difference.field)
        .unwrap_or("name");
    Err(invalid(field, Constraint::Immutable, message))
}

/// Only a checked declaration can be written, in the caller's transaction, so preview and
/// dry-run stay the caller not committing.
pub async fn write_project(
    tx: &mut Tx<'_>,
    organization: &Organization,
    checked: &CheckedProject,
) -> Result<DeclaredRecord<crate::domain::Project>> {
    checked
        .declarable
        .declare(tx, organization, &checked.name)
        .await
}

pub async fn write_agent(
    tx: &mut Tx<'_>,
    organization: &Organization,
    checked: &CheckedAgent,
) -> Result<DeclaredRecord<crate::domain::Agent>> {
    tx.agents()
        .declare(
            organization,
            &checked.name,
            &checked.harness,
            &checked.declared,
        )
        .await
}

pub struct TriggerRefs {
    pub project: crate::domain::Project,
    pub agent: crate::domain::Agent,
    pub allows: Vec<crate::domain::Agent>,
    pub profile: Option<crate::domain::SubscriptionProfile>,
}

pub async fn resolve_trigger_refs(
    tx: &mut Tx<'_>,
    organization: &Organization,
    checked: &CheckedTrigger,
) -> Result<TriggerRefs> {
    let project = tx.projects().named(organization, &checked.project).await?;
    let agent = tx.agents().named(organization, &checked.agent).await?;
    let allows = crate::trigger::allowed(tx, organization, &checked.allows).await?;
    let profile = match &checked.profile {
        Some(profile) => Some(tx.profiles().named(organization, profile).await?),
        None => None,
    };
    Ok(TriggerRefs {
        project,
        agent,
        allows,
        profile,
    })
}

pub async fn insert_trigger(
    tx: &mut Tx<'_>,
    organization: &Organization,
    refs: &TriggerRefs,
    checked: &CheckedTrigger,
    provenance: Provenance,
) -> Result<Trigger> {
    tx.triggers()
        .declare(
            organization,
            &checked.name,
            &checked.fires,
            &checked.templates,
            &refs.project,
            &refs.agent,
            &checked.declared,
            &refs.allows,
            refs.profile.as_ref(),
            provenance.applied(),
        )
        .await
}

pub async fn update_trigger(
    tx: &mut Tx<'_>,
    existing: &Trigger,
    refs: &TriggerRefs,
    checked: &CheckedTrigger,
    provenance: Provenance,
) -> Result<Trigger> {
    tx.triggers()
        .redeclare(
            existing,
            &checked.fires,
            &checked.templates,
            &refs.project,
            &refs.agent,
            &checked.declared,
            &refs.allows,
            refs.profile.as_ref(),
            provenance.applied(),
        )
        .await
}

pub struct PlacedTrigger {
    pub trigger: Trigger,
    pub change: Declaration,
}

pub async fn place_trigger(
    tx: &mut Tx<'_>,
    organization: &Organization,
    checked: &CheckedTrigger,
    provenance: Provenance,
) -> Result<PlacedTrigger> {
    let refs = resolve_trigger_refs(tx, organization, checked).await?;
    let existing = tx.triggers().all(organization).await?;
    let change = trigger_change(
        existing.iter().find(|trigger| trigger.name == checked.name),
        checked,
    );
    let trigger = match change.action {
        Action::Add => insert_trigger(tx, organization, &refs, checked, provenance).await?,
        Action::Change => {
            let existing = existing
                .iter()
                .find(|trigger| trigger.name == checked.name)
                .expect("a changed trigger was listed");
            update_trigger(tx, existing, &refs, checked, provenance).await?
        }
        Action::Unchanged | Action::Remove => existing
            .into_iter()
            .find(|trigger| trigger.name == checked.name)
            .expect("an unchanged trigger was listed"),
    };
    Ok(PlacedTrigger { trigger, change })
}

pub async fn place_project(
    tx: &mut Tx<'_>,
    organization: &Organization,
    checked: &CheckedProject,
    on_existing: OnExisting,
) -> Result<(DeclaredRecord<crate::domain::Project>, Declaration)> {
    let existing = tx.projects().all(organization).await?;
    let found = existing.iter().find(|project| project.name == checked.name);
    let change = project_change(found, checked);
    if matches!(on_existing, OnExisting::Refuse)
        && let Some(found) = found
    {
        refuse_on_change(&change, project_refusal(found))?;
    }
    let placed = write_project(tx, organization, checked).await?;
    Ok((placed, change))
}

pub async fn place_agent(
    tx: &mut Tx<'_>,
    organization: &Organization,
    checked: &CheckedAgent,
    on_existing: OnExisting,
) -> Result<(DeclaredRecord<crate::domain::Agent>, Declaration)> {
    let existing = tx.agents().all(organization).await?;
    let found = existing.iter().find(|agent| agent.name == checked.name);
    let change = agent_change(found, checked);
    if matches!(on_existing, OnExisting::Refuse)
        && let Some(found) = found
    {
        refuse_on_change(
            &change,
            format!(
                "the agent {} is declared on the harness {} with the model {}, and a start \
                 changes no declaration",
                found.name,
                found.harness,
                found
                    .declared
                    .model
                    .as_deref()
                    .unwrap_or("its harness's default")
            ),
        )?;
    }
    let placed = write_agent(tx, organization, checked).await?;
    Ok((placed, change))
}

fn project_refusal(found: &crate::domain::Project) -> String {
    let selecting = if found.integrations.is_empty() {
        String::new()
    } else {
        format!(
            " as {}",
            found
                .integrations
                .iter()
                .map(|selected| selected.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    format!(
        "the project {} is declared against {}{selecting} on {}, and a start changes no \
         declaration",
        found.name,
        found.repositories.join(", "),
        found.branch
    )
}

pub async fn apply(
    store: &Store,
    images: &Images,
    github: &Github,
    organization: &str,
    document: &Document,
    mode: ApplyMode,
) -> Result<Applied> {
    let operation = mode.operation();
    check_name(&document.project.name)
        .map_err(|error| scoped(Prefix::Project, operation, error))?;
    check_name(&document.agent.name).map_err(|error| scoped(Prefix::Agent, operation, error))?;
    check_name(&document.trigger.name)
        .map_err(|error| scoped(Prefix::Trigger, operation, error))?;
    check_trigger_links(
        &document.trigger.project,
        &document.trigger.agent,
        &document.project.name,
        &document.agent.name,
    )
    .map_err(|error| scoped(Prefix::Trigger, operation, error))?;
    let (filter, templates) = parse_filter_trigger(
        &document.trigger.filter,
        &TemplateFields {
            brief: &document.trigger.brief,
            branch: document.trigger.branch.as_deref(),
            correlation: document.trigger.correlation.as_deref(),
            on_miss: document.trigger.on_miss.as_deref(),
            on_open_workspace: document.trigger.on_open_workspace.as_deref(),
        },
    )
    .map_err(|error| scoped(Prefix::Trigger, operation, error))?;
    let agent = check_agent(
        images,
        &document.agent.name,
        &document.agent.harness,
        &document.agent.declared(),
        operation,
    )
    .await
    .map_err(|error| scoped(Prefix::Agent, operation, error))?;
    let project = check_project(store, github, organization, &document.project, operation)
        .await
        .map_err(|error| scoped(Prefix::Project, operation, error))?;
    let trigger = CheckedTrigger::new(TriggerProposal {
        name: document.trigger.name.clone(),
        fires: Fires::On(filter),
        templates,
        project: document.trigger.project.clone(),
        agent: document.trigger.agent.clone(),
        declared: document.trigger.declared(),
        allows: document.trigger.allows.clone(),
        profile: document.trigger.profile.clone(),
    });

    let mut tx = store.begin().await?;
    let organization = tx.organizations().named(organization).await?;
    let (project_record, project_change) =
        place_project(&mut tx, &organization, &project, OnExisting::Change).await?;
    let (agent_record, agent_change) =
        place_agent(&mut tx, &organization, &agent, OnExisting::Change).await?;
    let refs = TriggerRefs {
        project: project_record.record,
        agent: agent_record.record,
        allows: crate::trigger::allowed(&mut tx, &organization, &trigger.allows).await?,
        profile: match &trigger.profile {
            Some(profile) => Some(tx.profiles().named(&organization, profile).await?),
            None => None,
        },
    };
    let existing = tx.triggers().all(&organization).await?;
    let trigger_change = trigger_change(
        existing
            .iter()
            .find(|trigger| trigger.name == document.trigger.name),
        &trigger,
    );
    match trigger_change.action {
        Action::Add => {
            insert_trigger(&mut tx, &organization, &refs, &trigger, Provenance::File).await?;
        }
        Action::Change => {
            let existing = existing
                .iter()
                .find(|trigger| trigger.name == document.trigger.name)
                .expect("a changed trigger was listed");
            update_trigger(&mut tx, existing, &refs, &trigger, Provenance::File).await?;
        }
        Action::Unchanged | Action::Remove => {}
    }

    if matches!(mode, ApplyMode::Apply) {
        tx.commit().await?;
    }

    Ok(Applied {
        declarations: vec![project_change, agent_change, trigger_change],
        admitting_outsiders: match &trigger.fires {
            Fires::On(filter) if filter.admits_outsiders() => vec![trigger.name.clone()],
            _ => Vec::new(),
        },
    })
}
