use axum::Json;
use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Response;
use kestrel_operator_types as wire;

use crate::domain::SubscriptionProfile;
use crate::profile::{self, Entry};
use crate::provider::{self, Held};

use super::{ControlPlane, Refused, answered, named, wire_id};

impl From<Held> for wire::ProviderCredential {
    fn from(held: Held) -> Self {
        Self {
            variable: held.variable,
            set_at: held.set_at.to_string(),
        }
    }
}

pub(super) async fn credentials(
    State(control_plane): State<ControlPlane>,
    Path(organization): Path<String>,
) -> Result<Json<Vec<wire::ProviderCredential>>, Refused> {
    let held = provider::held(&control_plane.store, &organization).await?;

    Ok(Json(held.into_iter().map(Into::into).collect()))
}

pub(super) async fn hold_credential(
    State(control_plane): State<ControlPlane>,
    Path((organization, variable)): Path<(String, String)>,
    secret: Result<Json<wire::ProviderCredentialSecret>, JsonRejection>,
) -> Result<Json<wire::ProviderCredential>, Refused> {
    let Json(secret) = secret?;
    let held = provider::hold(
        &control_plane.store,
        &organization,
        &variable,
        &secret.secret,
    )
    .await?;

    Ok(Json(held.into()))
}

pub(super) async fn forget_credential(
    State(control_plane): State<ControlPlane>,
    Path((organization, variable)): Path<(String, String)>,
) -> Result<StatusCode, Refused> {
    provider::forget(&control_plane.store, &organization, &variable).await?;

    Ok(StatusCode::NO_CONTENT)
}

impl From<SubscriptionProfile> for wire::SubscriptionProfile {
    fn from(profile: SubscriptionProfile) -> Self {
        Self {
            id: wire_id(&profile.id),
            name: profile.name,
            owner: profile.owner,
            owner_operator: profile.owner_operator.map(|id| wire_id(&id)),
        }
    }
}

impl From<profile::Held> for wire::SubscriptionProfileLogin {
    fn from(held: profile::Held) -> Self {
        Self {
            kind: match held.entry.kind {
                profile::Kind::Variable => wire::SubscriptionProfileLoginKind::Variable,
                profile::Kind::File => wire::SubscriptionProfileLoginKind::File,
            },
            name: held.entry.name,
            set_at: held.set_at.to_string(),
        }
    }
}

pub(super) async fn profiles(
    State(control_plane): State<ControlPlane>,
    Path(organization): Path<String>,
) -> Result<Json<wire::ListSubscriptionProfilesResponse>, Refused> {
    let listed = profile::profiles(&control_plane.store, &organization).await?;

    Ok(Json(
        listed
            .into_iter()
            .map(|(profile, held)| {
                let profile = wire::SubscriptionProfile::from(profile);
                wire::SubscriptionProfileListed {
                    id: profile.id,
                    name: profile.name,
                    owner: profile.owner,
                    owner_operator: profile.owner_operator,
                    holds: held.into_iter().map(Into::into).collect(),
                }
            })
            .collect(),
    ))
}

pub(super) async fn declare_profile(
    State(control_plane): State<ControlPlane>,
    Path(organization): Path<String>,
    declaration: Result<Json<wire::SubscriptionProfileDeclaration>, JsonRejection>,
) -> Result<Response, Refused> {
    let Json(declaration) = declaration?;
    named("declare_subscription_profile", &declaration.name)?;

    let declared = profile::declare(
        &control_plane.store,
        &organization,
        &declaration.name,
        declaration.owner.as_deref(),
    )
    .await?;

    Ok(answered::<_, wire::SubscriptionProfile>(declared))
}

pub(super) async fn hold_profile_variable(
    State(control_plane): State<ControlPlane>,
    Path((organization, profile, variable)): Path<(String, String, String)>,
    secret: Result<Json<wire::ProviderCredentialSecret>, JsonRejection>,
) -> Result<Json<wire::SubscriptionProfileLogin>, Refused> {
    let entry = Entry::variable(&variable)?;
    held_in_profile(&control_plane, &organization, &profile, &entry, secret).await
}

pub(super) async fn hold_profile_file(
    State(control_plane): State<ControlPlane>,
    Path((organization, profile, path)): Path<(String, String, String)>,
    secret: Result<Json<wire::ProviderCredentialSecret>, JsonRejection>,
) -> Result<Json<wire::SubscriptionProfileLogin>, Refused> {
    let entry = Entry::file(&path)?;
    held_in_profile(&control_plane, &organization, &profile, &entry, secret).await
}

pub(super) async fn held_in_profile(
    control_plane: &ControlPlane,
    organization: &str,
    profile: &str,
    entry: &Entry,
    secret: Result<Json<wire::ProviderCredentialSecret>, JsonRejection>,
) -> Result<Json<wire::SubscriptionProfileLogin>, Refused> {
    let Json(secret) = secret?;
    let held = profile::hold(
        &control_plane.store,
        organization,
        profile,
        entry,
        &secret.secret,
    )
    .await?;

    Ok(Json(held.into()))
}

pub(super) async fn forget_profile_variable(
    State(control_plane): State<ControlPlane>,
    Path((organization, profile, variable)): Path<(String, String, String)>,
) -> Result<StatusCode, Refused> {
    let entry = Entry::variable(&variable)?;
    profile::forget(&control_plane.store, &organization, &profile, &entry).await?;

    Ok(StatusCode::NO_CONTENT)
}

pub(super) async fn forget_profile_file(
    State(control_plane): State<ControlPlane>,
    Path((organization, profile, path)): Path<(String, String, String)>,
) -> Result<StatusCode, Refused> {
    let entry = Entry::file(&path)?;
    profile::forget(&control_plane.store, &organization, &profile, &entry).await?;

    Ok(StatusCode::NO_CONTENT)
}
