use std::collections::BTreeMap;

use anyhow::{Context as _, Result};
use serde::{Deserialize, Serialize};

use crate::declaration;
use crate::domain::Fires;
use crate::filter::Filter;
use crate::store::Store;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Declared {
    pub name: String,
    pub filter: Filter,
    pub templates: crate::domain::Templates,
    pub project: String,
    pub agent: String,
    pub declared: crate::domain::Declared,
    pub allows: Vec<String>,
    pub profile: Option<String>,
}

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Difference {
    pub field: &'static str,
    pub was: Option<String>,
    pub becomes: Option<String>,
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

pub fn parse(text: &str) -> Result<Vec<Declared>> {
    declarations(yaml_serde::from_str(text).context("reading the declaration file")?)
}

pub fn declarations(file: File) -> Result<Vec<Declared>> {
    file.triggers
        .into_iter()
        .map(|(name, entry)| declared(&name, entry))
        .collect()
}

fn declared(name: &str, entry: Entry) -> Result<Declared> {
    if name.is_empty() {
        return Err(declaration::invalid(
            "name",
            crate::declined::Constraint::NonEmpty,
            "a declaration names each record",
        ));
    }
    let (filter, templates) = declaration::parse_filter_trigger(
        &entry.filter,
        &declaration::TemplateFields {
            brief: &entry.brief,
            branch: entry.branch.as_deref(),
            correlation: entry.correlation.as_deref(),
            on_miss: entry.on_miss.as_deref(),
            on_open_workspace: entry.on_open_workspace.as_deref(),
        },
    )
    .map_err(|error| named(name, error))?;

    Ok(Declared {
        name: name.to_owned(),
        filter,
        templates,
        project: entry.project,
        agent: entry.agent,
        declared: crate::domain::Declared::named(crate::domain::Declared {
            model: entry.model,
            mode: entry.mode,
            thought_level: entry.thought_level,
        }),
        allows: entry.allows,
        profile: entry.profile,
    })
}

fn named(name: &str, error: anyhow::Error) -> anyhow::Error {
    match error.downcast::<declaration::Invalid>() {
        Ok(invalid) => declaration::invalid(
            invalid.field,
            invalid.constraint,
            format!("the trigger {name}: {}", invalid.message),
        ),
        Err(error) => error,
    }
}

fn checked(declared: &Declared) -> declaration::CheckedTrigger {
    declaration::CheckedTrigger::new(declaration::TriggerProposal {
        name: declared.name.clone(),
        fires: Fires::On(declared.filter.clone()),
        templates: declared.templates.clone(),
        project: declared.project.clone(),
        agent: declared.agent.clone(),
        declared: declared.declared.clone(),
        allows: declared.allows.clone(),
        profile: declared.profile.clone(),
    })
}

/// One transaction, so a declaration naming a project or agent that does not exist changes
/// nothing; a dry run is that transaction rolled back.
pub async fn apply(
    store: &Store,
    organization: &str,
    declarations: &[Declared],
    dry_run: bool,
) -> Result<Applied> {
    let mut tx = store.begin().await?;
    let organization = tx.organizations().named(organization).await?;
    let existing = tx.triggers().all(&organization).await?;
    let mut changes = Vec::new();

    for declared in declarations {
        let checked = checked(declared);
        let refs = declaration::resolve_trigger_refs(&mut tx, &organization, &checked).await?;
        let change = declaration::trigger_change(
            existing
                .iter()
                .find(|trigger| trigger.name == declared.name),
            &checked,
        );
        let trigger = existing
            .iter()
            .find(|trigger| trigger.name == declared.name);
        match change.action {
            declaration::Action::Add => {
                declaration::insert_trigger(
                    &mut tx,
                    &organization,
                    &refs,
                    &checked,
                    declaration::Provenance::File,
                )
                .await?;
                changes.push(changed(&change));
            }
            declaration::Action::Change => {
                declaration::update_trigger(
                    &mut tx,
                    trigger.expect("a changed trigger was listed"),
                    &refs,
                    &checked,
                    declaration::Provenance::File,
                )
                .await?;
                changes.push(changed(&change));
            }
            declaration::Action::Unchanged | declaration::Action::Remove => {
                let trigger = trigger.expect("an unchanged trigger was listed");
                if !trigger.applied {
                    tx.triggers().adopt(trigger).await?;
                }
                let mut differences = change
                    .differences
                    .iter()
                    .map(|difference| Difference {
                        field: difference.field,
                        was: difference.was.clone(),
                        becomes: difference.becomes.clone(),
                    })
                    .collect::<Vec<_>>();
                if !trigger.applied {
                    differences.insert(
                        0,
                        Difference {
                            field: "declared by",
                            was: Some("flags".to_owned()),
                            becomes: Some("a file".to_owned()),
                        },
                    );
                }
                if !differences.is_empty() {
                    changes.push(Change {
                        name: declared.name.clone(),
                        action: Action::Change,
                        differences,
                    });
                }
            }
        }
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
            .filter(|declared| declared.filter.admits_outsiders())
            .map(|declared| declared.name.clone())
            .collect(),
    })
}

fn changed(change: &declaration::Declaration) -> Change {
    Change {
        name: change.name.clone(),
        action: match change.action {
            declaration::Action::Add => Action::Add,
            declaration::Action::Change
            | declaration::Action::Unchanged
            | declaration::Action::Remove => Action::Change,
        },
        differences: change
            .differences
            .iter()
            .map(|difference| Difference {
                field: difference.field,
                was: difference.was.clone(),
                becomes: difference.becomes.clone(),
            })
            .collect(),
    }
}
