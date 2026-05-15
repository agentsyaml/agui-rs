use ag_ui_client::verify_events;
use ag_ui_core::{factory, AgUiError, Event};
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
async fn step_finished_requires_matching_started_name() {
    let out = collect(vec![
        factory::run_started("thread", "run"),
        factory::step_started("test-step"),
        factory::step_finished("different-name"),
    ])
    .await;

    assert_eq!(out.len(), 3);
    assert!(out[..2].iter().all(Result::is_ok));
    assert_validation(
        &out[2],
        "Cannot send 'STEP_FINISHED' for step \"different-name\" that was not started",
    );
}

#[tokio::test]
async fn rejects_step_finished_without_step_started() {
    let out = collect(vec![
        factory::run_started("thread", "run"),
        factory::step_finished("test-step"),
    ])
    .await;

    assert_eq!(out.len(), 2);
    assert!(out[0].is_ok());
    assert_validation(
        &out[1],
        "Cannot send 'STEP_FINISHED' for step \"test-step\" that was not started",
    );
}

#[tokio::test]
async fn rejects_duplicate_active_step_names() {
    let out = collect(vec![
        factory::run_started("thread", "run"),
        factory::step_started("test-step"),
        factory::step_started("test-step"),
    ])
    .await;

    assert_eq!(out.len(), 3);
    assert!(out[..2].iter().all(Result::is_ok));
    assert_validation(
        &out[2],
        "Step \"test-step\" is already active for 'STEP_STARTED'",
    );
}

#[tokio::test]
async fn run_finished_requires_all_steps_to_be_closed() {
    let out = collect(vec![
        factory::run_started("thread", "run"),
        factory::step_started("step1"),
        factory::step_finished("step1"),
        factory::step_started("step2"),
        factory::run_finished("thread", "run"),
    ])
    .await;

    assert_eq!(out.len(), 5);
    assert!(out[..4].iter().all(Result::is_ok));
    assert_validation(
        &out[4],
        "Cannot send 'RUN_FINISHED' while steps are still active: step2",
    );
}

#[tokio::test]
async fn allows_balanced_step_sequence() {
    let out = collect(vec![
        factory::run_started("thread", "run"),
        factory::step_started("step1"),
        factory::step_finished("step1"),
        factory::run_finished("thread", "run"),
    ])
    .await;

    assert_eq!(out.len(), 4);
    assert!(out.iter().all(Result::is_ok));
}
