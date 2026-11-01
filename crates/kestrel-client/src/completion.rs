use std::ffi::{OsStr, OsString};
use std::io;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result};
use clap::{CommandFactory as _, ValueEnum};
use clap_complete::engine::{ArgValueCompleter, CompletionCandidate};
use clap_complete::env::Shells;
use kestrel_operator_types as wire;

use crate::Client;
use crate::api::ControlPlane;
use crate::scope::{Scope, Scoping, Source};

#[derive(Clone, Copy, Debug, ValueEnum)]
pub(crate) enum Shell {
    Bash,
    Zsh,
    Fish,
}

pub(crate) fn install(shell: Shell) -> Result<()> {
    let name = shell.to_possible_value().expect("a shell name");
    let shells = Shells::builtins();
    let engine = shells
        .completer(name.get_name())
        .expect("a supported shell");
    engine.write_registration(
        "COMPLETE",
        "kestrel",
        "kestrel",
        "kestrel",
        &mut io::stdout(),
    )?;
    Ok(())
}

pub(crate) fn dispatch() -> bool {
    let args: Vec<OsString> = std::env::args_os().collect();
    let line: Vec<OsString> = args
        .iter()
        .position(|arg| arg == "--")
        .map(|index| args[index + 1..].to_vec())
        .unwrap_or_default();
    clap_complete::CompleteEnv::with_factory(|| command(&line))
        .try_complete(args, std::env::current_dir().ok().as_deref())
        .unwrap_or(true)
}

#[derive(Clone, Copy)]
enum Names {
    Workspace,
    Agent,
    Profile,
    Harness,
    Method,
}

struct Selection {
    matches: Option<clap::ArgMatches>,
    deadline: Instant,
}

impl Selection {
    fn value(&self, name: &str) -> Option<String> {
        let mut here = self.matches.as_ref()?;
        let mut value = None;
        loop {
            if let Ok(Some(found)) = here.try_get_one::<String>(name) {
                value = Some(found.clone());
            }
            match here.subcommand() {
                Some((_, deeper)) => here = deeper,
                None => return value,
            }
        }
    }

    fn complete(&self, names: Names, current: &OsStr) -> Vec<CompletionCandidate> {
        let Some(remaining) = self.deadline.checked_duration_since(Instant::now()) else {
            return Vec::new();
        };
        let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        else {
            return Vec::new();
        };
        let result =
            runtime.block_on(async { tokio::time::timeout(remaining, self.names(names)).await });
        // DNS may still occupy a blocking worker after the remote budget expires.
        runtime.shutdown_background();
        let prefix = current.to_string_lossy();
        result
            .ok()
            .and_then(Result::ok)
            .unwrap_or_default()
            .into_iter()
            .filter(|name| name.starts_with(prefix.as_ref()))
            .map(CompletionCandidate::new)
            .collect()
    }

    async fn harness(&self, api: &ControlPlane, organization: &str) -> Result<Option<String>> {
        if let Some(harness) = self.value("harness") {
            return Ok(Some(harness));
        }
        let agent = match self.value("agent") {
            Some(agent) => agent,
            None => {
                let Some(workspace) = self.value("workspace") else {
                    return Ok(None);
                };
                let mut sessions: wire::ListSessionsResponse = api
                    .get_as(&[
                        "organizations",
                        organization,
                        "workspaces",
                        &workspace,
                        "sessions",
                    ])
                    .await?;
                if let Some(session) = sessions.pop() {
                    return Ok(Some(session.harness));
                }
                let workspace: wire::Workspace = api
                    .get_as(&["organizations", organization, "workspaces", &workspace])
                    .await?;
                workspace.opened_with
            }
        };
        let agents: wire::ListAgentsResponse = api
            .get_as(&["organizations", organization, "agents"])
            .await?;
        Ok(agents
            .into_iter()
            .find(|item| item.name == agent)
            .map(|item| item.harness))
    }

    async fn names(&self, names: Names) -> Result<Vec<String>> {
        let url = self
            .value("control_plane")
            .context("completion has no selected connection")?
            .parse()?;
        let api = ControlPlane::at(url, "completion");
        let named = self.value("organization").map(|organization| Scope {
            organization,
            source: Source::Flag,
        });
        let organization = Scoping::new(&api, named).resolve().await?.organization;
        let path = ["organizations", &organization];
        Ok(match names {
            Names::Workspace => api
                .get_as::<wire::ListWorkspacesResponse>(&[path[0], path[1], "workspaces"])
                .await?
                .into_iter()
                .map(|item| item.name)
                .collect(),
            Names::Agent => api
                .get_as::<wire::ListAgentsResponse>(&[path[0], path[1], "agents"])
                .await?
                .into_iter()
                .map(|item| item.name)
                .collect(),
            Names::Profile => api
                .get_as::<wire::ListSubscriptionProfilesResponse>(&[path[0], path[1], "profiles"])
                .await?
                .into_iter()
                .map(|item| item.name)
                .collect(),
            Names::Harness | Names::Method => {
                let harness = if matches!(names, Names::Method) {
                    self.harness(&api, &organization).await?
                } else {
                    None
                };
                let catalogue: wire::ListHarnessesResponse = api.get_as(&["harnesses"]).await?;
                if matches!(names, Names::Harness) {
                    catalogue.into_iter().map(|item| item.name).collect()
                } else {
                    catalogue
                        .into_iter()
                        .filter(|item| Some(&item.name) == harness.as_ref())
                        .flat_map(|item| item.sign_in_methods.into_iter().map(|method| method.id))
                        .collect()
                }
            }
        })
    }
}

fn command(line: &[OsString]) -> clap::Command {
    let selection = Arc::new(Selection {
        matches: if line.is_empty() {
            None
        } else {
            Client::command()
                .ignore_errors(true)
                .try_get_matches_from(line)
                .ok()
        },
        deadline: Instant::now() + Duration::from_millis(800),
    });
    decorate(Client::command(), "", &selection)
}

fn decorate(mut command: clap::Command, parent: &str, selection: &Arc<Selection>) -> clap::Command {
    let owner = if parent.is_empty() {
        command.get_name()
    } else {
        parent
    }
    .to_owned();
    let command_name = command.get_name().to_owned();
    command = command.mut_args(|arg| {
        let names = match arg.get_id().as_str() {
            "workspace" | "continues" | "on_open_workspace" => Some(Names::Workspace),
            "agent" | "allows" => Some(Names::Agent),
            "profile" => Some(Names::Profile),
            "harness" => Some(Names::Harness),
            "method" | "sign_in" => Some(Names::Method),
            "name" if owner == "agent" && command_name != "declare" => Some(Names::Agent),
            "name" if owner == "profile" && command_name != "declare" => Some(Names::Profile),
            _ => None,
        };
        if let Some(names) = names {
            let selection = Arc::clone(selection);
            arg.add(ArgValueCompleter::new(move |current: &OsStr| {
                selection.complete(names, current)
            }))
        } else {
            arg
        }
    });
    let children: Vec<_> = command
        .get_subcommands()
        .map(|child| child.get_name().to_owned())
        .collect();
    for child in children {
        let parent = if owner == "kestrel" {
            child.as_str()
        } else {
            owner.as_str()
        };
        command = command.mut_subcommand(&child, |command| decorate(command, parent, selection));
    }
    command
}
