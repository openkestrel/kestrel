use std::io::Write as _;

use anyhow::{Context as _, Result};
use kestrel_operator_types as wire;
use serde::Serialize;

use crate::diagnostic::{self, Invocation};
use crate::output::Presentation;
use crate::{typed, view};

#[derive(Serialize)]
pub(crate) struct Row {
    pub(crate) position: Option<i64>,
    pub(crate) name: String,
    pub(crate) state: String,
    pub(crate) agent: String,
    pub(crate) why: String,
    pub(crate) workspace: String,
    pub(crate) enqueued_at: String,
}

pub fn show(
    presentation: &Presentation,
    snapshot: &wire::Queue,
    invocation: &Invocation,
) -> Result<()> {
    if presentation.json() {
        let mut out = std::io::stdout().lock();
        crate::output::document(&mut out, presentation, &serde_json::to_value(snapshot)?)?;
        return out.flush().context("writing to standard output");
    }
    let slots = &snapshot.active_work;
    let instances = &snapshot.instances;
    {
        let mut out = std::io::stdout().lock();
        let occupants: Vec<_> = slots
            .occupants
            .iter()
            .map(|occupant| occupant.name.as_str())
            .collect();
        let counted: Vec<_> = instances.counted.iter().map(String::as_str).collect();
        let said = [
            (
                "active-work slots",
                occupancy(
                    slots.limit,
                    slots.occupied,
                    &occupants,
                    "no dispatch configuration recorded",
                    "occupied",
                ),
            ),
            (
                "live instances",
                occupancy(
                    instances.limit,
                    instances.count,
                    &counted,
                    "unbounded",
                    "counted",
                ),
            ),
            (
                "environment",
                snapshot
                    .work_role
                    .as_ref()
                    .map_or("no dispatch configuration recorded", |role| {
                        role.driver.as_str()
                    })
                    .to_owned(),
            ),
        ];
        let width = said
            .iter()
            .map(|(label, _)| label.chars().count())
            .max()
            .unwrap_or_default();
        for (label, line) in said {
            writeln!(out, "{label:width$}  {line}").context("writing the queue's limits")?;
        }
        out.flush().context("writing to standard output")?;
    }
    let mut rows = Vec::new();
    let mut diagnoses = Vec::new();
    for occupant in &slots.occupants {
        rows.push(Row {
            position: None,
            name: occupant.name.clone(),
            state: occupant.phase.to_string(),
            agent: occupant.agent.clone(),
            workspace: occupant.workspace.to_string(),
            enqueued_at: occupant.enqueued_at.clone(),
            why: match occupant.phase {
                wire::OccupantPhase::Working => "in a turn",
                wire::OccupantPhase::Trailing => "answered; its agent is still working",
            }
            .to_owned(),
        });
    }
    for queued in &snapshot.queued {
        rows.push(Row {
            position: queued.position,
            name: queued.name.clone(),
            state: "queued".to_owned(),
            agent: queued.agent.clone(),
            workspace: queued.workspace.to_string(),
            enqueued_at: queued.enqueued_at.clone(),
            why: why(
                "queued",
                None,
                None,
                None,
                &queued.reasons,
                invocation,
                presentation,
            ),
        });
        diagnoses.extend(diagnostics(&queued.name, &queued.reasons));
    }
    for waiting in &snapshot.waiting {
        rows.push(Row {
            position: waiting.position,
            name: waiting.name.clone(),
            state: "waiting".to_owned(),
            agent: waiting.agent.clone(),
            workspace: waiting.workspace.to_string(),
            enqueued_at: waiting.enqueued_at.clone(),
            why: why(
                "waiting",
                None,
                None,
                waiting.pending_since.as_deref(),
                &waiting.reasons,
                invocation,
                presentation,
            ),
        });
        diagnoses.extend(diagnostics(&waiting.name, &waiting.reasons));
    }
    for unbriefed in &snapshot.unbriefed {
        rows.push(Row {
            position: unbriefed.position,
            name: unbriefed.name.clone(),
            state: "unbriefed".to_owned(),
            agent: unbriefed.agent.clone(),
            workspace: unbriefed.workspace.to_string(),
            enqueued_at: unbriefed.enqueued_at.clone(),
            why: why(
                "unbriefed",
                unbriefed.preparing.as_ref(),
                unbriefed.brief_since.as_deref(),
                unbriefed.pending_since.as_deref(),
                &unbriefed.reasons,
                invocation,
                presentation,
            ),
        });
        diagnoses.extend(diagnostics(&unbriefed.name, &unbriefed.reasons));
    }
    if !rows.is_empty() {
        typed::collection(presentation, &view::QUEUE, &rows, "")?;
    }
    if matches!(presentation, Presentation::Human(_)) {
        let mut out = std::io::stdout().lock();
        for (session, diagnostic) in diagnoses {
            writeln!(
                out,
                "\n{session}:\n{}",
                diagnostic::inline(diagnostic, invocation)
            )
            .context("writing the queue's sign-in repairs")?;
        }
        out.flush().context("writing to standard output")?;
    }
    Ok(())
}

fn diagnostics<'a>(
    name: &'a str,
    reasons: &'a [wire::QueueReason],
) -> impl Iterator<Item = (&'a str, &'a wire::Diagnostic)> {
    reasons.iter().filter_map(move |reason| match reason {
        wire::QueueReason::QueueReasonSubscriptionProfile(reason) => {
            Some((name, &reason.diagnostic))
        }
        _ => None,
    })
}

fn why(
    state: &str,
    preparing: Option<&wire::UnbriefedSessionPreparing>,
    brief_since: Option<&str>,
    pending_since: Option<&str>,
    reasons: &[wire::QueueReason],
    invocation: &Invocation,
    presentation: &Presentation,
) -> String {
    let mut said = Vec::new();
    if let Some(preparing) = preparing {
        said.push(
            match preparing {
                wire::UnbriefedSessionPreparing::StartingHarness => "starting harness",
                wire::UnbriefedSessionPreparing::HarnessReady => "harness ready",
                step => step.as_str(),
            }
            .to_owned(),
        );
    }
    if let Some(since) = brief_since {
        said.push(format!("Brief since {since}"));
    }
    if let Some(since) = pending_since {
        said.push(format!("input since {since}"));
    }
    said.extend(
        reasons
            .iter()
            .map(|reason| reason_in_words(reason, invocation, presentation)),
    );
    if said.is_empty() {
        said.push(
            match state {
                "waiting" => "waiting for a turn",
                "unbriefed" => "getting ready for its first message",
                _ => "ready",
            }
            .to_owned(),
        );
    }
    said.join("; ")
}

pub(crate) fn reason_in_words(
    reason: &wire::QueueReason,
    invocation: &Invocation,
    presentation: &Presentation,
) -> String {
    use wire::QueueReason::*;
    match reason {
        QueueReasonDependencies(reason) => format!("waits on {}", reason.sessions.join(", ")),
        QueueReasonSubscriptionProfile(reason) => {
            if matches!(presentation, Presentation::Human(_)) {
                diagnostic::message(&reason.diagnostic).to_owned()
            } else {
                diagnostic::inline(&reason.diagnostic, invocation)
            }
        }
        QueueReasonInstanceArchiving(reason) => {
            format!("waits for the Instance {} to be archived", reason.instance)
        }
        QueueReasonLiveInstanceLimit(reason) => format!(
            "at the limit of {} live Instance{}",
            reason.limit,
            plural(reason.limit)
        ),
        QueueReasonActiveWorkSlots(reason) => format!(
            "all {} Active-Work Slot{} occupied",
            reason.limit,
            plural(reason.limit)
        ),
        QueueReasonAhead(reason) => {
            let mut ahead = reason.sessions.join(", ");
            if let Some(elsewhere) = reason.elsewhere {
                if !ahead.is_empty() {
                    ahead.push_str(", ");
                }
                ahead.push_str(&format!(
                    "{elsewhere} Session{} in other Organizations",
                    plural(elsewhere)
                ));
            }
            format!("behind {ahead}")
        }
    }
}

fn plural(count: i64) -> &'static str {
    if count == 1 { "" } else { "s" }
}

fn occupancy(
    limit: Option<i64>,
    counted: i64,
    named: &[&str],
    without_limit: &str,
    unit: &str,
) -> String {
    let limit = limit.map_or_else(
        || format!("{without_limit};"),
        |limit| format!("limit {limit};"),
    );
    let mut line = format!("{limit} {counted} {unit}");
    if !named.is_empty() {
        line.push_str(&format!(" ({})", named.join(", ")));
    }
    line
}
