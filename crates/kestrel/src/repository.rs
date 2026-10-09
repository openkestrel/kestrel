use std::collections::HashMap;

use url::Url;

use crate::declined::{Constraint, Reason};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    pub given: String,
    pub address: String,
    pub directory: String,
}

/// Which declaration a repository list is resolved for: the public-repository setup path selects
/// no Integration, so it takes only what an anonymous HTTPS clone can read.
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
) -> Result<Vec<Resolved>, Reason> {
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
        ));
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
            ));
        }
        let directory = cloned_into(&address);
        if matches!(directory, "" | "." | "..") {
            return Err(refused(
                Constraint::CheckoutDirectory,
                format!("{given} names no directory to check it out into"),
            ));
        }
        if let Some(earlier) = claimed.insert(directory.to_owned(), given) {
            return Err(refused(
                Constraint::DistinctCheckoutDirectories,
                format!("{earlier} and {given} would both be checked out into {directory}"),
            ));
        }
        resolved.push(Resolved {
            given: given.clone(),
            directory: directory.to_owned(),
            address,
        });
    }

    Ok(resolved)
}

pub fn addresses(resolved: Vec<Resolved>) -> Vec<String> {
    resolved
        .into_iter()
        .map(|resolved| resolved.address)
        .collect()
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

    Ok(())
}

/// `owner/repo` and `github.com/owner/repo`, with an optional `.git`: the conveniences ADR-0057
/// expands, never a path Git would read relative to the Instance's working directory.
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

    fn resolving(repositories: &[&str], purpose: Purpose) -> Result<Vec<Resolved>, Reason> {
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
        match resolving(repositories, purpose) {
            Err(Reason::InvalidField {
                constraint,
                message,
                field: "repositories",
                ..
            }) => (constraint, message),
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
