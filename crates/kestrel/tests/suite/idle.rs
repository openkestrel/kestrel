//! The idle window: a Workspace that finished its work and then sat idle for a day seals
//! itself, so a backlog of finished work stops piling up as Workspaces nobody ever closes.

use crate::support;

use std::time::Duration;

use kestrel::domain::{Workspace, WorkspaceState};
use kestrel::log::Window;
use support::Kestrel;
use support::fixture::Fixture;

const WELL_INSIDE_THE_WINDOW: Duration = Duration::from_secs(23 * 60 * 60);

async fn a_workspace(kestrel: &Kestrel) -> Workspace {
    Fixture::acme().model("claude-opus-5").open(kestrel).await
}

/// Too long to step a pass at a time, so the control plane was away for it. The pass first gives
/// the sweep a time to measure the gap from, so a lease the day outlasts is graced, not expired.
async fn a_day_passes(kestrel: &Kestrel) {
    kestrel.work_pass().await;
    kestrel.away_for(Duration::from_secs(25 * 60 * 60));
}

async fn sealed_by_the_sweep(kestrel: &Kestrel, workspace: &Workspace) -> Workspace {
    kestrel.work_pass().await;

    let shown = kestrel.show_workspace(workspace.id).await;
    assert_eq!(
        shown.state,
        WorkspaceState::Sealed,
        "the workspace {} was last active at {} and never sealed itself",
        shown.id,
        shown.last_active_at
    );
    shown
}

async fn stays_open(kestrel: &Kestrel, workspace: &Workspace) {
    kestrel.work_pass().await;

    assert_eq!(
        kestrel.show_workspace(workspace.id).await.state,
        WorkspaceState::Open,
        "the workspace {} sealed itself while something was still holding it",
        workspace.id
    );
}

#[tokio::test]
async fn a_workspace_is_last_active_when_it_opens() {
    let kestrel = Kestrel::boot().await;
    let workspace = a_workspace(&kestrel).await;

    assert_eq!(workspace.last_active_at, workspace.opened_at);
    assert_eq!(
        kestrel.show_workspace(workspace.id).await.last_active_at,
        workspace.opened_at
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn enqueueing_a_session_into_a_workspace_records_it_active() {
    let kestrel = Kestrel::stepped().await;
    let workspace = a_workspace(&kestrel).await;
    kestrel.away_for(WELL_INSIDE_THE_WINDOW);

    kestrel.enqueue_session(workspace.id).await;

    assert_eq!(
        kestrel.show_workspace(workspace.id).await.last_active_at,
        kestrel.now(),
        "a workspace that took a session is still last active when it opened"
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_session_ending_records_its_workspace_active() {
    let kestrel = Kestrel::stepped().await;
    let workspace = a_workspace(&kestrel).await;
    let session = kestrel.dispatch_session(workspace.id).await;
    a_day_passes(&kestrel).await;

    kestrel.complete_session(&session).await;

    assert_eq!(
        kestrel.show_workspace(workspace.id).await.last_active_at,
        kestrel.now(),
        "a workspace whose session ended is still last active when its session began"
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_workspace_whose_sessions_have_all_ended_seals_itself_once_the_window_elapses() {
    let kestrel = Kestrel::stepped().await;
    let workspace = a_workspace(&kestrel).await;
    let session = kestrel.dispatch_session(workspace.id).await;
    kestrel.complete_session(&session).await;

    a_day_passes(&kestrel).await;

    let sealed = sealed_by_the_sweep(&kestrel, &workspace).await;
    assert!(sealed.sealed_at.is_some());

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_workspace_that_never_ran_anything_seals_itself_once_the_window_elapses() {
    let kestrel = Kestrel::stepped().await;
    let workspace = a_workspace(&kestrel).await;

    a_day_passes(&kestrel).await;

    sealed_by_the_sweep(&kestrel, &workspace).await;

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_workspace_with_a_session_holding_its_slot_never_seals_however_old_it_is() {
    let kestrel = Kestrel::stepped().await;
    let workspace = a_workspace(&kestrel).await;
    let session = kestrel.dispatch_session(workspace.id).await;

    a_day_passes(&kestrel).await;

    stays_open(&kestrel, &workspace).await;

    kestrel.complete_session(&session).await;
    a_day_passes(&kestrel).await;
    sealed_by_the_sweep(&kestrel, &workspace).await;

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_workspace_with_messages_waiting_on_a_busy_session_never_seals() {
    let kestrel = Kestrel::stepped().await;
    let workspace = a_workspace(&kestrel).await;
    let session = kestrel.dispatch_session(workspace.id).await;
    assert!(
        kestrel
            .post_while_busy(workspace.id, "jack", "one more thing")
            .await
            .is_none(),
        "a message posted while a session was busy enqueued a session of its own"
    );
    kestrel.complete_session(&session).await;

    a_day_passes(&kestrel).await;

    stays_open(&kestrel, &workspace).await;

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_workspace_the_sweep_sealed_is_readable_refuses_work_and_is_never_reopened() {
    let kestrel = Kestrel::stepped().await;
    let workspace = a_workspace(&kestrel).await;
    let session = kestrel.dispatch_session(workspace.id).await;
    kestrel.said(&session, "what it did").await;
    kestrel.complete_session(&session).await;
    let before = kestrel.transcript(workspace.id).await;

    a_day_passes(&kestrel).await;
    sealed_by_the_sweep(&kestrel, &workspace).await;

    let walked: Vec<String> = kestrel
        .walk(workspace.id, None, Window::of(1).expect("a window"))
        .await
        .iter()
        .map(|entry| entry.entry.to_string())
        .collect();
    assert_eq!(
        walked,
        before
            .iter()
            .map(|entry| entry.entry.to_string())
            .collect::<Vec<_>>()
    );
    assert!(
        kestrel.try_enqueue_session(workspace.id).await.is_err(),
        "a workspace the sweep sealed took a new session"
    );
    assert!(
        kestrel
            .try_start(&session, support::harness())
            .await
            .is_err(),
        "a workspace the sweep sealed took a turn"
    );
    assert!(
        kestrel
            .try_seal_workspace(workspace.id)
            .await
            .expect_err("a sealed workspace is never reopened")
            .to_string()
            .contains("already sealed")
    );

    kestrel.teardown().await;
}

#[tokio::test]
async fn the_window_is_a_day() {
    let kestrel = Kestrel::stepped().await;
    let workspace = a_workspace(&kestrel).await;

    kestrel.away_for(WELL_INSIDE_THE_WINDOW);
    stays_open(&kestrel, &workspace).await;

    kestrel.away_for(Duration::from_secs(2 * 60 * 60));
    sealed_by_the_sweep(&kestrel, &workspace).await;

    kestrel.teardown().await;
}

#[tokio::test]
async fn a_workspace_idle_for_a_day_still_seals_after_the_control_plane_restarts() {
    let kestrel = Kestrel::stepped().await;
    let workspace = a_workspace(&kestrel).await;
    a_day_passes(&kestrel).await;

    let kestrel = kestrel.kill_and_restart().await;

    sealed_by_the_sweep(&kestrel, &workspace).await;

    kestrel.teardown().await;
}
