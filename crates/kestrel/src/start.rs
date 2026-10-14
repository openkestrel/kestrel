use anyhow::Result;

use crate::capability::Images;
use crate::declaration;
use crate::declined::Constraint;
use crate::domain::{Declared, Session, Workspace};
use crate::integration::github::Github;
use crate::participant;
use crate::provider;
use crate::sign_in::{Authentication, Source};
use crate::store::{Declared as DeclaredRecord, Store};
use crate::workspace;

pub struct Plan {
    pub organization: String,
    pub project: declaration::Project,
    pub agent: declaration::Agent,
    pub brief: String,
    pub credentials: Vec<Credential>,
}

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

pub struct Settled {
    pub name: String,
    pub created: bool,
}

/// A start only adds declarations and refuses one that would change, because the operator asked
/// for work rather than a redeclaration.
pub async fn start(
    store: &Store,
    images: &Images,
    github: &Github,
    plan: &Plan,
) -> Result<Started> {
    for credential in &plan.credentials {
        provider::holdable("start", &credential.variable, &credential.secret)?;
    }
    if plan.organization.is_empty() {
        return Err(declaration::scoped(
            declaration::Prefix::Root,
            "start",
            declaration::invalid(
                "organization",
                Constraint::NonEmpty,
                "a start names its organization, project and agent",
            ),
        ));
    }
    let project =
        declaration::check_project(store, github, &plan.organization, &plan.project, "start")
            .await
            .map_err(|error| declaration::scoped(declaration::Prefix::Project, "start", error))?;
    let declared = plan.agent.declared();
    let agent = declaration::check_agent(
        images,
        &plan.agent.name,
        &plan.agent.harness,
        &declared,
        "start",
    )
    .await
    .map_err(|error| declaration::scoped(declaration::Prefix::Agent, "start", error))?;
    if plan.brief.trim().is_empty() {
        return Err(declaration::scoped(
            declaration::Prefix::Root,
            "start",
            declaration::invalid("brief", Constraint::NonEmpty, "a start carries a brief"),
        ));
    }

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
                &Authentication::unchecked(Source::GenericWrite),
            )
            .await?;
    }

    let (project, _) = declaration::place_project(
        &mut tx,
        &organization.record,
        &project,
        declaration::OnExisting::Refuse,
    )
    .await
    .map_err(|error| declaration::scoped(declaration::Prefix::Project, "start", error))?;
    let (agent, _) = declaration::place_agent(
        &mut tx,
        &organization.record,
        &agent,
        declaration::OnExisting::Refuse,
    )
    .await
    .map_err(|error| declaration::scoped(declaration::Prefix::Agent, "start", error))?;

    let (profile, sign_in_method) = workspace::operators_own(
        &mut tx,
        "start",
        &organization.record,
        &agent.record.harness,
        None,
    )
    .await?;
    let resolved = workspace::Resolved {
        project: project.record.clone(),
        agent: agent.record.clone(),
        profile,
        sign_in_method,
        continues: None,
        branch: None,
        declared: Declared::default(),
        brief: Some(plan.brief.as_str()),
        participant: participant::operator(&mut tx, &organization.record, "start").await?,
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
