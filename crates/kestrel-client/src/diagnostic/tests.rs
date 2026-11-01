use reqwest::StatusCode;

use crate::diagnostic::answer::refused;
use crate::diagnostic::kind::exit_for;

use super::*;

pub fn invocation() -> Invocation {
    Invocation {
        control_plane: "http://127.0.0.1:7718".to_owned(),
        elsewhere: false,
        operation: "workspace open".to_owned(),
        args: [
            "workspace",
            "open",
            "--project",
            "absent",
            "--agent",
            "worker",
        ]
        .map(str::to_owned)
        .to_vec(),
        json: false,
        verbose: false,
    }
}

pub fn diagnostic(value: serde_json::Value) -> wire::Diagnostic {
    serde_json::from_value(value).expect("a diagnostic")
}

pub fn missing_project() -> wire::Diagnostic {
    diagnostic(json!({
        "kind": "missing_reference",
        "message": "no project named absent in the organization Acme East",
        "field": null,
        "context": { "resource": "project", "reference": "absent", "organization": "Acme East" },
        "next_steps": [{
            "action": "declare_project",
            "organization": "Acme East",
            "name": "absent",
            "repositories": null,
            "branch": null,
            "missing": ["repositories", "branch"],
        }],
    }))
}

pub fn stopping() -> wire::Diagnostic {
    diagnostic(json!({
        "kind": "state_conflict",
        "message": "the workspace w already has the session s",
        "field": null,
        "context": {
            "operation": "enqueue_session",
            "resource": "workspace",
            "reference": "w",
            "organization": "acme",
            "state": "session_in_flight",
            "holding_session": "s",
        },
        "next_steps": [
            {
                "action": "inspect_resource",
                "resource": "session",
                "reference": "s",
                "organization": "acme",
            },
            {
                "action": "stop_session",
                "organization": "acme",
                "session": "s",
                "consequence": "Stopping the session now records it failed.",
                "effect": "fails_session",
                "requires_choice": true,
            },
        ],
    }))
}

pub fn rendered(diagnostic: &wire::Diagnostic, invocation: &Invocation, wrote: bool) -> String {
    step::described(
        diagnostic,
        &step::steps(diagnostic, invocation, wrote),
        invocation,
    )
}

const READ: Request<'static> = Request {
    operation: "session show",
    write: false,
};
const WRITE: Request<'static> = Request {
    operation: "session stop",
    write: true,
};

#[test]
fn the_kind_decides_the_exit_whatever_the_status() {
    let cases = [
        (missing_project(), StatusCode::CONFLICT, Exit::Unresolved),
        (stopping(), StatusCode::NOT_FOUND, Exit::Rejected),
        (
            diagnostic(json!({
                "kind": "setup_gap",
                "message": "name the Operator",
                "field": null,
                "context": {
                    "prerequisite": "operator",
                    "resource": null,
                    "reference": null,
                    "organization": null,
                    "harness": null,
                    "method": null,
                    "sign_in": null,
                },
                "next_steps": [{ "action": "name_operator", "name": null, "missing": ["name"] }],
            })),
            StatusCode::CONFLICT,
            Exit::NotReady,
        ),
        (
            diagnostic(json!({
                "kind": "credential_rejected",
                "message": "anthropic refused the key",
                "field": null,
                "context": {
                    "organization": "acme",
                    "harness": "claude-code",
                    "method": "anthropic-api-key",
                    "provider": "anthropic",
                    "status": 401,
                    "provider_error": "authentication_error",
                },
                "next_steps": [],
            })),
            StatusCode::UNPROCESSABLE_ENTITY,
            Exit::Rejected,
        ),
    ];

    for (diagnostic, status, exit) in cases {
        assert_eq!(exit_for(&diagnostic, status), exit);
    }
}

#[test]
fn rewording_a_refusal_changes_nothing_but_its_sentence() {
    let mut reworded = serde_json::to_value(missing_project()).expect("json");
    reworded["message"] = json!("there is no such Project as the stop session thing");
    let reworded = diagnostic(reworded);

    assert_eq!(
        exit_for(&reworded, StatusCode::NOT_FOUND),
        exit_for(&missing_project(), StatusCode::NOT_FOUND)
    );
    assert_eq!(
        rendered(&reworded, &invocation(), false),
        rendered(&missing_project(), &invocation(), false)
    );
}

#[test]
fn an_unrecognised_answer_is_bounded_evidence_with_an_inspection() {
    let failed = refused(
        StatusCode::BAD_GATEWAY,
        None,
        format!("<html>\u{1b}[31m{}</html>", "x".repeat(1000)).as_bytes(),
        WRITE,
    );

    assert_eq!(failed.exit(), Exit::Unavailable);
    let said = rendered(
        failed.diagnostic().expect("a diagnostic"),
        &invocation(),
        true,
    );
    assert!(said.contains("may already have taken effect"), "{said}");
    assert!(!said.contains('\u{1b}'), "{said}");
    assert!(said.len() < 600, "{said}");
}

#[test]
fn a_lost_read_offers_a_retry_with_its_delay_and_a_lost_write_never_does() {
    let read = refused(StatusCode::SERVICE_UNAVAILABLE, Some(3), b"", READ);
    let said = rendered(
        read.diagnostic().expect("a diagnostic"),
        &invocation(),
        false,
    );
    assert!(said.contains("read again after 3 seconds"), "{said}");

    let write = unreachable("http://127.0.0.1:7718/", WRITE, true);
    let said = rendered(
        write.diagnostic().expect("a diagnostic"),
        &invocation(),
        true,
    );
    assert!(!said.contains("read again"), "{said}");
    assert!(said.contains("may already have taken effect"), "{said}");
    assert!(
        said.contains("at http://127.0.0.1:7718 is running"),
        "{said}"
    );
}

#[test]
fn a_retry_after_a_write_is_said_and_never_offered_to_run() {
    let read = refused(StatusCode::SERVICE_UNAVAILABLE, None, b"", READ);

    let said = rendered(
        read.diagnostic().expect("a diagnostic"),
        &invocation(),
        true,
    );

    assert!(said.contains("read again"), "{said}");
    assert!(!said.contains("kestrel workspace open"), "{said}");
}

#[test]
fn a_plain_refusal_keeps_its_sentence_and_its_status() {
    let failed = refused(
        StatusCode::CONFLICT,
        None,
        br#"{"message":"that name is taken"}"#,
        WRITE,
    );

    assert_eq!(failed.exit(), Exit::Rejected);
    assert_eq!(
        message(failed.diagnostic().expect("a diagnostic")),
        "that name is taken"
    );
}

#[test]
fn an_unread_answer_is_not_a_connection_failure() {
    let error = anyhow::anyhow!(serde_json::Error::io(std::io::Error::other("eof"))).context(
        Failed::new(Exit::Unavailable, "reading the applied triggers"),
    );

    let diagnostic = fallback(&error, Exit::Unavailable, &invocation(), false);

    let value = serde_json::to_value(&diagnostic).expect("json");
    assert_eq!(value["kind"], "unknown_response");
    assert_eq!(value["message"], "reading the applied triggers");
}

#[test]
fn a_local_failure_before_any_write_cannot_have_taken_effect() {
    let error = anyhow::anyhow!("writing to standard output");

    let before = rendered(
        &fallback(&error, Exit::Failure, &invocation(), false),
        &invocation(),
        false,
    );
    let after = rendered(
        &fallback(&error, Exit::Failure, &invocation(), true),
        &invocation(),
        true,
    );

    assert!(
        !before.contains("may already have taken effect"),
        "{before}"
    );
    assert!(after.contains("may already have taken effect"), "{after}");
}
