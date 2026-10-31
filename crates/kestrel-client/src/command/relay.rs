use std::time::Duration;

use anyhow::{Context as _, Result, anyhow, bail};
use kestrel_operator_types as wire;

use super::input::{self, interactive};
use super::login::held;
use crate::api::ControlPlane;
use crate::diagnostic;
use crate::exit::{Exit, Failed};
use crate::sse::Events;

const PAUSE: Duration = Duration::from_secs(1);
/// Unanswered reads in a row before the Client stops following; the relay runs on regardless.
const PATIENCE: u32 = 10;

/// Starts one relay and follows it to its end; a lost answer is read again, never started again.
pub(super) async fn signed_in(
    api: &ControlPlane,
    organization: &str,
    harness: &str,
    method: &wire::SignInMethod,
    kind: &wire::SignInMethodRelay,
) -> Result<wire::SavedSignIn> {
    if *kind == wire::SignInMethodRelay::ClaudeSetupToken && !interactive() {
        bail!(Failed::new(
            Exit::Usage,
            format!(
                "{} takes its code at a terminal, and stdin and stderr are not both one; run it at one, over SSH if need be, or import a token from `claude setup-token` with --method {} --token",
                method.name, method.id
            )
        ));
    }
    let start = [
        "organizations",
        organization,
        "sign-ins",
        harness,
        &method.id,
        "relay",
    ];
    let relay: wire::SignInRelay = api
        .post_as(&start, &wire::SignInRelayRequest { profile: None })
        .await?;
    eprintln!(
        "Signing in to {harness} by {}, to be held in {}. The relay {} ends by {}.",
        method.id,
        held(&relay.destination),
        relay.id,
        relay.deadline
    );
    let mut following = Following {
        api,
        relay: start
            .iter()
            .map(|&segment| segment.to_owned())
            .chain([relay.id.clone()])
            .collect(),
        said: Vec::new(),
        version: i64::MIN,
    };
    following.until_done(relay).await
}

struct Following<'a> {
    api: &'a ControlPlane,
    relay: Vec<String>,
    said: Vec<&'static str>,
    version: i64,
}

enum Followed {
    Done(Box<wire::SavedSignIn>),
    Dropped(Option<anyhow::Error>),
}

/// Only a failure to hear an answer is worth reading again; a refusal is the relay's answer.
fn transient(error: &anyhow::Error) -> bool {
    Exit::of(error) == Exit::Unavailable
}

impl Following<'_> {
    fn at(&self, tail: &[&'static str]) -> Vec<&str> {
        self.relay
            .iter()
            .map(String::as_str)
            .chain(tail.iter().copied())
            .collect()
    }

    fn first(&mut self, state: &'static str) -> bool {
        if self.said.contains(&state) {
            return false;
        }
        self.said.push(state);
        true
    }

    async fn until_done(&mut self, started: wire::SignInRelay) -> Result<wire::SavedSignIn> {
        if let Some(saved) = self.observed(started).await? {
            return Ok(saved);
        }
        let mut unanswered = 0;
        loop {
            let mut lost = match self.followed().await? {
                Followed::Done(saved) => return Ok(*saved),
                Followed::Dropped(lost) => lost,
            };
            match self.api.get_as::<wire::SignInRelay>(&self.at(&[])).await {
                Ok(relay) => {
                    unanswered = 0;
                    if let Some(saved) = self.observed(relay).await? {
                        return Ok(saved);
                    }
                }
                Err(error) if transient(&error) => {
                    unanswered += 1;
                    lost = Some(error);
                }
                Err(error) => return Err(error),
            }
            if unanswered >= PATIENCE {
                return Err(lost.unwrap_or_else(|| {
                    anyhow!(Failed::new(
                        Exit::Unavailable,
                        "the control plane stopped answering about the sign-in relay, which runs on until its deadline"
                    ))
                }));
            }
            tokio::time::sleep(PAUSE).await;
        }
    }

    async fn followed(&mut self) -> Result<Followed> {
        let response = match self.api.get_response(&self.at(&["events"]), &[]).await {
            Ok(response) => response,
            Err(error) if transient(&error) => return Ok(Followed::Dropped(Some(error))),
            Err(error) => return Err(error),
        };
        let mut events = Events::over(response);
        loop {
            let event = match events.next().await {
                Ok(Some(event)) => event,
                Ok(None) => return Ok(Followed::Dropped(None)),
                Err(error) => return Ok(Followed::Dropped(Some(anyhow!(error)))),
            };
            let Some(decoded) = event
                .name
                .as_deref()
                .and_then(|name| wire::RelayEvent::from_sse(name, &event.data))
            else {
                continue;
            };
            match decoded.context("reading the sign-in relay")? {
                wire::RelayEvent::SignInRelay(relay) => {
                    if let Some(saved) = self.observed(relay).await? {
                        return Ok(Followed::Done(Box::new(saved)));
                    }
                }
                wire::RelayEvent::RelayEnd(_) => return Ok(Followed::Dropped(None)),
            }
        }
    }

    /// Each state is told once, and a URL or code always on a line of its own to copy.
    async fn observed(&mut self, relay: wire::SignInRelay) -> Result<Option<wire::SavedSignIn>> {
        use wire::SignInRelayProgress as P;
        let mut relay = relay;
        loop {
            if relay.version <= self.version {
                return Ok(None);
            }
            self.version = relay.version;
            match relay.progress {
                P::RelayStarting(_) => {
                    if self.first("starting") {
                        eprintln!("Starting the vendor's sign-in…");
                    }
                }
                P::RelayAwaitingBrowser(waiting) => {
                    if self.first("awaiting_browser") {
                        eprintln!("Open this link in a browser, on any machine:");
                        eprintln!("{}", waiting.url);
                        if let Some(code) = &waiting.code {
                            eprintln!("and enter this one-time code:");
                            eprintln!("{code}");
                        }
                        eprintln!("Waiting for the vendor to approve it…");
                    }
                }
                P::RelayAwaitingCode(waiting) => {
                    if self.first("awaiting_code") {
                        eprintln!("Open this link in a browser, on any machine, and sign in:");
                        eprintln!("{}", waiting.url);
                        let code =
                            input::hidden_line("Paste the code the page shows and press Enter: ")?;
                        if code.trim().is_empty() {
                            bail!(Failed::new(
                                Exit::Usage,
                                "no code was given; the relay waits for one until its deadline"
                            ));
                        }
                        relay = self
                            .api
                            .post_as(&self.at(&["code"]), &wire::SignInRelayCode { code })
                            .await?;
                        continue;
                    }
                }
                P::RelaySaving(_) => {
                    if self.first("saving") {
                        eprintln!("Saving the sign-in…");
                    }
                }
                P::RelayCompleted(completed) => return Ok(Some(completed.sign_in)),
                P::RelayFailed(failed) => {
                    let error = anyhow!(diagnostic::carried(failed.diagnostic));
                    return Err(match failed.sign_in {
                        Some(saved) => diagnostic::after_save(
                            error,
                            &format!(
                                "Revision {} was saved in {}",
                                saved.revision,
                                held(&saved.holding)
                            ),
                        ),
                        None => error,
                    });
                }
                P::RelayExpired(_) => bail!(Failed::new(
                    Exit::Rejected,
                    format!(
                        "the sign-in relay {} reached its deadline {} unfinished, and nothing was saved",
                        relay.id, relay.deadline
                    )
                )),
                P::RelayCancelled(_) => bail!(Failed::new(
                    Exit::Rejected,
                    format!(
                        "the sign-in relay {} was cancelled, by an import or another Client, and saved nothing",
                        relay.id
                    )
                )),
            }
            return Ok(None);
        }
    }
}
