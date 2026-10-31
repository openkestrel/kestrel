use kestrel_operator_types as wire;
use serde_json::json;

use crate::diagnostic;
use crate::output::Presentation;
use crate::queue;

#[test]
fn queue_custody_reason_renders_the_diagnostics_repair() {
    let reason = json!({
        "kind": "subscription_profile", "profile": "jack", "session": "holding",
        "diagnostic": {
            "kind": "state_conflict", "message": "the sign-in is in use",
            "context": { "operation": "use_sign_in", "resource": "subscription_profile",
                "reference": "jack", "organization": "acme", "state": "credential_in_use", "holding_session": "holding" },
            "next_steps": [{ "action": "inspect_resource", "resource": "session", "reference": "holding", "organization": "acme" }]
        }
    });
    let reason: wire::QueueReason = serde_json::from_value(reason).unwrap();
    let rendered = queue::reason_in_words(
        &reason,
        &diagnostic::tests::invocation(),
        &Presentation::Delimited,
    );
    assert!(
        serde_json::from_value::<wire::QueueReason>(json!({ "kind": "subscription_profile" }))
            .is_err()
    );
    assert!(rendered.contains("the sign-in is in use"), "{rendered}");
    assert!(
        rendered.contains("kestrel session show holding --organization acme"),
        "{rendered}"
    );
}
