//! An explicit test of saved material: one prompt, sent once, by the harness in a short-lived
//! Instance that has no checkout and is handed nothing but the material selected. What it finds
//! is evidence for that revision, harness and model alone.

use std::collections::BTreeMap;
use std::io::{BufRead as _, BufReader, Read};
use std::time::Duration;

use anyhow::{Result, bail};
use jiff::Timestamp;
use kestrel_operator_types::{SignInMethod, SignInMethodOwnership};
use serde::Deserialize;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};
use uuid::Uuid;

use super::credential_use::{self, CredentialUse};
use super::{Source, State, TestOutcome, TestState, UseResult, UseSource, filled};
use crate::capability::{Capability, Images};
use crate::catalogue;
use crate::compute::Driver;
use crate::declined::{Constraint, Locator, Next, Reason, Resource};
use crate::domain::{Organization, OrganizationId, SubscriptionProfile, SubscriptionProfileId};
use crate::profile::{Entry, Kind};
use crate::store::{self, Store, Tx};

pub const TEST: &str = "test_sign_in";
pub const READ: &str = "read_sign_in_test";

pub const DEADLINE: Duration = Duration::from_secs(120);

const PROMPT: &str = "Reply with the single word: ok";
const MESSAGE_LIMIT: usize = 1024;
const POLL: Duration = Duration::from_millis(100);

#[derive(Debug, Clone)]
pub struct Test {
    pub id: String,
    pub organization: OrganizationId,
    pub harness: String,
    pub method: String,
    /// None for a key the Organization holds.
    pub profile: Option<SubscriptionProfileId>,
    pub revision: i64,
    pub requested_model: Option<String>,
    pub model: Option<String>,
    pub image: Option<String>,
    pub state: TestState,
    pub outcome: Option<TestOutcome>,
    pub message: Option<String>,
    pub created_at: Timestamp,
    pub started_at: Option<Timestamp>,
    pub finished_at: Option<Timestamp>,
}

struct Selected {
    organization: Organization,
    profile: Option<SubscriptionProfile>,
    revision: i64,
}

/// Refuses rather than queues: a subscription in use is a busy refusal here, and the hold
/// taken with the test's row is kept until whatever the harness refreshed has been handed back.
pub async fn start(
    store: &Store,
    images: &Images,
    organization: &str,
    harness: &str,
    method: &str,
    model: Option<&str>,
) -> Result<Test> {
    let offered = catalogue::sign_in_method(TEST, harness, method)?;
    let model = named(offered, harness, model)?;

    let examined = {
        let mut tx = store.read().await?;
        selected(&mut tx, organization, harness, offered).await?
    };

    let capability = images.read().await;
    let image = match &capability {
        Capability::Inspected { identity, .. } => Some(identity.clone()),
        Capability::Unchecked | Capability::Unavailable { .. } => None,
    };
    capability
        .admits(harness, TEST)
        .map_err(|reason| reason.concerning("harness"))?;

    let mut tx = store.begin().await?;
    let selected = selected(&mut tx, organization, harness, offered).await?;
    if selected.revision != examined.revision {
        let (resource, reference) = holding(offered, &selected)?;
        bail!(Reason::StateConflict {
            operation: TEST,
            resource,
            reference: reference.clone(),
            organization: Some(selected.organization.name.clone()),
            state: "replaced",
            holding_session: None,
            next: Next::inspect(resource, reference),
            message: format!(
                "the {} sign-in for {harness} was replaced while its test was starting; test \
                 the replacement instead",
                offered.id
            ),
        });
    }

    let test = Test {
        id: Uuid::now_v7().to_string(),
        organization: selected.organization.id,
        harness: harness.to_owned(),
        method: offered.id.clone(),
        profile: selected.profile.as_ref().map(|profile| profile.id),
        revision: selected.revision,
        requested_model: model,
        model: None,
        image,
        state: TestState::Pending,
        outcome: None,
        message: None,
        created_at: Timestamp::now(),
        started_at: None,
        finished_at: None,
    };
    if let Some(profile) = &selected.profile {
        credential_use::acquire(
            &mut tx,
            &selected.organization.name,
            &profile.name,
            harness,
            &holder(&test.id),
        )
        .await?;
    }
    tx.sign_ins().open_test(&test).await?;
    tx.commit().await?;

    Ok(test)
}

pub async fn read(
    store: &Store,
    organization: &str,
    harness: &str,
    method: &str,
    id: &str,
) -> Result<Test> {
    let offered = catalogue::sign_in_method(READ, harness, method)?;
    let mut tx = store.read().await?;
    let organization = tx.organizations().named(organization).await?;

    match tx.sign_ins().test(id).await? {
        Some(test)
            if test.organization == organization.id
                && test.harness == harness
                && test.method == offered.id =>
        {
            Ok(test)
        }
        _ => bail!(Reason::MissingReference {
            resource: Resource::SignInTest,
            reference: id.to_owned(),
            organization: Some(organization.name.clone()),
            within: Some(Locator::new(Resource::Organization, organization.name)),
            message: format!(
                "no test {id} of the {} sign-in for {harness} was started",
                offered.id
            ),
        }),
    }
}

/// A method listing providers routes by one, so its test names the model as `provider/model`
/// and no default is taken from whatever the harness last used.
fn named(offered: &SignInMethod, harness: &str, model: Option<&str>) -> Result<Option<String>> {
    let model = model.map(str::trim).filter(|model| !model.is_empty());
    let Some(providers) = &offered.test_providers else {
        return Ok(model.map(str::to_owned));
    };
    let Some(model) = model else {
        bail!(Reason::InvalidField {
            field: "model",
            operation: TEST,
            constraint: Constraint::NonEmpty,
            allowed: None,
            message: format!(
                "a test of {} on {harness} needs a model named as provider/model, the provider \
                 one of {}",
                offered.name,
                providers.join(", ")
            ),
        });
    };
    if provider(offered, model).is_none() {
        bail!(Reason::InvalidField {
            field: "model",
            operation: TEST,
            constraint: Constraint::Offered,
            allowed: Some(providers.clone()),
            message: format!(
                "{} is tested on {harness} with a model named as provider/model, the provider \
                 one of {}; {model} is not",
                offered.name,
                providers.join(", ")
            ),
        });
    }

    Ok(Some(model.to_owned()))
}

fn provider<'m>(offered: &SignInMethod, model: &'m str) -> Option<&'m str> {
    let (provider, named) = model.split_once('/')?;
    let compatible = offered
        .test_providers
        .as_ref()?
        .iter()
        .any(|offered| offered == provider);

    (compatible && !named.is_empty()).then_some(provider)
}

async fn selected(
    tx: &mut Tx<'_>,
    organization: &str,
    harness: &str,
    offered: &'static SignInMethod,
) -> Result<Selected> {
    let organization = tx.organizations().named(organization).await?;
    let entry = filled(offered)?;
    let not_saved = |organization: &Organization, resource: Resource, reference: String| {
        Reason::StateConflict {
            operation: TEST,
            resource,
            reference: reference.clone(),
            organization: Some(organization.name.clone()),
            state: "sign_in_not_saved",
            holding_session: None,
            next: Next::inspect(resource, reference),
            message: format!(
                "no {} sign-in for {harness} is saved, so there is nothing to test",
                offered.name
            ),
        }
    };

    match offered.ownership {
        SignInMethodOwnership::Organization => {
            let Some(revision) = tx
                .organizations()
                .provider_credential_revision(organization.id, &entry.name)
                .await?
            else {
                bail!(not_saved(
                    &organization,
                    Resource::ProviderCredential,
                    entry.name
                ));
            };

            Ok(Selected {
                organization,
                profile: None,
                revision,
            })
        }
        SignInMethodOwnership::Operator => {
            let operator = tx
                .operators()
                .current()
                .await?
                .ok_or(Reason::MissingOperator { operation: TEST })?;
            let mut holding = Vec::new();
            for profile in super::operator_profiles(tx, &organization).await? {
                if let Some(revision) = tx.profiles().revision_of(&profile, &entry).await? {
                    holding.push((profile, revision));
                }
            }
            if holding.len() > 1 {
                let among: Vec<SubscriptionProfile> =
                    holding.into_iter().map(|(profile, _)| profile).collect();
                let message = format!(
                    "{} owns several Subscription Profiles in {} holding a {} sign-in ({}), and \
                     a test takes one",
                    operator.name,
                    organization.name,
                    offered.name,
                    super::names(&among)
                );
                bail!(super::several_operator_profiles(
                    TEST,
                    &organization,
                    &among,
                    message
                ));
            }
            let Some((profile, revision)) = holding.pop() else {
                bail!(not_saved(
                    &organization,
                    Resource::SubscriptionProfile,
                    operator.name
                ));
            };

            Ok(Selected {
                organization,
                profile: Some(profile),
                revision,
            })
        }
    }
}

fn holding(offered: &SignInMethod, selected: &Selected) -> Result<(Resource, String)> {
    Ok(match &selected.profile {
        Some(profile) => (Resource::SubscriptionProfile, profile.name.clone()),
        None => (Resource::ProviderCredential, filled(offered)?.name),
    })
}

fn holder(id: &str) -> String {
    format!("the sign-in test {id}")
}

/// What `Driver::provision` is asked to name the test's Instance after, so a restart finds one
/// a dead process left behind.
fn instance(id: &str) -> String {
    format!("sign-in-test-{id}")
}

pub async fn claim(store: &Store) -> Result<Option<Test>> {
    let mut tx = store.begin().await?;
    let claimed = tx.sign_ins().claim_test().await?;
    tx.commit().await?;

    Ok(claimed)
}

/// What a work role with nowhere to run an Instance does with a test, so none waits forever
/// holding a sign-in.
pub async fn nowhere_to_run(store: &Store) -> Result<()> {
    while let Some(test) = claim(store).await? {
        conclude(
            store,
            &test,
            None,
            Waited::Ended(Ending::failed(
                TestOutcome::Unavailable,
                "this control plane has nowhere to run an Instance",
            )),
        )
        .await
        .map_err(|unconcluded| unconcluded.error)?;
    }

    Ok(())
}

/// No test outlives the process that ran it: nothing resumes one, because resuming would spend
/// a second model call nobody asked for.
pub async fn interrupt_unfinished(store: &Store, driver: Option<&Driver>) -> Result<()> {
    let mut tx = store.begin().await?;
    let unfinished = tx.sign_ins().unfinished_tests().await?;
    for test in &unfinished {
        tx.sign_ins()
            .end_test(
                &test.id,
                TestState::Interrupted,
                None,
                Some("the control plane restarted before this test finished"),
            )
            .await?;
    }
    tx.commit().await?;

    for test in unfinished {
        let Some(driver) = driver else { continue };
        let left = driver.names(instance(&test.id));
        if let Err(error) = driver.destroy_named(&left) {
            warn!(test = test.id, instance = left, %error, "an interrupted sign-in test's instance resisted being destroyed");
        }
    }

    Ok(())
}

pub struct Running<'a> {
    pub store: &'a Store,
    pub driver: &'a Driver,
    /// None when this work role spawns no harness of the test's name.
    pub command: Option<&'a str>,
    pub auth: Option<&'a str>,
    pub deadline: Duration,
    pub shutdown: &'a CancellationToken,
}

struct Handed {
    profile: Option<SubscriptionProfile>,
    entry: Entry,
    variables: BTreeMap<String, String>,
    files: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "probe", rename_all = "snake_case")]
enum Said {
    Ready {
        model: Option<String>,
    },
    Finished {
        model: Option<String>,
        outcome: Probed,
        because: Option<String>,
        #[serde(default)]
        refreshed: BTreeMap<String, String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Probed {
    Answered,
    AuthenticationRequired,
    ModelNotOffered,
    Failed,
}

#[derive(Clone)]
struct Ending {
    state: TestState,
    outcome: Option<TestOutcome>,
    message: Option<String>,
}

impl Ending {
    fn failed(outcome: TestOutcome, message: impl Into<String>) -> Self {
        Self {
            state: TestState::Failed,
            outcome: Some(outcome),
            message: Some(message.into()),
        }
    }
}

enum Waited {
    Probed {
        model: Option<String>,
        outcome: Probed,
        because: Option<String>,
        refreshed: BTreeMap<String, String>,
    },
    Ended(Ending),
}

impl Running<'_> {
    pub async fn run(&self, test: Test) {
        let (handed, waited) = match self.attempt(&test).await {
            Ok(attempted) => attempted,
            Err(error) => {
                warn!(test = test.id, %error, "a sign-in test could not be run");
                (
                    None,
                    Waited::Ended(Ending::failed(
                        TestOutcome::HarnessFailed,
                        format!("the test could not be run: {error:#}"),
                    )),
                )
            }
        };

        // Retried while the store is busy: a test left running would hold its sign-in.
        let mut waited = Some(waited);
        loop {
            let Some(found) = waited.take() else { return };
            match conclude(self.store, &test, handed.as_ref(), found).await {
                Ok(()) => return,
                Err(Unconcluded { error, found })
                    if store::busy(&error) && !self.shutdown.is_cancelled() =>
                {
                    waited = Some(found);
                    tokio::time::sleep(POLL).await;
                }
                Err(Unconcluded { error, .. }) => {
                    warn!(test = test.id, %error, "a sign-in test's result could not be recorded");
                    return;
                }
            }
        }
    }

    async fn attempt(&self, test: &Test) -> Result<(Option<Handed>, Waited)> {
        let handed = {
            let mut tx = self.store.read().await?;
            hand_over(&mut tx, test).await?
        };
        let Some(handed) = handed else {
            return Ok((
                None,
                Waited::Ended(Ending::failed(
                    TestOutcome::MaterialReplaced,
                    "the sign-in was replaced before the test could be handed it, and the \
                     replacement was not tested instead",
                )),
            ));
        };
        let Some(command) = self.command else {
            let message = format!("this work role spawns no harness named {}", test.harness);
            return Ok((
                Some(handed),
                Waited::Ended(Ending::failed(TestOutcome::Unavailable, message)),
            ));
        };

        let probe = serde_json::json!({
            "command": command,
            "auth": self.auth,
            "model": test.requested_model,
            "prompt": PROMPT,
            "variables": handed.variables,
            "files": handed.files,
        })
        .to_string();

        let mut instance = match self.driver.provision(instance(&test.id)) {
            Ok(instance) => instance,
            Err(error) => {
                let message = format!("the test's instance could not be provisioned: {error}");
                return Ok((
                    Some(handed),
                    Waited::Ended(Ending::failed(TestOutcome::Unavailable, message)),
                ));
            }
        };
        let waited = match instance.supervise(&[("KESTREL_PROBE", &probe)]) {
            Ok(mut supervisor) => {
                let mut said = lines(supervisor.take_stdout());
                if let Some(err) = supervisor.take_stderr() {
                    std::thread::spawn(move || BufReader::new(err).lines().for_each(drop));
                }
                let waited = tokio::select! {
                    waited = self.probed(test, &mut said) => waited,
                    () = tokio::time::sleep(self.deadline) => Waited::Ended(Ending::failed(
                        TestOutcome::Unavailable,
                        format!(
                            "the test did not finish within {} seconds, so it was stopped; \
                             what is saved is as it was",
                            self.deadline.as_secs()
                        ),
                    )),
                    () = self.shutdown.cancelled() => Waited::Ended(Ending {
                        state: TestState::Interrupted,
                        outcome: None,
                        message: Some(
                            "the control plane stopped before this test finished".to_owned(),
                        ),
                    }),
                };
                if let Err(error) = supervisor.stop() {
                    warn!(test = test.id, %error, "a sign-in test's supervisor resisted being stopped");
                }
                waited
            }
            Err(error) => Waited::Ended(Ending::failed(
                TestOutcome::Unavailable,
                format!("the test's supervisor could not be started: {error}"),
            )),
        };
        let name = instance.name().to_owned();
        if let Err(error) = instance.destroy() {
            warn!(test = test.id, instance = name, %error, "a sign-in test's instance resisted being destroyed");
        }

        Ok((Some(handed), waited))
    }

    async fn probed(&self, test: &Test, said: &mut mpsc::UnboundedReceiver<String>) -> Waited {
        while let Some(line) = said.recv().await {
            match serde_json::from_str::<Said>(&line) {
                Ok(Said::Ready { model: Some(model) }) => {
                    if let Err(error) = resolved(self.store, test, &model).await {
                        warn!(test = test.id, %error, "a sign-in test's model could not be recorded");
                    }
                }
                Ok(Said::Finished {
                    model,
                    outcome,
                    because,
                    refreshed,
                }) => {
                    return Waited::Probed {
                        model,
                        outcome,
                        because,
                        refreshed,
                    };
                }
                Ok(Said::Ready { model: None }) | Err(_) => {}
            }
        }

        Waited::Ended(Ending::failed(
            TestOutcome::HarnessFailed,
            "the test's supervisor exited without saying how the test went",
        ))
    }
}

fn lines(said: Option<impl Read + Send + 'static>) -> mpsc::UnboundedReceiver<String> {
    let (saying, heard) = mpsc::unbounded_channel();
    if let Some(said) = said {
        std::thread::spawn(move || {
            for line in BufReader::new(said).lines().map_while(Result::ok) {
                if saying.send(line).is_err() {
                    break;
                }
            }
        });
    }

    heard
}

async fn resolved(store: &Store, test: &Test, model: &str) -> Result<()> {
    let mut tx = store.begin().await?;
    tx.sign_ins().resolve_test_model(&test.id, model).await?;
    tx.commit().await
}

/// `None` when the revision selected is no longer what is held: a different revision is never
/// tested in its place. Nothing but the one entry is decrypted into what is handed.
async fn hand_over(tx: &mut Tx<'_>, test: &Test) -> Result<Option<Handed>> {
    let offered = catalogue::sign_in_method(TEST, &test.harness, &test.method)?;
    let entry = filled(offered)?;
    let organization = tx.organizations().by_id(test.organization).await?;

    let mut handed = Handed {
        profile: None,
        entry: entry.clone(),
        variables: BTreeMap::new(),
        files: BTreeMap::new(),
    };
    let secret = match test.profile {
        None => {
            if tx
                .organizations()
                .provider_credential_revision(organization.id, &entry.name)
                .await?
                != Some(test.revision)
            {
                return Ok(None);
            }
            tx.organizations()
                .provider_credentials(organization.id)
                .await?
                .remove(&entry.name)
        }
        Some(id) => {
            let Some(profile) = tx
                .profiles()
                .all(&organization)
                .await?
                .into_iter()
                .find(|profile| profile.id == id)
            else {
                return Ok(None);
            };
            if tx.profiles().revision_of(&profile, &entry).await? != Some(test.revision) {
                return Ok(None);
            }
            let mut contents = tx.profiles().contents(&profile).await?;
            handed.profile = Some(profile);
            match entry.kind {
                Kind::Variable => contents.variables.remove(&entry.name),
                Kind::File => contents.files.remove(&entry.name),
            }
        }
    };
    let Some(secret) = secret else {
        return Ok(None);
    };
    match entry.kind {
        Kind::Variable => handed.variables.insert(entry.name, secret),
        Kind::File => handed.files.insert(entry.name, secret),
    };
    if let Some(provider) = test
        .requested_model
        .as_deref()
        .and_then(|model| provider(offered, model))
        && let Some((variable, value)) = catalogue::confining(&test.harness, provider)
    {
        handed.variables.insert(variable.to_owned(), value);
    }

    Ok(Some(handed))
}

struct Unconcluded {
    error: anyhow::Error,
    found: Waited,
}

/// One transaction: what the harness refreshed is handed back, the evidence its answer
/// supports is recorded, the hold is released and the test ends. A test that established
/// nothing writes no evidence, so what was known before keeps its time.
async fn conclude(
    store: &Store,
    test: &Test,
    handed: Option<&Handed>,
    found: Waited,
) -> Result<(), Unconcluded> {
    match concluding(store, test, handed, &found).await {
        Ok(()) => Ok(()),
        Err(error) => Err(Unconcluded { error, found }),
    }
}

async fn concluding(
    store: &Store,
    test: &Test,
    handed: Option<&Handed>,
    found: &Waited,
) -> Result<()> {
    let mut tx = store.begin().await?;

    let ending = match found {
        Waited::Ended(ending) => ending.clone(),
        Waited::Probed {
            model,
            outcome,
            because,
            refreshed,
        } => {
            let revision = match handed {
                Some(handed) => refresh(&mut tx, test, handed, refreshed).await?,
                None => test.revision,
            };
            let model = model.as_deref().or(test.requested_model.as_deref());
            if let Some(model) = model {
                tx.sign_ins().resolve_test_model(&test.id, model).await?;
            }
            let used = match outcome {
                Probed::Answered => Some(UseResult::Worked),
                Probed::AuthenticationRequired => {
                    tx.sign_ins()
                        .observe(revision, State::AuthenticationFailed, Source::ModelTest)
                        .await?;
                    Some(UseResult::AuthenticationFailed)
                }
                Probed::ModelNotOffered => Some(UseResult::NotCovered),
                Probed::Failed => None,
            };
            if let (Some(used), Some(model)) = (used, model) {
                tx.sign_ins()
                    .record_use(
                        revision,
                        &test.harness,
                        model,
                        used,
                        UseSource::ModelTest,
                        test.image.as_deref(),
                    )
                    .await?;
            }
            let message = because.as_deref().map(bounded);
            match outcome {
                Probed::Answered => Ending {
                    state: TestState::Completed,
                    outcome: Some(TestOutcome::Worked),
                    message: None,
                },
                Probed::AuthenticationRequired => Ending {
                    state: TestState::Failed,
                    outcome: Some(TestOutcome::AuthenticationFailed),
                    message,
                },
                Probed::ModelNotOffered => Ending {
                    state: TestState::Failed,
                    outcome: Some(TestOutcome::NotCovered),
                    message,
                },
                Probed::Failed => Ending {
                    state: TestState::Failed,
                    outcome: Some(TestOutcome::HarnessFailed),
                    message,
                },
            }
        }
    };

    if let Some(profile) = test.profile {
        tx.sign_ins()
            .release(&CredentialUse {
                profile,
                harness: test.harness.clone(),
                holder: holder(&test.id),
            })
            .await?;
    }
    tx.sign_ins()
        .end_test(
            &test.id,
            ending.state,
            ending.outcome,
            ending.message.as_deref(),
        )
        .await?;
    tx.commit().await?;
    info!(
        test = test.id,
        harness = test.harness,
        method = test.method,
        revision = test.revision,
        state = ending.state.as_str(),
        "a sign-in test ended"
    );

    Ok(())
}

/// The revision the harness ended on: its own refresh when that was kept, which compare-and-set
/// refuses once anything else has replaced the revision the test began from.
async fn refresh(
    tx: &mut Tx<'_>,
    test: &Test,
    handed: &Handed,
    refreshed: &BTreeMap<String, String>,
) -> Result<i64> {
    let (Some(profile), Kind::File) = (&handed.profile, handed.entry.kind) else {
        return Ok(test.revision);
    };
    let Some(contents) = refreshed.get(&handed.entry.name) else {
        return Ok(test.revision);
    };

    match tx
        .profiles()
        .refresh_file(profile, &handed.entry, test.revision, contents)
        .await?
    {
        Some(revision) => Ok(revision),
        None => {
            info!(
                test = test.id,
                profile = %profile.name,
                file = %handed.entry.name,
                "a login a sign-in test refreshed was not kept: newer material replaced the revision it began from"
            );
            Ok(test.revision)
        }
    }
}

fn bounded(text: &str) -> String {
    if text.len() <= MESSAGE_LIMIT {
        return text.to_owned();
    }
    let mut end = MESSAGE_LIMIT;
    while !text.is_char_boundary(end) {
        end -= 1;
    }

    text[..end].to_owned()
}
