//! One prompt to a harness handed only what the probe names, said on stdout: no link is
//! dialled, nothing is checked out and nothing is carried afterwards.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use crate::Diagnostics;
use crate::evidence::Secrets;
use crate::harness::{Conversation, ConversationEvent, Harness, Turn};
use crate::link::{Evidence, Report, SessionInfo, SessionOptionKind};
use crate::login;

pub const VARIABLE: &str = "KESTREL_PROBE";

#[derive(Deserialize)]
struct Asked {
    command: String,
    #[serde(default)]
    auth: Option<String>,
    #[serde(default)]
    model: Option<String>,
    prompt: String,
    #[serde(default)]
    variables: BTreeMap<String, String>,
    #[serde(default)]
    files: BTreeMap<String, String>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "probe", rename_all = "snake_case")]
enum Said {
    /// Said before the prompt is sent, so the model is known before anything is spent on it.
    Ready { model: Option<String> },
    Finished {
        model: Option<String>,
        outcome: Outcome,
        because: Option<String>,
        refreshed: BTreeMap<String, String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum Outcome {
    Answered,
    AuthenticationRequired,
    ModelNotOffered,
    Failed,
}

struct Finding {
    model: Option<String>,
    outcome: Outcome,
    because: Option<String>,
}

impl Finding {
    fn failed(model: Option<String>, because: impl Into<String>) -> Self {
        Self {
            model,
            outcome: Outcome::Failed,
            because: Some(because.into()),
        }
    }
}

struct Selectable {
    id: String,
    current: String,
    values: Vec<String>,
}

pub async fn run(asked: &str, home: Option<&Path>, diagnostics: &dyn Diagnostics) -> i32 {
    let asked: Asked = match serde_json::from_str(asked) {
        Ok(asked) => asked,
        Err(error) => {
            diagnostics.info(&format!("{VARIABLE} did not read: {error}"));
            return 2;
        }
    };

    let secrets = Secrets::of(asked.variables.values().chain(asked.files.values()));
    let written = match (asked.files.is_empty(), home) {
        (true, _) => None,
        (false, Some(home)) => match login::write(home, asked.files.clone()) {
            Ok(written) => Some(written),
            Err(error) => {
                say(&Said::Finished {
                    model: asked.model.clone(),
                    outcome: Outcome::Failed,
                    because: Some(format!("the login files could not be written: {error}")),
                    refreshed: BTreeMap::new(),
                });
                return 0;
            }
        },
        (false, None) => {
            say(&Said::Finished {
                model: asked.model.clone(),
                outcome: Outcome::Failed,
                because: Some("this environment has no home to put login files in".to_owned()),
                refreshed: BTreeMap::new(),
            });
            return 0;
        }
    };

    let (stderr, mut muttered) = mpsc::unbounded_channel();
    // Read and dropped, since what a harness writes about a login is not this probe's to repeat.
    let draining = tokio::spawn(async move { while muttered.recv().await.is_some() {} });
    let harness = Harness {
        command: asked.command.clone(),
        auth: asked.auth.clone().filter(|method| !method.is_empty()),
        model: None,
        mode: None,
        thought_level: None,
        interrupt_deadline: crate::INTERRUPT_DEADLINE,
        quiet_period: crate::QUIET_PERIOD,
        recover: false,
        stderr,
    };
    let root = std::env::current_dir().unwrap_or_else(|_| std::env::temp_dir());
    let mut conversation =
        Conversation::open(&harness, asked.variables.clone(), secrets, None, root);

    let finding = probed(&mut conversation, &asked).await;
    conversation.end().await;
    draining.abort();
    let refreshed = written
        .as_ref()
        .map(login::Written::refreshed)
        .unwrap_or_default();
    if let Some(written) = written {
        written.remove();
    }
    say(&Said::Finished {
        model: finding.model,
        outcome: finding.outcome,
        because: finding.because,
        refreshed,
    });

    0
}

async fn probed(conversation: &mut Conversation, asked: &Asked) -> Finding {
    let mut selectable = None;
    let mut resolved = asked.model.clone();
    let mut prompted = false;

    loop {
        match conversation.next().await {
            ConversationEvent::Report(Report::SessionInfo(info)) => {
                selectable = model_option(&info);
            }
            ConversationEvent::Ready if !prompted => {
                match (&asked.model, &selectable) {
                    (Some(named), None) => {
                        return Finding::failed(
                            Some(named.clone()),
                            format!(
                                "this harness lets no client select a model, and {named} was named"
                            ),
                        );
                    }
                    (Some(named), Some(offered)) if !offered.values.contains(named) => {
                        return Finding {
                            model: Some(named.clone()),
                            outcome: Outcome::ModelNotOffered,
                            because: Some(format!(
                                "this harness does not offer the model {named} with this login"
                            )),
                        };
                    }
                    (Some(named), Some(offered)) if offered.current != *named => {
                        let set = conversation
                            .set_option(offered.id.clone(), named.clone())
                            .await;
                        if let Some(refused) = set.refused {
                            return Finding::failed(Some(named.clone()), refused);
                        }
                    }
                    (Some(_), Some(_)) => {}
                    (None, offered) => {
                        resolved = offered.as_ref().map(|offered| offered.current.clone());
                    }
                }
                say(&Said::Ready {
                    model: resolved.clone(),
                });
                conversation.prompt(Turn {
                    seq: 1,
                    prompt: asked.prompt.clone(),
                });
                prompted = true;
            }
            ConversationEvent::Worked(worked) => {
                let model = resolved.or(worked.on.map(|on| on.model));
                return match worked.failed {
                    None => Finding {
                        model,
                        outcome: Outcome::Answered,
                        because: None,
                    },
                    Some(failure) => Finding {
                        model,
                        outcome: match failure.evidence {
                            Some(Evidence::AuthenticationRequired { .. }) => {
                                Outcome::AuthenticationRequired
                            }
                            _ => Outcome::Failed,
                        },
                        because: Some(failure.because),
                    },
                };
            }
            _ => {}
        }
    }
}

fn model_option(info: &SessionInfo) -> Option<Selectable> {
    info.options.iter().find_map(|option| {
        if option.category.as_deref() != Some("model") {
            return None;
        }
        let SessionOptionKind::Select {
            current,
            values,
            groups,
        } = &option.kind
        else {
            return None;
        };

        Some(Selectable {
            id: option.id.clone(),
            current: current.clone(),
            values: values
                .iter()
                .chain(groups.iter().flat_map(|group| &group.values))
                .map(|value| value.value.clone())
                .collect(),
        })
    })
}

fn say(said: &Said) {
    println!(
        "{}",
        serde_json::to_string(said).expect("what a probe says serializes")
    );
}
