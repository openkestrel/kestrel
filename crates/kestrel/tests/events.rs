//! Discovering Events on a repository kestrel does not own by polling its GitHub App's Delivery
//! log (ADR-0055). An Event is identified by its Delivery, so an overlapping poll window records
//! each one exactly once, and a control plane that restarts carries on from where it stopped.

mod support;

use std::time::Duration;

use jiff::{SignedDuration, Timestamp};
use kestrel::domain::{
    Connection, Direction, Event, GithubConnection, Integration, IntegrationId, OrganizationId,
};
use kestrel::integration::credential::App;
use kestrel::integration::github::Github;
use support::github_stub::{self, GithubStub, INSTALLATION_TOKEN, ScriptedResponse};
use support::{APP_ID, INSTALLATION_ID, Kestrel, PRIVATE_KEY};

const PATIENCE: Duration = Duration::from_secs(30);
const REPOSITORY: &str = "jtmthf/kestrel";
const BOTH: &[Direction] = &[Direction::Inbound, Direction::Outbound];

/// Sooner than the wheel's own sweep, so what paces these tests is the sweep rather than a
/// wait written into them.
fn eagerly() -> SignedDuration {
    SignedDuration::from_millis(1)
}

async fn watching(kestrel: &Kestrel, stub: &GithubStub, carries: &[Direction]) {
    kestrel.declare_organization("acme").await;
    kestrel
        .register_integration(
            "acme",
            "github",
            REPOSITORY,
            &stub.base_url(),
            carries,
            eagerly(),
        )
        .await;
}

async fn recorded(kestrel: &Kestrel, count: usize) -> Vec<Event> {
    let deadline = tokio::time::Instant::now() + PATIENCE;

    loop {
        let events = kestrel.events("acme").await;
        if events.len() >= count {
            return events;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "{count} events were never recorded; {} were",
            events.len()
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

const LISTED: &str = "/app/hook/deliveries?";
const FETCHED: &str = "/app/hook/deliveries/";

/// How many of a stub's requests listed the Delivery log, rather than the one-off `GET /app` a
/// registration makes to learn the Integration's bot login.
fn polls(stub: &GithubStub) -> usize {
    requested(stub, LISTED)
}

fn requested(stub: &GithubStub, path: &str) -> usize {
    stub.requests()
        .iter()
        .filter(|request| request.url.starts_with(path))
        .count()
}

async fn polled(stub: &GithubStub, times: usize) {
    let deadline = tokio::time::Instant::now() + PATIENCE;

    loop {
        let asked = polls(stub);
        if asked >= times {
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "github was polled {asked} times rather than {times}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn labelled_ready(issue: i64) -> github_stub::Delivery {
    github_stub::labelled(issue, "ready-for-agent")
}

#[tokio::test]
async fn events_on_a_watched_repository_are_recorded_and_listed() {
    let stub = GithubStub::start();
    stub.deliver(labelled_ready(43));
    let kestrel = Kestrel::boot().await;
    watching(&kestrel, &stub, BOTH).await;

    let events = recorded(&kestrel, 1).await;

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
async fn an_integration_declares_which_directions_it_carries() {
    let kestrel = Kestrel::boot().await;
    let stub = GithubStub::start();
    watching(&kestrel, &stub, &[Direction::Inbound]).await;

    let registered = kestrel.integrations("acme").await;

    assert_eq!(registered.len(), 1);
    assert!(registered[0].carries(Direction::Inbound));
    assert!(!registered[0].carries(Direction::Outbound));

    kestrel.teardown().await;
}

/// The direction is what an Integration does rather than a label beside it: nothing polls one
/// that only carries kestrel's requests outward.
#[tokio::test]
async fn an_integration_that_carries_only_outbound_is_never_polled() {
    let outbound_only = GithubStub::start();
    let polling = GithubStub::start();
    let kestrel = Kestrel::boot().await;

    kestrel.declare_organization("acme").await;
    kestrel
        .register_integration(
            "acme",
            "outbound",
            REPOSITORY,
            &outbound_only.base_url(),
            &[Direction::Outbound],
            eagerly(),
        )
        .await;
    kestrel
        .register_integration(
            "acme",
            "inbound",
            REPOSITORY,
            &polling.base_url(),
            &[Direction::Inbound],
            eagerly(),
        )
        .await;

    polled(&polling, 3).await;

    assert_eq!(
        polls(&outbound_only),
        0,
        "an integration that carries nothing inbound was polled"
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn two_polls_with_an_overlapping_window_record_each_event_once() {
    let stub = GithubStub::start();
    stub.deliver(labelled_ready(43));
    stub.deliver(labelled_ready(44));
    let kestrel = Kestrel::boot().await;
    watching(&kestrel, &stub, BOTH).await;

    recorded(&kestrel, 2).await;
    polled(&stub, 3).await;

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
    let kestrel = Kestrel::boot().await;
    kestrel.declare_organization("acme").await;
    kestrel
        .register_integration(
            "acme",
            "first",
            REPOSITORY,
            &first.base_url(),
            BOTH,
            eagerly(),
        )
        .await;
    kestrel
        .register_integration(
            "acme",
            "second",
            REPOSITORY,
            &second.base_url(),
            BOTH,
            eagerly(),
        )
        .await;

    polled(&first, 2).await;
    polled(&second, 2).await;

    assert_eq!(kestrel.events("acme").await.len(), 1);

    kestrel.teardown().await;
}

#[tokio::test]
async fn polling_resumes_after_a_restart_without_re_recording_what_it_already_saw() {
    let stub = GithubStub::start();
    stub.deliver(labelled_ready(43));
    let kestrel = Kestrel::boot().await;
    watching(&kestrel, &stub, BOTH).await;
    recorded(&kestrel, 1).await;

    let kestrel = kestrel.kill_and_restart().await;
    let asked = polls(&stub);
    polled(&stub, asked + 1).await;

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
    stub.deliver_at(
        labelled_ready(41),
        Timestamp::now() - SignedDuration::from_hours(1),
    );
    stub.deliver(labelled_ready(43));
    let kestrel = Kestrel::boot().await;
    watching(&kestrel, &stub, BOTH).await;

    recorded(&kestrel, 1).await;
    polled(&stub, 3).await;

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
    let kestrel = Kestrel::boot().await;
    watching(&kestrel, &stub, BOTH).await;

    recorded(&kestrel, 1).await;
    polled(&stub, 3).await;

    assert_eq!(kestrel.events("acme").await.len(), 1);
    assert_eq!(requested(&stub, FETCHED), 1);

    kestrel.teardown().await;
}

/// A poll away longer than GitHub keeps Deliveries cannot know what it missed.
#[tokio::test]
async fn deliveries_older_than_githubs_retention_are_reported_lost() {
    let stub = GithubStub::start();
    let kestrel = Kestrel::boot().await;
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
    polled(&stub, 1).await;
    let pool = support::database(kestrel.data_dir()).await;
    let away = (Timestamp::now() - SignedDuration::from_hours(80)).to_string();
    sqlx::query(
        "UPDATE integration SET deliveries_read_from = ?, last_polled_at = ?, poll_due_at = ?",
    )
    .bind(&away)
    .bind(&away)
    .bind(Timestamp::now().to_string())
    .execute(&pool)
    .await
    .expect("the integration should be set back");

    let deadline = tokio::time::Instant::now() + PATIENCE;
    let refusal = loop {
        if let Some(refusal) = kestrel.integrations("acme").await[0]
            .last_event_refusal
            .clone()
        {
            break refusal;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "nothing said deliveries were lost"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    };

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
    let kestrel = Kestrel::boot().await;
    watching(&kestrel, &stub, BOTH).await;

    recorded(&kestrel, 1).await;
    polled(&stub, 3).await;

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
    let kestrel = Kestrel::boot().await;
    watching(&kestrel, &stub, BOTH).await;
    let ahead = Timestamp::now() + SignedDuration::from_secs(20);
    stub.deliver_at(labelled_ready(43), ahead);
    recorded(&kestrel, 1).await;
    polled(&stub, polls(&stub) + 2).await;

    stub.deliver_at(labelled_ready(44), ahead - SignedDuration::from_secs(10));

    let events = recorded(&kestrel, 2).await;
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
    let kestrel = Kestrel::boot().await;
    watching(&kestrel, &stub, BOTH).await;
    let registered = kestrel.integrations("acme").await[0].deliveries_read_from;

    while requested(&stub, FETCHED) < 3 {
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert_eq!(
        kestrel.integrations("acme").await[0].deliveries_read_from,
        registered,
        "a poll that could not fetch a payload moved on past it"
    );
    assert!(
        kestrel.events("acme").await.is_empty(),
        "a poll that could not read every new delivery recorded some of them"
    );

    let events = recorded(&kestrel, 2).await;
    assert_eq!(events.len(), 2);

    kestrel.teardown().await;
}

#[tokio::test]
async fn the_poll_interval_survives_a_restart() {
    let stub = GithubStub::start();
    let kestrel = Kestrel::boot().await;
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
    let kestrel = Kestrel::boot().await;
    watching(&kestrel, &stub, BOTH).await;

    let events = recorded(&kestrel, 1).await;

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
    let kestrel = Kestrel::boot().await;
    watching(&kestrel, &stub, BOTH).await;

    polled(&stub, 4).await;

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

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_transient_failure_loses_no_event_and_records_none_twice() {
    let stub = GithubStub::start();
    stub.script_answer("GET", LISTED, ScriptedResponse::answering(502));
    stub.deliver(labelled_ready(43));
    let kestrel = Kestrel::boot().await;
    watching(&kestrel, &stub, BOTH).await;

    recorded(&kestrel, 1).await;
    polled(&stub, 3).await;

    assert_eq!(kestrel.events("acme").await.len(), 1);

    kestrel.teardown().await;
}

/// The Delivery log is the App's own, so the poll presents the App's JWT, not an installation token.
#[tokio::test]
async fn the_poll_presents_the_apps_own_credential() {
    let stub = GithubStub::start();
    stub.deliver(labelled_ready(43));
    let kestrel = Kestrel::boot().await;
    watching(&kestrel, &stub, BOTH).await;

    recorded(&kestrel, 1).await;

    for request in stub
        .requests()
        .iter()
        .filter(|request| request.url.starts_with("/app/hook/deliveries"))
    {
        let authorization = request
            .headers
            .iter()
            .find(|(name, _)| name == "authorization")
            .map(|(_, value)| value.as_str())
            .expect("the poll should authenticate");
        let jwt = authorization
            .strip_prefix("Bearer ")
            .expect("a bearer credential");
        assert_eq!(jwt.split('.').count(), 3, "{jwt} is not a JWT");
        assert!(!authorization.contains(INSTALLATION_TOKEN));
    }

    kestrel.teardown().await;
}

/// An installation token lasts an hour; kestrel never keeps reading on one minted that long
/// ago. Rather than waiting out a real hour, the stub mints one good for less than
/// `EXPIRY_MARGIN`, so it already reads as stale by the time anything asks for it again.
#[tokio::test]
async fn an_installation_token_nearing_expiry_is_replaced_before_it_is_used_again() {
    let stub = GithubStub::start();
    stub.script_answer(
        "POST",
        "/access_tokens",
        github_stub::minted_token("first-installation-token", SignedDuration::from_secs(30)),
    );
    stub.script_answer(
        "POST",
        "/access_tokens",
        github_stub::minted_token("second-installation-token", SignedDuration::from_hours(1)),
    );
    let integration = Integration {
        id: IntegrationId::generate(),
        organization: OrganizationId::generate(),
        name: "github".to_owned(),
        connection: Connection::Github(GithubConnection {
            repository: REPOSITORY.to_owned(),
            api: stub.base_url(),
            credential: App::held(APP_ID, INSTALLATION_ID, PRIVATE_KEY),
            bot_login: "kestrel[bot]".to_owned(),
            interval: eagerly(),
            repository_id: github_stub::REPOSITORY_ID,
        }),
        carries: BOTH.to_vec(),
        poll_due_at: None,
        deliveries_read_from: None,
        last_polled_at: None,
        last_event_refusal: None,
    };
    let github = Github::dialling_out().expect("the GitHub client");

    for _ in 0..2 {
        github
            .issue(&integration, 43)
            .await
            .expect("the issue should read");
    }

    let authorizations: Vec<String> = stub
        .requests()
        .iter()
        .filter(|request| request.url.ends_with("/issues/43"))
        .filter_map(|request| {
            request
                .headers
                .iter()
                .find(|(name, _)| name == "authorization")
        })
        .map(|(_, value)| value.clone())
        .collect();

    assert_eq!(
        authorizations,
        vec![
            "Bearer first-installation-token",
            "Bearer second-installation-token"
        ],
        "a token nearing expiry was not replaced before its next use"
    );
}

#[tokio::test]
async fn an_integration_names_a_repository_as_owner_and_name() {
    let stub = GithubStub::start();
    let kestrel = Kestrel::boot().await;
    kestrel.declare_organization("acme").await;

    let refusal = kestrel
        .try_register_integration(
            "acme",
            "github",
            "kestrel",
            &stub.base_url(),
            BOTH,
            eagerly(),
        )
        .await
        .expect_err("a repository that is not owner/name should be refused");

    assert!(
        refusal.to_string().contains("owner/name"),
        "unhelpful refusal: {refusal}"
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn an_integration_and_what_it_discovers_belong_to_one_organization() {
    let stub = GithubStub::start();
    stub.deliver(labelled_ready(43));
    let kestrel = Kestrel::boot().await;
    watching(&kestrel, &stub, BOTH).await;
    kestrel.declare_organization("globex").await;

    recorded(&kestrel, 1).await;

    assert!(
        kestrel.integrations("globex").await.is_empty(),
        "another organization can see the integration acme registered"
    );
    assert!(
        kestrel.events("globex").await.is_empty(),
        "another organization can see the events acme's integration discovered"
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn two_organizations_keep_separate_copies_of_the_same_producer_event() {
    let acme = GithubStub::start();
    let globex = GithubStub::start();
    acme.deliver(labelled_ready(43));
    globex.deliver(labelled_ready(43));
    let kestrel = Kestrel::boot().await;
    kestrel.declare_organization("acme").await;
    kestrel.declare_organization("globex").await;
    kestrel
        .register_integration(
            "acme",
            "github",
            REPOSITORY,
            &acme.base_url(),
            BOTH,
            eagerly(),
        )
        .await;
    kestrel
        .register_integration(
            "globex",
            "github",
            REPOSITORY,
            &globex.base_url(),
            BOTH,
            eagerly(),
        )
        .await;

    let deadline = tokio::time::Instant::now() + PATIENCE;
    loop {
        let acme_events = kestrel.events("acme").await;
        let globex_events = kestrel.events("globex").await;
        if acme_events.len() == 1 && globex_events.len() == 1 {
            assert_eq!(acme_events[0].occurrence.id, globex_events[0].occurrence.id);
            assert_ne!(acme_events[0].record_id, globex_events[0].record_id);
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "both organizations did not retain their own Event"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    kestrel.teardown().await;
}
