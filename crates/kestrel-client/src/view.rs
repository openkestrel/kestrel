//! What each command shows, and the three shapes it can be shown in.

use kestrel_operator_types as wire;

use crate::typed::{Column, Columns, joined, object, objects, or_absent, text};

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
pub const GITHUB_APP_STARTED: Columns<wire::GithubAppStarted> =
    Columns::Value(Column("url", |started| text(&started.url)));
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
