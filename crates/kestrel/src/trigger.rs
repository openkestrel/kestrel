//! An Event supplies data and never authority (ADR-0013): the Agent and the Project a
//! firing starts work with are named in the declaration a human applied, never in the Event.

use std::collections::BTreeSet;

use anyhow::{Context as _, Result, anyhow, bail};
use jiff::{SignedDuration, Timestamp};

pub mod apply;

use crate::domain::{
    Agent, CorrelationMiss, DisableReason, Event, EventRecordId, Fires, Firing, FiringBudget,
    Integration, Occurrence, OnOpenWorkspace, Organization, Schedule, SessionId, Templates,
    Trigger, TriggerId, TriggerState, Workspace, WorkspaceId,
};
use crate::fanout::{self, Change};
use crate::integration::github::{self, EventData, Github};
use crate::log::Entry;
use crate::readiness::{Decision, Readiness, Request};
use crate::store::integration::Recorded;
use crate::store::workspace::{Opening, PendingSession};
use crate::store::{Store, Tx};
use crate::workspace;

/// A sweep takes a bounded bite rather than every Event a Trigger declared over a busy repository
/// matches at once.
const AT_A_TIME: usize = 32;

/// How long a held request waits for an event before its work item is asked about again, so a
/// missed event delays its start rather than stranding it.
const RECONSIDER: SignedDuration = SignedDuration::from_mins(5);

pub const AGENT_LABEL: &str = "agent:";

/// Fires only the Trigger it was dispatched to, never whatever else its source and subject match.
pub const DISPATCHED: &str = "dev.kestrel.dispatched";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Asked<'a> {
    pub instruction: Option<&'a str>,
    pub agent: Option<&'a str>,
}

impl<'a> Asked<'a> {
    pub fn by(event: &'a Event) -> Self {
        EventData::new(&event.occurrence)
            .command()
            .map(|command| Asked {
                instruction: command.instruction,
                agent: command.agent,
            })
            .unwrap_or_default()
    }
}

pub struct Dispatch<'a> {
    pub organization: &'a str,
    pub trigger: &'a str,
    pub integration: &'a str,
    pub issue: i64,
    pub asked: Asked<'a>,
}

/// Nothing a test renders against is recorded.
#[derive(Clone, Copy)]
pub enum Against<'a> {
    Event(EventRecordId),
    NextElapsing,
    Issue {
        github: &'a Github,
        integration: &'a str,
        issue: i64,
    },
}

pub struct Declaration<'a> {
    pub organization: &'a str,
    pub name: &'a str,
    pub fires: &'a Fires,
    pub templates: &'a Templates,
    pub project: &'a str,
    pub agent: &'a str,
    pub allows: &'a [String],
    pub profile: Option<&'a str>,
}

#[derive(Debug, Clone)]
pub enum Fired {
    Opened {
        event: EventRecordId,
        workspace: WorkspaceId,
        session: SessionId,
    },
    Fed {
        event: EventRecordId,
        workspace: WorkspaceId,
        session: Option<SessionId>,
    },
    Ignored {
        event: EventRecordId,
        trigger: String,
        correlation: String,
    },
    Failed {
        event: EventRecordId,
        trigger: String,
        because: String,
    },
    Held {
        event: EventRecordId,
        trigger: String,
        because: String,
    },
    Canceled {
        event: EventRecordId,
        trigger: String,
        because: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rendered {
    pub brief: String,
    pub branch: Option<String>,
    pub correlation: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Would {
    Open,
    Continue,
    NewSession,
    Ignore,
}

impl Would {
    pub const fn as_str(self) -> &'static str {
        match self {
            Would::Open => "open_workspace",
            Would::Continue => "continue_session",
            Would::NewSession => "new_session",
            Would::Ignore => "ignore",
        }
    }
}

#[derive(Debug)]
pub struct Tested {
    pub matches: bool,
    pub rendered: Result<Rendered>,
    pub agent: Result<String>,
    /// `None` when nothing rendered to correlate with.
    pub would: Option<Would>,
    /// When the elapsing a test named no Event for is due.
    pub elapsing: Option<Timestamp>,
}

pub async fn declare(store: &Store, declaration: Declaration<'_>) -> Result<Trigger> {
    if let Fires::Scheduled(schedule) = declaration.fires {
        let budget = FiringBudget::default();
        let allowed = budget.window / i32::try_from(budget.limit.get())?;
        let fastest = schedule.fastest();
        if fastest < allowed {
            let pace = match schedule {
                Schedule::Every(_) => format!("every {fastest:#}"),
                Schedule::Cron(_) => format!("as often as every {fastest:#}"),
            };
            bail!(
                "a trigger firing {pace} would exhaust its budget of {} firings in {:#}: \
                 fire at most every {allowed:#}",
                budget.limit,
                budget.window
            );
        }
    }

    let mut tx = store.begin().await?;
    let organization = tx.organizations().named(declaration.organization).await?;
    let project = tx
        .projects()
        .named(&organization, declaration.project)
        .await?;
    let agent = tx.agents().named(&organization, declaration.agent).await?;
    let allows = allowed(&mut tx, &organization, declaration.allows).await?;
    let profile = match declaration.profile {
        Some(profile) => Some(tx.profiles().named(&organization, profile).await?),
        None => None,
    };
    let existing = tx
        .triggers()
        .all(&organization)
        .await?
        .into_iter()
        .find(|trigger| trigger.name == declaration.name);
    let trigger = match existing {
        Some(trigger)
            if same_declaration(
                &trigger,
                declaration.fires,
                declaration.templates,
                &project,
                &agent,
                &allows,
                profile.as_ref(),
                false,
            ) =>
        {
            trigger
        }
        Some(trigger) => {
            tx.triggers()
                .redeclare(
                    &trigger,
                    declaration.fires,
                    declaration.templates,
                    &project,
                    &agent,
                    &allows,
                    profile.as_ref(),
                    false,
                )
                .await?
        }
        None => {
            tx.triggers()
                .declare(
                    &organization,
                    declaration.name,
                    declaration.fires,
                    declaration.templates,
                    &project,
                    &agent,
                    &allows,
                    profile.as_ref(),
                    false,
                )
                .await?
        }
    };
    tx.commit().await?;

    Ok(trigger)
}

#[expect(
    clippy::too_many_arguments,
    reason = "a trigger is what it is declared with"
)]
fn same_declaration(
    trigger: &Trigger,
    fires: &Fires,
    templates: &Templates,
    project: &crate::domain::Project,
    agent: &Agent,
    allows: &[Agent],
    profile: Option<&crate::domain::SubscriptionProfile>,
    applied: bool,
) -> bool {
    let names = |agents: &[Agent]| {
        let mut names = agents
            .iter()
            .map(|agent| agent.name.clone())
            .collect::<Vec<_>>();
        names.sort_unstable();
        names.dedup();
        names
    };

    trigger.fires == *fires
        && trigger.templates == *templates
        && trigger.project.id == project.id
        && trigger.agent.id == agent.id
        && names(&trigger.allows) == names(allows)
        && trigger.profile.as_ref().map(|profile| profile.id) == profile.map(|profile| profile.id)
        && trigger.applied == applied
}

pub(crate) async fn allowed(
    tx: &mut Tx<'_>,
    organization: &Organization,
    names: &[String],
) -> Result<Vec<Agent>> {
    let mut allows = Vec::with_capacity(names.len());
    for name in names {
        allows.push(tx.agents().named(organization, name).await?);
    }

    Ok(allows)
}

/// Whatever chooses the Agent, it is one the Trigger allows, so no Event reaches an Agent a human
/// did not name here.
pub fn chosen<'t>(trigger: &'t Trigger, event: &Event, asked: Option<&str>) -> Result<&'t Agent> {
    let allowed = || std::iter::once(&trigger.agent).chain(&trigger.allows);
    if let Some(name) = asked {
        return allowed().find(|agent| agent.name == name).ok_or_else(|| {
            anyhow!(
                "the trigger {} does not allow the agent {name} that was asked for",
                trigger.name
            )
        });
    }

    let data = EventData::new(&event.occurrence);
    let named: BTreeSet<&str> = data
        .labels()
        .filter_map(|label| label.strip_prefix(AGENT_LABEL))
        .collect();

    match named.into_iter().collect::<Vec<_>>().as_slice() {
        [] => Ok(&trigger.agent),
        [name] => allowed().find(|agent| agent.name == *name).ok_or_else(|| {
            anyhow!(
                "the label {AGENT_LABEL}{name} chooses an agent the trigger {} does not allow",
                trigger.name
            )
        }),
        several => bail!(
            "the labels {} each choose an agent, and the trigger {} will not guess which",
            several
                .iter()
                .map(|name| format!("{AGENT_LABEL}{name}"))
                .collect::<Vec<_>>()
                .join(" and "),
            trigger.name
        ),
    }
}

pub async fn firings(store: &Store, event: EventRecordId) -> Result<Vec<Firing>> {
    store.begin().await?.triggers().firings_of(event).await
}

pub async fn triggers(store: &Store, organization: &str) -> Result<Vec<Trigger>> {
    let mut tx = store.begin().await?;
    let organization = tx.organizations().named(organization).await?;

    tx.triggers().all(&organization).await
}

pub async fn show(store: &Store, organization: &str, name: &str) -> Result<Trigger> {
    let mut tx = store.begin().await?;
    let organization = tx.organizations().named(organization).await?;

    tx.triggers().named(&organization, name).await
}

/// Starts nothing, so an Event recorded before the Trigger was declared, or one it already
/// fired for, is still worth asking about; it renders even when the filter does not match, so
/// a brief can be written against an Event before the filter is right.
pub async fn test(
    store: &Store,
    organization: &str,
    name: &str,
    against: Against<'_>,
    asked: Asked<'_>,
) -> Result<Tested> {
    let trigger = {
        let mut tx = store.begin().await?;
        let organization = tx.organizations().named(organization).await?;
        tx.triggers().named(&organization, name).await?
    };

    tested(store, &trigger, against, asked).await
}

pub async fn test_declared(
    store: &Store,
    organization: &str,
    declared: &apply::Declared,
    against: Against<'_>,
    asked: Asked<'_>,
) -> Result<Tested> {
    let trigger = {
        let mut tx = store.begin().await?;
        let organization = tx.organizations().named(organization).await?;
        Trigger {
            id: TriggerId::generate(),
            project: tx
                .projects()
                .named(&organization, &declared.project)
                .await?,
            agent: tx.agents().named(&organization, &declared.agent).await?,
            allows: allowed(&mut tx, &organization, &declared.allows).await?,
            profile: match &declared.profile {
                Some(profile) => Some(tx.profiles().named(&organization, profile).await?),
                None => None,
            },
            organization,
            name: declared.name.clone(),
            fires: Fires::On(declared.filter.clone()),
            templates: declared.templates.clone(),
            state: TriggerState::Enabled,
            disabled_because: None,
            firing_budget: FiringBudget::default(),
            applied: true,
            declared_at: jiff::Timestamp::now(),
        }
    };

    tested(store, &trigger, against, asked).await
}

async fn tested(
    store: &Store,
    trigger: &Trigger,
    against: Against<'_>,
    asked: Asked<'_>,
) -> Result<Tested> {
    let unrecorded = |integration, occurrence| Event {
        record_id: EventRecordId::generate(),
        organization: trigger.organization.id,
        integration,
        occurrence,
        recorded_at: Timestamp::now(),
    };

    let mut tx = store.begin().await?;
    let (matches, event, elapsing) = match against {
        Against::NextElapsing => {
            let due = tx.triggers().due_at(trigger).await?.with_context(|| {
                format!(
                    "the trigger {} fires on events, so a test names one",
                    trigger.name
                )
            })?;
            let occurrence = trigger
                .elapsing(due)
                .context("a trigger with a due time has a schedule")?;
            (true, unrecorded(None, occurrence), Some(due))
        }
        Against::Issue {
            github,
            integration,
            issue,
        } => {
            let integration = tx
                .integrations()
                .named(&trigger.organization, integration)
                .await?;
            drop(tx);
            let occurrence = dispatched(github, trigger, &integration, issue, asked).await?;
            tx = store.begin().await?;
            (true, unrecorded(Some(integration.id), occurrence), None)
        }
        Against::Event(event) => {
            let event = tx.integrations().event(event).await?;
            if event.organization != trigger.organization.id {
                bail!(
                    "no event {} in the organization {}",
                    event.record_id,
                    trigger.organization.name
                );
            }
            let matches = tx.triggers().matches(trigger, &event).await?;
            (matches, event, None)
        }
    };
    let commanded = Asked::by(&event);
    let rendered = render(trigger, &event, asked.instruction.or(commanded.instruction));
    let would = match &rendered {
        Ok(rendered) => Some(would(&mut tx, trigger, rendered.correlation.as_deref()).await?),
        Err(_) => None,
    };

    Ok(Tested {
        matches,
        agent: chosen_name(trigger, &event, asked.agent.or(commanded.agent)),
        rendered,
        would,
        elapsing,
    })
}

async fn would(tx: &mut Tx<'_>, trigger: &Trigger, correlation: Option<&str>) -> Result<Would> {
    let Some(correlation) = correlation else {
        return Ok(Would::Open);
    };

    Ok(match correlated(tx, trigger, correlation).await? {
        Correlated::Holding(_) => match trigger.templates.correlation.on_open_workspace() {
            OnOpenWorkspace::Continue => Would::Continue,
            OnOpenWorkspace::NewSession => Would::NewSession,
        },
        Correlated::Ignored => Would::Ignore,
        Correlated::Missed(_) => Would::Open,
    })
}

enum Correlated {
    Holding(WorkspaceId),
    Ignored,
    /// The sealed Workspace an opening continues, if one held the key.
    Missed(Option<Box<Workspace>>),
}

async fn correlated(tx: &mut Tx<'_>, trigger: &Trigger, correlation: &str) -> Result<Correlated> {
    if let Some(holding) = tx
        .workspaces()
        .holding_correlation(&trigger.organization, correlation)
        .await?
    {
        return Ok(Correlated::Holding(holding));
    }

    // A key a sealed workspace held is kestrel's own work, so `ignore` does not drop it.
    let sealed = tx
        .workspaces()
        .sealed_holding_correlation(&trigger.organization, correlation)
        .await?;
    if sealed.is_none() && trigger.templates.correlation.on_miss() == Some(CorrelationMiss::Ignore)
    {
        return Ok(Correlated::Ignored);
    }

    Ok(Correlated::Missed(sealed.map(Box::new)))
}

fn chosen_name(trigger: &Trigger, event: &Event, asked: Option<&str>) -> Result<String> {
    chosen(trigger, event, asked).map(|agent| agent.name.clone())
}

pub fn render(trigger: &Trigger, event: &Event, instruction: Option<&str>) -> Result<Rendered> {
    let unrenderable = |field: &str| {
        format!(
            "the trigger {} cannot render its {field} for the event {}",
            trigger.name, event.record_id
        )
    };
    let templates = &trigger.templates;
    let occurrence = &event.occurrence;

    Ok(Rendered {
        brief: templates
            .brief
            .render_brief(occurrence, instruction)
            .with_context(|| unrenderable("brief"))?,
        branch: templates
            .branch
            .as_ref()
            .map(|branch| {
                branch
                    .render_line(occurrence)
                    .with_context(|| unrenderable("branch"))
            })
            .transpose()?,
        correlation: templates
            .correlation
            .template()
            .map(|correlation| {
                correlation
                    .render_line(occurrence)
                    .with_context(|| unrenderable("correlation"))
            })
            .transpose()?,
    })
}

pub async fn disable(store: &Store, organization: &str, name: &str) -> Result<Trigger> {
    set(
        store,
        organization,
        name,
        TriggerState::Disabled(DisableReason::Operator),
    )
    .await
}

pub async fn enable(store: &Store, organization: &str, name: &str) -> Result<Trigger> {
    set(store, organization, name, TriggerState::Enabled).await
}

/// Records what each due schedule mints and leaves the firing to [`fire`], so scheduled work
/// is recorded and fired like any other Event. Elapsings missed while nothing swept coalesce
/// into one.
pub async fn elapse(store: &Store, at: Timestamp) -> Result<Vec<Occurrence>> {
    let mut tx = store.begin().await?;
    let mut minted = Vec::new();

    for (trigger, due) in tx.triggers().schedules_due(at).await? {
        let Fires::Scheduled(schedule) = &trigger.fires else {
            bail!("the trigger {} is due but has no schedule", trigger.name);
        };
        let occurrence = trigger
            .elapsing(due)
            .context("a scheduled trigger mints an event")?;
        if let Recorded::Recorded = tx
            .integrations()
            .record_minted(&trigger.organization, &occurrence)
            .await?
        {
            minted.push(occurrence);
        }

        let next = schedule.following(due, at)?;
        tx.triggers().due_again(&trigger, next).await?;
    }
    tx.commit().await?;

    Ok(minted)
}

/// An Event no Trigger matches opens nothing, and that is not a failure.
pub async fn fire(store: &Store, github: &Github) -> Result<Vec<Fired>> {
    let (matched, held) = {
        let mut tx = store.read().await?;
        (
            tx.triggers().unfired_matches(AT_A_TIME).await?,
            tx.triggers()
                .held_due(Timestamp::now() - RECONSIDER, AT_A_TIME)
                .await?,
        )
    };

    let mut fired = Vec::with_capacity(matched.len() + held.len());
    let considered = matched
        .into_iter()
        .map(|matched| (matched, false))
        .chain(held.into_iter().map(|held| (held, true)));
    for ((trigger, event), reconsidering) in considered {
        let consideration = Consideration {
            at: Timestamp::now(),
            reconsidering,
        };
        let readiness = if event.occurrence.r#type.starts_with("com.github.") {
            Some(readiness(store, github, &event).await?)
        } else {
            None
        };
        let mut tx = store.begin().await?;
        // An earlier firing in this sweep may have opened or superseded it.
        if reconsidering && !tx.triggers().still_held(&trigger, &event).await? {
            continue;
        }
        fired.push(
            firing(
                tx,
                &trigger,
                &event,
                Asked::by(&event),
                readiness,
                consideration,
            )
            .await?,
        );
    }
    Ok(fired)
}

async fn readiness(
    store: &Store,
    github: &Github,
    event: &Event,
) -> Result<Result<Readiness, String>> {
    let Some(id) = event.integration else {
        return Ok(Err(
            "the work item has no integration to check readiness".to_owned()
        ));
    };
    let integration = {
        let mut tx = store.begin().await?;
        tx.integrations().with_id(id).await
    };
    Ok(match integration {
        Ok(integration) if integration.github().is_ok() => github
            .readiness(&integration, &event.occurrence)
            .await
            .map_err(|error| error.to_string()),
        Ok(_) => Err("the work item has no GitHub integration to check readiness".to_owned()),
        Err(error) => Err(format!(
            "the work item's integration could not be read: {error}"
        )),
    })
}

/// Fires one Trigger for an issue an operator names, whether or not its filter would match: the
/// request is the authority, so the Event it mints is recorded only as what was asked and why.
pub async fn dispatch(store: &Store, github: &Github, dispatch: Dispatch<'_>) -> Result<Fired> {
    let (trigger, integration) = {
        let mut tx = store.begin().await?;
        let organization = tx.organizations().named(dispatch.organization).await?;
        (
            tx.triggers().named(&organization, dispatch.trigger).await?,
            tx.integrations()
                .named(&organization, dispatch.integration)
                .await?,
        )
    };
    let occurrence = dispatched(
        github,
        &trigger,
        &integration,
        dispatch.issue,
        dispatch.asked,
    )
    .await?;
    // Refused rather than held: an operator waiting on the answer can ask again, and a dispatch
    // that started later would be work nobody is waiting on.
    let readiness = github
        .readiness(&integration, &occurrence)
        .await
        .map_err(|refused| {
            anyhow!("the blockers the dispatch works ahead of are unknown: {refused}")
        })?;

    let mut tx = store.begin().await?;
    if let Recorded::Refused { because } = tx
        .integrations()
        .record_event(&integration, &occurrence)
        .await?
    {
        bail!("the dispatch could not be recorded: {because}");
    }
    let event = tx
        .integrations()
        .recorded_event(&trigger.organization, &occurrence)
        .await?;

    firing(
        tx,
        &trigger,
        &event,
        dispatch.asked,
        Some(Ok(readiness)),
        Consideration {
            at: Timestamp::now(),
            reconsidering: false,
        },
    )
    .await
}

/// Shared by dispatch and test, so a test renders exactly what the dispatch fires.
async fn dispatched(
    github: &Github,
    trigger: &Trigger,
    integration: &Integration,
    issue: i64,
    asked: Asked<'_>,
) -> Result<Occurrence> {
    if let Fires::Scheduled(_) = trigger.fires {
        bail!(
            "the trigger {} fires on a schedule, so it cannot be dispatched",
            trigger.name
        );
    }
    let fetched = github
        .issue(integration, issue)
        .await
        .map_err(|refused| anyhow!("{refused}"))?;

    Ok(github::dispatched(
        integration.github()?,
        DISPATCHED,
        issue,
        serde_json::json!({
            "trigger": trigger.name,
            "instruction": asked.instruction,
            "agent": asked.agent,
            "issue": fetched,
        }),
    ))
}

/// An opening firing atomically commits its Workspace, first entry, Session and record, so a retry
/// never opens its work twice.
async fn firing(
    mut tx: Tx<'_>,
    trigger: &Trigger,
    event: &Event,
    asked: Asked<'_>,
    readiness: Option<Result<Readiness, String>>,
    consideration: Consideration,
) -> Result<Fired> {
    let rendered = render(trigger, event, asked.instruction);

    if let Some(because) = tx.triggers().disabled_because(trigger).await? {
        return failed(
            tx,
            trigger,
            event,
            format!("the trigger {} is disabled: {because}", trigger.name),
        )
        .await;
    }
    // A held firing was counted against the budget when it was first recorded.
    if !consideration.reconsidering
        && tx
            .triggers()
            .firing_budget_is_exhausted(trigger, jiff::Timestamp::now())
            .await?
    {
        let because = trigger.firing_budget_exhausted_because();
        tx.triggers()
            .set_state(trigger, TriggerState::Disabled(DisableReason::FiringBudget))
            .await?;
        return failed(tx, trigger, event, because).await;
    }

    if event.occurrence.r#type == github::COMMENTED
        && EventData::new(&event.occurrence).command().is_none()
    {
        return failed(
            tx,
            trigger,
            event,
            format!("the comment is not a {} command", github::MENTION),
        )
        .await;
    }

    let rendered = match rendered {
        Ok(rendered) => rendered,
        Err(error) => return failed(tx, trigger, event, format!("{error:#}")).await,
    };
    let continues = match &rendered.correlation {
        None => None,
        Some(correlation) => match correlated(&mut tx, trigger, correlation).await? {
            Correlated::Holding(holding) => {
                return match trigger.templates.correlation.on_open_workspace() {
                    OnOpenWorkspace::Continue => fed(tx, trigger, event, &rendered, holding).await,
                    OnOpenWorkspace::NewSession => {
                        started(tx, trigger, event, asked, rendered, holding).await
                    }
                };
            }
            Correlated::Ignored => return ignored(tx, trigger, event, correlation).await,
            Correlated::Missed(sealed) => sealed,
        },
    };

    let agent = match chosen(trigger, event, asked.agent) {
        Ok(agent) => agent,
        Err(error) => return failed(tx, trigger, event, format!("{error:#}")).await,
    };
    let correlation = rendered.correlation.as_deref();
    let worked_ahead = match readiness
        .map(|readiness| readiness.map(|readiness| readiness.decide(request(event))))
    {
        None => None,
        Some(Ok(Decision::Start { worked_ahead })) => worked_ahead,
        Some(Ok(Decision::Hold { because })) => {
            return held(tx, trigger, event, because, correlation, consideration).await;
        }
        Some(Ok(Decision::Cancel { because })) => {
            return canceled(tx, trigger, event, because).await;
        }
        Some(Err(error)) => {
            let because = format!("readiness could not be checked: {error}");
            return held(tx, trigger, event, because, correlation, consideration).await;
        }
    };
    let workspace = tx
        .workspaces()
        .open(Opening {
            organization: &trigger.organization,
            project: &trigger.project,
            agent,
            profile: trigger.profile.as_ref(),
            branch: continues
                .as_deref()
                .map(|sealed| sealed.checkout.branch.as_str())
                .or(rendered.branch.as_deref()),
            correlation: rendered.correlation.as_deref(),
            continues: continues.as_deref(),
            started_by: Some(event),
        })
        .await?;

    tx.log()
        .append(
            &workspace,
            Entry::Brief {
                trigger: Some(trigger.name.clone()),
                brief: rendered.brief,
            },
        )
        .await?;
    tx.log()
        .append(
            &workspace,
            Entry::ParticipantJoined {
                participant: agent.name.clone(),
            },
        )
        .await?;

    let session = tx
        .workspaces()
        .enqueue_session(&workspace, Some(agent), None)
        .await?;
    tx.triggers()
        .record_opened_firing(trigger, event, &workspace, worked_ahead.as_deref())
        .await?;
    if let Some(correlation) = &rendered.correlation {
        tx.triggers()
            .supersede_held(trigger, correlation, event)
            .await?;
    }
    tx.commit().await?;
    fanout::publish(Change::WorkspaceOpened(&workspace));

    Ok(Fired::Opened {
        event: event.record_id,
        workspace: workspace.id,
        session: session.id,
    })
}

async fn fed(
    mut tx: Tx<'_>,
    trigger: &Trigger,
    event: &Event,
    rendered: &Rendered,
    holding: WorkspaceId,
) -> Result<Fired> {
    let workspace = tx.workspaces().get(holding).await?;
    let session = workspace::post_in(&mut tx, &workspace, &trigger.name, &rendered.brief).await?;
    tx.triggers()
        .record_fed_firing(trigger, event, &workspace)
        .await?;
    tx.commit().await?;

    Ok(Fired::Fed {
        event: event.record_id,
        workspace: workspace.id,
        session: session.map(|session| session.id),
    })
}

/// Recorded as feeding the Workspace whether its Session starts now or waits its turn.
async fn started(
    mut tx: Tx<'_>,
    trigger: &Trigger,
    event: &Event,
    asked: Asked<'_>,
    rendered: Rendered,
    holding: WorkspaceId,
) -> Result<Fired> {
    let agent = match chosen(trigger, event, asked.agent) {
        Ok(agent) => agent.clone(),
        Err(error) => return failed(tx, trigger, event, format!("{error:#}")).await,
    };
    let workspace = tx.workspaces().get(holding).await?;
    let session = workspace::start_in(
        &mut tx,
        &workspace,
        PendingSession {
            agent,
            trigger: trigger.name.clone(),
            brief: rendered.brief,
        },
    )
    .await?;
    tx.triggers()
        .record_fed_firing(trigger, event, &workspace)
        .await?;
    tx.commit().await?;

    Ok(Fired::Fed {
        event: event.record_id,
        workspace: workspace.id,
        session: session.map(|session| session.id),
    })
}

async fn ignored(
    mut tx: Tx<'_>,
    trigger: &Trigger,
    event: &Event,
    correlation: &str,
) -> Result<Fired> {
    tx.triggers().record_ignored_firing(trigger, event).await?;
    tx.commit().await?;

    Ok(Fired::Ignored {
        event: event.record_id,
        trigger: trigger.name.clone(),
        correlation: correlation.to_owned(),
    })
}

async fn failed(
    mut tx: Tx<'_>,
    trigger: &Trigger,
    event: &Event,
    because: String,
) -> Result<Fired> {
    tx.triggers()
        .record_failed_firing(trigger, event, &because)
        .await?;
    tx.commit().await?;

    Ok(Fired::Failed {
        event: event.record_id,
        trigger: trigger.name.clone(),
        because,
    })
}

#[derive(Debug, Clone, Copy)]
struct Consideration {
    /// Before readiness was asked, so an event recorded while it was answering is newer.
    at: Timestamp,
    reconsidering: bool,
}

fn request(event: &Event) -> Request<'_> {
    let data = EventData::new(&event.occurrence);
    if event.occurrence.r#type == DISPATCHED {
        Request::Dispatched
    } else if data.command().is_some() {
        Request::Commanded {
            by: data.actor().unwrap_or("a commenter"),
        }
    } else {
        Request::Automatic
    }
}

async fn held(
    mut tx: Tx<'_>,
    trigger: &Trigger,
    event: &Event,
    because: String,
    correlation: Option<&str>,
    consideration: Consideration,
) -> Result<Fired> {
    tx.triggers()
        .record_held_firing(trigger, event, &because, correlation, consideration.at)
        .await?;
    if let Some(correlation) = correlation
        && !consideration.reconsidering
    {
        tx.triggers()
            .supersede_held(trigger, correlation, event)
            .await?;
    }
    tx.commit().await?;

    Ok(Fired::Held {
        event: event.record_id,
        trigger: trigger.name.clone(),
        because,
    })
}

async fn canceled(
    mut tx: Tx<'_>,
    trigger: &Trigger,
    event: &Event,
    because: String,
) -> Result<Fired> {
    tx.triggers()
        .record_canceled_firing(trigger, event, &because)
        .await?;
    tx.commit().await?;

    Ok(Fired::Canceled {
        event: event.record_id,
        trigger: trigger.name.clone(),
        because,
    })
}

async fn set(
    store: &Store,
    organization: &str,
    name: &str,
    state: TriggerState,
) -> Result<Trigger> {
    let mut tx = store.begin().await?;
    let organization = tx.organizations().named(organization).await?;
    let trigger = tx.triggers().named(&organization, name).await?;
    let changed = tx.triggers().set_state(&trigger, state).await?;
    tx.commit().await?;

    Ok(changed)
}
