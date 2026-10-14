use jiff::Timestamp;
use kestrel::domain::{EventRecordId, Session, SessionId, SessionOption, Workspace, WorkspaceId};
use kestrel::instance;
use kestrel::link::{self, Instruction};
use kestrel::work;

use super::super::{Kestrel, harness};

impl Kestrel {
    pub async fn report_checkout(&self, session: &Session, repositories: Vec<instance::Observed>) {
        work::report_on(
            &self.store,
            session,
            Some(1),
            work::Report::Checkout { repositories },
        )
        .await
        .expect("the checkout should be reported");
    }

    pub async fn report_ready(&self, session: &Session) {
        work::report_on(&self.store, session, None, work::Report::Ready)
            .await
            .expect("the ready report should be taken");
    }

    pub async fn start_on_the_link(&self, session: &Session) {
        link::start(&self.store, session, harness())
            .await
            .expect("the session should start");
    }

    pub async fn report_session_info(&self, session: &Session, options: &[SessionOption]) {
        work::report_on(
            &self.store,
            session,
            None,
            work::Report::SessionInfo {
                title: None,
                options: options.to_vec(),
                commands: Vec::new(),
            },
        )
        .await
        .expect("the session info should be taken");
    }

    pub async fn report_option_changed(&self, session: &Session, seq: i64, changed: work::Report) {
        work::report_on(&self.store, session, Some(seq), changed)
            .await
            .expect("the option change should be taken");
    }

    pub async fn report_activity(&self, session: &Session, seq: i64, message: &str) {
        work::report_on(
            &self.store,
            session,
            Some(seq),
            work::Report::Said {
                message: message.to_owned(),
                completion: kestrel::log::Completion::at(jiff::Timestamp::now()),
            },
        )
        .await
        .expect("the activity should be reported");
    }

    pub async fn report_answered(&self, session: &Session, seq: i64) {
        work::report_on(
            &self.store,
            session,
            Some(seq),
            work::Report::Answered { usage: None },
        )
        .await
        .expect("the answer should be taken");
    }

    pub async fn report_settled(&self, session: &Session, seq: i64) {
        work::report_on(
            &self.store,
            session,
            Some(seq),
            work::Report::Settled { usage: None },
        )
        .await
        .expect("the settling should be taken");
    }

    pub async fn instances_to_archive(&self) -> Vec<String> {
        instance::to_archive(&self.store)
            .await
            .expect("the instances to archive should read")
    }

    pub async fn instance_archived(&self, instance: &str) {
        instance::archived(&self.store, instance)
            .await
            .expect("the instance should be recorded archived");
    }

    pub async fn instance(&self, workspace: WorkspaceId) -> Option<String> {
        work::instance(&self.store, workspace)
            .await
            .expect("the workspace's instance should read")
    }

    pub async fn interrupt(
        &self,
        session: SessionId,
        participant: &str,
    ) -> anyhow::Result<Session> {
        work::interrupt(&self.store, session, Some(participant)).await
    }

    pub async fn report_interrupted(&self, session: &Session, trailing: bool) {
        work::report_on(
            &self.store,
            session,
            Some(1),
            work::Report::Interrupted { trailing },
        )
        .await
        .expect("the interruption should be reported");
    }

    pub async fn instruct(&self, session: &Session, instruction: Instruction) {
        self.try_instruct(session, instruction)
            .await
            .expect("the instruction should send");
    }

    pub async fn try_instruct(
        &self,
        session: &Session,
        instruction: Instruction,
    ) -> anyhow::Result<link::SentInstruction> {
        link::instruct(&self.store, session, instruction).await
    }

    pub async fn start(&self, session: &Session, harness: link::Harness) {
        self.try_start(session, harness)
            .await
            .expect("the session should start");
    }

    pub async fn try_start(
        &self,
        session: &Session,
        harness: link::Harness,
    ) -> anyhow::Result<link::SentInstruction> {
        link::start(&self.store, session, harness).await
    }

    /// A lease that is up when the caller says rather than when a real one would be. The only
    /// way to watch a sweep without waiting a whole lease out.
    pub async fn lease_until(&self, session: &Session, expires_at: Timestamp) {
        let mut tx = self.store.begin().await.expect("a transaction");
        tx.workspaces()
            .hold_lease(session, expires_at)
            .await
            .expect("the lease should hold");
        tx.commit().await.expect("the lease should commit");
    }

    /// Backdates when a Workspace was last active, the way `lease_until` backdates a lease: the
    /// only way to watch the idle sweep without waiting the window out.
    pub async fn last_active(&self, workspace: &Workspace, at: Timestamp) {
        let mut tx = self.store.begin().await.expect("a transaction");
        tx.workspaces()
            .record_active(workspace.organization.id, workspace.id, at)
            .await
            .expect("the workspace should record when it was last active");
        tx.commit().await.expect("the record should commit");
    }

    /// The only way to watch the periodic sweep find what a missed event would have.
    pub async fn last_considered(&self, event: EventRecordId, at: Timestamp) {
        let mut tx = self.store.begin().await.expect("a transaction");
        tx.triggers()
            .considered(event, at)
            .await
            .expect("the held firing should record when it was considered");
        tx.commit().await.expect("the record should commit");
    }
}
