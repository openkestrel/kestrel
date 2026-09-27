//! Three chatty Sessions at once, beside an enabled Trigger and the sweeps, wait for SQLite's
//! write lock well inside the busy timeout. It runs for tens of seconds and prints the
//! distribution it measured, so it is ignored by default:
//!
//! ```sh
//! cargo test --package kestrel --test contention -- --ignored --nocapture
//! ```

mod support;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use jiff::Timestamp;
use kestrel::domain::{Integration, Organization, SessionId, Workspace};
use kestrel::log::Entry;
use kestrel::store::WRITE_LOCK;
use kestrel_scripted_agent::{CHATTERED_MESSAGES, chattered};
use support::scripted_agent::{self, Script};
use support::{HARNESS, Kestrel, repository, supervisor};
use tokio_util::sync::CancellationToken;
use tracing::field::{Field, Visit};
use tracing_subscriber::Layer;
use tracing_subscriber::filter::Targets;
use tracing_subscriber::layer::{Context, SubscriberExt as _};
use tracing_subscriber::util::SubscriberInitExt as _;

const SESSIONS: usize = 3;
const TURNS: usize = 8;
const EVENTS_BEFORE: i64 = 50_000;
const EVENT_EVERY: Duration = Duration::from_millis(200);
const OPERATOR_POLL: Duration = Duration::from_millis(200);
const PATIENCE: Duration = Duration::from_secs(120);
const SECRET: &str = "a-shared-secret";
const REPOSITORY: &str = "jtmthf/kestrel";
const P99_UNDER: Duration = Duration::from_secs(1);

#[derive(Clone, Default)]
struct Waits(Arc<Mutex<Vec<Duration>>>);

impl Waits {
    fn drained(&self) -> Vec<Duration> {
        std::mem::take(&mut *self.0.lock().expect("the waits should not be poisoned"))
    }
}

impl<S: tracing::Subscriber> Layer<S> for Waits {
    fn on_event(&self, event: &tracing::Event<'_>, _: Context<'_, S>) {
        let mut waited = Waited(None);
        event.record(&mut waited);
        if let Some(waited) = waited.0 {
            self.0
                .lock()
                .expect("the waits should not be poisoned")
                .push(waited);
        }
    }
}

struct Waited(Option<Duration>);

impl Visit for Waited {
    fn record_u64(&mut self, field: &Field, value: u64) {
        if field.name() == "waited_us" {
            self.0 = Some(Duration::from_micros(value));
        }
    }

    fn record_debug(&mut self, _: &Field, _: &dyn std::fmt::Debug) {}
}

fn percentile(sorted: &[Duration], q: f64) -> Duration {
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss,
        reason = "a rank within a vector's length"
    )]
    let rank = ((q * sorted.len() as f64).ceil() as usize).clamp(1, sorted.len());
    sorted[rank - 1]
}

/// A label the Trigger does not fire for, so every one of them is looked at again on each sweep.
fn labelled(n: i64) -> serde_json::Value {
    serde_json::json!({
        "action": "labeled",
        "label": {"name": "needs-triage"},
        "issue": {"number": n, "title": "Fix the thing", "body": "It is broken. ".repeat(40)},
        "repository": {"full_name": REPOSITORY},
        "sender": {"login": "jtmthf"}
    })
}

/// Written straight to the database, because recording this many through the webhook is a load
/// scenario of its own.
async fn history(kestrel: &Kestrel, organization: &Organization, webhook: &Integration) {
    let pool = support::database(kestrel.data_dir()).await;
    sqlx::query(
        "WITH RECURSIVE n (n) AS (SELECT 0 UNION ALL SELECT n + 1 FROM n WHERE n + 1 < ?)
         INSERT INTO event (record_id, organization_id, integration_id, id, source, specversion,
                            type, time, data, recorded_at)
         SELECT printf('history-%d', n), ?, ?, printf('history-%d', n), ?, '1.0',
                'com.github.issues.labeled', ?, ?, ?
           FROM n",
    )
    .bind(EVENTS_BEFORE)
    .bind(organization.id.to_string())
    .bind(webhook.id.to_string())
    .bind(format!("https://github.com/{REPOSITORY}"))
    .bind(Timestamp::now().to_string())
    .bind(labelled(0).to_string())
    .bind(Timestamp::now().to_string())
    .execute(&pool)
    .await
    .expect("the history should record");
    pool.close().await;
}

async fn record_event(client: &reqwest::Client, url: &str, n: i64) {
    let answered = client
        .post(url)
        .bearer_auth(SECRET)
        .header("content-type", "application/cloudevents+json")
        .body(
            serde_json::json!({
                "specversion": "1.0",
                "id": format!("labeled-{n}"),
                "source": format!("https://github.com/{REPOSITORY}"),
                "type": "com.github.issues.labeled",
                "data": labelled(n)
            })
            .to_string(),
        )
        .send()
        .await
        .expect("the webhook answers");
    assert!(
        answered.status().is_success(),
        "the webhook refused an event: {}",
        answered.status()
    );
}

async fn answered(kestrel: &Kestrel, session: SessionId, turns: usize) {
    let deadline = tokio::time::Instant::now() + PATIENCE;
    loop {
        let answered = kestrel
            .turns(session)
            .await
            .iter()
            .filter(|turn| turn.answered_at.is_some())
            .count();
        if answered >= turns {
            return;
        }
        let session = kestrel.session(session).await;
        assert!(
            session.exit.is_none() && tokio::time::Instant::now() < deadline,
            "the session {} is {} with {answered} of {turns} turns answered and the exit {:?}",
            session.id,
            session.state,
            session.exit
        );
        tokio::time::sleep(OPERATOR_POLL).await;
    }
}

async fn chatting(kestrel: &Kestrel, workspace: &Workspace) -> SessionId {
    let session = kestrel.post(workspace.id, "operator", "turn 1").await.id;
    for turn in 1..=TURNS {
        answered(kestrel, session, turn).await;
        if turn < TURNS {
            kestrel
                .post_while_busy(workspace.id, "operator", &format!("turn {}", turn + 1))
                .await;
        }
    }

    session
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "a load scenario that measures lock waits over tens of seconds"]
async fn three_chatty_sessions_wait_for_the_write_lock_well_inside_the_busy_timeout() {
    let waits = Waits::default();
    tracing_subscriber::registry()
        .with(
            waits
                .clone()
                .with_filter(Targets::new().with_target(WRITE_LOCK, tracing::Level::TRACE)),
        )
        .init();

    let kestrel = Kestrel::dispatching_up_to(
        supervisor::binary(),
        &scripted_agent::playing(Script::Chatters),
        SESSIONS,
    )
    .await;
    let organization = kestrel.declare_organization("acme").await;
    kestrel
        .declare_project(
            &organization,
            repository::NAME,
            &[repository::url().to_owned()],
            repository::BRANCH,
        )
        .await;
    kestrel
        .declare_agent(&organization, "builder", HARNESS, None)
        .await;
    kestrel
        .hold_provider_credential(
            &organization,
            support::PROVIDER_KEY,
            support::A_PROVIDER_KEY,
        )
        .await;
    let webhook = kestrel.register_webhook("acme", "ci", SECRET).await;
    kestrel
        .declare_trigger(
            "acme",
            "ready",
            &support::labelled_on(REPOSITORY, "ready-for-agent"),
            repository::NAME,
            "builder",
        )
        .await;
    history(&kestrel, &organization, &webhook).await;
    let client = reqwest::Client::new();
    let webhook = format!("{}{}", kestrel.link(), webhook.webhook_path());
    let mut workspaces = Vec::with_capacity(SESSIONS);
    for _ in 0..SESSIONS {
        workspaces.push(
            kestrel
                .open_workspace("acme", repository::NAME, "builder")
                .await,
        );
    }
    waits.drained();

    let done = CancellationToken::new();
    let sessions = {
        let recording = async {
            let mut n = EVENTS_BEFORE;
            while !done.is_cancelled() {
                record_event(&client, &webhook, n).await;
                n += 1;
                tokio::time::sleep(EVENT_EVERY).await;
            }
        };
        let chatting = async {
            let [first, second, third] = workspaces.as_slice() else {
                unreachable!("three workspaces were opened");
            };
            let sessions = tokio::join!(
                chatting(&kestrel, first),
                chatting(&kestrel, second),
                chatting(&kestrel, third)
            );
            done.cancel();
            <[SessionId; SESSIONS]>::from(sessions)
        };
        tokio::join!(recording, chatting).1
    };

    let mut waited = waits.drained();
    waited.sort_unstable();
    let p99 = percentile(&waited, 0.99);
    println!(
        "write-lock waits over {} transactions: p50 {:?}, p90 {:?}, p99 {:?}, max {:?}",
        waited.len(),
        percentile(&waited, 0.5),
        percentile(&waited, 0.9),
        p99,
        waited.last().expect("at least one transaction"),
    );

    for workspace in &workspaces {
        let said: Vec<String> = kestrel
            .transcript(workspace.id)
            .await
            .into_iter()
            .filter_map(|recorded| match recorded.entry {
                Entry::Said {
                    participant,
                    message,
                } if participant == "builder" => Some(message),
                _ => None,
            })
            .collect();
        let expected: Vec<String> = (1..=TURNS)
            .flat_map(|turn| (1..=CHATTERED_MESSAGES).map(move |m| chattered(turn, m)))
            .collect();
        assert_eq!(said, expected, "the workspace {}", workspace.id);
    }
    for session in sessions {
        kestrel.stop_session(session).await;
    }
    kestrel.teardown().await;

    assert!(
        p99 < P99_UNDER,
        "the p99 write-lock wait was {p99:?}, and should be under {P99_UNDER:?}"
    );
}
