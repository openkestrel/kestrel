use crate::support;

use kestrel::domain::{Exit, Organization, SessionId, WorkspaceId};
use kestrel::log::{BriefSource, Entry};
use serde_json::Value;
use support::client::{Finished, Invocation, Shown, ran_by, ran_on_a_terminal_by};
use support::scripted_agent::{self, Script};
use support::{A_PROVIDER_KEY, Kestrel, PROVIDER_KEY, repository, supervisor};

const BRIEF: &str = "Make the README say what kestrel is";
const QUESTION: &str = "apply this plan?";
const SIGN_IN: &str = "ANTHROPIC_API_KEY";

fn in_a_fresh_clone() -> Invocation {
    Invocation::default()
        .cloned(repository::url(), repository::NAME)
        .within(repository::NAME)
}

fn refused_naming(finished: &Finished, code: i32, named: &[&str]) {
    assert_eq!(
        finished.status.code(),
        Some(code),
        "the start was not refused as expected:\n{}",
        finished.err
    );
    assert!(finished.out.is_empty(), "{:?}", finished.out);
    for name in named {
        assert!(
            finished.err.contains(name),
            "the refusal does not name {name}:\n{}",
            finished.err
        );
    }
}

/// The review, from its first line to the last step it would apply.
fn reviewed(said: &str) -> Vec<String> {
    said.lines()
        .skip_while(|line| *line != "start")
        .take_while(|line| {
            line.starts_with("  ") || ["start", "already saved", "apply"].contains(line)
        })
        .map(str::to_owned)
        .collect()
}

fn inferred(said: &str) -> String {
    reviewed(said)
        .into_iter()
        .find(|line| line.trim_start().starts_with("inferred"))
        .unwrap_or_else(|| panic!("nothing was inferred:\n{said}"))
}

fn reached(shown: &Shown) -> Value {
    let record = shown
        .lines()
        .into_iter()
        .find(|line| line.starts_with('{'))
        .unwrap_or_else(|| panic!("no record reached the terminal:\n{}", shown.said));

    serde_json::from_str(record).expect("a record")
}

/// An Operator named for the Organization, which holds a key opencode signs in with.
async fn ready_in(kestrel: &Kestrel, organization: &str) -> Organization {
    kestrel.name_operator(organization).await;
    let organization = kestrel.declare_organization(organization).await;
    kestrel
        .hold_provider_credential(&organization, SIGN_IN, "sk-ant-api03-key")
        .await;

    organization
}

/// An install as naming its Operator leaves it, one Organization, `default`, with a sign-in.
async fn named(kestrel: Kestrel) -> Kestrel {
    ready_in(&kestrel, "default").await;

    kestrel
}

async fn declared(kestrel: &Kestrel) -> Vec<String> {
    let mut declared = Vec::new();
    for organization in kestrel.organizations().await {
        declared.push(format!("organization {}", organization.name));
        for project in kestrel.projects(&organization).await {
            declared.push(format!(
                "project {} {:?} {}",
                project.name, project.repositories, project.branch
            ));
        }
        for agent in kestrel.agents(&organization).await {
            declared.push(format!(
                "agent {} {} {:?}",
                agent.name, agent.harness, agent.declared.model
            ));
        }
        for held in kestrel.provider_credentials_held(&organization).await {
            declared.push(format!("credential {}", held.variable));
        }
        declared.push(format!(
            "workspaces {}",
            kestrel.workspaces(&organization.name).await.len()
        ));
    }

    declared
}

const NOTHING_STARTED: [&str; 3] = [
    "organization default",
    "credential ANTHROPIC_API_KEY",
    "workspaces 0",
];

#[tokio::test]
async fn one_command_takes_a_fresh_clone_and_an_empty_control_plane_to_a_session_carrying_its_brief()
 {
    let kestrel = named(
        Kestrel::dispatching_to(
            supervisor::binary(),
            &scripted_agent::playing(Script::Echoes),
        )
        .await,
    )
    .await;

    let started = ran_by(
        &kestrel,
        &[
            "start",
            "--brief",
            BRIEF,
            "--credential",
            PROVIDER_KEY,
            "--yes",
            "--json",
        ],
        in_a_fresh_clone().env(PROVIDER_KEY, A_PROVIDER_KEY),
    )
    .await;

    assert!(
        !started.out.join("\n").contains(A_PROVIDER_KEY) && !started.err.contains(A_PROVIDER_KEY),
        "the provider key was written back"
    );
    assert_eq!(
        started.jq("[.organization.name, .workspace.id == .session.workspace]"),
        r#"["default",true]"#
    );
    let started = started.json();
    assert_eq!(started["organization"]["name"], "default");
    assert_eq!(started["project"]["name"], repository::NAME);
    assert_eq!(started["agent"]["name"], "opencode");
    let workspace: WorkspaceId = started["workspace"]["id"]
        .as_str()
        .and_then(|id| id.parse().ok())
        .expect("a workspace identifier");
    let session: SessionId = started["session"]["id"]
        .as_str()
        .and_then(|id| id.parse().ok())
        .expect("a session identifier");

    let ended = kestrel.after_one_turn(session).await;
    assert_eq!(ended.exit, Some(Exit::Succeeded));
    let transcript = kestrel.transcript(workspace).await;
    assert_eq!(
        transcript[0].entry,
        Entry::ParticipantJoined {
            participant: "opencode".to_owned(),
        }
    );
    assert_eq!(
        transcript[2].entry,
        Entry::Brief {
            source: BriefSource::Operator {
                participant: Some("default".to_owned()),
            },
            brief: BRIEF.to_owned(),
        }
    );
    assert!(
        transcript.iter().any(|recorded| matches!(&recorded.entry,
            Entry::Said { participant, message, session_id: Some(id), .. }
                if participant == "opencode" && message == BRIEF && *id == session
        )),
        "the agent was never prompted with the brief"
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn without_a_terminal_or_yes_the_plan_is_reviewed_with_inferred_values_folded_and_nothing_applied()
 {
    let kestrel = named(Kestrel::boot().await).await;

    let refused = ran_by(&kestrel, &["start", "--brief", BRIEF], in_a_fresh_clone()).await;

    refused_naming(&refused, 2, &["--yes"]);
    let folded = inferred(&refused.err);
    for value in [
        "organization default (the only one)".to_owned(),
        format!("project {} (named for its repository)", repository::NAME),
        format!("repository {} (the clone's origin)", repository::url()),
        "base branch main (origin's default)".to_owned(),
        "agent opencode (named for its harness)".to_owned(),
        "harness opencode (kestrel's default)".to_owned(),
    ] {
        assert!(
            folded.contains(&value),
            "{value} is not folded in:\n{folded}"
        );
    }
    let review = reviewed(&refused.err);
    for said in [
        "already saved",
        "  operator default · sign-ins ",
        &format!("  declare the project {}", repository::NAME),
        "  declare the agent opencode on the harness opencode",
        "  open a workspace in",
    ] {
        assert!(
            review.iter().any(|line| line.starts_with(said)),
            "the review does not say {said}:\n{}",
            refused.err
        );
    }
    assert_eq!(declared(&kestrel).await, NOTHING_STARTED);

    kestrel.teardown().await;
}

#[tokio::test]
async fn flags_say_every_value_and_each_keeps_its_own_line() {
    let kestrel = named(Kestrel::boot().await).await;

    let started = ran_by(
        &kestrel,
        &[
            "start",
            "--brief",
            BRIEF,
            "--organization",
            "acme",
            "--project",
            "widgets",
            "--repository",
            repository::url(),
            "--branch",
            repository::EXISTING_BRANCH,
            "--agent",
            "builder",
            "--harness",
            "opencode",
            "--credential",
            SIGN_IN,
            "--yes",
            "--json",
        ],
        Invocation::default().env(SIGN_IN, "sk-ant-api03-key"),
    )
    .await;

    for flag in [
        "--organization",
        "--project",
        "--repository",
        "--branch",
        "--agent",
        "--harness",
    ] {
        assert!(
            reviewed(&started.err)
                .iter()
                .any(|line| line.trim_start().starts_with(flag)),
            "{flag} has no line of its own:\n{}",
            started.err
        );
    }
    let started = started.json();
    assert_eq!(started["organization"]["name"], "acme");
    assert_eq!(started["project"]["name"], "widgets");
    assert_eq!(started["agent"]["name"], "builder");
    let acme = kestrel
        .organizations()
        .await
        .into_iter()
        .find(|organization| organization.name == "acme")
        .expect("acme");
    assert_eq!(
        kestrel.projects(&acme).await[0].branch,
        repository::EXISTING_BRANCH
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn with_no_terminal_and_no_clone_it_fails_naming_every_flag_and_declares_nothing() {
    let kestrel = named(Kestrel::boot().await).await;

    let refused = ran_by(
        &kestrel,
        &["start", "--credential", PROVIDER_KEY, "--yes"],
        Invocation::default().given(""),
    )
    .await;

    refused_naming(
        &refused,
        2,
        &[
            "--repository",
            "no git clone",
            "--branch",
            "--brief",
            "--credential",
            PROVIDER_KEY,
        ],
    );
    assert_eq!(declared(&kestrel).await, NOTHING_STARTED);

    kestrel.teardown().await;
}

#[tokio::test]
async fn without_an_operator_it_fails_on_the_setup_gap() {
    let kestrel = Kestrel::boot().await;

    let refused = ran_by(
        &kestrel,
        &["start", "--brief", BRIEF, "--yes"],
        in_a_fresh_clone(),
    )
    .await;

    refused_naming(&refused, 78, &["operator"]);
    assert!(kestrel.organizations().await.is_empty());

    kestrel.teardown().await;
}

#[tokio::test]
async fn with_several_organizations_and_none_named_it_fails_naming_the_flag() {
    let kestrel = Kestrel::boot().await;
    kestrel.name_operator("ada").await;
    kestrel.declare_organization("acme").await;
    kestrel.declare_organization("globex").await;

    let refused = ran_by(
        &kestrel,
        &["start", "--brief", BRIEF, "--yes"],
        in_a_fresh_clone(),
    )
    .await;

    refused_naming(&refused, 2, &["--organization", "acme", "globex"]);
    for organization in kestrel.organizations().await {
        assert!(kestrel.projects(&organization).await.is_empty());
    }

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_declaration_the_plan_would_change_is_refused_before_anything_is_sent() {
    let kestrel = Kestrel::boot().await;
    let acme = ready_in(&kestrel, "acme").await;
    kestrel
        .declare_project(
            &acme,
            repository::NAME,
            &[repository::url().to_owned()],
            repository::BRANCH,
        )
        .await;

    let refused = ran_by(
        &kestrel,
        &[
            "start",
            "--brief",
            BRIEF,
            "--project",
            repository::NAME,
            "--repository",
            repository::other_url(),
            "--credential",
            PROVIDER_KEY,
            "--yes",
        ],
        in_a_fresh_clone().env(PROVIDER_KEY, A_PROVIDER_KEY),
    )
    .await;

    refused_naming(&refused, 4, &["repositories"]);
    assert_eq!(
        kestrel.projects(&acme).await[0].repositories,
        [repository::url()]
    );
    assert!(kestrel.agents(&acme).await.is_empty());
    assert_eq!(kestrel.provider_credentials_held(&acme).await.len(), 1);
    assert!(kestrel.workspaces("acme").await.is_empty());

    kestrel.teardown().await;
}

#[tokio::test]
async fn github_shorthand_is_shown_at_its_address_and_finds_the_project_declared_there() {
    let kestrel = Kestrel::boot().await;
    let acme = ready_in(&kestrel, "acme").await;
    kestrel
        .declare_project(
            &acme,
            "widgets",
            &["https://github.com/acme/widgets.git".to_owned()],
            repository::BRANCH,
        )
        .await;

    let started = ran_by(
        &kestrel,
        &[
            "start",
            "--brief",
            BRIEF,
            "--repository",
            "acme/widgets",
            "--branch",
            repository::BRANCH,
            "--yes",
        ],
        in_a_fresh_clone(),
    )
    .await;

    assert!(started.status.success(), "{}", started.err);
    let review = reviewed(&started.err).join("\n");
    assert!(
        review.contains("https://github.com/acme/widgets.git (from acme/widgets)"),
        "{review}"
    );
    assert!(
        inferred(&started.err).contains("project widgets (as widgets declares)"),
        "{review}"
    );
    assert_eq!(kestrel.projects(&acme).await.len(), 1);
    assert_eq!(kestrel.workspaces("acme").await.len(), 1);

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_clone_by_another_transport_finds_the_project_and_starts_against_its_declared_address() {
    let kestrel = Kestrel::boot().await;
    let acme = ready_in(&kestrel, "acme").await;
    kestrel
        .declare_project(
            &acme,
            "storefront",
            &["https://github.com/acme/widgets.git".to_owned()],
            repository::BRANCH,
        )
        .await;

    let started = ran_by(
        &kestrel,
        &["start", "--brief", BRIEF, "--yes"],
        Invocation::default()
            .cloned_with_origin(
                repository::url(),
                repository::NAME,
                "git@github.com:Acme/widgets",
            )
            .within(repository::NAME),
    )
    .await;

    assert!(started.status.success(), "{}", started.err);
    assert!(
        inferred(&started.err)
            .contains("project storefront (storefront declares the clone's origin)"),
        "{}",
        started.err
    );
    let workspaces = kestrel.workspaces("acme").await;
    assert_eq!(workspaces.len(), 1);
    assert_eq!(
        workspaces[0].checkout.repositories,
        ["https://github.com/acme/widgets.git"]
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_clone_two_projects_begin_with_infers_neither_and_names_both() {
    let kestrel = Kestrel::boot().await;
    let acme = ready_in(&kestrel, "acme").await;
    for name in ["storefront", "storefront-next"] {
        kestrel
            .declare_project(
                &acme,
                name,
                &["https://github.com/acme/widgets.git".to_owned()],
                repository::BRANCH,
            )
            .await;
    }

    let refused = ran_by(
        &kestrel,
        &["start", "--brief", BRIEF, "--yes"],
        Invocation::default()
            .cloned_with_origin(
                repository::url(),
                repository::NAME,
                "https://github.com/acme/widgets",
            )
            .within(repository::NAME),
    )
    .await;

    refused_naming(&refused, 2, &["--project", "storefront", "storefront-next"]);
    assert!(kestrel.workspaces("acme").await.is_empty());

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_repository_no_declaration_could_hold_is_refused_before_anything_is_declared() {
    let kestrel = Kestrel::boot().await;
    let acme = ready_in(&kestrel, "acme").await;

    let refused = ran_by(
        &kestrel,
        &[
            "start",
            "--brief",
            BRIEF,
            "--repository",
            "widgets",
            "--yes",
        ],
        in_a_fresh_clone(),
    )
    .await;

    refused_naming(&refused, 4, &["repositories"]);
    assert!(kestrel.projects(&acme).await.is_empty());
    assert!(kestrel.agents(&acme).await.is_empty());

    kestrel.teardown().await;
}

#[tokio::test]
async fn an_inferred_project_name_declared_otherwise_names_the_flag_to_pass() {
    let kestrel = Kestrel::boot().await;
    let acme = ready_in(&kestrel, "acme").await;
    kestrel
        .declare_project(
            &acme,
            repository::NAME,
            &[repository::other_url().to_owned()],
            repository::BRANCH,
        )
        .await;

    let refused = ran_by(
        &kestrel,
        &["start", "--brief", BRIEF, "--yes"],
        in_a_fresh_clone(),
    )
    .await;

    refused_naming(&refused, 2, &["--project"]);
    assert!(kestrel.workspaces("acme").await.is_empty());

    kestrel.teardown().await;
}

#[tokio::test]
async fn on_a_terminal_confirming_once_applies_the_plan_yes_applies_without_one() {
    let args = ["start", "--brief", BRIEF, "--json"];
    let (interactive, noninteractive) = (
        named(Kestrel::boot().await).await,
        named(Kestrel::boot().await).await,
    );

    let confirmed = ran_on_a_terminal_by(&interactive, &args, in_a_fresh_clone(), "y\n").await;
    let applied = ran_by(
        &noninteractive,
        &[&args[..], &["--yes"]].concat(),
        in_a_fresh_clone(),
    )
    .await;

    assert!(confirmed.status.success(), "{}", confirmed.said);
    assert_eq!(
        confirmed.said.matches(QUESTION).count(),
        1,
        "the start asked something other than one confirmation:\n{}",
        confirmed.said
    );
    assert!(!applied.err.contains(QUESTION), "{}", applied.err);
    assert_eq!(reviewed(&confirmed.said), reviewed(&applied.err));
    let (confirmed, applied) = (reached(&confirmed), applied.json());
    for field in ["organization", "project", "agent"] {
        assert_eq!(confirmed[field]["name"], applied[field]["name"], "{field}");
    }
    assert_eq!(confirmed["completed_initial_setup"], true);
    assert_eq!(
        declared(&interactive).await,
        declared(&noninteractive).await
    );

    interactive.teardown().await;
    noninteractive.teardown().await;
}

#[tokio::test]
async fn on_a_terminal_declining_the_reviewed_plan_changes_nothing() {
    let kestrel = named(Kestrel::boot().await).await;

    let shown = ran_on_a_terminal_by(
        &kestrel,
        &["start", "--brief", BRIEF],
        in_a_fresh_clone(),
        "n\n",
    )
    .await;

    assert!(shown.status.success(), "{}", shown.said);
    assert!(shown.said.contains("nothing was applied"), "{}", shown.said);
    assert!(!reviewed(&shown.said).is_empty(), "{}", shown.said);
    assert_eq!(declared(&kestrel).await, NOTHING_STARTED);

    kestrel.teardown().await;
}

#[tokio::test]
async fn with_output_on_a_terminal_and_input_piped_nothing_is_asked_or_applied() {
    let kestrel = named(Kestrel::boot().await).await;

    let refused = ran_on_a_terminal_by(
        &kestrel,
        &["start", "--brief", BRIEF],
        in_a_fresh_clone().given(""),
        "",
    )
    .await;

    assert_eq!(refused.status.code(), Some(2), "{}", refused.said);
    assert!(!refused.said.contains(QUESTION), "{}", refused.said);
    assert!(refused.said.contains("--yes"), "{}", refused.said);
    assert_eq!(declared(&kestrel).await, NOTHING_STARTED);

    kestrel.teardown().await;
}
