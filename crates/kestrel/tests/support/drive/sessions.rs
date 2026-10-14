use kestrel::domain::{Declared, Exit, Session, SessionId, SessionState, Turn, WorkspaceId};
use kestrel::link::credential::Secret;
use kestrel::link::{self};
use kestrel::log::Entry;
use kestrel::scheduling;
use kestrel::work;

use super::super::{Kestrel, OnTheLink, PATIENCE, SERIALIZED, harness};

impl Kestrel {
    pub async fn hold_session(&self, id: WorkspaceId, brief: &str) {
        let mut tx = self.store.begin().await.expect("a transaction");
        let held = tx
            .workspaces()
            .get(id)
            .await
            .expect("the workspace should read");
        tx.workspaces()
            .add_pending_session(
                &held,
                &kestrel::store::workspace::PendingSession {
                    agent: held.opened_with.clone(),
                    declared: Declared::default(),
                    trigger: "test".to_owned(),
                    brief: brief.to_owned(),
                },
            )
            .await
            .expect("the session should be held");
        tx.commit().await.expect("the session should commit");
    }

    pub async fn enqueue_session(&self, workspace: WorkspaceId) -> Session {
        self.try_enqueue_session(workspace)
            .await
            .expect("the session should enqueue")
    }

    /// What `session enqueue` still lets through directly, with nothing ever posted to the
    /// Workspace: a Session its first Turn has no instruction for.
    pub async fn enqueue_session_with_nothing_posted(&self, workspace: WorkspaceId) -> Session {
        work::enqueue(&self.store, workspace, None, Declared::default(), &[])
            .await
            .expect("the session should enqueue")
    }

    pub async fn try_enqueue_session(&self, workspace: WorkspaceId) -> anyhow::Result<Session> {
        self.instructed(workspace).await?;
        work::enqueue(&self.store, workspace, None, Declared::default(), &[]).await
    }

    pub async fn enqueue_session_as(&self, workspace: WorkspaceId, agent: &str) -> Session {
        self.try_enqueue_session_as(workspace, agent)
            .await
            .expect("the session should enqueue")
    }

    pub async fn try_enqueue_session_as(
        &self,
        workspace: WorkspaceId,
        agent: &str,
    ) -> anyhow::Result<Session> {
        self.instructed(workspace).await?;
        work::enqueue(
            &self.store,
            workspace,
            Some(agent),
            Declared::default(),
            &[],
        )
        .await
    }

    pub async fn enqueue_session_naming(
        &self,
        workspace: WorkspaceId,
        model: Option<&str>,
    ) -> Session {
        self.try_enqueue_session_naming(workspace, model)
            .await
            .expect("the session should enqueue")
    }

    pub async fn try_enqueue_session_naming(
        &self,
        workspace: WorkspaceId,
        model: Option<&str>,
    ) -> anyhow::Result<Session> {
        self.instructed(workspace).await?;
        work::enqueue(
            &self.store,
            workspace,
            None,
            Declared {
                model: model.map(str::to_owned),
                ..Declared::default()
            },
            &[],
        )
        .await
    }

    pub async fn enqueue_session_declaring(
        &self,
        workspace: WorkspaceId,
        declared: Declared,
    ) -> Session {
        self.instructed(workspace)
            .await
            .expect("the message should post");
        work::enqueue(&self.store, workspace, None, declared, &[])
            .await
            .expect("the session should enqueue")
    }

    /// What a fixture calling straight into `work::enqueue` skips: the message a real operator
    /// posts to give the Session it starts something to do. Appended before enqueuing, the way an
    /// operator's post always precedes the session it starts, so nothing can claim and start the
    /// Session before it has an instruction to run.
    async fn instructed(&self, workspace: WorkspaceId) -> anyhow::Result<()> {
        let mut tx = self.store.begin().await.expect("a transaction");
        let record = tx
            .workspaces()
            .get(workspace)
            .await
            .expect("the workspace should read");
        tx.log()
            .append(
                &record,
                Entry::Said {
                    participant: "operator".to_owned(),
                    message: "do the work this environment was provisioned for".to_owned(),
                    session_id: None,
                    completion: None,
                },
            )
            .await?;
        tx.commit().await.expect("the instruction should commit");
        Ok(())
    }

    /// Claims what it enqueued, standing in for the work role a `boot`ed fixture leaves idle.
    pub async fn dispatch_session(&self, workspace: WorkspaceId) -> Session {
        self.enqueue_session(workspace).await;
        self.claim_session()
            .await
            .expect("a session was just enqueued to claim")
    }

    /// Claims what it enqueued and puts it on an Instance's link, for a supervisor the test starts.
    pub async fn dispatch_to_the_link(&self, workspace: WorkspaceId) -> (Session, OnTheLink) {
        let session = self.dispatch_session(workspace).await;
        let on = self.on_the_link(&session).await;

        (self.session(session.id).await, on)
    }

    /// The Workspace's Instance, or one named the way `LocalExec` names a fresh one, with a new
    /// supervisor credential that replaces any the Instance had.
    pub async fn on_the_link(&self, session: &Session) -> OnTheLink {
        let instance = match self.instance(session.workspace).await {
            Some(instance) => instance,
            None => format!("local-exec/kestrel-{}", session.id),
        };

        self.on_the_link_at(session, &instance).await
    }

    /// On an Instance a test provisioned through the port itself.
    pub async fn on_the_link_at(&self, session: &Session, instance: &str) -> OnTheLink {
        let instance = instance.to_owned();
        self.executes_on(session, &instance).await;
        let credential = Secret::mint();
        let mut tx = self.store.begin().await.expect("a transaction");
        let workspace = tx
            .workspaces()
            .get(session.workspace)
            .await
            .expect("the workspace should read");
        tx.workspaces()
            .start_supervisor(&workspace, &instance, &credential.digest())
            .await
            .expect("the supervisor should be recorded");
        tx.commit().await.expect("the supervisor should commit");
        let provisioned_by = instance
            .strip_prefix("local-exec/kestrel-")
            .and_then(|session| session.parse().ok())
            .unwrap_or(session.id);

        OnTheLink {
            instance,
            credential,
            provisioned_by,
        }
    }

    /// An Instance a Workspace keeps with no Session on it, and a supervisor's credential for it.
    pub async fn keep_an_instance(&self, workspace: WorkspaceId, instance: &str) -> OnTheLink {
        let credential = Secret::mint();
        let mut tx = self.store.begin().await.expect("a transaction");
        let record = tx
            .workspaces()
            .get(workspace)
            .await
            .expect("the workspace should read");
        tx.workspaces()
            .record_instance(record.organization.id, workspace, Some(instance))
            .await
            .expect("the instance should be recorded");
        tx.workspaces()
            .start_supervisor(&record, instance, &credential.digest())
            .await
            .expect("the supervisor should be recorded");
        tx.commit().await.expect("the instance should commit");

        OnTheLink {
            instance: instance.to_owned(),
            credential,
            provisioned_by: SessionId::generate(),
        }
    }

    pub async fn claim_session(&self) -> Option<Session> {
        work::claim(&self.store, &[SERIALIZED.to_owned()])
            .await
            .expect("the claim should ask")
    }

    pub async fn occupy_session(&self) -> Option<Session> {
        match scheduling::dispatch(&self.store, 2, &[SERIALIZED.to_owned()])
            .await
            .expect("the occupancy should ask")
        {
            Some(scheduling::Occupied::Claimed(claimed)) => Some(claimed),
            Some(scheduling::Occupied::Resumed(_)) => panic!("no session should resume"),
            None => None,
        }
    }

    /// Prompts a waiting Session with what is held for it, the way the work role's sweep does.
    pub async fn prompt_waiting(&self) {
        scheduling::dispatch(&self.store, 1, &[SERIALIZED.to_owned()])
            .await
            .expect("the occupancy should ask");
    }

    pub async fn occupy_up_to(&self, slots: usize) -> Option<scheduling::Occupied> {
        scheduling::dispatch(&self.store, slots, &[SERIALIZED.to_owned()])
            .await
            .expect("the occupancy should ask")
    }

    pub async fn record_dispatch(&self, slots: usize, driver: &str) {
        let mut tx = self.store.begin().await.expect("a transaction");
        tx.queue()
            .record(slots, &[SERIALIZED.to_owned()], driver)
            .await
            .expect("the dispatch should record");
        tx.commit().await.expect("the record should commit");
    }

    pub async fn queue(&self, organization: &str) -> scheduling::Snapshot {
        scheduling::snapshot(&self.store, organization)
            .await
            .expect("the queue should read")
    }

    pub async fn waits_after_its_first_turn(&self, session: &Session) -> Session {
        self.on_the_link(session).await;
        link::start(&self.store, session, harness())
            .await
            .expect("the session should start");
        work::report_on(
            &self.store,
            session,
            Some(1),
            work::Report::Answered { usage: None },
        )
        .await
        .expect("the answer should be reported");
        self.report_settled(session, 2).await;

        self.session(session.id).await
    }

    /// Enqueued and blocked in one transaction, so dispatch never sees it unblocked and starts it
    /// unbriefed.
    pub async fn enqueue_blocked(&self, workspace: WorkspaceId, blocker: &Session) -> Session {
        let mut tx = self.store.begin().await.expect("a transaction");
        let workspace = tx
            .workspaces()
            .get(workspace)
            .await
            .expect("the workspace should read");
        let session = tx
            .workspaces()
            .enqueue_session(&workspace, None, Declared::default(), None)
            .await
            .expect("the session should enqueue");
        tx.workspaces()
            .declare_blocked(&session, blocker)
            .await
            .expect("the session should be declared blocked");
        tx.commit().await.expect("the enqueue should commit");
        session
    }

    pub async fn block_session(&self, session: &Session, blocker: &Session) {
        let mut tx = self.store.begin().await.expect("a transaction");
        tx.workspaces()
            .declare_blocked(session, blocker)
            .await
            .expect("the session should be declared blocked");
        tx.commit().await.expect("the declaration should commit");
    }

    pub async fn session(&self, id: SessionId) -> Session {
        work::session(&self.store, id)
            .await
            .expect("the session should show")
    }

    pub async fn sessions(&self, workspace: WorkspaceId) -> Vec<Session> {
        work::sessions(&self.store, workspace)
            .await
            .expect("the sessions should list")
    }

    pub async fn turns(&self, session: SessionId) -> Vec<Turn> {
        work::turns(&self.store, session)
            .await
            .expect("the session's turns should read")
    }

    pub async fn stop_session(&self, session: SessionId) -> Exit {
        self.try_stop_session(session)
            .await
            .expect("the session should stop")
    }

    pub async fn try_stop_session(&self, session: SessionId) -> anyhow::Result<Exit> {
        work::stop(&self.store, session, &[]).await
    }

    pub async fn answered(&self, session: SessionId, count: usize) -> Session {
        self.answered_within(session, count, PATIENCE).await
    }

    /// Once `count` of the Session's turns are answered and it has stopped trailing the last, or
    /// once it has ended short of them.
    pub async fn answered_within(
        &self,
        session: SessionId,
        count: usize,
        patience: std::time::Duration,
    ) -> Session {
        self.answering_within(session, count, patience, true).await
    }

    /// Once `count` of the Session's turns are answered, trailing or not.
    pub async fn answering(&self, session: SessionId, count: usize) -> Session {
        self.answering_within(session, count, PATIENCE, false).await
    }

    async fn answering_within(
        &self,
        session: SessionId,
        count: usize,
        patience: std::time::Duration,
        settled: bool,
    ) -> Session {
        let deadline = tokio::time::Instant::now() + patience;

        loop {
            let answered = self
                .turns(session)
                .await
                .iter()
                .filter(|turn| turn.answered_at.is_some())
                .count();
            let session = self.session(session).await;
            let trailing = settled && session.state == SessionState::Trailing;
            if (answered >= count && !trailing) || session.state == SessionState::Ended {
                return session;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "the session {} is {} with {answered} of {count} turns answered",
                session.id,
                session.state
            );
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    }

    /// A Session whose first turn is over has ended either way: by that turn, or stopped after it
    /// the way an operator would, because answering never ends one (ADR-0024).
    pub async fn after_one_turn(&self, session: SessionId) -> Session {
        self.after_one_turn_within(session, PATIENCE).await
    }

    pub async fn after_one_turn_within(
        &self,
        session: SessionId,
        patience: std::time::Duration,
    ) -> Session {
        let answered = self.answering_within(session, 1, patience, false).await;
        if answered.state != SessionState::Ended {
            self.try_stop_session(session)
                .await
                .expect("a waiting session should stop");
        }

        self.session(session).await
    }

    pub async fn complete_session(&self, session: &Session) {
        work::complete(&self.store, session)
            .await
            .expect("the session should end");
    }

    pub async fn fail_session(&self, session: &Session, because: &str) {
        work::fail(&self.store, session, because)
            .await
            .expect("the session should end");
    }

    pub async fn executes_on(&self, session: &Session, instance: &str) {
        work::executes_on(&self.store, session, instance)
            .await
            .expect("the instance should be recorded");
    }
}
