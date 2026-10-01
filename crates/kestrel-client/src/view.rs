//! What each command shows, and the three shapes it can be shown in.

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

pub const ORGANIZATIONS: View = View::Rows(&["id", "name", "max_live_instances"]);
pub const PROJECTS: View = View::Rows(&["id", "name", "branch", "repositories"]);
pub const AGENTS: View = View::Rows(&["id", "name", "harness", "model"]);
pub const AGENT_MODEL: View = View::Value("model");
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
    "polled_every",
    "webhook_path",
    "last_event_refusal.reason",
]);
pub const EVENTS: View = View::Rows(&[
    "record",
    "event.time",
    "event.source",
    "event.type",
    "event.subject",
]);
pub const EVENT: View = View::Detail(&[
    "record",
    "organization",
    "integration",
    "recorded_at",
    "event.id",
    "event.specversion",
    "event.source",
    "event.type",
    "event.subject",
    "event.time",
    "event.data",
]);
pub const TRIGGERS: View = View::Rows(&[
    "id", "name", "state", "project", "agent", "every", "cron", "filter",
]);
pub const TRIGGER: View = View::Detail(&[
    "id",
    "organization",
    "name",
    "state",
    "disabled_because",
    "firing_budget",
    "every",
    "cron",
    "zone",
    "filter",
    "project",
    "agent",
    "allows",
    "profile",
    "branch",
    "correlation",
    "on_miss",
    "on_open_workspace",
    "applied",
    "declared_at",
    "brief",
]);
pub const TRIGGER_TEST: View = View::Detail(&[
    "matches",
    "would",
    "elapsing",
    "agent",
    "branch",
    "correlation",
    "brief",
]);
pub const TRIGGER_STATE: View = View::Value("state");
pub const FIRED: View = View::Detail(&["outcome", "workspace", "session", "event", "correlation"]);
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
    "pull_requests",
]);
pub const SESSIONS: View = View::Rows(&[
    "id",
    "name",
    "state",
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
    "exit.status",
    "exit.because",
    "outcome_message",
    "instance",
    "supervisor",
    "agent",
    "harness",
    "model",
    "worked_model",
    "title",
    "options",
    "commands",
    "enqueued_at",
    "started_at",
    "ended_at",
    "lease_expires_at",
    "connected_at",
    "supervisor_version",
    "usage",
    "tools",
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

/// What a creation answers back is the identifier the next command is given.
pub const DECLARED: View = View::Value("id");
