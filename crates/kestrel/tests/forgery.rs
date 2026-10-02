mod support;

use kestrel::domain::{SessionState, TriggerState, WorkspaceState};
use reqwest::StatusCode;
use reqwest::header::{HOST, ORIGIN};
use support::Kestrel;
use support::client;

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

async fn preflighted(kestrel: &Kestrel, path: &str, origin: &str) -> reqwest::Response {
    reqwest::Client::new()
        .request(
            reqwest::Method::OPTIONS,
            format!("{}{path}", kestrel.operator()),
        )
        .header(ORIGIN, origin)
        .header("access-control-request-method", "POST")
        .send()
        .await
        .expect("the operator boundary should answer")
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
async fn a_rebound_name_or_an_address_that_is_not_loopback_reads_nothing() {
    let kestrel = Kestrel::boot().await;
    kestrel.declare_organization("acme").await;
    let port = kestrel.operator_port();

    for host in [
        "evil.example".to_owned(),
        format!("evil.example:{port}"),
        format!("192.168.1.5:{port}"),
        format!("0.0.0.0:{port}"),
        "8.8.8.8".to_owned(),
    ] {
        assert_eq!(
            got(&kestrel, Some(&host), None).await,
            StatusCode::FORBIDDEN,
            "{host}"
        );
    }
}

#[tokio::test]
async fn a_page_served_from_the_boundary_itself_is_answered() {
    let kestrel = Kestrel::boot().await;
    kestrel.declare_organization("acme").await;
    let port = kestrel.operator_port();

    for host in ["127.0.0.1", "127.0.0.2", "localhost", "[::1]"] {
        let host = format!("{host}:{port}");
        for scheme in ["http", "https"] {
            assert_eq!(
                got(&kestrel, Some(&host), Some(&format!("{scheme}://{host}"))).await,
                StatusCode::OK,
                "{scheme}://{host}"
            );
        }
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
        "https://127.0.0.1:1",
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
async fn a_boundary_bound_beyond_loopback_still_answers_only_loopback() {
    let kestrel = Kestrel::boot_with_the_operator_beyond_loopback().await;
    kestrel.declare_organization("acme").await;
    let bound = format!("0.0.0.0:{}", kestrel.operator_port());
    let loopback = format!("127.0.0.1:{}", kestrel.operator_port());

    assert_eq!(
        got(&kestrel, Some(&bound), Some(&format!("http://{bound}"))).await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        got(
            &kestrel,
            Some(&loopback),
            Some(&format!("http://{loopback}"))
        )
        .await,
        StatusCode::OK
    );
}

#[tokio::test]
async fn a_cross_origin_preflight_is_granted_nothing() {
    let kestrel = Kestrel::boot().await;
    kestrel.declare_organization("acme").await;

    let preflight = preflighted(
        &kestrel,
        "/operator/organizations/acme/workspaces/latest/seal",
        ELSEWHERE,
    )
    .await;

    assert_eq!(preflight.status(), StatusCode::FORBIDDEN);
    assert!(
        !preflight
            .headers()
            .contains_key("access-control-allow-origin"),
        "a cross-origin preflight was granted access"
    );
}

#[tokio::test]
async fn a_page_served_from_the_boundary_itself_may_write() {
    let kestrel = Kestrel::boot().await;
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
    kestrel
        .declare_trigger(
            "acme",
            "enabled",
            r#"{"exact": {"type": "com.github.issues.labeled"}}"#,
            "kestrel",
            "builder",
        )
        .await;

    let here = format!("http://127.0.0.1:{}", kestrel.operator_port());
    let (status, body) = posted_from(
        &kestrel,
        "/operator/organizations/acme/triggers/enabled/disable",
        &here,
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(matches!(
        kestrel.show_trigger("acme", "enabled").await.state,
        TriggerState::Disabled(_)
    ));
}

#[tokio::test]
async fn a_cli_write_that_names_no_origin_is_answered() {
    let kestrel = Kestrel::boot().await;

    let written = client::ran_by(
        &kestrel,
        &["organization", "declare", "acme"],
        client::Invocation::default(),
    )
    .await;
    assert!(
        written.status.success(),
        "the CLI refused:\n{}",
        written.err
    );

    assert_eq!(kestrel.organizations().await.len(), 1);
}
