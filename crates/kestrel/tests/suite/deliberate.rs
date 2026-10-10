//! The declarations under test are the ones kestrel is dogfooded with.

use crate::support;

use std::time::Duration;

use jiff::SignedDuration;
use kestrel::domain::{Direction, Schedule, TriggerState, Workspace, WorkspaceId};
use kestrel::log::Entry;
use kestrel::trigger::{Asked, Fired};
use support::fixture::Fixture;
use support::github_stub::{self, GithubStub};
use support::{Kestrel, templates};

const DOGFOOD: &str = include_str!("../../../../.kestrel/triggers.yaml");
const REPOSITORY: &str = "openkestrel/kestrel";
const MAINTAINER: &str = "jtmthf";
const KESTREL: &str = "kestrel";
const PATIENCE: Duration = Duration::from_secs(30);

async fn dogfooding(kestrel: &Kestrel, stub: &GithubStub) {
    dogfood_declarations(kestrel).await;
    integrating(kestrel, stub).await;
}

/// Every declaration that must exist before an Event can be recorded. Registering the
/// Integration is what starts the poller, so declaring after it races the poll: an Event
/// recorded before a Trigger's `declared_at` is never matched, and the poller answers from an
/// exhausted script with a 404, never offering that Event again.
async fn dogfood_declarations(kestrel: &Kestrel) {
    Fixture::acme()
        .without_repositories()
        .agent("codex", "codex", None)
        .agent("claude", "claude", None)
        .declare(kestrel)
        .await;
    let applied = kestrel.apply_triggers("acme", DOGFOOD).await;
    assert!(
        applied.admitting_outsiders.is_empty(),
        "{:?}",
        applied.admitting_outsiders
    );
}

async fn integrating(kestrel: &Kestrel, stub: &GithubStub) {
    kestrel
        .register_integration(
            "acme",
            "github",
            REPOSITORY,
            &stub.base_url(),
            &[Direction::Inbound, Direction::Outbound],
            SignedDuration::from_millis(1),
        )
        .await;
}

async fn workspaces(kestrel: &Kestrel, count: usize) -> Vec<Workspace> {
    let deadline = tokio::time::Instant::now() + PATIENCE;
    loop {
        let workspaces = kestrel.workspaces("acme").await;
        if workspaces.len() >= count {
            return workspaces;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "{count} workspaces never opened"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

async fn brief(kestrel: &Kestrel, workspace: WorkspaceId) -> String {
    kestrel
        .transcript(workspace)
        .await
        .into_iter()
        .find_map(|recorded| match recorded.entry {
            Entry::Brief { brief, .. } => Some(brief),
            _ => None,
        })
        .expect("a workspace opened by a trigger starts with its brief")
}

async fn said(kestrel: &Kestrel, workspace: WorkspaceId) -> Vec<(String, String)> {
    kestrel
        .transcript(workspace)
        .await
        .into_iter()
        .flat_map(|recorded| match recorded.entry {
            Entry::Said {
                participant,
                message,
                ..
            } => vec![(participant, message)],
            Entry::Messages { messages } => messages
                .into_iter()
                .map(|message| (message.participant, message.message))
                .collect(),
            _ => Vec::new(),
        })
        .collect()
}

fn issue_link(issue: i64) -> String {
    format!("https://github.com/{REPOSITORY}/issues/{issue}")
}

#[tokio::test]
async fn neither_labels_nor_assignment_start_work() {
    let stub = GithubStub::start();
    stub.deliver(github_stub::labelled(41, "ready-for-agent"));
    stub.deliver(github_stub::labelled_carrying(
        42,
        "agent:codex",
        &["ready-for-agent"],
    ));
    stub.deliver(github_stub::assigned(45, KESTREL, MAINTAINER));
    stub.deliver(github_stub::issue_comment(20, 43, MAINTAINER, "@kestrel"));
    let kestrel = Kestrel::boot().await;
    dogfooding(&kestrel, &stub).await;

    let opened = workspaces(&kestrel, 1).await;

    assert_eq!(opened.len(), 1);
    assert_eq!(
        kestrel.sessions(opened[0].id).await[0].agent.name,
        "builder"
    );
    assert_eq!(opened[0].checkout.branch, "kestrel/issue-43");
    assert_eq!(
        brief(&kestrel, opened[0].id).await,
        format!(
            "/implement {}\n\nRead the issue and its comments with `gh issue view --comments` \
             before you start.",
            issue_link(43)
        )
    );
    for event in kestrel.events("acme").await {
        if event.occurrence.subject.as_deref() != Some("#43") {
            assert!(
                kestrel.firings(event.record_id).await.is_empty(),
                "{} fired",
                event.occurrence.r#type
            );
        }
    }

    kestrel.teardown().await;
}

#[tokio::test]
async fn the_maintainers_mention_starts_work_with_the_instruction_and_agent_it_names() {
    let stub = GithubStub::start();
    stub.deliver(github_stub::issue_comment(
        20,
        50,
        MAINTAINER,
        "@kestrel agent=codex $tdd the parser",
    ));
    let kestrel = Kestrel::boot().await;
    dogfooding(&kestrel, &stub).await;

    let opened = workspaces(&kestrel, 1).await.remove(0);

    assert_eq!(kestrel.sessions(opened.id).await[0].agent.name, "codex");
    assert!(
        brief(&kestrel, opened.id)
            .await
            .starts_with(&format!("$tdd the parser {}\n", issue_link(50)))
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn ordinary_comments_strangers_and_kestrel_itself_command_nothing() {
    let stub = GithubStub::start();
    stub.deliver(github_stub::issue_comment(
        21,
        51,
        MAINTAINER,
        "this one is ready",
    ));
    stub.deliver(github_stub::issue_comment(
        22,
        52,
        "a-stranger",
        "@kestrel /implement",
    ));
    stub.deliver(github_stub::issue_comment(
        23,
        53,
        "kestrel[bot]",
        "@kestrel done\n<!-- kestrel session 01a0 -->",
    ));
    stub.deliver(github_stub::issue_comment(
        24,
        54,
        MAINTAINER,
        "thanks @kestrel",
    ));
    stub.deliver(github_stub::issue_comment(
        25,
        56,
        MAINTAINER,
        "@kestrel-bot can you look?",
    ));
    stub.deliver(github_stub::issue_comment(26, 55, MAINTAINER, "@kestrel"));
    let kestrel = Kestrel::boot().await;
    dogfooding(&kestrel, &stub).await;

    let opened = workspaces(&kestrel, 1).await;

    assert_eq!(opened.len(), 1);
    assert!(
        brief(&kestrel, opened[0].id)
            .await
            .contains(&issue_link(55))
    );
    for event in kestrel.events("acme").await {
        let firings = kestrel.firings(event.record_id).await;
        match event.occurrence.subject.as_deref() {
            Some("#53") => assert!(
                firings.is_empty(),
                "the integration's own comment fired a trigger"
            ),
            Some("#55") => {}
            Some("#56") => assert!(
                firings.iter().all(|firing| firing.outcome == "failed"),
                "{firings:?}"
            ),
            _ => assert!(firings.is_empty()),
        }
    }

    kestrel.teardown().await;
}

#[tokio::test]
async fn repeated_signals_for_one_issue_open_one_workspace() {
    let stub = GithubStub::start();
    stub.deliver(github_stub::issue_comment(20, 43, MAINTAINER, "@kestrel"));
    stub.deliver(github_stub::issue_comment(21, 43, MAINTAINER, "@kestrel"));
    stub.deliver(github_stub::issue_comment(
        22,
        43,
        MAINTAINER,
        "@kestrel /implement",
    ));
    let kestrel = Kestrel::boot().await;
    dogfooding(&kestrel, &stub).await;

    let deadline = tokio::time::Instant::now() + PATIENCE;
    let outcomes = loop {
        let events = kestrel.events("acme").await;
        let mut outcomes = Vec::new();
        for event in &events {
            outcomes.extend(
                kestrel
                    .firings(event.record_id)
                    .await
                    .into_iter()
                    .map(|firing| firing.outcome),
            );
        }
        if events.len() == 3 && outcomes.len() == 3 {
            break outcomes;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "not every signal fired"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    };

    assert_eq!(kestrel.workspaces("acme").await.len(), 1);
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| *outcome == "opened")
            .count(),
        1,
        "{outcomes:?}"
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_command_on_an_open_workspaces_issue_is_not_also_heard_as_a_remark() {
    let stub = GithubStub::start();
    stub.deliver(github_stub::issue_comment(10, 43, MAINTAINER, "@kestrel"));
    let kestrel = Kestrel::boot().await;
    dogfooding(&kestrel, &stub).await;
    let workspace = workspaces(&kestrel, 1).await.remove(0);
    let first = kestrel.claim_session().await.expect("the first session");
    kestrel.complete_session(&first).await;

    stub.deliver(github_stub::issue_comment(
        20,
        43,
        MAINTAINER,
        "@kestrel /again",
    ));
    stub.deliver(github_stub::issue_comment(
        21,
        43,
        MAINTAINER,
        "and a test, please",
    ));

    let deadline = tokio::time::Instant::now() + PATIENCE;
    let heard = loop {
        let heard = said(&kestrel, workspace.id).await;
        let commanded = heard
            .iter()
            .any(|(by, message)| by == "delegated" && message.starts_with("/again "));
        let remarked = heard
            .iter()
            .any(|(by, message)| by == MAINTAINER && message == "and a test, please");
        if commanded && remarked {
            break heard;
        }
        if let Some(claimed) = kestrel.claim_session().await {
            kestrel.complete_session(&claimed).await;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the comments were never heard: {heard:?}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    };

    assert!(
        !heard
            .iter()
            .any(|(_, message)| message.starts_with("@kestrel")),
        "{heard:?}"
    );
    assert_eq!(kestrel.workspaces("acme").await.len(), 1);

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_comment_on_a_sealed_workspaces_issue_starts_nothing_and_a_command_continues_it() {
    let stub = GithubStub::start();
    stub.deliver(github_stub::issue_comment(
        9,
        43,
        MAINTAINER,
        "before the first command",
    ));
    stub.deliver(github_stub::issue_comment(10, 43, MAINTAINER, "@kestrel"));
    let kestrel = Kestrel::boot().await;
    dogfooding(&kestrel, &stub).await;
    let sealed = workspaces(&kestrel, 1).await.remove(0);
    let first = kestrel.claim_session().await.expect("the first session");
    kestrel.complete_session(&first).await;
    kestrel.seal_workspace(sealed.id).await;

    stub.deliver(github_stub::issue_comment(
        22,
        43,
        MAINTAINER,
        "still broken",
    ));
    stub.deliver(github_stub::issue_comment(
        21,
        43,
        MAINTAINER,
        "@kestrel /again",
    ));
    let opened = workspaces(&kestrel, 2).await;

    assert_eq!(opened.len(), 2);
    let continuation = opened
        .iter()
        .find(|workspace| workspace.id != sealed.id)
        .expect("the command continues the sealed workspace");
    assert_eq!(continuation.continues, Some(sealed.id));
    assert_eq!(continuation.correlation, sealed.correlation);
    assert!(
        brief(&kestrel, continuation.id)
            .await
            .starts_with("/again ")
    );
    stub.deliver(github_stub::issue_comment(
        23,
        43,
        MAINTAINER,
        "now please add a test",
    ));

    let deadline = tokio::time::Instant::now() + PATIENCE;
    let heard = loop {
        let heard = said(&kestrel, continuation.id).await;
        if heard
            .iter()
            .any(|(_, message)| message == "now please add a test")
        {
            break heard;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the newer remark was never heard: {heard:?}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    };
    assert_eq!(
        heard,
        vec![(MAINTAINER.to_owned(), "now please add a test".to_owned())]
    );
    assert!(!kestrel.database().holds_messages(continuation.id).await);

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_dispatch_starts_the_work_it_asks_for_on_the_issue_it_names() {
    let stub = GithubStub::start();
    stub.script_answer(
        "GET",
        "/issues/60",
        github_stub::issue(60, &["agent:claude"]),
    );
    let kestrel = Kestrel::boot().await;
    dogfooding(&kestrel, &stub).await;

    let fired = kestrel
        .dispatch(
            "acme",
            "delegated",
            60,
            Asked {
                instruction: Some("/tdd the parser"),
                agent: Some("codex"),
            },
        )
        .await
        .expect("the dispatch should fire");

    let Fired::Opened { workspace, .. } = fired else {
        panic!("the dispatch opened nothing: {fired:?}");
    };
    let workspace = kestrel.show_workspace(workspace).await;
    assert_eq!(kestrel.sessions(workspace.id).await[0].agent.name, "codex");
    assert_eq!(workspace.checkout.branch, "kestrel/issue-60");
    assert!(
        brief(&kestrel, workspace.id)
            .await
            .starts_with(&format!("/tdd the parser {}\n", issue_link(60)))
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_dispatch_fires_only_the_trigger_it_names() {
    let stub = GithubStub::start();
    stub.script_answer("GET", "/issues/60", github_stub::issue(60, &[]));
    let kestrel = Kestrel::boot().await;
    dogfooding(&kestrel, &stub).await;
    kestrel
        .declare_trigger(
            "acme",
            "everything",
            &format!(r#"{{"exact": {{"source": "https://github.com/{REPOSITORY}"}}}}"#),
            "kestrel",
            "builder",
        )
        .await;

    kestrel
        .dispatch("acme", "delegated", 60, Asked::default())
        .await
        .expect("the dispatch should fire");
    stub.deliver(github_stub::labelled(61, "bug"));

    let opened = workspaces(&kestrel, 2).await;
    assert_eq!(opened.len(), 2);
    let dispatched = kestrel
        .events("acme")
        .await
        .into_iter()
        .find(|event| event.occurrence.r#type == kestrel::trigger::DISPATCHED)
        .expect("the dispatch is recorded");
    let firings = kestrel.firings(dispatched.record_id).await;
    assert_eq!(firings.len(), 1);
    assert_eq!(firings[0].trigger, "delegated");

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_dispatch_asking_for_an_agent_the_trigger_does_not_allow_starts_nothing() {
    let stub = GithubStub::start();
    stub.script_answer("GET", "/issues/60", github_stub::issue(60, &[]));
    let kestrel = Kestrel::boot().await;
    let organization = kestrel.declare_organization("acme").await;
    kestrel
        .declare_agent(&organization, "stranger", "opencode", None)
        .await;
    dogfooding(&kestrel, &stub).await;

    let fired = kestrel
        .dispatch(
            "acme",
            "delegated",
            60,
            Asked {
                instruction: None,
                agent: Some("stranger"),
            },
        )
        .await
        .expect("the dispatch should be recorded");

    let Fired::Failed { because, .. } = fired else {
        panic!("the dispatch started work: {fired:?}");
    };
    assert_eq!(
        because,
        "the trigger delegated does not allow the agent stranger that was asked for"
    );
    assert!(kestrel.workspaces("acme").await.is_empty());

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_dispatch_test_renders_what_the_dispatch_then_starts_and_records_nothing() {
    let stub = GithubStub::start();
    stub.script_answer(
        "GET",
        "/issues/60",
        github_stub::issue(60, &["agent:claude"]),
    );
    let kestrel = Kestrel::boot().await;
    dogfooding(&kestrel, &stub).await;
    let asked = Asked {
        instruction: Some("/tdd the parser"),
        agent: Some("codex"),
    };

    let tested = kestrel
        .test_dispatch("acme", "delegated", 60, asked)
        .await
        .expect("the dispatch should test");

    assert!(kestrel.events("acme").await.is_empty());
    assert!(kestrel.workspaces("acme").await.is_empty());
    assert_eq!(
        kestrel.show_trigger("acme", "delegated").await.state,
        TriggerState::Enabled
    );
    assert!(tested.matches);
    let rendered = tested.rendered.expect("the dispatch should render");
    let agent = tested.agent.expect("the asked agent is allowed");

    let Fired::Opened {
        event, workspace, ..
    } = kestrel
        .dispatch("acme", "delegated", 60, asked)
        .await
        .expect("the dispatch should fire")
    else {
        panic!("the dispatch opened nothing");
    };
    assert_eq!(kestrel.events("acme").await.len(), 1);
    assert_eq!(kestrel.firings(event).await.len(), 1);
    let workspace = kestrel.show_workspace(workspace).await;
    assert_eq!(rendered.brief, brief(&kestrel, workspace.id).await);
    assert_eq!(
        rendered.branch.as_deref(),
        Some(workspace.checkout.branch.as_str())
    );
    assert_eq!(rendered.correlation, workspace.correlation);
    assert_eq!(agent, kestrel.sessions(workspace.id).await[0].agent.name);

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_dispatch_test_refuses_what_the_dispatch_refuses() {
    let stub = GithubStub::start();
    stub.script_answer("GET", "/issues/60", github_stub::issue(60, &[]));
    let kestrel = Kestrel::boot().await;
    let organization = kestrel.declare_organization("acme").await;
    kestrel
        .declare_agent(&organization, "stranger", "opencode", None)
        .await;
    dogfooding(&kestrel, &stub).await;

    let stranger = Asked {
        instruction: None,
        agent: Some("stranger"),
    };
    let refused = kestrel
        .test_dispatch("acme", "delegated", 60, stranger)
        .await
        .expect("the dispatch should test")
        .agent
        .expect_err("the trigger does not allow the agent");
    assert_eq!(
        refused.to_string(),
        "the trigger delegated does not allow the agent stranger that was asked for"
    );

    for _ in 0..2 {
        stub.script_answer(
            "GET",
            "/issues/61",
            github_stub::ScriptedResponse::answering(404),
        );
    }
    let unreadable = kestrel
        .test_dispatch("acme", "delegated", 61, Asked::default())
        .await
        .expect_err("github returns no issue 61");
    let undispatched = kestrel
        .dispatch("acme", "delegated", 61, Asked::default())
        .await
        .expect_err("github returns no issue 61");
    assert_eq!(unreadable.to_string(), undispatched.to_string());

    kestrel
        .try_declare_scheduled_trigger(
            "acme",
            "sweep",
            Schedule::Every(SignedDuration::from_hours(1)),
            &templates("Sweep", None, None),
        )
        .await
        .expect("an hourly schedule should declare");
    let scheduled = kestrel
        .test_dispatch("acme", "sweep", 60, Asked::default())
        .await
        .expect_err("a scheduled trigger cannot be dispatched");
    assert_eq!(
        scheduled.to_string(),
        "the trigger sweep fires on a schedule, so it cannot be dispatched"
    );
    let undispatchable = kestrel
        .dispatch("acme", "sweep", 60, Asked::default())
        .await
        .expect_err("a scheduled trigger cannot be dispatched");
    assert_eq!(scheduled.to_string(), undispatchable.to_string());
    assert!(kestrel.events("acme").await.is_empty());

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_command_works_ahead_of_a_blocker_on_an_unassigned_issue_and_says_so() {
    let stub = GithubStub::start();
    stub.deliver(github_stub::issue_comment(
        20,
        43,
        MAINTAINER,
        "@kestrel /implement",
    ));
    unassigned(&stub, 43);
    stub.script_answer(
        "GET",
        "/issues/comments/20",
        github_stub::ScriptedResponse::ok(
            github_stub::issue_comment(20, 43, MAINTAINER, "@kestrel /implement").payload["comment"].to_string(),
        ),
    );
    blocked_by(&stub, 43, 42);
    let kestrel = Kestrel::boot().await;
    dogfooding(&kestrel, &stub).await;

    let firing = command_firing(&kestrel).await;

    assert_eq!(firing.outcome, "opened", "{firing:?}");
    assert_eq!(
        firing.worked_ahead.as_deref(),
        Some(
            format!(
                "{} is blocked by {}, and {MAINTAINER} asked to work ahead",
                issue_link(43),
                issue_link(42)
            )
            .as_str()
        )
    );
    assert_eq!(kestrel.workspaces("acme").await.len(), 1);

    kestrel.teardown().await;
}

fn unassigned(stub: &GithubStub, issue: i64) {
    stub.script_answer(
        "GET",
        &format!("/issues/{issue}"),
        github_stub::ScriptedResponse::ok(
            serde_json::json!({
                "number": issue,
                "state": "open",
                "assignees": [],
            })
            .to_string(),
        ),
    );
}

async fn command_firing(kestrel: &Kestrel) -> kestrel::domain::Firing {
    let deadline = tokio::time::Instant::now() + PATIENCE;
    loop {
        for event in kestrel.events("acme").await {
            if event.occurrence.subject.as_deref() == Some("#43")
                && let Some(firing) = kestrel.firings(event.record_id).await.into_iter().next()
            {
                return firing;
            }
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the command did not fire"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn script_command(stub: &GithubStub) {
    stub.deliver(github_stub::issue_comment(20, 43, MAINTAINER, "@kestrel"));
}

#[tokio::test]
async fn a_closed_issue_holds_a_stale_command() {
    let stub = GithubStub::start();
    script_command(&stub);
    stub.script_answer(
        "GET",
        "/issues/43",
        github_stub::ScriptedResponse::ok(
            serde_json::json!({
                "number": 43,
                "state": "closed",
                "assignees": [{ "login": KESTREL }],
            })
            .to_string(),
        ),
    );
    let kestrel = Kestrel::boot().await;
    dogfooding(&kestrel, &stub).await;

    let firing = command_firing(&kestrel).await;
    assert_eq!(firing.outcome, "held");
    assert!(firing.failure.unwrap_or_default().contains("is closed"));
    assert!(kestrel.workspaces("acme").await.is_empty());

    kestrel.teardown().await;
}

#[tokio::test]
async fn an_issue_with_unknown_state_holds_the_start() {
    let stub = GithubStub::start();
    script_command(&stub);
    stub.script_answer(
        "GET",
        "/issues/43",
        github_stub::ScriptedResponse::ok(
            serde_json::json!({
                "number": 43,
                "assignees": [{ "login": KESTREL }],
            })
            .to_string(),
        ),
    );
    let kestrel = Kestrel::boot().await;
    dogfooding(&kestrel, &stub).await;

    let firing = command_firing(&kestrel).await;
    assert_eq!(firing.outcome, "held");
    assert!(firing.failure.unwrap_or_default().contains("unknown state"));
    assert!(kestrel.workspaces("acme").await.is_empty());

    kestrel.teardown().await;
}

#[tokio::test]
async fn an_edited_command_without_a_current_assignment_cancels_the_start() {
    let stub = GithubStub::start();
    script_command(&stub);
    unassigned(&stub, 43);
    stub.script_answer(
        "GET",
        "/issues/comments/20",
        github_stub::ScriptedResponse::ok(
            github_stub::issue_comment(20, 43, MAINTAINER, "never mind").payload["comment"]
                .to_string(),
        ),
    );
    let kestrel = Kestrel::boot().await;
    dogfooding(&kestrel, &stub).await;

    let firing = command_firing(&kestrel).await;
    assert_eq!(firing.outcome, "canceled");
    assert!(
        firing
            .failure
            .unwrap_or_default()
            .contains("no longer delegated")
    );
    assert!(kestrel.workspaces("acme").await.is_empty());

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_failed_dependency_query_holds_the_start() {
    let stub = GithubStub::start();
    script_command(&stub);
    stub.script_answer(
        "GET",
        "/issues/43/dependencies/blocked_by",
        github_stub::ScriptedResponse::answering(503),
    );
    let kestrel = Kestrel::boot().await;
    dogfooding(&kestrel, &stub).await;

    let firing = command_firing(&kestrel).await;
    assert_eq!(firing.outcome, "held");
    assert!(
        firing
            .failure
            .unwrap_or_default()
            .contains("readiness could not be checked")
    );
    assert!(kestrel.workspaces("acme").await.is_empty());

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_closed_native_dependency_does_not_hold_the_start() {
    let stub = GithubStub::start();
    script_command(&stub);
    stub.script_answer(
        "GET",
        "/issues/43/dependencies/blocked_by",
        github_stub::page(&[serde_json::json!({
            "number": 42,
            "state": "closed",
            "html_url": issue_link(42),
        })]),
    );
    let kestrel = Kestrel::boot().await;
    dogfooding(&kestrel, &stub).await;

    let opened = workspaces(&kestrel, 1).await;
    assert_eq!(opened.len(), 1);
    assert_eq!(command_firing(&kestrel).await.outcome, "opened");

    kestrel.teardown().await;
}

/// Assignment starts nothing in the dogfood declarations, so the automatic start under test is
/// declared beside them.
async fn delegating(kestrel: &Kestrel, stub: &GithubStub) {
    dogfood_declarations(kestrel).await;
    kestrel
        .declare_trigger_rendering(
            "acme",
            "assigned",
            &format!(
                r#"{{"all": [{{"exact": {{"source": "https://github.com/{REPOSITORY}"}}}},
                             {{"exact": {{"type": "com.github.issues.assigned"}}}}]}}"#
            ),
            "kestrel",
            "builder",
            &templates(
                "/implement {{ event.subject }}",
                None,
                Some("{{ event.source }}{{ event.subject }}"),
            ),
        )
        .await;
    integrating(kestrel, stub).await;
}

fn blocked_by(stub: &GithubStub, issue: i64, blocker: i64) {
    stub.script_answer(
        "GET",
        &format!("/issues/{issue}/dependencies/blocked_by"),
        github_stub::page(&[serde_json::json!({
            "number": blocker,
            "state": "open",
            "html_url": issue_link(blocker),
        })]),
    );
}

fn unblocked(stub: &GithubStub, issue: i64) {
    stub.script_answer(
        "GET",
        &format!("/issues/{issue}/dependencies/blocked_by"),
        github_stub::page(&[serde_json::json!({
            "number": 42,
            "state": "closed",
            "html_url": issue_link(42),
        })]),
    );
}

async fn firing_of(kestrel: &Kestrel, r#type: &str, outcome: &str) -> kestrel::domain::Firing {
    let deadline = tokio::time::Instant::now() + PATIENCE;
    loop {
        for event in kestrel.events("acme").await {
            if event.occurrence.r#type == r#type
                && let Some(firing) = kestrel
                    .firings(event.record_id)
                    .await
                    .into_iter()
                    .find(|firing| firing.outcome == outcome)
            {
                return firing;
            }
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "no {type} event was {outcome}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

const ASSIGNED: &str = "com.github.issues.assigned";

#[tokio::test]
async fn a_held_delegation_starts_once_its_blocker_closes() {
    let stub = GithubStub::start();
    stub.deliver(github_stub::assigned(43, KESTREL, MAINTAINER));
    blocked_by(&stub, 43, 42);
    let kestrel = Kestrel::boot().await;
    delegating(&kestrel, &stub).await;

    let held = firing_of(&kestrel, ASSIGNED, "held").await;
    assert!(held.failure.unwrap_or_default().contains(&issue_link(42)));
    assert!(kestrel.workspaces("acme").await.is_empty());

    unblocked(&stub, 43);
    stub.deliver(github_stub::issue_event(42, "closed", ""));

    let opened = workspaces(&kestrel, 1).await;
    assert_eq!(opened.len(), 1);
    assert_eq!(
        firing_of(&kestrel, ASSIGNED, "opened").await.trigger,
        "assigned"
    );
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert_eq!(kestrel.workspaces("acme").await.len(), 1);

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_held_delegation_waits_on_a_disabled_integration_and_starts_once_it_is_enabled() {
    let stub = GithubStub::start();
    stub.deliver(github_stub::assigned(43, KESTREL, MAINTAINER));
    blocked_by(&stub, 43, 42);
    let kestrel = Kestrel::boot().await;
    delegating(&kestrel, &stub).await;
    firing_of(&kestrel, ASSIGNED, "held").await;

    kestrel.disable_integration("acme", "github").await;
    unblocked(&stub, 43);
    let deadline = tokio::time::Instant::now() + PATIENCE;
    while !firing_of(&kestrel, ASSIGNED, "held")
        .await
        .failure
        .unwrap_or_default()
        .contains("the integration github is disabled")
    {
        assert!(
            tokio::time::Instant::now() < deadline,
            "the held firing never said its integration is disabled"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(kestrel.workspaces("acme").await.is_empty());

    kestrel.enable_integration("acme", "github").await;

    assert_eq!(workspaces(&kestrel, 1).await.len(), 1);

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_held_delegation_whose_unblocking_event_was_missed_starts_on_the_sweep() {
    let stub = GithubStub::start();
    stub.deliver(github_stub::assigned(43, KESTREL, MAINTAINER));
    blocked_by(&stub, 43, 42);
    let kestrel = Kestrel::boot().await;
    delegating(&kestrel, &stub).await;
    firing_of(&kestrel, ASSIGNED, "held").await;
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert!(kestrel.workspaces("acme").await.is_empty());

    unblocked(&stub, 43);
    let held = kestrel
        .events("acme")
        .await
        .into_iter()
        .find(|event| event.occurrence.r#type == ASSIGNED)
        .expect("the assignment is recorded");
    kestrel
        .last_considered(
            held.record_id,
            jiff::Timestamp::now() - SignedDuration::from_hours(1),
        )
        .await;

    assert_eq!(workspaces(&kestrel, 1).await.len(), 1);
    assert_eq!(
        firing_of(&kestrel, ASSIGNED, "opened").await.worked_ahead,
        None
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn unassigning_a_held_delegation_cancels_it() {
    let stub = GithubStub::start();
    stub.deliver(github_stub::assigned(43, KESTREL, MAINTAINER));
    blocked_by(&stub, 43, 42);
    let kestrel = Kestrel::boot().await;
    delegating(&kestrel, &stub).await;
    firing_of(&kestrel, ASSIGNED, "held").await;

    unassigned(&stub, 43);
    stub.deliver(github_stub::issue_event(43, "unassigned", ""));

    let canceled = firing_of(&kestrel, ASSIGNED, "canceled").await;
    assert!(
        canceled
            .failure
            .unwrap_or_default()
            .contains("no longer delegated")
    );
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert!(kestrel.workspaces("acme").await.is_empty());

    kestrel.teardown().await;
}

#[tokio::test]
async fn two_held_delegations_of_one_issue_start_it_once() {
    let stub = GithubStub::start();
    stub.deliver(github_stub::assigned(43, KESTREL, MAINTAINER));
    stub.deliver(github_stub::assigned(43, KESTREL, MAINTAINER));
    blocked_by(&stub, 43, 42);
    blocked_by(&stub, 43, 42);
    let kestrel = Kestrel::boot().await;
    delegating(&kestrel, &stub).await;
    firing_of(&kestrel, ASSIGNED, "canceled").await;

    stub.deliver(github_stub::issue_event(42, "closed", ""));

    let opened = workspaces(&kestrel, 1).await;
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert_eq!(kestrel.workspaces("acme").await.len(), 1);
    let mut outcomes = Vec::new();
    for event in kestrel.events("acme").await {
        if event.occurrence.r#type == ASSIGNED {
            outcomes.extend(
                kestrel
                    .firings(event.record_id)
                    .await
                    .into_iter()
                    .map(|firing| (firing.outcome, firing.workspace)),
            );
        }
    }
    outcomes.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(
        outcomes,
        vec![
            ("canceled".to_owned(), None),
            ("opened".to_owned(), Some(opened[0].id)),
        ]
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_dispatch_works_ahead_of_a_blocker_and_says_so() {
    let stub = GithubStub::start();
    unassigned(&stub, 60);
    unassigned(&stub, 60);
    blocked_by(&stub, 60, 42);
    let kestrel = Kestrel::boot().await;
    dogfooding(&kestrel, &stub).await;

    let fired = kestrel
        .dispatch("acme", "delegated", 60, Asked::default())
        .await
        .expect("the dispatch should fire");

    let Fired::Opened { event, .. } = fired else {
        panic!("the dispatch opened nothing: {fired:?}");
    };
    assert_eq!(
        kestrel.firings(event).await[0].worked_ahead.as_deref(),
        Some(
            format!(
                "{} is blocked by {}, and an operator's dispatch asked to work ahead",
                issue_link(60),
                issue_link(42)
            )
            .as_str()
        )
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_blocker_added_after_a_workspace_opens_does_not_freeze_it() {
    let stub = GithubStub::start();
    script_command(&stub);
    let kestrel = Kestrel::boot().await;
    dogfooding(&kestrel, &stub).await;
    let workspace = workspaces(&kestrel, 1).await.remove(0);
    let first = kestrel.claim_session().await.expect("the first session");
    kestrel.complete_session(&first).await;

    for _ in 0..4 {
        blocked_by(&stub, 43, 42);
    }
    stub.deliver(github_stub::issue_comment(
        21,
        43,
        MAINTAINER,
        "@kestrel /again",
    ));

    let deadline = tokio::time::Instant::now() + PATIENCE;
    let next = loop {
        if let Some(claimed) = kestrel.claim_session().await {
            break claimed;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the command never reached the open workspace"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    };
    assert_eq!(next.workspace, workspace.id);
    assert_eq!(kestrel.workspaces("acme").await.len(), 1);
    assert_eq!(
        firing_of(&kestrel, "com.github.issue_comment.created", "fed")
            .await
            .workspace,
        Some(workspace.id)
    );

    kestrel.teardown().await;
}
