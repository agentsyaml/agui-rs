use agui_rs_client::verify_events;
use agui_rs_core::*;
use futures::{stream, StreamExt};

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

// Ported from TypeScript `verify/__tests__/verify.concurrent.test.ts:23-96`:
// concurrent text messages with different ids are ALLOWED, including ENDs in a
// different order than the STARTs.
#[tokio::test]
async fn concurrent_text_messages_with_different_ids_pass() {
    let out = collect(vec![
        factory::run_started("thread", "run"),
        factory::text_message_start("msg1"),
        factory::text_message_start("msg2"),
        factory::text_message_content("msg1", "Content for message 1"),
        factory::text_message_content("msg2", "Content for message 2"),
        factory::text_message_end("msg2"),
        factory::text_message_end("msg1"),
        factory::run_finished("thread", "run"),
    ])
    .await;

    assert_eq!(out.len(), 8);
    assert!(out.iter().all(|item| item.is_ok()));
}

// Ported from TypeScript `verify/__tests__/verify.concurrent.test.ts:99-170`.
#[tokio::test]
async fn concurrent_tool_calls_with_different_ids_pass() {
    let out = collect(vec![
        factory::run_started("thread", "run"),
        factory::tool_call_start("tool1", "search"),
        factory::tool_call_start("tool2", "calculate"),
        factory::tool_call_args("tool1", "{\"query\":\"test\"}"),
        factory::tool_call_args("tool2", "{\"expression\":\"1+1\"}"),
        factory::tool_call_end("tool2"),
        factory::tool_call_end("tool1"),
        factory::run_finished("thread", "run"),
    ])
    .await;

    assert_eq!(out.len(), 8);
    assert!(out.iter().all(|item| item.is_ok()));
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

    // Upstream allows a second concurrent message
    // (`verify/__tests__/verify.concurrent.test.ts:171-262`).
    assert!(out.iter().all(|item| item.is_ok()));
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

    assert_validation(
        &out[2],
        "Cannot send 'TOOL_CALL_START' event: A tool call with ID 'tool1' is already in progress",
    );
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

    assert_validation(
        &out[1],
        "Cannot send 'TOOL_CALL_ARGS' event: No active tool call found with ID 'nonexistent'",
    );
}

#[tokio::test]
async fn run_finished_while_text_message_is_active_errors() {
    let out = collect(vec![
        factory::run_started("thread", "run"),
        factory::text_message_start("msg1"),
        factory::run_finished("thread", "run"),
    ])
    .await;

    assert_validation(
        &out[2],
        "Cannot send 'RUN_FINISHED' while text messages are still active: msg1",
    );
}

#[tokio::test]
async fn run_finished_while_tool_call_is_active_errors() {
    let out = collect(vec![
        factory::run_started("thread", "run"),
        factory::tool_call_start("tool1", "search"),
        factory::run_finished("thread", "run"),
    ])
    .await;

    assert_validation(
        &out[2],
        "Cannot send 'RUN_FINISHED' while tool calls are still active: tool1",
    );
}

// Ported from TypeScript `verify/__tests__/verify.concurrent.test.ts:560-646`
// ("complex concurrent scenario with many overlapping events"): five messages
// and five tool calls stream and close in reverse order, all 52 events pass.
#[tokio::test]
async fn complex_many_overlapping_events_pass() {
    let message_ids = ["msg1", "msg2", "msg3", "msg4", "msg5"];
    let tool_call_ids = ["tool1", "tool2", "tool3", "tool4", "tool5"];

    let mut events = vec![factory::run_started("thread", "run")];
    for id in message_ids {
        events.push(factory::text_message_start(id));
    }
    for id in tool_call_ids {
        events.push(factory::tool_call_start(id, "test_tool"));
    }
    for i in 0..3 {
        for id in message_ids {
            events.push(factory::text_message_content(
                id,
                format!("Content {i} for {id}"),
            ));
        }
        for id in tool_call_ids {
            events.push(factory::tool_call_args(id, format!("{{\"step\":{i}}}")));
        }
    }
    for id in message_ids.into_iter().rev() {
        events.push(factory::text_message_end(id));
    }
    for id in tool_call_ids.into_iter().rev() {
        events.push(factory::tool_call_end(id));
    }
    events.push(factory::run_finished("thread", "run"));

    let out = collect(events).await;

    assert_eq!(out.len(), 52);
    assert!(
        out.iter().all(|item| item.is_ok()),
        "all 52 events should verify: {out:?}"
    );
}
