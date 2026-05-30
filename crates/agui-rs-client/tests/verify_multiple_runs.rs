use agui_rs_client::verify_events;
use agui_rs_core::{factory, AgUiError, Event};
use futures::{stream, StreamExt};

async fn collect(events: Vec<Event>) -> Vec<Result<Event, AgUiError>> {
    verify_events(stream::iter(events.into_iter().map(Ok)))
        .collect::<Vec<_>>()
        .await
}

fn assert_validation(result: &Result<Event, AgUiError>, expected: &str) {
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

#[tokio::test]
async fn rejects_new_run_started_while_current_run_is_active() {
    let out = collect(vec![
        factory::run_started("thread", "run-1"),
        factory::run_started("thread", "run-2"),
    ])
    .await;

    assert_eq!(out.len(), 2);
    assert!(out[0].is_ok());
    assert_validation(
        &out[1],
        "Cannot send 'RUN_STARTED' while a run is still active",
    );
}

#[tokio::test]
async fn run_error_blocks_subsequent_events_within_the_same_run() {
    let out = collect(vec![
        factory::run_started("thread", "run-1"),
        factory::run_error("Test error"),
        factory::text_message_start("msg-1"),
    ])
    .await;

    assert_eq!(out.len(), 3);
    assert!(out[..2].iter().all(Result::is_ok));
    assert_validation(
        &out[2],
        "Cannot send event type 'TEXT_MESSAGE_START': The run has already errored with 'RUN_ERROR'",
    );
}

// Ported from TypeScript `verify/__tests__/verify.multiple-runs.test.ts`.
// `verify_events` now supports multiple sequential runs: a new RUN_STARTED
// after RUN_FINISHED resets per-run state, matching the canonical state machine.

#[tokio::test]
async fn allows_multiple_sequential_runs() {
    let out = collect(vec![
        factory::run_started("test-thread-1", "test-run-1"),
        factory::text_message_start("msg-1"),
        factory::text_message_content("msg-1", "Hello from run 1"),
        factory::text_message_end("msg-1"),
        factory::run_finished("test-thread-1", "test-run-1"),
        factory::run_started("test-thread-1", "test-run-2"),
        factory::text_message_start("msg-2"),
        factory::text_message_content("msg-2", "Hello from run 2"),
        factory::text_message_end("msg-2"),
        factory::run_finished("test-thread-1", "test-run-2"),
    ])
    .await;

    assert_eq!(out.len(), 10);
    assert!(out.iter().all(Result::is_ok), "all events should verify: {out:?}");
    assert!(matches!(out[0], Ok(Event::RunStarted(_))));
    assert!(matches!(out[4], Ok(Event::RunFinished(_))));
    assert!(matches!(out[5], Ok(Event::RunStarted(_))));
    assert!(matches!(out[9], Ok(Event::RunFinished(_))));
}

#[tokio::test]
async fn allows_reusing_message_ids_across_different_runs() {
    let out = collect(vec![
        factory::run_started("test-thread-1", "test-run-1"),
        factory::text_message_start("msg-1"),
        factory::text_message_end("msg-1"),
        factory::run_finished("test-thread-1", "test-run-1"),
        factory::run_started("test-thread-1", "test-run-2"),
        factory::text_message_start("msg-1"),
        factory::text_message_end("msg-1"),
        factory::run_finished("test-thread-1", "test-run-2"),
    ])
    .await;

    assert_eq!(out.len(), 8);
    assert!(out.iter().all(Result::is_ok), "all events should verify: {out:?}");
}

#[tokio::test]
async fn allows_multiple_runs_with_tool_calls() {
    let out = collect(vec![
        factory::run_started("test-thread-1", "test-run-1"),
        factory::tool_call_start("tool-1", "calculator"),
        factory::tool_call_args("tool-1", "{\"a\": 1, \"b\": 2}"),
        factory::tool_call_end("tool-1"),
        factory::run_finished("test-thread-1", "test-run-1"),
        factory::run_started("test-thread-1", "test-run-2"),
        factory::tool_call_start("tool-1", "weather"),
        factory::tool_call_args("tool-1", "{\"city\": \"NYC\"}"),
        factory::tool_call_end("tool-1"),
        factory::run_finished("test-thread-1", "test-run-2"),
    ])
    .await;

    assert_eq!(out.len(), 10);
    assert!(out.iter().all(Result::is_ok), "all events should verify: {out:?}");
}

#[tokio::test]
async fn allows_multiple_runs_with_steps() {
    let out = collect(vec![
        factory::run_started("test-thread-1", "test-run-1"),
        factory::step_started("planning"),
        factory::step_finished("planning"),
        factory::run_finished("test-thread-1", "test-run-1"),
        factory::run_started("test-thread-1", "test-run-2"),
        factory::step_started("planning"),
        factory::step_finished("planning"),
        factory::run_finished("test-thread-1", "test-run-2"),
    ])
    .await;

    assert_eq!(out.len(), 8);
    assert!(out.iter().all(Result::is_ok), "all events should verify: {out:?}");
}

#[tokio::test]
async fn allows_three_sequential_runs() {
    let mut events = Vec::new();
    for i in 1..=3 {
        events.push(factory::run_started("test-thread-1", format!("test-run-{i}")));
        events.push(factory::text_message_start(format!("msg-{i}")));
        events.push(factory::text_message_content(
            format!("msg-{i}"),
            format!("Message from run {i}"),
        ));
        events.push(factory::text_message_end(format!("msg-{i}")));
        events.push(factory::run_finished("test-thread-1", format!("test-run-{i}")));
    }

    let out = collect(events).await;

    assert_eq!(out.len(), 15);
    assert!(out.iter().all(Result::is_ok), "all events should verify: {out:?}");
    assert!(matches!(out[0], Ok(Event::RunStarted(_))));
    assert!(matches!(out[5], Ok(Event::RunStarted(_))));
    assert!(matches!(out[10], Ok(Event::RunStarted(_))));
}

#[tokio::test]
async fn handles_complex_scenario_with_multiple_runs_and_various_event_types() {
    let out = collect(vec![
        factory::run_started("test-thread-1", "test-run-1"),
        factory::text_message_start("msg-1"),
        factory::text_message_end("msg-1"),
        factory::tool_call_start("tool-1", "search"),
        factory::tool_call_end("tool-1"),
        factory::run_finished("test-thread-1", "test-run-1"),
        factory::run_started("test-thread-1", "test-run-2"),
        factory::step_started("analysis"),
        factory::text_message_start("msg-2"),
        factory::text_message_end("msg-2"),
        factory::step_finished("analysis"),
        factory::run_finished("test-thread-1", "test-run-2"),
    ])
    .await;

    assert_eq!(out.len(), 12);
    assert!(out.iter().all(Result::is_ok), "all events should verify: {out:?}");
    assert!(matches!(out[0], Ok(Event::RunStarted(_))));
    assert!(matches!(out[5], Ok(Event::RunFinished(_))));
    assert!(matches!(out[6], Ok(Event::RunStarted(_))));
    assert!(matches!(out[11], Ok(Event::RunFinished(_))));
}
