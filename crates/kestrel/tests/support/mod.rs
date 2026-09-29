//! The primary test seam (0.1/03): boot a complete control plane in-process against a fresh
//! temporary SQLite file, drive it through the same paths a person would use, and tear it
//! down. Assertions live in the language of Workspaces, Sessions and Transcripts; `Store` and `Log`
//! stay behind `Kestrel`, never reached for directly.

// Every integration-test binary compiles all of this; a helper one of them does not reach for
// is not dead, it belongs to a sibling.
#![allow(dead_code)]

pub mod built;
pub mod client;
pub mod compose;
pub mod control_plane;
pub mod diagnostics;
pub mod docker;
pub mod environment;
pub mod git;
pub mod github_stub;
pub mod image;
pub mod images;
pub mod lineage;
pub mod link_client;
pub mod model;
pub mod operator_log;
pub mod repository;
pub mod scripted_agent;
pub mod supervisor;

/// The crate's checkout root, resolved at run time so that a binary compiled in one
/// worktree still finds the right files when `cargo test` runs it from another.
pub fn crate_root() -> std::path::PathBuf {
    std::env::var_os("CARGO_MANIFEST_DIR").map_or_else(
        || std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")),
        std::path::PathBuf::from,
    )
}

use std::collections::BTreeSet;
use std::net::SocketAddr;
use std::num::NonZeroUsize;
use std::path::Path;

use jiff::{SignedDuration, Timestamp};
use kestrel::agent;
use kestrel::compute::{Docker, Driver, LocalExec};
use kestrel::domain::{
    Agent, Correlation, CorrelationMiss, Direction, Event, EventRecordId, Exit, Fires, Integration,
    Occurrence, OnOpenWorkspace, Organization, Project, Schedule, Session, SessionId, SessionState,
    SubscriptionProfile, Templates, Trigger, Turn, Workspace, WorkspaceId,
};
use kestrel::instance;
use kestrel::integration::{self, Connecting, Registration};
use kestrel::link::credential::Secret;
use kestrel::link::{self, Instruction};
use kestrel::log::{Cursor, Entry, Page, TranscriptEntry, Unreadable, Window};
use kestrel::profile::{self, Contents};
use kestrel::provider::{self, Held};
use kestrel::queue;
use kestrel::role::serve::{self, Listen};
use kestrel::role::work::{Dispatch, HarnessCommand};
use kestrel::store::Store;
use kestrel::timer::Wake;
use kestrel::trigger::apply::Applied;
use kestrel::trigger::{self, Against, Asked, Declaration, Tested};
use kestrel::work::{self, Claimed};
use kestrel::workspace;
use tempfile::TempDir;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

const PATIENCE: std::time::Duration = std::time::Duration::from_secs(30);

/// Distinctive enough that a test can assert it is nowhere it should not be.
pub const TOKEN: &str = "ghp_kestrel_should_never_say_this_out_loud";

/// The Provider Credential every fixture holds: a Session reaches no model without one, and the
/// scripted agent's `Confides` script says it can see this one.
pub const PROVIDER_KEY: &str = "SCRIPTED_API_KEY";
pub const A_PROVIDER_KEY: &str = "a-provider-key";

pub fn labelled_on(repository: &str, label: &str) -> String {
    serde_json::json!({"all": [
        {"exact": {"source": format!("https://github.com/{repository}")}},
        {"exact": {"type": "com.github.issues.labeled"}},
        {"exact": {"data.label.name": label}},
    ]})
    .to_string()
}

pub const BRIEF: &str = "Work on {{ event.data.issue.title }}";

pub fn templates(brief: &str, branch: Option<&str>, correlation: Option<&str>) -> Templates {
    let parsed = |template: &str| template.parse().expect("the template should parse");

    Templates {
        brief: parsed(brief),
        branch: branch.map(parsed),
        correlation: match correlation {
            Some(correlation) => Correlation::On {
                template: parsed(correlation),
                on_miss: CorrelationMiss::Open,
                on_open_workspace: OnOpenWorkspace::Continue,
            },
            None => Correlation::None,
        },
    }
}

/// A Template set with one of its correlation's behaviours overridden, so a declaration the
/// operator API would refuse can be built.
fn correlation_overridden(
    templates: &Templates,
    on_miss: Option<CorrelationMiss>,
    on_open_workspace: OnOpenWorkspace,
) -> anyhow::Result<Templates> {
    let template = templates.correlation.template().map(ToString::to_string);
    let correlation = Correlation::parse(
        template.as_deref(),
        on_miss.map(CorrelationMiss::as_str),
        Some(on_open_workspace.as_str()),
    )?;

    Ok(Templates {
        brief: templates.brief.clone(),
        branch: templates.branch.clone(),
        correlation,
    })
}

const LOOPBACK: SocketAddr =
    SocketAddr::new(std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST), 0);

pub struct Kestrel {
    cleanup: Cleanup,
    data_dir: TempDir,
    store: Store,
    bound: Listen,
    environment: Option<Provisions>,
    shutdown: CancellationToken,
    roles: JoinHandle<anyhow::Result<()>>,
}

struct Cleanup {
    data_dir: std::path::PathBuf,
    environment: Option<Provisions>,
    shutdown: CancellationToken,
    abort: tokio::task::AbortHandle,
    armed: bool,
}

impl Drop for Cleanup {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        self.shutdown.cancel();
        self.abort.abort();

        destroy_instances_on_drop(
            self.data_dir.clone(),
            self.environment
                .as_ref()
                .map(|environment| environment.driver.clone()),
        );
    }
}

/// What the work role provisions an Environment with.
#[derive(Clone)]
pub struct Provisions {
    driver: Driver,
    harnesses: Vec<HarnessCommand>,
    max_active_sessions: NonZeroUsize,
}

/// The harness an Agent names unless a test says otherwise, spawned as whatever the test plays.
pub const HARNESS: &str = "opencode";
/// The harness whose Sessions on one Subscription Profile the work role dispatches one at a time.
pub const SERIALIZED: &str = "codex";

fn spawning(harnesses: &[(&str, &str)]) -> Vec<HarnessCommand> {
    harnesses
        .iter()
        .map(|&(name, command)| HarnessCommand {
            name: name.to_owned(),
            command: command.to_owned(),
        })
        .collect()
}

/// Comes back on the address it was listening on, so what an Environment already dialled
/// still reaches it.
pub struct Stopped {
    cleanup: Cleanup,
    data_dir: TempDir,
    bound: Listen,
    environment: Option<Provisions>,
}

impl Kestrel {
    /// Boots with no supervisor to provision an Environment with, so the work role claims
    /// nothing and a test is the only thing dispatching the Sessions it opens.
    pub async fn boot() -> Self {
        Self::booted(None).await
    }

    /// Bound on every interface rather than on loopback, because what dials this one is a
    /// container and reaches this machine by its gateway address.
    pub async fn boot_reachable_from_an_environment() -> Self {
        let data_dir = TempDir::new().expect("a temporary data directory");
        Self::boot_against(
            data_dir,
            Listen {
                link: "0.0.0.0:0".parse().expect("every interface"),
                operator: LOOPBACK,
            },
            None,
        )
        .await
    }

    pub async fn boot_with_the_operator_beyond_loopback() -> Self {
        let data_dir = TempDir::new().expect("a temporary data directory");
        Self::boot_against(
            data_dir,
            Listen {
                link: LOOPBACK,
                operator: "0.0.0.0:0".parse().expect("every interface"),
            },
            None,
        )
        .await
    }

    pub async fn dispatching(supervisor: &Path) -> Self {
        Self::dispatching_to(
            supervisor,
            &scripted_agent::playing(scripted_agent::Script::Speaks),
        )
        .await
    }

    pub async fn dispatching_to(supervisor: &Path, command: &str) -> Self {
        Self::dispatching_up_to(supervisor, command, 2).await
    }

    pub async fn dispatching_up_to(supervisor: &Path, command: &str, maximum: usize) -> Self {
        Self::dispatching_harnesses_up_to(supervisor, &[(HARNESS, command)], maximum).await
    }

    pub async fn dispatching_harnesses(supervisor: &Path, harnesses: &[(&str, &str)]) -> Self {
        Self::dispatching_harnesses_up_to(supervisor, harnesses, 2).await
    }

    pub async fn dispatching_harnesses_up_to(
        supervisor: &Path,
        harnesses: &[(&str, &str)],
        maximum: usize,
    ) -> Self {
        Self::booted(Some(Provisions {
            driver: Driver::LocalExec(LocalExec::running(supervisor)),
            harnesses: spawning(harnesses),
            max_active_sessions: NonZeroUsize::new(maximum).expect("at least one active session"),
        }))
        .await
    }

    pub async fn dispatching_in(image: &str, command: &str) -> Self {
        Self::dispatching_harnesses_in(image, &[(HARNESS, command)]).await
    }

    /// The Docker driver, on a control plane bound where a container can dial out to it.
    pub async fn dispatching_harnesses_in(image: &str, harnesses: &[(&str, &str)]) -> Self {
        let data_dir = TempDir::new().expect("a temporary data directory");
        Self::boot_against(
            data_dir,
            Listen {
                link: "0.0.0.0:0".parse().expect("every interface"),
                operator: LOOPBACK,
            },
            Some(Provisions {
                driver: Driver::Docker(Docker::provisioning_from(image)),
                harnesses: spawning(harnesses),
                max_active_sessions: NonZeroUsize::new(2).unwrap(),
            }),
        )
        .await
    }

    async fn booted(environment: Option<Provisions>) -> Self {
        let data_dir = TempDir::new().expect("a temporary data directory");
        Self::boot_against(
            data_dir,
            Listen {
                link: LOOPBACK,
                operator: LOOPBACK,
            },
            environment,
        )
        .await
    }

    async fn boot_against(
        data_dir: TempDir,
        listen: Listen,
        environment: Option<Provisions>,
    ) -> Self {
        let store = Store::open(data_dir.path())
            .await
            .expect("the control plane should boot against a fresh data directory");
        let shutdown = CancellationToken::new();
        let all_in_one = kestrel::role::bind(store.clone(), listen)
            .await
            .expect("the control plane should bind its link");
        let bound = all_in_one.bound();
        let address = bound.link;
        let dispatch = environment.clone().map(|provisions| Dispatch {
            link: match provisions.driver {
                Driver::Docker(_) => format!("http://host.docker.internal:{}", address.port()),
                Driver::LocalExec(_) => format!("http://{address}"),
            },
            driver: provisions.driver,
            harnesses: provisions.harnesses,
            auth: None,
            max_active_sessions: provisions.max_active_sessions,
            serialized: vec![SERIALIZED.to_owned()],
        });
        let roles = tokio::spawn(all_in_one.run(dispatch, shutdown.clone()));

        Self::running(data_dir, store, bound, environment, shutdown, roles)
    }

    /// Serves the link with no work role behind it, so nothing sweeps a lease a test has let
    /// lapse until it restarts as a whole control plane.
    pub async fn boot_serving_alone() -> Self {
        let data_dir = TempDir::new().expect("a temporary data directory");
        let store = Store::open(data_dir.path())
            .await
            .expect("the control plane should boot against a fresh data directory");
        let shutdown = CancellationToken::new();
        let listening = serve::bind(
            store.clone(),
            Listen {
                link: LOOPBACK,
                operator: LOOPBACK,
            },
            Wake::default(),
        )
        .await
        .expect("the control plane should bind its link");
        let bound = listening.bound();
        let roles = tokio::spawn(serve::run(listening, shutdown.clone()));

        Self::running(data_dir, store, bound, None, shutdown, roles)
    }

    fn running(
        data_dir: TempDir,
        store: Store,
        bound: Listen,
        environment: Option<Provisions>,
        shutdown: CancellationToken,
        roles: JoinHandle<anyhow::Result<()>>,
    ) -> Self {
        let cleanup = Cleanup {
            data_dir: data_dir.path().to_path_buf(),
            environment: environment.clone(),
            shutdown: shutdown.clone(),
            abort: roles.abort_handle(),
            armed: true,
        };

        Self {
            cleanup,
            data_dir,
            store,
            bound,
            environment,
            shutdown,
            roles,
        }
    }

    pub fn data_dir(&self) -> &Path {
        self.data_dir.path()
    }

    pub fn is_running(&self) -> bool {
        !self.roles.is_finished()
    }

    /// Takes SQLite's write lock the way another process on the same file would.
    pub async fn while_the_database_is_locked<T>(&self, meanwhile: impl Future<Output = T>) -> T {
        let pool = database(self.data_dir()).await;
        let mut holder = pool.acquire().await.expect("a connection");
        sqlx::query("BEGIN IMMEDIATE")
            .execute(&mut *holder)
            .await
            .expect("the write lock should be free to take");

        let done = meanwhile.await;

        sqlx::query("ROLLBACK")
            .execute(&mut *holder)
            .await
            .expect("the write lock should release");
        drop(holder);
        pool.close().await;

        done
    }

    pub fn link(&self) -> String {
        format!("http://{}", self.bound.link)
    }

    pub fn operator(&self) -> String {
        format!("http://{}", self.bound.operator)
    }

    pub fn operator_port(&self) -> u16 {
        self.bound.operator.port()
    }

    pub fn link_from_an_environment(&self) -> String {
        format!("http://host.docker.internal:{}", self.bound.link.port())
    }

    pub async fn declare_organization(&self, name: &str) -> Organization {
        let mut tx = self.store.begin().await.expect("a transaction");
        let organization = tx
            .organizations()
            .declare(name, None)
            .await
            .expect("the organization should declare")
            .record;
        tx.commit().await.expect("the declaration should commit");
        organization
    }

    pub async fn declare_limited_organization(&self, name: &str, maximum: usize) -> Organization {
        let mut tx = self.store.begin().await.expect("a transaction");
        let organization = tx
            .organizations()
            .declare(
                name,
                Some(NonZeroUsize::new(maximum).expect("at least one live instance")),
            )
            .await
            .expect("the organization should declare")
            .record;
        tx.commit().await.expect("the declaration should commit");
        organization
    }

    pub async fn organizations(&self) -> Vec<Organization> {
        let mut tx = self.store.begin().await.expect("a transaction");
        tx.organizations()
            .all()
            .await
            .expect("organizations should list")
    }

    pub async fn declare_project(
        &self,
        organization: &Organization,
        name: &str,
        repositories: &[String],
        branch: &str,
    ) -> Project {
        let mut tx = self.store.begin().await.expect("a transaction");
        let project = tx
            .projects()
            .declare(organization, name, repositories, branch)
            .await
            .expect("the project should declare")
            .record;
        tx.commit().await.expect("the declaration should commit");
        project
    }

    pub async fn projects(&self, organization: &Organization) -> Vec<Project> {
        let mut tx = self.store.begin().await.expect("a transaction");
        tx.projects()
            .all(organization)
            .await
            .expect("projects should list")
    }

    pub async fn declare_agent(
        &self,
        organization: &Organization,
        name: &str,
        harness: &str,
        model: Option<&str>,
    ) -> Agent {
        self.try_declare_agent(organization, name, harness, model)
            .await
            .expect("the agent should declare")
    }

    pub async fn try_declare_agent(
        &self,
        organization: &Organization,
        name: &str,
        harness: &str,
        model: Option<&str>,
    ) -> anyhow::Result<Agent> {
        agent::declare(&self.store, &organization.name, name, harness, model)
            .await
            .map(|declared| declared.record)
    }

    pub async fn set_agent_model(
        &self,
        organization: &Organization,
        name: &str,
        model: Option<&str>,
    ) -> Agent {
        agent::set_model(&self.store, &organization.name, name, model)
            .await
            .expect("the model should change")
    }

    pub async fn agents(&self, organization: &Organization) -> Vec<Agent> {
        let mut tx = self.store.begin().await.expect("a transaction");
        tx.agents()
            .all(organization)
            .await
            .expect("agents should list")
    }

    pub async fn register_integration(
        &self,
        organization: &str,
        name: &str,
        repository: &str,
        api: &str,
        carries: &[Direction],
        interval: SignedDuration,
    ) -> Integration {
        self.try_register_integration(organization, name, repository, api, carries, interval)
            .await
            .expect("the integration should register")
    }

    pub async fn try_register_integration(
        &self,
        organization: &str,
        name: &str,
        repository: &str,
        api: &str,
        carries: &[Direction],
        interval: SignedDuration,
    ) -> anyhow::Result<Integration> {
        integration::register(
            &self.store,
            Registration {
                organization,
                name,
                carries,
                connecting: Connecting::Github {
                    repository,
                    api,
                    token: TOKEN,
                    interval,
                    signing_secret: None,
                },
            },
        )
        .await
    }

    pub async fn register_signed_github(
        &self,
        organization: &str,
        name: &str,
        repository: &str,
        api: &str,
        signing_secret: &str,
    ) -> Integration {
        integration::register(
            &self.store,
            Registration {
                organization,
                name,
                carries: &[Direction::Inbound, Direction::Outbound],
                connecting: Connecting::Github {
                    repository,
                    api,
                    token: TOKEN,
                    interval: SignedDuration::from_millis(1),
                    signing_secret: Some(signing_secret),
                },
            },
        )
        .await
        .expect("the integration should register")
    }

    pub async fn register_webhook(
        &self,
        organization: &str,
        name: &str,
        secret: &str,
    ) -> Integration {
        integration::register(
            &self.store,
            Registration {
                organization,
                name,
                carries: &[Direction::Inbound],
                connecting: Connecting::Webhook { secret },
            },
        )
        .await
        .expect("the webhook should register")
    }

    pub async fn integrations(&self, organization: &str) -> Vec<Integration> {
        integration::integrations(&self.store, organization)
            .await
            .expect("the integrations should list")
    }

    pub async fn acknowledge_event_refusal(&self, organization: &str, name: &str) {
        integration::acknowledge_event_refusal(&self.store, organization, name)
            .await
            .expect("the event refusal should be acknowledged");
    }

    pub async fn events(&self, organization: &str) -> Vec<Event> {
        integration::events(&self.store, organization, 100)
            .await
            .expect("the events should list")
    }

    pub async fn declare_trigger(
        &self,
        organization: &str,
        name: &str,
        filter: &str,
        project: &str,
        agent: &str,
    ) -> Trigger {
        self.declare_trigger_rendering(
            organization,
            name,
            filter,
            project,
            agent,
            &templates(BRIEF, None, None),
        )
        .await
    }

    pub async fn declare_trigger_rendering(
        &self,
        organization: &str,
        name: &str,
        filter: &str,
        project: &str,
        agent: &str,
        templates: &Templates,
    ) -> Trigger {
        self.declare_trigger_rendering_with_miss(
            organization,
            name,
            filter,
            project,
            agent,
            templates,
            templates.correlation.on_miss(),
        )
        .await
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "a trigger is what it is declared with"
    )]
    pub async fn declare_trigger_rendering_with_miss(
        &self,
        organization: &str,
        name: &str,
        filter: &str,
        project: &str,
        agent: &str,
        templates: &Templates,
        on_miss: Option<CorrelationMiss>,
    ) -> Trigger {
        self.try_declare_trigger_rendering_with_miss(
            organization,
            name,
            filter,
            project,
            agent,
            templates,
            on_miss,
        )
        .await
        .expect("the trigger should declare")
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "a trigger is what it is declared with"
    )]
    pub async fn try_declare_trigger_rendering_with_miss(
        &self,
        organization: &str,
        name: &str,
        filter: &str,
        project: &str,
        agent: &str,
        templates: &Templates,
        on_miss: Option<CorrelationMiss>,
    ) -> anyhow::Result<Trigger> {
        let templates = correlation_overridden(
            templates,
            on_miss,
            templates.correlation.on_open_workspace(),
        )?;
        trigger::declare(
            &self.store,
            Declaration {
                organization,
                name,
                fires: &Fires::On(filter.parse().expect("the filter should parse")),
                templates: &templates,
                project,
                agent,
                allows: &[],
                profile: None,
            },
        )
        .await
    }

    /// Labelled `ready-for-agent` on the repository, starting its work with `agent` unless a
    /// label chooses one of `allows`.
    pub async fn declare_trigger_allowing(
        &self,
        organization: &str,
        repository: &str,
        agent: &str,
        allows: &[&str],
        correlation: Option<&str>,
    ) -> Trigger {
        trigger::declare(
            &self.store,
            Declaration {
                organization,
                name: "ready",
                fires: &Fires::On(
                    labelled_on(repository, "ready-for-agent")
                        .parse()
                        .expect("the filter should parse"),
                ),
                templates: &templates(BRIEF, None, correlation),
                project: "kestrel",
                agent,
                allows: &allows
                    .iter()
                    .map(|&name| name.to_owned())
                    .collect::<Vec<_>>(),
                profile: None,
            },
        )
        .await
        .expect("the trigger should declare")
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "a trigger is what it is declared with"
    )]
    pub async fn declare_correlated_trigger(
        &self,
        organization: &str,
        name: &str,
        filter: &str,
        agent: &str,
        allows: &[&str],
        templates: &Templates,
        on_open_workspace: OnOpenWorkspace,
    ) -> Trigger {
        self.try_declare_correlated_trigger(
            organization,
            name,
            filter,
            agent,
            allows,
            templates,
            on_open_workspace,
        )
        .await
        .expect("the trigger should declare")
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "a trigger is what it is declared with"
    )]
    pub async fn try_declare_correlated_trigger(
        &self,
        organization: &str,
        name: &str,
        filter: &str,
        agent: &str,
        allows: &[&str],
        templates: &Templates,
        on_open_workspace: OnOpenWorkspace,
    ) -> anyhow::Result<Trigger> {
        let templates = correlation_overridden(
            templates,
            templates.correlation.on_miss(),
            on_open_workspace,
        )?;
        trigger::declare(
            &self.store,
            Declaration {
                organization,
                name,
                fires: &Fires::On(filter.parse().expect("the filter should parse")),
                templates: &templates,
                project: "kestrel",
                agent,
                allows: &allows
                    .iter()
                    .map(|&name| name.to_owned())
                    .collect::<Vec<_>>(),
                profile: None,
            },
        )
        .await
    }

    pub async fn apply_triggers(&self, organization: &str, file: &str) -> Applied {
        trigger::apply::apply(
            &self.store,
            organization,
            &trigger::apply::parse(file).expect("the declaration file should parse"),
            false,
        )
        .await
        .expect("the declaration file should apply")
    }

    pub async fn test_trigger(
        &self,
        organization: &str,
        name: &str,
        event: EventRecordId,
    ) -> Tested {
        self.try_test_trigger(organization, name, event)
            .await
            .expect("the trigger should test")
    }

    pub async fn try_test_trigger(
        &self,
        organization: &str,
        name: &str,
        event: EventRecordId,
    ) -> anyhow::Result<Tested> {
        trigger::test(
            &self.store,
            organization,
            name,
            Against::Event(event),
            Asked::default(),
        )
        .await
    }

    pub async fn dispatch(
        &self,
        organization: &str,
        name: &str,
        issue: i64,
        asked: trigger::Asked<'_>,
    ) -> anyhow::Result<trigger::Fired> {
        trigger::dispatch(
            &self.store,
            &kestrel::integration::github::Github::dialling_out()?,
            trigger::Dispatch {
                organization,
                trigger: name,
                integration: "github",
                issue,
                asked,
            },
        )
        .await
    }

    pub async fn test_dispatch(
        &self,
        organization: &str,
        name: &str,
        issue: i64,
        asked: Asked<'_>,
    ) -> anyhow::Result<Tested> {
        trigger::test(
            &self.store,
            organization,
            name,
            Against::Issue {
                github: &kestrel::integration::github::Github::dialling_out()?,
                integration: "github",
                issue,
            },
            asked,
        )
        .await
    }

    pub async fn try_declare_scheduled_trigger(
        &self,
        organization: &str,
        name: &str,
        schedule: Schedule,
        templates: &Templates,
    ) -> anyhow::Result<Trigger> {
        trigger::declare(
            &self.store,
            Declaration {
                organization,
                name,
                fires: &Fires::Scheduled(schedule),
                templates,
                project: "kestrel",
                agent: "builder",
                allows: &[],
                profile: None,
            },
        )
        .await
    }

    pub async fn test_scheduled_trigger(&self, organization: &str, name: &str) -> Tested {
        self.try_test_trigger_naming_no_event(organization, name)
            .await
            .expect("the trigger should test against its next elapsing")
    }

    pub async fn try_test_trigger_naming_no_event(
        &self,
        organization: &str,
        name: &str,
    ) -> anyhow::Result<Tested> {
        trigger::test(
            &self.store,
            organization,
            name,
            Against::NextElapsing,
            Asked::default(),
        )
        .await
    }

    /// Stands in for the wheel reaching `at`, which a test cannot wait for.
    pub async fn elapse(&self, at: Timestamp) -> Vec<Occurrence> {
        trigger::elapse(&self.store, at)
            .await
            .expect("the due schedules should elapse")
    }

    pub async fn test_declared_trigger(
        &self,
        organization: &str,
        file: &str,
        name: &str,
        event: EventRecordId,
    ) -> Tested {
        let declarations = trigger::apply::parse(file).expect("the declaration file should parse");
        let declared = declarations
            .iter()
            .find(|declared| declared.name == name)
            .expect("the declaration file should declare the trigger");

        trigger::test_declared(
            &self.store,
            organization,
            declared,
            Against::Event(event),
            Asked::default(),
        )
        .await
        .expect("the declared trigger should test")
    }

    pub async fn firings(&self, event: EventRecordId) -> Vec<kestrel::domain::Firing> {
        trigger::firings(&self.store, event)
            .await
            .expect("the firings should read")
    }

    pub async fn triggers(&self, organization: &str) -> Vec<Trigger> {
        trigger::triggers(&self.store, organization)
            .await
            .expect("the triggers should list")
    }

    pub async fn show_trigger(&self, organization: &str, name: &str) -> Trigger {
        trigger::show(&self.store, organization, name)
            .await
            .expect("the trigger should show")
    }

    pub async fn disable_trigger(&self, organization: &str, name: &str) -> Trigger {
        trigger::disable(&self.store, organization, name)
            .await
            .expect("the trigger should disable")
    }

    pub async fn enable_trigger(&self, organization: &str, name: &str) -> Trigger {
        trigger::enable(&self.store, organization, name)
            .await
            .expect("the trigger should enable")
    }

    pub async fn hold_provider_credential(
        &self,
        organization: &Organization,
        variable: &str,
        secret: &str,
    ) {
        provider::hold(&self.store, &organization.name, variable, secret)
            .await
            .expect("the provider credential should be held");
    }

    pub async fn provider_credentials_held(&self, organization: &Organization) -> Vec<Held> {
        provider::held(&self.store, &organization.name)
            .await
            .expect("what the organization holds should list")
    }

    pub async fn declare_profile(
        &self,
        organization: &str,
        name: &str,
        owner: &str,
    ) -> anyhow::Result<SubscriptionProfile> {
        profile::declare(&self.store, organization, name, owner)
            .await
            .map(|declared| declared.record)
    }

    pub async fn hold_in_profile(
        &self,
        organization: &str,
        name: &str,
        entry: &profile::Entry,
        login: &str,
    ) {
        profile::hold(&self.store, organization, name, entry, login)
            .await
            .expect("the login should be held");
    }

    pub async fn profiles(
        &self,
        organization: &str,
    ) -> Vec<(SubscriptionProfile, Vec<profile::Held>)> {
        profile::profiles(&self.store, organization)
            .await
            .expect("the profiles should list")
    }

    /// What the next Session spawned with the profile would be handed.
    pub async fn profile_contents(&self, profile: &SubscriptionProfile) -> Contents {
        profile::contents(&self.store, profile)
            .await
            .expect("the profile should open")
    }

    pub async fn open_workspace_with(
        &self,
        organization: &str,
        project: &str,
        agent: &str,
        profile: &str,
    ) -> Workspace {
        workspace::open(
            &self.store,
            organization,
            project,
            agent,
            Some(profile),
            None,
            None,
        )
        .await
        .expect("the workspace should open")
    }

    pub async fn open_workspace(
        &self,
        organization: &str,
        project: &str,
        agent: &str,
    ) -> Workspace {
        self.try_open_workspace(organization, project, agent, None)
            .await
            .expect("the workspace should open")
    }

    pub async fn open_workspace_on(
        &self,
        organization: &str,
        project: &str,
        agent: &str,
        branch: &str,
    ) -> Workspace {
        workspace::open(
            &self.store,
            organization,
            project,
            agent,
            None,
            Some(branch),
            None,
        )
        .await
        .expect("the workspace should open")
    }

    pub async fn continue_workspace(
        &self,
        organization: &str,
        project: &str,
        agent: &str,
        continues: WorkspaceId,
    ) -> Workspace {
        self.try_open_workspace(organization, project, agent, Some(continues))
            .await
            .expect("the workspace should open")
    }

    pub async fn try_open_workspace(
        &self,
        organization: &str,
        project: &str,
        agent: &str,
        continues: Option<WorkspaceId>,
    ) -> anyhow::Result<Workspace> {
        let continues = continues.map(|sealed| sealed.to_string());
        workspace::open(
            &self.store,
            organization,
            project,
            agent,
            None,
            None,
            continues.as_deref(),
        )
        .await
    }

    pub async fn seal_workspace(&self, id: WorkspaceId) -> Workspace {
        self.try_seal_workspace(id)
            .await
            .expect("the workspace should seal")
    }

    pub async fn try_seal_workspace(&self, id: WorkspaceId) -> anyhow::Result<Workspace> {
        workspace::seal(&self.store, id).await
    }

    pub async fn held_instances(&self, organization: &str) -> Vec<instance::Held> {
        instance::held(&self.store, organization)
            .await
            .expect("the held instances should read")
    }

    pub async fn release_instance(&self, workspace: WorkspaceId) -> String {
        self.try_release_instance(workspace)
            .await
            .expect("the instance should release")
    }

    pub async fn try_release_instance(&self, workspace: WorkspaceId) -> anyhow::Result<String> {
        instance::release(&self.store, workspace, "operator").await
    }

    pub async fn continuations(&self, id: WorkspaceId) -> Vec<WorkspaceId> {
        workspace::continuations(&self.store, id)
            .await
            .expect("the continuations should read")
    }

    pub async fn workspaces(&self, organization: &str) -> Vec<Workspace> {
        workspace::workspaces(&self.store, organization)
            .await
            .expect("the workspaces should list")
    }

    pub async fn show_workspace(&self, id: WorkspaceId) -> Workspace {
        workspace::show(&self.store, id)
            .await
            .expect("the workspace should show")
    }

    pub async fn transcript(&self, id: WorkspaceId) -> Vec<TranscriptEntry> {
        self.walk(id, None, Window::DEFAULT).await
    }

    /// One bounded window is the only read there is, so a whole Transcript is a walk.
    pub async fn walk(
        &self,
        id: WorkspaceId,
        from: Option<Cursor>,
        window: Window,
    ) -> Vec<TranscriptEntry> {
        let mut walked = Vec::new();
        let mut cursor = from;

        loop {
            let page = self
                .page(id, cursor, window)
                .await
                .expect("the transcript should read");
            walked.extend(page.entries);
            cursor = page.cursor;

            if !page.more {
                return walked;
            }
        }
    }

    pub async fn page(
        &self,
        id: WorkspaceId,
        from: Option<Cursor>,
        window: Window,
    ) -> Result<Page, Unreadable> {
        workspace::transcript(&self.store, id, from, window).await
    }

    pub async fn said(&self, session: &Session, message: &str) {
        self.try_said(session, message)
            .await
            .expect("the message should reach the transcript");
    }

    pub async fn try_said(&self, session: &Session, message: &str) -> anyhow::Result<()> {
        let mut tx = self.store.begin().await.expect("a transaction");
        let workspace = tx.workspaces().get(session.workspace).await?;
        tx.log()
            .append(
                &workspace,
                Entry::Said {
                    participant: session.agent.name.clone(),
                    message: message.to_owned(),
                },
            )
            .await?;
        tx.commit().await
    }

    pub async fn post(&self, id: WorkspaceId, participant: &str, message: &str) -> Session {
        self.post_while_busy(id, participant, message)
            .await
            .expect("an idle workspace should enqueue a session")
    }

    pub async fn post_while_busy(
        &self,
        id: WorkspaceId,
        participant: &str,
        message: &str,
    ) -> Option<Session> {
        workspace::post(&self.store, id, participant, message)
            .await
            .expect("the message should post")
    }

    pub async fn has_pending_messages(&self, id: WorkspaceId) -> bool {
        let pool = database(self.data_dir()).await;
        let pending = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS (SELECT 1 FROM pending_message WHERE workspace_id = ?)",
        )
        .bind(id.to_string())
        .fetch_one(&pool)
        .await
        .expect("pending messages should read");

        pool.close().await;
        pending
    }

    pub async fn enqueue_session(&self, workspace: WorkspaceId) -> Session {
        self.try_enqueue_session(workspace)
            .await
            .expect("the session should enqueue")
    }

    /// What `session enqueue` still lets through directly, with nothing ever posted to the
    /// Workspace: a Session its first Turn has no instruction for.
    pub async fn enqueue_session_with_nothing_posted(&self, workspace: WorkspaceId) -> Session {
        work::enqueue(&self.store, workspace, None, None)
            .await
            .expect("the session should enqueue")
    }

    pub async fn try_enqueue_session(&self, workspace: WorkspaceId) -> anyhow::Result<Session> {
        self.instructed(workspace).await?;
        work::enqueue(&self.store, workspace, None, None).await
    }

    pub async fn enqueue_session_as(&self, workspace: WorkspaceId, agent: &str) -> Session {
        self.try_enqueue_session_as(workspace, agent)
            .await
            .expect("the session should enqueue")
    }

    pub async fn try_enqueue_session_as(
        &self,
        workspace: WorkspaceId,
        agent: &str,
    ) -> anyhow::Result<Session> {
        self.instructed(workspace).await?;
        work::enqueue(&self.store, workspace, Some(agent), None).await
    }

    pub async fn enqueue_session_naming(
        &self,
        workspace: WorkspaceId,
        model: Option<&str>,
    ) -> Session {
        self.try_enqueue_session_naming(workspace, model)
            .await
            .expect("the session should enqueue")
    }

    pub async fn try_enqueue_session_naming(
        &self,
        workspace: WorkspaceId,
        model: Option<&str>,
    ) -> anyhow::Result<Session> {
        self.instructed(workspace).await?;
        work::enqueue(&self.store, workspace, None, model).await
    }

    /// What a fixture calling straight into `work::enqueue` skips: the message a real operator
    /// posts to give the Session it starts something to do. Appended before enqueuing, the way an
    /// operator's post always precedes the session it starts, so nothing can claim and start the
    /// Session before it has an instruction to run.
    async fn instructed(&self, workspace: WorkspaceId) -> anyhow::Result<()> {
        let mut tx = self.store.begin().await.expect("a transaction");
        let record = tx
            .workspaces()
            .get(workspace)
            .await
            .expect("the workspace should read");
        tx.log()
            .append(
                &record,
                Entry::Said {
                    participant: "operator".to_owned(),
                    message: "do the work this environment was provisioned for".to_owned(),
                },
            )
            .await?;
        tx.commit().await.expect("the instruction should commit");
        Ok(())
    }

    /// Claims what it enqueued, standing in for the work role a `boot`ed fixture leaves idle.
    pub async fn dispatch_session(&self, workspace: WorkspaceId) -> (Session, Secret) {
        self.enqueue_session(workspace).await;
        let claimed = self
            .claim_session()
            .await
            .expect("a session was just enqueued to claim");

        (claimed.session, claimed.credential)
    }

    pub async fn claim_session(&self) -> Option<Claimed> {
        work::claim(&self.store, &[SERIALIZED.to_owned()])
            .await
            .expect("the claim should ask")
    }

    pub async fn occupy_session(&self) -> Option<Claimed> {
        match work::occupy(&self.store, 2, &[SERIALIZED.to_owned()])
            .await
            .expect("the occupancy should ask")
        {
            Some(work::Occupied::Claimed(claimed)) => Some(claimed),
            Some(work::Occupied::Resumed(_)) => panic!("no session should resume"),
            None => None,
        }
    }

    /// Prompts a waiting Session with what is held for it, the way the work role's sweep does.
    pub async fn prompt_waiting(&self) {
        work::occupy(&self.store, 1, &[SERIALIZED.to_owned()])
            .await
            .expect("the occupancy should ask");
    }

    pub async fn occupy_up_to(&self, slots: usize) -> Option<work::Occupied> {
        work::occupy(&self.store, slots, &[SERIALIZED.to_owned()])
            .await
            .expect("the occupancy should ask")
    }

    pub async fn record_dispatch(&self, slots: usize) {
        let mut tx = self.store.begin().await.expect("a transaction");
        tx.queue()
            .record(slots, &[SERIALIZED.to_owned()])
            .await
            .expect("the dispatch should record");
        tx.commit().await.expect("the record should commit");
    }

    pub async fn queue(&self, organization: &str) -> queue::Snapshot {
        queue::snapshot(&self.store, organization)
            .await
            .expect("the queue should read")
    }

    pub async fn waits_after_its_first_turn(&self, session: &Session) -> Session {
        link::start(&self.store, session)
            .await
            .expect("the session should start");
        work::report(
            &self.store,
            session,
            work::Reported {
                seq: Some(1),
                report: work::Report::Answered,
            },
        )
        .await
        .expect("the answer should be reported");

        self.session(session.id).await
    }

    pub async fn block_session(&self, session: &Session, blocker: &Session) {
        let mut tx = self.store.begin().await.expect("a transaction");
        tx.workspaces()
            .declare_blocked(session, blocker)
            .await
            .expect("the session should be declared blocked");
        tx.commit().await.expect("the declaration should commit");
    }

    pub async fn session(&self, id: SessionId) -> Session {
        work::session(&self.store, id)
            .await
            .expect("the session should show")
    }

    pub async fn sessions(&self, workspace: WorkspaceId) -> Vec<Session> {
        work::sessions(&self.store, workspace)
            .await
            .expect("the sessions should list")
    }

    pub async fn turns(&self, session: SessionId) -> Vec<Turn> {
        work::turns(&self.store, session)
            .await
            .expect("the session's turns should read")
    }

    pub async fn stop_session(&self, session: SessionId) -> Exit {
        self.try_stop_session(session)
            .await
            .expect("the session should stop")
    }

    pub async fn try_stop_session(&self, session: SessionId) -> anyhow::Result<Exit> {
        work::stop(&self.store, session).await
    }

    pub async fn answered(&self, session: SessionId, count: usize) -> Session {
        self.answered_within(session, count, PATIENCE).await
    }

    /// Once `count` of the Session's turns are answered, or once it has ended short of them.
    pub async fn answered_within(
        &self,
        session: SessionId,
        count: usize,
        patience: std::time::Duration,
    ) -> Session {
        let deadline = tokio::time::Instant::now() + patience;

        loop {
            let answered = self
                .turns(session)
                .await
                .iter()
                .filter(|turn| turn.answered_at.is_some())
                .count();
            let session = self.session(session).await;
            if answered >= count || session.state == SessionState::Ended {
                return session;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "the session {} is {} with {answered} of {count} turns answered",
                session.id,
                session.state
            );
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    }

    /// A Session whose first turn is over has ended either way: by that turn, or stopped after it
    /// the way an operator would, because answering never ends one (ADR-0024).
    pub async fn after_one_turn(&self, session: SessionId) -> Session {
        self.after_one_turn_within(session, PATIENCE).await
    }

    pub async fn after_one_turn_within(
        &self,
        session: SessionId,
        patience: std::time::Duration,
    ) -> Session {
        let answered = self.answered_within(session, 1, patience).await;
        if answered.state != SessionState::Ended {
            self.try_stop_session(session)
                .await
                .expect("a waiting session should stop");
        }

        self.session(session).await
    }

    pub async fn complete_session(&self, session: &Session) {
        work::complete(&self.store, session)
            .await
            .expect("the session should end");
    }

    pub async fn fail_session(&self, session: &Session, because: &str) {
        work::fail(&self.store, session, because)
            .await
            .expect("the session should end");
    }

    /// The `ended`/NULL row migration 0003 leaves behind for every Session predating kestrel
    /// scheduling. `end_session` always records an exit, so nothing reachable through the store
    /// produces one.
    pub async fn end_session_without_an_exit(&self, session: &Session) {
        let pool = database(self.data_dir()).await;

        sqlx::query("UPDATE session SET state = 'ended', ended_at = ?, exit = NULL WHERE id = ?")
            .bind(jiff::Timestamp::now().to_string())
            .bind(session.id.to_string())
            .execute(&pool)
            .await
            .expect("the session should end without an exit");

        pool.close().await;
    }

    pub async fn supervised(&self, session: &Session, supervisor: &str) {
        work::supervised(&self.store, session, supervisor)
            .await
            .expect("the supervisor should be recorded");
    }

    pub async fn executes_on(&self, session: &Session, instance: &str) {
        work::executes_on(&self.store, session, instance)
            .await
            .expect("the instance should be recorded");
    }

    pub async fn report_checkout(&self, session: &Session, repositories: Vec<instance::Observed>) {
        work::report(
            &self.store,
            session,
            work::Reported {
                seq: Some(1),
                report: work::Report::Checkout { repositories },
            },
        )
        .await
        .expect("the checkout should be reported");
    }

    pub async fn instances_to_archive(&self) -> Vec<String> {
        instance::to_archive(&self.store)
            .await
            .expect("the instances to archive should read")
    }

    pub async fn instance_archived(&self, instance: &str) {
        instance::archived(&self.store, instance)
            .await
            .expect("the instance should be recorded archived");
    }

    pub async fn supervisors_to_stop(&self) -> Vec<(Session, String)> {
        work::supervisors_to_stop(&self.store)
            .await
            .expect("ended sessions' supervisors should read")
    }

    pub async fn supervisor_gone(&self, session: &Session) {
        work::supervisor_gone(&self.store, session)
            .await
            .expect("the supervisor should be gone");
    }

    /// The claimant records a supervisor gone some time after its container exits, and until
    /// then the Workspace still counts the ended Session as holding it.
    pub async fn supervisor_recorded_gone(&self, session: &Session) {
        let deadline = tokio::time::Instant::now() + PATIENCE;

        loop {
            let mut tx = self.store.begin().await.expect("a transaction");
            let gone = tx
                .workspaces()
                .supervisor_is_gone(session)
                .await
                .expect("the supervisor's state should read");
            drop(tx);
            if gone {
                return;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "the supervisor of session {} was never recorded gone",
                session.id
            );
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    }

    pub async fn instance(&self, workspace: WorkspaceId) -> Option<String> {
        work::instance(&self.store, workspace)
            .await
            .expect("the workspace's instance should read")
    }

    pub async fn instruct(&self, session: &Session, instruction: Instruction) {
        self.try_instruct(session, instruction)
            .await
            .expect("the instruction should send");
    }

    pub async fn try_instruct(
        &self,
        session: &Session,
        instruction: Instruction,
    ) -> anyhow::Result<link::SentInstruction> {
        link::instruct(&self.store, session, instruction).await
    }

    pub async fn start(&self, session: &Session) {
        self.try_start(session)
            .await
            .expect("the session should start");
    }

    pub async fn try_start(&self, session: &Session) -> anyhow::Result<link::SentInstruction> {
        link::start(&self.store, session).await
    }

    /// A lease that is up when the caller says rather than when a real one would be. The only
    /// way to watch a sweep without waiting a whole lease out.
    pub async fn lease_until(&self, session: &Session, expires_at: Timestamp) {
        let mut tx = self.store.begin().await.expect("a transaction");
        tx.workspaces()
            .hold_lease(session, expires_at)
            .await
            .expect("the lease should hold");
        tx.commit().await.expect("the lease should commit");
    }

    /// Backdates when a Workspace was last active, the way `lease_until` backdates a lease: the
    /// only way to watch the idle sweep without waiting the window out.
    pub async fn last_active(&self, workspace: &Workspace, at: Timestamp) {
        let mut tx = self.store.begin().await.expect("a transaction");
        tx.workspaces()
            .record_active(workspace.id, at)
            .await
            .expect("the workspace should record when it was last active");
        tx.commit().await.expect("the record should commit");
    }

    /// The only way to watch the periodic sweep find what a missed event would have.
    pub async fn last_considered(&self, event: EventRecordId, at: Timestamp) {
        let mut tx = self.store.begin().await.expect("a transaction");
        tx.triggers()
            .considered(event, at)
            .await
            .expect("the held firing should record when it was considered");
        tx.commit().await.expect("the record should commit");
    }

    /// A second credential for the same Session, with an expiry the caller chooses. The only way
    /// to hold an expired one without waiting out a real credential's life.
    pub async fn issue_credential(&self, session: &Session, expires_at: Timestamp) -> Secret {
        let secret = Secret::mint();
        let mut tx = self.store.begin().await.expect("a transaction");
        tx.workspaces()
            .issue_credential(session, &secret.digest(), expires_at)
            .await
            .expect("the credential should issue");
        tx.commit().await.expect("the credential should commit");
        secret
    }

    /// Simulates the process going away: every role and every stream it was holding open stops
    /// at once, and the store is dropped rather than closed.
    pub async fn kill(self) -> Stopped {
        self.shutdown.cancel();
        self.roles.abort();
        let _ = self.roles.await;
        drop(self.store);

        Stopped {
            cleanup: self.cleanup,
            data_dir: self.data_dir,
            bound: self.bound,
            environment: self.environment,
        }
    }

    pub async fn kill_and_restart(self) -> Self {
        self.kill().await.restart().await
    }

    /// Stops the way a signalled control plane does: every role is told to stop and is waited
    /// for, rather than being cut off where it stood.
    pub async fn teardown(mut self) -> Stopped {
        self.shutdown.cancel();
        let _ = self.roles.await;
        destroy_instances(
            self.data_dir.path(),
            self.environment
                .as_ref()
                .map(|environment| &environment.driver),
        )
        .await;
        self.cleanup.armed = false;
        drop(self.store);

        Stopped {
            cleanup: self.cleanup,
            data_dir: self.data_dir,
            bound: self.bound,
            environment: self.environment,
        }
    }
}

pub async fn database(data_dir: &Path) -> sqlx::SqlitePool {
    let database = data_dir.join("kestrel.db");
    sqlx::SqlitePool::connect(&format!("sqlite://{}", database.display()))
        .await
        .expect("the database should open")
}

/// An Instance outlives every Session on it and nothing here seals a Workspace into releasing one,
/// so a test's Instances go with the test.
pub(crate) async fn destroy_instances(data_dir: &Path, driver: Option<&Driver>) {
    let Some(driver) = driver else {
        return;
    };
    let pool = database(data_dir).await;
    let mut instances: BTreeSet<String> =
        sqlx::query_scalar("SELECT DISTINCT instance FROM session WHERE instance IS NOT NULL")
            .fetch_all(&pool)
            .await
            .expect("the instances should read")
            .into_iter()
            .collect();
    if matches!(driver, Driver::LocalExec(_)) {
        let sessions: Vec<String> = sqlx::query_scalar("SELECT id FROM session")
            .fetch_all(&pool)
            .await
            .expect("the sessions should read");
        instances.extend(
            sessions
                .into_iter()
                .map(|session| format!("local-exec/kestrel-{session}")),
        );
    }
    pool.close().await;

    for instance in instances {
        if let Err(error) = driver.destroy_named(&instance) {
            eprintln!("failed to destroy {instance}: {error}");
        }
    }
}

pub(crate) fn destroy_instances_on_drop(data_dir: std::path::PathBuf, driver: Option<Driver>) {
    if std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("a cleanup runtime");
        runtime.block_on(destroy_instances(&data_dir, driver.as_ref()));
    })
    .join()
    .is_err()
    {
        eprintln!("the test fixture could not clean up its instances");
    }
}

impl Stopped {
    pub async fn restart(mut self) -> Kestrel {
        self.cleanup.armed = false;
        Kestrel::boot_against(self.data_dir, self.bound, self.environment).await
    }

    /// Restarted with a dispatch configuration the flags carry, which replaces whatever the
    /// work role started with before.
    pub async fn restart_with(
        mut self,
        supervisor: &Path,
        command: &str,
        maximum: usize,
    ) -> Kestrel {
        self.cleanup.armed = false;
        Kestrel::boot_against(
            self.data_dir,
            self.bound,
            Some(Provisions {
                driver: Driver::LocalExec(LocalExec::running(supervisor)),
                harnesses: vec![HarnessCommand {
                    name: HARNESS.to_owned(),
                    command: command.to_owned(),
                }],
                max_active_sessions: NonZeroUsize::new(maximum)
                    .expect("at least one active session"),
            }),
        )
        .await
    }

    pub async fn session(&self, id: SessionId) -> Session {
        let store = Store::open(self.data_dir.path())
            .await
            .expect("the database should still be there");

        work::session(&store, id)
            .await
            .expect("the session should show")
    }

    /// A due time set while nothing is keeping time, so what fires it afterwards is a control
    /// plane that could only have read it back.
    pub async fn lease_until(&self, session: &Session, expires_at: Timestamp) {
        let store = Store::open(self.data_dir.path())
            .await
            .expect("the database should still be there");
        let mut tx = store.begin().await.expect("a transaction");
        tx.workspaces()
            .hold_lease(session, expires_at)
            .await
            .expect("the lease should hold");
        tx.commit().await.expect("the lease should commit");
    }

    /// Reaches the durable record while nothing is serving it, which is the only way to make
    /// an instruction that an Environment provably could not have been handed as it was sent.
    pub async fn instruct(&self, session: &Session, instruction: Instruction) {
        let store = Store::open(self.data_dir.path())
            .await
            .expect("the database should still be there");
        link::instruct(&store, session, instruction)
            .await
            .expect("the instruction should send");
    }
}
