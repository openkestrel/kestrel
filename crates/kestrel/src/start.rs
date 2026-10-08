use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::declaration::{self, sharing_a_directory};
use crate::declined::Declined;
use crate::domain::{Declared, Session, Workspace};
use crate::provider;
use crate::store::{Declared as DeclaredRecord, Store};
use crate::workspace;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    pub organization: String,
    pub project: declaration::Project,
    pub agent: declaration::Agent,
    pub brief: String,
    #[serde(default)]
    pub credentials: Vec<Credential>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Credential {
    pub variable: String,
    pub secret: String,
}

pub struct Started {
    pub organization: Settled,
    pub project: Settled,
    pub agent: Settled,
    pub workspace: Workspace,
    pub session: Session,
}

#[derive(Serialize)]
pub struct Settled {
    pub name: String,
    pub created: bool,
}

/// A start only adds declarations and refuses one that would change, because the operator asked
/// for work rather than a redeclaration.
pub async fn start(store: &Store, plan: &Plan) -> Result<Started> {
    checked(plan)?;
    let declared = plan.agent.declared();

    let mut tx = store.begin().await?;
    // Redeclaring an Organization would clear the live Instance limit it may carry.
    let organization = match tx.organizations().find(&plan.organization).await? {
        Some(record) => DeclaredRecord {
            record,
            created: false,
        },
        None => tx.organizations().declare(&plan.organization, None).await?,
    };
    for credential in &plan.credentials {
        tx.organizations()
            .hold_provider_credential(
                organization.record.id,
                &credential.variable,
                &credential.secret,
            )
            .await?;
    }

    let found = tx
        .projects()
        .find(&organization.record, &plan.project.name)
        .await?;
    if let Some(found) = found.filter(|found| {
        found.repositories != plan.project.repositories || found.branch != plan.project.branch
    }) {
        return Err(Declined::Taken(format!(
            "the project {} is declared against {} on {}, and a start changes no declaration",
            found.name,
            found.repositories.join(", "),
            found.branch
        ))
        .into());
    }
    let project = tx
        .projects()
        .declare(
            &organization.record,
            &plan.project.name,
            &plan.project.repositories,
            &plan.project.branch,
        )
        .await?;

    let found = tx
        .agents()
        .find(&organization.record, &plan.agent.name)
        .await?;
    if let Some(found) =
        found.filter(|found| found.harness != plan.agent.harness || found.declared != declared)
    {
        return Err(Declined::Taken(format!(
            "the agent {} is declared on the harness {} with the model {}, and a start changes \
             no declaration",
            found.name,
            found.harness,
            found
                .declared
                .model
                .as_deref()
                .unwrap_or("its harness's default")
        ))
        .into());
    }
    let agent = tx
        .agents()
        .declare(
            &organization.record,
            &plan.agent.name,
            &plan.agent.harness,
            &declared,
        )
        .await?;

    let resolved = workspace::Resolved {
        project: project.record.clone(),
        agent: agent.record.clone(),
        profile: None,
        continues: None,
        branch: None,
        declared: Declared::default(),
        brief: Some(plan.brief.as_str()),
        participant: None,
    };
    let (workspace, session) =
        workspace::opened_in(&mut tx, &organization.record, &resolved).await?;
    tx.commit().await?;

    Ok(Started {
        organization: Settled {
            name: organization.record.name,
            created: organization.created,
        },
        project: Settled {
            name: project.record.name,
            created: project.created,
        },
        agent: Settled {
            name: agent.record.name,
            created: agent.created,
        },
        workspace,
        session,
    })
}

fn checked(plan: &Plan) -> Result<()> {
    for credential in &plan.credentials {
        provider::holdable("start", &credential.variable, &credential.secret)?;
    }
    let unacceptable = |why: &str| Err(Declined::Unacceptable(why.to_owned()).into());
    if [&plan.organization, &plan.project.name, &plan.agent.name]
        .iter()
        .any(|name| name.is_empty())
    {
        return unacceptable("a start names its organization, project and agent");
    }
    if plan.project.repositories.is_empty() {
        return unacceptable("a project names at least one repository");
    }
    if let Some(clash) = sharing_a_directory(&plan.project.repositories) {
        return Err(Declined::Unacceptable(clash).into());
    }
    if plan.project.branch.is_empty() {
        return unacceptable("a project names the branch its work happens on");
    }
    if plan.agent.harness.is_empty() {
        return unacceptable("an agent names the harness that drives it");
    }
    if plan.brief.trim().is_empty() {
        return unacceptable("a start carries a brief");
    }

    Ok(())
}
