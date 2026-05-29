type Result<T> = agui_rs_client::Result<T>;

#[path = "../src/legacy.rs"]
mod legacy_impl;

use agui_rs_core::Event;
use futures::{stream, StreamExt};
use legacy_impl::{
    convert_legacy_events, LegacyActionExecutionArgs, LegacyActionExecutionEnd,
    LegacyActionExecutionResult, LegacyActionExecutionStart, LegacyAgentStateMessage, LegacyEvent,
    LegacyMetaEvent, LegacyMetaEventName, LegacyRunError, LegacyTextMessageContent,
    LegacyTextMessageEnd, LegacyTextMessageStart, LegacyThinkingEnd, LegacyThinkingStart,
    LegacyThinkingTextMessageContent, LegacyThinkingTextMessageEnd, LegacyThinkingTextMessageStart,
};
use serde_json::json;

async fn collect(events: Vec<LegacyEvent>) -> Vec<agui_rs_client::Result<Event>> {
    convert_legacy_events(stream::iter(events)).collect().await
}

#[tokio::test]
async fn converts_text_message_legacy_events_to_ag_ui_events() {
    let events = collect(vec![
        LegacyEvent::TextMessageStart(LegacyTextMessageStart {
            message_id: "m1".into(),
            parent_message_id: None,
            role: Some("assistant".into()),
        }),
        LegacyEvent::TextMessageContent(LegacyTextMessageContent {
            message_id: "m1".into(),
            content: "hello".into(),
        }),
        LegacyEvent::TextMessageEnd(LegacyTextMessageEnd {
            message_id: "m1".into(),
        }),
    ])
    .await;

    assert!(
        matches!(&events[0], Ok(event) if *event == agui_rs_core::factory::text_message_start("m1"))
    );
    assert!(
        matches!(&events[1], Ok(event) if *event == agui_rs_core::factory::text_message_content("m1", "hello"))
    );
    assert!(
        matches!(&events[2], Ok(event) if *event == agui_rs_core::factory::text_message_end("m1"))
    );
}

#[tokio::test]
async fn converts_tool_call_legacy_events_and_preserves_result_payload() {
    let events = collect(vec![
        LegacyEvent::ActionExecutionStart(LegacyActionExecutionStart {
            action_execution_id: "tc1".into(),
            action_name: "search".into(),
            parent_message_id: Some("m1".into()),
        }),
        LegacyEvent::ActionExecutionArgs(LegacyActionExecutionArgs {
            action_execution_id: "tc1".into(),
            args: "{}".into(),
        }),
        LegacyEvent::ActionExecutionEnd(LegacyActionExecutionEnd {
            action_execution_id: "tc1".into(),
        }),
        LegacyEvent::ActionExecutionResult(LegacyActionExecutionResult {
            action_name: "search".into(),
            action_execution_id: "tc1".into(),
            result: "ok".into(),
        }),
    ])
    .await;

    assert!(
        matches!(&events[0], Ok(Event::ToolCallStart(start)) if start.tool_call_id == "tc1" && start.tool_call_name == "search" && start.parent_message_id.as_deref() == Some("m1"))
    );
    assert!(
        matches!(&events[1], Ok(event) if *event == agui_rs_core::factory::tool_call_args("tc1", "{}"))
    );
    assert!(
        matches!(&events[2], Ok(event) if *event == agui_rs_core::factory::tool_call_end("tc1"))
    );
    assert!(
        matches!(&events[3], Ok(Event::ToolCallResult(result)) if result.tool_call_id == "tc1" && result.content == "ok")
    );
}

#[tokio::test]
async fn converts_thinking_legacy_events_to_reasoning_events() {
    let events = collect(vec![
        LegacyEvent::ThinkingStart(LegacyThinkingStart::default()),
        LegacyEvent::ThinkingTextMessageStart(LegacyThinkingTextMessageStart::default()),
        LegacyEvent::ThinkingTextMessageContent(LegacyThinkingTextMessageContent {
            delta: "plan".into(),
        }),
        LegacyEvent::ThinkingTextMessageEnd(LegacyThinkingTextMessageEnd::default()),
        LegacyEvent::ThinkingEnd(LegacyThinkingEnd::default()),
    ])
    .await;

    let reasoning_start_id = match &events[0] {
        Ok(Event::ReasoningStart(event)) => event.message_id.clone(),
        other => panic!("unexpected event: {other:?}"),
    };
    let reasoning_message_id = match &events[1] {
        Ok(Event::ReasoningMessageStart(event)) => event.message_id.clone(),
        other => panic!("unexpected event: {other:?}"),
    };

    assert_ne!(reasoning_start_id, reasoning_message_id);
    assert!(
        matches!(&events[2], Ok(Event::ReasoningMessageContent(event)) if event.message_id == reasoning_message_id && event.delta == "plan")
    );
    assert!(
        matches!(&events[3], Ok(Event::ReasoningMessageEnd(event)) if event.message_id == reasoning_message_id)
    );
    assert!(
        matches!(&events[4], Ok(Event::ReasoningEnd(event)) if event.message_id == reasoning_start_id)
    );
}

#[tokio::test]
async fn converts_agent_state_meta_and_run_error_legacy_events() {
    let events = collect(vec![
        LegacyEvent::AgentStateMessage(LegacyAgentStateMessage {
            state: json!({"count": 1}).to_string(),
        }),
        LegacyEvent::MetaEvent(LegacyMetaEvent {
            name: LegacyMetaEventName::PredictState,
            value: json!({"tool": "search"}),
        }),
        LegacyEvent::RunError(LegacyRunError {
            message: "boom".into(),
            code: Some("E_BOOM".into()),
        }),
    ])
    .await;

    assert!(
        matches!(&events[0], Ok(Event::StateSnapshot(snapshot)) if snapshot.snapshot == json!({"count": 1}))
    );
    assert!(matches!(&events[1], Ok(Event::Custom(custom)) if custom.name == "PredictState"));
    assert!(
        matches!(&events[2], Ok(Event::RunError(error)) if error.message == "boom" && error.code.as_deref() == Some("E_BOOM"))
    );
}

#[tokio::test]
async fn invalid_agent_state_message_returns_error() {
    let events = collect(vec![LegacyEvent::AgentStateMessage(
        LegacyAgentStateMessage {
            state: "not-json".into(),
        },
    )])
    .await;

    assert!(events[0].is_err());
}

// SKIPPED: TypeScript legacy_to_be_removed_runAgentBridged API does not exist in the Rust SDK; Rust exposes convert_legacy_events instead.
// SKIPPED: configuration passthrough tests for the legacy bridged AbstractAgent method cannot be ported because that method is absent in Rust.
