use std::fmt;

use anyhow::Result;

use crate::capability::Images;
use crate::declaration;
use crate::declined::Constraint;
use crate::domain::{Declared, Session, Workspace};
use crate::integration::github::Github;
use crate::participant;
use crate::provider;
use crate::sign_in::{Authentication, Source as SignInSource, handoff};
use crate::start_readiness::Gap;
use crate::store::{Declared as DeclaredRecord, Store};
use crate::workspace;

mod plan;
mod resolving;

pub use plan::{
    Action, Credential, Kind, LocalClone, OPERATION, Outcome, Preview, Request, Source, Step,
    Valued, preview,
};

pub struct Started {
    pub organization: Settled,
    pub project: Settled,
    pub agent: Settled,
    pub workspace: Workspace,
    pub session: Session,
    pub completed_initial_setup: bool,
}

pub struct Settled {
    pub name: String,
    pub created: bool,
}

/// A setup prerequisite the plan's path lacks, refusing the start it would block.
#[derive(Debug)]
pub struct Blocked {
    pub gap: Gap,
    pub organization: Option<String>,
}

impl fmt::Display for Blocked {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.gap.message(self.organization.as_deref()))
    }
}

impl std::error::Error for Blocked {}

/// A start resolves its plan again from what exists now, so nothing a preview read is trusted;
/// it only adds declarations and refuses one that would change.
pub async fn start(
    store: &Store,
    images: &Images,
    github: &Github,
    request: &Request,
) -> Result<Started> {
    for credential in &request.credentials {
        let Some(secret) = &credential.secret else {
            return Err(declaration::scoped(
                declaration::Prefix::Root,
                OPERATION,
                declaration::invalid(
                    "credentials",
                    Constraint::NonEmpty,
                    format!("a start holds {} only with its secret", credential.variable),
                ),
            ));
        };
        provider::holdable(OPERATION, &credential.variable, secret)?;
    }
    let preview = preview(store, images, request).await?;
    let complete = complete(preview, request.brief.as_deref())?;

    let project = declaration::check_project(
        store,
        github,
        &complete.organization,
        &complete.project,
        OPERATION,
    )
    .await
    .map_err(|error| declaration::scoped(declaration::Prefix::Project, OPERATION, error))?;
    let agent = &complete.agent;
    let agent = declaration::check_agent(
        images,
        &agent.name,
        &agent.harness,
        &agent.declared(),
        OPERATION,
    )
    .await
    .map_err(|error| declaration::scoped(declaration::Prefix::Agent, OPERATION, error))?;

    let mut tx = store.begin().await?;
    // Redeclaring an Organization would clear the live Instance limit it may carry.
    let organization = match tx.organizations().find(&complete.organization).await? {
        Some(record) => DeclaredRecord {
            record,
            created: false,
        },
        None => {
            tx.organizations()
                .declare(&complete.organization, None)
                .await?
        }
    };
    for credential in &request.credentials {
        tx.organizations()
            .hold_provider_credential(
                organization.record.id,
                &credential.variable,
                credential.secret.as_deref().unwrap_or_default(),
                &Authentication::unchecked(SignInSource::GenericWrite, store.clock().now()),
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
    .map_err(|error| declaration::scoped(declaration::Prefix::Project, OPERATION, error))?;
    let (agent, _) = declaration::place_agent(
        &mut tx,
        &organization.record,
        &agent,
        declaration::OnExisting::Refuse,
    )
    .await
    .map_err(|error| declaration::scoped(declaration::Prefix::Agent, OPERATION, error))?;

    let sign_in_method = request.sign_in_method.as_deref();
    let (profile, sign_in_method) = match &request.profile {
        Some(profile) => (
            Some(workspace::named(
                "profile",
                tx.profiles().named(&organization.record, profile).await,
            )?),
            sign_in_method,
        ),
        None => {
            workspace::operators_own(
                &mut tx,
                OPERATION,
                &organization.record,
                &agent.record.harness,
                sign_in_method,
            )
            .await?
        }
    };
    if let Some(method) = request.sign_in_method.as_deref() {
        handoff::selected(
            &mut tx,
            OPERATION,
            &organization.record,
            &agent.record.harness,
            profile.as_ref(),
            method,
        )
        .await?;
    }
    let resolved = workspace::Resolved {
        project: project.record.clone(),
        agent: agent.record.clone(),
        profile,
        sign_in_method,
        continues: None,
        branch: None,
        declared: Declared::default(),
        brief: Some(complete.brief.as_str()),
        participant: participant::operator(&mut tx, &organization.record, OPERATION).await?,
    };
    let (workspace, session) =
        workspace::opened_in(&mut tx, &organization.record, &resolved).await?;
    let completed_initial_setup = tx.operators().complete_initial_setup().await?;
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
        completed_initial_setup,
    })
}

struct Complete {
    organization: String,
    project: declaration::Project,
    agent: declaration::Agent,
    brief: String,
}

/// The first conflict, gap or missing input refuses the start, in the order a preview reports
/// them.
fn complete(preview: Preview, brief: Option<&str>) -> Result<Complete> {
    let organization = preview
        .organization
        .as_ref()
        .map(|organization| organization.value.clone());
    if let Some(conflict) = preview.conflicts.into_iter().next() {
        return Err(conflict);
    }
    if let Some(gap) = preview.readiness.gaps.into_iter().next() {
        return Err(match gap {
            Gap::Refused(reason) => reason.into(),
            gap => Blocked { gap, organization }.into(),
        });
    }
    if let Some(missing) = preview.missing.into_iter().next() {
        return Err(missing);
    }
    let (Some(organization), Some(project), Some(repositories), Some(branch), Some(brief)) = (
        preview.organization,
        preview.project,
        preview.repositories,
        preview.branch,
        brief,
    ) else {
        anyhow::bail!("a start plan reported nothing missing and still left a value unresolved");
    };

    Ok(Complete {
        organization: organization.value,
        project: declaration::Project {
            name: project.value,
            repositories: repositories
                .value
                .into_iter()
                .map(|resolved| resolved.address)
                .collect(),
            branch: branch.value,
            integrations: preview.integrations,
        },
        agent: declaration::Agent {
            name: preview.agent.value,
            harness: preview.harness.value,
            model: preview.model.value,
            mode: None,
            thought_level: None,
        },
        brief: brief.to_owned(),
    })
}
