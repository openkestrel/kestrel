//! What each command shows, and the three shapes it can be shown in.

use kestrel_operator_types as wire;

use crate::typed::{Column, Columns, compound, joined, or_absent, text};

/// What a command produces, as dotted paths into the record the control plane answered.
pub enum View {
    /// The one field a `$(…)` captures.
    Value(&'static str),
    Rows(&'static [&'static str]),
    Detail(&'static [&'static str]),
}

impl View {
    pub fn fields(&self) -> &[&'static str] {
        match self {
            View::Value(field) => std::slice::from_ref(field),
            View::Rows(fields) | View::Detail(fields) => fields,
        }
    }
}

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
pub const CREDENTIAL: View = View::Value("variable");
pub const CREDENTIALS: View = View::Rows(&["variable", "set_at"]);
pub const PROFILES: View = View::Rows(&["id", "name", "owner", "holds"]);
pub const LOGIN: View = View::Value("name");
pub const INTEGRATIONS: View = View::Rows(&[
    "id",
    "name",
    "kind",
    "repository",
    "carries",
    "state",
    "revision",
    "polled_every",
    "webhook_path",
    "last_event_refusal.reason",
]);
pub const INTEGRATION: View = View::Detail(&[
    "id",
    "name",
    "kind",
    "repository",
    "bot_login",
    "carries",
    "state",
    "revision",
    "disabled_at",
    "retired_at",
    "canceled_posts",
    "diagnostic",
    "polled_every",
    "webhook_path",
    "last_event_refusal",
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
pub const FIRED: Columns<wire::Fired> = Columns::Detail(&[
    Column("outcome", |fired| match fired {
        wire::Fired::FiredOpened(opened) => compound(&opened.outcome),
        wire::Fired::FiredFed(fed) => compound(&fed.outcome),
        wire::Fired::FiredIgnored(ignored) => compound(&ignored.outcome),
    }),
    Column("workspace", |fired| match fired {
        wire::Fired::FiredOpened(opened) => text(opened.workspace),
        wire::Fired::FiredFed(fed) => text(fed.workspace),
        wire::Fired::FiredIgnored(_) => or_absent(None::<&String>),
    }),
    Column("session", |fired| match fired {
        wire::Fired::FiredOpened(opened) => text(opened.session),
        wire::Fired::FiredFed(fed) => or_absent(fed.session.as_ref()),
        wire::Fired::FiredIgnored(_) => or_absent(None::<&String>),
    }),
    Column("event", |fired| match fired {
        wire::Fired::FiredOpened(opened) => text(opened.event),
        wire::Fired::FiredFed(fed) => text(fed.event),
        wire::Fired::FiredIgnored(ignored) => text(ignored.event),
    }),
    Column("correlation", |fired| match fired {
        wire::Fired::FiredIgnored(ignored) => text(&ignored.correlation),
        wire::Fired::FiredOpened(_) | wire::Fired::FiredFed(_) => or_absent(None::<&String>),
    }),
]);
pub const WORKSPACES: Columns<wire::WorkspaceListed> = Columns::Rows(&[
    Column("id", |record| text(record.id)),
    Column("name", |record| text(&record.name)),
    Column("state", |record| text(&record.state)),
    Column("project", |record| text(&record.project)),
    Column("opened with", |record| text(&record.opened_with)),
    Column("started by", |record| or_absent(record.started_by.as_ref())),
]);
pub const ENTRIES: View = View::Rows(&["seq", "appended_at", "entry"]);
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
    Column("held messages", |record| structured(&record.held_messages)),
    Column("pull requests", |record| structured(&record.pull_requests)),
    Column("state", |record| {
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
pub const SESSION: Columns<wire::Session> = Columns::Detail(&[
    Column("id", |record| text(record.id)),
    Column("name", |record| text(&record.name)),
    Column("workspace", |record| text(record.workspace)),
    Column("state", |record| text(&record.state)),
    Column("preparing", |record| or_absent(record.preparing.as_ref())),
    Column("status", |record| {
        or_absent(record.exit.as_ref().map(|exit| &exit.status))
    }),
    Column("because", |record| {
        or_absent(record.exit.as_ref().and_then(|exit| exit.because.as_ref()))
    }),
    Column("diagnostic", |record| structured(&record.diagnostic)),
    Column("outcome message", |record| {
        or_absent(record.outcome_message.as_ref())
    }),
    Column("instance", |record| or_absent(record.instance.as_ref())),
    Column("supervisor", |record| or_absent(record.supervisor.as_ref())),
    Column("agent", |record| text(&record.agent)),
    Column("harness", |record| text(&record.harness)),
    Column("model", |record| or_absent(record.model.as_ref())),
    Column("worked model", |record| {
        or_absent(record.worked_model.as_ref())
    }),
    Column("title", |record| or_absent(record.title.as_ref())),
    Column("options", |record| option_summary(&record.options)),
    Column("changing options", |record| {
        record
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
    Column("commands", |record| {
        record
            .commands
            .iter()
            .map(|command| command.name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    }),
    Column("enqueued at", |record| text(&record.enqueued_at)),
    Column("started at", |record| or_absent(record.started_at.as_ref())),
    Column("ended at", |record| or_absent(record.ended_at.as_ref())),
    Column("lease expires at", |record| {
        or_absent(record.lease_expires_at.as_ref())
    }),
    Column("connected at", |record| {
        or_absent(record.connected_at.as_ref())
    }),
    Column("supervisor version", |record| {
        or_absent(record.supervisor_version.as_ref())
    }),
    Column("usage", |record| structured(&record.usage)),
    Column("depends on", |record| structured(&record.depends_on)),
    Column("tools", |record| structured(&record.tools)),
    Column("units", |record| structured(&record.units)),
    Column("last activity at", |record| {
        or_absent(record.last_activity_at.as_ref())
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
pub const INSTANCES: View = View::Rows(&["workspace", "instance", "because"]);
pub const QUEUE: View = View::Rows(&[
    "position",
    "name",
    "state",
    "agent",
    "why",
    "workspace",
    "enqueued_at",
]);
pub const RELEASED: View = View::Value("instance");
pub const STATUS: View = View::Detail(&[
    "control_plane",
    "control_plane_source",
    "organization",
    "organization_source",
    "projects",
    "agents",
    "triggers",
    "workspaces",
    "integrations",
    "credentials",
    "profiles",
    "next",
]);
pub const UNRESOLVED: View = View::Detail(&[
    "control_plane",
    "control_plane_source",
    "organization",
    "organization_source",
    "organizations",
    "next",
]);
pub const EXIT_CODES: View = View::Detail(&["code", "name", "meaning", "branch"]);

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

/// What a creation answers back is the identifier the next command is given.
pub const DECLARED: View = View::Value("id");

fn structured(value: &impl serde::Serialize) -> String {
    crate::output::rendered(Some(
        &serde_json::to_value(value).expect("a wire field serializes"),
    ))
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
        .unwrap_or_else(|| "-".to_owned())
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
