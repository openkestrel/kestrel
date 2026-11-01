//! The Session's own ending is said only when it adds something to the responses the issue already
//! carries, and the turn responses reach it before the ending does (ADR-0024).

use kestrel::domain::{Exit, SessionState};
use kestrel::work::Report;

use crate::support::Kestrel;
use crate::support::github_stub::{self, GithubStub};
use crate::support::link_client::Link;
use crate::support::replies::{
    COMMENTS, a_working_session, a_workspace_from_the_issue, nothing_more_is_said, replies, report,
};

#[tokio::test]
async fn a_turns_response_reaches_the_issue_before_the_session_ends() {
    let stub = GithubStub::start();
    stub.script_answer("POST", COMMENTS, github_stub::created(1, "posted"));
    let kestrel = Kestrel::boot().await;
    let workspace = a_workspace_from_the_issue(&kestrel, &stub).await;
    let (session, on) = a_working_session(&kestrel, workspace.id).await;
    let link = Link::to(&kestrel.link());

    report(&link, &session, &on, 1, Report::Started).await;
    report(
        &link,
        &session,
        &on,
        2,
        Report::Said {
            message: "the first answer".to_owned(),
            completion: kestrel::log::Completion::at("2026-09-29T12:00:00Z".parse().unwrap()),
        },
    )
    .await;
    report(&link, &session, &on, 3, Report::Answered { usage: None }).await;

    let bodies = replies(&stub, 1).await;
    assert!(bodies[0].contains("the first answer"), "{}", bodies[0]);
    assert!(
        bodies[0].contains(&format!("session {} turn 1 -->", session.id)),
        "the reply does not carry this turn's marker: {}",
        bodies[0]
    );
    assert_eq!(
        kestrel.session(session.id).await.state,
        SessionState::Trailing
    );

    kestrel.stop_session(session.id).await;
    let ended = kestrel.session(session.id).await;
    assert_eq!(ended.exit, Some(Exit::Succeeded));
    assert_eq!(ended.outcome_message.as_deref(), Some("the first answer"));
    nothing_more_is_said(&stub, 1).await;

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_final_message_repeating_a_combined_turn_response_is_not_posted_again() {
    let stub = GithubStub::start();
    stub.script_answer("POST", COMMENTS, github_stub::created(1, "posted"));
    let kestrel = Kestrel::boot().await;
    let workspace = a_workspace_from_the_issue(&kestrel, &stub).await;
    let (session, on) = a_working_session(&kestrel, workspace.id).await;
    let link = Link::to(&kestrel.link());

    report(&link, &session, &on, 1, Report::Started).await;
    for (seq, message) in [(2, "The investigation is complete."), (3, "CI is green.")] {
        report(
            &link,
            &session,
            &on,
            seq,
            Report::Said {
                message: message.to_owned(),
                completion: kestrel::log::Completion::at("2026-09-29T12:00:00Z".parse().unwrap()),
            },
        )
        .await;
    }
    report(&link, &session, &on, 4, Report::Answered { usage: None }).await;
    replies(&stub, 1).await;
    report(
        &link,
        &session,
        &on,
        5,
        Report::Said {
            message: "The investigation is complete.\n\nCI is green.".to_owned(),
            completion: kestrel::log::Completion::at("2026-09-29T12:00:00Z".parse().unwrap()),
        },
    )
    .await;
    report(
        &link,
        &session,
        &on,
        6,
        Report::Finished {
            evidence: None,
            exit: Exit::Succeeded,
            usage: None,
        },
    )
    .await;

    assert_eq!(
        kestrel.session(session.id).await.exit,
        Some(Exit::Succeeded)
    );
    nothing_more_is_said(&stub, 1).await;
    kestrel.teardown().await;
}

#[tokio::test]
async fn new_final_information_after_a_turn_is_saved_and_reported_once() {
    let stub = GithubStub::start();
    stub.script_answer("POST", COMMENTS, github_stub::created(1, "posted"));
    let kestrel = Kestrel::boot().await;
    let workspace = a_workspace_from_the_issue(&kestrel, &stub).await;
    let (session, on) = a_working_session(&kestrel, workspace.id).await;
    let link = Link::to(&kestrel.link());

    report(&link, &session, &on, 1, Report::Started).await;
    report(
        &link,
        &session,
        &on,
        2,
        Report::Said {
            message: "The investigation is complete.".to_owned(),
            completion: kestrel::log::Completion::at("2026-09-29T12:00:00Z".parse().unwrap()),
        },
    )
    .await;
    report(&link, &session, &on, 3, Report::Answered { usage: None }).await;
    replies(&stub, 1).await;

    report(
        &link,
        &session,
        &on,
        4,
        Report::Said {
            message: "The follow-up found a regression.".to_owned(),
            completion: kestrel::log::Completion::at("2026-09-29T12:00:00Z".parse().unwrap()),
        },
    )
    .await;
    report(
        &link,
        &session,
        &on,
        5,
        Report::Finished {
            evidence: None,
            exit: Exit::Succeeded,
            usage: None,
        },
    )
    .await;

    let ended = kestrel.session(session.id).await;
    assert_eq!(ended.exit, Some(Exit::Succeeded));
    assert_eq!(
        ended.outcome_message.as_deref(),
        Some("The follow-up found a regression.")
    );
    let bodies = replies(&stub, 2).await;
    assert!(bodies[1].contains("The follow-up found a regression."));
    kestrel.complete_session(&session).await;
    assert_eq!(
        kestrel.session(session.id).await.outcome_message.as_deref(),
        Some("The follow-up found a regression.")
    );
    nothing_more_is_said(&stub, 2).await;

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_session_that_answered_no_turn_still_says_how_it_ended() {
    let stub = GithubStub::start();
    stub.script_answer("POST", COMMENTS, github_stub::created(1, "posted"));
    let kestrel = Kestrel::boot().await;
    let workspace = a_workspace_from_the_issue(&kestrel, &stub).await;
    let (session, on) = a_working_session(&kestrel, workspace.id).await;
    let link = Link::to(&kestrel.link());

    report(&link, &session, &on, 1, Report::Started).await;
    report(
        &link,
        &session,
        &on,
        2,
        Report::Finished {
            evidence: None,
            exit: Exit::Succeeded,
            usage: None,
        },
    )
    .await;

    let bodies = replies(&stub, 1).await;
    assert!(bodies[0].contains("session succeeded"), "{}", bodies[0]);
    assert!(
        bodies[0].contains(&format!("session {} -->", session.id)),
        "the outcome does not carry the session's marker: {}",
        bodies[0]
    );

    kestrel.teardown().await;
}

/// A Turn that fails the Session still posts the Turn it already answered, and the failure is said
/// because the exit status is information the responses did not carry.
#[tokio::test]
async fn a_failed_session_posts_its_turns_response_and_then_the_failure() {
    let stub = GithubStub::start();
    stub.script_answer("POST", COMMENTS, github_stub::created(1, "posted"));
    let kestrel = Kestrel::boot().await;
    let workspace = a_workspace_from_the_issue(&kestrel, &stub).await;
    let (session, on) = a_working_session(&kestrel, workspace.id).await;
    let link = Link::to(&kestrel.link());

    report(&link, &session, &on, 1, Report::Started).await;
    report(
        &link,
        &session,
        &on,
        2,
        Report::Said {
            message: "the first answer".to_owned(),
            completion: kestrel::log::Completion::at("2026-09-29T12:00:00Z".parse().unwrap()),
        },
    )
    .await;
    report(&link, &session, &on, 3, Report::Answered { usage: None }).await;
    replies(&stub, 1).await;

    report(
        &link,
        &session,
        &on,
        4,
        Report::Finished {
            evidence: None,
            exit: Exit::Failed {
                because: "the agent answered the prompt with nothing".to_owned(),
            },
            usage: None,
        },
    )
    .await;

    let bodies = replies(&stub, 2).await;
    assert!(bodies[1].contains("session failed"), "{}", bodies[1]);
    assert!(
        bodies[1].contains("the agent answered the prompt with nothing"),
        "{}",
        bodies[1]
    );

    kestrel.complete_session(&session).await;
    nothing_more_is_said(&stub, 2).await;
    kestrel.teardown().await;
}
