use ag_ui_client::verify_events;
use ag_ui_core::*;
use futures::{stream, StreamExt};

async fn collect(events: Vec<Event>) -> Vec<Result<Event>> {
    let input = stream::iter(events.into_iter().map(Ok));
    verify_events(Box::pin(input)).collect().await
}

fn assert_validation(result: &Result<Event>, expected: &str) {
    match result {
        Err(AgUiError::Validation(message)) => {
            assert!(message.contains(expected), "expected '{expected}' in '{message}'");
        }
        other => panic!("expected validation error, got {other:?}"),
    }
}

#[tokio::test]
async fn concurrent_text_messages_are_rejected_by_single_active_message_model() {
    let out = collect(vec![
        factory::run_started("thread", "run"),
        factory::text_message_start("msg1"),
        factory::text_message_start("msg2"),
    ])
    .await;

    assert_eq!(out.len(), 3);
    assert_validation(&out[2], "Cannot send 'TEXT_MESSAGE_START' event: A text message with ID 'msg1' is already in progress");
}

#[tokio::test]
async fn concurrent_tool_calls_are_rejected_by_single_active_tool_call_model() {
    let out = collect(vec![
        factory::run_started("thread", "run"),
        factory::tool_call_start("tool1", "search"),
        factory::tool_call_start("tool2", "calculate"),
    ])
    .await;

    assert_eq!(out.len(), 3);
    assert_validation(&out[2], "Cannot send 'TOOL_CALL_START' event: A tool call with ID 'tool1' is already in progress");
}

#[tokio::test]
async fn overlapping_one_text_message_and_one_tool_call_passes() {
    let out = collect(vec![
        factory::run_started("thread", "run"),
        factory::text_message_start("msg1"),
        factory::tool_call_start("tool1", "search"),
        factory::text_message_content("msg1", "Thinking..."),
        factory::tool_call_args("tool1", "{\"query\":\"test\"}"),
        factory::tool_call_end("tool1"),
        factory::text_message_end("msg1"),
        factory::run_finished("thread", "run"),
    ])
    .await;

    assert_eq!(out.len(), 8);
    assert!(out.iter().all(|item| item.is_ok()));
}

#[tokio::test]
async fn second_text_message_cannot_start_while_first_text_message_and_tool_call_are_active() {
    let out = collect(vec![
        factory::run_started("thread", "run"),
        factory::text_message_start("msg1"),
        factory::tool_call_start("tool1", "search"),
        factory::text_message_content("msg1", "Thinking..."),
        factory::tool_call_args("tool1", "{\"query\":\"test\"}"),
        factory::text_message_start("msg2"),
    ])
    .await;

    assert_validation(&out[5], "Cannot send 'TEXT_MESSAGE_START' event: A text message with ID 'msg1' is already in progress");
}

#[tokio::test]
async fn lifecycle_events_during_overlapping_message_and_tool_call_pass() {
    let out = collect(vec![
        factory::run_started("thread", "run"),
        factory::step_started("search_step"),
        factory::text_message_start("msg1"),
        factory::tool_call_start("tool1", "search"),
        factory::step_started("analysis_step"),
        factory::text_message_content("msg1", "Searching..."),
        factory::tool_call_args("tool1", "{\"query\":\"test\"}"),
        factory::text_message_end("msg1"),
        factory::tool_call_end("tool1"),
        factory::step_finished("analysis_step"),
        factory::step_finished("search_step"),
        factory::run_finished("thread", "run"),
    ])
    .await;

    assert_eq!(out.len(), 12);
    assert!(out.iter().all(|item| item.is_ok()));
}

#[tokio::test]
async fn duplicate_message_id_start_errors() {
    let out = collect(vec![
        factory::run_started("thread", "run"),
        factory::text_message_start("msg1"),
        factory::text_message_start("msg1"),
    ])
    .await;

    assert_validation(&out[2], "Cannot send 'TEXT_MESSAGE_START' event: A text message with ID 'msg1' is already in progress");
}

#[tokio::test]
async fn duplicate_tool_call_id_start_errors() {
    let out = collect(vec![
        factory::run_started("thread", "run"),
        factory::tool_call_start("tool1", "search"),
        factory::tool_call_start("tool1", "calculate"),
    ])
    .await;

    assert_validation(&out[2], "Cannot send 'TOOL_CALL_START' event: A tool call with ID 'tool1' is already in progress");
}

#[tokio::test]
async fn content_for_nonexistent_message_id_errors() {
    let out = collect(vec![
        factory::run_started("thread", "run"),
        factory::text_message_content("nonexistent", "test content"),
    ])
    .await;

    assert_validation(&out[1], "Cannot send 'TEXT_MESSAGE_CONTENT' event: No active text message found with ID 'nonexistent'");
}

#[tokio::test]
async fn args_for_nonexistent_tool_call_id_errors() {
    let out = collect(vec![
        factory::run_started("thread", "run"),
        factory::tool_call_args("nonexistent", "{\"test\":\"value\"}"),
    ])
    .await;

    assert_validation(&out[1], "Cannot send 'TOOL_CALL_ARGS' event: No active tool call found with ID 'nonexistent'");
}

#[tokio::test]
async fn run_finished_while_text_message_is_active_errors() {
    let out = collect(vec![
        factory::run_started("thread", "run"),
        factory::text_message_start("msg1"),
        factory::run_finished("thread", "run"),
    ])
    .await;

    assert_validation(&out[2], "Cannot send 'RUN_FINISHED' while text message 'msg1' is still active");
}

#[tokio::test]
async fn run_finished_while_tool_call_is_active_errors() {
    let out = collect(vec![
        factory::run_started("thread", "run"),
        factory::tool_call_start("tool1", "search"),
        factory::run_finished("thread", "run"),
    ])
    .await;

    assert_validation(&out[2], "Cannot send 'RUN_FINISHED' while tool call 'tool1' is still active");
}

#[tokio::test]
async fn complex_many_overlapping_events_fail_at_first_unsupported_concurrent_start() {
    let out = collect(vec![
        factory::run_started("thread", "run"),
        factory::text_message_start("msg1"),
        factory::text_message_start("msg2"),
        factory::tool_call_start("tool1", "test_tool"),
    ])
    .await;

    assert_eq!(out.len(), 3);
    assert_validation(&out[2], "Cannot send 'TEXT_MESSAGE_START' event: A text message with ID 'msg1' is already in progress");
}
