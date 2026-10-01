//! The Integration's outbound direction: a completed Turn's response, and a Session's own final
//! Outcome when it adds something the Turn responses did not, said back where the work came
//! from (ADR-0024). kestrel relays what its Agent said and reasons about no git of its own, so
//! the pull request a comment points at is there because the Agent named it.

use anyhow::Result;
use jiff::Timestamp;
use tracing::warn;

use crate::domain::{
    Delivery, Direction, Event, Exit, Integration, Session, SessionId, StartedBy, Workspace,
};
use crate::integration::back_off;
use crate::integration::github::{Github, MARKER, Refused};
use crate::store::{Store, Tx};

/// Invisible where GitHub renders it, and the whole of how a delivery that never learned whether
/// its comment landed recognises its own. The Turn names which of a Session's messages it is.
fn marker(session: SessionId, turn: Option<i64>) -> String {
    match turn {
        Some(turn) => format!("{MARKER}{session} turn {turn} -->"),
        None => format!("{MARKER}{session} -->"),
    }
}

/// The surface a Workspace came in through, when it came in through one that carries outbound.
async fn surface(tx: &mut Tx<'_>, workspace: &Workspace) -> Result<Option<(Integration, Event)>> {
    let Some(StartedBy::Event(started_by)) = workspace.started_by.clone() else {
        return Ok(None);
    };

    let event = tx.integrations().event(started_by).await?;
    let Some(integration) = event.integration else {
        return Ok(None);
    };
    let integration = tx.integrations().with_id(integration).await?;
    if !integration.carries(Direction::Outbound) {
        warn!(
            integration = integration.name,
            "a workspace came in through an integration that carries nothing outbound, so what it \
             says reaches nobody"
        );
        return Ok(None);
    }

    Ok(Some((integration, event)))
}

/// Recorded in the transaction that answers a Turn, so a Turn once over has a response waiting
/// to be posted. A Turn that produced no message has nothing to say and records nothing.
pub(crate) async fn record_turn(
    tx: &mut Tx<'_>,
    session: &Session,
    workspace: &Workspace,
    turn: i64,
    said: &[String],
) -> Result<()> {
    let Some((integration, event)) = surface(tx, workspace).await? else {
        return Ok(());
    };

    let body = format!(
        "{}\n\n{}\n",
        said.join("\n\n"),
        marker(session.id, Some(turn))
    );
    tx.integrations()
        .record_delivery(session, &integration, &event, Some(turn), &body, Some(said))
        .await
}

pub(crate) async fn record_outcome(
    tx: &mut Tx<'_>,
    session: &Session,
    workspace: &Workspace,
    exit: &Exit,
    said: Option<&str>,
) -> Result<()> {
    if matches!(exit, Exit::Succeeded) {
        let responses = tx.integrations().turn_responses(session.id).await?;
        if !responses.is_empty()
            && said.is_none_or(|said| {
                responses.iter().any(|messages| {
                    messages.iter().any(|message| message == said) || messages.join("\n\n") == said
                })
            })
        {
            return Ok(());
        }
    }
    let Some((integration, event)) = surface(tx, workspace).await? else {
        return Ok(());
    };

    let body = body(workspace, session, exit, said);

    tx.integrations()
        .record_delivery(session, &integration, &event, None, &body, None)
        .await
}

/// One delivery attempt. What comes back is where the comment landed, or nothing — a refusal
/// defers it rather than failing anything, because the Turn is already over and nothing said
/// afterwards changes it.
pub async fn deliver(
    store: &Store,
    github: &Github,
    delivery: &Delivery,
) -> Result<Option<String>> {
    let integration = {
        let mut tx = store.begin().await?;
        tx.integrations().with_id(delivery.integration).await?
    };
    let marker = marker(delivery.session, delivery.turn);

    // An earlier attempt went out and never came back, so a comment may already be there.
    if let Some(attempted_at) = delivery.attempted_at {
        match github
            .comment_carrying(&integration, delivery.subject, &marker, attempted_at)
            .await
        {
            Ok(Some(already)) => return delivered(store, delivery, &already.html_url).await,
            Ok(None) => {}
            Err(refused) => return deferred(store, delivery, &integration, &refused).await,
        }
    }

    let mut tx = store.begin().await?;
    tx.integrations()
        .attempting_delivery(delivery, Timestamp::now())
        .await?;
    tx.commit().await?;

    match github
        .comment(&integration, delivery.subject, &delivery.body)
        .await
    {
        Ok(comment) => delivered(store, delivery, &comment.html_url).await,
        Err(refused) => deferred(store, delivery, &integration, &refused).await,
    }
}

async fn delivered(store: &Store, delivery: &Delivery, to: &str) -> Result<Option<String>> {
    let mut tx = store.begin().await?;
    tx.integrations().delivery_delivered(delivery, to).await?;
    tx.commit().await?;

    Ok(Some(to.to_owned()))
}

async fn deferred(
    store: &Store,
    delivery: &Delivery,
    integration: &Integration,
    refused: &Refused,
) -> Result<Option<String>> {
    warn!(
        session = %delivery.session,
        turn = delivery.turn,
        integration = integration.name,
        because = %refused,
        "what a session said could not be said back on the issue it came from"
    );

    let mut tx = store.begin().await?;
    tx.integrations()
        .delivery_deferred(delivery, back_off(integration.github()?.interval, refused))
        .await?;
    tx.commit().await?;

    Ok(None)
}

fn body(workspace: &Workspace, session: &Session, exit: &Exit, said: Option<&str>) -> String {
    let mut body = format!("**kestrel** — session {exit}\n");

    if let Some(message) = said.map(str::trim).filter(|said| !said.is_empty()) {
        body.push('\n');
        for line in message.lines() {
            match line.trim().is_empty() {
                true => body.push_str(">\n"),
                false => body.push_str(&format!("> {line}\n")),
            }
        }
    }

    body.push_str(&format!(
        "\nWorkspace `{}` · session `{}`\n{}\n",
        workspace.id,
        session.id,
        marker(session.id, None)
    ));

    body
}

#[cfg(test)]
mod tests {
    use jiff::Timestamp;

    use super::*;
    use crate::domain::{
        Agent, AgentId, Checkout, Organization, OrganizationId, Project, ProjectId, SessionState,
        WorkspaceId, WorkspaceState,
    };

    fn a_workspace() -> Workspace {
        let organization = Organization {
            id: OrganizationId::generate(),
            name: "acme".to_owned(),
            max_live_instances: None,
        };

        Workspace {
            id: WorkspaceId::generate(),
            name: "bright-falcon".to_owned(),
            project: Project {
                id: ProjectId::generate(),
                organization: organization.id,
                name: "kestrel".to_owned(),
                repositories: Vec::new(),
                branch: "main".to_owned(),
            },
            opened_with: an_agent(organization.id),
            profile: None,
            organization,
            checkout: Checkout {
                repositories: Vec::new(),
                base: "main".to_owned(),
                branch: "main".to_owned(),
            },
            correlation: None,
            state: WorkspaceState::Open,
            opened_at: Timestamp::now(),
            last_active_at: Timestamp::now(),
            sealed_at: None,
            continues: None,
            started_by: None,
        }
    }

    fn an_agent(organization: OrganizationId) -> Agent {
        Agent {
            id: AgentId::generate(),
            organization,
            name: "builder".to_owned(),
            harness: "opencode".to_owned(),
            model: None,
        }
    }

    fn a_session(workspace: &Workspace) -> Session {
        Session {
            id: SessionId::generate(),
            name: "quiet-river".to_owned(),
            organization: workspace.organization.id,
            workspace: workspace.id,
            agent: workspace.opened_with.clone(),
            state: SessionState::Ended,
            exit: None,
            outcome_message: None,
            instance: None,
            supervisor: None,
            worked_model: None,
            enqueued_at: Timestamp::now(),
            started_at: None,
            ended_at: None,
            lease_expires_at: None,
            connected: None,
            usage: None,
        }
    }

    #[test]
    fn what_the_agent_said_last_is_quoted_under_the_exit_status() {
        let workspace = a_workspace();
        let session = a_session(&workspace);

        let body = body(
            &workspace,
            &session,
            &Exit::Succeeded,
            Some("Opened https://github.com/jtmthf/kestrel/pull/92.\n\nIt has a test."),
        );

        assert!(body.starts_with("**kestrel** — session succeeded\n"));
        assert!(body.contains(
            "> Opened https://github.com/jtmthf/kestrel/pull/92.\n>\n> It has a test.\n"
        ));
        assert!(body.contains(&workspace.id.to_string()));
        assert!(body.ends_with(&format!("{}\n", marker(session.id, None))));
    }

    #[test]
    fn a_session_that_failed_says_so_and_says_why() {
        let workspace = a_workspace();
        let session = a_session(&workspace);

        let body = body(
            &workspace,
            &session,
            &Exit::Failed {
                because: "the environment could not be provisioned".to_owned(),
            },
            None,
        );

        assert!(body.contains("session failed: the environment could not be provisioned"));
    }

    #[test]
    fn an_agent_that_said_nothing_leaves_no_empty_quote() {
        let workspace = a_workspace();
        let session = a_session(&workspace);

        let body = body(&workspace, &session, &Exit::Succeeded, Some("   "));

        assert!(!body.lines().any(|line| line.starts_with('>')));
    }

    #[test]
    fn a_turns_marker_names_the_session_and_the_turn() {
        let session = SessionId::generate();

        assert_eq!(
            marker(session, Some(3)),
            format!("{MARKER}{session} turn 3 -->")
        );
        assert_eq!(marker(session, None), format!("{MARKER}{session} -->"));
        assert_ne!(marker(session, Some(1)), marker(session, Some(2)));
    }
}
