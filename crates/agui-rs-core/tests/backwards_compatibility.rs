use agui_rs_core::types::{Message, RunAgentInput};
use agui_rs_core::{Context, Event, Tool};
use serde_json::json;

#[test]
fn user_message_accepts_and_strips_extra_fields() {
    let message: Message = serde_json::from_value(json!({
        "id": "msg_1",
        "role": "user",
        "content": "Hello",
        "futureField": "This is from a future version",
        "anotherNewProp": { "nested": "data" }
    }))
    .expect("deserialize user message");

    let serialized = serde_json::to_value(&message).expect("serialize user message");
    assert_eq!(serialized["id"], "msg_1");
    assert_eq!(serialized["role"], "user");
    assert_eq!(serialized["content"], "Hello");
    assert!(serialized.get("futureField").is_none());
    assert!(serialized.get("anotherNewProp").is_none());
}

#[test]
fn assistant_message_accepts_extra_fields() {
    let message: Message = serde_json::from_value(json!({
        "id": "msg_2",
        "role": "assistant",
        "content": "Response",
        "newFeatureFlag": true,
        "experimentalData": [1, 2, 3]
    }))
    .expect("deserialize assistant message");

    let serialized = serde_json::to_value(&message).expect("serialize assistant message");
    assert_eq!(serialized["id"], "msg_2");
    assert_eq!(serialized["content"], "Response");
    assert!(serialized.get("newFeatureFlag").is_none());
}

#[test]
fn run_agent_input_accepts_extra_top_level_fields() {
    let input: RunAgentInput = serde_json::from_value(json!({
        "threadId": "thread_1",
        "runId": "run_1",
        "parentRunId": "parent_run_1",
        "state": {},
        "messages": [],
        "tools": [],
        "context": [],
        "forwardedProps": {},
        "newFeatureFlag": true,
        "experimentalTimeout": 5000,
        "futureConfig": { "option": "value" }
    }))
    .expect("deserialize run input");

    assert_eq!(input.thread_id, "thread_1");
    assert_eq!(input.run_id, "run_1");
    assert_eq!(input.parent_run_id.as_deref(), Some("parent_run_1"));
    let serialized = serde_json::to_value(input).expect("serialize run input");
    assert!(serialized.get("newFeatureFlag").is_none());
    assert!(serialized.get("experimentalTimeout").is_none());
}

#[test]
fn run_agent_input_accepts_messages_with_extra_fields() {
    let input: RunAgentInput = serde_json::from_value(json!({
        "threadId": "thread_2",
        "runId": "run_2",
        "state": {},
        "messages": [
            {
                "id": "m1",
                "role": "user",
                "content": "Hi",
                "extraProp": "value",
                "metadata": { "source": "web" }
            }
        ],
        "tools": [],
        "context": [],
        "forwardedProps": {}
    }))
    .expect("deserialize run input with extra message fields");

    assert_eq!(input.messages.len(), 1);
    let serialized = serde_json::to_value(input).expect("serialize run input");
    assert_eq!(serialized["messages"][0]["content"], "Hi");
    assert!(serialized["messages"][0].get("extraProp").is_none());
}

#[test]
fn text_message_start_event_accepts_extra_fields() {
    let event: Event = serde_json::from_value(json!({
        "type": "TEXT_MESSAGE_START",
        "messageId": "msg_1",
        "role": "assistant",
        "metadata": { "tokenCount": 10 },
        "experimentalFeature": true
    }))
    .expect("deserialize text message start event");

    let serialized = serde_json::to_value(event).expect("serialize text message start event");
    assert_eq!(serialized["type"], "TEXT_MESSAGE_START");
    assert_eq!(serialized["messageId"], "msg_1");
    assert!(serialized.get("metadata").is_none());
    assert!(serialized.get("experimentalFeature").is_none());
}

#[test]
fn run_started_event_accepts_extra_fields() {
    let event: Event = serde_json::from_value(json!({
        "type": "RUN_STARTED",
        "threadId": "thread_1",
        "runId": "run_1",
        "startTime": 1710000000,
        "priority": "high"
    }))
    .expect("deserialize run started event");

    let serialized = serde_json::to_value(event).expect("serialize run started event");
    assert_eq!(serialized["threadId"], "thread_1");
    assert_eq!(serialized["runId"], "run_1");
    assert!(serialized.get("startTime").is_none());
    assert!(serialized.get("priority").is_none());
}

#[test]
fn tool_and_context_accept_extra_fields() {
    let tool: Tool = serde_json::from_value(json!({
        "name": "calculator",
        "description": "Performs calculations",
        "parameters": { "type": "object" },
        "version": "2.0",
        "deprecationWarning": null
    }))
    .expect("deserialize tool");
    let context: Context = serde_json::from_value(json!({
        "description": "User preferences",
        "value": "{\"theme\":\"dark\"}",
        "priority": 1,
        "expiresAt": 99999999
    }))
    .expect("deserialize context");

    assert_eq!(tool.name, "calculator");
    assert_eq!(tool.description, "Performs calculations");
    assert_eq!(context.description, "User preferences");
    assert_eq!(context.value, "{\"theme\":\"dark\"}");
    assert!(serde_json::to_value(tool).unwrap().get("version").is_none());
    assert!(serde_json::to_value(context)
        .unwrap()
        .get("priority")
        .is_none());
}

#[test]
fn complex_nested_structures_ignore_extra_fields_at_multiple_levels() {
    let input: RunAgentInput = serde_json::from_value(json!({
        "threadId": "thread_complex",
        "runId": "run_complex",
        "state": { "currentStep": 1 },
        "messages": [
            {
                "id": "m1",
                "role": "user",
                "content": "Hello",
                "extraUserProp": "value1"
            },
            {
                "id": "m2",
                "role": "assistant",
                "content": "Hi there",
                "toolCalls": [
                    {
                        "id": "tc1",
                        "type": "function",
                        "function": {
                            "name": "search",
                            "arguments": "{}",
                            "extraFunctionProp": "value2"
                        },
                        "extraToolCallProp": "value3"
                    }
                ],
                "extraAssistantProp": "value4"
            }
        ],
        "tools": [
            {
                "name": "search",
                "description": "Search tool",
                "parameters": {},
                "extraToolProp": "value5"
            }
        ],
        "context": [
            {
                "description": "ctx",
                "value": "val",
                "extraContextProp": "value6"
            }
        ],
        "forwardedProps": { "custom": true },
        "extraTopLevelProp": "value7"
    }))
    .expect("deserialize complex input");

    assert_eq!(input.messages.len(), 2);
    let serialized = serde_json::to_value(input).expect("serialize complex input");
    assert_eq!(serialized["messages"].as_array().unwrap().len(), 2);
    assert_eq!(serialized["tools"].as_array().unwrap().len(), 1);
    assert_eq!(serialized["context"].as_array().unwrap().len(), 1);
    assert_eq!(
        serialized["messages"][1]["toolCalls"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert!(serialized.get("extraTopLevelProp").is_none());
    assert!(serialized["messages"][0].get("extraUserProp").is_none());
    assert!(serialized["messages"][1]["toolCalls"][0]
        .get("extraToolCallProp")
        .is_none());
    assert!(serialized["messages"][1]["toolCalls"][0]["function"]
        .get("extraFunctionProp")
        .is_none());
}

#[test]
fn legacy_thinking_events_exist_and_serialize_with_legacy_type_strings() {
    let events = [
        serde_json::to_value(Event::ThinkingStart(agui_rs_core::ThinkingStartEvent {
            title: Some("planning".into()),
            base: Default::default(),
        }))
        .unwrap(),
        serde_json::to_value(Event::ThinkingTextMessageStart(Default::default())).unwrap(),
        serde_json::to_value(Event::ThinkingTextMessageContent(
            agui_rs_core::ThinkingTextMessageContentEvent {
                delta: "hello".into(),
                base: Default::default(),
            },
        ))
        .unwrap(),
        serde_json::to_value(Event::ThinkingEnd(Default::default())).unwrap(),
    ];

    assert_eq!(events[0]["type"], "THINKING_START");
    assert_eq!(events[1]["type"], "THINKING_TEXT_MESSAGE_START");
    assert_eq!(events[2]["type"], "THINKING_TEXT_MESSAGE_CONTENT");
    assert_eq!(events[3]["type"], "THINKING_END");
}

#[test]
fn legacy_thinking_payloads_deserialize_successfully() {
    let thinking_start: Event = serde_json::from_value(json!({
        "type": "THINKING_START",
        "title": "planning"
    }))
    .expect("deserialize thinking start");
    let thinking_text_start: Event = serde_json::from_value(json!({
        "type": "THINKING_TEXT_MESSAGE_START"
    }))
    .expect("deserialize thinking text start");
    let thinking_text_content: Event = serde_json::from_value(json!({
        "type": "THINKING_TEXT_MESSAGE_CONTENT",
        "delta": "hello"
    }))
    .expect("deserialize thinking text content");
    let thinking_end: Event = serde_json::from_value(json!({
        "type": "THINKING_END"
    }))
    .expect("deserialize thinking end");

    assert!(matches!(thinking_start, Event::ThinkingStart(_)));
    assert!(matches!(
        thinking_text_start,
        Event::ThinkingTextMessageStart(_)
    ));
    assert!(matches!(
        thinking_text_content,
        Event::ThinkingTextMessageContent(_)
    ));
    assert!(matches!(thinking_end, Event::ThinkingEnd(_)));
}
