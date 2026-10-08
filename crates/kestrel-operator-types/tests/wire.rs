use kestrel_operator_types::{ChangesEvent, Event, FollowerEvent, WorkReported};
use uuid::Uuid;

#[test]
fn sse_names_disambiguate_identical_payloads() {
    let open = ChangesEvent::from_sse("open", "{}").unwrap().unwrap();
    let resync = ChangesEvent::from_sse("resync", "{}").unwrap().unwrap();
    assert!(matches!(open, ChangesEvent::ChangesOpen(_)));
    assert!(matches!(resync, ChangesEvent::ChangesResync(_)));
    assert_eq!(open.event_name(), "open");
    assert_eq!(resync.event_name(), "resync");
    assert_eq!(serde_json::to_string(&open).unwrap(), "{}");
    assert_eq!(serde_json::to_string(&resync).unwrap(), "{}");
}

#[test]
fn sse_dispatch_keeps_cursors_and_rejects_malformed_known_events() {
    let cursor = Event::from_sse("cursor", r#""workspace:42""#)
        .unwrap()
        .unwrap();
    assert!(matches!(&cursor, Event::CursorEvent(value) if value == "workspace:42"));
    assert_eq!(cursor.event_name(), "cursor");
    assert_eq!(serde_json::to_string(&cursor).unwrap(), r#""workspace:42""#);
    assert!(Event::from_sse("presence", "not json").unwrap().is_err());
    assert!(Event::from_sse("future_event", "not json").is_none());
}

#[test]
fn identifiers_are_uuids_and_timestamps_preserve_wire_text() {
    let id = Uuid::nil();
    let follower = FollowerEvent {
        id,
        lease_seconds: 30,
    };
    let event = Event::from_sse("follower", &serde_json::to_string(&follower).unwrap())
        .unwrap()
        .unwrap();
    assert!(matches!(event, Event::FollowerEvent(value) if value.id == id));
    let reported: WorkReported = serde_json::from_str(
        r#"{"state":"reported","repositories":[],"reported_at":"2026-10-06T00:00:00.123456789+00:00","last_report":{"report":"none"},"earlier_reports":[]}"#,
    ).unwrap();
    let timestamp: String = reported.reported_at;
    assert_eq!(timestamp, "2026-10-06T00:00:00.123456789+00:00");
}
