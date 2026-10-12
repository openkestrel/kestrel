//! The applied-Triggers file: the one declaration path that holds a set, so reconciling it
//! (adopting what flags declared, removing what the file dropped) lives here.

use std::collections::BTreeMap;

use anyhow::{Context as _, Result};
use serde::{Deserialize, Serialize};

use super::{
    CheckedTrigger, Difference, Provenance, ScheduleForms, TemplateFields, TriggerFields,
    check_trigger, place_trigger, under,
};
use crate::domain::Declared;
use crate::store::Store;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Change {
    pub name: String,
    pub action: Action,
    pub differences: Vec<Difference>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Add,
    Change,
    Remove,
}

#[derive(Debug, Serialize)]
pub struct Applied {
    pub changes: Vec<Change>,
    pub admitting_outsiders: Vec<String>,
}

/// `triggers` is required, so an empty file is a mistake rather than every applied trigger
/// removed; `triggers: {}` says that on purpose.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct File {
    triggers: BTreeMap<String, Entry>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    filter: serde_json::Value,
    brief: String,
    branch: Option<String>,
    correlation: Option<String>,
    on_miss: Option<String>,
    on_open_workspace: Option<String>,
    project: String,
    agent: String,
    #[serde(default)]
    allows: Vec<String>,
    profile: Option<String>,
    model: Option<String>,
    mode: Option<String>,
    thought_level: Option<String>,
}

pub fn parse(text: &str) -> Result<Vec<CheckedTrigger>> {
    declarations(yaml_serde::from_str(text).context("reading the declaration file")?)
}

/// A refused field is reported under the entry it was read from, `triggers.<name>`.
pub fn declarations(file: File) -> Result<Vec<CheckedTrigger>> {
    file.triggers
        .into_iter()
        .map(|(name, entry)| {
            check_trigger(&TriggerFields {
                name: &name,
                forms: ScheduleForms::filter(&entry.filter),
                templates: TemplateFields {
                    brief: &entry.brief,
                    branch: entry.branch.as_deref(),
                    correlation: entry.correlation.as_deref(),
                    on_miss: entry.on_miss.as_deref(),
                    on_open_workspace: entry.on_open_workspace.as_deref(),
                },
                project: &entry.project,
                agent: &entry.agent,
                declared: Declared {
                    model: entry.model.clone(),
                    mode: entry.mode.clone(),
                    thought_level: entry.thought_level.clone(),
                },
                allows: &entry.allows,
                profile: entry.profile.as_deref(),
            })
            .map_err(|error| under(format!("triggers.{name}"), error))
        })
        .collect()
}

/// One transaction, so a declaration naming a project or agent that does not exist changes
/// nothing; a dry run is that transaction rolled back.
pub async fn apply(
    store: &Store,
    organization: &str,
    declarations: &[CheckedTrigger],
    dry_run: bool,
) -> Result<Applied> {
    let mut tx = store.begin().await?;
    let organization = tx.organizations().named(organization).await?;
    let existing = tx.triggers().all(&organization).await?;
    let mut changes = Vec::new();

    for checked in declarations {
        let placed = place_trigger(&mut tx, &organization, checked, Provenance::File).await?;
        let mut differences = placed.change.differences;
        let action = match placed.change.action {
            super::Action::Add => Action::Add,
            super::Action::Change => Action::Change,
            super::Action::Unchanged | super::Action::Remove => {
                if !placed.trigger.applied {
                    tx.triggers().adopt(&placed.trigger).await?;
                    differences.insert(
                        0,
                        Difference {
                            field: "declared by",
                            was: Some("flags".to_owned()),
                            becomes: Some("a file".to_owned()),
                        },
                    );
                }
                if differences.is_empty() {
                    continue;
                }
                Action::Change
            }
        };
        changes.push(Change {
            name: checked.name.clone(),
            action,
            differences,
        });
    }

    for trigger in &existing {
        if trigger.applied
            && !declarations
                .iter()
                .any(|declared| declared.name == trigger.name)
        {
            tx.triggers().remove(trigger).await?;
            changes.push(Change {
                name: trigger.name.clone(),
                action: Action::Remove,
                differences: Vec::new(),
            });
        }
    }

    if !dry_run {
        tx.commit().await?;
    }
    changes.sort_by(|a, b| a.name.cmp(&b.name));

    Ok(Applied {
        changes,
        admitting_outsiders: declarations
            .iter()
            .filter(|declared| declared.admits_outsiders())
            .map(|declared| declared.name.clone())
            .collect(),
    })
}
