use crate::support;

use serde_json::Value;
use support::Kestrel;
use support::client::{Finished, Invocation, ran_by};
use support::fixture::Fixture;

fn names(records: &[Value]) -> Vec<&str> {
    records
        .iter()
        .map(|record| record["name"].as_str().expect("a name"))
        .collect()
}

fn bound_to(organization: &str) -> Invocation {
    Invocation::default().file(".kestrel/organization", organization)
}

fn refused(finished: &Finished, naming: &[&str]) {
    assert!(!finished.status.success(), "the client did not refuse");
    for named in naming {
        assert!(
            finished.err.contains(named),
            "the refusal does not name {named}:\n{}",
            finished.err
        );
    }
}

/// Two Organizations, each holding a Project a listing can tell apart.
async fn two_organizations() -> Kestrel {
    let kestrel = Kestrel::boot().await;
    let acme = kestrel.declare_organization("acme").await;
    let globex = kestrel.declare_organization("globex").await;
    let repository = vec!["https://github.com/jtmthf/kestrel".to_owned()];
    kestrel
        .declare_project(&acme, "for-acme", &repository, "main")
        .await;
    kestrel
        .declare_project(&globex, "for-globex", &repository, "main")
        .await;

    kestrel
}

#[tokio::test]
async fn the_flag_names_the_scope_ahead_of_the_environment_and_a_binding() {
    let kestrel = two_organizations().await;

    let listed = ran_by(
        &kestrel,
        &["project", "list", "--json", "--organization", "acme"],
        bound_to("globex").env("KESTREL_ORGANIZATION", "globex"),
    )
    .await;

    assert_eq!(names(&listed.records()), ["for-acme"]);
    kestrel.teardown().await;
}

#[tokio::test]
async fn the_environment_names_the_scope_ahead_of_a_binding() {
    let kestrel = two_organizations().await;

    let listed = ran_by(
        &kestrel,
        &["project", "list", "--json"],
        bound_to("acme").env("KESTREL_ORGANIZATION", "globex"),
    )
    .await;

    assert_eq!(names(&listed.records()), ["for-globex"]);
    kestrel.teardown().await;
}

#[tokio::test]
async fn a_committed_binding_names_the_scope_from_the_working_directory() {
    let kestrel = two_organizations().await;

    let listed = ran_by(&kestrel, &["project", "list", "--json"], bound_to("acme")).await;

    assert_eq!(names(&listed.records()), ["for-acme"]);
    kestrel.teardown().await;
}

#[tokio::test]
async fn a_committed_binding_names_the_scope_from_anywhere_in_its_repository() {
    let kestrel = two_organizations().await;

    let listed = ran_by(
        &kestrel,
        &["project", "list", "--json"],
        bound_to("acme")
            .file(".git/HEAD", "ref: refs/heads/main\n")
            .within("crates/kestrel"),
    )
    .await;

    assert_eq!(names(&listed.records()), ["for-acme"]);
    kestrel.teardown().await;
}

#[tokio::test]
async fn a_binding_above_the_repository_is_no_remembered_scope() {
    let kestrel = two_organizations().await;

    let listed = ran_by(
        &kestrel,
        &["project", "list"],
        bound_to("acme")
            .file("kestrel/.git/HEAD", "ref: refs/heads/main\n")
            .within("kestrel/crates"),
    )
    .await;

    refused(&listed, &["--organization", "acme", "globex"]);
    kestrel.teardown().await;
}

#[tokio::test]
async fn the_only_organization_is_the_scope_when_nothing_names_one() {
    let kestrel = Kestrel::boot().await;
    let acme = kestrel.declare_organization("acme").await;
    kestrel
        .declare_project(
            &acme,
            "kestrel",
            &["https://github.com/jtmthf/kestrel".to_owned()],
            "main",
        )
        .await;

    let listed = ran_by(
        &kestrel,
        &["project", "list", "--json"],
        Invocation::default(),
    )
    .await;

    assert_eq!(names(&listed.records()), ["kestrel"]);
    kestrel.teardown().await;
}

#[tokio::test]
async fn two_organizations_make_an_unqualified_command_fail_before_writing_anything() {
    let kestrel = two_organizations().await;

    let declared = ran_by(
        &kestrel,
        &[
            "project",
            "declare",
            "kestrel",
            "--repository",
            "https://github.com/jtmthf/kestrel",
            "--branch",
            "main",
        ],
        Invocation::default(),
    )
    .await;

    refused(&declared, &["--organization", "acme", "globex"]);
    let mut held = Vec::new();
    for organization in kestrel.organizations().await {
        held.extend(
            kestrel
                .projects(&organization)
                .await
                .into_iter()
                .map(|project| project.name),
        );
    }
    held.sort();
    assert_eq!(held, ["for-acme", "for-globex"]);
    kestrel.teardown().await;
}

#[tokio::test]
async fn every_scoped_listing_refuses_to_guess_between_two_organizations() {
    let kestrel = two_organizations().await;

    for noun in [
        "project",
        "agent",
        "credential",
        "profile",
        "integration",
        "event",
        "trigger",
        "workspace",
    ] {
        let listed = ran_by(&kestrel, &[noun, "list"], Invocation::default()).await;
        refused(&listed, &["--organization", "acme", "globex"]);
    }
    kestrel.teardown().await;
}

#[tokio::test]
async fn no_organization_and_no_operator_names_the_command_that_makes_both() {
    let kestrel = Kestrel::boot().await;

    let listed = ran_by(&kestrel, &["project", "list"], Invocation::default()).await;

    refused(&listed, &["kestrel operator name"]);
    kestrel.teardown().await;
}

#[tokio::test]
async fn an_empty_name_is_refused_rather_than_falling_through_to_another_scope() {
    let kestrel = Kestrel::boot().await;
    kestrel.declare_organization("acme").await;

    let flagged = ran_by(
        &kestrel,
        &["project", "list", "--organization", ""],
        bound_to("acme").env("KESTREL_ORGANIZATION", "acme"),
    )
    .await;
    let exported = ran_by(
        &kestrel,
        &["project", "list"],
        bound_to("acme").env("KESTREL_ORGANIZATION", ""),
    )
    .await;

    refused(&flagged, &["--organization"]);
    refused(&exported, &["--organization"]);
    kestrel.teardown().await;
}

#[tokio::test]
async fn status_prints_every_resolved_value_its_source_and_what_exists() {
    let kestrel = Kestrel::boot().await;
    Fixture::acme().declare(&kestrel).await;

    let reported = ran_by(
        &kestrel,
        &["status", "--organization", "acme", "--json"],
        Invocation::default(),
    )
    .await;
    let reported = blocked_status(&reported);

    assert_eq!(reported.len(), 1);
    assert_eq!(reported[0]["control_plane"], kestrel.operator());
    assert_eq!(reported[0]["control_plane_source"], "KESTREL_CONTROL_PLANE");
    assert_eq!(reported[0]["organization"], "acme");
    assert_eq!(reported[0]["organization_source"], "--organization");
    assert_eq!(reported[0]["projects"], 1);
    assert_eq!(reported[0]["agents"], 1);
    assert_eq!(reported[0]["triggers"], 0);
    assert_eq!(reported[0]["workspaces"], 0);
    assert_eq!(reported[0]["integrations"], 0);
    assert_eq!(reported[0]["credentials"], 0);
    assert_eq!(reported[0]["profiles"], 0);
    assert_eq!(reported[0]["readiness"]["outcome"], "blocked");
    kestrel.teardown().await;
}

#[tokio::test]
async fn status_names_the_environment_and_the_binding_when_each_is_the_source() {
    let kestrel = Kestrel::boot().await;
    kestrel.declare_organization("acme").await;

    let sourced = &["status", "--json"];
    let environment = ran_by(
        &kestrel,
        sourced,
        Invocation::default().env("KESTREL_ORGANIZATION", "acme"),
    )
    .await;
    let environment = blocked_status(&environment);
    let binding = blocked_status(&ran_by(&kestrel, sourced, bound_to("acme")).await);
    let only = blocked_status(&ran_by(&kestrel, sourced, Invocation::default()).await);

    assert_eq!(
        environment[0]["organization_source"],
        "KESTREL_ORGANIZATION"
    );
    assert!(
        binding[0]["organization_source"]
            .as_str()
            .expect("a bound path")
            .ends_with(".kestrel/organization"),
        "{}",
        binding[0]["organization_source"]
    );
    assert_eq!(only[0]["organization_source"], "only organization");
    kestrel.teardown().await;
}

#[tokio::test]
async fn status_requires_a_scope_when_several_organizations_exist() {
    let kestrel = two_organizations().await;
    let reported = ran_by(&kestrel, &["status"], Invocation::default()).await;
    assert_eq!(reported.status.code(), Some(3));
    assert!(reported.out.is_empty());
    assert!(reported.err.contains("--organization"), "{}", reported.err);
    kestrel.teardown().await;
}

#[tokio::test]
async fn a_command_naming_its_record_needs_no_scope() {
    let kestrel = two_organizations().await;

    let event = ran_by(
        &kestrel,
        &["event", "show", "yesterday"],
        Invocation::default(),
    )
    .await;

    refused(&event, &["no event yesterday"]);
    kestrel.teardown().await;
}

/// A Workspace reference is resolved inside the scope the invocation named, so it cannot be
/// reached without one, unlike an Event's record which stands alone.
#[tokio::test]
async fn a_workspace_reference_refuses_to_guess_between_two_organizations() {
    let kestrel = two_organizations().await;

    let shown = ran_by(
        &kestrel,
        &["workspace", "show", "01a0a2d8-baf8-7c02-99fa-7280f174c14a"],
        Invocation::default(),
    )
    .await;

    refused(&shown, &["--organization", "acme", "globex"]);
    kestrel.teardown().await;
}

#[tokio::test]
async fn a_command_naming_its_record_refuses_a_flag_it_would_ignore() {
    let kestrel = two_organizations().await;

    let flagged = ran_by(
        &kestrel,
        &["event", "show", "yesterday", "--organization", "globex"],
        Invocation::default(),
    )
    .await;
    let exported = ran_by(
        &kestrel,
        &["event", "show", "yesterday"],
        Invocation::default().env("KESTREL_ORGANIZATION", "globex"),
    )
    .await;

    refused(&flagged, &["--organization scopes nothing"]);
    refused(&exported, &["no event yesterday"]);
    kestrel.teardown().await;
}

#[tokio::test]
async fn the_client_keeps_no_current_context_and_switches_none() {
    let kestrel = Kestrel::boot().await;
    let acme = kestrel.declare_organization("acme").await;

    let switched = ran_by(&kestrel, &["context", "use", "acme"], Invocation::default()).await;
    let configured = ran_by(
        &kestrel,
        &["config", "use-context", "acme"],
        Invocation::default(),
    )
    .await;
    let status = ran_by(
        &kestrel,
        &["status", "--organization", "acme", "--json"],
        Invocation::default(),
    )
    .await;

    assert!(!switched.status.success());
    assert!(!configured.status.success());
    assert_eq!(blocked_status(&status)[0]["organization"], "acme");
    assert_eq!(
        kestrel.organizations().await[0].id,
        acme.id,
        "a scope-switch would have rewritten the Organization"
    );
    assert!(
        status.left_behind.is_empty(),
        "the client persisted a current context in {:?}",
        status.left_behind
    );
    kestrel.teardown().await;
}

fn blocked_status(finished: &crate::support::client::Finished) -> Vec<Value> {
    assert_eq!(finished.status.code(), Some(78), "{}", finished.err);
    finished
        .out
        .iter()
        .map(|line| serde_json::from_str(line).expect("a complete status read"))
        .collect()
}
