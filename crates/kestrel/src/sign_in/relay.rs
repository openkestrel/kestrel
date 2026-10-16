mod claude;
mod codex;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Result, bail};
use jiff::Timestamp;
use kestrel_operator_types as wire;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::{Authentication, Holding, SignIn, Source, State, filled};
use crate::capability::Images;
use crate::catalogue;
use crate::compute::Driver;
use crate::declined::{Constraint, Next, Reason, Resource, Step};
use crate::domain::{OrganizationId, SubscriptionProfileId};
use crate::profile::Entry;
use crate::store::{self, Store, Tx};

pub const START: &str = "start_sign_in_relay";
pub const READ: &str = "read_sign_in_relay";
pub const CODE: &str = "submit_sign_in_relay_code";
pub const CANCEL: &str = "cancel_sign_in_relay";
pub const DEADLINE: Duration = Duration::from_secs(600);
const POLL: Duration = Duration::from_millis(100);

#[derive(Clone, Default)]
pub(crate) struct Inputs(Arc<Mutex<HashMap<String, mpsc::Sender<String>>>>);

impl Inputs {
    fn insert(&self, id: &str) -> mpsc::Receiver<String> {
        let (send, receive) = mpsc::channel(1);
        self.0.lock().unwrap().insert(id.to_owned(), send);
        receive
    }

    fn remove(&self, id: &str) {
        self.0.lock().unwrap().remove(id);
    }
}

pub struct Attempt {
    pub organization: OrganizationId,
    pub profile: SubscriptionProfileId,
    pub entry: Entry,
    pub baseline: Option<i64>,
    pub view: wire::SignInRelay,
}

impl Attempt {
    pub fn state(&self) -> &'static str {
        match &self.view.progress {
            wire::SignInRelayProgress::RelayStarting(_) => "starting",
            wire::SignInRelayProgress::RelayAwaitingBrowser(_) => "awaiting_browser",
            wire::SignInRelayProgress::RelayAwaitingCode(_) => "awaiting_code",
            wire::SignInRelayProgress::RelaySaving(_) => "saving",
            wire::SignInRelayProgress::RelayCompleted(_) => "completed",
            wire::SignInRelayProgress::RelayFailed(_) => "failed",
            wire::SignInRelayProgress::RelayExpired(_) => "expired",
            wire::SignInRelayProgress::RelayCancelled(_) => "cancelled",
        }
    }

    fn conflict(&self, operation: &'static str) -> Reason {
        Reason::StateConflict {
            operation,
            resource: Resource::SignInRelay,
            reference: self.view.id.clone(),
            organization: Some(self.view.organization.clone()),
            state: self.state(),
            holding_session: None,
            next: Next::inspect(Resource::SignInRelay, &self.view.id)
                .then(Step::ContinueSignInRelay {
                    harness: self.view.harness.clone(),
                    method: self.view.method.clone(),
                    relay: self.view.id.clone(),
                })
                .then(Step::CancelSignInRelay {
                    harness: self.view.harness.clone(),
                    method: self.view.method.clone(),
                    relay: self.view.id.clone(),
                }),
            message: "inspect this sign-in relay before continuing or cancelling it".to_owned(),
        }
    }
}

pub fn terminal(progress: &wire::SignInRelayProgress) -> bool {
    matches!(
        progress,
        wire::SignInRelayProgress::RelayCompleted(_)
            | wire::SignInRelayProgress::RelayFailed(_)
            | wire::SignInRelayProgress::RelayExpired(_)
            | wire::SignInRelayProgress::RelayCancelled(_)
    )
}

pub async fn start(
    store: &Store,
    images: &Images,
    organization: &str,
    harness: &str,
    method: &str,
    profile: Option<&str>,
) -> Result<wire::SignInRelay> {
    let offered = catalogue::sign_in_method(START, harness, method)?;
    if offered.relay.is_none() {
        bail!(Reason::InvalidField {
            field: "method",
            operation: START,
            constraint: Constraint::Offered,
            allowed: Some(vec![
                "claude-setup-token".to_owned(),
                "codex-device-auth".to_owned(),
            ]),
            message: "this Sign-in Method has no supported relay".to_owned()
        });
    }
    images.admit(harness, START, "harness").await?;
    let mut tx = store.begin().await?;
    let organization = tx.organizations().named(organization).await?;
    let profile = match profile {
        None => super::operator_profile(&mut tx, &organization, harness, true)
            .await?
            .expect("declared"),
        Some(profile) => {
            let profile = tx.profiles().named(&organization, profile).await?;
            let operator = tx
                .operators()
                .current()
                .await?
                .ok_or(Reason::MissingOperator { operation: START })?;
            if profile.owner_operator != Some(operator.id) {
                bail!(Reason::Forbidden {
                    operation: START,
                    resource: Resource::SubscriptionProfile,
                    reference: profile.name.clone(),
                    organization: Some(organization.name.clone()),
                    constraint: "operator_owned",
                    next: Next::inspect(Resource::SubscriptionProfile, &profile.name),
                    message: "a subscription sign-in belongs in the Operator's own Profile"
                        .to_owned()
                });
            }
            profile
        }
    };
    let entry = filled(offered)?;
    if let Some(mut current) = tx.relays().active(profile.id, &entry).await? {
        if current.view.deadline.parse::<Timestamp>()? <= Timestamp::now() {
            tx.relays().update(&mut current, expired()).await?;
        } else {
            bail!(current.conflict(START));
        }
    }
    let now = Timestamp::now();
    let attempt = Attempt {
        organization: organization.id,
        profile: profile.id,
        baseline: tx.profiles().revision_of(&profile, &entry).await?,
        entry,
        view: wire::SignInRelay {
            id: Uuid::now_v7().to_string(),
            organization: organization.name,
            harness: harness.to_owned(),
            method: method.to_owned(),
            destination: wire::SignInHolding {
                resource: wire::SignInHoldingResource::SubscriptionProfile,
                reference: profile.name,
                fills: offered.fills.clone(),
            },
            version: 1,
            created_at: now.to_string(),
            deadline: (now + jiff::SignedDuration::from_secs(DEADLINE.as_secs() as i64))
                .to_string(),
            finished_at: None,
            progress: wire::SignInRelayProgress::RelayStarting(wire::RelayStarting {
                state: Default::default(),
            }),
        },
    };
    tx.relays().open(&attempt).await?;
    tx.commit().await?;
    Ok(attempt.view)
}

async fn scoped(
    tx: &mut Tx<'_>,
    organization: &str,
    harness: &str,
    method: &str,
    id: &str,
) -> Result<Attempt> {
    let organization = tx.organizations().named(organization).await?;
    let found = tx.relays().get(id).await?.filter(|attempt| {
        attempt.organization == organization.id
            && attempt.view.harness == harness
            && attempt.view.method == method
    });
    let Some(found) = found else {
        bail!(Reason::MissingReference {
            resource: Resource::SignInRelay,
            reference: id.to_owned(),
            organization: Some(organization.name),
            within: None,
            message: "no sign-in relay exists at this address".to_owned()
        });
    };
    if found
        .view
        .finished_at
        .as_ref()
        .map(|at| at.parse::<Timestamp>())
        .transpose()?
        .is_some_and(|at| Timestamp::now().as_second() - at.as_second() >= 86400)
    {
        bail!(Reason::Expired {operation: READ, resource: Resource::SignInRelay, reference: id.to_owned(), organization: Some(organization.name),
            next: Next::inspect(Resource::SignInRelay, id), message: "this sign-in relay's retained state has expired; inspect saved sign-ins before starting another".to_owned() });
    }
    Ok(found)
}

pub async fn read(
    store: &Store,
    organization: &str,
    harness: &str,
    method: &str,
    id: &str,
) -> Result<wire::SignInRelay> {
    let mut tx = store.begin().await?;
    let mut attempt = scoped(&mut tx, organization, harness, method, id).await?;
    expire(&mut tx, &mut attempt).await?;
    tx.commit().await?;
    Ok(attempt.view)
}

pub async fn cancel(
    store: &Store,
    organization: &str,
    harness: &str,
    method: &str,
    id: &str,
) -> Result<wire::SignInRelay> {
    let mut tx = store.begin().await?;
    let mut attempt = scoped(&mut tx, organization, harness, method, id).await?;
    expire(&mut tx, &mut attempt).await?;
    if !terminal(&attempt.view.progress) {
        tx.relays()
            .update(
                &mut attempt,
                wire::SignInRelayProgress::RelayCancelled(wire::RelayCancelled {
                    state: Default::default(),
                }),
            )
            .await?;
    }
    tx.commit().await?;
    Ok(attempt.view)
}

pub async fn code(
    store: &Store,
    organization: &str,
    harness: &str,
    method: &str,
    id: &str,
    code: &str,
) -> Result<wire::SignInRelay> {
    let code = code.trim();
    if code.is_empty() || code.len() > 4096 || !code.chars().all(|c| c.is_ascii_graphic()) {
        bail!(Reason::InvalidField {
            field: "code",
            operation: CODE,
            constraint: Constraint::SingleToken,
            allowed: None,
            message: "the authorization code must be one nonempty token of at most 4096 bytes"
                .to_owned()
        });
    }
    let mut tx = store.begin().await?;
    let mut attempt = scoped(&mut tx, organization, harness, method, id).await?;
    expire(&mut tx, &mut attempt).await?;
    if attempt.state() != "awaiting_code" {
        let reason = attempt.conflict(CODE);
        tx.commit().await?;
        bail!(reason);
    }
    let sender = store.relay_inputs.0.lock().unwrap().get(id).cloned();
    let Some(sender) = sender.filter(|sender| !sender.is_closed()) else {
        bail!(attempt.conflict(CODE));
    };
    tx.relays()
        .update(
            &mut attempt,
            wire::SignInRelayProgress::RelaySaving(wire::RelaySaving {
                state: Default::default(),
            }),
        )
        .await?;
    tx.commit().await?;
    // Once committed this answer is never replayed, even if its response is lost.
    let _ = sender.try_send(code.to_owned());
    Ok(attempt.view)
}

fn expired() -> wire::SignInRelayProgress {
    wire::SignInRelayProgress::RelayExpired(wire::RelayExpired {
        state: Default::default(),
    })
}

async fn expire(tx: &mut Tx<'_>, attempt: &mut Attempt) -> Result<()> {
    if !terminal(&attempt.view.progress)
        && attempt.view.deadline.parse::<Timestamp>()? <= Timestamp::now()
    {
        tx.relays().update(attempt, expired()).await?;
    }
    Ok(())
}

fn failed(attempt: &Attempt, message: &str) -> wire::SignInRelayProgress {
    wire::SignInRelayProgress::RelayFailed(wire::RelayFailed {
        state: Default::default(),
        sign_in: None,
        diagnostic: wire::Diagnostic::StateConflictDiagnostic(wire::StateConflictDiagnostic {
            kind: serde_json::json!("state_conflict"),
            message: message.to_owned(),
            field: None,
            context: wire::StateConflictContext {
                operation: START.to_owned(),
                resource: wire::Resource::SignInRelay,
                reference: attempt.view.id.clone(),
                organization: Some(attempt.view.organization.clone()),
                state: "failed".to_owned(),
                holding_session: None,
            },
            next_steps: vec![wire::Action::SignInAction(wire::SignInAction {
                action: serde_json::json!("sign_in"),
                harness: Some(attempt.view.harness.clone()),
                method: Some(attempt.view.method.clone()),
                sign_in: Some(attempt.view.destination.reference.clone()),
            })],
        }),
    })
}

pub async fn claim(store: &Store) -> Result<Option<Attempt>> {
    let mut tx = store.begin().await?;
    let found = tx.relays().claim().await?;
    tx.commit().await?;
    Ok(found)
}

async fn transition(store: &Store, id: &str, progress: wire::SignInRelayProgress) -> Result<()> {
    let mut tx = store.begin().await?;
    if let Some(mut attempt) = tx.relays().get(id).await? {
        expire(&mut tx, &mut attempt).await?;
        if !terminal(&attempt.view.progress) {
            tx.relays().update(&mut attempt, progress).await?;
        }
    }
    tx.commit().await
}

async fn complete(store: &Store, id: &str, material: &str) -> Result<()> {
    let mut tx = store.begin().await?;
    let Some(mut attempt) = tx.relays().get(id).await? else {
        return Ok(());
    };
    expire(&mut tx, &mut attempt).await?;
    if !tx.relays().take_completion(id).await? {
        tx.commit().await?;
        return Ok(());
    }
    let organization = tx.organizations().by_id(attempt.organization).await?;
    let profile = tx
        .profiles()
        .all(&organization)
        .await?
        .into_iter()
        .find(|profile| profile.id == attempt.profile)
        .ok_or_else(|| anyhow::anyhow!("the destination Profile is gone"))?;
    if tx.profiles().revision_of(&profile, &attempt.entry).await? != attempt.baseline {
        let progress = wire::SignInRelayProgress::RelayCancelled(wire::RelayCancelled {
            state: Default::default(),
        });
        tx.relays().update(&mut attempt, progress).await?;
    } else {
        let method = catalogue::sign_in_method(START, &attempt.view.harness, &attempt.view.method)?;
        let material = super::structural(method, material)?;
        let authentication = Authentication {
            state: State::LoginCompleted,
            source: Source::Relay,
            observed_at: Timestamp::now(),
            provider_check: None,
        };
        let held = tx
            .profiles()
            .hold(&profile, &attempt.entry, &material, &authentication)
            .await?;
        let saved = SignIn {
            harness: attempt.view.harness.clone(),
            method,
            holding: Holding::Profile(profile),
            revision: held.revision,
            refreshed_from: None,
            saved_at: held.set_at,
            authentication,
            model_use: Vec::new(),
        };
        tx.relays()
            .update(
                &mut attempt,
                wire::SignInRelayProgress::RelayCompleted(wire::RelayCompleted {
                    state: Default::default(),
                    sign_in: saved.into(),
                }),
            )
            .await?;
    }
    tx.commit().await
}

fn instance(id: &str) -> String {
    format!("sign-in-relay-{id}")
}

pub async fn recover(store: &Store, driver: Option<&Driver>) -> Result<()> {
    loop {
        let mut tx = store.begin().await?;
        let attempts = tx.relays().unfinished().await?;
        if attempts.is_empty() {
            tx.relays().release_all_cleanup().await?;
            tx.commit().await?;
            break;
        }
        for mut attempt in attempts {
            let progress = failed(
                &attempt,
                "the control plane restarted before sign-in finished; start a new sign-in or import material",
            );
            tx.relays().update(&mut attempt, progress).await?;
        }
        tx.commit().await?;
    }
    if let Err(error) = maintain(store, driver).await {
        tracing::warn!(%error, "interrupted relay cleanup will be retried");
    }
    Ok(())
}

pub async fn maintain(store: &Store, driver: Option<&Driver>) -> Result<()> {
    let mut tx = store.begin().await?;
    let mut cleaning = Vec::new();
    for mut attempt in tx.relays().unattended().await? {
        expire(&mut tx, &mut attempt).await?;
        if terminal(&attempt.view.progress) {
            cleaning.push(attempt.view.id);
        }
    }
    tx.relays().forget_old_snapshots().await?;
    tx.commit().await?;
    for id in cleaning {
        if let Some(driver) = driver {
            destroy(driver, &id).await?;
        }
        let mut tx = store.begin().await?;
        tx.relays().cleaned(&id).await?;
        tx.commit().await?;
    }
    Ok(())
}

pub async fn run(
    store: Store,
    driver: Option<Driver>,
    attempt: Attempt,
    shutdown: CancellationToken,
) {
    let id = attempt.view.id.clone();
    let mut answers = store.relay_inputs.insert(&id);
    let result = match &driver {
        Some(driver) => drive(&store, driver, &attempt, &mut answers, &shutdown).await,
        None => Err(anyhow::anyhow!("no compute driver")),
    };
    store.relay_inputs.remove(&id);
    loop {
        let saved =
            match &result {
                Ok(Some(material)) => complete(&store, &id, material).await,
                Ok(None) => Ok(()),
                Err(error) => transition(
                    &store,
                    &id,
                    failed(
                        &attempt,
                        error.downcast_ref::<Unusable>().map_or(
                            "the vendor sign-in could not complete; retry or import the sign-in instead",
                            |unusable| unusable.0,
                        ),
                    ),
                )
                .await,
            };
        match saved {
            Err(error) if store::busy(&error) => tokio::time::sleep(POLL).await,
            Err(_) => {
                let _ = transition(&store, &id, failed(&attempt, "the sign-in result could not be saved; inspect saved sign-ins before retrying")).await;
                break;
            }
            Ok(()) => break,
        }
    }
    if let Some(driver) = driver
        && destroy(&driver, &id).await.is_err()
    {
        if let Ok(mut tx) = store.begin().await {
            let _ = tx.relays().release_cleanup(&id).await;
            let _ = tx.commit().await;
        }
        return;
    }
    if let Ok(mut tx) = store.begin().await {
        let _ = tx.relays().cleaned(&id).await;
        let _ = tx.commit().await;
    }
}

async fn drive(
    store: &Store,
    driver: &Driver,
    attempt: &Attempt,
    answers: &mut mpsc::Receiver<String>,
    shutdown: &CancellationToken,
) -> Result<Option<String>> {
    let relay = catalogue::sign_in_method(START, &attempt.view.harness, &attempt.view.method)?
        .relay
        .clone();
    let mut instance = provision(store, driver, attempt, shutdown).await?;
    let remaining = remaining(attempt)?;
    let relayed = async {
        match relay {
            Some(wire::SignInMethodRelay::ClaudeSetupToken) => {
                let mut terminal = instance.terminal(&["claude", "setup-token"])?;
                drive_terminal(store, attempt, answers, &mut terminal).await
            }
            Some(wire::SignInMethodRelay::CodexDeviceAuth) => {
                authorize_device(store, attempt, &mut instance).await
            }
            None => bail!("this Sign-in Method has no relay"),
        }
    };
    tokio::select! {
        result = tokio::time::timeout(remaining, relayed) => result?,
        () = shutdown.cancelled() => bail!("sign-in interrupted"),
    }
}

/// A relay failure whose message is safe to show and names what the Operator can do instead.
#[derive(Debug, Clone, Copy)]
struct Unusable(&'static str);

impl std::fmt::Display for Unusable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

impl std::error::Error for Unusable {}

const OUTPUT_LIMIT: usize = 1024 * 1024;

async fn running(store: &Store, id: &str) -> Result<Option<Attempt>> {
    let mut tx = store.begin().await?;
    let Some(mut current) = tx.relays().get(id).await? else {
        return Ok(None);
    };
    expire(&mut tx, &mut current).await?;
    tx.commit().await?;
    Ok((!terminal(&current.view.progress)).then_some(current))
}

/// The vendor polls for the Operator's approval itself, so nothing is ever typed into it.
async fn authorize_device(
    store: &Store,
    attempt: &Attempt,
    instance: &mut crate::compute::Instance,
) -> Result<Option<String>> {
    let id = &attempt.view.id;
    let wire::SignInFill::SignInFileFill(fill) = &attempt.view.destination.fills else {
        bail!("device authorization fills a login file");
    };
    let mut terminal = instance.terminal(codex::LOGIN)?;
    let mut decoder = codex::Decoder::new();
    let mut bytes = 0;
    let mut output_closed = false;
    loop {
        let Some(current) = running(store, id).await? else {
            return Ok(None);
        };
        tokio::select! {
            output = terminal.output.recv(), if !output_closed => match output {
                None => output_closed = true,
                Some(output) => {
                    bytes += output.len();
                    if bytes > OUTPUT_LIMIT { bail!("vendor output exceeded its limit"); }
                    if let Some(prompt) = decoder.feed(&output)
                        && current.state() == "starting" {
                        transition(store, id, wire::SignInRelayProgress::RelayAwaitingBrowser(wire::RelayAwaitingBrowser {
                            state: Default::default(),
                            url: prompt.url,
                            code: Some(prompt.code),
                        })).await?;
                    }
                }
            },
            () = tokio::time::sleep(POLL) => {}
        }
        if output_closed && let Some(success) = terminal.exited()? {
            if !success {
                bail!(Unusable(
                    "the vendor device authorization did not complete; retry, or import the auth.json a Codex sign-in writes instead"
                ));
            }
            break;
        }
    }
    drop(terminal);
    transition(
        store,
        id,
        wire::SignInRelayProgress::RelaySaving(wire::RelaySaving {
            state: Default::default(),
        }),
    )
    .await?;
    let unusable = Unusable(
        "the vendor finished without a ChatGPT login file; retry, or import the auth.json a Codex sign-in writes instead",
    );
    let material = instance
        .read_home_file(&fill.path)
        .ok()
        .filter(|material| material.len() <= OUTPUT_LIMIT)
        .and_then(|material| String::from_utf8(material).ok())
        .ok_or(unusable)?;
    let method = catalogue::sign_in_method(START, &attempt.view.harness, &attempt.view.method)?;
    super::structural(method, &material).map_err(|_| unusable)?;
    Ok(Some(material))
}

fn remaining(attempt: &Attempt) -> Result<Duration> {
    Ok(Duration::try_from(
        attempt
            .view
            .deadline
            .parse::<Timestamp>()?
            .duration_since(Timestamp::now()),
    )?)
}

async fn stopped(store: &Store, id: &str) -> Result<()> {
    loop {
        let mut tx = store.read().await?;
        if tx
            .relays()
            .get(id)
            .await?
            .is_none_or(|attempt| terminal(&attempt.view.progress))
        {
            return Ok(());
        }
        drop(tx);
        tokio::time::sleep(POLL).await;
    }
}

async fn provision(
    store: &Store,
    driver: &Driver,
    attempt: &Attempt,
    shutdown: &CancellationToken,
) -> Result<crate::compute::Instance> {
    let remaining = remaining(attempt)?;
    let deadline = std::time::Instant::now() + remaining;
    let cancel = shutdown.child_token();
    let _cancel_on_drop = cancel.clone().drop_guard();
    let driver = driver.clone();
    let holder = instance(&attempt.view.id);
    let command_cancel = cancel.clone();
    let mut provisioning = tokio::task::spawn_blocking(move || {
        driver.provision_until(&holder, deadline, &command_cancel)
    });
    tokio::select! {
        result = &mut provisioning => return Ok(result??),
        () = shutdown.cancelled() => {},
        () = tokio::time::sleep(remaining) => {},
        _ = stopped(store, &attempt.view.id) => {},
    }
    cancel.cancel();
    let _ = provisioning.await;
    bail!("sign-in provisioning was interrupted")
}

async fn destroy(driver: &Driver, id: &str) -> Result<()> {
    let driver = driver.clone();
    let name = driver.instance_for(instance(id));
    tokio::task::spawn_blocking(move || {
        driver.destroy_until(&name, std::time::Instant::now() + Duration::from_secs(10))
    })
    .await??;
    Ok(())
}

async fn drive_terminal(
    store: &Store,
    attempt: &Attempt,
    answers: &mut mpsc::Receiver<String>,
    terminal: &mut crate::compute::Terminal,
) -> Result<Option<String>> {
    let id = &attempt.view.id;
    let mut decoder = claude::Decoder::new();
    let mut token = None;
    let mut answered = false;
    let mut continued = false;
    let mut bytes = 0;
    let mut output_closed = false;
    loop {
        let Some(current) = running(store, id).await? else {
            return Ok(None);
        };
        tokio::select! {
            answer = answers.recv(), if !answered => {
                if let Some(answer) = answer {
                    terminal.write(answer.as_bytes())?;
                    terminal.write(b"\r")?;
                    answered = true;
                }
            }
            output = terminal.output.recv(), if !output_closed => {
                match output {
                    None => {
                        output_closed = true;
                        if answered { token = token.or_else(|| decoder.finish()); }
                    },
                    Some(output) => {
                        bytes += output.len();
                        if bytes > OUTPUT_LIMIT { bail!("vendor output exceeded its limit"); }
                        let decoded = decoder.feed(&output, answered)?;
                        if let Some(url) = decoded.url
                            && current.state() == "starting" {
                                transition(store, id, wire::SignInRelayProgress::RelayAwaitingCode(wire::RelayAwaitingCode {state: Default::default(), url: url.parse()?})).await?;
                        }
                        if let Some(found) = decoded.token { token = Some(found); }
                        if token.is_some() && decoded.continue_prompt && !continued {
                            terminal.write(b"\r")?;
                            continued = true;
                        }
                    }
                }
            }
            () = tokio::time::sleep(POLL) => {}
        }
        if output_closed && let Some(success) = terminal.exited()? {
            if success && answered && token.is_some() {
                return Ok(token);
            }
            bail!("vendor exited without completing sign-in");
        }
    }
}
