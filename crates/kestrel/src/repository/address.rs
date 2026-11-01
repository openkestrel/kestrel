use anyhow::Result;
use url::Url;

pub(super) fn address(given: &str) -> Result<String, String> {
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
pub(super) fn cloned_into(repository: &str) -> &str {
    let name = repository
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or(repository);

    name.strip_suffix(".git").unwrap_or(name)
}
