//! Subscription Profile fixtures: a person's login held beside the Provider Credentials, and the
//! Sessions that reach a model through it.

use kestrel::domain::{Session, SessionState, Workspace};
use kestrel::profile::Entry;
use kestrel_scripted_agent::{LOGIN, Script};

use super::fixture::Fixture;
use super::{Kestrel, scripted_agent, supervisor};

/// Confided by the scripted agent, because it carries the prefix `Confides` says out loud.
pub const SUBSCRIPTION_KEY: &str = "SCRIPTED_SUBSCRIPTION_KEY";
pub const JACKS_KEY: &str = "jacks-subscription-key";
pub const ALEXS_KEY: &str = "alexs-subscription-key";
pub const FIRST_LOGIN: &str = "a-first-login";

pub async fn playing(script: Script) -> Kestrel {
    Kestrel::dispatching_to(supervisor::binary(), &scripted_agent::playing(script)).await
}

/// An Organization holding no Provider Credential, so a model is reached through a profile or
/// not at all.
pub async fn declared(kestrel: &Kestrel, harness: &str) {
    Fixture::acme()
        .checked_out()
        .harness(harness)
        .declare(kestrel)
        .await;
}

pub async fn a_profile(kestrel: &Kestrel, name: &str, owner: &str, entry: &Entry, login: &str) {
    kestrel
        .declare_profile("acme", name, owner)
        .await
        .expect("the profile should declare");
    kestrel.hold_in_profile("acme", name, entry, login).await;
}

pub fn subscription_key() -> Entry {
    Entry::variable(SUBSCRIPTION_KEY).expect("a variable")
}

pub fn login_file() -> Entry {
    Entry::file(LOGIN).expect("a file")
}

pub async fn transcript(kestrel: &Kestrel, workspace: &Workspace) -> String {
    kestrel
        .transcript(workspace.id)
        .await
        .iter()
        .map(|entry| entry.entry.to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Only an explicit stop, a sealed Workspace, or a failure ends a Session (ADR-0024): a Session
/// whose agent answered stays open between turns until this stops it, and one that already failed
/// before an agent ever answered is left as it ended.
pub async fn worked(kestrel: &Kestrel, workspace: &Workspace) -> (Session, String) {
    let session = kestrel.enqueue_session(workspace.id).await;
    finished(kestrel, workspace, session).await
}

pub async fn finished(
    kestrel: &Kestrel,
    workspace: &Workspace,
    session: Session,
) -> (Session, String) {
    let mut session = kestrel.answered(session.id, 1).await;
    if session.state != SessionState::Ended {
        kestrel.stop_session(session.id).await;
        session = kestrel.session(session.id).await;
        // The Session ends in the database the moment it is told to stop; what its supervisor holds
        // of the profile is only gone once the supervisor has let the Session go.
        let instance = session
            .instance
            .as_deref()
            .and_then(|instance| instance.strip_prefix("local-exec/"))
            .expect("a local instance");
        let login = std::env::temp_dir()
            .join(format!("{instance}.home"))
            .join(LOGIN);
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
        while login.exists() {
            assert!(
                tokio::time::Instant::now() < deadline,
                "the login outlived its session"
            );
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    }

    (session, transcript(kestrel, workspace).await)
}
