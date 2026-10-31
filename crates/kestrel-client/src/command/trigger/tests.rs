use kestrel_operator_types as wire;
use serde_json::json;

use super::*;

#[test]
fn a_trigger_declaration_is_the_generated_request_the_flags_describe() {
    let declaration = wire::TriggerDeclaration {
        name: "ready".to_owned(),
        brief: "Work on {{ event.type }}".to_owned(),
        branch: Some(None),
        correlation: Some(None),
        on_miss: Some(None),
        on_open_workspace: Some(None),
        project: "kestrel".to_owned(),
        agent: "builder".to_owned(),
        allows: Some(vec!["reviewer".to_owned()]),
        profile: Some(None),
        filter: None,
        every: None,
        cron: None,
        zone: None,
        model: None,
        mode: None,
        thought_level: None,
        variant: trigger_condition(Some(json!({ "exact": { "type": "x" } })), None, None, None),
    };

    assert_eq!(
        serde_json::to_value(&declaration).unwrap(),
        json!({
            "name": "ready", "brief": "Work on {{ event.type }}", "project": "kestrel",
            "agent": "builder", "allows": ["reviewer"], "branch": null, "correlation": null,
            "on_miss": null, "on_open_workspace": null, "profile": null,
            "filter": { "exact": { "type": "x" } },
        })
    );
    assert_eq!(
        serde_json::to_value(trigger_condition(
            None,
            None,
            Some("0 9 * * 1-5".to_owned()),
            Some("UTC".to_owned()),
        ))
        .unwrap(),
        json!({ "cron": "0 9 * * 1-5", "zone": "UTC" })
    );
}

#[test]
fn a_test_request_carries_the_declaration_file_the_control_plane_reads() {
    let document = json!({ "triggers": { "ready": {
        "filter": { "exact": { "type": "x" } }, "brief": "Work", "project": "kestrel",
        "agent": "builder",
    } } });

    let typed = declaration_document(document.clone()).expect("the file the document holds");
    assert_eq!(serde_json::to_value(&typed).unwrap(), document);

    let stray = declaration_document(json!({ "triggers": { "ready": {
        "filter": { "exact": { "type": "x" } }, "brief": "Work", "project": "kestrel",
        "agent": "builder", "agnet": "builder",
    } } }));
    assert_eq!(
        stray.unwrap_err().to_string(),
        "the declaration file: triggers.ready: unknown field `agnet`"
    );
}
