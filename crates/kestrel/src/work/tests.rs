use tempfile::TempDir;

use super::*;
use crate::domain::SessionState;
use crate::log::Window;
use crate::workspace;

struct Fixture {
    store: Store,
    session: Session,
    data_dir: TempDir,
}

impl Fixture {
    async fn new() -> Self {
        let data_dir = TempDir::new().unwrap();
        let store = Store::open(data_dir.path()).await.unwrap();
        let mut tx = store.begin().await.unwrap();
        let organization = tx
            .organizations()
            .declare("acme", None)
            .await
            .unwrap()
            .record;
        tx.projects()
            .declare(&organization, "kestrel", &[], "main")
            .await
            .unwrap();
        tx.agents()
            .declare(&organization, "builder", "opencode", None)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        let workspace = workspace::open(&store, "acme", "kestrel", "builder", None, None, None)
            .await
            .unwrap();
        enqueue(&store, workspace.id, None, None).await.unwrap();
        let session = claim(&store, &[]).await.unwrap().unwrap().session;

        Self {
            store,
            session,
            data_dir,
        }
    }

    async fn report(&self, seq: Option<i64>, reported: Report) -> Result<(), ReportRefused> {
        report(
            &self.store,
            &self.session,
            Reported {
                seq,
                report: reported,
            },
        )
        .await
    }

    async fn entries(&self) -> Vec<Entry> {
        workspace::transcript(&self.store, self.session.workspace, None, Window::DEFAULT)
            .await
            .unwrap()
            .entries
            .into_iter()
            .map(|entry| entry.entry)
            .collect()
    }
}

fn usage() -> Usage {
    Usage {
        context_used: 1200,
        context_size: 200_000,
        cost: None,
    }
}

#[tokio::test]
async fn a_waiting_codex_session_yields_its_profile_and_resumes_when_free() {
    let data_dir = TempDir::new().unwrap();
    let store = Store::open(data_dir.path()).await.unwrap();
    let mut tx = store.begin().await.unwrap();
    let organization = tx
        .organizations()
        .declare("acme", None)
        .await
        .unwrap()
        .record;
    tx.projects()
        .declare(&organization, "kestrel", &[], "main")
        .await
        .unwrap();
    tx.agents()
        .declare(&organization, "builder", "codex", None)
        .await
        .unwrap();
    tx.profiles()
        .declare(&organization, "jack", "Jack")
        .await
        .unwrap();
    tx.commit().await.unwrap();

    let first = workspace::open(
        &store,
        "acme",
        "kestrel",
        "builder",
        Some("jack"),
        None,
        None,
    )
    .await
    .unwrap();
    let second = workspace::open(
        &store,
        "acme",
        "kestrel",
        "builder",
        Some("jack"),
        None,
        None,
    )
    .await
    .unwrap();
    let first_queued = enqueue(&store, first.id, None, None).await.unwrap();
    let first_session = match occupy(&store, 1, &["codex".to_owned()]).await.unwrap() {
        Some(Occupied::Claimed(claimed)) => claimed.session,
        _ => panic!("the first session should claim"),
    };
    assert_eq!(first_session.id, first_queued.id);
    link::start(&store, &first_session).await.unwrap();
    report(
        &store,
        &first_session,
        Reported {
            seq: Some(1),
            report: Report::Answered,
        },
    )
    .await
    .unwrap();
    assert_eq!(
        session(&store, first_session.id).await.unwrap().state,
        SessionState::Waiting
    );

    let second_queued = enqueue(&store, second.id, None, None).await.unwrap();
    let second_session = match occupy(&store, 1, &["codex".to_owned()]).await.unwrap() {
        Some(Occupied::Claimed(claimed)) => claimed.session,
        _ => panic!("the waiting session should leave its slot and profile available"),
    };
    assert_eq!(second_session.id, second_queued.id);

    workspace::post(&store, first.id, "operator", "continue")
        .await
        .unwrap();
    assert!(
        occupy(&store, 2, &["codex".to_owned()])
            .await
            .unwrap()
            .is_none()
    );

    let mut tx = store.begin().await.unwrap();
    tx.profiles()
        .declare(&organization, "alex", "Alex")
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let alex = workspace::open(
        &store,
        "acme",
        "kestrel",
        "builder",
        Some("alex"),
        None,
        None,
    )
    .await
    .unwrap();
    let alex_queued = enqueue(&store, alex.id, None, None).await.unwrap();
    let alex_session = match occupy(&store, 2, &["codex".to_owned()]).await.unwrap() {
        Some(Occupied::Claimed(claimed)) => claimed.session,
        _ => panic!("another profile should be able to claim while Jack is busy"),
    };
    assert_eq!(alex_session.id, alex_queued.id);
    link::start(&store, &alex_session).await.unwrap();
    report(
        &store,
        &alex_session,
        Reported {
            seq: Some(1),
            report: Report::Answered,
        },
    )
    .await
    .unwrap();
    workspace::post(&store, alex.id, "operator", "continue")
        .await
        .unwrap();
    match occupy(&store, 2, &["codex".to_owned()]).await.unwrap() {
        Some(Occupied::Resumed(session)) => assert_eq!(session.id, alex_session.id),
        _ => panic!("an eligible held prompt should pass the blocked one"),
    }

    complete(&store, &second_session).await.unwrap();
    match occupy(&store, 2, &["codex".to_owned()]).await.unwrap() {
        Some(Occupied::Resumed(session)) => assert_eq!(session.id, first_session.id),
        _ => panic!("the held prompt should resume after the profile is free"),
    }
    assert_eq!(
        store
            .begin()
            .await
            .unwrap()
            .workspaces()
            .turns(first_session.id)
            .await
            .unwrap()
            .len(),
        2
    );
}

#[tokio::test]
async fn reports_record_the_session_and_its_transcript_together() {
    let fixture = Fixture::new().await;
    fixture.report(Some(1), Report::Started).await.unwrap();
    fixture
        .report(
            Some(2),
            Report::Model {
                model: "scripted-mini".to_owned(),
            },
        )
        .await
        .unwrap();
    fixture
        .report(
            Some(3),
            Report::Said {
                message: "done".to_owned(),
            },
        )
        .await
        .unwrap();
    fixture
        .report(Some(4), Report::Used { usage: usage() })
        .await
        .unwrap();
    fixture
        .report(
            Some(5),
            Report::Finished {
                exit: Exit::Succeeded,
            },
        )
        .await
        .unwrap();

    let recorded = session(&fixture.store, fixture.session.id).await.unwrap();
    assert_eq!(recorded.state, SessionState::Ended);
    assert_eq!(recorded.exit, Some(Exit::Succeeded));
    assert!(recorded.started_at.is_some());
    assert!(recorded.ended_at.is_some());
    assert_eq!(recorded.worked_model.as_deref(), Some("scripted-mini"));
    assert_eq!(recorded.usage, Some(usage()));
    assert_eq!(
        fixture.entries().await,
        vec![
            Entry::ParticipantJoined {
                participant: "builder".to_owned()
            },
            Entry::SessionStarted {
                session: fixture.session.id,
                agent: "builder".to_owned()
            },
            Entry::Said {
                participant: "builder".to_owned(),
                message: "done".to_owned()
            },
            Entry::SessionEnded {
                session: fixture.session.id,
                exit: Exit::Succeeded
            },
        ]
    );
}

#[tokio::test]
async fn concurrent_reports_and_a_replay_after_reopening_the_store_append_once() {
    let mut fixture = Fixture::new().await;
    let said = Report::Said {
        message: "said once".to_owned(),
    };
    let (first, second) = tokio::join!(
        fixture.report(Some(1), said.clone()),
        fixture.report(Some(1), said.clone()),
    );
    first.unwrap();
    second.unwrap();
    fixture.store = Store::open(fixture.data_dir.path()).await.unwrap();
    fixture.report(Some(1), said).await.unwrap();
    fixture
        .report(
            Some(2),
            Report::Said {
                message: "next".to_owned(),
            },
        )
        .await
        .unwrap();

    assert_eq!(
        fixture.entries().await,
        vec![
            Entry::ParticipantJoined {
                participant: "builder".to_owned()
            },
            Entry::Said {
                participant: "builder".to_owned(),
                message: "said once".to_owned()
            },
            Entry::Said {
                participant: "builder".to_owned(),
                message: "next".to_owned()
            },
        ]
    );
}

#[tokio::test]
async fn numbered_reports_refuse_missing_and_invalid_numbers_without_effects() {
    let fixture = Fixture::new().await;
    let before = fixture.entries().await;
    for reported in [
        Report::Started,
        Report::Model {
            model: "unexpected".to_owned(),
        },
        Report::Said {
            message: "refused".to_owned(),
        },
        Report::Used { usage: usage() },
        Report::Finished {
            exit: Exit::Succeeded,
        },
    ] {
        assert!(matches!(
            fixture.report(None, reported.clone()).await,
            Err(ReportRefused::MissingSequence)
        ));
        for seq in [-1, 0, 2] {
            assert!(
                matches!(fixture.report(Some(seq), reported.clone()).await, Err(ReportRefused::SkippedSequence(number)) if number == seq)
            );
        }
    }

    let recorded = session(&fixture.store, fixture.session.id).await.unwrap();
    assert_eq!(recorded.state, SessionState::Working);
    assert!(recorded.started_at.is_none());
    assert!(recorded.worked_model.is_none());
    assert!(recorded.usage.is_none());
    assert_eq!(fixture.entries().await, before);
    fixture.report(Some(1), Report::Started).await.unwrap();
    assert!(
        session(&fixture.store, fixture.session.id)
            .await
            .unwrap()
            .started_at
            .is_some()
    );
}

#[tokio::test]
async fn connection_and_heartbeat_reports_ignore_numbers_and_do_not_consume_them() {
    let fixture = Fixture::new().await;
    for seq in [None, Some(99)] {
        let mut tx = fixture.store.begin().await.unwrap();
        tx.workspaces()
            .hold_lease(
                &fixture.session,
                Timestamp::now() - SignedDuration::from_secs(1),
            )
            .await
            .unwrap();
        tx.commit().await.unwrap();
        fixture
            .report(
                seq,
                Report::Connected {
                    version: "test-version".to_owned(),
                },
            )
            .await
            .unwrap();
        let before = Timestamp::now();
        fixture.report(seq, Report::Heartbeat).await.unwrap();
        let recorded = session(&fixture.store, fixture.session.id).await.unwrap();
        assert_eq!(recorded.connected.unwrap().version, "test-version");
        assert!(recorded.lease_expires_at.unwrap() > before);
    }
    assert_eq!(fixture.entries().await.len(), 1);
    fixture.report(Some(1), Report::Started).await.unwrap();
    assert_eq!(fixture.entries().await.len(), 2);
}

#[tokio::test]
async fn what_a_harness_writes_to_stderr_never_enters_the_transcript_or_takes_a_number() {
    let fixture = Fixture::new().await;
    let before = fixture.entries().await;

    fixture
        .report(
            None,
            Report::Stderr {
                lines: vec!["level=INFO message=init".to_owned()],
            },
        )
        .await
        .unwrap();

    assert_eq!(fixture.entries().await, before);
    fixture.report(Some(1), Report::Started).await.unwrap();
    assert_eq!(fixture.entries().await.len(), before.len() + 1);
}

#[tokio::test]
async fn what_a_harness_writes_to_stderr_never_waits_for_the_write_lock() {
    let fixture = Fixture::new().await;
    let holding = fixture.store.begin().await.unwrap();

    tokio::time::timeout(
        std::time::Duration::from_secs(1),
        fixture.report(
            None,
            Report::Stderr {
                lines: vec!["git ran".to_owned()],
            },
        ),
    )
    .await
    .expect("a stderr report waited for the write lock")
    .unwrap();

    drop(holding);
}

#[tokio::test]
async fn a_failed_append_rolls_back_the_session_change_and_report_acceptance() {
    let fixture = Fixture::new().await;
    complete(&fixture.store, &fixture.session).await.unwrap();
    workspace::seal(&fixture.store, fixture.session.workspace)
        .await
        .unwrap();
    let before = fixture.entries().await;

    let refused = fixture.report(Some(1), Report::Started).await.unwrap_err();
    assert!(matches!(refused, ReportRefused::Unavailable(_)));
    assert!(refused.to_string().contains("sealed"));
    assert!(
        session(&fixture.store, fixture.session.id)
            .await
            .unwrap()
            .started_at
            .is_none()
    );
    assert_eq!(fixture.entries().await, before);

    fixture
        .report(Some(1), Report::Used { usage: usage() })
        .await
        .unwrap();
    assert_eq!(
        session(&fixture.store, fixture.session.id)
            .await
            .unwrap()
            .usage,
        Some(usage())
    );
}

#[tokio::test]
async fn a_finished_report_keeps_the_exit_that_already_stands() {
    let fixture = Fixture::new().await;
    let failed = fail(&fixture.store, &fixture.session, "lease expired")
        .await
        .unwrap();
    let before = fixture.entries().await;

    fixture
        .report(
            Some(1),
            Report::Finished {
                exit: Exit::Succeeded,
            },
        )
        .await
        .unwrap();

    assert_eq!(
        session(&fixture.store, fixture.session.id)
            .await
            .unwrap()
            .exit,
        Some(failed)
    );
    assert_eq!(fixture.entries().await, before);
}
