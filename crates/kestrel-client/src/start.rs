//! What only the Client can see for a start, the local clone and its own environment, and how
//! the control plane's plan reads on a terminal.

use std::path::{Path, PathBuf};
use std::process::Command;

use kestrel_operator_types as wire;

#[derive(Default)]
pub struct LocalClone {
    pub root: Option<PathBuf>,
    pub origin: Option<String>,
    pub default_branch: Option<String>,
    pub checked_out: Option<String>,
}

impl LocalClone {
    pub fn of(directory: &Path) -> Self {
        let git = |arguments: &[&str]| {
            Command::new("git")
                .arg("-C")
                .arg(directory)
                .args(arguments)
                .output()
                .ok()
                .filter(|output| output.status.success())
                .and_then(|output| String::from_utf8(output.stdout).ok())
                .map(|said| said.trim().to_owned())
                .filter(|said| !said.is_empty())
        };

        let Some(root) = git(&["rev-parse", "--show-toplevel"]) else {
            return Self::default();
        };
        Self {
            root: Some(PathBuf::from(root)),
            origin: git(&["remote", "get-url", "origin"]),
            default_branch: git(&[
                "symbolic-ref",
                "--quiet",
                "--short",
                "refs/remotes/origin/HEAD",
            ])
            .map(|branch| {
                branch
                    .strip_prefix("origin/")
                    .map_or(branch.clone(), str::to_owned)
            }),
            checked_out: git(&["symbolic-ref", "--quiet", "--short", "HEAD"]),
        }
    }

    /// The clone's facts as the plan takes them; nothing is sent without an origin.
    pub fn sent(&self) -> Option<wire::StartClone> {
        self.origin.as_ref().map(|origin| wire::StartClone {
            repository: origin.clone(),
            default_branch: self.default_branch.clone(),
            checked_out: self.checked_out.clone(),
        })
    }

    /// Why the clone supplied no repository, for the flag that would.
    pub fn silent(&self) -> Option<String> {
        match (&self.root, &self.origin) {
            (Some(root), None) => Some(format!(
                "the clone at {} has no origin remote",
                root.display()
            )),
            (None, _) => Some("the working directory is in no git clone".to_owned()),
            (Some(_), Some(_)) => None,
        }
    }
}

/// Out of the Client's own environment, so a secret is never in a shell history or in what
/// `ps` shows of this process. Each unset variable is named.
pub fn secrets(
    variables: &[String],
    read: impl Fn(&str) -> Option<String>,
) -> Result<Vec<(String, String)>, Vec<String>> {
    let mut unset = Vec::new();
    let mut secrets = Vec::new();
    for variable in variables {
        match read(variable).filter(|secret| !secret.is_empty()) {
            Some(secret) => secrets.push((variable.clone(), secret)),
            None => unset.push(format!("{variable} is not set in this environment")),
        }
    }

    if unset.is_empty() {
        Ok(secrets)
    } else {
        Err(unset)
    }
}

/// The flag a plan field is given by.
pub fn flag(field: &str) -> String {
    match field {
        "repositories" | "clone.repository" => "--repository".to_owned(),
        "credentials" => "--credential".to_owned(),
        "sign_in_method" => "--sign-in".to_owned(),
        field => {
            let record = field.split('.').next().unwrap_or(field);
            format!("--{}", record.replace('_', "-"))
        }
    }
}

/// Inferred values fold onto one line; a given value, and the sign-in, keep a line each.
pub fn review(preview: &wire::StartPreview) -> Vec<String> {
    let mut inferred = Vec::new();
    let mut given = Vec::new();
    let mut fold = |what: &str, flag: &str, value: Option<&wire::StartValue>, shown: String| {
        let Some(value) = value else { return };
        if value.source == wire::StartSource::Given {
            given.push(format!("  {flag:<14} {shown}"));
        } else if value.value.is_none() && value.source == wire::StartSource::Default {
            inferred.push(format!("{what} {shown}"));
        } else {
            inferred.push(format!("{what} {shown} ({})", because(value)));
        }
    };
    fold(
        "organization",
        "--organization",
        preview.organization.as_ref(),
        shown(preview.organization.as_ref()),
    );
    fold(
        "project",
        "--project",
        preview.project.as_ref(),
        shown(preview.project.as_ref()),
    );
    if let Some(repositories) = &preview.repositories {
        let listed = repositories
            .value
            .iter()
            .map(|repository| {
                if repository.given == repository.address {
                    repository.address.clone()
                } else {
                    format!("{} (from {})", repository.address, repository.given)
                }
            })
            .collect::<Vec<_>>()
            .join(", ");
        let value = wire::StartValue {
            value: None,
            source: repositories.source.clone(),
            record: repositories.record.clone(),
        };
        fold("repository", "--repository", Some(&value), listed);
    }
    fold(
        "base branch",
        "--branch",
        preview.branch.as_ref(),
        shown(preview.branch.as_ref()),
    );
    fold(
        "agent",
        "--agent",
        Some(&preview.agent),
        shown(Some(&preview.agent)),
    );
    fold(
        "harness",
        "--harness",
        Some(&preview.harness),
        shown(Some(&preview.harness)),
    );
    let model = preview
        .model
        .value
        .clone()
        .unwrap_or_else(|| "its harness's default".to_owned());
    fold("model", "--model", Some(&preview.model), model);
    if preview.profile.source == wire::StartSource::Given {
        fold(
            "profile",
            "--profile",
            Some(&preview.profile),
            shown(Some(&preview.profile)),
        );
    }

    let mut said = vec!["start".to_owned()];
    if !inferred.is_empty() {
        said.push(format!("  {:<14} {}", "inferred", inferred.join(" · ")));
    }
    said.extend(given);
    said.push(format!("  {:<14} {}", "sign-in", sign_in(preview)));
    said.push("already saved".to_owned());
    said.push(format!("  {}", saved(&preview.saved)));
    said.push("apply".to_owned());
    said.extend(
        applying(preview)
            .into_iter()
            .map(|step| format!("  {step}")),
    );

    said
}

fn shown(value: Option<&wire::StartValue>) -> String {
    value
        .and_then(|value| value.value.clone())
        .unwrap_or_else(|| "none".to_owned())
}

fn because(value: &wire::StartValue) -> String {
    let record = value.record.as_deref().unwrap_or_default();
    match value.source {
        wire::StartSource::Given => "given".to_owned(),
        wire::StartSource::CloneOrigin if value.record.is_some() => {
            format!("{record} declares the clone's origin")
        }
        wire::StartSource::CloneOrigin => "the clone's origin".to_owned(),
        wire::StartSource::CloneDefaultBranch => "origin's default".to_owned(),
        wire::StartSource::CloneCheckedOut => "checked out, and origin names no default".to_owned(),
        wire::StartSource::OnlyOrganization | wire::StartSource::OnlyAgent => {
            "the only one".to_owned()
        }
        wire::StartSource::Project => format!("as {record} declares"),
        wire::StartSource::Agent => format!("as {record} declares"),
        wire::StartSource::NamedForRepository => "named for its repository".to_owned(),
        wire::StartSource::NamedForHarness => "named for its harness".to_owned(),
        wire::StartSource::SavedSignIn => "a saved sign-in serves it".to_owned(),
        wire::StartSource::OperatorProfile => "the Operator's own".to_owned(),
        wire::StartSource::Default => "kestrel's default".to_owned(),
    }
}

fn sign_in(preview: &wire::StartPreview) -> String {
    let organization = shown(preview.organization.as_ref());
    match &preview.sign_in {
        Some(sign_in) => {
            let held = match sign_in.holding.resource {
                wire::SignInHoldingResource::SubscriptionProfile => {
                    format!("the subscription profile {}", sign_in.holding.reference)
                }
                wire::SignInHoldingResource::ProviderCredential => format!(
                    "{organization}'s provider credential {}",
                    sign_in.holding.reference
                ),
            };
            format!("{} {}, held in {held}", sign_in.harness, sign_in.method)
        }
        None => match &preview.sign_in_method.value {
            Some(method) => format!("{method}, not saved yet"),
            None => {
                format!("none selected; the harness is handed what {organization} holds for it")
            }
        },
    }
}

fn saved(saved: &wire::ReadinessSaved) -> String {
    let mut parts = Vec::new();
    match &saved.operator {
        Some(operator) => parts.push(format!("operator {}", operator.name)),
        None => parts.push("no operator".to_owned()),
    }
    let sign_ins: Vec<String> = saved
        .sign_ins
        .iter()
        .map(|sign_in| format!("{}/{}", sign_in.harness, sign_in.method))
        .collect();
    let listed = |values: &[String]| {
        if values.is_empty() {
            "none".to_owned()
        } else {
            values.join(", ")
        }
    };
    parts.push(format!("sign-ins {}", listed(&sign_ins)));
    parts.push(format!(
        "github integrations {}",
        listed(&saved.github_integrations)
    ));
    parts.push(format!("projects {}", listed(&saved.projects)));

    parts.join(" · ")
}

fn applying(preview: &wire::StartPreview) -> Vec<String> {
    let organization = shown(preview.organization.as_ref());
    let project = shown(preview.project.as_ref());
    let branch = shown(preview.branch.as_ref());
    let agent = shown(Some(&preview.agent));
    let repositories = preview.repositories.as_ref().map_or_else(
        || "its repositories".to_owned(),
        |repositories| {
            repositories
                .value
                .iter()
                .map(|repository| repository.address.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        },
    );
    let model = preview
        .model
        .value
        .as_deref()
        .map_or("its harness's default model".to_owned(), |model| {
            format!("the model {model}")
        });

    preview
        .declarations
        .iter()
        .filter_map(|step| {
            let name = step.name.clone().unwrap_or_default();
            use wire::StartDeclarationAction as Action;
            use wire::StartDeclarationKind as Kind;
            Some(match (&step.kind, &step.action) {
                (_, Action::Reuse) => return None,
                (Kind::Organization, _) => format!("declare the organization {name}"),
                (Kind::Project, _) => {
                    format!("declare the project {name} against {repositories}, based on {branch}")
                }
                (Kind::Agent, _) => format!(
                    "declare the agent {name} on the harness {} with {model}",
                    shown(Some(&preview.harness))
                ),
                (Kind::ProviderCredential, Action::Replace) => {
                    format!("replace {organization}'s provider credential {name}")
                }
                (Kind::ProviderCredential, _) => {
                    format!("hold {name} as a provider credential of {organization}")
                }
                (Kind::Workspace, _) => format!(
                    "open a workspace in {project} on a branch of its own from {branch}, \
                     carrying the brief, and enqueue a session of {agent}"
                ),
            })
        })
        .collect()
}

/// Every value the preview resolved, sent as given so a start that finds anything changed
/// refuses rather than resolving something else.
pub fn applied(
    preview: &wire::StartPreview,
    brief: String,
    secrets: Vec<(String, String)>,
) -> wire::StartPlan {
    let value = |value: Option<&wire::StartValue>| value.and_then(|value| value.value.clone());
    let repositories = preview.repositories.as_ref();
    let integrations: Vec<wire::RepositoryIntegrationSelection> = repositories
        .into_iter()
        .flat_map(|repositories| &repositories.value)
        .filter_map(|repository| {
            repository
                .integration
                .clone()
                .map(|integration| wire::RepositoryIntegrationSelection {
                    repository: repository.address.clone(),
                    integration,
                })
        })
        .collect();

    wire::StartPlan {
        organization: value(preview.organization.as_ref()),
        project: value(preview.project.as_ref()),
        repositories: repositories.map(|repositories| {
            repositories
                .value
                .iter()
                .map(|repository| repository.address.clone())
                .collect()
        }),
        integrations: (!integrations.is_empty()).then_some(integrations),
        branch: value(preview.branch.as_ref()),
        agent: value(Some(&preview.agent)),
        harness: value(Some(&preview.harness)),
        model: value(Some(&preview.model)),
        profile: value(Some(&preview.profile)),
        sign_in_method: value(Some(&preview.sign_in_method)),
        brief: Some(brief),
        credentials: (!secrets.is_empty()).then(|| {
            secrets
                .into_iter()
                .map(|(variable, secret)| wire::StartCredential {
                    variable,
                    secret: Some(secret),
                })
                .collect()
        }),
        clone: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_credential_is_read_from_the_environment_and_each_unset_one_is_named() {
        let variables = ["ANTHROPIC_API_KEY".to_owned(), "OPENAI_API_KEY".to_owned()];
        let read = |variable: &str| (variable == "ANTHROPIC_API_KEY").then(|| "sk".to_owned());

        assert_eq!(
            secrets(&variables, read),
            Err(vec![
                "OPENAI_API_KEY is not set in this environment".to_owned()
            ])
        );
        assert_eq!(
            secrets(&variables[..1], read),
            Ok(vec![("ANTHROPIC_API_KEY".to_owned(), "sk".to_owned())])
        );
    }

    #[test]
    fn a_plan_field_is_named_by_the_flag_that_gives_it() {
        for (field, named) in [
            ("repositories", "--repository"),
            ("project.branch", "--project"),
            ("agent.harness", "--agent"),
            ("sign_in_method", "--sign-in"),
            ("brief", "--brief"),
            ("organization", "--organization"),
        ] {
            assert_eq!(flag(field), named);
        }
    }
}
