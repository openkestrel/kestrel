use anyhow::Result;

use super::*;
use crate::declined::{Constraint, Reason};

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
