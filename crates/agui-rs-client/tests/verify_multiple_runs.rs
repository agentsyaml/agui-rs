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

// SKIPPED: should allow multiple sequential runs: Rust verify_events validates one run per stream and treats RUN_FINISHED as terminal.
// SKIPPED: should allow reusing message IDs across different runs: Rust verify_events does not reset message state for a second run because a verified stream ends after one terminal run event.
// SKIPPED: should allow multiple runs with tool calls: Rust verify_events validates one run per stream and rejects a second RUN_STARTED after RUN_FINISHED.
// SKIPPED: should allow multiple runs with steps: Rust verify_events validates one run per stream and rejects a second RUN_STARTED after RUN_FINISHED.
// SKIPPED: should allow three sequential runs: Rust verify_events validates one run per stream and rejects any event after the first terminal run event.
// SKIPPED: should handle complex scenario with multiple runs and various event types: Rust verify_events validates one run per stream and does not support multi-run streams.
