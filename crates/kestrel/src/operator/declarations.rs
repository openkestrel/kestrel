use axum::Json;
use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, State};
use kestrel_operator_types as wire;
use serde::de::DeserializeOwned;

use crate::declaration::{self, triggers};
use crate::integration::github::Github;

use super::{ControlPlane, Refused, malformed, named_refusal};

pub(super) async fn apply_declaration(
    State(control_plane): State<ControlPlane>,
    Path(organization): Path<String>,
    declaration: Result<Json<serde_json::Value>, JsonRejection>,
) -> Result<Json<wire::AppliedDeclaration>, Refused> {
    declared_declaration(
        control_plane,
        organization,
        declaration,
        declaration::ApplyMode::Apply,
    )
    .await
}

pub(super) async fn preview_declaration(
    State(control_plane): State<ControlPlane>,
    Path(organization): Path<String>,
    declaration: Result<Json<serde_json::Value>, JsonRejection>,
) -> Result<Json<wire::AppliedDeclaration>, Refused> {
    declared_declaration(
        control_plane,
        organization,
        declaration,
        declaration::ApplyMode::Preview,
    )
    .await
}

pub(super) async fn declared_declaration(
    control_plane: ControlPlane,
    organization: String,
    declaration: Result<Json<serde_json::Value>, JsonRejection>,
    mode: declaration::ApplyMode,
) -> Result<Json<wire::AppliedDeclaration>, Refused> {
    let Json(sent) = declaration?;
    let declaration = document(closed(&sent).map_err(|why| malformed(None, why))?)?;
    let applied = declaration::apply(
        &control_plane.store,
        &control_plane.images,
        &Github::unfenced()?,
        &organization,
        &declaration,
        mode,
    )
    .await
    .map_err(named_refusal)?;

    Ok(Json(wire::AppliedDeclaration {
        declarations: applied.declarations.into_iter().map(change).collect(),
        admitting_outsiders: applied.admitting_outsiders,
    }))
}

/// A generated type reads past a field it has no place for, so a body whose schema the
/// document closes is read here, where one is refused by its path.
pub(super) fn closed<T: DeserializeOwned>(sent: &serde_json::Value) -> Result<T, String> {
    let mut stray = None;
    let mut ignored = |path: serde_ignored::Path<'_>| {
        stray.get_or_insert_with(|| path.to_string());
    };
    let read =
        serde_path_to_error::deserialize(serde_ignored::Deserializer::new(sent, &mut ignored));
    match stray {
        Some(field) => Err(format!("unknown field `{field}`")),
        None => read.map_err(|error| error.to_string()),
    }
}

/// A body whose schema the document leaves open, refused by the path of what it could not read.
pub(super) fn open<T: DeserializeOwned>(sent: &serde_json::Value) -> Result<T, Refused> {
    serde_path_to_error::deserialize(sent).map_err(|error| malformed(None, error.to_string()))
}

pub(super) fn project_declaration(declaration: wire::ProjectDeclaration) -> declaration::Project {
    declaration::Project {
        name: declaration.name,
        repositories: declaration.repositories,
        integrations: declaration.integrations.unwrap_or_default(),
        branch: declaration.branch,
    }
}

/// The document's trigger fires on a filter alone, which the generated type cannot say.
pub(super) fn document(
    document: wire::DeclarationDocument,
) -> Result<declaration::Document, Refused> {
    let trigger = document.trigger;
    for (field, schedule) in [
        ("trigger.every", &trigger.every),
        ("trigger.cron", &trigger.cron),
        ("trigger.zone", &trigger.zone),
    ] {
        if schedule.is_some() {
            return Err(malformed(
                Some(field),
                "a declaration document's trigger fires on a filter, not a schedule",
            ));
        }
    }

    Ok(declaration::Document {
        project: project_declaration(document.project),
        agent: declaration::Agent {
            name: document.agent.name,
            harness: document.agent.harness,
            model: document.agent.model.flatten(),
            mode: document.agent.mode.flatten(),
            thought_level: document.agent.thought_level.flatten(),
        },
        trigger: declaration::TriggerDeclaration {
            name: trigger.name,
            filter: trigger.filter.ok_or_else(|| {
                malformed(
                    Some("trigger.filter"),
                    "a declaration document's trigger declares its filter",
                )
            })?,
            brief: trigger.brief,
            branch: trigger.branch.flatten(),
            correlation: trigger.correlation.flatten(),
            on_miss: trigger.on_miss.flatten(),
            on_open_workspace: trigger.on_open_workspace.flatten(),
            project: trigger.project,
            agent: trigger.agent,
            allows: trigger.allows.unwrap_or_default(),
            profile: trigger.profile.flatten(),
            model: trigger.model.flatten(),
            mode: trigger.mode.flatten(),
            thought_level: trigger.thought_level.flatten(),
        },
    })
}

/// The generated file flattens its entries out of serde's sight, so each is read alone first
/// for a refusal to name the entry it is in.
pub(super) fn trigger_file(sent: &serde_json::Value) -> Result<triggers::File, Refused> {
    let entries = sent.get("triggers").and_then(serde_json::Value::as_object);
    for (name, entry) in entries.into_iter().flatten() {
        closed::<wire::TriggerFileEntry>(entry)
            .map_err(|why| malformed(None, format!("triggers.{name}: {why}")))?;
    }
    let file: wire::TriggerFile = closed(sent).map_err(|why| malformed(None, why))?;

    Ok(triggers::File {
        triggers: file
            .triggers
            .additional_properties
            .into_iter()
            .map(|(name, entry)| {
                (
                    name,
                    triggers::Entry {
                        filter: entry.filter,
                        brief: entry.brief,
                        branch: entry.branch.flatten(),
                        correlation: entry.correlation.flatten(),
                        on_miss: entry.on_miss.flatten(),
                        on_open_workspace: entry.on_open_workspace.flatten(),
                        project: entry.project,
                        agent: entry.agent,
                        allows: entry.allows.unwrap_or_default(),
                        profile: entry.profile.flatten(),
                        model: entry.model.flatten(),
                        mode: entry.mode.flatten(),
                        thought_level: entry.thought_level.flatten(),
                    },
                )
            })
            .collect(),
    })
}

pub(super) fn change(change: declaration::Declaration) -> wire::DeclarationChange {
    wire::DeclarationChange {
        kind: match change.kind {
            declaration::Kind::Project => wire::DeclarationChangeKind::Project,
            declaration::Kind::Agent => wire::DeclarationChangeKind::Agent,
            declaration::Kind::Trigger => wire::DeclarationChangeKind::Trigger,
        },
        name: change.name,
        action: match change.action {
            declaration::Action::Add => wire::DeclarationChangeAction::Add,
            declaration::Action::Change => wire::DeclarationChangeAction::Change,
            declaration::Action::Unchanged => wire::DeclarationChangeAction::Unchanged,
            declaration::Action::Remove => wire::DeclarationChangeAction::Remove,
        },
        differences: differences(change.differences),
    }
}

pub(super) fn differences(
    differences: Vec<declaration::Difference>,
) -> Vec<wire::DeclarationDifference> {
    differences
        .into_iter()
        .map(|difference| wire::DeclarationDifference {
            field: difference.field.to_owned(),
            was: difference.was,
            becomes: difference.becomes,
        })
        .collect()
}
