use std::collections::HashMap;

use anyhow::Result;
use url::Url;

use crate::declined::{Again, Constraint, Next, Reason, Resource};
use crate::domain::{Integration, Organization, Project, SelectedIntegration};
use crate::integration::{self, github, github::Github};
use crate::store::{Declared, Store, Tx};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    pub given: String,
    pub address: String,
    pub directory: String,
}

/// Public-repository setup selects no Integration, so it takes only what an anonymous HTTPS clone
/// can read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Purpose {
    Declaration,
    PublicSetup,
}

/// Checks syntax and checkout directories only: no source is contacted, and a local path names
/// the Instance's filesystem, so a resolved address promises no successful checkout (ADR-0057).
pub fn resolved(
    operation: &'static str,
    field: &'static str,
    repositories: &[String],
    purpose: Purpose,
) -> Result<Vec<Resolved>> {
    let refused = |constraint: Constraint, message: String| Reason::InvalidField {
        field,
        operation,
        constraint,
        allowed: None,
        message,
    };
    if repositories.is_empty() {
        return Err(refused(
            Constraint::NonEmpty,
            "a project names at least one repository".to_owned(),
        )
        .into());
    }

    let mut resolved = Vec::with_capacity(repositories.len());
    let mut claimed = HashMap::new();
    for given in repositories {
        let address = address(given).map_err(|why| refused(Constraint::GitRepository, why))?;
        if purpose == Purpose::PublicSetup && !address.starts_with("https://") {
            return Err(refused(
                Constraint::HttpsRepository,
                format!(
                    "{given} is not an HTTPS address: a public repository is given as an \
                     https:// URL, owner/repo or github.com/owner/repo"
                ),
            )
            .into());
        }
        let directory = cloned_into(&address);
        if matches!(directory, "" | "." | "..") {
            return Err(refused(
                Constraint::CheckoutDirectory,
                format!("{given} names no directory to check it out into"),
            )
            .into());
        }
        if let Some(earlier) = claimed.insert(directory.to_owned(), given) {
            return Err(refused(
                Constraint::DistinctCheckoutDirectories,
                format!("{earlier} and {given} would both be checked out into {directory}"),
            )
            .into());
        }
        resolved.push(Resolved {
            given: given.clone(),
            directory: directory.to_owned(),
            address,
        });
    }

    Ok(resolved)
}

pub use kestrel_operator_types::RepositoryIntegrationSelection as Selection;

pub struct Selecting<'a> {
    pub operation: &'static str,
    pub repositories_field: &'static str,
    pub integrations_field: &'static str,
    pub organization: &'a str,
    pub repositories: &'a [String],
    pub selections: &'a [Selection],
}

pub struct Declaring<'a> {
    pub selecting: Selecting<'a>,
    pub branch_field: &'static str,
    pub project: &'a str,
    pub branch: &'a str,
}

pub struct Bound {
    pub resolved: Resolved,
    pub integration: Option<Integration>,
}

pub async fn resolution(
    store: &Store,
    selecting: &Selecting<'_>,
    purpose: Purpose,
) -> Result<Vec<Bound>> {
    let refused = |field, constraint, message: &str| Reason::InvalidField {
        field,
        operation: selecting.operation,
        constraint,
        allowed: None,
        message: message.to_owned(),
    };
    if selecting.selections.is_empty() {
        return Ok(resolved(
            selecting.operation,
            selecting.repositories_field,
            selecting.repositories,
            purpose,
        )?
        .into_iter()
        .map(|resolved| Bound {
            resolved,
            integration: None,
        })
        .collect());
    }
    if purpose == Purpose::PublicSetup {
        return Err(refused(
            selecting.integrations_field,
            Constraint::DeclarationPurpose,
            "public-repository setup selects no integration; resolve a declaration to select one",
        )
        .into());
    }
    if selecting.organization.is_empty() {
        return Err(refused(
            "organization",
            Constraint::NonEmpty,
            "an integration is selected within an organization; name it",
        )
        .into());
    }

    bound(store, selecting).await
}

/// Asks GitHub nothing: which Integration each repository selects, and the HTTPS address a
/// selected one is checked out by, since an App lends no SSH authority (ADR-0057).
pub async fn bound(store: &Store, selecting: &Selecting<'_>) -> Result<Vec<Bound>> {
    let mut bound: Vec<Bound> = resolved(
        selecting.operation,
        selecting.repositories_field,
        selecting.repositories,
        Purpose::Declaration,
    )?
    .into_iter()
    .map(|resolved| Bound {
        resolved,
        integration: None,
    })
    .collect();
    if selecting.selections.is_empty() {
        return Ok(bound);
    }

    let refused = |constraint: Constraint, message: String| Reason::InvalidField {
        field: selecting.integrations_field,
        operation: selecting.operation,
        constraint,
        allowed: None,
        message,
    };
    let mut tx = store.read().await?;
    let organization = tx.organizations().named(selecting.organization).await?;
    for selection in selecting.selections {
        let named = address(&selection.repository)
            .map_err(|why| refused(Constraint::GitRepository, why))?;
        let Some(repository) = bound.iter_mut().find(|bound| {
            bound.resolved.given == selection.repository || bound.resolved.address == named
        }) else {
            return Err(refused(
                Constraint::DeclaredRepository,
                format!(
                    "{} is given the integration {}, and the project does not declare it",
                    selection.repository, selection.integration
                ),
            )
            .into());
        };
        if repository.integration.is_some() {
            return Err(refused(
                Constraint::DeclaredRepository,
                format!("{} is given an integration twice", selection.repository),
            )
            .into());
        }
        let integration = tx
            .integrations()
            .named(&organization, &selection.integration)
            .await?;
        let connection =
            integration::lending(&integration, &organization.name, selecting.operation)?;
        let Some(names) = github::named_repository(&repository.resolved.address)
            .filter(|names| names.eq_ignore_ascii_case(&connection.repository))
        else {
            return Err(refused(
                Constraint::WatchedByIntegration,
                format!(
                    "the integration {} watches {}, which {} does not name",
                    integration.name, connection.repository, selection.repository
                ),
            )
            .into());
        };
        if !repository
            .resolved
            .address
            .get(..8)
            .is_some_and(|scheme| scheme.eq_ignore_ascii_case("https://"))
        {
            repository.resolved.address = github::https_address(&names);
        }
        repository.integration = Some(integration);
    }

    Ok(bound)
}

struct Checked {
    repository: String,
    integration: Integration,
    /// False when the Project already held this binding on this branch as it was read.
    verified: bool,
}

pub struct Declarable {
    operation: &'static str,
    repositories: Vec<String>,
    branch: String,
    selected: Vec<Checked>,
}

/// GitHub's answer says the App reaches the repository and branch now, never that an Instance
/// will check them out (ADR-0057).
pub async fn declarable(
    store: &Store,
    github: &Github,
    declaring: &Declaring<'_>,
) -> Result<Declarable> {
    let Selecting {
        operation,
        organization,
        ..
    } = declaring.selecting;
    let bound = bound(store, &declaring.selecting).await?;
    let repositories = bound
        .iter()
        .map(|bound| bound.resolved.address.clone())
        .collect();
    let mut declarable = Declarable {
        operation,
        repositories,
        branch: declaring.branch.to_owned(),
        selected: Vec::new(),
    };
    if bound.iter().all(|bound| bound.integration.is_none()) {
        return Ok(declarable);
    }

    let held = {
        let mut tx = store.read().await?;
        let organization = tx.organizations().named(organization).await?;
        tx.projects().find(&organization, declaring.project).await?
    };
    for bound in bound {
        let Some(integration) = bound.integration else {
            continue;
        };
        let mut checked = Checked {
            repository: bound.resolved.address,
            integration,
            verified: false,
        };
        if !declarable.holds(held.as_ref(), &checked) {
            let connection = checked.integration.github()?;
            match github.reaches(connection, declaring.branch).await {
                Ok(true) => checked.verified = true,
                Ok(false) => {
                    return Err(Reason::InvalidField {
                        field: declaring.branch_field,
                        operation,
                        constraint: Constraint::BranchInRepository,
                        allowed: None,
                        message: format!(
                            "{} has no branch {} that the integration {} can see",
                            connection.repository, declaring.branch, checked.integration.name
                        ),
                    }
                    .into());
                }
                Err(unproven) => {
                    return Err(integration::unreached(
                        &checked.integration,
                        organization,
                        operation,
                        unproven,
                        Again::Request,
                    )
                    .into());
                }
            }
        }
        declarable.selected.push(checked);
    }

    Ok(declarable)
}

impl Declarable {
    pub fn repositories(&self) -> &[String] {
        &self.repositories
    }

    pub fn selected(&self) -> impl Iterator<Item = (&str, &str)> {
        self.selected.iter().map(|checked| {
            (
                checked.repository.as_str(),
                checked.integration.name.as_str(),
            )
        })
    }

    pub fn describes(&self, project: &Project) -> bool {
        project.repositories == self.repositories
            && project.branch == self.branch
            && project.integrations.len() == self.selected.len()
            && self
                .selected
                .iter()
                .all(|checked| self.holds(Some(project), checked))
    }

    fn holds(&self, project: Option<&Project>, checked: &Checked) -> bool {
        project.is_some_and(|project| {
            project.branch == self.branch
                && project.integrations.iter().any(|selected| {
                    selected.repository == checked.repository
                        && selected.integration == checked.integration.id
                })
        })
    }

    /// Whatever was checked is saved only if nothing it was checked against has moved since.
    pub async fn declare(
        &self,
        tx: &mut Tx<'_>,
        organization: &Organization,
        name: &str,
    ) -> Result<Declared<Project>> {
        let held = tx.projects().find(organization, name).await?;
        let mut integrations = Vec::with_capacity(self.selected.len());
        for checked in &self.selected {
            if !self.holds(held.as_ref(), checked) && !checked.verified {
                return Err(Reason::StateConflict {
                    operation: self.operation,
                    resource: Resource::Project,
                    reference: name.to_owned(),
                    organization: Some(organization.name.clone()),
                    state: "revised",
                    holding_session: None,
                    next: Next::inspect(Resource::Project, name),
                    message: format!(
                        "the project {name} changed while its repositories were being \
                         checked; read it again and declare what it should be now"
                    ),
                }
                .into());
            }
            let current = tx.integrations().with_id(checked.integration.id).await?;
            if current.revision != checked.integration.revision {
                return Err(integration::revised(
                    &current,
                    &organization.name,
                    self.operation,
                    checked.integration.revision,
                    Next::inspect(Resource::Integration, current.name.clone()),
                )
                .into());
            }
            integration::lending(&current, &organization.name, self.operation)?;
            integrations.push(SelectedIntegration {
                repository: checked.repository.clone(),
                integration: checked.integration.id,
                name: checked.integration.name.clone(),
            });
        }

        tx.projects()
            .declare(
                organization,
                name,
                &self.repositories,
                &integrations,
                &self.branch,
            )
            .await
    }
}

fn address(given: &str) -> Result<String, String> {
    if given.trim().is_empty() {
        return Err("a repository cannot be empty".to_owned());
    }
    if given.chars().any(char::is_control) || given.trim() != given {
        return Err(format!(
            "{given:?} carries whitespace or control characters around or within its address"
        ));
    }
    if given.starts_with('/') || given.starts_with("./") || given.starts_with("../") {
        return Ok(given.to_owned());
    }
    if let Some((scheme, _)) = given.split_once("://") {
        return explicit(given, scheme).map(|()| given.to_owned());
    }
    if given.contains("::") {
        return Err(format!(
            "{given} names a Git remote helper, which kestrel does not check out"
        ));
    }
    if let Some((owner, name)) = github_shorthand(given) {
        return Ok(format!("https://github.com/{owner}/{name}.git"));
    }
    if scp_style(given) {
        return Ok(given.to_owned());
    }

    Err(format!(
        "{given} is no repository kestrel can check out: give an http(s)://, ssh://, git:// or \
         file:// URL, an scp-style user@host:path, owner/repo for GitHub, or a local path \
         beginning /, ./ or ../"
    ))
}

fn explicit(given: &str, scheme: &str) -> Result<(), String> {
    let malformed = |what: &str| Err(format!("{given} is a {scheme} URL {what}"));
    let host_required = match scheme {
        "http" | "https" | "ssh" | "git" => true,
        "file" => false,
        _ => {
            return Err(format!(
                "{given} uses the {scheme} transport, which kestrel does not check out: give an \
                 http(s)://, ssh://, git:// or file:// URL"
            ));
        }
    };
    let Ok(url) = Url::parse(given) else {
        return malformed("kestrel cannot read");
    };
    if host_required && url.host_str().is_none_or(str::is_empty) {
        return malformed("naming no host");
    }
    if url.path().trim_matches('/').is_empty() {
        return malformed("naming no repository path");
    }
    if url.query().is_some() || url.fragment().is_some() {
        return malformed("carrying a query or fragment, which names no repository");
    }

    Ok(())
}

/// Never a path Git would read relative to the Instance's working directory (ADR-0057).
fn github_shorthand(given: &str) -> Option<(&str, &str)> {
    let path = given.strip_prefix("github.com/").unwrap_or(given);
    let (owner, name) = path.split_once('/')?;
    let name = name.strip_suffix(".git").unwrap_or(name);
    let owner_ok = !owner.is_empty()
        && !owner.starts_with('-')
        && owner.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
    let name_ok = !matches!(name, "" | "." | "..")
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));

    (owner_ok && name_ok).then_some((owner, name))
}

/// Git's own rule: a colon before any slash makes `[user@]host:path` an SSH address.
fn scp_style(given: &str) -> bool {
    let Some((host, path)) = given.split_once(':') else {
        return false;
    };
    let host = host.rsplit_once('@').map_or(host, |(_, host)| host);

    !host.is_empty() && !host.contains('/') && !path.trim_matches('/').is_empty()
}

// Must name the directory kestrel-supervisor's checkout clones into.
fn cloned_into(repository: &str) -> &str {
    let name = repository
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or(repository);

    name.strip_suffix(".git").unwrap_or(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resolving(repositories: &[&str], purpose: Purpose) -> Result<Vec<Resolved>> {
        let repositories: Vec<String> = repositories.iter().map(|&r| r.to_owned()).collect();
        resolved("declare_project", "repositories", &repositories, purpose)
    }

    fn address_of(given: &str) -> String {
        resolving(&[given], Purpose::Declaration)
            .unwrap_or_else(|reason| panic!("{given} was refused: {reason}"))
            .remove(0)
            .address
    }

    fn refusal(repositories: &[&str], purpose: Purpose) -> (Constraint, String) {
        match resolving(repositories, purpose).map_err(|error| error.downcast::<Reason>()) {
            Err(Ok(Reason::InvalidField {
                constraint,
                message,
                field: "repositories",
                ..
            })) => (constraint, message),
            other => panic!("{repositories:?} was not refused on its field: {other:?}"),
        }
    }

    #[test]
    fn explicit_transports_and_local_paths_are_kept_as_given() {
        for given in [
            "https://github.com/acme/widgets.git",
            "http://git.example.com/acme/widgets",
            "ssh://git@git.example.com:2222/acme/widgets.git",
            "git@github.com:acme/widgets.git",
            "git.example.com:acme/widgets",
            "git://git.example.com/acme/widgets.git",
            "file:///srv/git/widgets.git",
            "/srv/git/widgets.git",
            "./owner/repo",
            "../widgets",
        ] {
            assert_eq!(address_of(given), given);
        }
    }

    #[test]
    fn github_conveniences_expand_to_https() {
        for given in [
            "acme/widgets",
            "acme/widgets.git",
            "github.com/acme/widgets",
            "github.com/acme/widgets.git",
        ] {
            assert_eq!(address_of(given), "https://github.com/acme/widgets.git");
        }
    }

    #[test]
    fn empty_bare_unknown_and_helper_forms_are_refused() {
        for given in [
            "",
            "   ",
            "widgets",
            "acme/widgets/extra",
            "svn://example.com/widgets",
            "s3://bucket/widgets",
            "ext::ssh -i key host",
            "https://",
            "https:///widgets",
            "https://github.com",
            "ssh://host/",
            " acme/widgets",
            "https://github.com/acme/widgets?ref=main",
            "https://github.com/acme/widgets#readme",
        ] {
            let (constraint, _) = refusal(&[given], Purpose::Declaration);
            assert_eq!(constraint, Constraint::GitRepository, "{given:?}");
        }
    }

    #[test]
    fn one_bad_entry_refuses_the_list() {
        let (constraint, message) = refusal(&["acme/widgets", ""], Purpose::Declaration);

        assert_eq!(constraint, Constraint::GitRepository);
        assert!(message.contains("empty"), "{message}");
    }

    #[test]
    fn an_empty_list_is_refused() {
        assert_eq!(refusal(&[], Purpose::Declaration).0, Constraint::NonEmpty);
    }

    #[test]
    fn collisions_are_found_after_resolution() {
        let (constraint, message) = refusal(
            &["acme/widgets", "https://git.example.com/team/widgets"],
            Purpose::Declaration,
        );

        assert_eq!(constraint, Constraint::DistinctCheckoutDirectories);
        assert!(message.contains("checked out into widgets"), "{message}");
    }

    #[test]
    fn a_repository_naming_no_directory_is_refused() {
        for given in ["./", "../", "/", "https://example.com/acme/.git"] {
            let (constraint, _) = refusal(&[given], Purpose::Declaration);
            assert_eq!(constraint, Constraint::CheckoutDirectory, "{given:?}");
        }
    }

    #[test]
    fn resolution_reports_the_checkout_directory() {
        let resolved = resolving(&["acme/widgets", "./tools/"], Purpose::Declaration).unwrap();

        assert_eq!(resolved[0].directory, "widgets");
        assert_eq!(resolved[1].directory, "tools");
        assert_eq!(resolved[1].given, "./tools/");
    }

    #[test]
    fn public_setup_takes_https_and_github_conveniences_only() {
        assert_eq!(
            resolving(&["acme/widgets"], Purpose::PublicSetup).unwrap()[0].address,
            "https://github.com/acme/widgets.git"
        );
        assert!(resolving(&["https://example.com/acme/widgets"], Purpose::PublicSetup).is_ok());
        for given in [
            "git@github.com:acme/widgets.git",
            "http://example.com/acme/widgets",
            "./widgets",
        ] {
            let (constraint, _) = refusal(&[given], Purpose::PublicSetup);
            assert_eq!(constraint, Constraint::HttpsRepository, "{given:?}");
        }
    }
}
