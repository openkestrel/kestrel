use axum::Json;
use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use kestrel_operator_types as wire;

use crate::declaration::{self, triggers};
use crate::declined::{Concerning, Reason, Resource, Within};
use crate::domain::{
    CorrelationMiss, Declared, DisableReason, EventRecordId, Fires, OnOpenWorkspace, Schedule,
    Trigger, TriggerState,
};
use crate::integration::github::Github;
use crate::trigger;

use super::{ControlPlane, Refused, differences, malformed, open, trigger_file, wire_id};

impl From<Trigger> for wire::Trigger {
    fn from(trigger: Trigger) -> Self {
        let (filter, every, cron, zone, admits_outsiders) = match trigger.fires {
            Fires::On(filter) => {
                let admits_outsiders = filter.admits_outsiders();
                (Some(filter.to_json()), None, None, None, admits_outsiders)
            }
            Fires::Scheduled(Schedule::Every(every)) => {
                (None, Some(format!("{every:#}")), None, None, false)
            }
            Fires::Scheduled(Schedule::Cron(cron)) => (
                None,
                None,
                Some(cron.to_string()),
                Some(cron.zone().to_owned()),
                false,
            ),
        };
        let correlation = trigger.templates.correlation;

        Self {
            id: wire_id(&trigger.id),
            organization: trigger.organization.name,
            name: trigger.name,
            state: match trigger.state {
                TriggerState::Enabled => wire::TriggerState::Enabled,
                TriggerState::Disabled(DisableReason::Operator) => {
                    wire::TriggerState::DisabledOperator
                }
                TriggerState::Disabled(DisableReason::FiringBudget) => {
                    wire::TriggerState::DisabledFiringBudget
                }
            },
            disabled_because: trigger.disabled_because,
            firing_budget: wire::TriggerFiringBudget {
                limit: i64::try_from(trigger.firing_budget.limit.get())
                    .expect("a firing budget's limit fits the wire"),
                window: format!("{:#}", trigger.firing_budget.window),
            },
            filter,
            every,
            cron,
            zone,
            admits_outsiders,
            brief: trigger.templates.brief.to_string(),
            branch: trigger.templates.branch.map(|branch| branch.to_string()),
            correlation: correlation.template().map(ToString::to_string),
            on_miss: correlation.on_miss().map(|miss| match miss {
                CorrelationMiss::Open => wire::TriggerOnMiss::Open,
                CorrelationMiss::Ignore => wire::TriggerOnMiss::Ignore,
            }),
            on_open_workspace: match correlation.on_open_workspace() {
                OnOpenWorkspace::Continue => wire::TriggerOnOpenWorkspace::Continue_,
                OnOpenWorkspace::NewSession => wire::TriggerOnOpenWorkspace::NewSession,
            },
            project: trigger.project.name,
            agent: trigger.agent.name,
            allows: trigger.allows.into_iter().map(|agent| agent.name).collect(),
            profile: trigger.profile.map(|profile| profile.name),
            model: trigger.declared.model,
            mode: trigger.declared.mode,
            thought_level: trigger.declared.thought_level,
            applied: trigger.applied,
            declared_at: trigger.declared_at.to_string(),
        }
    }
}

pub(super) async fn triggers(
    State(control_plane): State<ControlPlane>,
    Path(organization): Path<String>,
) -> Result<Json<wire::ListTriggersResponse>, Refused> {
    let triggers = trigger::triggers(&control_plane.store, &organization)
        .await
        .map_err(named_refusal)?;

    Ok(Json(triggers.into_iter().map(Into::into).collect()))
}

pub(super) async fn declare_trigger(
    State(control_plane): State<ControlPlane>,
    Path(organization): Path<String>,
    declaration: Result<Json<wire::TriggerDeclaration>, JsonRejection>,
) -> Result<(StatusCode, Json<wire::Trigger>), Refused> {
    let Json(declaration) = declaration?;
    let checked = declaration::check_trigger(&declaration::TriggerFields {
        name: &declaration.name,
        forms: declaration::ScheduleForms {
            filter: declaration.filter.as_ref(),
            every: declaration.every.as_deref(),
            cron: declaration.cron.as_deref(),
            zone: declaration.zone.as_deref(),
        },
        templates: declaration::TemplateFields {
            brief: &declaration.brief,
            branch: declaration.branch.as_ref().and_then(Option::as_deref),
            correlation: declaration.correlation.as_ref().and_then(Option::as_deref),
            on_miss: declaration.on_miss.as_ref().and_then(Option::as_deref),
            on_open_workspace: declaration
                .on_open_workspace
                .as_ref()
                .and_then(Option::as_deref),
        },
        project: &declaration.project,
        agent: &declaration.agent,
        declared: Declared::named(Declared {
            model: declaration.model.clone().flatten(),
            mode: declaration.mode.clone().flatten(),
            thought_level: declaration.thought_level.clone().flatten(),
        }),
        allows: declaration.allows.as_deref().unwrap_or_default(),
        profile: declaration.profile.as_ref().and_then(Option::as_deref),
    })
    .map_err(|error| declaration::scoped(declaration::Prefix::Root, "declare_trigger", error))?;
    let mut tx = control_plane.store.begin().await?;
    let organization = tx.organizations().named(&organization).await?;
    let placed = declaration::place_trigger(
        &mut tx,
        &organization,
        &checked,
        declaration::Provenance::Flags,
    )
    .await?;
    tx.commit().await?;

    Ok((
        match placed.change.action {
            declaration::Action::Add => StatusCode::CREATED,
            _ => StatusCode::OK,
        },
        Json(placed.trigger.into()),
    ))
}

pub(super) async fn show_trigger(
    State(control_plane): State<ControlPlane>,
    Path((organization, name)): Path<(String, String)>,
) -> Result<Json<wire::Trigger>, Refused> {
    let trigger = trigger::show(&control_plane.store, &organization, &name)
        .await
        .map_err(named_refusal)?;

    Ok(Json(trigger.into()))
}

pub(super) async fn test_trigger(
    State(control_plane): State<ControlPlane>,
    Path((organization, name)): Path<(String, String)>,
    tested: Result<Json<serde_json::Value>, JsonRejection>,
) -> Result<Json<wire::TriggerTestResult>, Refused> {
    let Json(sent) = tested?;
    let tested: wire::TriggerTest = open(&sent)?;
    let declared = match tested.declared.flatten() {
        Some(_) => Some(trigger_file(&sent["declared"])?),
        None => None,
    };
    let event: Option<EventRecordId> = tested.event.flatten().map(|event| {
        event
            .to_string()
            .parse()
            .expect("an Event's identifier is a UUID")
    });
    let integration = tested.integration.flatten();
    let instruction = tested.instruction.flatten();
    let agent = tested.agent.flatten();
    let github;
    let against = match (event, integration.as_deref(), tested.issue.flatten()) {
        (Some(_), _, Some(_)) => {
            return Err(malformed(
                Some("issue"),
                "a test renders against an event or an issue, not both",
            ));
        }
        (Some(event), _, None) => trigger::Against::Event(event),
        (None, Some(integration), Some(issue)) => {
            github = Github::dialling_out(&control_plane.store)?;
            trigger::Against::Issue {
                github: &github,
                integration,
                issue,
            }
        }
        (None, None, Some(_)) => {
            return Err(malformed(
                Some("integration"),
                "an issue is read through an integration, so a test naming one names both",
            ));
        }
        (None, Some(_), None) => {
            return Err(malformed(
                Some("issue"),
                "an integration is read for an issue, so a test naming one names both",
            ));
        }
        (None, None, None) => trigger::Against::NextElapsing,
    };
    let asked = trigger::Asked {
        instruction: instruction.as_deref(),
        agent: agent.as_deref(),
    };
    let tested = match declared {
        Some(file) => {
            let declarations = triggers::declarations(file)
                .map_err(|error| {
                    declaration::scoped(declaration::Prefix::Root, "test_trigger", error)
                })
                .map_err(named_refusal)?;
            let declared = declarations
                .iter()
                .find(|declared| declared.name() == name)
                .ok_or_else(|| {
                    named_refusal(
                        Reason::MissingReference {
                            resource: Resource::Trigger,
                            reference: name.clone(),
                            organization: Some(organization.clone()),
                            within: None,
                            message: format!("the declaration file declares no trigger {name}"),
                        }
                        .into(),
                    )
                })?;
            trigger::test_declared(
                &control_plane.store,
                &organization,
                declared,
                against,
                asked,
            )
            .await
        }
        None => trigger::test(&control_plane.store, &organization, &name, against, asked).await,
    }
    .map_err(named_refusal)?;
    let rendered = tested
        .rendered
        .map_err(|error| Refused::Unprocessable(error.to_string()))?;
    let agent = tested
        .agent
        .map_err(|error| Refused::Unprocessable(error.to_string()))?;
    let would = tested
        .would
        .ok_or_else(|| anyhow::anyhow!("a trigger that rendered says what its firing would do"))?;

    Ok(Json(wire::TriggerTestResult {
        matches: tested.matches,
        brief: rendered.brief,
        branch: rendered.branch,
        correlation: rendered.correlation,
        agent,
        would: match would {
            trigger::Would::Open => wire::TriggerTestResultWould::OpenWorkspace,
            trigger::Would::Continue => wire::TriggerTestResultWould::ContinueSession,
            trigger::Would::NewSession => wire::TriggerTestResultWould::NewSession,
            trigger::Would::Ignore => wire::TriggerTestResultWould::Ignore,
        },
        elapsing: tested.elapsing.map(|elapsing| elapsing.to_string()),
    }))
}

pub(super) async fn disable_trigger(
    State(control_plane): State<ControlPlane>,
    Path((organization, name)): Path<(String, String)>,
) -> Result<Json<wire::Trigger>, Refused> {
    let trigger = trigger::disable(&control_plane.store, &organization, &name)
        .await
        .map_err(named_refusal)?;

    Ok(Json(trigger.into()))
}

pub(super) async fn enable_trigger(
    State(control_plane): State<ControlPlane>,
    Path((organization, name)): Path<(String, String)>,
) -> Result<Json<wire::Trigger>, Refused> {
    let trigger = trigger::enable(&control_plane.store, &organization, &name)
        .await
        .map_err(named_refusal)?;

    Ok(Json(trigger.into()))
}

pub(super) async fn dispatch_trigger(
    State(control_plane): State<ControlPlane>,
    Path((organization, name)): Path<(String, String)>,
    dispatch: Result<Json<wire::TriggerDispatch>, JsonRejection>,
) -> Result<Json<wire::Fired>, Refused> {
    let Json(dispatch) = dispatch?;
    let instruction = dispatch.instruction.flatten();
    let agent = dispatch.agent.flatten();
    let fired = trigger::dispatch(
        &control_plane.store,
        &Github::dialling_out(&control_plane.store)?,
        trigger::Dispatch {
            organization: &organization,
            trigger: &name,
            integration: &dispatch.integration,
            issue: dispatch.issue,
            asked: trigger::Asked {
                instruction: instruction.as_deref(),
                agent: agent.as_deref(),
            },
        },
    )
    .await
    .map_err(named_refusal)?;

    Ok(Json(match fired {
        trigger::Fired::Opened {
            event,
            workspace,
            session,
        } => wire::Fired::FiredOpened(wire::FiredOpened {
            outcome: serde_json::json!("opened"),
            event: wire_id(&event),
            workspace: wire_id(&workspace),
            session: wire_id(&session),
        }),
        trigger::Fired::Fed {
            event,
            workspace,
            session,
        } => wire::Fired::FiredFed(wire::FiredFed {
            outcome: serde_json::json!("fed"),
            event: wire_id(&event),
            workspace: wire_id(&workspace),
            session: session.map(|session| wire_id(&session)),
        }),
        trigger::Fired::Ignored {
            event, correlation, ..
        } => wire::Fired::FiredIgnored(wire::FiredIgnored {
            outcome: serde_json::json!("ignored"),
            event: wire_id(&event),
            correlation,
        }),
        trigger::Fired::Failed { because, .. }
        | trigger::Fired::Held { because, .. }
        | trigger::Fired::Canceled { because, .. } => {
            return Err(Refused::Unprocessable(because));
        }
    }))
}

pub(super) async fn apply_triggers(
    State(control_plane): State<ControlPlane>,
    Path(organization): Path<String>,
    file: Result<Json<serde_json::Value>, JsonRejection>,
) -> Result<Json<wire::AppliedTriggers>, Refused> {
    applied_triggers(control_plane, organization, file, false).await
}

pub(super) async fn preview_applied_triggers(
    State(control_plane): State<ControlPlane>,
    Path(organization): Path<String>,
    file: Result<Json<serde_json::Value>, JsonRejection>,
) -> Result<Json<wire::AppliedTriggers>, Refused> {
    applied_triggers(control_plane, organization, file, true).await
}

pub(super) async fn applied_triggers(
    control_plane: ControlPlane,
    organization: String,
    file: Result<Json<serde_json::Value>, JsonRejection>,
    dry_run: bool,
) -> Result<Json<wire::AppliedTriggers>, Refused> {
    let Json(sent) = file?;
    let file = trigger_file(&sent)?;
    let operation = if dry_run {
        "preview_applied_triggers"
    } else {
        "apply_triggers"
    };
    let declarations = triggers::declarations(file)
        .map_err(|error| declaration::scoped(declaration::Prefix::Root, operation, error))
        .map_err(named_refusal)?;
    let applied = triggers::apply(&control_plane.store, &organization, &declarations, dry_run)
        .await
        .map_err(named_refusal)?;

    Ok(Json(wire::AppliedTriggers {
        changes: applied
            .changes
            .into_iter()
            .map(|change| wire::DeclarationChange {
                kind: wire::DeclarationChangeKind::Trigger,
                name: change.name,
                action: match change.action {
                    triggers::Action::Add => wire::DeclarationChangeAction::Add,
                    triggers::Action::Change => wire::DeclarationChangeAction::Change,
                    triggers::Action::Remove => wire::DeclarationChangeAction::Remove,
                },
                differences: differences(change.differences),
            })
            .collect(),
        admitting_outsiders: applied.admitting_outsiders,
    }))
}

/// An untyped refusal here is about what the request named, never a control plane that could
/// not answer.
pub(super) fn named_refusal(error: anyhow::Error) -> Refused {
    if error.is::<Reason>() || error.is::<Concerning>() || error.is::<Within>() {
        return error.into();
    }
    Refused::Unprocessable(error.to_string())
}
