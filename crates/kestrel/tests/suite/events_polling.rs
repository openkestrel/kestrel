//! Discovering Events on a repository kestrel does not own by polling its GitHub App's Delivery
//! log (ADR-0055). An Event is identified by its Delivery, so an overlapping poll window records
//! each one exactly once, and a control plane that restarts carries on from where it stopped.

use std::time::Duration;

use jiff::SignedDuration;

use crate::support::events::{
    BOTH, FETCHED, LISTED, POLL_INTERVAL, REPOSITORY, labelled_ready, polls, requested, watching,
};
use crate::support::github_stub::{self, GithubStub, ScriptedResponse};
use crate::support::{INSTALLATION_ID, Kestrel};

#[tokio::test]
async fn events_on_a_watched_repository_are_recorded_and_listed() {
    let stub = GithubStub::start();
    stub.deliver(labelled_ready(43));
    let kestrel = Kestrel::stepped().await;
    watching(&kestrel, &stub, BOTH).await;

    kestrel.work_pass().await;
    let events = kestrel.events("acme").await;

    assert_eq!(events.len(), 1);
    assert_eq!(
        events[0].occurrence.source,
        format!("https://github.com/{REPOSITORY}")
    );
    assert_eq!(events[0].occurrence.r#type, "com.github.issues.labeled");
    assert_eq!(events[0].occurrence.specversion, "1.0");
    let data = kestrel::integration::github::EventData::new(&events[0].occurrence);
    assert_eq!(data.label(), Some("ready-for-agent"));
    assert_eq!(data.subject_issue(), Some(43));
    assert_eq!(data.actor(), Some("jtmthf"));

    kestrel.teardown().await;
}

#[tokio::test]
async fn two_polls_with_an_overlapping_window_record_each_event_once() {
    let stub = GithubStub::start();
    stub.deliver(labelled_ready(43));
    stub.deliver(labelled_ready(44));
    let kestrel = Kestrel::stepped().await;
    watching(&kestrel, &stub, BOTH).await;

    kestrel.work_pass().await;
    assert_eq!(kestrel.events("acme").await.len(), 2);
    kestrel.advance(POLL_INTERVAL).await;

    let events = kestrel.events("acme").await;
    assert_eq!(
        events.len(),
        2,
        "the same two events were recorded {} times over",
        events.len()
    );
    assert_eq!(
        requested(&stub, FETCHED),
        2,
        "a delivery already recorded had its payload fetched again"
    );
    kestrel.teardown().await;
}

#[tokio::test]
async fn two_integrations_in_one_organization_record_one_producer_event() {
    let first = GithubStub::start();
    let second = GithubStub::start();
    first.deliver(labelled_ready(43));
    second.deliver(labelled_ready(43));
    let kestrel = Kestrel::stepped().await;
    kestrel.declare_organization("acme").await;
    kestrel
        .register_integration(
            "acme",
            "first",
            REPOSITORY,
            &first.base_url(),
            BOTH,
            SignedDuration::from_secs(1),
        )
        .await;
    kestrel
        .register_integration(
            "acme",
            "second",
            REPOSITORY,
            &second.base_url(),
            BOTH,
            SignedDuration::from_secs(1),
        )
        .await;

    kestrel.work_pass().await;
    kestrel.advance(POLL_INTERVAL).await;
    assert_eq!(polls(&first), 2);
    assert_eq!(polls(&second), 2);

    assert_eq!(kestrel.events("acme").await.len(), 1);

    kestrel.teardown().await;
}

#[tokio::test]
async fn polling_resumes_after_a_restart_without_re_recording_what_it_already_saw() {
    let stub = GithubStub::start();
    stub.deliver(labelled_ready(43));
    let kestrel = Kestrel::stepped().await;
    watching(&kestrel, &stub, BOTH).await;
    kestrel.work_pass().await;
    assert_eq!(kestrel.events("acme").await.len(), 1);

    let kestrel = kestrel.kill_and_restart().await;
    let asked = polls(&stub);
    kestrel.advance(POLL_INTERVAL).await;
    assert_eq!(polls(&stub), asked + 1);

    let events = kestrel.events("acme").await;
    assert_eq!(
        events.len(),
        1,
        "a restarted control plane recorded what it had already seen"
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_first_poll_reads_only_what_was_delivered_since_registration() {
    let stub = GithubStub::start();
    let kestrel = Kestrel::stepped().await;
    stub.deliver_at(labelled_ready(41), kestrel.now());
    kestrel.away_for(Duration::from_secs(60 * 60));
    stub.deliver_at(labelled_ready(43), kestrel.now());
    watching(&kestrel, &stub, BOTH).await;

    kestrel.work_pass().await;
    kestrel.advance(POLL_INTERVAL).await;

    let events = kestrel.events("acme").await;
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].occurrence.subject.as_deref(), Some("#43"));

    kestrel.teardown().await;
}

#[tokio::test]
async fn another_installations_deliveries_are_not_this_integrations() {
    let stub = GithubStub::start();
    stub.deliver_to(
        labelled_ready(41),
        INSTALLATION_ID + 1,
        github_stub::REPOSITORY_ID,
    );
    stub.deliver(labelled_ready(43));
    let kestrel = Kestrel::stepped().await;
    watching(&kestrel, &stub, BOTH).await;

    kestrel.work_pass().await;
    kestrel.advance(POLL_INTERVAL).await;

    let events = kestrel.events("acme").await;
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].occurrence.subject.as_deref(), Some("#43"));
    assert_eq!(requested(&stub, FETCHED), 1);

    kestrel.teardown().await;
}

/// A poll away longer than GitHub keeps Deliveries cannot know what it missed.
#[tokio::test]
async fn deliveries_older_than_githubs_retention_are_reported_lost() {
    let stub = GithubStub::start();
    let kestrel = Kestrel::stepped().await;
    kestrel.declare_organization("acme").await;
    kestrel
        .register_integration(
            "acme",
            "github",
            REPOSITORY,
            &stub.base_url(),
            BOTH,
            SignedDuration::from_hours(1),
        )
        .await;
    kestrel.work_pass().await;
    assert_eq!(polls(&stub), 1);
    kestrel.away_for(Duration::from_secs(80 * 60 * 60));

    kestrel.work_pass().await;

    let refusal = kestrel.integrations("acme").await[0]
        .last_event_refusal
        .clone()
        .expect("nothing said deliveries were lost");

    assert_eq!(refusal.id, None);
    assert!(
        refusal.reason.contains("may have been lost"),
        "{}",
        refusal.reason
    );
    assert!(kestrel.events("acme").await.is_empty());

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_delivery_about_another_repository_the_app_is_installed_on_is_skipped() {
    let stub = GithubStub::start();
    stub.deliver_to(
        labelled_ready(41),
        INSTALLATION_ID,
        github_stub::REPOSITORY_ID + 1,
    );
    stub.deliver(labelled_ready(43));
    let kestrel = Kestrel::stepped().await;
    watching(&kestrel, &stub, BOTH).await;

    kestrel.work_pass().await;
    kestrel.advance(POLL_INTERVAL).await;

    let events = kestrel.events("acme").await;
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].occurrence.subject.as_deref(), Some("#43"));

    kestrel.teardown().await;
}

/// GitHub lists a Delivery a few seconds after it is made, so one made before the newest the
/// last poll read can still be listed after it.
#[tokio::test]
async fn a_delivery_listed_after_a_newer_one_is_still_read() {
    let stub = GithubStub::start();
    let kestrel = Kestrel::stepped().await;
    watching(&kestrel, &stub, BOTH).await;
    let ahead = kestrel.now() + SignedDuration::from_secs(20);
    stub.deliver_at(labelled_ready(43), ahead);
    kestrel.work_pass().await;
    assert_eq!(kestrel.events("acme").await.len(), 1);
    kestrel.advance(POLL_INTERVAL).await;

    stub.deliver_at(labelled_ready(44), ahead - SignedDuration::from_secs(10));

    kestrel.advance(POLL_INTERVAL).await;
    let events = kestrel.events("acme").await;
    assert_eq!(events.len(), 2);

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_delivery_whose_payload_cannot_be_fetched_leaves_the_poll_where_it_was() {
    let stub = GithubStub::start();
    for _ in 0..3 {
        stub.script_answer("GET", FETCHED, ScriptedResponse::answering(502));
    }
    stub.deliver(labelled_ready(43));
    stub.deliver(labelled_ready(44));
    let kestrel = Kestrel::stepped().await;
    watching(&kestrel, &stub, BOTH).await;
    let registered = kestrel.integrations("acme").await[0].deliveries_read_from;

    kestrel.work_pass().await;
    for attempt in 1..=3 {
        assert_eq!(requested(&stub, FETCHED), attempt);
        assert_eq!(
            kestrel.integrations("acme").await[0].deliveries_read_from,
            registered
        );
        assert!(kestrel.events("acme").await.is_empty());
        if attempt < 3 {
            kestrel.advance(POLL_INTERVAL).await;
        }
    }
    kestrel.advance(POLL_INTERVAL).await;
    let events = kestrel.events("acme").await;
    assert_eq!(events.len(), 2);

    kestrel.teardown().await;
}

#[tokio::test]
async fn the_poll_interval_survives_a_restart() {
    let stub = GithubStub::start();
    let kestrel = Kestrel::stepped().await;
    kestrel.declare_organization("acme").await;
    kestrel
        .register_integration(
            "acme",
            "github",
            REPOSITORY,
            &stub.base_url(),
            BOTH,
            SignedDuration::from_secs(97),
        )
        .await;

    let kestrel = kestrel.kill_and_restart().await;

    assert_eq!(
        kestrel.integrations("acme").await[0]
            .github()
            .expect("a github integration")
            .interval,
        SignedDuration::from_secs(97)
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_rate_limited_poll_loses_no_event_and_records_none_twice() {
    let stub = GithubStub::start();
    stub.script_answer("GET", LISTED, github_stub::rate_limited());
    stub.deliver(labelled_ready(43));
    let kestrel = Kestrel::stepped().await;
    watching(&kestrel, &stub, BOTH).await;
    kestrel.work_pass().await;
    assert!(kestrel.events("acme").await.is_empty());
    kestrel.advance(POLL_INTERVAL).await;

    assert_eq!(kestrel.events("acme").await.len(), 1);
    kestrel.advance(POLL_INTERVAL).await;
    let events = kestrel.events("acme").await;

    assert_eq!(events.len(), 1);
    assert_eq!(
        kestrel::integration::github::EventData::new(&events[0].occurrence).subject_issue(),
        Some(43)
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_payload_over_one_mebibyte_is_refused_rather_than_stored() {
    let stub = GithubStub::start();
    let oversized = "x".repeat(1024 * 1024 + 1);
    stub.deliver(labelled_ready(43));
    let comment = stub.deliver(github_stub::issue_comment(11, 43, "jack", &oversized));
    let kestrel = Kestrel::stepped().await;
    watching(&kestrel, &stub, BOTH).await;

    kestrel.work_pass().await;
    kestrel.advance(POLL_INTERVAL).await;
    assert_eq!(polls(&stub), 2);

    let events = kestrel.events("acme").await;
    assert_eq!(
        events.len(),
        1,
        "the oversized payload was stored and is {} event(s) big",
        events.len()
    );
    let integration = &kestrel.integrations("acme").await[0];
    let refusal = integration
        .last_event_refusal
        .as_ref()
        .expect("the oversized event refusal should remain visible");
    assert_eq!(refusal.id.as_deref(), Some(comment.as_str()));
    assert!(refusal.bytes.is_some_and(|bytes| bytes > 1024 * 1024));

    kestrel.acknowledge_event_refusal("acme", "github").await;
    assert!(
        kestrel.integrations("acme").await[0]
            .last_event_refusal
            .is_none()
    );

    kestrel.advance(POLL_INTERVAL).await;
    assert_eq!(
        kestrel.integrations("acme").await[0]
            .last_event_refusal
            .as_ref()
            .and_then(|refusal| refusal.id.as_deref()),
        Some(comment.as_str())
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_transient_failure_loses_no_event_and_records_none_twice() {
    let stub = GithubStub::start();
    stub.script_answer("GET", LISTED, ScriptedResponse::answering(502));
    stub.deliver(labelled_ready(43));
    let kestrel = Kestrel::stepped().await;
    watching(&kestrel, &stub, BOTH).await;
    kestrel.work_pass().await;
    assert!(kestrel.events("acme").await.is_empty());
    kestrel.advance(POLL_INTERVAL).await;

    assert_eq!(kestrel.events("acme").await.len(), 1);
    kestrel.advance(POLL_INTERVAL).await;

    assert_eq!(kestrel.events("acme").await.len(), 1);

    kestrel.teardown().await;
}
