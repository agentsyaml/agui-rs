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
async fn requires_run_started_as_first_event() {
    let out = collect(vec![factory::text_message_start("m1")]).await;

    assert_eq!(out.len(), 1);
    assert_validation(&out[0], "First event must be 'RUN_STARTED'");
}

#[tokio::test]
async fn rejects_multiple_run_started_events_in_same_stream() {
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
async fn rejects_events_after_run_finished() {
    let out = collect(vec![
        factory::run_started("thread", "run"),
        factory::text_message_start("m1"),
        factory::text_message_end("m1"),
        factory::run_finished("thread", "run"),
        factory::text_message_start("m2"),
    ])
    .await;

    assert_eq!(out.len(), 5);
    assert!(out[..4].iter().all(Result::is_ok));
    assert_validation(
        &out[4],
        "Cannot send event type 'TEXT_MESSAGE_START': The run has already finished with 'RUN_FINISHED'",
    );
}

#[tokio::test]
async fn rejects_events_after_run_error() {
    let out = collect(vec![
        factory::run_started("thread", "run"),
        factory::run_error("boom"),
        factory::text_message_start("m1"),
    ])
    .await;

    assert_eq!(out.len(), 3);
    assert!(out[..2].iter().all(Result::is_ok));
    assert_validation(
        &out[2],
        "Cannot send event type 'TEXT_MESSAGE_START': The run has already errored with 'RUN_ERROR'",
    );
}

#[tokio::test]
async fn allows_valid_lifecycle_sequence() {
    let out = collect(vec![
        factory::run_started("thread", "run"),
        factory::text_message_start("m1"),
        factory::text_message_content("m1", "hello"),
        factory::text_message_end("m1"),
        factory::tool_call_start("tc1", "search"),
        factory::tool_call_args("tc1", "{}"),
        factory::tool_call_end("tc1"),
        factory::run_finished("thread", "run"),
    ])
    .await;

    assert_eq!(out.len(), 8);
    assert!(out.iter().all(Result::is_ok));
}

// Ported from TypeScript `verify/__tests__/verify.lifecycle.test.ts`.

#[tokio::test]
async fn allows_run_error_after_run_finished() {
    let out = collect(vec![
        factory::run_started("test-thread-id", "test-run-id"),
        factory::text_message_start("1"),
        factory::text_message_end("1"),
        factory::run_finished("test-thread-id", "test-run-id"),
        factory::run_error("Test error"),
    ])
    .await;

    assert_eq!(out.len(), 5);
    assert!(
        out.iter().all(Result::is_ok),
        "all events should verify: {out:?}"
    );
    assert!(matches!(out[4], Ok(Event::RunError(_))));
}

#[tokio::test]
async fn allows_run_error_as_first_event() {
    let out = collect(vec![factory::run_error("Test error")]).await;

    assert_eq!(out.len(), 1);
    assert!(matches!(out[0], Ok(Event::RunError(_))));
}
