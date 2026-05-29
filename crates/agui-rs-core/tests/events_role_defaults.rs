use agui_rs_core::{Event, EventType, TextMessageRole};
use serde_json::json;

fn parse_event(value: serde_json::Value) -> Event {
    serde_json::from_value(value).expect("deserialize event")
}

#[test]
fn text_message_start_defaults_role_to_assistant_when_omitted() {
    let event = parse_event(json!({
        "type": "TEXT_MESSAGE_START",
        "messageId": "test-msg"
    }));

    match event {
        Event::TextMessageStart(event) => {
            assert_eq!(event.role, TextMessageRole::Assistant);
            assert_eq!(event.message_id, "test-msg");
        }
        other => panic!("expected TextMessageStart, got {other:?}"),
    }
}

#[test]
fn text_message_start_allows_overriding_default_role() {
    let event = parse_event(json!({
        "type": "TEXT_MESSAGE_START",
        "messageId": "test-msg",
        "role": "user"
    }));

    match event {
        Event::TextMessageStart(event) => assert_eq!(event.role, TextMessageRole::User),
        other => panic!("expected TextMessageStart, got {other:?}"),
    }
}

#[test]
fn text_message_start_accepts_all_valid_roles() {
    for (role_name, role) in [
        ("developer", TextMessageRole::Developer),
        ("system", TextMessageRole::System),
        ("assistant", TextMessageRole::Assistant),
        ("user", TextMessageRole::User),
    ] {
        let event = parse_event(json!({
            "type": EventType::TextMessageStart,
            "messageId": format!("test-msg-{role_name}"),
            "role": role_name
        }));

        match event {
            Event::TextMessageStart(event) => assert_eq!(event.role, role),
            other => panic!("expected TextMessageStart, got {other:?}"),
        }
    }
}

#[test]
fn text_message_chunk_keeps_role_optional() {
    let without_role = parse_event(json!({
        "type": "TEXT_MESSAGE_CHUNK",
        "messageId": "test-msg",
        "delta": "test content"
    }));
    let with_role = parse_event(json!({
        "type": "TEXT_MESSAGE_CHUNK",
        "messageId": "test-msg",
        "delta": "test content",
        "role": "user"
    }));

    match without_role {
        Event::TextMessageChunk(event) => assert_eq!(event.role, None),
        other => panic!("expected TextMessageChunk, got {other:?}"),
    }

    match with_role {
        Event::TextMessageChunk(event) => assert_eq!(event.role, Some(TextMessageRole::User)),
        other => panic!("expected TextMessageChunk, got {other:?}"),
    }
}

#[test]
fn text_message_events_reject_invalid_roles() {
    let invalid = serde_json::from_value::<Event>(json!({
        "type": "TEXT_MESSAGE_START",
        "messageId": "test-msg",
        "role": "invalid_role"
    }))
    .expect_err("invalid role should fail");

    assert!(
        invalid.to_string().contains("unknown variant")
            || invalid.to_string().contains("invalid_role")
    );
}

#[test]
fn text_message_events_reject_tool_role() {
    let start_error = serde_json::from_value::<Event>(json!({
        "type": "TEXT_MESSAGE_START",
        "messageId": "test-msg",
        "role": "tool"
    }))
    .expect_err("tool role should fail for text start");
    let chunk_error = serde_json::from_value::<Event>(json!({
        "type": "TEXT_MESSAGE_CHUNK",
        "messageId": "test-msg",
        "role": "tool",
        "delta": "content"
    }))
    .expect_err("tool role should fail for text chunk");

    assert!(
        start_error.to_string().contains("tool")
            || start_error.to_string().contains("unknown variant")
    );
    assert!(
        chunk_error.to_string().contains("tool")
            || chunk_error.to_string().contains("unknown variant")
    );
}

#[test]
fn text_message_start_allows_name() {
    let event = parse_event(json!({
        "type": "TEXT_MESSAGE_START",
        "messageId": "test-msg",
        "name": "research-agent"
    }));

    match event {
        Event::TextMessageStart(event) => assert_eq!(event.name.as_deref(), Some("research-agent")),
        other => panic!("expected TextMessageStart, got {other:?}"),
    }
}

#[test]
fn text_message_start_omits_name_when_not_provided() {
    let event = parse_event(json!({
        "type": "TEXT_MESSAGE_START",
        "messageId": "test-msg"
    }));

    match event {
        Event::TextMessageStart(event) => assert_eq!(event.name, None),
        other => panic!("expected TextMessageStart, got {other:?}"),
    }
}

#[test]
fn text_message_chunk_allows_name() {
    let event = parse_event(json!({
        "type": "TEXT_MESSAGE_CHUNK",
        "messageId": "test-msg",
        "delta": "Hello",
        "name": "research-agent"
    }));

    match event {
        Event::TextMessageChunk(event) => assert_eq!(event.name.as_deref(), Some("research-agent")),
        other => panic!("expected TextMessageChunk, got {other:?}"),
    }
}

#[test]
fn text_message_chunk_omits_name_when_not_provided() {
    let event = parse_event(json!({
        "type": "TEXT_MESSAGE_CHUNK",
        "messageId": "test-msg",
        "delta": "Hello"
    }));

    match event {
        Event::TextMessageChunk(event) => assert_eq!(event.name, None),
        other => panic!("expected TextMessageChunk, got {other:?}"),
    }
}
