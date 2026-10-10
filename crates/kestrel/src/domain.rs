use std::fmt;
use std::num::NonZeroUsize;
use std::str::FromStr;

use anyhow::{Context as _, Result, bail};
use jiff::{SignedDuration, Timestamp};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::cron::Cron;
use crate::filter::{Attribute, Filter};
use crate::integration::credential::App;
use crate::template::Template;

macro_rules! identifiers {
    ($($name:ident),+ $(,)?) => {$(
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        pub struct $name(Uuid);

        impl $name {
            pub fn generate() -> Self {
                Self(Uuid::now_v7())
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(f)
            }
        }

        impl FromStr for $name {
            type Err = uuid::Error;

            fn from_str(text: &str) -> Result<Self, Self::Err> {
                Ok(Self(text.parse()?))
            }
        }
    )+};
}

identifiers!(
    OperatorId,
    OrganizationId,
    ProjectId,
    AgentId,
    WorkspaceId,
    SessionId,
    IntegrationId,
    EventRecordId,
    TriggerId,
    SubscriptionProfileId,
);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Operator {
    pub id: OperatorId,
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct Organization {
    pub id: OrganizationId,
    pub name: String,
    pub max_live_instances: Option<NonZeroUsize>,
}

#[derive(Debug, Clone)]
pub struct Project {
    pub id: ProjectId,
    pub organization: OrganizationId,
    pub name: String,
    pub repositories: Vec<String>,
    pub branch: String,
}

/// Each a Harness value id; a category named none for is the Harness's own default (ADR-0041).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Declared {
    pub model: Option<String>,
    pub mode: Option<String>,
    pub thought_level: Option<String>,
}

impl Declared {
    pub fn over(self, below: Self) -> Self {
        Self {
            model: self.model.or(below.model),
            mode: self.mode.or(below.mode),
            thought_level: self.thought_level.or(below.thought_level),
        }
    }

    /// A value named as nothing is a value nobody named.
    pub fn named(values: Self) -> Self {
        fn named(value: Option<String>) -> Option<String> {
            value.filter(|value| !value.is_empty())
        }

        Self {
            model: named(values.model),
            mode: named(values.mode),
            thought_level: named(values.thought_level),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.model.is_none() && self.mode.is_none() && self.thought_level.is_none()
    }
}

#[derive(Debug, Clone)]
pub struct Agent {
    pub id: AgentId,
    pub organization: OrganizationId,
    pub name: String,
    pub harness: String,
    pub declared: Declared,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubscriptionProfile {
    pub id: SubscriptionProfileId,
    pub organization: OrganizationId,
    pub name: String,
    pub owner: String,
    pub owner_operator: Option<OperatorId>,
}

/// Which way an Integration carries: events inbound, kestrel's requests outbound, or both.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    Inbound,
    Outbound,
}

impl Direction {
    pub const fn as_str(self) -> &'static str {
        match self {
            Direction::Inbound => "inbound",
            Direction::Outbound => "outbound",
        }
    }
}

impl FromStr for Direction {
    type Err = anyhow::Error;

    fn from_str(direction: &str) -> Result<Self> {
        match direction {
            "inbound" => Ok(Direction::Inbound),
            "outbound" => Ok(Direction::Outbound),
            other => bail!("{other} is not a direction an integration carries"),
        }
    }
}

impl fmt::Display for Direction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntegrationKind {
    Github,
    Webhook,
}

impl IntegrationKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            IntegrationKind::Github => "github",
            IntegrationKind::Webhook => "webhook",
        }
    }
}

impl FromStr for IntegrationKind {
    type Err = anyhow::Error;

    fn from_str(kind: &str) -> Result<Self> {
        match kind {
            "github" => Ok(IntegrationKind::Github),
            "webhook" => Ok(IntegrationKind::Webhook),
            other => bail!("{other} is not an external system kestrel integrates with"),
        }
    }
}

impl fmt::Display for IntegrationKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntegrationState {
    Enabled,
    Disabled,
}

impl IntegrationState {
    pub const fn as_str(self) -> &'static str {
        match self {
            IntegrationState::Enabled => "enabled",
            IntegrationState::Disabled => "disabled",
        }
    }
}

impl FromStr for IntegrationState {
    type Err = anyhow::Error;

    fn from_str(state: &str) -> Result<Self> {
        match state {
            "enabled" => Ok(IntegrationState::Enabled),
            "disabled" => Ok(IntegrationState::Disabled),
            other => bail!("{other} is not a state an integration is in"),
        }
    }
}

impl fmt::Display for IntegrationState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone)]
pub struct Integration {
    pub id: IntegrationId,
    pub organization: OrganizationId,
    pub name: String,
    pub connection: Connection,
    pub carries: Vec<Direction>,
    pub state: IntegrationState,
    pub revision: i64,
    pub disabled_at: Option<Timestamp>,
    pub poll_due_at: Option<Timestamp>,
    /// By GitHub's clock: every Delivery made earlier has been read.
    pub deliveries_read_from: Option<Timestamp>,
    /// By kestrel's clock, when a poll last read every new Delivery.
    pub last_polled_at: Option<Timestamp>,
    pub last_event_refusal: Option<EventRefusal>,
}

impl Integration {
    pub fn carries(&self, direction: Direction) -> bool {
        self.carries.contains(&direction)
    }

    pub fn in_use(&self, direction: Direction) -> bool {
        !self.disabled() && self.carries(direction)
    }

    pub fn disabled(&self) -> bool {
        self.state == IntegrationState::Disabled
    }

    pub fn polled(&self) -> bool {
        self.kind() == IntegrationKind::Github && self.carries(Direction::Inbound)
    }

    pub const fn kind(&self) -> IntegrationKind {
        match self.connection {
            Connection::Github(_) => IntegrationKind::Github,
            Connection::Webhook => IntegrationKind::Webhook,
        }
    }

    pub fn github(&self) -> Result<&GithubConnection> {
        match &self.connection {
            Connection::Github(github) => Ok(github),
            Connection::Webhook => bail!("the integration {} is not a github one", self.name),
        }
    }

    pub fn webhook_path(&self) -> String {
        format!("/webhooks/{}", self.id)
    }
}

#[derive(Debug, Clone)]
pub enum Connection {
    Github(GithubConnection),
    Webhook,
}

#[derive(Debug, Clone)]
pub struct GithubConnection {
    pub repository: String,
    pub api: String,
    pub credential: App,
    /// The App's own bot account, `<slug>[bot]`, learned from GitHub when it was registered.
    pub bot_login: String,
    pub interval: SignedDuration,
    pub repository_id: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Occurrence {
    pub id: String,
    pub source: String,
    pub specversion: String,
    #[serde(rename = "type")]
    pub r#type: String,
    pub subject: Option<String>,
    pub time: Timestamp,
    pub data: serde_json::Value,
}

#[derive(Debug, Clone)]
pub struct Event {
    pub record_id: EventRecordId,
    pub organization: OrganizationId,
    /// None for an Event kestrel minted itself.
    pub integration: Option<IntegrationId>,
    pub occurrence: Occurrence,
    pub recorded_at: Timestamp,
}

#[derive(Debug, Clone)]
pub struct EventRefusal {
    pub source: String,
    /// Absent when GitHub's retention lost Deliveries a poll never read.
    pub id: Option<String>,
    pub bytes: Option<usize>,
    pub reason: String,
    pub observed_at: Timestamp,
}

/// Something kestrel says back on the surface that started the Workspace: a completed Turn's
/// response, or the Session's own final Outcome. Posted once however many attempts that takes.
/// `turn` is the Turn's seq, or `None` for the Session's own outcome.
#[derive(Debug, Clone)]
pub struct Post {
    pub session: SessionId,
    pub turn: Option<i64>,
    pub organization: OrganizationId,
    pub integration: IntegrationId,
    pub event: EventRecordId,
    pub subject: i64,
    pub body: String,
    /// Set before a request goes out and left set: a Post that has been attempted may
    /// already have a comment on the issue, and is read back rather than posted twice.
    pub attempted_at: Option<Timestamp>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TriggerState {
    Enabled,
    Disabled(DisableReason),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisableReason {
    Operator,
    FiringBudget,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CorrelationMiss {
    Open,
    Ignore,
}

impl CorrelationMiss {
    pub const fn as_str(self) -> &'static str {
        match self {
            CorrelationMiss::Open => "open",
            CorrelationMiss::Ignore => "ignore",
        }
    }
}

impl FromStr for CorrelationMiss {
    type Err = anyhow::Error;

    fn from_str(miss: &str) -> Result<Self> {
        match miss {
            "open" => Ok(CorrelationMiss::Open),
            "ignore" => Ok(CorrelationMiss::Ignore),
            other => bail!("{other} is not what a trigger does when correlation misses"),
        }
    }
}

impl fmt::Display for CorrelationMiss {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What a firing does to the open Workspace it correlates to, declared rather than inferred from
/// its Agent: a second CI failure for the same Agent still wants a fresh context (ADR-0031).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum OnOpenWorkspace {
    #[default]
    Continue,
    NewSession,
}

impl OnOpenWorkspace {
    pub const fn as_str(self) -> &'static str {
        match self {
            OnOpenWorkspace::Continue => "continue",
            OnOpenWorkspace::NewSession => "new-session",
        }
    }
}

impl FromStr for OnOpenWorkspace {
    type Err = anyhow::Error;

    fn from_str(on_open: &str) -> Result<Self> {
        match on_open {
            "continue" => Ok(OnOpenWorkspace::Continue),
            "new-session" => Ok(OnOpenWorkspace::NewSession),
            other => bail!("{other} is not what a trigger does to an open workspace"),
        }
    }
}

impl fmt::Display for OnOpenWorkspace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl TriggerState {
    pub const fn as_str(&self) -> &'static str {
        match self {
            TriggerState::Enabled => "enabled",
            TriggerState::Disabled(DisableReason::Operator) => "disabled:operator",
            TriggerState::Disabled(DisableReason::FiringBudget) => "disabled:firing-budget",
        }
    }
}

impl FromStr for TriggerState {
    type Err = anyhow::Error;

    fn from_str(state: &str) -> Result<Self> {
        match state {
            "enabled" => Ok(TriggerState::Enabled),
            "disabled:operator" => Ok(TriggerState::Disabled(DisableReason::Operator)),
            "disabled:firing-budget" => Ok(TriggerState::Disabled(DisableReason::FiringBudget)),
            other => bail!("{other} is not a state a trigger can be in"),
        }
    }
}

impl fmt::Display for TriggerState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TriggerState::Enabled => f.write_str("enabled"),
            TriggerState::Disabled(_) => f.write_str("disabled"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FiringBudget {
    pub limit: NonZeroUsize,
    pub window: SignedDuration,
}

impl Default for FiringBudget {
    fn default() -> Self {
        Self {
            limit: NonZeroUsize::new(10).expect("a firing budget has a positive limit"),
            window: SignedDuration::from_hours(1),
        }
    }
}

/// The rule, never an individual firing.
#[derive(Debug, Clone)]
pub struct Trigger {
    pub id: TriggerId,
    pub organization: Organization,
    pub name: String,
    pub fires: Fires,
    pub templates: Templates,
    pub project: Project,
    pub agent: Agent,
    /// Over the Agent's and under a Session's own (ADR-0041).
    pub declared: Declared,
    pub allows: Vec<Agent>,
    pub profile: Option<SubscriptionProfile>,
    pub state: TriggerState,
    pub disabled_because: Option<String>,
    pub firing_budget: FiringBudget,
    pub applied: bool,
    pub declared_at: Timestamp,
}

impl Trigger {
    /// A scheduled Trigger matches only the Events its own elapsing mints, which is what puts
    /// scheduled work on the one firing path.
    pub fn filter(&self) -> Filter {
        match &self.fires {
            Fires::On(filter) => filter.clone(),
            Fires::Scheduled(_) => Filter::All(vec![
                Filter::Exact(Attribute::Source, self.source()),
                Filter::Exact(Attribute::Type, ELAPSED.to_owned()),
            ]),
        }
    }

    /// The Event its schedule mints on elapsing when due, keyed by that due time so a sweep
    /// that runs twice records it once.
    pub fn elapsing(&self, due: Timestamp) -> Option<Occurrence> {
        let Fires::Scheduled(schedule) = &self.fires else {
            return None;
        };
        let data = match schedule {
            Schedule::Every(every) => serde_json::json!({
                "trigger": self.name,
                "every": format!("{every:#}"),
            }),
            Schedule::Cron(cron) => serde_json::json!({
                "trigger": self.name,
                "cron": cron.to_string(),
                "zone": cron.zone(),
            }),
        };

        Some(Occurrence {
            id: due.to_string(),
            source: self.source(),
            specversion: "1.0".to_owned(),
            r#type: ELAPSED.to_owned(),
            subject: None,
            time: due,
            data,
        })
    }

    fn source(&self) -> String {
        format!("urn:kestrel:trigger:{}", self.id)
    }

    pub fn firing_budget_exhausted_because(&self) -> String {
        format!(
            "the trigger {} exhausted its budget of {} firings in {}",
            self.name, self.firing_budget.limit, self.firing_budget.window
        )
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Firing {
    pub trigger: String,
    pub outcome: String,
    pub workspace: Option<WorkspaceId>,
    pub failure: Option<String>,
    pub worked_ahead: Option<String>,
}

/// What kestrel calls a schedule elapsing.
pub const ELAPSED: &str = "dev.kestrel.schedule.elapsed";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fires {
    On(Filter),
    Scheduled(Schedule),
}

impl fmt::Display for Fires {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Fires::On(filter) => filter.fmt(f),
            Fires::Scheduled(Schedule::Every(every)) => write!(f, "every {every:#}"),
            Fires::Scheduled(Schedule::Cron(cron)) => {
                write!(f, "on the cron {cron} in {}", cron.zone())
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Schedule {
    Every(SignedDuration),
    Cron(Cron),
}

impl Schedule {
    /// An interval counts from `anchor`; a cron expression ignores it.
    pub fn following(&self, anchor: Timestamp, at: Timestamp) -> Result<Timestamp> {
        match self {
            Schedule::Every(every) => {
                let elapsed = at.duration_since(anchor).as_nanos() / every.as_nanos();
                Ok(Timestamp::from_nanosecond(
                    anchor.as_nanosecond() + (elapsed + 1) * every.as_nanos(),
                )?)
            }
            Schedule::Cron(cron) => cron.after(at),
        }
    }

    pub fn fastest(&self) -> SignedDuration {
        match self {
            Schedule::Every(every) => *every,
            Schedule::Cron(cron) => cron.fastest(),
        }
    }
}

/// What a firing renders from the Event: never the Agent or the Project (ADR-0013).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Templates {
    pub brief: Template,
    pub branch: Option<Template>,
    pub correlation: Correlation,
}

/// A Trigger's correlation key and what a firing does around it, present together or not at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Correlation {
    None,
    On {
        template: Template,
        on_miss: CorrelationMiss,
        on_open_workspace: OnOpenWorkspace,
    },
}

impl Correlation {
    /// The one place a declared correlation, its miss behaviour and its open-workspace behaviour
    /// are read, so a Trigger never holds one without the others.
    pub fn parse(
        correlation: Option<&str>,
        on_miss: Option<&str>,
        on_open_workspace: Option<&str>,
    ) -> Result<Self> {
        let template = correlation
            .map(str::parse)
            .transpose()
            .context("a trigger correlation")?;
        let on_miss = on_miss
            .map(str::parse)
            .transpose()
            .context("a trigger on_miss")?;
        let on_open_workspace = on_open_workspace
            .map(str::parse)
            .transpose()
            .context("a trigger on_open_workspace")?
            .unwrap_or_default();

        match (template, on_miss) {
            (Some(template), Some(on_miss)) => Ok(Correlation::On {
                template,
                on_miss,
                on_open_workspace,
            }),
            (Some(_), None) => {
                bail!("a trigger with a correlation must declare what it does when it misses")
            }
            (None, Some(_)) => {
                bail!("a trigger without a correlation cannot declare what it does when it misses")
            }
            (None, None) if on_open_workspace == OnOpenWorkspace::NewSession => bail!(
                "a trigger without a correlation never finds an open workspace to start a new \
                 session in"
            ),
            (None, None) => Ok(Correlation::None),
        }
    }

    pub fn template(&self) -> Option<&Template> {
        match self {
            Correlation::None => None,
            Correlation::On { template, .. } => Some(template),
        }
    }

    pub fn on_miss(&self) -> Option<CorrelationMiss> {
        match self {
            Correlation::None => None,
            Correlation::On { on_miss, .. } => Some(*on_miss),
        }
    }

    pub fn on_open_workspace(&self) -> OnOpenWorkspace {
        match self {
            Correlation::None => OnOpenWorkspace::Continue,
            Correlation::On {
                on_open_workspace, ..
            } => *on_open_workspace,
        }
    }
}

/// Fixed when the Workspace opens, so a Project redeclared later moves no Workspace already on it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Checkout {
    pub repositories: Vec<String>,
    pub base: String,
    pub branch: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Interrupting {
    pub participant: String,
    pub requested_at: Timestamp,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeldMessage {
    /// The Workspace's own sequence, never reused because a taken or withdrawn row is kept.
    pub id: i64,
    pub participant: String,
    pub message: String,
    pub posted_at: Timestamp,
    pub edited_at: Option<Timestamp>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartedBy {
    Event(EventRecordId),
    Participant(String),
}

#[derive(Debug, Clone)]
pub struct Workspace {
    pub id: WorkspaceId,
    pub name: String,
    pub organization: Organization,
    pub project: Project,
    /// As declared now: its first Session runs this Agent unless it names another.
    pub opened_with: Agent,
    pub profile: Option<SubscriptionProfile>,
    pub checkout: Checkout,
    pub correlation: Option<String>,
    pub state: WorkspaceState,
    pub opened_at: Timestamp,
    pub last_active_at: Timestamp,
    pub sealed_at: Option<Timestamp>,
    pub continues: Option<WorkspaceId>,
    pub started_by: Option<StartedBy>,
}

impl Workspace {
    pub fn accepts(&self, operation: &'static str, what: &str) -> Result<()> {
        if self.state == WorkspaceState::Sealed {
            bail!(crate::workspace::workspace_conflict(
                operation,
                self,
                "sealed",
                format!("the workspace {} is sealed, and accepts no {what}", self.id),
            ));
        }

        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct Session {
    pub id: SessionId,
    pub name: String,
    pub organization: OrganizationId,
    pub workspace: WorkspaceId,
    /// With the harness and model fixed when the Session was enqueued, never redeclared under it.
    pub agent: Agent,
    pub state: SessionState,
    pub preparing: Option<Preparing>,
    pub exit: Option<Exit>,
    pub outcome_message: Option<String>,
    pub evidence: Option<Evidence>,
    pub sign_in_method: Option<String>,
    pub supplied: Vec<Supplied>,
    pub instance: Option<String>,
    pub supervisor: Option<String>,
    /// What the Harness reported it worked on.
    pub worked_model: Option<String>,
    pub title: Option<String>,
    pub options: Vec<SessionOption>,
    pub changing_options: Vec<ChangingOption>,
    pub commands: Vec<SessionCommand>,
    pub interrupting: Option<Interrupting>,
    pub enqueued_at: Timestamp,
    pub started_at: Option<Timestamp>,
    pub ended_at: Option<Timestamp>,
    pub lease_expires_at: Option<Timestamp>,
    pub connected: Option<Connected>,
    pub usage: Option<Usage>,
    pub depends_on: Vec<Blocker>,
}

impl Session {
    /// Why nothing could ever wait on this Session, when it can no longer end successfully.
    pub fn never_succeeds(&self) -> Option<String> {
        match (&self.state, &self.exit) {
            (SessionState::Unreachable, _) => Some("is unreachable".to_owned()),
            (_, Some(Exit::Failed { because })) => Some(format!("failed: {because}")),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Blocker {
    pub id: SessionId,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Turn {
    pub seq: i64,
    pub prompted_at: Timestamp,
    pub answered_at: Option<Timestamp>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangingOption {
    pub option: String,
    pub category: String,
    pub value: String,
    pub participant: String,
}

/// `category` is kestrel's own name for a well-known option, or the harness's own string, which may
/// begin with `_`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionOption {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(flatten)]
    pub kind: SessionOptionKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SessionOptionKind {
    Select {
        current: String,
        #[serde(default)]
        values: Vec<SessionOptionValue>,
        #[serde(default)]
        groups: Vec<SessionOptionGroup>,
    },
    Boolean {
        current: bool,
    },
}

impl SessionOption {
    pub const MODE: &'static str = "mode";
    pub const MODEL: &'static str = "model";
    pub const MODEL_CONFIG: &'static str = "model_config";
    pub const THOUGHT_LEVEL: &'static str = "thought_level";

    pub fn current_value(&self) -> Option<String> {
        match &self.kind {
            SessionOptionKind::Select { current, .. } => Some(current.clone()),
            SessionOptionKind::Boolean { current } => Some(current.to_string()),
        }
    }

    pub fn is_category(&self, category: &str) -> bool {
        self.category.as_deref() == Some(category)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionOptionValue {
    pub value: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionOptionGroup {
    pub group: String,
    pub name: String,
    pub values: Vec<SessionOptionValue>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionCommand {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub input_hint: Option<String>,
}

/// What the Harness has spent on behalf of a Session, cumulative rather than per turn.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Usage {
    pub context_used: u64,
    pub context_size: u64,
    pub cost: Option<Cost>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Cost {
    pub amount: f64,
    pub currency: String,
}

impl fmt::Display for Usage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} of {} tokens", self.context_used, self.context_size)?;
        match &self.cost {
            Some(cost) => write!(f, ", {:.2} {}", cost.amount, cost.currency),
            None => Ok(()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    Queued,
    Working,
    /// Answered, while work its agent started still runs (ADR-0040): busy like Working, so it holds
    /// its Active-Work Slot and its Instance.
    Trailing,
    Waiting,
    /// The first message becomes the Brief and the first Turn (ADR-0038); it holds no Active-Work
    /// Slot.
    Unbriefed,
    Ended,
    /// Terminal like `Ended`, but with no exit status: a queued Session whose declared tolerance
    /// can no longer be met never ran, so nothing failed.
    Unreachable,
}

impl SessionState {
    pub const LIVE: [SessionState; 4] = [
        SessionState::Working,
        SessionState::Trailing,
        SessionState::Waiting,
        SessionState::Unbriefed,
    ];

    pub const OCCUPYING: [SessionState; 2] = [SessionState::Working, SessionState::Trailing];

    pub const fn as_str(self) -> &'static str {
        match self {
            SessionState::Queued => "queued",
            SessionState::Working => "working",
            SessionState::Trailing => "trailing",
            SessionState::Waiting => "waiting",
            SessionState::Unbriefed => "unbriefed",
            SessionState::Ended => "ended",
            SessionState::Unreachable => "unreachable",
        }
    }

    /// `None` once the Session has ended: there is nothing left to stop.
    pub fn stop_exit(self) -> Option<Exit> {
        match self {
            SessionState::Ended | SessionState::Unreachable => None,
            SessionState::Queued => Some(Exit::Failed {
                because: "it was stopped before it started".into(),
            }),
            SessionState::Unbriefed => Some(Exit::Failed {
                because: "it was stopped before its first turn".into(),
            }),
            SessionState::Working => Some(Exit::Failed {
                because: "it was stopped mid-turn, before its agent answered".into(),
            }),
            SessionState::Trailing | SessionState::Waiting => Some(Exit::Succeeded),
        }
    }
}

impl FromStr for SessionState {
    type Err = anyhow::Error;

    fn from_str(state: &str) -> Result<Self> {
        match state {
            "queued" => Ok(SessionState::Queued),
            "working" => Ok(SessionState::Working),
            "trailing" => Ok(SessionState::Trailing),
            "waiting" => Ok(SessionState::Waiting),
            "unbriefed" => Ok(SessionState::Unbriefed),
            "ended" => Ok(SessionState::Ended),
            "unreachable" => Ok(SessionState::Unreachable),
            other => bail!("{other} is not a state a session can be in"),
        }
    }
}

/// Current Session state, never a Transcript entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Preparing {
    Provisioning,
    Cloning,
    StartingHarness,
    HarnessReady,
}

impl Preparing {
    pub const fn as_str(self) -> &'static str {
        match self {
            Preparing::Provisioning => "provisioning",
            Preparing::Cloning => "cloning",
            Preparing::StartingHarness => "starting_harness",
            Preparing::HarnessReady => "harness_ready",
        }
    }
}

impl FromStr for Preparing {
    type Err = anyhow::Error;

    fn from_str(preparing: &str) -> Result<Self> {
        match preparing {
            "provisioning" => Ok(Preparing::Provisioning),
            "cloning" => Ok(Preparing::Cloning),
            "starting_harness" => Ok(Preparing::StartingHarness),
            "harness_ready" => Ok(Preparing::HarnessReady),
            other => bail!("{other} is not a step a session prepares on"),
        }
    }
}

impl fmt::Display for SessionState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Exit {
    Succeeded,
    Failed { because: String },
}

impl Exit {
    pub const fn status(&self) -> &'static str {
        match self {
            Exit::Succeeded => "succeeded",
            Exit::Failed { .. } => "failed",
        }
    }

    pub fn because(&self) -> Option<&str> {
        match self {
            Exit::Succeeded => None,
            Exit::Failed { because } => Some(because),
        }
    }

    pub fn read(status: &str, because: Option<String>) -> Result<Self> {
        match status {
            "succeeded" => Ok(Exit::Succeeded),
            "failed" => Ok(Exit::Failed {
                because: because.unwrap_or_default(),
            }),
            other => bail!("{other} is not an exit status a session can end with"),
        }
    }
}

impl fmt::Display for Exit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Exit::Succeeded => f.write_str("succeeded"),
            Exit::Failed { because } => write!(f, "failed: {because}"),
        }
    }
}

/// One variable or file a Session's harness was last spawned with, by the revision handed over
/// and never by value.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Supplied {
    /// The Subscription Profile it came from, or none for the Organization's Provider Credential.
    pub profile: Option<String>,
    pub kind: crate::profile::Kind,
    pub name: String,
    pub revision: i64,
    /// Whether that revision is still what is held.
    pub current: bool,
    pub handed_at: Timestamp,
}

/// Facts about a failure, never a harness's prose: an unknown summary cannot establish that a
/// sign-in expired or is not covered (ADR-0052).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Evidence {
    AuthenticationRequired {
        code: i32,
        methods: Vec<String>,
        method: Option<String>,
    },
    ExecutableMissing {
        command: String,
        error: OsError,
    },
    Unknown {
        summary: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OsError {
    pub kind: OsErrorKind,
    pub code: Option<i32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OsErrorKind {
    NotFound,
    PermissionDenied,
    Other,
}

impl Evidence {
    const SUMMARY_LIMIT: usize = 1024;
    const NAME_LIMIT: usize = 256;
    const METHODS_LIMIT: usize = 16;

    /// Bounded again on receipt, since a supervisor runs beside the harness it reports on.
    #[must_use]
    pub fn bounded(self) -> Self {
        match self {
            Evidence::AuthenticationRequired {
                code,
                methods,
                method,
            } => Evidence::AuthenticationRequired {
                code,
                methods: methods
                    .into_iter()
                    .take(Self::METHODS_LIMIT)
                    .map(|method| truncated(method, Self::NAME_LIMIT))
                    .collect(),
                method: method.map(|method| truncated(method, Self::NAME_LIMIT)),
            },
            Evidence::ExecutableMissing { command, error } => Evidence::ExecutableMissing {
                command: truncated(command, Self::NAME_LIMIT),
                error,
            },
            Evidence::Unknown { summary } => Evidence::Unknown {
                summary: truncated(summary, Self::SUMMARY_LIMIT),
            },
        }
    }
}

fn truncated(mut text: String, limit: usize) -> String {
    if text.len() > limit {
        let mut end = limit;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
    }
    text
}

#[derive(Debug, Clone)]
pub struct Connected {
    pub at: Timestamp,
    pub version: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceState {
    Open,
    Sealed,
}

impl WorkspaceState {
    pub const fn as_str(self) -> &'static str {
        match self {
            WorkspaceState::Open => "open",
            WorkspaceState::Sealed => "sealed",
        }
    }
}

impl FromStr for WorkspaceState {
    type Err = anyhow::Error;

    fn from_str(state: &str) -> Result<Self> {
        match state {
            "open" => Ok(WorkspaceState::Open),
            "sealed" => Ok(WorkspaceState::Sealed),
            other => bail!("{other} is not a state a workspace can be in"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PullRequestState {
    Open,
    Closed,
    Merged,
}

impl PullRequestState {
    pub const fn as_str(self) -> &'static str {
        match self {
            PullRequestState::Open => "open",
            PullRequestState::Closed => "closed",
            PullRequestState::Merged => "merged",
        }
    }
}

impl FromStr for PullRequestState {
    type Err = anyhow::Error;

    fn from_str(state: &str) -> Result<Self> {
        match state {
            "open" => Ok(PullRequestState::Open),
            "closed" => Ok(PullRequestState::Closed),
            "merged" => Ok(PullRequestState::Merged),
            other => bail!("{other} is not a state a pull request can be in"),
        }
    }
}

/// `repository` is the head repository's URL as fixed on the Workspace, not GitHub's `owner/name`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PullRequest {
    pub repository: String,
    pub number: i64,
    pub url: String,
    pub title: String,
    pub state: PullRequestState,
    pub head_branch: String,
    pub head_revision: String,
    pub updated_at: Timestamp,
    pub event: EventRecordId,
}

impl fmt::Display for WorkspaceState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stopping_succeeds_only_an_answered_session() {
        assert!(matches!(
            SessionState::Queued.stop_exit(),
            Some(Exit::Failed { .. })
        ));
        assert!(matches!(
            SessionState::Working.stop_exit(),
            Some(Exit::Failed { .. })
        ));
        assert_eq!(SessionState::Waiting.stop_exit(), Some(Exit::Succeeded));
        assert_eq!(SessionState::Trailing.stop_exit(), Some(Exit::Succeeded));
        assert_eq!(SessionState::Ended.stop_exit(), None);
        assert_eq!(SessionState::Unreachable.stop_exit(), None);
    }
}
