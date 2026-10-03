use crate::scheduling::{Occupied, dispatch};
use tempfile::TempDir;

use super::*;
use crate::domain::SessionState;
use crate::log::Window;
use crate::workspace;

const INSTANCE: &str = "local-exec/kestrel-fixture";

fn harness() -> link::Harness {
    link::Harness {
        command: "opencode acp".to_owned(),
        auth: None,
        model: None,
        mode: None,
        thought_level: None,
    }
}

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
            .declare(&organization, "builder", "opencode", &Declared::default())
            .await
            .unwrap();
        tx.commit().await.unwrap();
        let workspace = workspace::open_without_a_session(
            &store, "acme", "kestrel", "builder", None, None, None,
        )
        .await
        .unwrap();
        let queued = enqueue(&store, workspace.id, None, Declared::default())
            .await
            .unwrap();
        let session = {
            let mut tx = store.begin().await.unwrap();
            let session = tx
                .workspaces()
                .claim_session(&queued, Timestamp::now() + LEASE, false)
                .await
                .unwrap()
                .expect("the fixture's session should claim");
            tx.commit().await.unwrap();
            session
        };
        executes_on(&store, &session, INSTANCE).await.unwrap();

        Self {
            store,
            session,
            data_dir,
        }
    }

    async fn report(&self, seq: Option<i64>, reported: Report) -> Result<(), ReportRefused> {
        report(
            &self.store,
            INSTANCE,
            Reported {
                session: Some(self.session.id),
                seq,
                report: reported,
            },
        )
        .await
    }

    /// What the Session's own report does, as it would once the Instance had passed it through.
    async fn report_on(&self, seq: Option<i64>, reported: Report) -> Result<(), ReportRefused> {
        report_on(&self.store, &self.session, seq, reported).await
    }

    async fn entries(&self) -> Vec<Entry> {
        workspace::transcript(
            &self.store,
            self.session.workspace,
            None,
            Window::DEFAULT,
            &Default::default(),
        )
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
        .declare(&organization, "builder", "codex", &Declared::default())
        .await
        .unwrap();
    tx.profiles()
        .declare(&organization, "jack", "Jack")
        .await
        .unwrap();
    tx.commit().await.unwrap();

    let first = workspace::open_without_a_session(
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
    let second = workspace::open_without_a_session(
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
    let first_queued = workspace::post(&store, first.id, "operator", "start please")
        .await
        .unwrap()
        .session
        .expect("a fresh workspace's first message starts a session");
    let first_session = match dispatch(&store, 1, &["codex".to_owned()]).await.unwrap() {
        Some(Occupied::Claimed(claimed)) => claimed,
        _ => panic!("the first session should claim"),
    };
    assert_eq!(first_session.id, first_queued.id);
    executes_on(
        &store,
        &first_session,
        &format!("local-exec/{}", first_session.id),
    )
    .await
    .unwrap();
    link::start(&store, &first_session, harness())
        .await
        .unwrap();
    report(
        &store,
        &format!("local-exec/{}", first_session.id),
        Reported {
            session: Some(first_session.id),
            seq: Some(1),
            report: Report::Answered { usage: None },
        },
    )
    .await
    .unwrap();
    report(
        &store,
        &format!("local-exec/{}", first_session.id),
        Reported {
            session: Some(first_session.id),
            seq: Some(2),
            report: Report::Settled,
        },
    )
    .await
    .unwrap();
    assert_eq!(
        session(&store, first_session.id).await.unwrap().state,
        SessionState::Waiting
    );

    let second_queued = workspace::post(&store, second.id, "operator", "start please")
        .await
        .unwrap()
        .session
        .expect("a fresh workspace's first message starts a session");
    let second_session = match dispatch(&store, 1, &["codex".to_owned()]).await.unwrap() {
        Some(Occupied::Claimed(claimed)) => claimed,
        _ => panic!("the waiting session should leave its slot and profile available"),
    };
    assert_eq!(second_session.id, second_queued.id);

    workspace::post(&store, first.id, "operator", "continue")
        .await
        .unwrap();
    assert!(
        dispatch(&store, 2, &["codex".to_owned()])
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
    let alex = workspace::open_without_a_session(
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
    let alex_queued = workspace::post(&store, alex.id, "operator", "start please")
        .await
        .unwrap()
        .session
        .expect("a fresh workspace's first message starts a session");
    let alex_session = match dispatch(&store, 2, &["codex".to_owned()]).await.unwrap() {
        Some(Occupied::Claimed(claimed)) => claimed,
        _ => panic!("another profile should be able to claim while Jack is busy"),
    };
    assert_eq!(alex_session.id, alex_queued.id);
    executes_on(
        &store,
        &alex_session,
        &format!("local-exec/{}", alex_session.id),
    )
    .await
    .unwrap();
    link::start(&store, &alex_session, harness()).await.unwrap();
    report(
        &store,
        &format!("local-exec/{}", alex_session.id),
        Reported {
            session: Some(alex_session.id),
            seq: Some(1),
            report: Report::Answered { usage: None },
        },
    )
    .await
    .unwrap();
    report(
        &store,
        &format!("local-exec/{}", alex_session.id),
        Reported {
            session: Some(alex_session.id),
            seq: Some(2),
            report: Report::Settled,
        },
    )
    .await
    .unwrap();
    workspace::post(&store, alex.id, "operator", "continue")
        .await
        .unwrap();
    match dispatch(&store, 2, &["codex".to_owned()]).await.unwrap() {
        Some(Occupied::Resumed(session)) => assert_eq!(session.id, alex_session.id),
        _ => panic!("an eligible held prompt should pass the blocked one"),
    }

    complete(&store, &second_session).await.unwrap();
    match dispatch(&store, 2, &["codex".to_owned()]).await.unwrap() {
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
                completion: crate::log::Completion::at("2026-09-29T12:00:00Z".parse().unwrap()),
            },
        )
        .await
        .unwrap();
    fixture
        .report(None, Report::Usage { usage: usage() })
        .await
        .unwrap();
    fixture
        .report(
            Some(4),
            Report::Finished {
                exit: Exit::Succeeded,
                usage: Some(usage()),
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
                message: "done".to_owned(),
                session_id: Some(fixture.session.id),
                completion: Some(crate::log::Completion::at(
                    "2026-09-29T12:00:00Z".parse().unwrap()
                ))
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
        completion: crate::log::Completion::at("2026-09-29T12:00:00Z".parse().unwrap()),
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
                completion: crate::log::Completion::at("2026-09-29T12:00:00Z".parse().unwrap()),
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
                message: "said once".to_owned(),
                session_id: Some(fixture.session.id),
                completion: Some(crate::log::Completion::at(
                    "2026-09-29T12:00:00Z".parse().unwrap()
                ))
            },
            Entry::Said {
                participant: "builder".to_owned(),
                message: "next".to_owned(),
                session_id: Some(fixture.session.id),
                completion: Some(crate::log::Completion::at(
                    "2026-09-29T12:00:00Z".parse().unwrap()
                ))
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
            completion: crate::log::Completion::at("2026-09-29T12:00:00Z".parse().unwrap()),
        },
        Report::Answered { usage: None },
        Report::Finished {
            exit: Exit::Succeeded,
            usage: None,
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
        fixture
            .report(
                seq,
                Report::Connected {
                    version: "test-version".to_owned(),
                },
            )
            .await
            .unwrap();
        let before = session(&fixture.store, fixture.session.id)
            .await
            .unwrap()
            .lease_expires_at
            .unwrap();
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
async fn a_heartbeat_never_revives_a_lease_that_has_passed_and_its_session_is_gone() {
    let fixture = Fixture::new().await;
    let lapsed = Timestamp::now() - SignedDuration::from_secs(1);
    let mut tx = fixture.store.begin().await.unwrap();
    tx.workspaces()
        .hold_lease(&fixture.session, lapsed)
        .await
        .unwrap();
    tx.commit().await.unwrap();

    fixture.report(None, Report::Heartbeat).await.unwrap();

    let recorded = session(&fixture.store, fixture.session.id).await.unwrap();
    assert!(recorded.lease_expires_at.unwrap() <= Timestamp::now());
    assert!(matches!(
        fixture.report(Some(1), Report::Started).await,
        Err(ReportRefused::Gone(gone)) if gone == fixture.session.id
    ));
}

#[tokio::test]
async fn a_supervisor_that_exits_after_its_sessions_lease_lapsed_fails_it_for_its_lease() {
    let fixture = Fixture::new().await;
    let mut tx = fixture.store.begin().await.unwrap();
    tx.workspaces()
        .hold_lease(
            &fixture.session,
            Timestamp::now() - SignedDuration::from_secs(1),
        )
        .await
        .unwrap();
    tx.commit().await.unwrap();

    supervisor_exited(&fixture.store, INSTANCE, "the supervisor exited unreported")
        .await
        .unwrap();

    let recorded = session(&fixture.store, fixture.session.id).await.unwrap();
    assert_eq!(recorded.exit, Some(expired_lease()));
}

#[tokio::test]
async fn a_supervisor_that_exits_while_its_sessions_lease_holds_fails_it_for_the_exit() {
    let fixture = Fixture::new().await;

    supervisor_exited(&fixture.store, INSTANCE, "the supervisor exited unreported")
        .await
        .unwrap();

    let recorded = session(&fixture.store, fixture.session.id).await.unwrap();
    assert_eq!(
        recorded.exit,
        Some(Exit::Failed {
            because: "the supervisor exited unreported".to_owned()
        })
    );
}

#[tokio::test]
async fn a_report_about_a_session_that_has_ended_is_gone() {
    let fixture = Fixture::new().await;
    complete(&fixture.store, &fixture.session).await.unwrap();

    assert!(matches!(
        fixture.report(Some(1), Report::Started).await,
        Err(ReportRefused::Gone(_))
    ));
    assert!(matches!(
        fixture.report(None, Report::Started).await,
        Err(ReportRefused::Gone(_))
    ));
}

#[tokio::test]
async fn a_report_about_no_session_is_refused() {
    let fixture = Fixture::new().await;

    let refused = report(
        &fixture.store,
        INSTANCE,
        Reported {
            session: None,
            seq: Some(1),
            report: Report::Started,
        },
    )
    .await;

    assert!(matches!(refused, Err(ReportRefused::MissingSession)));
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
    crate::instance::release(&fixture.store, fixture.session.workspace, "operator")
        .await
        .unwrap();
    workspace::seal(&fixture.store, fixture.session.workspace)
        .await
        .unwrap();
    let before = fixture.entries().await;

    let refused = fixture
        .report_on(Some(1), Report::Started)
        .await
        .unwrap_err();
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
        .report_on(
            Some(1),
            Report::Finished {
                exit: Exit::Succeeded,
                usage: Some(usage()),
            },
        )
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
        .report_on(
            Some(1),
            Report::Finished {
                exit: Exit::Succeeded,
                usage: None,
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
