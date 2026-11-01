use std::fmt;

use anyhow::Result;
use serde::Deserialize;

use crate::capability::Images;
use crate::cron::Cron;
use crate::declined::{Concerning, Constraint, Reason, Within};
use crate::domain::{
    Correlation, Declared, Fires, FiringBudget, Organization, Schedule, Templates, Trigger,
};
use crate::filter::Filter;
use crate::integration::github::Github;
use crate::repository::{self, Declarable, Declaring, Selecting};
use crate::store::{Declared as DeclaredRecord, Store, Tx};
use crate::template::Template;

pub mod triggers;

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

pub struct Applied {
    pub declarations: Vec<Declaration>,
    pub admitting_outsiders: Vec<String>,
}

/// The one change vocabulary every declaration path answers in: what was declared, and what
/// writing it did.
#[derive(Debug, Clone)]
pub struct Declaration {
    pub kind: Kind,
    pub name: String,
    pub action: Action,
    pub differences: Vec<Difference>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Difference {
    pub field: &'static str,
    pub was: Option<String>,
    pub becomes: Option<String>,
}

#[derive(Debug, Clone, Copy)]
pub enum Kind {
    Project,
    Agent,
    Trigger,
}

#[derive(Debug, Clone, Copy)]
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
/// it broke and why. The caller stamps its operation and names the record the field is in.
#[derive(Debug)]
pub struct Invalid {
    record: Option<String>,
    field: &'static str,
    constraint: Constraint,
    message: String,
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
        record: None,
        field,
        constraint,
        message: message.into(),
    }
    .into()
}

/// Names the record a refused field was read from, for a caller that holds many of one kind.
fn under(record: String, error: anyhow::Error) -> anyhow::Error {
    match error.downcast::<Invalid>() {
        Ok(invalid) => Invalid {
            record: Some(record),
            ..invalid
        }
        .into(),
        Err(error) => error,
    }
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

impl Prefix {
    const fn record(self) -> Option<&'static str> {
        match self {
            Prefix::Root => None,
            Prefix::Project => Some("project"),
            Prefix::Agent => Some("agent"),
            Prefix::Trigger => Some("trigger"),
        }
    }
}

/// Gives a module error its route: stamps the caller's operation onto a field refusal and
/// names the record its field is in, and leaves the rest alone.
pub fn scoped(prefix: Prefix, operation: &'static str, error: anyhow::Error) -> anyhow::Error {
    let record = prefix.record().map(str::to_owned);
    let error = match error.downcast::<Invalid>() {
        Ok(invalid) => {
            return within(
                invalid.record.or(record),
                None,
                Reason::InvalidField {
                    field: invalid.field,
                    operation,
                    constraint: invalid.constraint,
                    allowed: None,
                    message: invalid.message,
                },
            );
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
            return within(
                record,
                None,
                Reason::InvalidField {
                    field,
                    operation,
                    constraint,
                    allowed,
                    message,
                },
            );
        }
        Ok(reason) => return reason.into(),
        Err(error) => error,
    };
    match error.downcast::<Concerning>() {
        Ok(concerning) => within(record, Some(concerning.field), concerning.reason),
        Err(error) => error,
    }
}

fn within(record: Option<String>, field: Option<&'static str>, reason: Reason) -> anyhow::Error {
    match (record, field) {
        (Some(record), field) => Within {
            record,
            field,
            reason,
        }
        .into(),
        (None, Some(field)) => reason.concerning(field).into(),
        (None, None) => reason.into(),
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

/// Only the checks below build a checked declaration, and only a checked one is written.
pub struct CheckedProject {
    name: String,
    declarable: Declarable,
}

pub struct CheckedAgent {
    name: String,
    harness: String,
    declared: Declared,
}

pub struct CheckedTrigger {
    name: String,
    fires: Fires,
    templates: Templates,
    project: String,
    agent: String,
    declared: Declared,
    allows: Vec<String>,
    profile: Option<String>,
}

impl CheckedTrigger {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn admits_outsiders(&self) -> bool {
        matches!(&self.fires, Fires::On(filter) if filter.admits_outsiders())
    }
}

/// A Trigger whose schedule and templates a caller already holds as values.
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

/// A Trigger as a route received it, before anything is parsed.
pub struct TriggerFields<'a> {
    pub name: &'a str,
    pub forms: ScheduleForms<'a>,
    pub templates: TemplateFields<'a>,
    pub project: &'a str,
    pub agent: &'a str,
    pub declared: Declared,
    pub allows: &'a [String],
    pub profile: Option<&'a str>,
}

/// The schedule half of a Trigger declaration: exactly one of a filter, an interval or a cron
/// expression read in a zone.
pub struct ScheduleForms<'a> {
    pub filter: Option<&'a serde_json::Value>,
    pub every: Option<&'a str>,
    pub cron: Option<&'a str>,
    pub zone: Option<&'a str>,
}

impl<'a> ScheduleForms<'a> {
    /// The document and the applied-Triggers file declare event Triggers only; their schemas
    /// refuse a schedule.
    pub const fn filter(filter: &'a serde_json::Value) -> Self {
        Self {
            filter: Some(filter),
            every: None,
            cron: None,
            zone: None,
        }
    }
}

pub struct TemplateFields<'a> {
    pub brief: &'a str,
    pub branch: Option<&'a str>,
    pub correlation: Option<&'a str>,
    pub on_miss: Option<&'a str>,
    pub on_open_workspace: Option<&'a str>,
}

fn check_name(name: &str) -> Result<()> {
    if name.is_empty() {
        return Err(invalid(
            "name",
            Constraint::NonEmpty,
            "a declaration names each record",
        ));
    }
    Ok(())
}

fn check_branch(branch: &str) -> Result<()> {
    if branch.is_empty() {
        return Err(invalid(
            "branch",
            Constraint::NonEmpty,
            "a project names the branch its work happens on",
        ));
    }
    Ok(())
}

fn check_harness(harness: &str) -> Result<()> {
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
fn check_trigger_links(document: &Document) -> Result<()> {
    let (trigger, project, agent) = (
        &document.trigger,
        &document.project.name,
        &document.agent.name,
    );
    if &trigger.project != project {
        return Err(invalid(
            "project",
            Constraint::MatchesDeclared,
            format!(
                "the trigger names project {}, not the declared project {project}",
                trigger.project
            ),
        ));
    }
    if &trigger.agent != agent {
        return Err(invalid(
            "agent",
            Constraint::MatchesDeclared,
            format!(
                "the trigger names agent {}, not the declared agent {agent}",
                trigger.agent
            ),
        ));
    }
    Ok(())
}

/// The one Trigger parser: a filter, an interval or a cron expression, and only one, with the
/// firing-budget check wherever a schedule arrives.
pub fn check_trigger(fields: &TriggerFields<'_>) -> Result<CheckedTrigger> {
    check_name(fields.name)?;
    let fires = parse_fires(&fields.forms)?;
    check_budget(&fires)?;
    Ok(CheckedTrigger {
        name: fields.name.to_owned(),
        fires,
        templates: parse_templates(&fields.templates)?,
        project: fields.project.to_owned(),
        agent: fields.agent.to_owned(),
        declared: Declared::named(fields.declared.clone()),
        allows: fields.allows.to_vec(),
        profile: fields.profile.map(str::to_owned),
    })
}

pub fn check_parsed_trigger(proposal: TriggerProposal) -> Result<CheckedTrigger> {
    check_name(&proposal.name)?;
    check_budget(&proposal.fires)?;
    Ok(CheckedTrigger {
        name: proposal.name,
        fires: proposal.fires,
        templates: proposal.templates,
        project: proposal.project,
        agent: proposal.agent,
        declared: Declared::named(proposal.declared),
        allows: proposal.allows,
        profile: proposal.profile,
    })
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

fn check_budget(fires: &Fires) -> Result<()> {
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

type Described = Vec<(&'static str, Option<String>)>;

fn described_project(
    repositories: &[String],
    mut selected: Vec<(&str, &str)>,
    branch: &str,
) -> Described {
    selected.sort();
    vec![
        ("repositories", Some(repositories.join("\n"))),
        (
            "integrations",
            (!selected.is_empty()).then(|| {
                selected
                    .iter()
                    .map(|(repository, integration)| format!("{repository} as {integration}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            }),
        ),
        ("branch", Some(branch.to_owned())),
    ]
}

fn described_agent(harness: &str, declared: &Declared) -> Described {
    vec![
        ("harness", Some(harness.to_owned())),
        ("model", declared.model.clone()),
        ("mode", declared.mode.clone()),
        ("thought level", declared.thought_level.clone()),
    ]
}

struct TriggerDescription<'a> {
    fires: &'a Fires,
    templates: &'a Templates,
    project: &'a str,
    agent: &'a str,
    declared: &'a Declared,
    allows: Vec<&'a str>,
    profile: Option<&'a str>,
}

fn described_trigger(mut trigger: TriggerDescription<'_>) -> Described {
    trigger.allows.sort_unstable();
    trigger.allows.dedup();
    let correlation = &trigger.templates.correlation;
    vec![
        ("matches", Some(trigger.fires.to_string())),
        ("project", Some(trigger.project.to_owned())),
        ("agent", Some(trigger.agent.to_owned())),
        (
            "allows",
            (!trigger.allows.is_empty()).then(|| trigger.allows.join(", ")),
        ),
        ("profile", trigger.profile.map(str::to_owned)),
        (
            "branch",
            trigger.templates.branch.as_ref().map(ToString::to_string),
        ),
        (
            "correlation",
            correlation.template().map(ToString::to_string),
        ),
        (
            "on miss",
            correlation.on_miss().map(|miss| miss.to_string()),
        ),
        (
            "on open workspace",
            Some(correlation.on_open_workspace().to_string()),
        ),
        ("brief", Some(trigger.templates.brief.to_string())),
        ("model", trigger.declared.model.clone()),
        ("mode", trigger.declared.mode.clone()),
        ("thought level", trigger.declared.thought_level.clone()),
    ]
}

fn project_change(
    existing: Option<&crate::domain::Project>,
    checked: &CheckedProject,
) -> Declaration {
    let (action, differences) = compared(
        existing.map(|project| {
            described_project(
                &project.repositories,
                project
                    .integrations
                    .iter()
                    .map(|selected| (selected.repository.as_str(), selected.name.as_str()))
                    .collect(),
                &project.branch,
            )
        }),
        described_project(
            checked.declarable.repositories(),
            checked.declarable.selected().collect(),
            checked.declarable.branch(),
        ),
    );
    Declaration {
        kind: Kind::Project,
        name: checked.name.clone(),
        action,
        differences,
    }
}

fn agent_change(existing: Option<&crate::domain::Agent>, checked: &CheckedAgent) -> Declaration {
    let (action, differences) = compared(
        existing.map(|agent| described_agent(&agent.harness, &agent.declared)),
        described_agent(&checked.harness, &checked.declared),
    );
    Declaration {
        kind: Kind::Agent,
        name: checked.name.clone(),
        action,
        differences,
    }
}

fn trigger_change(existing: Option<&Trigger>, checked: &CheckedTrigger) -> Declaration {
    let (action, differences) = compared(
        existing.map(|trigger| {
            described_trigger(TriggerDescription {
                fires: &trigger.fires,
                templates: &trigger.templates,
                project: &trigger.project.name,
                agent: &trigger.agent.name,
                declared: &trigger.declared,
                allows: trigger
                    .allows
                    .iter()
                    .map(|agent| agent.name.as_str())
                    .collect(),
                profile: trigger
                    .profile
                    .as_ref()
                    .map(|profile| profile.name.as_str()),
            })
        }),
        described_trigger(TriggerDescription {
            fires: &checked.fires,
            templates: &checked.templates,
            project: &checked.project,
            agent: &checked.agent,
            declared: &checked.declared,
            allows: checked.allows.iter().map(String::as_str).collect(),
            profile: checked.profile.as_deref(),
        }),
    );
    Declaration {
        kind: Kind::Trigger,
        name: checked.name.clone(),
        action,
        differences,
    }
}

fn compared(was: Option<Described>, becomes: Described) -> (Action, Vec<Difference>) {
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
fn refuse_on_change(change: &Declaration, message: String) -> Result<()> {
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

struct TriggerRefs {
    project: crate::domain::Project,
    agent: crate::domain::Agent,
    allows: Vec<crate::domain::Agent>,
    profile: Option<crate::domain::SubscriptionProfile>,
}

async fn trigger_refs(
    tx: &mut Tx<'_>,
    organization: &Organization,
    checked: &CheckedTrigger,
) -> Result<TriggerRefs> {
    let project = tx.projects().named(organization, &checked.project).await?;
    let agent = tx.agents().named(organization, &checked.agent).await?;
    let mut allows = Vec::with_capacity(checked.allows.len());
    for name in &checked.allows {
        allows.push(tx.agents().named(organization, name).await?);
    }
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

/// The Trigger a checked declaration would write, for testing one a file declares without
/// writing it.
pub async fn unwritten_trigger(
    tx: &mut Tx<'_>,
    organization: Organization,
    checked: &CheckedTrigger,
) -> Result<Trigger> {
    let refs = trigger_refs(tx, &organization, checked).await?;
    let declared_at = tx.now();
    Ok(Trigger {
        id: crate::domain::TriggerId::generate(),
        organization,
        name: checked.name.clone(),
        fires: checked.fires.clone(),
        templates: checked.templates.clone(),
        project: refs.project,
        agent: refs.agent,
        declared: checked.declared.clone(),
        allows: refs.allows,
        profile: refs.profile,
        state: crate::domain::TriggerState::Enabled,
        disabled_because: None,
        firing_budget: FiringBudget::default(),
        applied: true,
        declared_at,
    })
}

pub struct PlacedTrigger {
    pub trigger: Trigger,
    pub change: Declaration,
}

/// Writes take the caller's transaction, so preview and dry-run stay the caller not committing.
pub async fn place_trigger(
    tx: &mut Tx<'_>,
    organization: &Organization,
    checked: &CheckedTrigger,
    provenance: Provenance,
) -> Result<PlacedTrigger> {
    let refs = trigger_refs(tx, organization, checked).await?;
    let existing = tx
        .triggers()
        .all(organization)
        .await?
        .into_iter()
        .find(|trigger| trigger.name == checked.name);
    let change = trigger_change(existing.as_ref(), checked);
    let trigger = match (change.action, existing) {
        (Action::Change, Some(existing)) => {
            tx.triggers()
                .redeclare(
                    &existing,
                    &checked.fires,
                    &checked.templates,
                    &refs.project,
                    &refs.agent,
                    &checked.declared,
                    &refs.allows,
                    refs.profile.as_ref(),
                    provenance.applied(),
                )
                .await?
        }
        (_, Some(existing)) => existing,
        (_, None) => {
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
                .await?
        }
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
    let placed = checked
        .declarable
        .declare(tx, organization, &checked.name)
        .await?;
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
        refuse_on_change(&change, agent_refusal(found))?;
    }
    let placed = tx
        .agents()
        .declare(
            organization,
            &checked.name,
            &checked.harness,
            &checked.declared,
        )
        .await?;
    Ok((placed, change))
}

/// What a start that found this Project declared would be refused with, asking GitHub nothing.
pub fn start_project_conflict(
    found: &crate::domain::Project,
    repositories: &[String],
    selected: Vec<(&str, &str)>,
    branch: &str,
) -> Result<()> {
    let (action, differences) = compared(
        Some(described_project(
            &found.repositories,
            found
                .integrations
                .iter()
                .map(|selected| (selected.repository.as_str(), selected.name.as_str()))
                .collect(),
            &found.branch,
        )),
        described_project(repositories, selected, branch),
    );
    let change = Declaration {
        kind: Kind::Project,
        name: found.name.clone(),
        action,
        differences,
    };
    refuse_on_change(&change, project_refusal(found))
}

/// What a start that found this Agent declared would be refused with.
pub fn start_agent_conflict(
    found: &crate::domain::Agent,
    harness: &str,
    declared: &Declared,
) -> Result<()> {
    let (action, differences) = compared(
        Some(described_agent(&found.harness, &found.declared)),
        described_agent(harness, &Declared::named(declared.clone())),
    );
    let change = Declaration {
        kind: Kind::Agent,
        name: found.name.clone(),
        action,
        differences,
    };
    refuse_on_change(&change, agent_refusal(found))
}

fn agent_refusal(found: &crate::domain::Agent) -> String {
    format!(
        "the agent {} is declared on the harness {} with the model {}, and a start changes no \
         declaration",
        found.name,
        found.harness,
        found
            .declared
            .model
            .as_deref()
            .unwrap_or("its harness's default")
    )
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
    let in_trigger = |error| scoped(Prefix::Trigger, operation, error);
    check_name(&document.project.name)
        .map_err(|error| scoped(Prefix::Project, operation, error))?;
    check_name(&document.agent.name).map_err(|error| scoped(Prefix::Agent, operation, error))?;
    check_name(&document.trigger.name).map_err(in_trigger)?;
    check_trigger_links(document).map_err(in_trigger)?;
    let trigger = check_trigger(&TriggerFields {
        name: &document.trigger.name,
        forms: ScheduleForms::filter(&document.trigger.filter),
        templates: TemplateFields {
            brief: &document.trigger.brief,
            branch: document.trigger.branch.as_deref(),
            correlation: document.trigger.correlation.as_deref(),
            on_miss: document.trigger.on_miss.as_deref(),
            on_open_workspace: document.trigger.on_open_workspace.as_deref(),
        },
        project: &document.trigger.project,
        agent: &document.trigger.agent,
        declared: document.trigger.declared(),
        allows: &document.trigger.allows,
        profile: document.trigger.profile.as_deref(),
    })
    .map_err(in_trigger)?;
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

    let mut tx = store.begin().await?;
    let organization = tx.organizations().named(organization).await?;
    let (_, project) = place_project(&mut tx, &organization, &project, OnExisting::Change).await?;
    let (_, agent) = place_agent(&mut tx, &organization, &agent, OnExisting::Change).await?;
    let placed = place_trigger(&mut tx, &organization, &trigger, Provenance::File).await?;

    if matches!(mode, ApplyMode::Apply) {
        tx.commit().await?;
    }

    Ok(Applied {
        declarations: vec![project, agent, placed.change],
        admitting_outsiders: if trigger.admits_outsiders() {
            vec![trigger.name]
        } else {
            Vec::new()
        },
    })
}
