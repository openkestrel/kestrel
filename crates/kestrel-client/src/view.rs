//! What each command shows, and the three shapes it can be shown in.

use kestrel_operator_types as wire;

use crate::typed::{Column, Columns, joined, or_absent, text};

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
    Column("control plane", |record| {
        text(&record.location.control_plane)
    }),
    Column("control plane source", |record| {
        text(&record.location.control_plane_source)
    }),
    Column("organization", |record| text(&record.organization)),
    Column("organization source", |record| {
        text(&record.organization_source)
    }),
    Column("projects", |record| text(record.projects)),
    Column("agents", |record| text(record.agents)),
    Column("triggers", |record| text(record.triggers)),
    Column("workspaces", |record| text(record.workspaces)),
    Column("integrations", |record| text(record.integrations)),
    Column("credentials", |record| text(record.credentials)),
    Column("profiles", |record| text(record.profiles)),
    Column("next", |record| text(&record.next)),
]);
pub const UNRESOLVED: Columns<crate::UnresolvedStatus> = Columns::Detail(&[
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
    Column("organizations", |record| joined(&record.organizations)),
    Column("next", |record| text(&record.next)),
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
