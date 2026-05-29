use agui_rs_client::verify_events;
use agui_rs_core::*;
use futures::{stream, StreamExt};
use serde_json::json;

async fn collect(events: Vec<Event>) -> Vec<Result<Event>> {
    let input = stream::iter(events.into_iter().map(Ok));
    verify_events(Box::pin(input)).collect().await
}

fn assert_validation(result: &Result<Event>, expected: &str) {
    match result {
        Err(AgUiError::Validation(message)) => {
            assert!(
                message.contains(expected),
                "expected '{expected}' in '{message}'"
            );
        }
        other => panic!("expected validation error, got {other:?}"),
    }
}

fn raw_event() -> Event {
    Event::Raw(RawEvent {
        event: json!({ "type": "raw_data", "content": "test" }),
        source: None,
        base: BaseEventFields::default(),
    })
}

fn custom_event() -> Event {
    Event::Custom(CustomEvent {
        name: "test_event".into(),
        value: json!("test_value"),
        base: BaseEventFields::default(),
    })
}

fn messages_snapshot_event() -> Event {
    Event::MessagesSnapshot(MessagesSnapshotEvent {
        messages: vec![],
        base: BaseEventFields::default(),
    })
}

fn tool_call_start_with_parent(tool_call_id: &str, parent_message_id: &str) -> Event {
    Event::ToolCallStart(ToolCallStartEvent {
        tool_call_id: tool_call_id.into(),
        tool_call_name: "search".into(),
        parent_message_id: Some(parent_message_id.into()),
        base: BaseEventFields::default(),
    })
}

fn tool_call_result(message_id: &str, tool_call_id: &str, content: &str) -> Event {
    Event::ToolCallResult(ToolCallResultEvent {
        message_id: message_id.into(),
        tool_call_id: tool_call_id.into(),
        content: content.into(),
        role: Some(ToolResultRole::Tool),
        base: BaseEventFields::default(),
    })
}

#[tokio::test]
async fn tool_call_args_before_start_errors() {
    let out = collect(vec![
        factory::run_started("thread", "run"),
        factory::tool_call_args("tc1", "{}"),
    ])
    .await;

    assert_validation(
        &out[1],
        "Cannot send 'TOOL_CALL_ARGS' event: No active tool call found with ID 'tc1'",
    );
}

#[tokio::test]
async fn tool_call_end_before_start_errors() {
    let out = collect(vec![
        factory::run_started("thread", "run"),
        factory::tool_call_end("tc1"),
    ])
    .await;

    assert_validation(
        &out[1],
        "Cannot send 'TOOL_CALL_END' event: No active tool call found with ID 'tc1'",
    );
}

#[tokio::test]
async fn second_tool_call_start_while_active_errors() {
    let out = collect(vec![
        factory::run_started("thread", "run"),
        factory::tool_call_start("tc1", "search"),
        factory::tool_call_start("tc2", "calculate"),
    ])
    .await;

    assert_validation(&out[2], "A tool call with ID 'tc1' is already in progress");
}

#[tokio::test]
async fn mismatched_tool_call_args_id_errors() {
    let out = collect(vec![
        factory::run_started("thread", "run"),
        factory::tool_call_start("tc1", "search"),
        factory::tool_call_args("tc2", "{}"),
    ])
    .await;

    assert_validation(
        &out[2],
        "Cannot send 'TOOL_CALL_ARGS' event: No active tool call found with ID 'tc2'",
    );
}

#[tokio::test]
async fn tool_call_start_with_non_active_parent_message_errors() {
    let out = collect(vec![
        factory::run_started("thread", "run"),
        tool_call_start_with_parent("tc1", "m1"),
    ])
    .await;

    assert_validation(
        &out[1],
        "Parent message ID 'm1' does not reference an active text message",
    );
}

#[tokio::test]
async fn tool_call_start_with_mismatched_active_parent_message_errors() {
    let out = collect(vec![
        factory::run_started("thread", "run"),
        factory::text_message_start("m1"),
        tool_call_start_with_parent("tc1", "m2"),
    ])
    .await;

    assert_validation(
        &out[2],
        "Parent message ID 'm2' must match the active text message ID 'm1'",
    );
}

#[tokio::test]
async fn raw_custom_state_messages_and_steps_inside_tool_call_pass() {
    let out = collect(vec![
        factory::run_started("thread", "run"),
        factory::tool_call_start("tc1", "search"),
        factory::step_started("test-step"),
        factory::tool_call_args("tc1", "{"),
        raw_event(),
        custom_event(),
        factory::state_snapshot(json!({})),
        factory::state_delta(vec![
            json!({ "op": "add", "path": "/result", "value": "success" }),
        ]),
        messages_snapshot_event(),
        factory::step_finished("test-step"),
        factory::tool_call_end("tc1"),
        factory::run_finished("thread", "run"),
    ])
    .await;

    assert_eq!(out.len(), 12);
    assert!(out.iter().all(|item| item.is_ok()));
}

#[tokio::test]
async fn text_message_can_run_inside_active_tool_call() {
    let out = collect(vec![
        factory::run_started("thread", "run"),
        factory::tool_call_start("tc1", "search"),
        factory::tool_call_args("tc1", "Preparing..."),
        factory::text_message_start("m1"),
        factory::text_message_content("m1", "Tool is processing..."),
        factory::text_message_end("m1"),
        factory::tool_call_args("tc1", "Completed."),
        factory::tool_call_end("tc1"),
        factory::run_finished("thread", "run"),
    ])
    .await;

    assert_eq!(out.len(), 9);
    assert!(out.iter().all(|item| item.is_ok()));
}

#[tokio::test]
async fn multiple_args_then_end_and_result_pass() {
    let out = collect(vec![
        factory::run_started("thread", "run"),
        factory::tool_call_start("tc1", "test-tool"),
        factory::tool_call_args("tc1", "test args 1"),
        factory::tool_call_args("tc1", "test args 2"),
        factory::tool_call_end("tc1"),
        tool_call_result("m1", "tc1", "done"),
        factory::run_finished("thread", "run"),
    ])
    .await;

    assert_eq!(out.len(), 7);
    assert!(out.iter().all(|item| item.is_ok()));
}

#[tokio::test]
async fn multiple_sequential_tool_calls_pass() {
    let out = collect(vec![
        factory::run_started("thread", "run"),
        factory::tool_call_start("tc1", "search"),
        factory::tool_call_args("tc1", "{\"query\":\"test\"}"),
        factory::tool_call_end("tc1"),
        factory::tool_call_start("tc2", "calculate"),
        factory::tool_call_args("tc2", "{\"expression\":\"1+1\"}"),
        factory::tool_call_end("tc2"),
        factory::run_finished("thread", "run"),
    ])
    .await;

    assert_eq!(out.len(), 8);
    assert!(out.iter().all(|item| item.is_ok()));
}

#[tokio::test]
async fn tool_call_at_run_boundaries_passes() {
    let out = collect(vec![
        factory::run_started("thread", "run"),
        factory::tool_call_start("tc1", "test-tool"),
        factory::tool_call_args("tc1", "test args"),
        factory::tool_call_end("tc1"),
        factory::run_finished("thread", "run"),
    ])
    .await;

    assert_eq!(out.len(), 5);
    assert!(out.iter().all(|item| item.is_ok()));
}

#[tokio::test]
async fn starting_tool_call_before_run_started_errors() {
    let out = collect(vec![factory::tool_call_start("tc1", "test-tool")]).await;

    assert_eq!(out.len(), 1);
    assert_validation(&out[0], "First event must be 'RUN_STARTED'");
}

#[tokio::test]
async fn run_finished_with_active_tool_call_errors() {
    let out = collect(vec![
        factory::run_started("thread", "run"),
        factory::tool_call_start("tc1", "search"),
        factory::run_finished("thread", "run"),
    ])
    .await;

    assert_validation(
        &out[2],
        "Cannot send 'RUN_FINISHED' while tool call 'tc1' is still active",
    );
}
