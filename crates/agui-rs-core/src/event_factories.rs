use crate::events::{
    ActivityDeltaEvent, ActivitySnapshotEvent, BaseEventFields, CustomEvent, Event,
    MessagesSnapshotEvent, RawEvent, ReasoningEncryptedValueEvent, ReasoningEncryptedValueSubtype,
    ReasoningEndEvent, ReasoningMessageChunkEvent, ReasoningMessageContentEvent,
    ReasoningMessageEndEvent, ReasoningMessageRole, ReasoningMessageStartEvent,
    ReasoningStartEvent, RunErrorEvent, RunFinishedEvent, RunFinishedOutcome, RunStartedEvent,
    StateDeltaEvent, StateSnapshotEvent, StepFinishedEvent, StepStartedEvent,
    TextMessageChunkEvent, TextMessageContentEvent, TextMessageEndEvent, TextMessageStartEvent,
    ThinkingEndEvent, ThinkingStartEvent, ThinkingTextMessageContentEvent,
    ThinkingTextMessageEndEvent, ThinkingTextMessageStartEvent, ToolCallArgsEvent,
    ToolCallChunkEvent, ToolCallEndEvent, ToolCallResultEvent, ToolCallStartEvent, ToolResultRole,
};
use crate::types::{Interrupt, Message, RunAgentInput, State, TextMessageRole};
use serde_json::Value;
use std::time::{SystemTime, UNIX_EPOCH};

fn current_timestamp_millis() -> i64 {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    millis.min(i64::MAX as u128) as i64
}

fn base_fields(timestamp: Option<i64>, raw_event: Option<Value>) -> BaseEventFields {
    BaseEventFields {
        timestamp: Some(timestamp.unwrap_or_else(current_timestamp_millis)),
        raw_event,
    }
}

/// Creates a text message start event.
pub fn create_text_message_start_event(
    message_id: impl Into<String>,
    role: Option<TextMessageRole>,
    name: Option<String>,
    timestamp: Option<i64>,
    raw_event: Option<Value>,
) -> Event {
    Event::TextMessageStart(TextMessageStartEvent {
        message_id: message_id.into(),
        role: role.unwrap_or_default(),
        name,
        base: base_fields(timestamp, raw_event),
    })
}

/// Creates a text message content event.
pub fn create_text_message_content_event(
    message_id: impl Into<String>,
    delta: impl Into<String>,
    timestamp: Option<i64>,
    raw_event: Option<Value>,
) -> Event {
    Event::TextMessageContent(TextMessageContentEvent {
        message_id: message_id.into(),
        delta: delta.into(),
        base: base_fields(timestamp, raw_event),
    })
}

/// Creates a text message end event.
pub fn create_text_message_end_event(
    message_id: impl Into<String>,
    timestamp: Option<i64>,
    raw_event: Option<Value>,
) -> Event {
    Event::TextMessageEnd(TextMessageEndEvent {
        message_id: message_id.into(),
        base: base_fields(timestamp, raw_event),
    })
}

/// Creates a text message chunk event.
pub fn create_text_message_chunk_event(
    message_id: Option<String>,
    role: Option<TextMessageRole>,
    delta: Option<String>,
    name: Option<String>,
    timestamp: Option<i64>,
    raw_event: Option<Value>,
) -> Event {
    Event::TextMessageChunk(TextMessageChunkEvent {
        message_id,
        role,
        delta,
        name,
        base: base_fields(timestamp, raw_event),
    })
}

/// Creates a thinking text message start event.
pub fn create_thinking_text_message_start_event(
    timestamp: Option<i64>,
    raw_event: Option<Value>,
) -> Event {
    Event::ThinkingTextMessageStart(ThinkingTextMessageStartEvent {
        base: base_fields(timestamp, raw_event),
    })
}

/// Creates a thinking text message content event.
pub fn create_thinking_text_message_content_event(
    delta: impl Into<String>,
    timestamp: Option<i64>,
    raw_event: Option<Value>,
) -> Event {
    Event::ThinkingTextMessageContent(ThinkingTextMessageContentEvent {
        delta: delta.into(),
        base: base_fields(timestamp, raw_event),
    })
}

/// Creates a thinking text message end event.
pub fn create_thinking_text_message_end_event(
    timestamp: Option<i64>,
    raw_event: Option<Value>,
) -> Event {
    Event::ThinkingTextMessageEnd(ThinkingTextMessageEndEvent {
        base: base_fields(timestamp, raw_event),
    })
}

/// Creates a tool call start event.
pub fn create_tool_call_start_event(
    tool_call_id: impl Into<String>,
    tool_call_name: impl Into<String>,
    parent_message_id: Option<String>,
    timestamp: Option<i64>,
    raw_event: Option<Value>,
) -> Event {
    Event::ToolCallStart(ToolCallStartEvent {
        tool_call_id: tool_call_id.into(),
        tool_call_name: tool_call_name.into(),
        parent_message_id,
        base: base_fields(timestamp, raw_event),
    })
}

/// Creates a tool call args event.
pub fn create_tool_call_args_event(
    tool_call_id: impl Into<String>,
    delta: impl Into<String>,
    timestamp: Option<i64>,
    raw_event: Option<Value>,
) -> Event {
    Event::ToolCallArgs(ToolCallArgsEvent {
        tool_call_id: tool_call_id.into(),
        delta: delta.into(),
        base: base_fields(timestamp, raw_event),
    })
}

/// Creates a tool call end event.
pub fn create_tool_call_end_event(
    tool_call_id: impl Into<String>,
    timestamp: Option<i64>,
    raw_event: Option<Value>,
) -> Event {
    Event::ToolCallEnd(ToolCallEndEvent {
        tool_call_id: tool_call_id.into(),
        base: base_fields(timestamp, raw_event),
    })
}

/// Creates a tool call chunk event.
pub fn create_tool_call_chunk_event(
    tool_call_id: Option<String>,
    tool_call_name: Option<String>,
    parent_message_id: Option<String>,
    delta: Option<String>,
    timestamp: Option<i64>,
    raw_event: Option<Value>,
) -> Event {
    Event::ToolCallChunk(ToolCallChunkEvent {
        tool_call_id,
        tool_call_name,
        parent_message_id,
        delta,
        base: base_fields(timestamp, raw_event),
    })
}

/// Creates a tool call result event.
pub fn create_tool_call_result_event(
    message_id: impl Into<String>,
    tool_call_id: impl Into<String>,
    content: impl Into<String>,
    role: Option<ToolResultRole>,
    timestamp: Option<i64>,
    raw_event: Option<Value>,
) -> Event {
    Event::ToolCallResult(ToolCallResultEvent {
        message_id: message_id.into(),
        tool_call_id: tool_call_id.into(),
        content: content.into(),
        role,
        base: base_fields(timestamp, raw_event),
    })
}

/// Creates a thinking start event.
pub fn create_thinking_start_event(
    title: Option<String>,
    timestamp: Option<i64>,
    raw_event: Option<Value>,
) -> Event {
    Event::ThinkingStart(ThinkingStartEvent {
        title,
        base: base_fields(timestamp, raw_event),
    })
}

/// Creates a thinking end event.
pub fn create_thinking_end_event(timestamp: Option<i64>, raw_event: Option<Value>) -> Event {
    Event::ThinkingEnd(ThinkingEndEvent {
        base: base_fields(timestamp, raw_event),
    })
}

/// Creates a state snapshot event.
pub fn create_state_snapshot_event(
    snapshot: State,
    timestamp: Option<i64>,
    raw_event: Option<Value>,
) -> Event {
    Event::StateSnapshot(StateSnapshotEvent {
        snapshot,
        base: base_fields(timestamp, raw_event),
    })
}

/// Creates a state delta event.
pub fn create_state_delta_event(
    delta: Vec<Value>,
    timestamp: Option<i64>,
    raw_event: Option<Value>,
) -> Event {
    Event::StateDelta(StateDeltaEvent {
        delta,
        base: base_fields(timestamp, raw_event),
    })
}

/// Creates a messages snapshot event.
pub fn create_messages_snapshot_event(
    messages: Vec<Message>,
    timestamp: Option<i64>,
    raw_event: Option<Value>,
) -> Event {
    Event::MessagesSnapshot(MessagesSnapshotEvent {
        messages,
        base: base_fields(timestamp, raw_event),
    })
}

/// Creates an activity snapshot event.
pub fn create_activity_snapshot_event(
    message_id: impl Into<String>,
    activity_type: impl Into<String>,
    content: serde_json::Map<String, Value>,
    replace: Option<bool>,
    timestamp: Option<i64>,
    raw_event: Option<Value>,
) -> Event {
    Event::ActivitySnapshot(ActivitySnapshotEvent {
        message_id: message_id.into(),
        activity_type: activity_type.into(),
        content,
        replace: replace.unwrap_or(true),
        base: base_fields(timestamp, raw_event),
    })
}

/// Creates an activity delta event.
pub fn create_activity_delta_event(
    message_id: impl Into<String>,
    activity_type: impl Into<String>,
    patch: Vec<Value>,
    timestamp: Option<i64>,
    raw_event: Option<Value>,
) -> Event {
    Event::ActivityDelta(ActivityDeltaEvent {
        message_id: message_id.into(),
        activity_type: activity_type.into(),
        patch,
        base: base_fields(timestamp, raw_event),
    })
}

/// Creates a raw event.
pub fn create_raw_event(
    event: Value,
    source: Option<String>,
    timestamp: Option<i64>,
    raw_event: Option<Value>,
) -> Event {
    Event::Raw(RawEvent {
        event,
        source,
        base: base_fields(timestamp, raw_event),
    })
}

/// Creates a custom event.
pub fn create_custom_event(
    name: impl Into<String>,
    value: Value,
    timestamp: Option<i64>,
    raw_event: Option<Value>,
) -> Event {
    Event::Custom(CustomEvent {
        name: name.into(),
        value,
        base: base_fields(timestamp, raw_event),
    })
}

/// Creates a run started event.
pub fn create_run_started_event(
    thread_id: impl Into<String>,
    run_id: impl Into<String>,
    parent_run_id: Option<String>,
    input: Option<RunAgentInput>,
    timestamp: Option<i64>,
    raw_event: Option<Value>,
) -> Event {
    Event::RunStarted(RunStartedEvent {
        thread_id: thread_id.into(),
        run_id: run_id.into(),
        parent_run_id,
        input,
        base: base_fields(timestamp, raw_event),
    })
}

/// Creates a run finished event.
pub fn create_run_finished_event(
    thread_id: impl Into<String>,
    run_id: impl Into<String>,
    result: Option<Value>,
    outcome: Option<RunFinishedOutcome>,
    timestamp: Option<i64>,
    raw_event: Option<Value>,
) -> Event {
    Event::RunFinished(RunFinishedEvent {
        thread_id: thread_id.into(),
        run_id: run_id.into(),
        result,
        outcome,
        base: base_fields(timestamp, raw_event),
    })
}

/// Creates a successful run finished event.
pub fn create_run_finished_success_event(
    thread_id: impl Into<String>,
    run_id: impl Into<String>,
    result: Option<Value>,
    timestamp: Option<i64>,
    raw_event: Option<Value>,
) -> Event {
    create_run_finished_event(
        thread_id,
        run_id,
        result,
        Some(RunFinishedOutcome::Success),
        timestamp,
        raw_event,
    )
}

/// Creates an interrupted run finished event.
pub fn create_run_finished_interrupt_event(
    thread_id: impl Into<String>,
    run_id: impl Into<String>,
    result: Option<Value>,
    interrupts: Vec<Interrupt>,
    timestamp: Option<i64>,
    raw_event: Option<Value>,
) -> Event {
    create_run_finished_event(
        thread_id,
        run_id,
        result,
        Some(RunFinishedOutcome::Interrupt { interrupts }),
        timestamp,
        raw_event,
    )
}

/// Creates a run error event.
pub fn create_run_error_event(
    message: impl Into<String>,
    code: Option<String>,
    timestamp: Option<i64>,
    raw_event: Option<Value>,
) -> Event {
    Event::RunError(RunErrorEvent {
        message: message.into(),
        code,
        base: base_fields(timestamp, raw_event),
    })
}

/// Creates a step started event.
pub fn create_step_started_event(
    step_name: impl Into<String>,
    timestamp: Option<i64>,
    raw_event: Option<Value>,
) -> Event {
    Event::StepStarted(StepStartedEvent {
        step_name: step_name.into(),
        base: base_fields(timestamp, raw_event),
    })
}

/// Creates a step finished event.
pub fn create_step_finished_event(
    step_name: impl Into<String>,
    timestamp: Option<i64>,
    raw_event: Option<Value>,
) -> Event {
    Event::StepFinished(StepFinishedEvent {
        step_name: step_name.into(),
        base: base_fields(timestamp, raw_event),
    })
}

/// Creates a reasoning start event.
pub fn create_reasoning_start_event(
    message_id: impl Into<String>,
    timestamp: Option<i64>,
    raw_event: Option<Value>,
) -> Event {
    Event::ReasoningStart(ReasoningStartEvent {
        message_id: message_id.into(),
        base: base_fields(timestamp, raw_event),
    })
}

/// Creates a reasoning message start event.
pub fn create_reasoning_message_start_event(
    message_id: impl Into<String>,
    role: Option<ReasoningMessageRole>,
    timestamp: Option<i64>,
    raw_event: Option<Value>,
) -> Event {
    Event::ReasoningMessageStart(ReasoningMessageStartEvent {
        message_id: message_id.into(),
        role: role.unwrap_or(ReasoningMessageRole::Reasoning),
        base: base_fields(timestamp, raw_event),
    })
}

/// Creates a reasoning message content event.
pub fn create_reasoning_message_content_event(
    message_id: impl Into<String>,
    delta: impl Into<String>,
    timestamp: Option<i64>,
    raw_event: Option<Value>,
) -> Event {
    Event::ReasoningMessageContent(ReasoningMessageContentEvent {
        message_id: message_id.into(),
        delta: delta.into(),
        base: base_fields(timestamp, raw_event),
    })
}

/// Creates a reasoning message end event.
pub fn create_reasoning_message_end_event(
    message_id: impl Into<String>,
    timestamp: Option<i64>,
    raw_event: Option<Value>,
) -> Event {
    Event::ReasoningMessageEnd(ReasoningMessageEndEvent {
        message_id: message_id.into(),
        base: base_fields(timestamp, raw_event),
    })
}

/// Creates a reasoning message chunk event.
pub fn create_reasoning_message_chunk_event(
    message_id: Option<String>,
    delta: Option<String>,
    timestamp: Option<i64>,
    raw_event: Option<Value>,
) -> Event {
    Event::ReasoningMessageChunk(ReasoningMessageChunkEvent {
        message_id,
        delta,
        base: base_fields(timestamp, raw_event),
    })
}

/// Creates a reasoning end event.
pub fn create_reasoning_end_event(
    message_id: impl Into<String>,
    timestamp: Option<i64>,
    raw_event: Option<Value>,
) -> Event {
    Event::ReasoningEnd(ReasoningEndEvent {
        message_id: message_id.into(),
        base: base_fields(timestamp, raw_event),
    })
}

/// Creates a reasoning encrypted value event.
pub fn create_reasoning_encrypted_value_event(
    subtype: ReasoningEncryptedValueSubtype,
    entity_id: impl Into<String>,
    encrypted_value: impl Into<String>,
    timestamp: Option<i64>,
    raw_event: Option<Value>,
) -> Event {
    Event::ReasoningEncryptedValue(ReasoningEncryptedValueEvent {
        subtype,
        entity_id: entity_id.into(),
        encrypted_value: encrypted_value.into(),
        base: base_fields(timestamp, raw_event),
    })
}

#[cfg(test)]
mod factories_tests {
    use super::*;
    use crate::events::EventType;
    use crate::types::{
        AssistantMessage, BinaryInputContent, InputContent, UserMessage, UserMessageContent,
    };
    use serde_json::json;

    fn assert_timestamped(event: &Event) {
        assert!(event.base().timestamp.is_some());
    }

    macro_rules! factory_test {
        ($name:ident, $expr:expr, $pattern:pat, $event_type:expr) => {
            #[test]
            fn $name() {
                let event = $expr;
                assert!(matches!(event, $pattern));
                assert_eq!(event.event_type(), $event_type);
                assert_timestamped(&event);
            }
        };
    }

    factory_test!(
        create_text_message_start_event_builds_variant,
        create_text_message_start_event("m1", None, None, None, None),
        Event::TextMessageStart(_),
        EventType::TextMessageStart
    );
    factory_test!(
        create_text_message_content_event_builds_variant,
        create_text_message_content_event("m1", "hello", None, None),
        Event::TextMessageContent(_),
        EventType::TextMessageContent
    );
    factory_test!(
        create_text_message_end_event_builds_variant,
        create_text_message_end_event("m1", None, None),
        Event::TextMessageEnd(_),
        EventType::TextMessageEnd
    );
    factory_test!(
        create_text_message_chunk_event_builds_variant,
        create_text_message_chunk_event(
            Some("m1".into()),
            Some(TextMessageRole::Assistant),
            Some("hi".into()),
            None,
            None,
            None
        ),
        Event::TextMessageChunk(_),
        EventType::TextMessageChunk
    );
    factory_test!(
        create_thinking_text_message_start_event_builds_variant,
        create_thinking_text_message_start_event(None, None),
        Event::ThinkingTextMessageStart(_),
        EventType::ThinkingTextMessageStart
    );
    factory_test!(
        create_thinking_text_message_content_event_builds_variant,
        create_thinking_text_message_content_event("thinking", None, None),
        Event::ThinkingTextMessageContent(_),
        EventType::ThinkingTextMessageContent
    );
    factory_test!(
        create_thinking_text_message_end_event_builds_variant,
        create_thinking_text_message_end_event(None, None),
        Event::ThinkingTextMessageEnd(_),
        EventType::ThinkingTextMessageEnd
    );
    factory_test!(
        create_tool_call_start_event_builds_variant,
        create_tool_call_start_event("tc1", "search", Some("m1".into()), None, None),
        Event::ToolCallStart(_),
        EventType::ToolCallStart
    );
    factory_test!(
        create_tool_call_args_event_builds_variant,
        create_tool_call_args_event("tc1", "{", None, None),
        Event::ToolCallArgs(_),
        EventType::ToolCallArgs
    );
    factory_test!(
        create_tool_call_end_event_builds_variant,
        create_tool_call_end_event("tc1", None, None),
        Event::ToolCallEnd(_),
        EventType::ToolCallEnd
    );
    factory_test!(
        create_tool_call_chunk_event_builds_variant,
        create_tool_call_chunk_event(
            Some("tc1".into()),
            Some("search".into()),
            Some("m1".into()),
            Some("}".into()),
            None,
            None
        ),
        Event::ToolCallChunk(_),
        EventType::ToolCallChunk
    );
    factory_test!(
        create_tool_call_result_event_builds_variant,
        create_tool_call_result_event("m1", "tc1", "ok", Some(ToolResultRole::Tool), None, None),
        Event::ToolCallResult(_),
        EventType::ToolCallResult
    );
    factory_test!(
        create_thinking_start_event_builds_variant,
        create_thinking_start_event(Some("plan".into()), None, None),
        Event::ThinkingStart(_),
        EventType::ThinkingStart
    );
    factory_test!(
        create_thinking_end_event_builds_variant,
        create_thinking_end_event(None, None),
        Event::ThinkingEnd(_),
        EventType::ThinkingEnd
    );
    factory_test!(
        create_state_snapshot_event_builds_variant,
        create_state_snapshot_event(json!({"x": 1}), None, None),
        Event::StateSnapshot(_),
        EventType::StateSnapshot
    );
    factory_test!(
        create_state_delta_event_builds_variant,
        create_state_delta_event(
            vec![json!({"op": "replace", "path": "/x", "value": 2})],
            None,
            None
        ),
        Event::StateDelta(_),
        EventType::StateDelta
    );
    factory_test!(
        create_messages_snapshot_event_builds_variant,
        create_messages_snapshot_event(
            vec![Message::Assistant(AssistantMessage {
                id: "a1".into(),
                content: Some("hello".into()),
                name: None,
                tool_calls: None,
                encrypted_value: None
            })],
            None,
            None
        ),
        Event::MessagesSnapshot(_),
        EventType::MessagesSnapshot
    );
    factory_test!(
        create_activity_snapshot_event_builds_variant,
        create_activity_snapshot_event("a1", "plan", serde_json::Map::new(), None, None, None),
        Event::ActivitySnapshot(_),
        EventType::ActivitySnapshot
    );
    factory_test!(
        create_activity_delta_event_builds_variant,
        create_activity_delta_event(
            "a1",
            "plan",
            vec![json!({"op": "add", "path": "/0", "value": "x"})],
            None,
            None
        ),
        Event::ActivityDelta(_),
        EventType::ActivityDelta
    );
    factory_test!(
        create_raw_event_builds_variant,
        create_raw_event(
            json!({"provider": "test"}),
            Some("source".into()),
            None,
            None
        ),
        Event::Raw(_),
        EventType::Raw
    );
    factory_test!(
        create_custom_event_builds_variant,
        create_custom_event("my-event", json!({"x": 1}), None, None),
        Event::Custom(_),
        EventType::Custom
    );
    factory_test!(
        create_run_started_event_builds_variant,
        create_run_started_event(
            "t1",
            "r1",
            Some("p1".into()),
            Some(RunAgentInput::new("t1", "r1")),
            None,
            None
        ),
        Event::RunStarted(_),
        EventType::RunStarted
    );
    factory_test!(
        create_run_finished_event_builds_variant,
        create_run_finished_event("t1", "r1", Some(json!({"done": true})), None, None, None),
        Event::RunFinished(_),
        EventType::RunFinished
    );
    factory_test!(
        create_run_finished_success_event_builds_variant,
        create_run_finished_success_event("t1", "r1", None, None, None),
        Event::RunFinished(_),
        EventType::RunFinished
    );
    factory_test!(
        create_run_finished_interrupt_event_builds_variant,
        create_run_finished_interrupt_event(
            "t1",
            "r1",
            None,
            vec![Interrupt {
                id: "i1".into(),
                reason: "approval".into(),
                message: None,
                tool_call_id: None,
                response_schema: None,
                expires_at: None,
                metadata: None
            }],
            None,
            None
        ),
        Event::RunFinished(_),
        EventType::RunFinished
    );
    factory_test!(
        create_run_error_event_builds_variant,
        create_run_error_event("boom", Some("E_FAIL".into()), None, None),
        Event::RunError(_),
        EventType::RunError
    );
    factory_test!(
        create_step_started_event_builds_variant,
        create_step_started_event("plan", None, None),
        Event::StepStarted(_),
        EventType::StepStarted
    );
    factory_test!(
        create_step_finished_event_builds_variant,
        create_step_finished_event("plan", None, None),
        Event::StepFinished(_),
        EventType::StepFinished
    );
    factory_test!(
        create_reasoning_start_event_builds_variant,
        create_reasoning_start_event("r1", None, None),
        Event::ReasoningStart(_),
        EventType::ReasoningStart
    );
    factory_test!(
        create_reasoning_message_start_event_builds_variant,
        create_reasoning_message_start_event("r1", None, None, None),
        Event::ReasoningMessageStart(_),
        EventType::ReasoningMessageStart
    );
    factory_test!(
        create_reasoning_message_content_event_builds_variant,
        create_reasoning_message_content_event("r1", "step", None, None),
        Event::ReasoningMessageContent(_),
        EventType::ReasoningMessageContent
    );
    factory_test!(
        create_reasoning_message_end_event_builds_variant,
        create_reasoning_message_end_event("r1", None, None),
        Event::ReasoningMessageEnd(_),
        EventType::ReasoningMessageEnd
    );
    factory_test!(
        create_reasoning_message_chunk_event_builds_variant,
        create_reasoning_message_chunk_event(Some("r1".into()), Some("chunk".into()), None, None),
        Event::ReasoningMessageChunk(_),
        EventType::ReasoningMessageChunk
    );
    factory_test!(
        create_reasoning_end_event_builds_variant,
        create_reasoning_end_event("r1", None, None),
        Event::ReasoningEnd(_),
        EventType::ReasoningEnd
    );
    factory_test!(
        create_reasoning_encrypted_value_event_builds_variant,
        create_reasoning_encrypted_value_event(
            ReasoningEncryptedValueSubtype::ToolCall,
            "tc1",
            "cipher",
            None,
            None
        ),
        Event::ReasoningEncryptedValue(_),
        EventType::ReasoningEncryptedValue
    );

    #[test]
    fn create_text_message_start_event_defaults_role() {
        let event = create_text_message_start_event("m1", None, None, Some(42), None);
        match event {
            Event::TextMessageStart(event) => {
                assert_eq!(event.role, TextMessageRole::Assistant);
                assert_eq!(event.base.timestamp, Some(42));
            }
            _ => panic!("expected text message start"),
        }
    }

    #[test]
    fn create_activity_snapshot_event_defaults_replace_true() {
        let event = create_activity_snapshot_event(
            "a1",
            "plan",
            serde_json::Map::new(),
            None,
            Some(42),
            None,
        );
        match event {
            Event::ActivitySnapshot(event) => {
                assert!(event.replace);
                assert_eq!(event.base.timestamp, Some(42));
            }
            _ => panic!("expected activity snapshot"),
        }
    }

    #[test]
    fn create_run_finished_success_event_sets_success_outcome() {
        let event = create_run_finished_success_event("t1", "r1", None, Some(42), None);
        match event {
            Event::RunFinished(event) => {
                assert_eq!(event.outcome, Some(RunFinishedOutcome::Success));
                assert_eq!(event.base.timestamp, Some(42));
            }
            _ => panic!("expected run finished"),
        }
    }

    #[test]
    fn create_run_finished_interrupt_event_sets_interrupt_outcome() {
        let event = create_run_finished_interrupt_event(
            "t1",
            "r1",
            None,
            vec![Interrupt {
                id: "i1".into(),
                reason: "approval".into(),
                message: None,
                tool_call_id: None,
                response_schema: None,
                expires_at: None,
                metadata: None,
            }],
            Some(42),
            Some(json!({"raw": true})),
        );
        match event {
            Event::RunFinished(event) => match event.outcome {
                Some(RunFinishedOutcome::Interrupt { interrupts }) => {
                    assert_eq!(interrupts.len(), 1);
                    assert_eq!(event.base.raw_event, Some(json!({"raw": true})));
                }
                _ => panic!("expected interrupt outcome"),
            },
            _ => panic!("expected run finished"),
        }
    }

    #[test]
    fn create_messages_snapshot_event_accepts_binary_parts() {
        let event = create_messages_snapshot_event(
            vec![Message::User(UserMessage {
                id: "u1".into(),
                content: UserMessageContent::Parts(vec![InputContent::Binary {
                    content: BinaryInputContent {
                        mime_type: "application/octet-stream".into(),
                        id: Some("blob-1".into()),
                        url: None,
                        data: None,
                        filename: None,
                    },
                }]),
                name: None,
                encrypted_value: None,
            })],
            Some(42),
            None,
        );

        match event {
            Event::MessagesSnapshot(event) => {
                assert_eq!(event.messages.len(), 1);
                assert_eq!(event.base.timestamp, Some(42));
            }
            _ => panic!("expected messages snapshot"),
        }
    }
}
