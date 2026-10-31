use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use clap::Args;
use kestrel_operator_types as wire;

use super::input::{self, interactive};
use super::{Running, relay};
use crate::api::ControlPlane;
use crate::exit::{Exit, Failed};
use crate::output::Presentation;
use crate::typed::{self, Column, Columns, text};

#[derive(Debug, Args)]
#[command(
    after_help = "Without --token or --file, a subscription signs in through its vendor and a key is read unechoed or from standard input. No secret is ever an argument. Saving checks a key with the provider where one can be asked, and never sends a prompt; `kestrel sign-in test` does."
)]
pub(crate) struct Login {
    /// The harness to sign in to
    harness: String,
    /// The Sign-in Method; a terminal is asked without it, and anything else needs it unless the
    /// harness and --token or --file leave one
    #[arg(long, value_name = "ID")]
    method: Option<String>,
    /// Import a token or key already issued, read unechoed at a terminal or from standard input
    #[arg(long, conflicts_with = "file")]
    token: bool,
    /// Import a login file already written, read from this path
    #[arg(long, value_name = "PATH")]
    file: Option<PathBuf>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Any,
    Token,
    File,
}

impl Mode {
    fn admits(self, method: &wire::SignInMethod) -> bool {
        match self {
            Mode::Any => true,
            Mode::Token => method.input == wire::SignInMethodInput::Token,
            Mode::File => method.input == wire::SignInMethodInput::File,
        }
    }
}

#[derive(Debug)]
enum Chosen<'a> {
    Method(&'a wire::SignInMethod),
    Ask(Vec<&'a wire::SignInMethod>),
}

pub(crate) async fn run(running: Running<'_>, login: Login) -> Result<()> {
    let Running {
        api,
        presentation,
        scoping,
        ..
    } = running;
    let organization = scoping.resolve().await?.organization;
    let catalogue: wire::ListHarnessesResponse = api.get_as(&["harnesses"]).await?;
    let Some(harness) = catalogue.iter().find(|entry| entry.name == login.harness) else {
        let names: Vec<&str> = catalogue.iter().map(|entry| entry.name.as_str()).collect();
        bail!(Failed::new(
            Exit::Usage,
            format!(
                "{} has no guided sign-in; choose one of {}, or hold material with `kestrel profile set`",
                login.harness,
                names.join(", ")
            ),
        ));
    };
    let mode = match (login.token, &login.file) {
        (true, _) => Mode::Token,
        (false, Some(_)) => Mode::File,
        (false, None) => Mode::Any,
    };
    let method = match chosen(harness, login.method.as_deref(), mode, interactive())
        .map_err(|why| Failed::new(Exit::Usage, why))?
    {
        Chosen::Method(method) => method,
        Chosen::Ask(candidates) => asked(harness, &candidates, mode)?,
    };
    let json = presentation.json();
    if !json && harness.name == "claude" && method.kind == wire::SignInMethodKind::Subscription {
        eprintln!(
            "A Claude plan serves only its owner's own Kestrel work; use an Anthropic API key for anyone else's."
        );
    }

    let saved = match (mode, &method.relay, &login.file) {
        (Mode::Any, Some(kind), _) => {
            relay::signed_in(api, &organization, &harness.name, method, kind).await?
        }
        (_, _, Some(path)) => {
            let value = from_file(path)?;
            saved(api, &organization, &harness.name, method, value).await?
        }
        (_, _, None) if method.input == wire::SignInMethodInput::File => bail!(Failed::new(
            Exit::Usage,
            format!(
                "{} is imported from a login file; name it with --file PATH",
                method.id
            )
        )),
        (_, _, None) => {
            let value = token(method)?;
            saved(api, &organization, &harness.name, method, value).await?
        }
    };
    shown(presentation, &saved)
}

fn chosen<'a>(
    harness: &'a wire::HarnessCatalogueEntry,
    named: Option<&str>,
    mode: Mode,
    interactive: bool,
) -> Result<Chosen<'a>, String> {
    let methods = &harness.sign_in_methods;
    if let Some(named) = named {
        let Some(method) = methods.iter().find(|method| method.id == named) else {
            return Err(format!(
                "{} has no Sign-in Method {named}; it offers {}",
                harness.name,
                offered(methods.iter())
            ));
        };
        return match (mode, &method.input) {
            (Mode::Token, wire::SignInMethodInput::File) => Err(format!(
                "{named} is imported from a login file; name it with --file PATH rather than --token"
            )),
            (Mode::File, wire::SignInMethodInput::Token) => Err(format!(
                "{named} takes a token or key; import it with --token, which reads it unechoed or from standard input, rather than --file"
            )),
            _ => Ok(Chosen::Method(method)),
        };
    }

    let candidates: Vec<&wire::SignInMethod> = methods
        .iter()
        .filter(|method| mode.admits(method))
        .collect();
    match (candidates.as_slice(), interactive) {
        ([], _) => Err(format!(
            "{} has no Sign-in Method that takes a {}; it offers {}",
            harness.name,
            if mode == Mode::File {
                "login file"
            } else {
                "token"
            },
            offered(methods.iter())
        )),
        (_, true) => Ok(Chosen::Ask(candidates)),
        ([only], false) => Ok(Chosen::Method(only)),
        (_, false) => Err(format!(
            "name the Sign-in Method with --method, since nothing here can be asked which: {} offers {}",
            harness.name,
            offered(candidates.into_iter())
        )),
    }
}

fn offered<'a>(methods: impl Iterator<Item = &'a wire::SignInMethod>) -> String {
    methods
        .map(|method| format!("{} ({}, {})", method.id, method.name, belongs(method)))
        .collect::<Vec<_>>()
        .join("; ")
}

/// Where the material is held, which the catalogue decides and nobody is asked.
fn belongs(method: &wire::SignInMethod) -> String {
    match method.ownership {
        wire::SignInMethodOwnership::Operator => "held in your own Profile".to_owned(),
        wire::SignInMethodOwnership::Organization => {
            format!("held by the Organization as {}", filled(&method.fills))
        }
    }
}

fn filled(fills: &wire::SignInFill) -> &str {
    match fills {
        wire::SignInFill::SignInVariableFill(fill) => &fill.variable,
        wire::SignInFill::SignInFileFill(fill) => &fill.path,
    }
}

fn how(method: &wire::SignInMethod, mode: Mode) -> &'static str {
    match (mode, &method.relay, &method.input) {
        (Mode::Any, Some(_), _) => "by signing in at a link you open in any browser",
        (_, _, wire::SignInMethodInput::File) => "from a login file",
        _ => "by pasting it",
    }
}

fn asked<'a>(
    harness: &wire::HarnessCatalogueEntry,
    candidates: &[&'a wire::SignInMethod],
    mode: Mode,
) -> Result<&'a wire::SignInMethod> {
    eprintln!("Sign in to {} with:", harness.name);
    for (number, method) in candidates.iter().enumerate() {
        eprintln!(
            "  {}. {} ({}), {}, {}",
            number + 1,
            method.name,
            method.id,
            belongs(method),
            how(method, mode)
        );
    }
    let answer = input::line(&format!("Choose 1-{}: ", candidates.len()))?;
    let answer = answer.trim();
    let picked = match (answer, candidates) {
        ("", [only]) => Some(*only),
        (answer, _) => answer
            .parse::<usize>()
            .ok()
            .and_then(|number| number.checked_sub(1))
            .and_then(|index| candidates.get(index).copied()),
    };
    picked.ok_or_else(|| {
        Failed::new(
            Exit::Usage,
            format!(
                "{answer:?} is none of 1-{}; nothing was saved",
                candidates.len()
            ),
        )
        .into()
    })
}

fn token(method: &wire::SignInMethod) -> Result<String> {
    let value = if interactive() {
        if let Some(console) = &method.console_url {
            eprintln!("Get a key at:\n{console}");
        }
        input::hidden_line(&format!("Paste the {} and press Enter: ", method.name))?
    } else {
        input::without_line_end(input::standard_input()?)
    };
    if value.trim().is_empty() {
        bail!(Failed::new(
            Exit::Usage,
            format!("no {} was given, so nothing was saved", method.name)
        ));
    }
    Ok(value)
}

/// A login file is held byte for byte, because it is handed back exactly as it was held.
fn from_file(path: &Path) -> Result<String> {
    let shown = path.display();
    let bytes = std::fs::read(path)
        .map_err(|error| Failed::new(Exit::Usage, format!("{shown} could not be read: {error}")))?;
    let value = String::from_utf8(bytes)
        .map_err(|_| Failed::new(Exit::Usage, format!("{shown} is not UTF-8 text")))?;
    if value.trim().is_empty() {
        bail!(Failed::new(
            Exit::Usage,
            format!("{shown} is empty, so nothing was saved")
        ));
    }
    Ok(value)
}

async fn saved(
    api: &ControlPlane,
    organization: &str,
    harness: &str,
    method: &wire::SignInMethod,
    value: String,
) -> Result<wire::SavedSignIn> {
    api.put_as(
        &[
            "organizations",
            organization,
            "sign-ins",
            harness,
            &method.id,
        ],
        &wire::SignInMaterial { value },
    )
    .await
}

pub(super) fn shown(presentation: &Presentation, saved: &wire::SavedSignIn) -> Result<()> {
    if !presentation.json()
        && saved.authentication.state == wire::AuthenticationEvidenceState::Unchecked
    {
        eprintln!(
            "Saved unchecked: nothing can confirm this {} without a model call. `kestrel sign-in test {} --method {}` makes one.",
            saved.kind, saved.harness, saved.method
        );
    }
    typed::show(presentation, &SAVED, saved)
}

pub(super) fn held(holding: &wire::SignInHolding) -> String {
    match holding.resource {
        wire::SignInHoldingResource::SubscriptionProfile => {
            format!(
                "your own Profile {}, as {}",
                holding.reference,
                filled(&holding.fills)
            )
        }
        wire::SignInHoldingResource::ProviderCredential => {
            format!(
                "the Organization's Provider Credential {}",
                holding.reference
            )
        }
    }
}

const SAVED: Columns<wire::SavedSignIn> = Columns::Detail(&[
    Column("harness", |saved| text(&saved.harness)),
    Column("method", |saved| text(&saved.method)),
    Column("kind", |saved| text(&saved.kind)),
    Column("held in", |saved| held(&saved.holding)),
    Column("revision", |saved| text(saved.revision)),
    Column("authentication", |saved| {
        format!(
            "{} ({})",
            saved.authentication.state, saved.authentication.source
        )
    }),
    Column("model use", |saved| {
        if saved.model_use.is_empty() {
            return "none recorded".to_owned();
        }
        saved
            .model_use
            .iter()
            .map(|used| format!("{} {}", used.model, used.result))
            .collect::<Vec<_>>()
            .join(", ")
    }),
]);

#[cfg(test)]
mod tests;
