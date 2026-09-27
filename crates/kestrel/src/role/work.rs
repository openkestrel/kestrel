use std::io::{BufRead as _, BufReader, Read};
use std::num::NonZeroUsize;
use std::str::FromStr;
use std::time::Duration;

use anyhow::{Context as _, Result, bail};
use jiff::{SignedDuration, Timestamp};
use tokio::task::{JoinError, JoinSet};
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

use crate::cli::Role;
use crate::compute::{Driver, Exited, Instance, Supervisor};
use crate::domain::{Exit, Session, SessionId, Workspace};
use crate::instance;
use crate::link;
use crate::profile;
use crate::provider;
use crate::store::{self, Store};
use crate::timer;
use crate::work::{self, Claimed, Occupied};
use crate::workspace;

/// Nothing subscribes to `Fanout` at 0.1 (ADR-0005), so a queued Session is found by asking
/// `Store` again rather than by being told.
const POLL: Duration = Duration::from_millis(100);
const LEAVING: SignedDuration = SignedDuration::from_secs(3);

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

/// What ended the attending, rather than how the Session went.
enum Ended {
    Supervisor(Exited),
    TheSession(Exit),
    ControlPlane,
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
                Some(dispatch) => dispatching(&store, dispatch, &shutdown).await,
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

    while !shutdown.is_cancelled() {
        if let Err(error) = stop_left_behind(store, &dispatch.driver).await {
            warn!(%error, "a pass over ended sessions' supervisors found nothing it could do");
        }
        if let Err(error) = archive(store, &dispatch.driver).await {
            warn!(%error, "an archiving pass found nothing it could do");
        }
        match work::occupy(
            store,
            dispatch.max_active_sessions.get(),
            &dispatch.serialized,
        )
        .await
        {
            Ok(Some(Occupied::Claimed(claimed))) => {
                let store = store.clone();
                let dispatch = dispatch.clone();
                let shutdown = shutdown.clone();
                active.spawn(async move {
                    execute_or_fail(&store, &dispatch, claimed, &shutdown).await
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

async fn execute_or_fail(
    store: &Store,
    dispatch: &Dispatch,
    claimed: Claimed,
    shutdown: &CancellationToken,
) {
    let session = claimed.session.clone();
    let Err(error) = execute(store, dispatch, claimed, shutdown).await else {
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
    Claimed {
        session,
        credential,
    }: Claimed,
    shutdown: &CancellationToken,
) -> Result<()> {
    let workspace = match workspace::show(store, session.workspace).await {
        Ok(workspace) => workspace,
        Err(error) => {
            work::fail(
                store,
                &session,
                &format!("the session's workspace could not be read: {error}"),
            )
            .await?;
            return Ok(());
        }
    };

    if let Err(error) = a_way_to_reach_a_model(store, dispatch, &workspace).await {
        work::fail(store, &session, &error.to_string()).await?;
        return Ok(());
    }
    let command = match dispatch.spawns(&session.agent.harness) {
        Ok(command) => command,
        Err(error) => {
            work::fail(store, &session, &error.to_string()).await?;
            return Ok(());
        }
    };

    let Some(mut instance) = instance(store, dispatch, &session, &workspace).await? else {
        return Ok(());
    };
    // An Instance provisioned and never recorded is one `archive` can never find.
    let name = instance.name().to_owned();
    until_not_busy(shutdown, || work::executes_on(store, &session, &name)).await?;

    // Committed before the supervisor is spawned, so one that outlives this process fetches its
    // Start on reconnect rather than holding the lease out forever on a Session that cannot begin.
    if let Err(error) = link::start(store, &session).await {
        work::fail(
            store,
            &session,
            &format!("the session could not be started: {error}"),
        )
        .await?;
        return Ok(());
    }

    let mut supervisor = match instance.supervise(&[
        ("KESTREL_LINK", dispatch.link.as_str()),
        ("KESTREL_SESSION", &session.id.to_string()),
        ("KESTREL_SESSION_CREDENTIAL", credential.as_str()),
        ("KESTREL_HARNESS_COMMAND", command),
        (
            "KESTREL_AGENT_AUTH",
            dispatch.auth.as_deref().unwrap_or_default(),
        ),
        (
            "KESTREL_AGENT_MODEL",
            session.agent.model.as_deref().unwrap_or_default(),
        ),
    ]) {
        Ok(supervisor) => supervisor,
        Err(error) => {
            work::fail(
                store,
                &session,
                &format!(
                    "the supervisor could not be started on the instance {}: {error}",
                    instance.name()
                ),
            )
            .await?;
            return Ok(());
        }
    };
    work::supervised(store, &session, supervisor.name()).await?;
    if let Some(out) = supervisor.take_stdout() {
        relay(session.id, out);
    }
    if let Some(err) = supervisor.take_stderr() {
        relay(session.id, err);
    }

    let exit = start(store, &session, supervisor, shutdown).await?;
    info!(session = %session.id, %exit, "a session ended");

    Ok(())
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
fn relay(session: SessionId, said: impl Read + Send + 'static) {
    std::thread::spawn(move || {
        for line in BufReader::new(said).lines().map_while(Result::ok) {
            info!(session = %session, "{line}");
        }
    });
}

async fn stop_left_behind(store: &Store, driver: &Driver) -> Result<()> {
    for (session, supervisor) in work::supervisors_to_stop(store).await? {
        if session
            .ended_at
            .is_some_and(|ended| Timestamp::now().duration_since(ended) < LEAVING)
        {
            continue;
        }
        match driver.stop_named(&supervisor) {
            Ok(()) => {
                work::supervisor_gone(store, &session).await?;
            }
            Err(error) => {
                warn!(session = %session.id, %error, "an ended session's supervisor resisted being stopped");
            }
        }
    }

    Ok(())
}

async fn archive(store: &Store, driver: &Driver) -> Result<()> {
    for instance in instance::to_archive(store).await? {
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

async fn start(
    store: &Store,
    session: &Session,
    mut supervisor: Supervisor,
    shutdown: &CancellationToken,
) -> Result<Exit> {
    info!(session = %session.id, supervisor = supervisor.name(), "a session's supervisor started");

    let exit = match attend(store, session, &mut supervisor, shutdown).await {
        Ended::TheSession(exit) => {
            left_the_link(&mut supervisor).await;
            exit
        }
        Ended::Supervisor(exited) => {
            let unreported =
                format!("the supervisor exited {exited} without reporting how the session went");
            work::fail(store, session, &unreported).await?
        }
        Ended::ControlPlane => {
            work::fail(
                store,
                session,
                "the control plane stopped while this session was in flight",
            )
            .await?
        }
    };

    match supervisor.stop() {
        Ok(()) => {
            work::supervisor_gone(store, session).await?;
        }
        Err(error) => warn!(session = %session.id, %error, "a supervisor resisted being stopped"),
    }

    Ok(exit)
}

/// A Session stopped or sealed tells its supervisor to leave, and one that does closes its agent
/// conversation on the way out; killed first, it would leave the agent's own process group
/// running.
async fn left_the_link(supervisor: &mut Supervisor) {
    let deadline = tokio::time::Instant::now() + LEAVING.unsigned_abs();

    while tokio::time::Instant::now() < deadline {
        if !matches!(supervisor.status(), Ok(None)) {
            return;
        }
        tokio::time::sleep(POLL).await;
    }
}

/// The supervisor reports its own outcome over the link, so what this waits for is the
/// supervisor being gone. It stops for a Session that ended some other way too — a lease the
/// supervisor stopped holding out — because a supervisor that outlives its Session would otherwise
/// hold this role's one dispatch forever.
async fn attend(
    store: &Store,
    session: &Session,
    supervisor: &mut Supervisor,
    shutdown: &CancellationToken,
) -> Ended {
    loop {
        match supervisor.status() {
            Ok(Some(exited)) => return Ended::Supervisor(exited),
            Ok(None) => {}
            // A daemon that cannot answer is not a supervisor that is gone. The Session's lease
            // ends it if this never clears.
            Err(error) => {
                warn!(session = %session.id, %error, "a supervisor could not be asked how it is")
            }
        }
        match work::session(store, session.id).await {
            Ok(Session {
                exit: Some(exit), ..
            }) => return Ended::TheSession(exit),
            Ok(_) => {}
            Err(error) => {
                warn!(session = %session.id, %error, "a session could not be asked how it is")
            }
        }

        tokio::select! {
            () = tokio::time::sleep(POLL) => {}
            () = shutdown.cancelled() => return Ended::ControlPlane,
        }
    }
}
