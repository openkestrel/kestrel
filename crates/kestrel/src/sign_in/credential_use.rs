//! Checks never reserve a sign-in; acquire rechecks under the single-writer Tx so a lost
//! race returns the same busy refusal before any material is lent.

use anyhow::{Result, bail};

use crate::declined::{Next, Reason, Resource};
use crate::domain::{Organization, Session, SessionId, SubscriptionProfile, SubscriptionProfileId};
use crate::store::{Store, Tx};
use crate::workspace;

pub const ACQUIRE: &str = "use_sign_in";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Holder {
    Session { session: String, workspace: String },
    CredentialUse(String),
}

impl Holder {
    pub(crate) fn session(&self) -> Option<&str> {
        match self {
            Self::Session { session, .. } => Some(session),
            Self::CredentialUse(_) => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Busy {
    pub profile: String,
    pub organization: String,
    pub harness: String,
    pub holder: Holder,
    pub next: Next,
}

impl std::fmt::Display for Busy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let by = match &self.holder {
            Holder::Session { session, .. } => format!("the session {session}"),
            Holder::CredentialUse(holder) => holder.clone(),
        };
        write!(
            f,
            "the {} sign-in in the subscription profile {} is in use by {by}",
            self.harness, self.profile
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CredentialUse {
    pub profile: SubscriptionProfileId,
    pub harness: String,
    pub holder: String,
}

async fn check(
    tx: &mut Tx<'_>,
    profile: &SubscriptionProfile,
    organization: &Organization,
    harness: &str,
    serialized: &[String],
    except: Option<SessionId>,
) -> Result<Option<Busy>> {
    let serialized = serialized.iter().any(|name| name == harness);
    // Non-Session loans stay exclusive even when the harness permits overlapping Sessions.
    let held = if serialized
        && let Some(session) = tx
            .sign_ins()
            .session_using(profile, harness, except)
            .await?
    {
        let session = tx.workspaces().session(session).await?;
        let workspace = tx.workspaces().get(session.workspace).await?;
        Some((
            Holder::Session {
                session: session.name.clone(),
                workspace: workspace.name,
            },
            workspace::stoppable(Next::inspect(Resource::Session, &session.name), &session),
        ))
    } else if serialized || except.is_none() {
        tx.sign_ins().holder(profile, harness).await?.map(|holder| {
            (
                Holder::CredentialUse(holder),
                Next::inspect(Resource::SubscriptionProfile, &profile.name),
            )
        })
    } else {
        None
    };
    let Some((holder, next)) = held else {
        return Ok(None);
    };
    Ok(Some(Busy {
        profile: profile.name.clone(),
        organization: organization.name.clone(),
        harness: harness.to_owned(),
        holder,
        next,
    }))
}

pub async fn check_session(
    tx: &mut Tx<'_>,
    session: &Session,
    serialized: &[String],
) -> Result<Option<Busy>> {
    let workspace = tx.workspaces().get(session.workspace).await?;
    let Some(profile) = workspace.profile else {
        return Ok(None);
    };
    check(
        tx,
        &profile,
        &workspace.organization,
        &session.agent.harness,
        serialized,
        Some(session.id),
    )
    .await
}

/// The caller commits the hold with its other writes and releases only after refreshed
/// material has been handed back.
pub async fn acquire(
    tx: &mut Tx<'_>,
    organization: &str,
    profile: &str,
    harness: &str,
    holder: &str,
) -> Result<CredentialUse> {
    let organization = tx.organizations().named(organization).await?;
    let profile = tx.profiles().named(&organization, profile).await?;
    let serialized = tx
        .queue()
        .recorded()
        .await?
        .map_or_else(Vec::new, |recorded| recorded.serialized_harnesses);
    if let Some(busy) = check(tx, &profile, &organization, harness, &serialized, None).await? {
        bail!(Reason::SignInBusy(busy));
    }
    let held = CredentialUse {
        profile: profile.id,
        harness: harness.to_owned(),
        holder: holder.to_owned(),
    };
    tx.sign_ins().hold(&profile, &held).await?;
    Ok(held)
}

pub async fn release(store: &Store, held: &CredentialUse) -> Result<()> {
    let mut tx = store.begin().await?;
    tx.sign_ins().release(held).await?;
    tx.commit().await
}

/// No holder outlives the process it ran in.
pub async fn release_all(store: &Store) -> Result<()> {
    let mut tx = store.begin().await?;
    tx.sign_ins().release_all().await?;
    tx.commit().await
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) async fn session(tx: &mut Tx<'_>, harness: &str) -> Session {
        let organization = tx
            .organizations()
            .declare("acme", None)
            .await
            .unwrap()
            .record;
        let profile = tx
            .profiles()
            .declare(&organization, "jack", "Jack", None)
            .await
            .unwrap()
            .record;
        let project = tx
            .projects()
            .declare(&organization, "project", &[], &[], "main")
            .await
            .unwrap()
            .record;
        let agent = tx
            .agents()
            .declare(
                &organization,
                harness,
                harness,
                &crate::domain::Declared::default(),
            )
            .await
            .unwrap()
            .record;
        let workspace = tx
            .workspaces()
            .open(crate::store::workspace::Opening {
                organization: &organization,
                project: &project,
                agent: &agent,
                profile: Some(&profile),
                branch: None,
                correlation: None,
                continues: None,
                started_by: None,
            })
            .await
            .unwrap();
        tx.log()
            .append(
                &workspace,
                crate::log::Entry::Brief {
                    source: crate::log::BriefSource::Operator { participant: None },
                    brief: "work".to_owned(),
                },
            )
            .await
            .unwrap();
        tx.workspaces()
            .enqueue_session(&workspace, None, crate::domain::Declared::default(), None)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn release_then_reacquire_and_release_all() {
        let directory = tempfile::tempdir().unwrap();
        let store = Store::open(directory.path()).await.unwrap();
        let mut tx = store.begin().await.unwrap();
        session(&mut tx, "rotating").await;
        let held = acquire(&mut tx, "acme", "jack", "rotating", "probe")
            .await
            .unwrap();
        tx.commit().await.unwrap();
        release(&store, &held).await.unwrap();
        let mut tx = store.begin().await.unwrap();
        let next = acquire(&mut tx, "acme", "jack", "rotating", "other")
            .await
            .unwrap();
        tx.commit().await.unwrap();
        release(&store, &held).await.unwrap();
        let mut tx = store.begin().await.unwrap();
        assert!(
            acquire(&mut tx, "acme", "jack", "rotating", "third")
                .await
                .is_err()
        );
        drop(tx);
        release_all(&store).await.unwrap();
        let mut tx = store.begin().await.unwrap();
        assert!(
            acquire(&mut tx, "acme", "jack", "rotating", "third")
                .await
                .is_ok()
        );
        drop(tx);
        release(&store, &next).await.unwrap();
    }

    #[tokio::test]
    async fn serialized_session_and_acquire_return_the_same_holder() {
        let directory = tempfile::tempdir().unwrap();
        let store = Store::open(directory.path()).await.unwrap();
        let mut tx = store.begin().await.unwrap();
        let serialized = vec!["rotating".to_owned()];
        tx.queue()
            .record(2, &serialized, "local-exec")
            .await
            .unwrap();
        let first = session(&mut tx, "rotating").await;
        let first = tx
            .workspaces()
            .claim_session(&first, store.clock().now() + crate::work::LEASE, false)
            .await
            .unwrap()
            .unwrap();
        let second = session(&mut tx, "rotating").await;
        let checked = check_session(&mut tx, &second, &serialized)
            .await
            .unwrap()
            .unwrap();
        let refused = acquire(&mut tx, "acme", "jack", "rotating", "probe")
            .await
            .unwrap_err();
        let Some(Reason::SignInBusy(busy)) = refused.downcast_ref::<Reason>() else {
            panic!("{refused:?}");
        };
        assert_eq!(busy, &checked);
        assert!(
            matches!(&busy.holder, Holder::Session { session, workspace }
            if session == &first.name && !workspace.is_empty())
        );
        assert!(
            check_session(&mut tx, &first, &serialized)
                .await
                .unwrap()
                .is_none()
        );
        tx.commit().await.unwrap();
        let mut tx = store.begin().await.unwrap();
        let pending = session(&mut tx, "parallel").await;
        tx.workspaces()
            .claim_session(&pending, store.clock().now() + crate::work::LEASE, false)
            .await
            .unwrap()
            .unwrap();
        let second = session(&mut tx, "parallel").await;
        assert!(
            check_session(&mut tx, &second, &serialized)
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            acquire(&mut tx, "acme", "jack", "parallel", "probe")
                .await
                .is_ok()
        );
        assert!(
            check_session(&mut tx, &second, &serialized)
                .await
                .unwrap()
                .is_none()
        );
        assert!(matches!(
            acquire(&mut tx, "acme", "jack", "parallel", "other")
                .await
                .unwrap_err()
                .downcast_ref::<Reason>(),
            Some(Reason::SignInBusy(_))
        ));
    }

    #[tokio::test]
    async fn check_does_not_hold_and_acquire_refuses_a_lost_race() {
        let directory = tempfile::tempdir().unwrap();
        let store = Store::open(directory.path()).await.unwrap();
        let mut tx = store.begin().await.unwrap();
        let pending = session(&mut tx, "rotating").await;
        let serialized = vec!["rotating".to_owned()];
        tx.queue()
            .record(2, &serialized, "local-exec")
            .await
            .unwrap();
        assert!(
            check_session(&mut tx, &pending, &serialized)
                .await
                .unwrap()
                .is_none()
        );
        tx.commit().await.unwrap();
        let mut tx = store.begin().await.unwrap();
        acquire(&mut tx, "acme", "jack", "rotating", "winner")
            .await
            .unwrap();
        tx.commit().await.unwrap();
        let mut tx = store.begin().await.unwrap();
        let checked = check_session(&mut tx, &pending, &serialized)
            .await
            .unwrap()
            .unwrap();
        let error = acquire(&mut tx, "acme", "jack", "rotating", "loser")
            .await
            .unwrap_err();
        assert!(
            matches!(error.downcast_ref::<Reason>(), Some(Reason::SignInBusy(busy)) if busy == &checked)
        );
    }

    #[tokio::test]
    async fn double_acquire_refuses_with_holder() {
        let directory = tempfile::tempdir().unwrap();
        let store = Store::open(directory.path()).await.unwrap();
        let mut tx = store.begin().await.unwrap();
        let organization = tx
            .organizations()
            .declare("acme", None)
            .await
            .unwrap()
            .record;
        tx.profiles()
            .declare(&organization, "jack", "Jack", None)
            .await
            .unwrap();
        tx.queue()
            .record(1, &["rotating".to_owned()], "local-exec")
            .await
            .unwrap();
        let held = acquire(&mut tx, "acme", "jack", "rotating", "probe")
            .await
            .unwrap();
        tx.commit().await.unwrap();
        let mut tx = store.begin().await.unwrap();
        let refused = acquire(&mut tx, "acme", "jack", "rotating", "other")
            .await
            .unwrap_err();
        assert!(
            matches!(refused.downcast_ref::<Reason>(), Some(Reason::SignInBusy(busy))
            if busy.holder == Holder::CredentialUse("probe".to_owned()))
        );
        drop(tx);
        release(&store, &held).await.unwrap();
    }
}
