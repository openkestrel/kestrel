//! The primary test seam (0.1/03): boot a complete control plane in-process against a fresh
//! temporary SQLite file, drive it through the same paths a person would use, and tear it
//! down. Assertions live in the language of Workspaces, Sessions and Transcripts; `Store` and `Log`
//! stay behind `Kestrel`, never reached for directly.

// `suite` and every heavy suite beside it compile all of this; a helper one of them does not
// reach for is not dead, it belongs to a sibling.
#![allow(dead_code)]

pub mod built;
pub mod client;
pub mod compose;
pub mod control_plane;
pub mod database;
pub mod diagnostics;
pub mod docker;
mod drive {
    mod boot;
    mod credentials;
    mod declarations;
    mod integrations;
    mod link;
    mod sessions;
    mod transcript;
    mod triggers;
    mod workspaces;
}
pub mod environment;
pub mod fixture;
pub mod git;
pub mod github_stub;
pub mod image;
pub mod images;
pub mod lineage;
pub mod link_client;
pub mod model;
pub mod operator_log;
pub mod provider_stub;
pub mod repository;
pub mod scripted_agent;
pub mod supervisor;

pub use database::Database;

/// The crate's checkout root, resolved at run time so that a binary compiled in one
/// worktree still finds the right files when `cargo test` runs it from another.
pub fn crate_root() -> std::path::PathBuf {
    std::env::var_os("CARGO_MANIFEST_DIR").map_or_else(
        || std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")),
        std::path::PathBuf::from,
    )
}

use std::net::SocketAddr;
use std::num::NonZeroUsize;
use std::path::Path;
use std::time::Duration;

use jiff::Timestamp;
use kestrel::capability::Images;
use kestrel::compute::{Driver, LocalExec};
use kestrel::declaration::{self};
use kestrel::domain::{
    Agent, Correlation, CorrelationMiss, Declared, Fires, OnOpenWorkspace, Session, SessionId,
    Templates, Trigger, WorkspaceId,
};
use kestrel::link::credential::Secret;
use kestrel::link::{self, Instruction};
use kestrel::role::serve::Listen;
use kestrel::role::work::HarnessCommand;
use kestrel::sign_in::check::Providers;
use kestrel::store::Store;
use kestrel::work;
use tempfile::TempDir;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

const PATIENCE: std::time::Duration = std::time::Duration::from_secs(30);

/// The GitHub App credential every fixture registers an Integration with. The key is
/// distinctive enough that a test can assert it is nowhere it should not be.
pub const APP_ID: i64 = 1;
pub const INSTALLATION_ID: i64 = 2;
pub const PRIVATE_KEY: &str = "-----BEGIN RSA PRIVATE KEY-----
MIIEpAIBAAKCAQEA+v3GAQtFnP1Q7mAAhcSSrPOcbvvtEqTOD4TnrRg7gI1EHVbI
0BdCwuR5swqewUyabBxfG1DFInwMiS5AqhP1NSsA7Mdf3BDAj6MABuW8mjUEnkyL
RD+84cVy7Wgl56hfbrSAQ/BYhRNP48Rg14t0LeNMiofhLt+A4KQhb7CLAXs8QNgR
l8L0iSvGgpQhjeHCFaqgm5Ez+iGnm22Xe1EmHd1MgdVyXVII1sJSC5iDmOjg2JAB
eGS7i74/a1mjgC6FffsPPQ6c0FfVjinfFV5Lh65lfpV/dNJTEna10WY8eFXq9YPe
vE1oxRTbI8/cPYPUEhWSk0WzJTf9QJCd2ywZcwIDAQABAoIBABqxCTEMO1N2Jviq
sfOH05db8ttkpDIS360L0713McC6hS9fjsx+ZMWoGX8eVoK0/EkZ/U1A2i4QJh8i
kZLku3Nzhs4nZ7zV9TxxN6wEG3oEwGB7QGY6JwKVhmLT7BAGOlviCy/ua+N/CcRt
mmThujG1Fes3hw1+jEyXd8zSZ/34oKXStPIV/yzZ6rIL8QlqraBRSEgBR5XeS2pb
dZQ5x0blEfnMFiesb6gLO101DajYdq2RB2ClnkDuYXnejQ5FN7CYCmMDUaaW9BFs
SmqhylXU6w265UMqcqlRMqi0ynkSeYqdRCth/1n46918/Nj85kiNfOHpkdrrSqPa
dnLd5KkCgYEA/4zxXfEieP54VDcigBFtr1QYbGvo2jHe0waedsKyhELeORcDfoAG
Ym8RzpmRZZ3okEKULEBkxJ4gIYZwHIyKVPcPsy4ovO8COCiJyH3CBiU6aDc4BSXd
Cnugq/FOA409Hbh1MAfnpG1Ec6ksdVkHP85TSAW4/OxJrXt+Alh2GdkCgYEA+27H
I7npu+HvUSoMyA80w9a2o0nO5aZ49lBF0/ISqrhBCC6riIHUrtqgWuCz6Cd6gEyI
E2FcEoezhCnt/ibj3XKGYq+usqYVLpnc2RaVYpTk+saMu6t6HGQ5rG8cRksrgyEm
A85XmS9FWEaKQ+gdLXR93OUQa8lV1ksjh5JYkisCgYB0eRqxhGv/1YumZpK06Dyj
wqJ3GB/DAyPZ7pRe8JJIsely/8g56cP/LTxie5/HPtd2mMHMAXVsH93pKcsDWBLc
ctaXBhzPL+hFDETwOlEqV+FtCm3f+sfqNTqf45V5DPCkESRoRdIQITUnuh1nRrKF
JFbU5rbPoCpchYKDMypw0QKBgQDI9LU9HK5veBjqag1lKKInlEjNIYS3j+JGG8pz
p5q70CBKBeMba4ktZ5eaitW/yy3htUldFAee187m5wxMgZDFwW8fGQMrzgpIALm+
2In1DMQmmuKIbHlgj8p4wyV6GUJhv/C+QgFJcbqe32JK/A218tFV587EF1r/t3Zi
ONmJmQKBgQDtThzguDibmNa0Ebctytq1BMjEt8FnGe32gEba28GdgZZjM47hca0B
Ax8ECK4JhZG/0uoJtjPuII1U8vS261Xmeqg72AxcC85BLTnQawUjmboq9gYauuoa
1IRY80HLvWrZU/GZ13KF5CQyX8mSTDWs0Nmz8mNTkTl4QAYyizpRTg==
-----END RSA PRIVATE KEY-----
";

pub const REPLACEMENT_PRIVATE_KEY: &str = "-----BEGIN RSA PRIVATE KEY-----
MIIEpAIBAAKCAQEAmZ/iLSN68HeXC/Jsh1Fnc5qdG8mYttMu4orK30+Un0peGdHp
l757gHyNtyzkVl22ojm+kpeGNIBxGCh6supcu+GPJMihNbZA+DEteHYfaA16uLVD
BaROR32B3rMCqCJOvU9ocE83q2jGMfK8GGZvQDG6qnQkkpsZzRhTR9j6T47gKRhE
OP8QxYcYBHPtAyGfnpQcdu2L7DVLS2PUjK9hY+WBHmd1KfHlwKRy+aXN3DY+0r/C
1x0kthU1JFhIfkYPmvw2g8QizLZcys6sBUnLLbt2lw6Snppn4q+FVX5fZH/5jjxA
ad8oz0dXfANL79hlb/+gbvEw9uHwZGzJhDFc5wIDAQABAoIBABIk+KVFLgWN0mjG
UrwuyK22Qr0f8+oQClcWdnymnRKtSWz6RoE2LccQrvpGEpgCzInfkRsmS2MTEoh8
a3WLSbWlH++DxMqfwo+t20WBihcTlH2RZx+Y9ufpsneXpAYwuyUP5nFxYc1cfCA1
H7MwEg6+cOImApXQPN1k9J0lSvcvZYAPHQbDhZIHNJ7Kpz3e1kA2SvSdwm5ibXak
O45ceO6vddauGrB8SGPBn8esA8Wm0sC/j1jqzK+DY7OWlBxTk/EshLLdWyPFoSQP
UrpeU0kNIYVpwPfkERmWZLPh1YYBoS0iBMgwzbpPDGzjO8HufD0HWUpL7ezRy5tG
v2JE/30CgYEAyK7btHcSei1nLUsJerFCjRSbBiSWpVt38UHhd0lsAL/F+ykyLheM
7Eo2uHovikBgdGHuEInLuAUu/R5lYzOTqD4io+dbDOJhw5pIBBrwdgzDqHnQWCGh
52JsX1tX3ZZNEbCHr6h8mNARnQb5csbWn18hdECQohuJj9SOLiIwCfMCgYEAw/hb
HjTEk140f/LnYMHi56RUoI1quemPsyIK8sxdk/cB0rcVL05bONyjwHDzJhovxLu3
+q6+5GM967mbhvzjC4LeTmsyRG2SKpJB3xuLALbsPmlVoaMvZzH6rR66S8UQ0XNW
Nil+zzIAJIYVDh7toHHzPvYyHdKB60iRxgeWij0CgYAYqyXmsRxcWGhucOLhahi8
LuQ/hQ/fk7bSuT5141ibXvd/QDki4VI5WYYQdzSQ0gkJRWB2N/oKzNhR1QKxhw2H
iNvJOg8/LjTGTpi7qHQWjsseUpsBumCkiLy5nAjY/DjNC94iAZwPTMRdgRX4vehz
/sWl/KnKw4fNXlt74Ef3cQKBgQCXgvOHpgp5WXNDnRxQjXaN21pmiw+Bz1b/iwll
8KVE2cs6C7zAR1d13fhot6r1wXc/Jlc25FXIJAZisqzRL8GmS7bucCh8CS7h5RQy
l4vfsI10L2I6Rv1GTrjdkg08uY6oMQDtfSWCsci0dEnhEf5sds0214el0PQLYZ76
uTEljQKBgQCKhaw3sri9CF7NludpO0LPiVG4Ty/tMlZ4eFE417GTGMXNGUeraLlR
ftEri1WqDbF5EK0BFURQsZUNoxpghqVvehZydrWnvx+4RAZvNye+iWqFNoMz9ZgI
/Qu2+51lho6V1/PPrmH6cCwNlHST+vsR+mhVhqSUTKMbvQJG3pSYmA==
-----END RSA PRIVATE KEY-----
";

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

fn client_url() -> url::Url {
    kestrel::cli::CLIENT_URL
        .parse()
        .expect("the default Client URL")
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
    follow_lease: Duration,
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

pub const DEFAULT_INTERRUPT_DEADLINE: Duration = Duration::from_secs(30);
/// Short, so a test of a provider that never answers does not wait the ten seconds a real one gets.
pub const PROVIDER_DEADLINE: Duration = Duration::from_secs(2);

/// Refuses every connection, so a test that saves a key without a stub never reaches a provider.
fn no_provider() -> Providers {
    Providers::at(
        "http://127.0.0.1:1",
        "http://127.0.0.1:1",
        PROVIDER_DEADLINE,
    )
}
/// Short, so a Session that answered is waiting soon after, unless a test watches it trail.
pub const QUIET_PERIOD: Duration = Duration::from_secs(1);

/// What the work role provisions an Environment with.
#[derive(Clone)]
pub struct Provisions {
    driver: Driver,
    harnesses: Vec<HarnessCommand>,
    max_active_sessions: NonZeroUsize,
    interrupt_deadline: Duration,
    sign_in_test_deadline: Duration,
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

/// An Instance a test stands in for the work role on: the Session is recorded executing on it, and
/// a supervisor's credential is issued for it.
pub struct OnTheLink {
    pub instance: String,
    pub credential: Secret,
    pub provisioned_by: SessionId,
}

/// What a Session's `start` has the supervisor spawn when a test does not say.
pub fn harness() -> link::Harness {
    harness_playing(scripted_agent::Script::Speaks)
}

pub fn harness_playing(script: scripted_agent::Script) -> link::Harness {
    link::Harness {
        command: scripted_agent::playing(script),
        auth: None,
        model: None,
        mode: None,
        thought_level: None,
    }
}

pub struct Consideration {
    pub outcome: String,
    pub candidates: Vec<(WorkspaceId, String)>,
}

/// Comes back on the address it was listening on, so what an Environment already dialled
/// still reaches it.
pub struct Stopped {
    cleanup: Cleanup,
    data_dir: TempDir,
    bound: Listen,
    environment: Option<Provisions>,
    follow_lease: Duration,
}

impl Kestrel {
    pub fn data_dir(&self) -> &Path {
        self.data_dir.path()
    }

    pub fn is_running(&self) -> bool {
        !self.roles.is_finished()
    }

    pub fn database(&self) -> Database<'_> {
        Database::at(self.data_dir())
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
            follow_lease: self.follow_lease,
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
            follow_lease: self.follow_lease,
        }
    }
}

/// An Instance outlives every Session on it and nothing here seals a Workspace into releasing one,
/// so a test's Instances go with the test.
pub(crate) async fn destroy_instances(data_dir: &Path, driver: Option<&Driver>) {
    let Some(driver) = driver else {
        return;
    };
    let database = Database::at(data_dir);
    let mut instances = database.instances().await;
    if matches!(driver, Driver::LocalExec(_)) {
        instances.extend(
            database
                .sessions()
                .await
                .into_iter()
                .map(|session| format!("local-exec/kestrel-{session}")),
        );
    }

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
    pub fn database(&self) -> Database<'_> {
        Database::at(self.data_dir.path())
    }

    pub async fn restart(mut self) -> Kestrel {
        self.cleanup.armed = false;
        Kestrel::boot_against(
            self.data_dir,
            self.bound,
            self.environment,
            self.follow_lease,
        )
        .await
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
                interrupt_deadline: DEFAULT_INTERRUPT_DEADLINE,
                sign_in_test_deadline: kestrel::sign_in::model_test::DEADLINE,
            }),
            self.follow_lease,
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

struct Declaration<'a> {
    organization: &'a str,
    name: &'a str,
    fires: &'a Fires,
    templates: &'a Templates,
    project: &'a str,
    agent: &'a str,
    declared: &'a Declared,
    allows: &'a [String],
    profile: Option<&'a str>,
}

async fn placed_trigger(store: &Store, declaring: Declaration<'_>) -> anyhow::Result<Trigger> {
    let checked = declaration::check_parsed_trigger(declaration::TriggerProposal {
        name: declaring.name.to_owned(),
        fires: declaring.fires.clone(),
        templates: declaring.templates.clone(),
        project: declaring.project.to_owned(),
        agent: declaring.agent.to_owned(),
        declared: declaring.declared.clone(),
        allows: declaring.allows.to_vec(),
        profile: declaring.profile.map(str::to_owned),
    })?;
    let mut tx = store.begin().await?;
    let organization = tx.organizations().named(declaring.organization).await?;
    let placed = declaration::place_trigger(
        &mut tx,
        &organization,
        &checked,
        declaration::Provenance::Flags,
    )
    .await?;
    tx.commit().await?;

    Ok(placed.trigger)
}

async fn placed_agent(
    store: &Store,
    organization: &str,
    name: &str,
    harness: &str,
    declared: &Declared,
) -> anyhow::Result<Agent> {
    let checked =
        declaration::check_agent(&Images::default(), name, harness, declared, "declare_agent")
            .await
            .map_err(|error| {
                declaration::scoped(declaration::Prefix::Root, "declare_agent", error)
            })?;
    let mut tx = store.begin().await?;
    let organization = tx.organizations().named(organization).await?;
    let (declared, _) = declaration::place_agent(
        &mut tx,
        &organization,
        &checked,
        declaration::OnExisting::Change,
    )
    .await?;
    tx.commit().await?;

    Ok(declared.record)
}
