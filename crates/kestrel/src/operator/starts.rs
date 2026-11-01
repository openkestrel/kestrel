//! The shared start plan: a preview that writes nothing and the start that commits it.

use axum::Json;
use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use axum::http::StatusCode;
use kestrel_operator_types as wire;

use super::{ControlPlane, Refused, readiness, session_record, workspace_record};
use crate::integration::github::Github;
use crate::repository;
use crate::start::{self, Action, Kind, Outcome, Source, Valued};

pub(super) async fn preview(
    State(control_plane): State<ControlPlane>,
    plan: Result<Json<wire::StartPlan>, JsonRejection>,
) -> Result<Json<wire::StartPreview>, Refused> {
    let Json(plan) = plan?;
    let previewed =
        start::preview(&control_plane.store, &control_plane.images, &request(plan)).await?;

    Ok(Json(previewed_plan(previewed, &control_plane.client)?))
}

pub(super) async fn start(
    State(control_plane): State<ControlPlane>,
    plan: Result<Json<wire::StartPlan>, JsonRejection>,
) -> Result<(StatusCode, Json<wire::Started>), Refused> {
    let Json(plan) = plan?;
    let started = start::start(
        &control_plane.store,
        &control_plane.images,
        &Github::unfenced(control_plane.store.clock())?,
        &request(plan),
    )
    .await?;
    let organization = started.organization.name.clone();

    Ok((
        StatusCode::CREATED,
        Json(wire::Started {
            organization: settled(started.organization),
            project: settled(started.project),
            agent: settled(started.agent),
            workspace: workspace_record(&control_plane.store, started.workspace).await?,
            session: session_record(
                started.session,
                &organization,
                &control_plane.live.summaries,
            ),
            completed_initial_setup: started.completed_initial_setup,
        }),
    ))
}

fn settled(settled: start::Settled) -> wire::Settled {
    wire::Settled {
        name: settled.name,
        created: settled.created,
    }
}

fn request(plan: wire::StartPlan) -> start::Request {
    start::Request {
        organization: plan.organization,
        project: plan.project,
        repositories: plan.repositories.unwrap_or_default(),
        integrations: plan.integrations.unwrap_or_default(),
        branch: plan.branch,
        agent: plan.agent,
        harness: plan.harness,
        model: plan.model,
        profile: plan.profile,
        sign_in_method: plan.sign_in_method,
        brief: plan.brief,
        credentials: plan
            .credentials
            .unwrap_or_default()
            .into_iter()
            .map(|credential| start::Credential {
                variable: credential.variable,
                secret: credential.secret,
            })
            .collect(),
        clone: plan.clone.map(|clone| start::LocalClone {
            repository: clone.repository,
            default_branch: clone.default_branch,
            checked_out: clone.checked_out,
        }),
    }
}

fn previewed_plan(
    previewed: start::Preview,
    client: &url::Url,
) -> Result<wire::StartPreview, Refused> {
    let outcome = match previewed.outcome() {
        Outcome::Ready => wire::StartPreviewOutcome::Ready,
        Outcome::Incomplete => wire::StartPreviewOutcome::Incomplete,
        Outcome::Blocked => wire::StartPreviewOutcome::Blocked,
        Outcome::Unavailable => wire::StartPreviewOutcome::Unavailable,
    };
    let selected = selection(&previewed.integrations);
    let mut gaps = Vec::new();
    for conflict in previewed.conflicts {
        gaps.push(diagnostic(conflict)?);
    }
    let missing = previewed
        .missing
        .into_iter()
        .map(diagnostic)
        .collect::<Result<_, _>>()?;
    // The path's prerequisites come before the declarations the start would change.
    let read = readiness(previewed.readiness, client);
    gaps.splice(0..0, read.gaps);

    Ok(wire::StartPreview {
        outcome,
        organization: previewed.organization.map(scalar),
        project: previewed.project.map(scalar),
        repositories: previewed
            .repositories
            .map(|repositories| wire::StartRepositories {
                value: repositories
                    .value
                    .into_iter()
                    .map(|resolved| wire::ResolvedRepository {
                        integration: selected(&resolved),
                        given: resolved.given,
                        address: resolved.address,
                        checkout_directory: resolved.directory,
                    })
                    .collect(),
                source: source(repositories.source),
                record: repositories.record,
            }),
        branch: previewed.branch.map(scalar),
        agent: scalar(previewed.agent),
        harness: scalar(previewed.harness),
        model: optional(previewed.model),
        profile: optional(previewed.profile),
        sign_in_method: optional(previewed.sign_in_method),
        sign_in: read.sign_in,
        saved: read.saved,
        declarations: previewed
            .declarations
            .into_iter()
            .map(|step| wire::StartDeclaration {
                kind: match step.kind {
                    Kind::Organization => wire::StartDeclarationKind::Organization,
                    Kind::Project => wire::StartDeclarationKind::Project,
                    Kind::Agent => wire::StartDeclarationKind::Agent,
                    Kind::ProviderCredential => wire::StartDeclarationKind::ProviderCredential,
                    Kind::Workspace => wire::StartDeclarationKind::Workspace,
                },
                name: step.name,
                action: match step.action {
                    Action::Declare => wire::StartDeclarationAction::Declare,
                    Action::Reuse => wire::StartDeclarationAction::Reuse,
                    Action::Hold => wire::StartDeclarationAction::Hold,
                    Action::Replace => wire::StartDeclarationAction::Replace,
                    Action::Open => wire::StartDeclarationAction::Open,
                },
            })
            .collect(),
        initial_setup_complete: read.initial_setup_complete,
        missing,
        gaps,
        warnings: read.warnings,
        unavailable: read.unavailable,
    })
}

fn selection(
    integrations: &[repository::Selection],
) -> impl Fn(&repository::Resolved) -> Option<String> + '_ {
    move |resolved| {
        integrations
            .iter()
            .find(|selected| {
                selected.repository == resolved.address || selected.repository == resolved.given
            })
            .map(|selected| selected.integration.clone())
    }
}

/// A missing input or a conflict is reported the way a start would refuse it.
fn diagnostic(error: anyhow::Error) -> Result<wire::Diagnostic, Refused> {
    match Refused::from(error) {
        Refused::Diagnosed { diagnostic, .. } => Ok(*diagnostic),
        refused => Err(refused),
    }
}

fn scalar(valued: Valued<String>) -> wire::StartValue {
    optional(Valued {
        value: Some(valued.value),
        source: valued.source,
        record: valued.record,
    })
}

fn optional(valued: Valued<Option<String>>) -> wire::StartValue {
    wire::StartValue {
        value: valued.value,
        source: source(valued.source),
        record: valued.record,
    }
}

const fn source(source: Source) -> wire::StartSource {
    match source {
        Source::Given => wire::StartSource::Given,
        Source::CloneOrigin => wire::StartSource::CloneOrigin,
        Source::CloneDefaultBranch => wire::StartSource::CloneDefaultBranch,
        Source::CloneCheckedOut => wire::StartSource::CloneCheckedOut,
        Source::OnlyOrganization => wire::StartSource::OnlyOrganization,
        Source::Project => wire::StartSource::Project,
        Source::OnlyAgent => wire::StartSource::OnlyAgent,
        Source::Agent => wire::StartSource::Agent,
        Source::NamedForRepository => wire::StartSource::NamedForRepository,
        Source::NamedForHarness => wire::StartSource::NamedForHarness,
        Source::SavedSignIn => wire::StartSource::SavedSignIn,
        Source::OperatorProfile => wire::StartSource::OperatorProfile,
        Source::Default => wire::StartSource::Default,
    }
}
