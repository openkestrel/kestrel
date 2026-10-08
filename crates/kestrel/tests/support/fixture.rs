use kestrel::domain::{Declared, Organization, Workspace};

use super::{A_PROVIDER_KEY, HARNESS, Kestrel, PROVIDER_KEY, repository};

/// The first Agent is the one a Workspace opens with.
#[derive(Clone)]
pub struct Fixture {
    organization: String,
    instance_limit: Option<usize>,
    project: String,
    repositories: Vec<String>,
    branch: String,
    agents: Vec<AgentDeclaration>,
    provider_key: Option<String>,
}

impl Fixture {
    /// `acme` declaring the `kestrel` Project on a repository nothing clones, and a `builder`
    /// Agent on the default harness that names no model.
    pub fn acme() -> Self {
        Self {
            organization: "acme".to_owned(),
            instance_limit: None,
            project: repository::NAME.to_owned(),
            repositories: vec!["https://github.com/jtmthf/kestrel".to_owned()],
            branch: repository::BRANCH.to_owned(),
            agents: vec![AgentDeclaration::new("builder", HARNESS, None)],
            provider_key: None,
        }
    }

    pub fn organization(mut self, name: &str) -> Self {
        name.clone_into(&mut self.organization);
        self
    }

    pub fn limited_to(mut self, maximum: usize) -> Self {
        self.instance_limit = Some(maximum);
        self
    }

    pub fn project(mut self, name: &str) -> Self {
        name.clone_into(&mut self.project);
        self
    }

    pub fn repositories(mut self, repositories: &[impl AsRef<str>]) -> Self {
        self.repositories = repositories
            .iter()
            .map(|url| url.as_ref().to_owned())
            .collect();
        self
    }

    pub fn without_repositories(mut self) -> Self {
        self.repositories.clear();
        self
    }

    /// The local repository a supervisor can clone.
    pub fn checked_out(self) -> Self {
        self.repositories(&[repository::url()])
    }

    pub fn agent_name(mut self, name: &str) -> Self {
        name.clone_into(&mut self.agents[0].name);
        self
    }

    pub fn branch(mut self, branch: &str) -> Self {
        branch.clone_into(&mut self.branch);
        self
    }

    pub fn harness(mut self, harness: &str) -> Self {
        harness.clone_into(&mut self.agents[0].harness);
        self
    }

    pub fn model<'m>(self, model: impl Into<Option<&'m str>>) -> Self {
        self.declaring(naming(model.into()))
    }

    pub fn declaring(mut self, declared: Declared) -> Self {
        self.agents[0].declared = declared;
        self
    }

    pub fn agent(mut self, name: &str, harness: &str, model: Option<&str>) -> Self {
        self.agents
            .push(AgentDeclaration::new(name, harness, model));
        self
    }

    pub fn holding_a_provider_key(self) -> Self {
        self.holding(A_PROVIDER_KEY)
    }

    pub fn holding(mut self, secret: &str) -> Self {
        self.provider_key = Some(secret.to_owned());
        self
    }

    pub async fn declare(&self, kestrel: &Kestrel) -> Organization {
        let organization = match self.instance_limit {
            Some(maximum) => {
                kestrel
                    .declare_limited_organization(&self.organization, maximum)
                    .await
            }
            None => kestrel.declare_organization(&self.organization).await,
        };
        kestrel
            .declare_project(
                &organization,
                &self.project,
                &self.repositories,
                &self.branch,
            )
            .await;
        for agent in &self.agents {
            kestrel
                .declare_agent_declaring(
                    &organization,
                    &agent.name,
                    &agent.harness,
                    agent.declared.clone(),
                )
                .await;
        }
        if let Some(secret) = &self.provider_key {
            kestrel
                .hold_provider_credential(&organization, PROVIDER_KEY, secret)
                .await;
        }

        organization
    }

    pub async fn open(&self, kestrel: &Kestrel) -> Workspace {
        self.declare(kestrel).await;
        self.open_another(kestrel).await
    }

    /// Opens a Workspace on what an earlier `declare` or `open` already declared.
    pub async fn open_another(&self, kestrel: &Kestrel) -> Workspace {
        kestrel
            .open_workspace(&self.organization, &self.project, &self.agents[0].name)
            .await
    }
}

#[derive(Clone)]
struct AgentDeclaration {
    name: String,
    harness: String,
    declared: Declared,
}

impl AgentDeclaration {
    fn new(name: &str, harness: &str, model: Option<&str>) -> Self {
        Self {
            name: name.to_owned(),
            harness: harness.to_owned(),
            declared: naming(model),
        }
    }
}

fn naming(model: Option<&str>) -> Declared {
    Declared {
        model: model.map(str::to_owned),
        ..Declared::default()
    }
}
