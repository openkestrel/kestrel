mod support;

use std::time::Duration;

use jiff::SignedDuration;
use kestrel::domain::{
    Connection, Direction, GithubConnection, Integration, IntegrationId, OrganizationId,
    SessionCommand, SessionState,
};
use kestrel::integration::credential::App;
use kestrel::integration::github::Github;
use kestrel::link::Instruction;
use kestrel::log::{Entry, Message};
use kestrel::store::workspace::HeldMessageRefusal;
use kestrel_scripted_agent::{FIRST_MEMORY, LAST_MEMORY};
use support::Kestrel;
use support::github_stub::{self, GithubStub};
use support::scripted_agent::Script;
use support::supervisor::Supervisor;
use support::{A_PROVIDER_KEY, APP_ID, INSTALLATION_ID, PRIVATE_KEY, PROVIDER_KEY};

const REPOSITORY: &str = "jtmthf/kestrel";
const MAINTAINER: &str = "jack";
const ISSUE: i64 = 43;
const LISTED: &str = "/app/hook/deliveries?";
const PATIENCE: Duration = Duration::from_secs(30);

async fn a_workspace(kestrel: &Kestrel) -> kestrel::domain::Workspace {
    let organization = kestrel.declare_organization("acme").await;
    kestrel
        .declare_project(&organization, "kestrel", &[], "main")
        .await;
    kestrel
        .declare_agent(&organization, "builder", "opencode", None)
        .await;
    kestrel.open_workspace("acme", "kestrel", "builder").await
}

/// The Trigger comes before the poll: an Event recorded before the Trigger was declared fires
/// nothing.
async fn watching(kestrel: &Kestrel, stub: &GithubStub) {
    let organization = kestrel.declare_organization("acme").await;
    kestrel
        .declare_project(&organization, "kestrel", &[], "main")
        .await;
    kestrel
        .declare_agent(&organization, "builder", "opencode", None)
        .await;
    kestrel
        .declare_trigger(
            "acme",
            "ready",
            &support::labelled_on(REPOSITORY, "ready-for-agent"),
            "kestrel",
            "builder",
        )
        .await;
    kestrel
        .register_integration(
            "acme",
            "github",
            REPOSITORY,
            &stub.base_url(),
            &[Direction::Inbound],
            SignedDuration::from_millis(1),
        )
        .await;
}

async fn workspaces(kestrel: &Kestrel, count: usize) -> Vec<kestrel::domain::Workspace> {
    let deadline = tokio::time::Instant::now() + PATIENCE;
    loop {
        let workspaces = kestrel.workspaces("acme").await;
        if workspaces.len() == count {
            return workspaces;
        }
        assert!(tokio::time::Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

async fn sessions(kestrel: &Kestrel, workspace: kestrel::domain::WorkspaceId, count: usize) {
    let deadline = tokio::time::Instant::now() + PATIENCE;
    loop {
        if kestrel.sessions(workspace).await.len() == count {
            return;
        }
        assert!(tokio::time::Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

async fn message_arrived(
    kestrel: &Kestrel,
    workspace: kestrel::domain::WorkspaceId,
    message: &str,
) {
    let deadline = tokio::time::Instant::now() + PATIENCE;
    loop {
        if kestrel.transcript(workspace).await.iter().any(|recorded| {
            matches!(&recorded.entry, Entry::Said { message: said, .. } if said == message)
        }) {
            return;
        }
        assert!(tokio::time::Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

async fn requested(stub: &GithubStub, path: &str, after: usize) {
    let deadline = tokio::time::Instant::now() + PATIENCE;
    loop {
        let count = stub
            .requests()
            .iter()
            .filter(|request| request.url.contains(path))
            .count();
        if count > after {
            return;
        }
        assert!(tokio::time::Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

async fn pending_arrived(kestrel: &Kestrel, workspace: kestrel::domain::WorkspaceId) {
    let deadline = tokio::time::Instant::now() + PATIENCE;
    loop {
        if kestrel.has_pending_messages(workspace).await {
            return;
        }
        assert!(tokio::time::Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test]
async fn posting_a_message_into_an_idle_workspace_enqueues_its_next_session() {
    let kestrel = Kestrel::boot().await;
    let workspace = a_workspace(&kestrel).await;

    let session = kestrel
        .post(workspace.id, "operator", "please add the missing test")
        .await;

    assert_eq!(session.workspace, workspace.id);
    assert_eq!(session.state, SessionState::Queued);
    assert!(
        kestrel
            .transcript(workspace.id)
            .await
            .iter()
            .any(|recorded| {
                matches!(
                    &recorded.entry,
                    Entry::Said { participant, message, .. }
                        if participant == "operator" && message == "please add the missing test"
                )
            })
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_message_arriving_during_a_session_waits_for_that_session_to_end() {
    let kestrel = Kestrel::boot().await;
    let workspace = a_workspace(&kestrel).await;
    let active = kestrel.dispatch_session(workspace.id).await;
    let before: Vec<Entry> = kestrel
        .transcript(workspace.id)
        .await
        .into_iter()
        .map(|recorded| recorded.entry)
        .collect();

    assert!(
        kestrel
            .post_while_busy(workspace.id, "operator", "one more change")
            .await
            .is_none()
    );
    assert!(
        kestrel
            .post_while_busy(workspace.id, "operator", "and update the docs")
            .await
            .is_none()
    );
    assert_eq!(kestrel.sessions(workspace.id).await.len(), 1);
    let after: Vec<Entry> = kestrel
        .transcript(workspace.id)
        .await
        .into_iter()
        .map(|recorded| recorded.entry)
        .collect();
    assert_eq!(
        after, before,
        "a message posted while busy should not yet be on the record"
    );

    kestrel.complete_session(&active).await;

    let sessions = kestrel.sessions(workspace.id).await;
    assert_eq!(sessions.len(), 2);
    assert_eq!(sessions[1].state, SessionState::Queued);
    let messages = kestrel
        .transcript(workspace.id)
        .await
        .into_iter()
        .filter_map(|recorded| match recorded.entry {
            Entry::Messages { messages } => Some(messages),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        messages,
        vec![vec![
            Message {
                participant: "operator".to_owned(),
                message: "one more change".to_owned(),
            },
            Message {
                participant: "operator".to_owned(),
                message: "and update the docs".to_owned(),
            },
        ]]
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn held_messages_join_their_new_authors_once_in_message_order() {
    let kestrel = Kestrel::boot().await;
    let workspace = a_workspace(&kestrel).await;
    let active = kestrel.dispatch_session(workspace.id).await;

    assert!(
        kestrel
            .post_while_busy(workspace.id, "alice", "one")
            .await
            .is_none()
    );
    assert!(
        kestrel
            .post_while_busy(workspace.id, "bob", "two")
            .await
            .is_none()
    );
    assert!(
        kestrel
            .post_while_busy(workspace.id, "alice", "three")
            .await
            .is_none()
    );

    kestrel.complete_session(&active).await;

    let entries: Vec<Entry> = kestrel
        .transcript(workspace.id)
        .await
        .into_iter()
        .map(|recorded| recorded.entry)
        .collect();
    let messages_at = entries
        .iter()
        .position(|entry| matches!(entry, Entry::Messages { .. }))
        .expect("a turn took the held messages");
    assert_eq!(
        entries[messages_at - 2..messages_at],
        [
            Entry::ParticipantJoined {
                participant: "alice".to_owned(),
            },
            Entry::ParticipantJoined {
                participant: "bob".to_owned(),
            },
        ],
        "each new author joins once, in message order, directly before the batch"
    );

    let joins = entries
        .iter()
        .filter_map(|entry| match entry {
            Entry::ParticipantJoined { participant } => Some(participant.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(joins, vec!["builder", "alice", "bob"]);

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_cold_session_is_seeded_with_every_page_of_earlier_context() {
    let kestrel = Kestrel::boot().await;
    let workspace = a_workspace(&kestrel).await;
    let first = kestrel.dispatch_session(workspace.id).await;

    for index in 0..105 {
        let message = match index {
            0 => FIRST_MEMORY.to_owned(),
            104 => LAST_MEMORY.to_owned(),
            _ => format!("earlier message {index}"),
        };
        kestrel.said(&first, &message).await;
    }
    kestrel.complete_session(&first).await;
    let second = kestrel
        .post(workspace.id, "operator", "please continue")
        .await;
    let claimed = kestrel
        .claim_session()
        .await
        .expect("the second session should claim");
    assert_eq!(claimed.id, second.id);

    let on = kestrel.on_the_link(&claimed).await;

    let mut supervisor = Supervisor::provision_playing(&kestrel.link(), &on, Script::Recalls);
    supervisor.wait_until_it_says("reported connected").await;
    kestrel.start(&second, supervisor.harness()).await;
    supervisor.wait_until_it_says("reported answered").await;
    kestrel.stop_session(second.id).await;

    assert!(
        kestrel
            .transcript(workspace.id)
            .await
            .iter()
            .any(|recorded| {
                matches!(
                    &recorded.entry,
                    Entry::Said { message, .. } if message == "I remember the whole earlier context"
                )
            })
    );

    supervisor.lets_go_of(second.id).await;
    kestrel.teardown().await;
}

#[tokio::test]
async fn the_second_session_runs_through_the_supervisor_the_first_left_on_the_instance() {
    let kestrel = Kestrel::dispatching_to(
        support::supervisor::binary(),
        &support::scripted_agent::playing(Script::Lingers),
    )
    .await;
    let organization = kestrel.declare_organization("acme").await;
    kestrel
        .declare_project(&organization, "kestrel", &[], "main")
        .await;
    kestrel
        .declare_agent(&organization, "builder", "opencode", None)
        .await;
    kestrel
        .hold_provider_credential(&organization, PROVIDER_KEY, A_PROVIDER_KEY)
        .await;
    let workspace = kestrel.open_workspace("acme", "kestrel", "builder").await;

    let first = kestrel.post(workspace.id, "operator", FIRST_MEMORY).await;
    kestrel.answered(first.id, 1).await;
    kestrel.stop_session(first.id).await;
    let first = kestrel.session(first.id).await;
    let first_supervisor = first.supervisor.as_deref().expect("a supervisor");

    let second = kestrel.post(workspace.id, "operator", LAST_MEMORY).await;
    kestrel.answered(second.id, 1).await;
    let second = kestrel.session(second.id).await;
    let second_supervisor = second.supervisor.as_deref().expect("a supervisor");

    assert_eq!(first_supervisor, second_supervisor);
    assert_eq!(first.instance, second.instance);
    kestrel.stop_session(second.id).await;

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_github_comment_enqueues_a_second_session_in_the_originating_workspace() {
    let stub = GithubStub::start();
    stub.deliver(github_stub::labelled(ISSUE, "ready-for-agent"));
    let kestrel = Kestrel::boot().await;
    watching(&kestrel, &stub).await;
    let workspace = workspaces(&kestrel, 1).await.remove(0);
    let first = kestrel
        .claim_session()
        .await
        .expect("the first session should claim");

    let comments_before = stub
        .requests()
        .iter()
        .filter(|request| request.url.starts_with(LISTED))
        .count();
    stub.deliver(github_stub::issue_comment(
        11,
        ISSUE,
        "jack",
        "please add the missing test",
    ));
    requested(&stub, LISTED, comments_before).await;
    pending_arrived(&kestrel, workspace.id).await;
    assert_eq!(
        kestrel.sessions(workspace.id).await.len(),
        1,
        "the comment started a concurrent session"
    );
    assert!(!kestrel.transcript(workspace.id).await.iter().any(|recorded| {
        matches!(&recorded.entry, Entry::Said { message, .. } if message == "please add the missing test")
    }));

    kestrel.complete_session(&first).await;
    sessions(&kestrel, workspace.id, 2).await;

    assert_eq!(kestrel.workspaces("acme").await.len(), 1);
    assert!(
        kestrel
            .transcript(workspace.id)
            .await
            .iter()
            .any(|recorded| {
                matches!(
                    &recorded.entry,
                    Entry::Messages { messages }
                        if messages == &[Message {
                            participant: "jack".to_owned(),
                            message: "please add the missing test".to_owned(),
                        }]
                )
            })
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn comments_arriving_during_a_turn_wait_in_order_with_their_authors() {
    let stub = GithubStub::start();
    stub.deliver(github_stub::labelled(ISSUE, "ready-for-agent"));
    let kestrel = Kestrel::boot().await;
    watching(&kestrel, &stub).await;
    let workspace = workspaces(&kestrel, 1).await.remove(0);
    let first = kestrel
        .claim_session()
        .await
        .expect("the first session should claim");

    let comments_before = stub
        .requests()
        .iter()
        .filter(|request| request.url.starts_with(LISTED))
        .count();
    stub.deliver(github_stub::issue_comment(
        11,
        ISSUE,
        "jack",
        "one more change",
    ));
    stub.deliver(github_stub::issue_comment(
        12,
        ISSUE,
        "jill",
        "and update the docs",
    ));
    requested(&stub, LISTED, comments_before).await;
    pending_arrived(&kestrel, workspace.id).await;
    // Both comments are one poll's, and each is received in its own transaction; let the sweep
    // finish holding the second before the session ends.
    tokio::time::sleep(Duration::from_millis(300)).await;

    kestrel.complete_session(&first).await;
    sessions(&kestrel, workspace.id, 2).await;

    assert!(
        kestrel
            .transcript(workspace.id)
            .await
            .iter()
            .any(|recorded| {
                recorded.entry
                    == Entry::Messages {
                        messages: vec![
                            Message {
                                participant: "jack".to_owned(),
                                message: "one more change".to_owned(),
                            },
                            Message {
                                participant: "jill".to_owned(),
                                message: "and update the docs".to_owned(),
                            },
                        ],
                    }
            })
    );

    let entries: Vec<Entry> = kestrel
        .transcript(workspace.id)
        .await
        .into_iter()
        .map(|recorded| recorded.entry)
        .collect();
    let joins = entries
        .iter()
        .filter_map(|entry| match entry {
            Entry::ParticipantJoined { participant } => Some(participant.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        joins,
        vec!["builder", "jack", "jill"],
        "each commenter joins under their login, once"
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_comment_polled_with_its_origin_waits_for_the_workspace_to_open() {
    let stub = GithubStub::start();
    stub.deliver(github_stub::labelled(ISSUE, "ready-for-agent"));
    stub.deliver(github_stub::issue_comment(
        17,
        ISSUE,
        "jack",
        "picked up together",
    ));

    let kestrel = Kestrel::boot().await;
    watching(&kestrel, &stub).await;
    let workspace = workspaces(&kestrel, 1).await.remove(0);
    message_arrived(&kestrel, workspace.id, "picked up together").await;

    assert_eq!(kestrel.sessions(workspace.id).await.len(), 1);
    kestrel.teardown().await;
}

#[tokio::test]
async fn a_delivery_backlog_longer_than_a_page_loses_nothing() {
    let stub = GithubStub::start();
    let from = jiff::Timestamp::now();
    stub.deliver_at(
        github_stub::issue_comment(99, ISSUE, "jack", "read by the last poll"),
        from - SignedDuration::from_secs(1),
    );
    for id in 100..350 {
        stub.deliver_at(
            github_stub::issue_comment(id, ISSUE, "jack", &format!("comment {id}")),
            from + SignedDuration::from_millis(id),
        );
    }
    let integration = Integration {
        id: IntegrationId::generate(),
        organization: OrganizationId::generate(),
        name: "github".to_owned(),
        connection: Connection::Github(GithubConnection {
            repository: REPOSITORY.to_owned(),
            api: stub.base_url(),
            credential: App::held(APP_ID, INSTALLATION_ID, PRIVATE_KEY),
            bot_login: "kestrel[bot]".to_owned(),
            interval: SignedDuration::from_secs(1),
        }),
        carries: vec![Direction::Inbound],
        poll_due_at: None,
        deliveries_read_from: Some(from),
        last_event_refusal: None,
    };

    let listing = Github::dialling_out()
        .expect("the GitHub client")
        .deliveries(&integration, from)
        .await
        .expect("the delivery backlog should be read");

    assert_eq!(listing.listed.len(), 250);
    assert!(!listing.ran_out);
    assert!(
        listing
            .listed
            .windows(2)
            .all(|pair| pair[0].delivered_at < pair[1].delivered_at),
        "oldest first"
    );
    assert_eq!(
        stub.requests()
            .iter()
            .filter(|request| request.url.starts_with("/app/hook/deliveries?"))
            .count(),
        3
    );
}

async fn watching_correlated(kestrel: &Kestrel, stub: &GithubStub, correlation: &str) {
    let organization = kestrel.declare_organization("acme").await;
    kestrel
        .declare_project(&organization, "kestrel", &[], "main")
        .await;
    kestrel
        .declare_agent(&organization, "builder", "opencode", None)
        .await;
    kestrel
        .declare_trigger_rendering(
            "acme",
            "ready",
            &support::labelled_on(REPOSITORY, "ready-for-agent"),
            "kestrel",
            "builder",
            &support::templates(support::BRIEF, None, Some(correlation)),
        )
        .await;
    kestrel
        .register_integration(
            "acme",
            "github",
            REPOSITORY,
            &stub.base_url(),
            &[Direction::Inbound],
            SignedDuration::from_millis(1),
        )
        .await;
}

#[tokio::test]
async fn a_comment_on_a_sealed_workspace_feeds_the_open_one_holding_its_correlation() {
    let stub = GithubStub::start();
    stub.deliver(github_stub::labelled(ISSUE, "ready-for-agent"));
    let kestrel = Kestrel::boot().await;
    watching_correlated(&kestrel, &stub, "the release").await;
    let sealed = workspaces(&kestrel, 1).await.remove(0);
    let first = kestrel
        .claim_session()
        .await
        .expect("the first session should claim");
    kestrel.complete_session(&first).await;
    kestrel.seal_workspace(sealed.id).await;

    stub.deliver(github_stub::labelled(ISSUE + 1, "ready-for-agent"));
    let holding = workspaces(&kestrel, 2)
        .await
        .into_iter()
        .find(|workspace| workspace.id != sealed.id)
        .expect("the second label should continue the sealed workspace");
    stub.deliver(github_stub::issue_comment(
        12,
        ISSUE,
        "jack",
        "about the release",
    ));

    message_arrived(&kestrel, holding.id, "about the release").await;
    assert_eq!(kestrel.workspaces("acme").await.len(), 2);

    kestrel.teardown().await;
}

/// A Trigger that names the one login it obeys, so a Workspace it opened has an author it
/// authorizes and everyone else is a stranger to it.
async fn watching_a_named_actor(kestrel: &Kestrel, stub: &GithubStub) {
    let organization = kestrel.declare_organization("acme").await;
    kestrel
        .declare_project(&organization, "kestrel", &[], "main")
        .await;
    kestrel
        .declare_agent(&organization, "builder", "opencode", None)
        .await;
    kestrel
        .declare_trigger_rendering(
            "acme",
            "delegated",
            &serde_json::json!({"all": [
                {"exact": {"source": format!("https://github.com/{REPOSITORY}")}},
                {"exact": {"type": "com.github.issue_comment.created"}},
                {"exact": {"data.comment.user.login": MAINTAINER}},
                {"prefix": {"data.comment.body": "@kestrel"}},
            ]})
            .to_string(),
            "kestrel",
            "builder",
            &support::templates("Work on {{ event.subject }}", None, Some("the release")),
        )
        .await;
    kestrel
        .register_integration(
            "acme",
            "github",
            REPOSITORY,
            &stub.base_url(),
            &[Direction::Inbound],
            SignedDuration::from_millis(1),
        )
        .await;
}

/// A Trigger that names no author, so a Workspace it opened admits whatever the Event says: the
/// own-identity guard is the only thing left that can keep kestrel's voice out.
async fn watching_any_author(kestrel: &Kestrel, stub: &GithubStub) {
    let organization = kestrel.declare_organization("acme").await;
    kestrel
        .declare_project(&organization, "kestrel", &[], "main")
        .await;
    kestrel
        .declare_agent(&organization, "builder", "opencode", None)
        .await;
    kestrel
        .declare_trigger_rendering(
            "acme",
            "delegated",
            &serde_json::json!({"all": [
                {"exact": {"source": format!("https://github.com/{REPOSITORY}")}},
                {"exact": {"type": "com.github.issue_comment.created"}},
                {"prefix": {"data.comment.body": "@kestrel"}},
            ]})
            .to_string(),
            "kestrel",
            "builder",
            &support::templates("Work on {{ event.subject }}", None, Some("the release")),
        )
        .await;
    kestrel
        .register_integration(
            "acme",
            "github",
            REPOSITORY,
            &stub.base_url(),
            &[Direction::Inbound],
            SignedDuration::from_millis(1),
        )
        .await;
}

/// The maintainer's command, which is the comment that opens the workspace.
fn the_command(stub: &GithubStub) {
    stub.deliver(github_stub::issue_comment(
        10, ISSUE, MAINTAINER, "@kestrel",
    ));
}

/// Scripted only once the session is already active, so a remark is judged against an open session
/// rather than taken as its first prompt.
fn a_remark_from(stub: &GithubStub, author: &str, remark: &str) {
    stub.deliver(github_stub::issue_comment(11, ISSUE, author, remark));
}

/// The sweep records the remark as an Event before it decides whether to feed it, so an Event
/// on the issue is the signal that the decision has been made.
async fn the_remark_was_recorded(kestrel: &Kestrel, remark: &str) {
    let deadline = tokio::time::Instant::now() + PATIENCE;
    loop {
        let recorded = kestrel.events("acme").await.iter().any(|event| {
            event
                .occurrence
                .data
                .pointer("/comment/body")
                .and_then(serde_json::Value::as_str)
                == Some(remark)
        });
        if recorded {
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the remark was never polled"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test]
async fn a_remark_from_a_stranger_does_not_feed_an_open_workspace() {
    let stub = GithubStub::start();
    the_command(&stub);
    let kestrel = Kestrel::boot().await;
    watching_a_named_actor(&kestrel, &stub).await;
    let workspace = workspaces(&kestrel, 1).await.remove(0);
    let session = kestrel
        .claim_session()
        .await
        .expect("the command should have opened a session");
    a_remark_from(&stub, "a-stranger", "please also change the parser");

    the_remark_was_recorded(&kestrel, "please also change the parser").await;
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert!(
        !kestrel.has_pending_messages(workspace.id).await,
        "a stranger's remark was held as input to the session"
    );

    kestrel.complete_session(&session).await;
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert_eq!(
        kestrel.sessions(workspace.id).await.len(),
        1,
        "a stranger's remark started a session"
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_remark_from_the_trigger_actor_feeds_an_open_workspace() {
    let stub = GithubStub::start();
    the_command(&stub);
    let kestrel = Kestrel::boot().await;
    watching_a_named_actor(&kestrel, &stub).await;
    let workspace = workspaces(&kestrel, 1).await.remove(0);
    let session = kestrel
        .claim_session()
        .await
        .expect("the command should have opened a session");
    a_remark_from(&stub, MAINTAINER, "please also change the parser");

    pending_arrived(&kestrel, workspace.id).await;
    kestrel.complete_session(&session).await;
    sessions(&kestrel, workspace.id, 2).await;

    assert!(
        kestrel
            .transcript(workspace.id)
            .await
            .iter()
            .any(|recorded| {
                recorded.entry
                    == Entry::Messages {
                        messages: vec![Message {
                            participant: MAINTAINER.to_owned(),
                            message: "please also change the parser".to_owned(),
                        }],
                    }
            })
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_comment_from_the_integration_s_own_identity_is_never_heard_as_input() {
    let stub = GithubStub::start();
    stub.deliver(github_stub::issue_comment(
        10, ISSUE, MAINTAINER, "@kestrel",
    ));
    let kestrel = Kestrel::boot().await;
    watching_any_author(&kestrel, &stub).await;
    let workspace = workspaces(&kestrel, 1).await.remove(0);
    kestrel
        .claim_session()
        .await
        .expect("the command should have opened a session");

    stub.deliver(github_stub::issue_comment(
        11,
        ISSUE,
        "kestrel[bot]",
        "what kestrel said\n\n<!-- kestrel session 01a0 turn 1 -->",
    ));
    tokio::time::sleep(Duration::from_secs(2)).await;

    assert!(
        !kestrel.has_pending_messages(workspace.id).await,
        "kestrel heard its own comment as input"
    );
    assert_eq!(kestrel.sessions(workspace.id).await.len(), 1);

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_marker_does_not_silence_a_comment_from_an_operator() {
    let stub = GithubStub::start();
    stub.deliver(github_stub::issue_comment(
        10, ISSUE, MAINTAINER, "@kestrel",
    ));
    let kestrel = Kestrel::boot().await;
    watching_a_named_actor(&kestrel, &stub).await;
    let workspace = workspaces(&kestrel, 1).await.remove(0);
    kestrel
        .claim_session()
        .await
        .expect("the command should have opened a session");
    stub.deliver(github_stub::issue_comment(
        11,
        ISSUE,
        MAINTAINER,
        "what kestrel said\n\n<!-- kestrel session 01a0 turn 1 -->",
    ));

    pending_arrived(&kestrel, workspace.id).await;

    kestrel.teardown().await;
}

async fn entries(kestrel: &Kestrel, workspace: kestrel::domain::WorkspaceId) -> Vec<Entry> {
    kestrel
        .transcript(workspace)
        .await
        .into_iter()
        .map(|recorded| recorded.entry)
        .collect()
}

async fn drained(kestrel: &Kestrel, workspace: kestrel::domain::WorkspaceId) -> Vec<Vec<Message>> {
    entries(kestrel, workspace)
        .await
        .into_iter()
        .filter_map(|entry| match entry {
            Entry::Messages { messages } => Some(messages),
            _ => None,
        })
        .collect()
}

fn compact() -> SessionCommand {
    SessionCommand {
        name: "compact".to_owned(),
        description: "Compact the conversation".to_owned(),
        input_hint: Some("/compact".to_owned()),
    }
}

async fn prompt(kestrel: &Kestrel, session: &kestrel::domain::Session) -> String {
    kestrel.on_the_link(session).await;
    kestrel.start(session, support::harness()).await;
    let Instruction::Start { prompt, .. } = kestrel.instruction(session).await else {
        panic!("a Session starts with a start instruction");
    };

    prompt
}

fn instruction(prompt: &str) -> &str {
    match prompt.strip_prefix("Earlier context, oldest first:\n") {
        Some(rest) => rest
            .split_once("\n\n")
            .map_or(rest, |(_, instruction)| instruction),
        None => prompt,
    }
}

#[tokio::test]
async fn a_message_posted_mid_turn_is_listed_with_the_id_the_post_answered_with() {
    let kestrel = Kestrel::boot().await;
    let workspace = a_workspace(&kestrel).await;
    let active = kestrel.dispatch_session(workspace.id).await;

    let posted = kestrel
        .posted_while_busy(workspace.id, "alice", "one more change")
        .await;
    assert!(posted.session.is_none(), "{posted:?}");
    let held = posted.held_message.expect("a mid-turn message is held");
    assert_eq!(held.participant, "alice");
    assert_eq!(held.message, "one more change");
    assert!(held.edited_at.is_none());

    let listed = kestrel.held_messages(workspace.id).await;
    assert_eq!(listed, vec![held], "the post answers with the id it listed");
    assert!(
        kestrel.has_pending_messages(workspace.id).await,
        "the message waits for a Turn"
    );

    kestrel.complete_session(&active).await;
    kestrel.teardown().await;
}

#[tokio::test]
async fn an_author_edits_a_held_message_and_the_next_turn_carries_the_edited_text() {
    let kestrel = Kestrel::boot().await;
    let workspace = a_workspace(&kestrel).await;
    let active = kestrel.dispatch_session(workspace.id).await;
    let held = kestrel
        .posted_while_busy(workspace.id, "alice", "one more change")
        .await
        .held_message
        .expect("a mid-turn message is held");
    let before = entries(&kestrel, workspace.id).await;

    let edited = kestrel
        .edit_message(workspace.id, held.id, "alice", "the edited change")
        .await
        .expect("its author should edit it");
    assert_eq!(edited.id, held.id, "an edit keeps the message's id");
    assert_eq!(edited.message, "the edited change");
    assert_eq!(edited.participant, "alice");
    assert!(edited.edited_at.is_some(), "an edit is stamped");
    assert_eq!(
        entries(&kestrel, workspace.id).await,
        before,
        "an edit writes no Transcript entry"
    );

    kestrel.complete_session(&active).await;
    let next = kestrel
        .claim_session()
        .await
        .expect("the held message starts the next Session");
    assert_eq!(
        drained(&kestrel, workspace.id).await,
        vec![vec![Message {
            participant: "alice".to_owned(),
            message: "the edited change".to_owned(),
        }]]
    );
    assert_eq!(
        instruction(&prompt(&kestrel, &next).await),
        "the edited change"
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn an_author_withdraws_a_held_message_and_the_next_turn_never_sees_it() {
    let kestrel = Kestrel::boot().await;
    let workspace = a_workspace(&kestrel).await;
    let active = kestrel.dispatch_session(workspace.id).await;
    let held = kestrel
        .posted_while_busy(workspace.id, "alice", "never mind")
        .await
        .held_message
        .expect("a mid-turn message is held");
    let before = entries(&kestrel, workspace.id).await;

    kestrel
        .withdraw_message(workspace.id, held.id, "alice")
        .await
        .expect("its author should withdraw it");
    assert!(kestrel.held_messages(workspace.id).await.is_empty());
    assert!(
        !kestrel.has_pending_messages(workspace.id).await,
        "a withdrawn message is not waiting input"
    );
    assert_eq!(
        entries(&kestrel, workspace.id).await,
        before,
        "a withdrawal writes no Transcript entry"
    );

    kestrel.complete_session(&active).await;
    assert_eq!(
        kestrel.sessions(workspace.id).await.len(),
        1,
        "a withdrawn message starts no Session"
    );
    assert!(drained(&kestrel, workspace.id).await.is_empty());

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_name_other_than_the_authors_may_neither_edit_nor_withdraw_a_held_message() {
    let kestrel = Kestrel::boot().await;
    let workspace = a_workspace(&kestrel).await;
    let active = kestrel.dispatch_session(workspace.id).await;
    let held = kestrel
        .posted_while_busy(workspace.id, "alice", "one more change")
        .await
        .held_message
        .expect("a mid-turn message is held");

    for refusal in [
        kestrel
            .edit_message(workspace.id, held.id, "bob", "mine now")
            .await
            .expect_err("another name cannot edit it"),
        kestrel
            .withdraw_message(workspace.id, held.id, "bob")
            .await
            .expect_err("another name cannot withdraw it"),
    ] {
        assert_eq!(
            refusal.downcast_ref::<HeldMessageRefusal>(),
            Some(&HeldMessageRefusal::NotTheAuthor("alice".to_owned())),
            "{refusal}"
        );
    }
    assert_eq!(
        kestrel.held_messages(workspace.id).await,
        vec![held],
        "a refused change leaves the message as it was"
    );

    kestrel.complete_session(&active).await;
    kestrel.teardown().await;
}

#[tokio::test]
async fn an_edit_after_a_turn_took_the_message_or_after_withdrawal_is_refused() {
    let kestrel = Kestrel::boot().await;
    let workspace = a_workspace(&kestrel).await;
    let active = kestrel.dispatch_session(workspace.id).await;
    let first = kestrel
        .posted_while_busy(workspace.id, "alice", "the first thing")
        .await
        .held_message
        .expect("a mid-turn message is held");

    kestrel.complete_session(&active).await;
    let taken = kestrel
        .claim_session()
        .await
        .expect("the held message starts a Session");
    assert_eq!(
        drained(&kestrel, workspace.id).await,
        vec![vec![Message {
            participant: "alice".to_owned(),
            message: "the first thing".to_owned(),
        }]],
        "the Turn took it"
    );

    let second = kestrel
        .posted_while_busy(workspace.id, "alice", "a later thing")
        .await
        .held_message
        .expect("a message behind a working Session is held");
    assert_ne!(second.id, first.id, "a new message gets a new id");

    for refusal in [
        kestrel
            .edit_message(workspace.id, first.id, "alice", "changed my mind")
            .await
            .expect_err("a taken message cannot be edited"),
        kestrel
            .withdraw_message(workspace.id, first.id, "alice")
            .await
            .expect_err("a taken message cannot be withdrawn"),
    ] {
        assert_eq!(
            refusal.downcast_ref::<HeldMessageRefusal>(),
            Some(&HeldMessageRefusal::AlreadyTaken)
        );
        assert!(
            refusal.to_string().contains("already sent to the agent"),
            "{refusal}"
        );
    }
    assert_eq!(
        kestrel.held_messages(workspace.id).await,
        vec![second.clone()],
        "an edit to an old id does not touch a newer message"
    );

    kestrel
        .withdraw_message(workspace.id, second.id, "alice")
        .await
        .expect("its author withdraws the newer message");
    assert!(kestrel.held_messages(workspace.id).await.is_empty());
    for refusal in [
        kestrel
            .edit_message(workspace.id, second.id, "alice", "too late")
            .await
            .expect_err("a withdrawn message cannot be edited"),
        kestrel
            .withdraw_message(workspace.id, second.id, "alice")
            .await
            .expect_err("a withdrawn message cannot be withdrawn again"),
    ] {
        assert_eq!(
            refusal.downcast_ref::<HeldMessageRefusal>(),
            Some(&HeldMessageRefusal::AlreadyWithdrawn)
        );
        assert!(
            refusal.to_string().contains("already withdrawn"),
            "{refusal}"
        );
    }

    kestrel.complete_session(&taken).await;
    kestrel.teardown().await;
}

#[tokio::test]
async fn a_message_held_behind_a_pending_firing_session_is_listed_and_drains_after_it() {
    let kestrel = Kestrel::boot().await;
    let workspace = a_workspace(&kestrel).await;
    let active = kestrel.dispatch_session(workspace.id).await;
    kestrel
        .hold_session(workspace.id, "the firing's brief")
        .await;
    let held = kestrel
        .posted_while_busy(workspace.id, "alice", "after the firing")
        .await
        .held_message
        .expect("a message behind a firing session is held");
    assert_eq!(kestrel.held_messages(workspace.id).await, vec![held]);

    kestrel.complete_session(&active).await;
    let firing = kestrel.sessions(workspace.id).await;
    assert_eq!(firing.len(), 2, "the firing Session starts first");
    assert_eq!(firing[1].state, SessionState::Queued);
    assert_eq!(
        kestrel.held_messages(workspace.id).await.len(),
        1,
        "the message waits behind the firing Session"
    );

    let firing = kestrel
        .claim_session()
        .await
        .expect("the firing Session claims");
    assert_eq!(
        instruction(&prompt(&kestrel, &firing).await),
        "the firing's brief"
    );
    kestrel.complete_session(&firing).await;

    assert!(kestrel.held_messages(workspace.id).await.is_empty());
    assert_eq!(
        drained(&kestrel, workspace.id).await,
        vec![vec![Message {
            participant: "alice".to_owned(),
            message: "after the firing".to_owned(),
        }]],
        "the message drains after the firing Session"
    );

    let last = kestrel
        .claim_session()
        .await
        .expect("the held message starts a Session");
    kestrel.complete_session(&last).await;
    kestrel.teardown().await;
}

#[tokio::test]
async fn a_command_held_between_two_messages_drains_as_three_turns_in_order() {
    let kestrel = Kestrel::boot().await;
    let workspace = a_workspace(&kestrel).await;
    let active = kestrel.dispatch_session(workspace.id).await;
    kestrel.record_commands(&active, &[compact()]).await;
    for (who, what) in [
        ("alice", "the first thing"),
        ("alice", "/compact"),
        ("bob", "the last thing"),
    ] {
        kestrel
            .posted_while_busy(workspace.id, who, what)
            .await
            .held_message
            .expect("a mid-turn message is held");
    }
    assert_eq!(kestrel.held_messages(workspace.id).await.len(), 3);

    kestrel.complete_session(&active).await;
    let first = kestrel
        .claim_session()
        .await
        .expect("the messages before the command start a Session");
    assert_eq!(
        drained(&kestrel, workspace.id).await,
        vec![vec![Message {
            participant: "alice".to_owned(),
            message: "the first thing".to_owned(),
        }]],
        "the messages before the first command are one Turn"
    );
    assert_eq!(
        kestrel.held_messages(workspace.id).await.len(),
        2,
        "the command and what follows it wait"
    );

    kestrel.record_commands(&first, &[compact()]).await;
    kestrel.complete_session(&first).await;
    let middle = kestrel
        .claim_session()
        .await
        .expect("the command starts a Session of its own");
    assert_eq!(
        drained(&kestrel, workspace.id).await.last(),
        Some(&vec![Message {
            participant: "alice".to_owned(),
            message: "/compact".to_owned(),
        }]),
        "the command message is taken alone"
    );
    assert_eq!(
        instruction(&prompt(&kestrel, &middle).await),
        "/compact",
        "the command's own instruction is its whole text, unlabelled"
    );

    kestrel.record_commands(&middle, &[compact()]).await;
    kestrel.complete_session(&middle).await;
    let last = kestrel
        .claim_session()
        .await
        .expect("what follows the command starts the third Session");
    assert_eq!(
        drained(&kestrel, workspace.id).await,
        vec![
            vec![Message {
                participant: "alice".to_owned(),
                message: "the first thing".to_owned(),
            }],
            vec![Message {
                participant: "alice".to_owned(),
                message: "/compact".to_owned(),
            }],
            vec![Message {
                participant: "bob".to_owned(),
                message: "the last thing".to_owned(),
            }],
        ]
    );

    kestrel.complete_session(&last).await;
    kestrel.teardown().await;
}

#[tokio::test]
async fn a_slash_held_with_no_offered_command_drains_as_an_ordinary_message() {
    let kestrel = Kestrel::boot().await;
    let workspace = a_workspace(&kestrel).await;
    let active = kestrel.dispatch_session(workspace.id).await;
    kestrel.record_commands(&active, &[compact()]).await;
    for (who, what) in [("alice", "/notacommand"), ("bob", "and one more")] {
        kestrel
            .posted_while_busy(workspace.id, who, what)
            .await
            .held_message
            .expect("a mid-turn message is held");
    }

    kestrel.complete_session(&active).await;
    let next = kestrel
        .claim_session()
        .await
        .expect("the messages start one Session");
    assert_eq!(
        drained(&kestrel, workspace.id).await,
        vec![vec![
            Message {
                participant: "alice".to_owned(),
                message: "/notacommand".to_owned(),
            },
            Message {
                participant: "bob".to_owned(),
                message: "and one more".to_owned(),
            },
        ]],
        "an unoffered slash drains with the others"
    );
    assert_eq!(
        instruction(&prompt(&kestrel, &next).await),
        "alice: /notacommand\n\nbob: and one more"
    );

    kestrel.complete_session(&next).await;
    kestrel.teardown().await;
}
