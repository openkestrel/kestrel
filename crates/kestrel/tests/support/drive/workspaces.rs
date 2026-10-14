use kestrel::domain::{Workspace, WorkspaceId};
use kestrel::instance;
use kestrel::workspace;

use super::super::Kestrel;

impl Kestrel {
    pub async fn open_workspace_with(
        &self,
        organization: &str,
        project: &str,
        agent: &str,
        profile: &str,
    ) -> Workspace {
        workspace::open_without_a_session(
            &self.store,
            organization,
            project,
            agent,
            Some(profile),
            None,
            None,
        )
        .await
        .expect("the workspace should open")
    }

    pub async fn open_workspace(
        &self,
        organization: &str,
        project: &str,
        agent: &str,
    ) -> Workspace {
        self.try_open_workspace(organization, project, agent, None)
            .await
            .expect("the workspace should open")
    }

    pub async fn open_workspace_on(
        &self,
        organization: &str,
        project: &str,
        agent: &str,
        branch: &str,
    ) -> Workspace {
        workspace::open_without_a_session(
            &self.store,
            organization,
            project,
            agent,
            None,
            Some(branch),
            None,
        )
        .await
        .expect("the workspace should open")
    }

    pub async fn continue_workspace(
        &self,
        organization: &str,
        project: &str,
        agent: &str,
        continues: WorkspaceId,
    ) -> Workspace {
        self.try_open_workspace(organization, project, agent, Some(continues))
            .await
            .expect("the workspace should open")
    }

    pub async fn try_open_workspace(
        &self,
        organization: &str,
        project: &str,
        agent: &str,
        continues: Option<WorkspaceId>,
    ) -> anyhow::Result<Workspace> {
        let continues = continues.map(|sealed| sealed.to_string());
        workspace::open_without_a_session(
            &self.store,
            organization,
            project,
            agent,
            None,
            None,
            continues.as_deref(),
        )
        .await
    }

    pub async fn seal_workspace(&self, id: WorkspaceId) -> Workspace {
        self.try_seal_workspace(id)
            .await
            .expect("the workspace should seal")
    }

    pub async fn try_seal_workspace(&self, id: WorkspaceId) -> anyhow::Result<Workspace> {
        workspace::seal(&self.store, id).await
    }

    pub async fn held_instances(&self, organization: &str) -> Vec<instance::Held> {
        instance::held(&self.store, organization)
            .await
            .expect("the held instances should read")
    }

    pub async fn release_instance(&self, workspace: WorkspaceId) -> String {
        self.try_release_instance(workspace)
            .await
            .expect("the instance should release")
    }

    pub async fn try_release_instance(&self, workspace: WorkspaceId) -> anyhow::Result<String> {
        instance::release(&self.store, workspace, Some("operator")).await
    }

    pub async fn continuations(&self, id: WorkspaceId) -> Vec<WorkspaceId> {
        workspace::continuations(&self.store, id)
            .await
            .expect("the continuations should read")
    }

    pub async fn workspaces(&self, organization: &str) -> Vec<Workspace> {
        workspace::workspaces(&self.store, organization)
            .await
            .expect("the workspaces should list")
    }

    pub async fn show_workspace(&self, id: WorkspaceId) -> Workspace {
        workspace::show(&self.store, id)
            .await
            .expect("the workspace should show")
    }
}
