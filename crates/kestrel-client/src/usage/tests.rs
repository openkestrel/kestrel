use clap::{CommandFactory as _, Parser as _};

use super::*;

#[test]
fn an_organization_named_workspace_does_not_change_the_guessed_noun() {
    let arguments = ["--organization", "workspace", "session", "start"];
    let error = Client::command()
        .try_get_matches_from(["kestrel"].into_iter().chain(arguments))
        .expect_err("start is not a session verb");
    let arguments = arguments.map(str::to_owned);
    let explanation = explain_invalid_subcommand(&error, &arguments).expect("session help");

    assert!(
        explanation.contains("kestrel session enqueue"),
        "{explanation}"
    );
    assert!(
        explanation.contains("Accepted session verbs:"),
        "{explanation}"
    );
}

fn args(given: &[&str]) -> Vec<String> {
    given.iter().map(|&arg| arg.to_owned()).collect()
}

#[test]
fn any_value_given_to_json_is_refused_and_dropped_from_the_retry() {
    let given = args(&["workspace", "post", "w", "--json", "fix a, then b"]);
    assert_eq!(
        json_value(&given),
        Some(("fix a, then b", args(&["workspace", "post", "w", "--json"])))
    );
    assert_eq!(
        json_value(&args(&["agent", "list", "--json=id"])),
        Some(("id", args(&["agent", "list", "--json"])))
    );
    assert_eq!(
        json_value(&args(&["agent", "list", "--json", "--organization", "a"])),
        None
    );
    assert_eq!(
        json_value(&args(&["workspace", "changes", "w", "--", "--json", "a"])),
        None
    );
    let parsed = Client::try_parse_from(["kestrel", "agent", "list", "--json"])
        .expect("--json alone is a flag");
    assert!(parsed.json);
}

#[test]
fn a_misspelling_uses_the_parsers_suggested_verb() {
    let error = Client::command()
        .try_get_matches_from(["kestrel", "workspace", "sael"])
        .expect_err("sael is not a workspace verb");
    let explanation = explain_invalid_subcommand(&error, &["workspace".into(), "sael".into()])
        .expect("workspace help");

    assert!(
        explanation.contains("kestrel workspace seal"),
        "{explanation}"
    );
}
