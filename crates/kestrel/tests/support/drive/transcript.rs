use kestrel::domain::{HeldMessage, Session, SessionCommand, WorkspaceId};
use kestrel::log::{Cursor, Entry, Page, TranscriptEntry, Unreadable, Window};
use kestrel::workspace;

use super::super::Kestrel;

impl Kestrel {
    pub async fn transcript(&self, id: WorkspaceId) -> Vec<TranscriptEntry> {
        self.walk(id, None, Window::DEFAULT).await
    }

    /// The default read is shared state alone; this one includes narration and detail.
    pub async fn every_entry(&self, id: WorkspaceId) -> Vec<Entry> {
        let kinds: kestrel::log::Kinds =
            "shared_state,narration,detail".parse().expect("every kind");
        workspace::transcript(&self.store, id, None, Window::DEFAULT, &kinds)
            .await
            .expect("the transcript should read")
            .entries
            .into_iter()
            .map(|recorded| recorded.entry)
            .collect()
    }

    /// One bounded window is the only read there is, so a whole Transcript is a walk.
    pub async fn walk(
        &self,
        id: WorkspaceId,
        from: Option<Cursor>,
        window: Window,
    ) -> Vec<TranscriptEntry> {
        let mut walked = Vec::new();
        let mut cursor = from;

        loop {
            let page = self
                .page(id, cursor, window)
                .await
                .expect("the transcript should read");
            walked.extend(page.entries);
            cursor = page.cursor;

            if !page.more {
                return walked;
            }
        }
    }

    pub async fn page(
        &self,
        id: WorkspaceId,
        from: Option<Cursor>,
        window: Window,
    ) -> Result<Page, Unreadable> {
        workspace::transcript(&self.store, id, from, window, &Default::default()).await
    }

    pub async fn said(&self, session: &Session, message: &str) {
        self.try_said(session, message)
            .await
            .expect("the message should reach the transcript");
    }

    pub async fn retain_transcript(&self) -> anyhow::Result<usize> {
        kestrel::timer::expire_transcript(&self.store).await
    }

    pub async fn try_said(&self, session: &Session, message: &str) -> anyhow::Result<()> {
        let mut tx = self.store.begin().await.expect("a transaction");
        let workspace = tx.workspaces().get(session.workspace).await?;
        tx.log()
            .append(
                &workspace,
                Entry::Said {
                    participant: session.agent.name.clone(),
                    message: message.to_owned(),
                    session_id: Some(session.id),
                    completion: Some(kestrel::log::Completion::at(
                        "2026-09-29T12:00:00Z".parse().unwrap(),
                    )),
                },
            )
            .await?;
        tx.commit().await
    }

    pub async fn post(&self, id: WorkspaceId, participant: &str, message: &str) -> Session {
        self.post_while_busy(id, participant, message)
            .await
            .expect("an idle workspace should enqueue a session")
    }

    pub async fn post_while_busy(
        &self,
        id: WorkspaceId,
        participant: &str,
        message: &str,
    ) -> Option<Session> {
        self.posted_while_busy(id, participant, message)
            .await
            .session
    }

    pub async fn posted_while_busy(
        &self,
        id: WorkspaceId,
        participant: &str,
        message: &str,
    ) -> workspace::Posted {
        workspace::post(&self.store, id, Some(participant), message)
            .await
            .expect("the message should post")
    }

    pub async fn held_messages(&self, id: WorkspaceId) -> Vec<HeldMessage> {
        workspace::held_messages(&self.store, id)
            .await
            .expect("held messages should read")
    }

    pub async fn edit_message(
        &self,
        id: WorkspaceId,
        message: i64,
        participant: &str,
        text: &str,
    ) -> anyhow::Result<HeldMessage> {
        workspace::edit_message(&self.store, id, message, Some(participant), text).await
    }

    pub async fn withdraw_message(
        &self,
        id: WorkspaceId,
        message: i64,
        participant: &str,
    ) -> anyhow::Result<()> {
        workspace::withdraw_message(&self.store, id, message, Some(participant)).await
    }

    pub async fn record_commands(&self, session: &Session, commands: &[SessionCommand]) {
        let mut tx = self.store.begin().await.expect("a transaction");
        tx.workspaces()
            .record_session_info(
                session,
                session.title.as_deref(),
                &session.options,
                commands,
            )
            .await
            .expect("the commands should record");
        tx.commit().await.expect("the commands should commit");
    }
}
