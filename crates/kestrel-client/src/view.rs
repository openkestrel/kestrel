//! What each command shows, and the columns it reads those fields through.

use kestrel_operator_types as wire;

use crate::typed::{Column, Columns, compound, joined, object, objects, or_absent, text};

pub const OPERATOR: Columns<wire::Operator> = Columns::Detail(&[
    Column("id", |operator| text(operator.id)),
    Column("name", |operator| text(&operator.name)),
]);
pub const ORGANIZATIONS: Columns<wire::Organization> = Columns::Rows(&[
    Column("id", |organization| text(organization.id)),
    Column("name", |organization| text(&organization.name)),
    Column("max live instances", |organization| {
        or_absent(organization.max_live_instances.as_ref())
    }),
]);
pub const ORGANIZATION_DECLARED: Columns<wire::Organization> =
    Columns::Value(Column("id", |organization| text(organization.id)));
pub const PROJECTS: Columns<wire::Project> = Columns::Rows(&[
    Column("id", |project| text(project.id)),
    Column("name", |project| text(&project.name)),
    Column("branch", |project| text(&project.branch)),
    Column("repositories", |project| joined(&project.repositories)),
]);
pub const PROJECT_DECLARED: Columns<wire::Project> =
    Columns::Value(Column("id", |project| text(project.id)));
pub const AGENTS: Columns<wire::Agent> = Columns::Rows(&[
    Column("id", |agent| text(agent.id)),
    Column("name", |agent| text(&agent.name)),
    Column("harness", |agent| text(&agent.harness)),
    Column("model", |agent| or_absent(agent.model.as_ref())),
]);
pub const AGENT_DECLARED: Columns<wire::Agent> =
    Columns::Value(Column("id", |agent| text(agent.id)));
pub const AGENT_MODEL: Columns<wire::Agent> =
    Columns::Value(Column("model", |agent| or_absent(agent.model.as_ref())));
pub const CREDENTIAL: Columns<wire::ProviderCredential> =
    Columns::Value(Column("variable", |credential| text(&credential.variable)));
pub const CREDENTIALS: Columns<wire::ProviderCredential> = Columns::Rows(&[
    Column("variable", |credential| text(&credential.variable)),
    Column("set at", |credential| text(&credential.set_at)),
]);
pub const PROFILE_DECLARED: Columns<wire::SubscriptionProfile> =
    Columns::Value(Column("id", |profile| text(profile.id)));
pub const PROFILES: Columns<wire::SubscriptionProfileListed> = Columns::Rows(&[
    Column("id", |profile| text(profile.id)),
    Column("name", |profile| text(&profile.name)),
    Column("owner", |profile| text(&profile.owner)),
    Column("holds", |profile| objects(&profile.holds)),
]);
pub const LOGIN: Columns<wire::SubscriptionProfileLogin> =
    Columns::Value(Column("name", |login| text(&login.name)));
pub const GITHUB_CONNECTED: Columns<wire::GithubAppFlow> =
    Columns::Value(Column("connected", |flow| {
        format!(
            "GitHub connected · Integration {} · Organization {} · repository {}",
            flow.name,
            flow.organization,
            or_absent(flow.repository.as_ref())
        )
    }));
pub const INTEGRATION_WEBHOOK: Columns<wire::IntegrationWebhook> = Columns::Detail(&[
    Column("configured URL", |hook| text(&hook.url)),
    Column("unfinished URL update", |hook| {
        or_absent(hook.pending_url.as_ref())
    }),
    Column("recent Deliveries", |hook| objects(&hook.deliveries)),
]);

pub const INTEGRATIONS: Columns<wire::Integration> = Columns::Rows(&[
    Column("id", |integration| text(integration.id)),
    Column("name", |integration| text(&integration.name)),
    Column("kind", |integration| text(&integration.kind)),
    Column("repository", |integration| {
        or_absent(integration.repository.as_ref())
    }),
    Column("carries", |integration| joined(&integration.carries)),
    Column("state", |integration| text(&integration.state)),
    Column("revision", |integration| text(integration.revision)),
    Column("polled every", |integration| {
        or_absent(integration.polled_every.as_ref())
    }),
    Column("webhook path", |integration| {
        or_absent(integration.webhook_path.as_ref())
    }),
    Column("reason", |integration| {
        or_absent(
            integration
                .last_event_refusal
                .as_ref()
                .map(|refusal| &refusal.reason),
        )
    }),
]);
pub const INTEGRATION_DECLARED: Columns<wire::Integration> =
    Columns::Value(Column("id", |integration| text(integration.id)));

/// An Integration as a person reads it: the record the boundary answered, with the two cells a
/// decoded record answers only with the invocation at hand.
pub struct IntegrationShown {
    pub integration: wire::Integration,
    pub canceled_posts: String,
    pub diagnostic: String,
    pub rotation: String,
}

pub const INTEGRATION: Columns<IntegrationShown> = Columns::Detail(&[
    Column("id", |shown| text(shown.integration.id)),
    Column("name", |shown| text(&shown.integration.name)),
    Column("kind", |shown| text(&shown.integration.kind)),
    Column("repository", |shown| {
        or_absent(shown.integration.repository.as_ref())
    }),
    Column("bot login", |shown| {
        or_absent(shown.integration.bot_login.as_ref())
    }),
    Column("carries", |shown| joined(&shown.integration.carries)),
    Column("state", |shown| text(&shown.integration.state)),
    Column("revision", |shown| text(shown.integration.revision)),
    Column("disabled at", |shown| {
        or_absent(shown.integration.disabled_at.as_ref())
    }),
    Column("retired at", |shown| {
        or_absent(shown.integration.retired_at.as_ref())
    }),
    Column("canceled posts", |shown| text(&shown.canceled_posts)),
    Column("diagnostic", |shown| text(&shown.diagnostic)),
    Column("polled every", |shown| {
        or_absent(shown.integration.polled_every.as_ref())
    }),
    Column("webhook path", |shown| {
        or_absent(shown.integration.webhook_path.as_ref())
    }),
    Column("last event refusal", |shown| {
        object(shown.integration.last_event_refusal.as_ref())
    }),
    Column("webhook secret rotation", |shown| text(&shown.rotation)),
]);

pub struct RotationShown {
    pub integration: wire::Integration,
    pub rotation: String,
}

pub const ROTATION: Columns<RotationShown> = Columns::Detail(&[
    Column("integration", |shown| text(&shown.integration.name)),
    Column("revision", |shown| text(shown.integration.revision)),
    Column("webhook secret rotation", |shown| text(&shown.rotation)),
    Column("attempted at", |shown| {
        or_absent(
            shown
                .integration
                .webhook_secret_rotation
                .as_ref()
                .and_then(|rotation| rotation.attempted_at.as_ref()),
        )
    }),
]);
pub const EVENTS: Columns<wire::EventRecord> = Columns::Rows(&[
    Column("record", |event| text(event.record)),
    Column("time", |event| text(&event.event.time)),
    Column("source", |event| text(&event.event.source)),
    Column("type", |event| text(&event.event.r#type)),
    Column("subject", |event| or_absent(event.event.subject.as_ref())),
]);
pub const EVENT: Columns<wire::EventRecord> = Columns::Detail(&[
    Column("record", |event| text(event.record)),
    Column("organization", |event| text(event.organization)),
    Column("integration", |event| or_absent(event.integration.as_ref())),
    Column("recorded at", |event| text(&event.recorded_at)),
    Column("id", |event| text(&event.event.id)),
    Column("specversion", |event| text(&event.event.specversion)),
    Column("source", |event| text(&event.event.source)),
    Column("type", |event| text(&event.event.r#type)),
    Column("subject", |event| or_absent(event.event.subject.as_ref())),
    Column("time", |event| text(&event.event.time)),
    Column("data", |event| compound(&event.event.data)),
]);
pub const TRIGGERS: Columns<wire::Trigger> = Columns::Rows(&[
    Column("id", |trigger| text(trigger.id)),
    Column("name", |trigger| text(&trigger.name)),
    Column("state", |trigger| text(&trigger.state)),
    Column("project", |trigger| text(&trigger.project)),
    Column("agent", |trigger| text(&trigger.agent)),
    Column("every", |trigger| or_absent(trigger.every.as_ref())),
    Column("cron", |trigger| or_absent(trigger.cron.as_ref())),
    Column("filter", |trigger| compound(&trigger.filter)),
]);
pub const TRIGGER: Columns<wire::Trigger> = Columns::Detail(&[
    Column("id", |trigger| text(trigger.id)),
    Column("organization", |trigger| text(&trigger.organization)),
    Column("name", |trigger| text(&trigger.name)),
    Column("state", |trigger| text(&trigger.state)),
    Column("disabled because", |trigger| {
        or_absent(trigger.disabled_because.as_ref())
    }),
    Column("firing budget", |trigger| compound(&trigger.firing_budget)),
    Column("every", |trigger| or_absent(trigger.every.as_ref())),
    Column("cron", |trigger| or_absent(trigger.cron.as_ref())),
    Column("zone", |trigger| or_absent(trigger.zone.as_ref())),
    Column("filter", |trigger| compound(&trigger.filter)),
    Column("project", |trigger| text(&trigger.project)),
    Column("agent", |trigger| text(&trigger.agent)),
    Column("allows", |trigger| joined(&trigger.allows)),
    Column("profile", |trigger| or_absent(trigger.profile.as_ref())),
    Column("branch", |trigger| or_absent(trigger.branch.as_ref())),
    Column("correlation", |trigger| {
        or_absent(trigger.correlation.as_ref())
    }),
    Column("on miss", |trigger| or_absent(trigger.on_miss.as_ref())),
    Column("on open workspace", |trigger| {
        text(&trigger.on_open_workspace)
    }),
    Column("applied", |trigger| text(trigger.applied)),
    Column("declared at", |trigger| text(&trigger.declared_at)),
    Column("brief", |trigger| text(&trigger.brief)),
]);
pub const TRIGGER_DECLARED: Columns<wire::Trigger> =
    Columns::Value(Column("id", |trigger| text(trigger.id)));
pub const TRIGGER_STATE: Columns<wire::Trigger> =
    Columns::Value(Column("state", |trigger| text(&trigger.state)));
pub const TRIGGER_TEST: Columns<wire::TriggerTestResult> = Columns::Detail(&[
    Column("matches", |tested| text(tested.matches)),
    Column("would", |tested| text(&tested.would)),
    Column("elapsing", |tested| or_absent(tested.elapsing.as_ref())),
    Column("agent", |tested| text(&tested.agent)),
    Column("branch", |tested| or_absent(tested.branch.as_ref())),
    Column("correlation", |tested| {
        or_absent(tested.correlation.as_ref())
    }),
    Column("brief", |tested| text(&tested.brief)),
]);
/// A Fired as a person reads it: the variant is chosen once, and every column reads the one
/// record it answered.
pub struct FiredShown {
    outcome: serde_json::Value,
    workspace: Option<String>,
    session: Option<String>,
    event: String,
    correlation: Option<String>,
}

impl FiredShown {
    pub fn of(fired: &wire::Fired) -> Self {
        match fired {
            wire::Fired::FiredOpened(opened) => Self {
                outcome: opened.outcome.clone(),
                workspace: Some(opened.workspace.to_string()),
                session: Some(opened.session.to_string()),
                event: opened.event.to_string(),
                correlation: None,
            },
            wire::Fired::FiredFed(fed) => Self {
                outcome: fed.outcome.clone(),
                workspace: Some(fed.workspace.to_string()),
                session: fed.session.map(|session| session.to_string()),
                event: fed.event.to_string(),
                correlation: None,
            },
            wire::Fired::FiredIgnored(ignored) => Self {
                outcome: ignored.outcome.clone(),
                workspace: None,
                session: None,
                event: ignored.event.to_string(),
                correlation: Some(ignored.correlation.clone()),
            },
        }
    }
}

pub const FIRED: Columns<FiredShown> = Columns::Detail(&[
    Column("outcome", |shown| compound(&shown.outcome)),
    Column("workspace", |shown| or_absent(shown.workspace.as_ref())),
    Column("session", |shown| or_absent(shown.session.as_ref())),
    Column("event", |shown| text(&shown.event)),
    Column("correlation", |shown| or_absent(shown.correlation.as_ref())),
]);
pub const WORKSPACES: Columns<wire::WorkspaceListed> = Columns::Rows(&[
    Column("id", |record| text(record.id)),
    Column("name", |record| text(&record.name)),
    Column("state", |record| text(&record.state)),
    Column("project", |record| text(&record.project)),
    Column("opened with", |record| text(&record.opened_with)),
    Column("started by", |record| or_absent(record.started_by.as_ref())),
]);
pub const ENTRIES: Columns<wire::Recorded> = Columns::Rows(&[
    Column("seq", |record| text(record.seq)),
    Column("appended at", |record| text(&record.appended_at)),
    Column("entry", |record| compound(&record.entry)),
]);
pub const SESSION_STATE: Columns<wire::TranscriptSessionState> =
    Columns::Value(Column("session state", session_state_line));
pub const PRESENCE: Columns<wire::Presence> = Columns::Value(Column("presence", presence_line));
pub const WORKSPACE: Columns<wire::Workspace> = Columns::Detail(&[
    Column("id", |record| text(record.id)),
    Column("name", |record| text(&record.name)),
    Column("organization", |record| text(&record.organization)),
    Column("project", |record| text(&record.project)),
    Column("opened with", |record| text(&record.opened_with)),
    Column("profile", |record| or_absent(record.profile.as_ref())),
    Column("base", |record| text(&record.checkout.base)),
    Column("branch", |record| text(&record.checkout.branch)),
    Column("instance", |record| or_absent(record.instance.as_ref())),
    Column("held", |record| or_absent(record.held.as_ref())),
    Column("correlation", |record| {
        or_absent(record.correlation.as_ref())
    }),
    Column("state", |record| text(&record.state)),
    Column("opened at", |record| text(&record.opened_at)),
    Column("last active at", |record| text(&record.last_active_at)),
    Column("sealed at", |record| or_absent(record.sealed_at.as_ref())),
    Column("started by", |record| or_absent(record.started_by.as_ref())),
    Column("continues", |record| or_absent(record.continues.as_ref())),
    Column("continued by", |record| joined(&record.continued_by)),
    Column("held messages", |record| compound(&record.held_messages)),
    Column("pull requests", |record| compound(&record.pull_requests)),
    Column("unfinished session state", |record| {
        or_absent(
            record
                .unfinished_session
                .as_ref()
                .map(|session| &session.state),
        )
    }),
    Column("preparing", |record| {
        or_absent(
            record
                .unfinished_session
                .as_ref()
                .and_then(|session| session.preparing.as_ref()),
        )
    }),
]);
pub const HELD_MESSAGE: Columns<wire::HeldMessage> = Columns::Detail(&[
    Column("id", |record| text(record.id)),
    Column("participant", |record| text(&record.participant)),
    Column("message", |record| text(&record.message)),
    Column("posted at", |record| text(&record.posted_at)),
    Column("edited at", |record| or_absent(record.edited_at.as_ref())),
]);
pub const SESSIONS: Columns<wire::Session> = Columns::Rows(&[
    Column("id", |record| text(record.id)),
    Column("name", |record| text(&record.name)),
    Column("state", |record| text(&record.state)),
    Column("preparing", |record| or_absent(record.preparing.as_ref())),
    Column("agent", |record| text(&record.agent)),
    Column("status", |record| {
        or_absent(record.exit.as_ref().map(|exit| &exit.status))
    }),
    Column("because", |record| {
        or_absent(record.exit.as_ref().and_then(|exit| exit.because.as_ref()))
    }),
    Column("instance", |record| or_absent(record.instance.as_ref())),
    Column("worked model", |record| {
        or_absent(record.worked_model.as_ref())
    }),
]);
/// A Session as a person reads it: the record the boundary answered, with its diagnostic as the
/// invocation at hand words it, and its nested records as prose where a person is reading.
pub struct SessionShown {
    pub session: wire::Session,
    pub diagnostic: String,
    pub prose: bool,
}

pub const SESSION: Columns<SessionShown> =
    Columns::Detail(&[
        Column("id", |shown| text(shown.session.id)),
        Column("name", |shown| text(&shown.session.name)),
        Column("workspace", |shown| text(shown.session.workspace)),
        Column("state", |shown| text(&shown.session.state)),
        Column("preparing", |shown| {
            or_absent(shown.session.preparing.as_ref())
        }),
        Column("status", |shown| {
            or_absent(shown.session.exit.as_ref().map(|exit| &exit.status))
        }),
        Column("because", |shown| {
            or_absent(
                shown
                    .session
                    .exit
                    .as_ref()
                    .and_then(|exit| exit.because.as_ref()),
            )
        }),
        Column("diagnostic", |shown| text(&shown.diagnostic)),
        Column("outcome message", |shown| {
            or_absent(shown.session.outcome_message.as_ref())
        }),
        Column("instance", |shown| {
            or_absent(shown.session.instance.as_ref())
        }),
        Column("supervisor", |shown| {
            or_absent(shown.session.supervisor.as_ref())
        }),
        Column("agent", |shown| text(&shown.session.agent)),
        Column("harness", |shown| text(&shown.session.harness)),
        Column("model", |shown| or_absent(shown.session.model.as_ref())),
        Column("worked model", |shown| {
            or_absent(shown.session.worked_model.as_ref())
        }),
        Column("title", |shown| or_absent(shown.session.title.as_ref())),
        Column("options", |shown| option_summary(&shown.session.options)),
        Column("changing options", |shown| {
            shown
                .session
                .changing_options
                .iter()
                .map(|change| {
                    format!(
                        "{}: {} ({})",
                        change.option, change.value, change.participant
                    )
                })
                .collect::<Vec<_>>()
                .join("\n")
        }),
        Column("commands", |shown| {
            shown
                .session
                .commands
                .iter()
                .map(|command| command.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        }),
        Column("enqueued at", |shown| text(&shown.session.enqueued_at)),
        Column("started at", |shown| {
            or_absent(shown.session.started_at.as_ref())
        }),
        Column("ended at", |shown| {
            or_absent(shown.session.ended_at.as_ref())
        }),
        Column("lease expires at", |shown| {
            or_absent(shown.session.lease_expires_at.as_ref())
        }),
        Column("connected at", |shown| {
            or_absent(shown.session.connected_at.as_ref())
        }),
        Column("supervisor version", |shown| {
            or_absent(shown.session.supervisor_version.as_ref())
        }),
        Column("usage", |shown| compound(&shown.session.usage)),
        Column("depends on", |shown| {
            if shown.prose {
                joined(
                    &shown
                        .session
                        .depends_on
                        .iter()
                        .map(|dependency| &dependency.name)
                        .collect::<Vec<_>>(),
                )
            } else {
                compound(&shown.session.depends_on)
            }
        }),
        Column("tools", |shown| {
            if shown.prose {
                lines(
                    shown.session.tools.iter().map(|tool| {
                        format!("{}\n{}  {}", tool.title, tool.status, tool.started_at)
                    }),
                )
            } else {
                compound(&shown.session.tools)
            }
        }),
        Column("units", |shown| {
            if shown.prose {
                lines(
                    shown
                        .session
                        .units
                        .iter()
                        .map(|unit| format!("{}\n{}  {}", unit.title, unit.kind, unit.started_at)),
                )
            } else {
                compound(&shown.session.units)
            }
        }),
        Column("last activity at", |shown| {
            or_absent(shown.session.last_activity_at.as_ref())
        }),
    ]);
pub const INTERRUPTED: Columns<wire::Session> = Columns::Detail(&[
    Column("id", |record| text(record.id)),
    Column("state", |record| text(&record.state)),
    Column("participant", |record| {
        or_absent(record.interrupting.as_ref().map(|value| &value.participant))
    }),
    Column("requested at", |record| {
        or_absent(
            record
                .interrupting
                .as_ref()
                .map(|value| &value.requested_at),
        )
    }),
]);
pub const STOPPED: Columns<wire::Session> = Columns::Value(Column("status", |record| {
    or_absent(record.exit.as_ref().map(|exit| &exit.status))
}));
pub const QUEUE: Columns<crate::queue::Row> = Columns::Rows(&[
    Column("position", |row| or_absent(row.position.as_ref())),
    Column("name", |row| text(&row.name)),
    Column("state", |row| text(&row.state)),
    Column("agent", |row| text(&row.agent)),
    Column("why", |row| text(&row.why)),
    Column("workspace", |row| text(&row.workspace)),
    Column("enqueued at", |row| text(&row.enqueued_at)),
]);

pub const INSTANCES: Columns<wire::HeldInstance> = Columns::Rows(&[
    Column("workspace", |record| text(record.workspace)),
    Column("instance", |record| text(&record.instance)),
    Column("because", |record| text(&record.because)),
]);
pub const RELEASED: Columns<wire::ReleasedInstance> =
    Columns::Value(Column("instance", |record| text(&record.instance)));
pub const STATUS: Columns<crate::Status> = Columns::Detail(&[
    Column("Client", |record| text(&record.client_url)),
    Column("readiness", |record| text(&record.readiness.outcome)),
    Column("Workspaces by state", |record| {
        counts(&record.workspace_counts)
    }),
    Column("Sessions by state", |record| counts(&record.session_counts)),
    Column("control plane", |record| {
        text(&record.location.control_plane)
    }),
    Column("control plane source", |record| {
        text(&record.location.control_plane_source)
    }),
    Column("organization", |record| {
        or_absent(record.organization.as_ref())
    }),
    Column("organization source", |record| {
        or_absent(record.organization_source.as_ref())
    }),
    Column("projects", |record| text(record.projects)),
    Column("agents", |record| text(record.agents)),
    Column("triggers", |record| text(record.triggers)),
    Column("workspaces", |record| text(record.workspaces)),
    Column("integrations", |record| text(record.integrations)),
    Column("credentials", |record| text(record.credentials)),
    Column("profiles", |record| text(record.profiles)),
]);
pub const EXIT_CODES: Columns<crate::exit::Record> = Columns::Detail(&[
    Column("code", |record| text(record.code)),
    Column("name", |record| text(record.name)),
    Column("meaning", |record| text(record.meaning)),
    Column("branch", |record| text(record.branch)),
]);

pub const STARTED: Columns<wire::Started> = Columns::Detail(&[
    Column("organization", |record| text(&record.organization.name)),
    Column("project", |record| text(&record.project.name)),
    Column("agent", |record| text(&record.agent.name)),
    Column("workspace", |record| text(&record.workspace.name)),
    Column("session", |record| text(&record.session.name)),
]);

pub const OPENED: Columns<wire::Opened> = Columns::Detail(&[
    Column("workspace", |record| text(&record.workspace.name)),
    Column("workspace id", |record| text(record.workspace.id)),
    Column("session", |record| text(&record.session.name)),
    Column("session id", |record| text(record.session.id)),
]);

fn lines(said: impl Iterator<Item = String>) -> String {
    said.collect::<Vec<_>>().join("\n")
}

fn session_state_line(state: &wire::TranscriptSessionState) -> String {
    let observation = match &state.observation {
        wire::SessionObservation::SessionObservationCurrent(_) => "current",
        wire::SessionObservation::SessionObservationUnavailable(_) => "unavailable",
    };
    let mut said = vec![
        "session state".to_owned(),
        or_absent(state.session_id.as_ref()),
        observation.to_owned(),
        format!("{} tools, {} units", state.tools.len(), state.units.len()),
    ];
    if let Some(usage) = &state.usage {
        said.push(format!(
            "{} of {} context used",
            usage.context_used, usage.context_size
        ));
    }
    if let Some(last) = &state.last_activity_at {
        said.push(format!("last active {last}"));
    }

    said.join(crate::output::GAP)
}

fn presence_line(presence: &wire::Presence) -> String {
    let named = if presence.named.is_empty() {
        crate::output::ABSENT.to_owned()
    } else {
        presence.named.join(", ")
    };

    format!(
        "presence{}{named}{}{} anonymous",
        crate::output::GAP,
        crate::output::GAP,
        presence.anonymous
    )
}

pub const WORKSPACE_DECLARED: Columns<wire::Workspace> =
    Columns::Value(Column("id", |record| text(record.id)));
pub const SESSION_DECLARED: Columns<wire::Session> =
    Columns::Value(Column("id", |record| text(record.id)));
pub const HELD_DECLARED: Columns<wire::HeldMessage> =
    Columns::Value(Column("id", |record| text(record.id)));
pub const POSTED: Columns<wire::Posted> = Columns::Value(Column("id", |record| {
    record
        .session
        .as_ref()
        .map(|session| text(session.id))
        .or_else(|| record.held_message.as_ref().map(|held| text(held.id)))
        .unwrap_or_else(|| crate::output::ABSENT.to_owned())
}));

fn option_summary(options: &[wire::SessionOption]) -> String {
    options
        .iter()
        .map(|option| {
            let current = match &option.current {
                wire::SessionOptionCurrent::SelectedOptionValue(value) => value.clone(),
                wire::SessionOptionCurrent::ToggleOptionValue(value) => value.to_string(),
            };
            format!(
                "{}: {current}",
                option.category.as_deref().unwrap_or(&option.id)
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn counts(counts: &std::collections::BTreeMap<String, usize>) -> String {
    counts
        .iter()
        .map(|(state, count)| format!("{state}: {count}"))
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    const EVENT: &str = "00000000-0000-0000-0000-000000000009";
    const WORKSPACE: &str = "00000000-0000-0000-0000-000000000002";
    const SESSION: &str = "00000000-0000-0000-0000-000000000001";

    fn fired(value: serde_json::Value) -> wire::Fired {
        serde_json::from_value(value).expect("a firing")
    }

    /// The line a pipe reads, joined the way `typed::record` joins a delimited record.
    fn said(fired: &wire::Fired) -> String {
        let Columns::Detail(columns) = FIRED else {
            panic!("a firing is a detail");
        };
        let shown = FiredShown::of(fired);

        columns
            .iter()
            .map(|column| (column.1)(&shown))
            .collect::<Vec<_>>()
            .join("\t")
            + "\n"
    }

    #[test]
    fn a_firing_reads_as_the_cells_its_variant_answers() {
        let opened = fired(json!({
            "outcome": "opened", "event": EVENT, "workspace": WORKSPACE, "session": SESSION,
        }));
        assert_eq!(
            said(&opened),
            format!("opened\t{WORKSPACE}\t{SESSION}\t{EVENT}\t-\n")
        );

        let fed = fired(json!({
            "outcome": "fed", "event": EVENT, "workspace": WORKSPACE, "session": null,
        }));
        assert_eq!(said(&fed), format!("fed\t{WORKSPACE}\t-\t{EVENT}\t-\n"));

        let ignored = fired(json!({
            "outcome": "ignored", "event": EVENT, "correlation": "issue-1",
        }));
        assert_eq!(said(&ignored), format!("ignored\t-\t-\t{EVENT}\tissue-1\n"));
    }
}
