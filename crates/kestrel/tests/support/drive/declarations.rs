use std::num::NonZeroUsize;

use kestrel::agent;
use kestrel::domain::{Agent, Declared, Organization, Project};

use super::super::{Kestrel, placed_agent};

impl Kestrel {
    pub async fn declare_organization(&self, name: &str) -> Organization {
        let mut tx = self.store.begin().await.expect("a transaction");
        let organization = tx
            .organizations()
            .declare(name, None)
            .await
            .expect("the organization should declare")
            .record;
        tx.commit().await.expect("the declaration should commit");
        organization
    }

    pub async fn declare_limited_organization(&self, name: &str, maximum: usize) -> Organization {
        let mut tx = self.store.begin().await.expect("a transaction");
        let organization = tx
            .organizations()
            .declare(
                name,
                Some(NonZeroUsize::new(maximum).expect("at least one live instance")),
            )
            .await
            .expect("the organization should declare")
            .record;
        tx.commit().await.expect("the declaration should commit");
        organization
    }

    pub async fn organizations(&self) -> Vec<Organization> {
        let mut tx = self.store.begin().await.expect("a transaction");
        tx.organizations()
            .all()
            .await
            .expect("organizations should list")
    }

    pub async fn declare_project(
        &self,
        organization: &Organization,
        name: &str,
        repositories: &[String],
        branch: &str,
    ) -> Project {
        let mut tx = self.store.begin().await.expect("a transaction");
        let project = tx
            .projects()
            .declare(organization, name, repositories, &[], branch)
            .await
            .expect("the project should declare")
            .record;
        tx.commit().await.expect("the declaration should commit");
        project
    }

    pub async fn projects(&self, organization: &Organization) -> Vec<Project> {
        let mut tx = self.store.begin().await.expect("a transaction");
        tx.projects()
            .all(organization)
            .await
            .expect("projects should list")
    }

    pub async fn declare_agent(
        &self,
        organization: &Organization,
        name: &str,
        harness: &str,
        model: Option<&str>,
    ) -> Agent {
        self.try_declare_agent(organization, name, harness, model)
            .await
            .expect("the agent should declare")
    }

    pub async fn declare_agent_declaring(
        &self,
        organization: &Organization,
        name: &str,
        harness: &str,
        declared: Declared,
    ) -> Agent {
        placed_agent(&self.store, &organization.name, name, harness, &declared)
            .await
            .expect("the agent should declare")
    }

    pub async fn try_declare_agent(
        &self,
        organization: &Organization,
        name: &str,
        harness: &str,
        model: Option<&str>,
    ) -> anyhow::Result<Agent> {
        placed_agent(
            &self.store,
            &organization.name,
            name,
            harness,
            &Declared {
                model: model.map(str::to_owned),
                ..Declared::default()
            },
        )
        .await
    }

    pub async fn set_agent_model(
        &self,
        organization: &Organization,
        name: &str,
        model: Option<&str>,
    ) -> Agent {
        agent::set_model(&self.store, &organization.name, name, model)
            .await
            .expect("the model should change")
    }

    pub async fn agents(&self, organization: &Organization) -> Vec<Agent> {
        let mut tx = self.store.begin().await.expect("a transaction");
        tx.agents()
            .all(organization)
            .await
            .expect("agents should list")
    }
}
