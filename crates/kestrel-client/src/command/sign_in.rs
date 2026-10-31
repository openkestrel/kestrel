use std::future::Future;
use std::time::Duration;

use anyhow::{Result, bail};
use clap::Subcommand;
use kestrel_operator_types as wire;
use serde_json::json;

use super::Running;
use crate::diagnostic;
use crate::exit::{Exit, Failed};
use crate::typed::{self, Column, Columns, or_absent, text};

const OPERATION: &str = "testSignIn";
const PAUSE: Duration = Duration::from_secs(1);
/// Unanswered reads in a row before the Client stops waiting; the test runs on regardless.
const PATIENCE: u32 = 30;

#[derive(Debug, Subcommand)]
pub(crate) enum SignInCommand {
    /// Test saved material with one prompt to a model, which spends model usage; saving never
    /// does
    #[command(
        after_help = "The test runs once. A lost answer is read again by the test's id; nothing sends another prompt but running this command again."
    )]
    Test {
        /// The harness whose saved material is tested
        harness: String,
        /// The Sign-in Method whose saved material is tested
        #[arg(long, value_name = "ID")]
        method: String,
        /// The model to test, as PROVIDER/MODEL where the method names providers; without it,
        /// the harness's default
        #[arg(long, value_name = "MODEL")]
        model: Option<String>,
    },
}

pub(crate) async fn run(running: Running<'_>, command: SignInCommand) -> Result<()> {
    let SignInCommand::Test {
        harness,
        method,
        model,
    } = command;
    let organization = running.scoping.resolve().await?.organization;
    let catalogued: wire::SignInMethod = running
        .api
        .get_as(&["harnesses", &harness, "sign-in-methods", &method])
        .await?;
    model_named(&catalogued, model.as_deref()).map_err(|why| Failed::new(Exit::Usage, why))?;
    eprintln!(
        "This sends one prompt through {harness} with the saved {} to {}, which spends model usage.",
        catalogued.name,
        model.as_deref().unwrap_or("the harness's default model")
    );
    let tests = [
        "organizations",
        &organization,
        "sign-ins",
        &harness,
        &method,
        "tests",
    ];
    let started: wire::SignInTest = running
        .api
        .post_as(&tests, &wire::SignInTestRequest { model })
        .await?;
    eprintln!(
        "Testing revision {} as test {}…",
        started.revision, started.id
    );
    let id = started.id.clone();
    let known: Vec<&str> = tests.iter().copied().chain([id.as_str()]).collect();
    let ended = awaited(started, || running.api.get_as(&known), PAUSE)
        .await
        .map_err(|error| diagnostic::after_save(error, &format!("The test {id} was started")))?;

    if ended.outcome == Some(wire::SignInTestOutcome::Worked) {
        return typed::show(running.presentation, &TESTED, &ended);
    }
    bail!(failed(&organization, ended))
}

fn model_named(method: &wire::SignInMethod, model: Option<&str>) -> Result<(), String> {
    let Some(providers) = &method.test_providers else {
        return Ok(());
    };
    match model.and_then(|model| model.split_once('/')) {
        Some((provider, name)) if !name.is_empty() && providers.iter().any(|p| p == provider) => {
            Ok(())
        }
        _ => Err(format!(
            "{} tests the model named by --model PROVIDER/MODEL, the provider one of {}; nothing was tested",
            method.id,
            providers.join(", ")
        )),
    }
}

/// Reads the one known test until it ends; nothing here can start another.
async fn awaited<F, Fut>(
    mut test: wire::SignInTest,
    mut read: F,
    pause: Duration,
) -> Result<wire::SignInTest>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<wire::SignInTest>>,
{
    let mut unanswered = 0;
    let mut told = false;
    loop {
        if let (false, Some(model)) = (told, &test.model) {
            eprintln!("The harness resolved the model {model}; prompting it…");
            told = true;
        }
        if matches!(
            test.state,
            wire::SignInTestState::Completed
                | wire::SignInTestState::Failed
                | wire::SignInTestState::Interrupted
        ) {
            return Ok(test);
        }
        tokio::time::sleep(pause).await;
        match read().await {
            Ok(read) => {
                unanswered = 0;
                test = read;
            }
            Err(error) if Exit::of(&error) == Exit::Unavailable && unanswered + 1 < PATIENCE => {
                unanswered += 1;
            }
            Err(error) => return Err(error),
        }
    }
}

fn failed(organization: &str, test: wire::SignInTest) -> Failed {
    use wire::SignInTestOutcome as O;
    let ended = test.outcome.as_ref().map_or(test.state.as_str(), O::as_str);
    let message = format!(
        "the test {} of {} {} revision {} ended {ended}{}; it is not run again by itself",
        test.id,
        test.harness,
        test.method,
        test.revision,
        test.message
            .as_deref()
            .map_or_else(String::new, |said| format!(": {said}"))
    );
    match test.outcome {
        None | Some(O::Unavailable | O::HarnessFailed) => Failed::diagnosed(
            Exit::Unavailable,
            wire::Diagnostic::UnavailableDiagnostic(wire::UnavailableDiagnostic {
                kind: json!("unavailable"),
                message,
                field: None,
                context: wire::UnavailableContext {
                    service: "work role".to_owned(),
                    operation: OPERATION.to_owned(),
                    resource: Some(test.id),
                    retry_after_seconds: None,
                    saved: None,
                },
                next_steps: Vec::new(),
            }),
        ),
        Some(outcome) => {
            let next_steps = match outcome {
                O::AuthenticationFailed | O::MaterialReplaced => {
                    vec![wire::Action::SignInAction(wire::SignInAction {
                        action: json!("sign_in"),
                        harness: Some(test.harness.clone()),
                        method: Some(test.method.clone()),
                        sign_in: None,
                    })]
                }
                _ => Vec::new(),
            };
            Failed::diagnosed(
                Exit::Rejected,
                wire::Diagnostic::StateConflictDiagnostic(wire::StateConflictDiagnostic {
                    kind: json!("state_conflict"),
                    message,
                    field: None,
                    context: wire::StateConflictContext {
                        operation: OPERATION.to_owned(),
                        resource: wire::Resource::SignInTest,
                        reference: test.id,
                        organization: Some(organization.to_owned()),
                        state: outcome.as_str().to_owned(),
                        holding_session: None,
                    },
                    next_steps,
                }),
            )
        }
    }
}

const TESTED: Columns<wire::SignInTest> = Columns::Detail(&[
    Column("test", |test| text(&test.id)),
    Column("harness", |test| text(&test.harness)),
    Column("method", |test| text(&test.method)),
    Column("revision", |test| text(test.revision)),
    Column("model", |test| or_absent(test.model.as_ref())),
    Column("image", |test| or_absent(test.image.as_ref())),
    Column("outcome", |test| or_absent(test.outcome.as_ref())),
    Column("evidence", |test| {
        format!(
            "model use worked for {} {} on revision {}",
            test.harness,
            test.model.as_deref().unwrap_or("its default model"),
            test.revision
        )
    }),
]);

#[cfg(test)]
mod tests;
