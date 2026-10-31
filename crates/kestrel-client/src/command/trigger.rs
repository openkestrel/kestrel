use anyhow::{Context as _, Result, bail};
use clap::Subcommand;
use kestrel_operator_types as wire;
use serde_json::{Value, json};

use super::declaration::{
    parsed, read_file, show_applied_whole, show_differences, sign, warn_of_outsiders,
};
use super::input::given;
use super::{Running, empty_list};
use crate::exit::{Exit, Failed};
use crate::{typed, view};

#[derive(Debug, Subcommand)]
pub(crate) enum TriggerCommand {
    /// Make the Organization's applied Triggers what a declaration file says, printing the diff
    Apply {
        /// The declaration file, or `-` for standard input
        #[arg(short = 'f', long, value_name = "FILE")]
        file: String,
        /// Print the diff without making it
        #[arg(long)]
        dry_run: bool,
    },
    /// Declare a Trigger, or make the named one what this declaration describes
    Declare {
        /// The name it is referred to by
        name: String,
        /// The Events it matches: a CloudEvents filter as JSON; `@FILE` reads it from a file
        /// and `-` from standard input
        #[arg(long, value_name = "JSON", required_unless_present_any = ["every", "cron"])]
        filter: Option<String>,
        /// Fire on an interval counted from the declaration, in place of a filter
        #[arg(long, value_name = "DURATION", conflicts_with = "filter")]
        every: Option<String>,
        /// Fire on a five-field cron expression, in place of a filter or an interval
        #[arg(
            long,
            value_name = "EXPRESSION",
            conflicts_with_all = ["filter", "every"],
            requires = "zone"
        )]
        cron: Option<String>,
        /// The IANA time zone the cron expression is read in, such as UTC or America/New_York
        #[arg(long, value_name = "ZONE", requires = "cron")]
        zone: Option<String>,
        /// The Brief a firing hands its Workspace, rendered over an Event; `@FILE` reads it from
        /// a file and `-` from standard input
        #[arg(long)]
        brief: String,
        /// The branch a firing's work happens on, rendered from the Event
        #[arg(long)]
        branch: Option<String>,
        /// The key that finds an open Workspace for this work, rendered from the Event
        #[arg(long)]
        correlation: Option<String>,
        /// What to do when correlation finds no open Workspace: open or ignore
        #[arg(long, value_name = "OPEN|IGNORE")]
        on_miss: Option<String>,
        /// What to do when correlation finds an open Workspace: continue its waiting Session, or
        /// start a new Session with this Trigger's Agent once the unfinished one lets go
        #[arg(long, value_name = "CONTINUE|NEW-SESSION", requires = "correlation")]
        on_open_workspace: Option<String>,
        /// The Project a firing's work happens against
        #[arg(long)]
        project: String,
        /// The Agent a firing starts work with
        #[arg(long)]
        agent: String,
        /// Another Agent an agent:<name> label may choose instead; repeat for many
        #[arg(long = "allow", value_name = "AGENT")]
        allows: Vec<String>,
        /// The Subscription Profile a firing's Sessions use
        #[arg(long)]
        profile: Option<String>,
    },
    /// List every Trigger in the Organization
    #[command(
        after_help = "Empty lists show scoped guidance at a terminal; non-JSON pipes emit nothing."
    )]
    List,
    /// Show a Trigger
    Show { name: String },
    /// Say whether a Trigger matches an Event and what it would render, starting no work
    Test {
        name: String,
        /// The Event's record; absent tests the next elapsing of a scheduled Trigger
        #[arg(long)]
        event: Option<String>,
        /// The GitHub Integration the issue is read through
        #[arg(long, requires = "issue")]
        integration: Option<String>,
        /// Test against the Event a dispatch of this issue would record, recording nothing
        #[arg(
            long,
            value_name = "NUMBER",
            requires = "integration",
            conflicts_with = "event"
        )]
        issue: Option<i64>,
        /// Test the Trigger as a declaration file declares it rather than as it was applied;
        /// `-` for standard input
        #[arg(short = 'f', long, value_name = "FILE")]
        file: Option<String>,
        /// The instruction a dispatch supplies for the Brief to render; `@FILE` reads it from a
        /// file and `-` from standard input
        #[arg(long)]
        instruction: Option<String>,
        /// An Agent the Trigger allows, in place of the one it or a label would choose
        #[arg(long)]
        agent: Option<String>,
    },
    /// Start a Trigger's work on an issue now, whether or not its filter matches anything
    Dispatch {
        /// The Trigger whose work starts
        name: String,
        /// The GitHub Integration the issue is read through, and its Outcome said back through
        #[arg(long)]
        integration: String,
        /// The issue to work on
        #[arg(long, value_name = "NUMBER")]
        issue: i64,
        /// The instruction the Brief reads as `instruction`; `@FILE` reads it from a file and
        /// `-` from standard input
        #[arg(long)]
        instruction: Option<String>,
        /// An Agent the Trigger allows, in place of the one it or a label would choose
        #[arg(long)]
        agent: Option<String>,
    },
    /// Stop a Trigger firing, without forgetting it
    Disable { name: String },
    /// Let a disabled Trigger fire again
    Enable { name: String },
}

pub(crate) async fn run(running: Running<'_>, command: TriggerCommand) -> Result<()> {
    let Running {
        api,
        presentation,
        scoping,
        invocation,
        control_plane,
        ..
    } = running;
    match command {
        TriggerCommand::Declare {
            name,
            filter,
            every,
            cron,
            zone,
            brief,
            branch,
            correlation,
            on_miss,
            on_open_workspace,
            project,
            agent,
            allows,
            profile,
        } => {
            let organization = scoping.resolve().await?.organization;
            if filter.as_deref() == Some("-") && brief == "-" {
                bail!(Failed::new(
                    Exit::Usage,
                    "the filter and the brief cannot both be read from standard input"
                ));
            }
            let filter = filter
                .map(|filter| {
                    serde_json::from_str::<Value>(&given(&filter)?)
                        .context(Failed::new(Exit::Usage, "a trigger filter is JSON"))
                })
                .transpose()?;
            let brief = given(&brief)?;
            let declaration = wire::TriggerDeclaration {
                name,
                brief,
                branch: branch.map(Some),
                correlation: correlation.map(Some),
                on_miss: on_miss.map(Some),
                on_open_workspace: on_open_workspace.map(Some),
                project,
                agent,
                allows: Some(allows),
                profile: profile.map(Some),
                filter: None,
                every: None,
                cron: None,
                zone: None,
                model: None,
                mode: None,
                thought_level: None,
                variant: trigger_condition(filter, every, cron, zone),
            };
            let declared: wire::Trigger = api
                .post_as(&["organizations", &organization, "triggers"], &declaration)
                .await?;
            if declared.admits_outsiders {
                warn_of_outsiders(&declared.name);
            }
            typed::show(presentation, &view::TRIGGER_DECLARED, &declared)?;
        }
        TriggerCommand::Apply { file, dry_run } => {
            let organization = scoping.resolve().await?.organization;
            let declarations = trigger_file(&file)?;
            let mut path = vec!["organizations", organization.as_str(), "applied-triggers"];
            if dry_run {
                path.push("preview");
            }
            let applied: wire::AppliedTriggers = api.post_as(&path, &declarations).await?;
            if invocation.json {
                show_applied_whole(presentation, &applied, &applied.admitting_outsiders)?;
            } else {
                show_applied_triggers(applied);
            }
        }
        TriggerCommand::Dispatch {
            name,
            integration,
            issue,
            instruction,
            agent,
        } => {
            let organization = scoping.resolve().await?.organization;
            let mut dispatch = wire::TriggerDispatch::new(integration, issue);
            dispatch.instruction = instruction.as_deref().map(given).transpose()?.map(Some);
            dispatch.agent = agent.map(Some);
            let fired: wire::Fired = api
                .post_as(
                    &[
                        "organizations",
                        &organization,
                        "triggers",
                        &name,
                        "dispatch",
                    ],
                    &dispatch,
                )
                .await?;
            typed::show_as(
                presentation,
                &view::FIRED,
                &fired,
                &view::FiredShown::of(&fired),
            )?;
        }
        TriggerCommand::List => {
            let organization = scoping.resolve().await?.organization;
            let triggers: wire::ListTriggersResponse = api
                .get_as(&["organizations", &organization, "triggers"])
                .await?;
            typed::collection(
                presentation,
                &view::TRIGGERS,
                &triggers,
                &empty_list(
                    control_plane,
                    "Triggers",
                    Some(&organization),
                    "trigger declare --help",
                ),
            )?;
        }
        TriggerCommand::Show { name } => {
            let organization = scoping.resolve().await?.organization;
            let trigger: wire::Trigger = api
                .get_as(&["organizations", &organization, "triggers", &name])
                .await?;
            typed::show(presentation, &view::TRIGGER, &trigger)?;
        }
        TriggerCommand::Test {
            name,
            event,
            integration,
            issue,
            file,
            instruction,
            agent,
        } => {
            let organization = scoping.resolve().await?.organization;
            if file.as_deref() == Some("-") && instruction.as_deref() == Some("-") {
                bail!(Failed::new(
                    Exit::Usage,
                    "the declaration file and the instruction cannot both be read from standard input"
                ));
            }
            let declared = file.as_deref().map(trigger_document).transpose()?;
            let instruction = instruction.as_deref().map(given).transpose()?;
            let event = event
                .map(|event| {
                    event.parse::<uuid::Uuid>().map_err(|_| {
                        Failed::new(Exit::Usage, format!("{event} is not an Event's record"))
                    })
                })
                .transpose()?;
            let tested: wire::TriggerTestResult = api
                .post_as(
                    &["organizations", &organization, "triggers", &name, "test"],
                    &wire::TriggerTest {
                        event: Some(event),
                        integration: Some(integration),
                        issue: Some(issue),
                        instruction: Some(instruction),
                        agent: Some(agent),
                        declared: Some(declared),
                    },
                )
                .await?;
            typed::show(presentation, &view::TRIGGER_TEST, &tested)?;
        }
        TriggerCommand::Disable { name } => {
            let organization = scoping.resolve().await?.organization;
            let trigger: wire::Trigger = api
                .post_as(
                    &["organizations", &organization, "triggers", &name, "disable"],
                    &json!({}),
                )
                .await?;
            typed::show(presentation, &view::TRIGGER_STATE, &trigger)?;
        }
        TriggerCommand::Enable { name } => {
            let organization = scoping.resolve().await?.organization;
            let trigger: wire::Trigger = api
                .post_as(
                    &["organizations", &organization, "triggers", &name, "enable"],
                    &json!({}),
                )
                .await?;
            typed::show(presentation, &view::TRIGGER_STATE, &trigger)?;
        }
    }

    Ok(())
}

/// An empty file is sent as an empty mapping, so the control plane refuses it for the
/// `triggers` it lacks rather than for being no mapping at all.
fn trigger_file(file: &str) -> Result<Value> {
    let text = read_file(file)?;
    if text.trim().is_empty() {
        return Ok(json!({}));
    }

    parsed(&text)
}

/// The schedule form as the generated union carries it: exactly one of a filter, an interval,
/// or a cron expression with its zone.
fn trigger_condition(
    filter: Option<Value>,
    every: Option<String>,
    cron: Option<String>,
    zone: Option<String>,
) -> wire::TriggerDeclarationVariant {
    match (filter, every, cron) {
        (Some(filter), _, _) => {
            wire::TriggerDeclarationVariant::EventTriggerCondition(json!({ "filter": filter }))
        }
        (None, Some(every), _) => {
            wire::TriggerDeclarationVariant::IntervalTriggerCondition(json!({ "every": every }))
        }
        (None, None, Some(cron)) => wire::TriggerDeclarationVariant::CronTriggerCondition(
            json!({ "cron": cron, "zone": zone }),
        ),
        (None, None, None) => {
            unreachable!("clap requires a filter, an interval or a cron expression")
        }
    }
}

/// The declaration file as the generated request carries it, so a document the control plane
/// could not read is refused before it is sent.
fn trigger_document(file: &str) -> Result<wire::TriggerFile> {
    declaration_document(trigger_file(file)?)
}

fn declaration_document(document: Value) -> Result<wire::TriggerFile> {
    let typed: wire::TriggerFile = serde_json::from_value(document.clone())
        .context(Failed::new(Exit::Usage, "reading the declaration file"))?;
    match unread_field(&document, &serde_json::to_value(&typed)?) {
        Some(field) => bail!(Failed::new(
            Exit::Usage,
            format!("the declaration file: {field}")
        )),
        None => Ok(typed),
    }
}

/// The generated file reads past a field it does not know, so a document with one is refused
/// rather than sent with the field silently dropped.
fn unread_field(sent: &Value, read: &Value) -> Option<String> {
    fn within(sent: &Value, read: &Value, path: &mut Vec<String>) -> Option<String> {
        for (name, value) in sent.as_object()? {
            let Some(kept) = read.get(name) else {
                return Some(name.clone());
            };
            path.push(name.clone());
            if let Some(field) = within(value, kept, path) {
                return Some(field);
            }
            path.pop();
        }

        None
    }

    let mut path = Vec::new();
    let field = within(sent, read, &mut path)?;
    Some(if path.is_empty() {
        format!("unknown field `{field}`")
    } else {
        format!("{}: unknown field `{field}`", path.join("."))
    })
}

fn show_applied_triggers(applied: wire::AppliedTriggers) {
    if applied.changes.is_empty() {
        println!("no changes");
    }
    for change in applied.changes {
        println!("{} {}", sign(&change.action), change.name);
        show_differences(change.differences);
    }
    for name in applied.admitting_outsiders {
        warn_of_outsiders(&name);
    }
}

#[cfg(test)]
mod tests;
