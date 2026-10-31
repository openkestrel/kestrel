use url::Url;

use crate::integration::github;

/// Which repository an address names, whatever transport or spelling it is given in.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Identity(Named);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Named {
    Github(String),
    Hosted { host: String, path: String },
    Local(String),
}

impl Identity {
    /// None for anything a declaration would refuse.
    pub fn of(given: &str) -> Option<Self> {
        let address = super::address(given).ok()?;
        if let Some(named) = github::named_repository(&address) {
            return Some(Self(Named::Github(named.to_ascii_lowercase())));
        }
        if ["/", "./", "../"]
            .iter()
            .any(|local| address.starts_with(local))
        {
            return Some(Self(Named::Local(address)));
        }
        let (host, path) = if address.contains("://") {
            let url = Url::parse(&address).ok()?;
            if url.scheme() == "file" {
                return Some(Self(Named::Local(address)));
            }
            (url.host_str()?.to_owned(), url.path().to_owned())
        } else {
            let (host, path) = address.split_once(':')?;
            let host = host.rsplit_once('@').map_or(host, |(_, host)| host);
            (host.to_owned(), path.to_owned())
        };
        let path = path.trim_matches('/');

        Some(Self(Named::Hosted {
            host: host.to_ascii_lowercase(),
            path: path.strip_suffix(".git").unwrap_or(path).to_owned(),
        }))
    }

    pub fn same(one: &str, other: &str) -> bool {
        Self::of(one).is_some_and(|one| Self::of(other) == Some(one))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_transport_and_spelling_of_a_repository_names_it() {
        for (one, other) in [
            (
                "https://github.com/acme/widgets.git",
                "https://github.com/acme/widgets",
            ),
            (
                "https://github.com/acme/widgets.git",
                "https://github.com/Acme/Widgets/",
            ),
            (
                "https://github.com/acme/widgets.git",
                "git@github.com:acme/widgets.git",
            ),
            (
                "https://github.com/acme/widgets.git",
                "ssh://git@github.com/acme/widgets",
            ),
            ("https://github.com/acme/widgets.git", "acme/widgets"),
            (
                "https://github.com/acme/widgets.git",
                "github.com/acme/widgets",
            ),
            (
                "https://git.example.com/acme/widgets.git",
                "git@GIT.example.com:acme/widgets",
            ),
            (
                "https://git.example.com/acme/widgets",
                "ssh://git@git.example.com:2222/acme/widgets.git",
            ),
            (
                "git://git.example.com/acme/widgets.git",
                "http://git.example.com/acme/widgets",
            ),
            ("/srv/widgets", "/srv/widgets"),
        ] {
            assert!(Identity::same(one, other), "{one} and {other}");
        }
    }

    #[test]
    fn another_repository_host_or_path_is_another_repository() {
        for (one, other) in [
            (
                "https://github.com/acme/widgets.git",
                "https://github.com/acme/widgets-fork.git",
            ),
            (
                "https://github.com/acme/widgets.git",
                "https://gitlab.com/acme/widgets.git",
            ),
            (
                "https://git.example.com/acme/widgets",
                "https://git.example.com/Acme/Widgets",
            ),
            ("/srv/widgets", "/srv/widgets.git"),
            ("/srv/widgets", "file:///srv/widgets"),
            ("./widgets", "../widgets"),
        ] {
            assert!(!Identity::same(one, other), "{one} and {other}");
        }
    }

    #[test]
    fn an_address_a_declaration_refuses_names_nothing() {
        for refused in [
            "",
            " acme/widgets",
            "widgets",
            "hg::https://example.com/widgets",
        ] {
            assert_eq!(Identity::of(refused), None, "{refused:?}");
        }
        assert!(!Identity::same("widgets", "widgets"));
    }
}
