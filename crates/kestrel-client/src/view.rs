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
pub const WORKSPACES: View = View::Rows(&[
    "id",
    "name",
    "state",
    "project",
    "opened_with",
    "started_by",
]);
pub const ENTRIES: View = View::Rows(&["seq", "appended_at", "entry"]);
pub const WORKSPACE: View = View::Detail(&[
    "id",
    "name",
    "organization",
    "project",
    "opened_with",
    "profile",
    "checkout.base",
    "checkout.branch",
    "instance",
    "held",
    "correlation",
    "state",
    "opened_at",
    "last_active_at",
    "sealed_at",
    "started_by",
    "continues",
    "continued_by",
    "held_messages",
    "pull_requests",
    "unfinished_session.state",
    "unfinished_session.preparing",
]);
pub const HELD_MESSAGE: View =
    View::Detail(&["id", "participant", "message", "posted_at", "edited_at"]);
pub const SESSIONS: View = View::Rows(&[
    "id",
    "name",
    "state",
    "preparing",
    "agent",
    "exit.status",
    "exit.because",
    "instance",
    "worked_model",
]);
pub const SESSION: View = View::Detail(&[
    "id",
    "name",
    "workspace",
    "state",
    "preparing",
    "exit.status",
    "exit.because",
    "diagnostic",
    "outcome_message",
    "instance",
    "supervisor",
    "agent",
    "harness",
    "model",
    "worked_model",
    "title",
    "options",
    "changing_options",
    "commands",
    "enqueued_at",
    "started_at",
    "ended_at",
    "lease_expires_at",
    "connected_at",
    "supervisor_version",
    "usage",
    "depends_on",
    "tools",
    "units",
    "last_activity_at",
]);
pub const INTERRUPTED: View = View::Detail(&[
    "id",
    "state",
    "interrupting.participant",
    "interrupting.requested_at",
]);
pub const STOPPED: View = View::Value("exit.status");
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

pub const STARTED: View =
    View::Detail(&["organization", "project", "agent", "workspace", "session"]);

pub const OPENED: View = View::Detail(&["workspace", "workspace_id", "session", "session_id"]);

/// What a creation answers back is the identifier the next command is given.
pub const DECLARED: View = View::Value("id");
