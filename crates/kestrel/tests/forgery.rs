mod support;

use kestrel::domain::{SessionState, TriggerState, WorkspaceState};
use reqwest::StatusCode;
use reqwest::header::{HOST, ORIGIN};
use support::Kestrel;

const ELSEWHERE: &str = "https://evil.example";

async fn an_idle_workspace_a_working_session_and_triggers(
    kestrel: &Kestrel,
) -> (kestrel::domain::Workspace, kestrel::domain::Session) {
    let organization = kestrel.declare_organization("acme").await;
    kestrel
        .declare_project(
            &organization,
            "kestrel",
            &["https://github.com/jtmthf/kestrel".to_owned()],
            "main",
        )
        .await;
    kestrel
        .declare_agent(&organization, "builder", "opencode", None)
        .await;
    for name in ["enabled", "disabled"] {
        kestrel
            .declare_trigger(
                "acme",
                name,
                r#"{"exact": {"type": "com.github.issues.labeled"}}"#,
                "kestrel",
                "builder",
            )
            .await;
    }
    kestrel.disable_trigger("acme", "disabled").await;
    let working = kestrel.open_workspace("acme", "kestrel", "builder").await;
    let session = kestrel.dispatch_session(working.id).await;
    let idle = kestrel.open_workspace("acme", "kestrel", "builder").await;

    (idle, session)
}

async fn posted_from(kestrel: &Kestrel, path: &str, origin: &str) -> (StatusCode, String) {
    let response = reqwest::Client::new()
        .post(format!("{}{path}", kestrel.operator()))
        .header(ORIGIN, origin)
        .send()
        .await
        .expect("the operator boundary should answer");

    (response.status(), response.text().await.expect("an answer"))
}

async fn got(kestrel: &Kestrel, host: Option<&str>, origin: Option<&str>) -> StatusCode {
    let mut request = reqwest::Client::new().get(format!(
        "{}/operator/organizations/acme/workspaces",
        kestrel.operator()
    ));
    if let Some(host) = host {
        request = request.header(HOST, host);
    }
    if let Some(origin) = origin {
        request = request.header(ORIGIN, origin);
    }

    request
        .send()
        .await
        .expect("the operator boundary should answer")
        .status()
}

#[tokio::test]
async fn a_page_elsewhere_changes_nothing_with_a_bodiless_post() {
    let kestrel = Kestrel::boot().await;
    let (workspace, session) = an_idle_workspace_a_working_session_and_triggers(&kestrel).await;

    for path in [
        format!(
            "/operator/organizations/acme/workspaces/{}/seal",
            workspace.id
        ),
        format!("/operator/organizations/acme/sessions/{}/stop", session.id),
        "/operator/organizations/acme/triggers/enabled/disable".to_owned(),
        "/operator/organizations/acme/triggers/disabled/enable".to_owned(),
    ] {
        let (status, body) = posted_from(&kestrel, &path, ELSEWHERE).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{path}: {body}");
        assert!(
            body.contains("origin"),
            "{path} refused without a reason: {body}"
        );
    }

    assert_eq!(
        kestrel.show_workspace(workspace.id).await.state,
        WorkspaceState::Open
    );
    assert_eq!(
        kestrel.session(session.id).await.state,
        SessionState::Working
    );
    assert_eq!(
        kestrel.show_trigger("acme", "enabled").await.state,
        TriggerState::Enabled
    );
    assert!(matches!(
        kestrel.show_trigger("acme", "disabled").await.state,
        TriggerState::Disabled(_)
    ));
}

#[tokio::test]
async fn a_rebound_name_reads_nothing() {
    let kestrel = Kestrel::boot().await;
    kestrel.declare_organization("acme").await;

    assert_eq!(
        got(
            &kestrel,
            Some(&format!("evil.example:{}", kestrel.operator_port())),
            None
        )
        .await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        got(&kestrel, Some("evil.example"), None).await,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn a_page_served_from_the_boundary_itself_is_answered() {
    let kestrel = Kestrel::boot().await;
    kestrel.declare_organization("acme").await;
    let port = kestrel.operator_port();

    for host in ["127.0.0.1", "localhost", "[::1]"] {
        let host = format!("{host}:{port}");
        assert_eq!(
            got(&kestrel, Some(&host), Some(&format!("http://{host}"))).await,
            StatusCode::OK,
            "{host}"
        );
    }
}

#[tokio::test]
async fn another_page_on_this_machine_is_elsewhere() {
    let kestrel = Kestrel::boot().await;
    kestrel.declare_organization("acme").await;
    let host = format!("127.0.0.1:{}", kestrel.operator_port());

    for origin in [
        "http://127.0.0.1:1",
        "http://localhost:3000",
        &format!("https://{host}"),
        "null",
    ] {
        assert_eq!(
            got(&kestrel, Some(&host), Some(origin)).await,
            StatusCode::FORBIDDEN,
            "{origin}"
        );
    }
}

#[tokio::test]
async fn a_client_that_names_no_origin_is_answered() {
    let kestrel = Kestrel::boot().await;
    kestrel.declare_organization("acme").await;

    assert_eq!(got(&kestrel, None, None).await, StatusCode::OK);
}

#[tokio::test]
async fn a_loopback_port_the_boundary_was_forwarded_from_is_answered() {
    let kestrel = Kestrel::boot().await;
    kestrel.declare_organization("acme").await;

    assert_eq!(
        got(
            &kestrel,
            Some("127.0.0.1:9000"),
            Some("http://127.0.0.1:9000")
        )
        .await,
        StatusCode::OK
    );
}

#[tokio::test]
async fn a_boundary_bound_beyond_loopback_answers_its_bound_address() {
    let kestrel = Kestrel::boot_with_the_operator_beyond_loopback().await;
    kestrel.declare_organization("acme").await;
    let host = format!("0.0.0.0:{}", kestrel.operator_port());

    assert_eq!(
        got(&kestrel, Some(&host), Some(&format!("http://{host}"))).await,
        StatusCode::OK
    );
}
