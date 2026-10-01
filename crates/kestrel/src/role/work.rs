use std::collections::HashMap;
use std::io::{BufRead as _, BufReader, Read};
use std::num::NonZeroUsize;
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context as _, Result, bail};
use jiff::Timestamp;
use tokio::task::{JoinError, JoinSet};
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

use crate::cli::Role;
use crate::compute::{Driver, Exited, Instance, Supervisor};
use crate::domain::{Session, SessionState, Workspace};
use crate::instance;
use crate::link::{self, credential::Secret};
use crate::profile;
use crate::provider;
use crate::store::{self, Store};
use crate::timer;
use crate::work::{self, Occupied};
use crate::workspace;

/// Nothing subscribes to `Fanout` at 0.1 (ADR-0005), so a queued Session is found by asking
/// `Store` again rather than by being told.
const POLL: Duration = Duration::from_millis(100);

#[derive(Clone)]
pub struct Dispatch {
    pub link: String,
    pub driver: Driver,
    pub harnesses: Vec<HarnessCommand>,
    pub auth: Option<String>,
    pub max_active_sessions: NonZeroUsize,
    pub serialized: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HarnessCommand {
    pub name: String,
    pub command: String,
}

impl FromStr for HarnessCommand {
    type Err = anyhow::Error;

    fn from_str(given: &str) -> Result<Self> {
        match given.split_once('=') {
            Some((name, command)) if !name.is_empty() && !command.trim().is_empty() => Ok(Self {
                name: name.to_owned(),
                command: command.to_owned(),
            }),
            _ => bail!("{given} is not NAME=COMMAND"),
        }
    }
}

impl Dispatch {
    fn spawns(&self, harness: &str) -> Result<&str> {
        self.harnesses
            .iter()
            .find(|spawned| spawned.name == harness)
            .map(|spawned| spawned.command.as_str())
            .with_context(|| format!("this work role spawns no harness named {harness}"))
    }

    fn logs_the_agent_in(&self) -> bool {
        self.auth
            .as_deref()
            .is_some_and(|method| !method.is_empty())
    }
}

enum Held {
    Running(String),
    Gone,
    Nothing,
}

/// The supervisors this process started, by Instance. A restart starts with none held, and finds
/// the ones still running by what they last reported.
#[derive(Clone, Default)]
struct Supervisors(Arc<Mutex<HashMap<String, Supervisor>>>);

impl Supervisors {
    fn held(&self) -> std::sync::MutexGuard<'_, HashMap<String, Supervisor>> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn held_on(&self, instance: &str) -> Held {
        let mut held = self.held();
        let Some(supervisor) = held.get_mut(instance) else {
            return Held::Nothing;
        };
        if matches!(supervisor.status(), Ok(None)) {
            return Held::Running(supervisor.name().to_owned());
        }
        held.remove(instance);

        Held::Gone
    }

    fn exited(&self) -> Vec<(String, Exited)> {
        let mut held = self.held();
        let exited: Vec<_> = held
            .iter_mut()
            .filter_map(|(instance, supervisor)| match supervisor.status() {
                Ok(Some(exited)) => Some((instance.clone(), exited)),
                Ok(None) => None,
                // Reaped by something else: gone, and what it exited with is not known here.
                Err(_) => Some((instance.clone(), Exited::without_a_code())),
            })
            .collect();
        for (instance, _) in &exited {
            held.remove(instance);
        }

        exited
    }
}

/// The wheel keeps time whether or not this role has anywhere to dispatch a Session, because a
/// lease left unswept wedges a Workspace no matter who was going to execute it.
pub async fn run(
    store: Store,
    dispatch: Option<Dispatch>,
    wake: timer::Wake,
    shutdown: CancellationToken,
) -> Result<()> {
    info!(role = %Role::Work, "role started");

    tokio::try_join!(
        timer::sweeping(&store, &wake, &shutdown),
        // A work role with nowhere to run a Session claims none: claiming one it cannot dispatch
        // would spend the Session's one dispatch on nothing.
        async {
            match &dispatch {
                Some(dispatch) => {
                    record(&store, dispatch).await?;
                    dispatching(&store, dispatch, &shutdown).await
                }
                None => {
                    shutdown.cancelled().await;
                    Ok(())
                }
            }
        },
    )?;

    info!(role = %Role::Work, "role stopped");
    Ok(())
}

/// A pass that fails is warned of and the next one tries again: nothing here is a reason to stop
/// the role, because stopping it cuts every in-flight Session's supervisor off its link.
async fn dispatching(
    store: &Store,
    dispatch: &Dispatch,
    shutdown: &CancellationToken,
) -> Result<()> {
    let mut active = JoinSet::new();
    let supervisors = Supervisors::default();

    while !shutdown.is_cancelled() {
        if let Err(error) = watch(store, &supervisors).await {
            warn!(%error, "a pass over instances' supervisors found nothing it could do");
        }
        if let Err(error) = archive(store, &dispatch.driver, &supervisors).await {
            warn!(%error, "an archiving pass found nothing it could do");
        }
        match work::occupy(
            store,
            dispatch.max_active_sessions.get(),
            &dispatch.serialized,
        )
        .await
        {
            Ok(Some(Occupied::Claimed(session))) => {
                let store = store.clone();
                let dispatch = dispatch.clone();
                let supervisors = supervisors.clone();
                let shutdown = shutdown.clone();
                active.spawn(async move {
                    execute_or_fail(&store, &dispatch, &supervisors, session, &shutdown).await
                });
                continue;
            }
            Ok(Some(Occupied::Resumed(session))) => {
                info!(session = %session.id, "a waiting session was prompted with what was held for it");
                continue;
            }
            Ok(None) => {}
            Err(error) => warn!(%error, "a dispatch found nothing it could do"),
        }

        tokio::select! {
            Some(finished) = active.join_next(), if !active.is_empty() => warn_if_it_panicked(finished),
            () = tokio::time::sleep(POLL) => {}
            () = shutdown.cancelled() => {}
        }
    }

    while let Some(finished) = active.join_next().await {
        warn_if_it_panicked(finished);
    }

    Ok(())
}

fn warn_if_it_panicked(finished: Result<(), JoinError>) {
    if let Err(error) = finished {
        warn!(%error, "a session's execution task ended without ending its session");
    }
}

/// The queue reads this record rather than the serve role's flags, so restart with new flags
/// is what replaces it.
async fn record(store: &Store, dispatch: &Dispatch) -> Result<()> {
    let mut tx = store.begin().await?;
    tx.queue()
        .record(
            dispatch.max_active_sessions.get(),
            &dispatch.serialized,
            dispatch.driver.name(),
        )
        .await?;
    tx.commit().await?;

    Ok(())
}

async fn execute_or_fail(
    store: &Store,
    dispatch: &Dispatch,
    supervisors: &Supervisors,
    session: Session,
    shutdown: &CancellationToken,
) {
    let Err(error) = execute(store, dispatch, supervisors, &session, shutdown).await else {
        return;
    };
    warn!(session = %session.id, %error, "a session's execution failed");
    let because = format!("the session's execution failed: {error:#}");

    // The Session's lease ends it instead.
    if let Err(error) = until_not_busy(shutdown, || work::fail(store, &session, &because)).await {
        warn!(session = %session.id, %error, "a session whose execution failed could not be failed");
    }
}

async fn until_not_busy<T, Doing>(
    shutdown: &CancellationToken,
    doing: impl Fn() -> Doing,
) -> Result<T>
where
    Doing: Future<Output = Result<T>>,
{
    loop {
        match doing().await {
            Err(error) if store::busy(&error) && !shutdown.is_cancelled() => {
                tokio::time::sleep(POLL).await;
            }
            done => return done,
        }
    }
}

async fn execute(
    store: &Store,
    dispatch: &Dispatch,
    supervisors: &Supervisors,
    session: &Session,
    shutdown: &CancellationToken,
) -> Result<()> {
    let workspace = match workspace::show(store, session.workspace).await {
        Ok(workspace) => workspace,
        Err(error) => {
            work::fail(
                store,
                session,
                &format!("the session's workspace could not be read: {error}"),
            )
            .await?;
            return Ok(());
        }
    };

    if let Err(error) = a_way_to_reach_a_model(store, dispatch, &workspace).await {
        work::fail(store, session, &error.to_string()).await?;
        return Ok(());
    }
    let command = match dispatch.spawns(&session.agent.harness) {
        Ok(command) => command,
        Err(error) => {
            work::fail(store, session, &error.to_string()).await?;
            return Ok(());
        }
    };

    let Some(mut instance) = instance(store, dispatch, session, &workspace).await? else {
        return Ok(());
    };
    // An Instance provisioned and never recorded is one `archive` can never find.
    let name = instance.name().to_owned();
    until_not_busy(shutdown, || work::executes_on(store, session, &name)).await?;

    // Committed before any supervisor is started, so one that outlives this process still finds
    // it rather than holding the lease out forever on a Session that cannot begin.
    let harness = link::Harness {
        command: command.to_owned(),
        auth: dispatch.auth.clone().filter(|method| !method.is_empty()),
        model: session.agent.declared.model.clone(),
        mode: session.agent.declared.mode.clone(),
        thought_level: session.agent.declared.thought_level.clone(),
    };
    let opened = match session.state {
        SessionState::Unbriefed => link::unbriefed(store, session, harness).await,
        _ => link::start(store, session, harness).await,
    };
    let started = match opened {
        Ok(started) => started,
        Err(error) => {
            work::fail(
                store,
                session,
                &format!("the session could not be started: {error}"),
            )
            .await?;
            return Ok(());
        }
    };

    let supervisor = match supervised(
        store,
        dispatch,
        supervisors,
        &workspace,
        &mut instance,
        started.seq - 1,
    )
    .await
    {
        Ok(supervisor) => supervisor,
        Err(error) => {
            work::fail(
                store,
                session,
                &format!("the supervisor could not be started on the instance {name}: {error:#}"),
            )
            .await?;
            return Ok(());
        }
    };
    work::supervised(store, session, &name, supervisor.as_deref()).await?;
    info!(session = %session.id, instance = name, "a session was started on its instance's supervisor");

    Ok(())
}

/// A recorded supervisor off the link is stopped before another starts, so two never share the
/// checkout; the new one reads the stream from `after`, so it never replays an earlier Session.
async fn supervised(
    store: &Store,
    dispatch: &Dispatch,
    supervisors: &Supervisors,
    workspace: &Workspace,
    instance: &mut Instance,
    after: i64,
) -> Result<Option<String>> {
    let name = instance.name().to_owned();
    let held = supervisors.held_on(&name);
    if let Held::Running(running) = held {
        return Ok(Some(running));
    }
    let recorded = store.read().await?.workspaces().supervisor(&name).await?;
    if let Some(recorded) = recorded {
        if matches!(held, Held::Nothing)
            && recorded
                .reached_at
                .is_some_and(|reached| Timestamp::now().duration_since(reached) < link::ON_THE_LINK)
        {
            return Ok(recorded.name);
        }
        if let Some(supervisor) = &recorded.name {
            dispatch.driver.stop_named(supervisor).with_context(|| {
                format!("the supervisor {supervisor}, off the link, could not be stopped")
            })?;
        }
    }

    let credential = Secret::mint();
    let mut tx = store.begin().await?;
    tx.workspaces()
        .start_supervisor(workspace, &name, &credential.digest())
        .await?;
    tx.commit().await?;

    let lease = work::LEASE.as_secs().to_string();
    let mut supervisor = instance.supervise(&[
        ("KESTREL_LINK", dispatch.link.as_str()),
        ("KESTREL_INSTANCE", &name),
        ("KESTREL_INSTANCE_CREDENTIAL", credential.as_str()),
        ("KESTREL_INSTRUCTIONS_AFTER", &after.to_string()),
        // How long a Session's lease is held out for, so a supervisor nothing answers can let its
        // Session go once the lease has certainly lapsed.
        ("KESTREL_LEASE", &lease),
    ])?;
    let mut tx = store.begin().await?;
    tx.workspaces()
        .name_supervisor(&name, &credential.digest(), supervisor.name())
        .await?;
    tx.commit().await?;
    info!(
        instance = name,
        supervisor = supervisor.name(),
        "an instance's supervisor started"
    );

    if let Some(out) = supervisor.take_stdout() {
        relay(&name, out);
    }
    if let Some(err) = supervisor.take_stderr() {
        relay(&name, err);
    }
    let started = supervisor.name().to_owned();
    supervisors.held().insert(name, supervisor);

    Ok(Some(started))
}

/// The Workspace's own Instance, or a fresh one for a Workspace that has none. `None` once the
/// Session has been ended for want of one.
async fn instance(
    store: &Store,
    dispatch: &Dispatch,
    session: &Session,
    workspace: &Workspace,
) -> Result<Option<Instance>> {
    let Some(kept) = work::instance(store, workspace.id).await? else {
        return match dispatch.driver.provision(session.id) {
            Ok(instance) => Ok(Some(instance)),
            Err(error) => {
                work::fail(
                    store,
                    session,
                    &format!("the instance could not be provisioned: {error}"),
                )
                .await?;
                Ok(None)
            }
        };
    };

    match dispatch.driver.resume(&kept) {
        Ok(Some(instance)) => Ok(Some(instance)),
        Ok(None) => {
            let because = format!(
                "the instance {kept} this workspace's work was on is gone, and whatever it held \
                 that was never pushed went with it; the workspace's next session starts on a fresh \
                 instance from the branch {} as the remote has it",
                workspace.checkout.branch
            );
            work::instance_lost(store, session, &because).await?;
            Ok(None)
        }
        // Not forgotten: a daemon that cannot answer has not lost what the Instance holds.
        Err(error) => {
            work::fail(
                store,
                session,
                &format!("the instance {kept} could not be resumed: {error}"),
            )
            .await?;
            Ok(None)
        }
    }
}

/// A supervisor blocks once a pipe nobody reads is full, so what it says is read as it says it.
fn relay(instance: &str, said: impl Read + Send + 'static) {
    let instance = instance.to_owned();
    std::thread::spawn(move || {
        for line in BufReader::new(said).lines().map_while(Result::ok) {
            info!(instance, "{line}");
        }
    });
}

/// A supervisor that exits takes the Session it was carrying with it, since nothing is left to
/// report how that Session went.
async fn watch(store: &Store, supervisors: &Supervisors) -> Result<()> {
    for (instance, exited) in supervisors.exited() {
        warn!(instance, %exited, "an instance's supervisor exited");
        work::supervisor_exited(
            store,
            &instance,
            &format!("the supervisor exited {exited} without reporting how the session went"),
        )
        .await?;
    }

    Ok(())
}

async fn archive(store: &Store, driver: &Driver, supervisors: &Supervisors) -> Result<()> {
    for instance in instance::to_archive(store).await? {
        let held = supervisors.held().remove(&instance);
        if let Some(supervisor) = held
            && let Err(error) = supervisor.stop()
        {
            warn!(instance, %error, "an instance's supervisor resisted being stopped");
        }
        match driver.destroy_named(&instance) {
            Ok(()) => {
                instance::archived(store, &instance).await?;
                info!(instance, "an instance was archived");
            }
            Err(error) => warn!(instance, %error, "an instance resisted being archived"),
        }
    }

    Ok(())
}

/// A Harness reaches a model with the Workspace's Subscription Profile, a Provider
/// Credential its Organization holds, or an ACP login kestrel was configured with. A Session with
/// none of them fails here rather than inside an Instance provisioned to find that out.
async fn a_way_to_reach_a_model(
    store: &Store,
    dispatch: &Dispatch,
    workspace: &Workspace,
) -> Result<()> {
    if let Some(named) = &workspace.profile {
        if profile::holds_anything(store, named).await? {
            return Ok(());
        }
        bail!(
            "the subscription profile {} this workspace names holds no login",
            named.name
        );
    }
    if dispatch.logs_the_agent_in() || provider::holds_any(store, workspace.organization.id).await?
    {
        return Ok(());
    }

    bail!(
        "the organization {} holds no provider credential, and this session's harness was \
         given no other way to reach a model",
        workspace.organization.name
    )
}
