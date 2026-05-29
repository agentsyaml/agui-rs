use agui_rs_core::types::{ActivityMessage, Message};
use agui_rs_core::{ActivityDeltaEvent, ActivitySnapshotEvent, Event};
use serde_json::json;

#[test]
fn activity_snapshot_event_defaults_replace_to_true() {
    let event: Event = serde_json::from_value(json!({
        "type": "ACTIVITY_SNAPSHOT",
        "messageId": "msg_activity",
        "activityType": "PLAN",
        "content": { "tasks": ["search"] }
    }))
    .expect("deserialize activity snapshot");

    match event {
        Event::ActivitySnapshot(ActivitySnapshotEvent {
            message_id,
            activity_type,
            content,
            replace,
            ..
        }) => {
            assert_eq!(message_id, "msg_activity");
            assert_eq!(activity_type, "PLAN");
            assert_eq!(content.get("tasks"), Some(&json!(["search"])));
            assert!(replace);
        }
        other => panic!("expected ActivitySnapshot, got {other:?}"),
    }
}

#[test]
fn activity_snapshot_event_respects_replace_flag() {
    let event: Event = serde_json::from_value(json!({
        "type": "ACTIVITY_SNAPSHOT",
        "messageId": "msg_activity",
        "activityType": "PLAN",
        "content": { "tasks": [] },
        "replace": false
    }))
    .expect("deserialize activity snapshot");

    match event {
        Event::ActivitySnapshot(event) => assert!(!event.replace),
        other => panic!("expected ActivitySnapshot, got {other:?}"),
    }
}

#[test]
fn activity_delta_event_parses_patch_payload() {
    let event: Event = serde_json::from_value(json!({
        "type": "ACTIVITY_DELTA",
        "messageId": "msg_activity",
        "activityType": "PLAN",
        "patch": [{ "op": "replace", "path": "/tasks/0", "value": "✓ search" }]
    }))
    .expect("deserialize activity delta");

    match event {
        Event::ActivityDelta(ActivityDeltaEvent { patch, .. }) => assert_eq!(patch.len(), 1),
        other => panic!("expected ActivityDelta, got {other:?}"),
    }
}

#[test]
fn activity_message_parses_canonical_shape() {
    let message: Message = serde_json::from_value(json!({
        "id": "activity_1",
        "role": "activity",
        "activityType": "PLAN",
        "content": { "tasks": [] }
    }))
    .expect("deserialize activity message");

    match message {
        Message::Activity(ActivityMessage {
            activity_type,
            content,
            ..
        }) => {
            assert_eq!(activity_type, "PLAN");
            assert_eq!(
                content,
                serde_json::Map::from_iter([("tasks".to_string(), json!([]))])
            );
        }
        other => panic!("expected Activity message, got {other:?}"),
    }
}
