use ag_ui_client::verify_events;
use ag_ui_core::{
    factory, AgUiError, BaseEventFields, CustomEvent, Event, MessagesSnapshotEvent, RawEvent,
    StateDeltaEvent, StateSnapshotEvent,
};
use futures::{stream, StreamExt};
use serde_json::{json, Value};

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

fn custom_event(name: &str, value: Value) -> Event {
    Event::Custom(CustomEvent {
        name: name.into(),
        value,
        base: BaseEventFields::default(),
    })
}

fn raw_event() -> Event {
    Event::Raw(RawEvent {
        event: json!({"type": "raw_data", "content": "test"}),
        source: None,
        base: BaseEventFields::default(),
    })
}

fn state_snapshot() -> Event {
    Event::StateSnapshot(StateSnapshotEvent {
        snapshot: json!({"state": "initial", "data": {"foo": "bar"}}),
        base: BaseEventFields::default(),
    })
}

fn state_delta() -> Event {
    Event::StateDelta(StateDeltaEvent {
        delta: vec![json!({"op": "add", "path": "/result", "value": "success"})],
        base: BaseEventFields::default(),
    })
}

fn messages_snapshot() -> Event {
    Event::MessagesSnapshot(MessagesSnapshotEvent {
        messages: Vec::new(),
        base: BaseEventFields::default(),
    })
}

#[tokio::test]
async fn text_message_content_requires_matching_start_id() {
    let out = collect(vec![
        factory::run_started("thread", "run"),
        factory::text_message_start("msg1"),
        factory::text_message_content("different-id", "test content"),
    ])
    .await;

    assert_eq!(out.len(), 3);
    assert!(out[..2].iter().all(Result::is_ok));
    assert_validation(
        &out[2],
        "Cannot send 'TEXT_MESSAGE_CONTENT' event: No active text message found with ID 'different-id'",
    );
}

#[tokio::test]
async fn text_message_end_requires_matching_start_id() {
    let out = collect(vec![
        factory::run_started("thread", "run"),
        factory::text_message_end("msg1"),
    ])
    .await;

    assert_eq!(out.len(), 2);
    assert!(out[0].is_ok());
    assert_validation(
        &out[1],
        "Cannot send 'TEXT_MESSAGE_END' event: No active text message found with ID 'msg1'",
    );
}

#[tokio::test]
async fn tool_call_args_require_matching_start_id() {
    let out = collect(vec![
        factory::run_started("thread", "run"),
        factory::tool_call_start("t1", "test-tool"),
        factory::tool_call_args("different-id", "test args"),
    ])
    .await;

    assert_eq!(out.len(), 3);
    assert!(out[..2].iter().all(Result::is_ok));
    assert_validation(
        &out[2],
        "Cannot send 'TOOL_CALL_ARGS' event: No active tool call found with ID 'different-id'",
    );
}

#[tokio::test]
async fn tool_call_end_requires_matching_start_id() {
    let out = collect(vec![
        factory::run_started("thread", "run"),
        factory::tool_call_end("t1"),
    ])
    .await;

    assert_eq!(out.len(), 2);
    assert!(out[0].is_ok());
    assert_validation(
        &out[1],
        "Cannot send 'TOOL_CALL_END' event: No active tool call found with ID 't1'",
    );
}

#[tokio::test]
async fn custom_and_raw_events_are_allowed_during_a_run() {
    let out = collect(vec![
        factory::run_started("thread", "run"),
        custom_event("Exit", Value::Null),
        raw_event(),
        factory::text_message_start("m1"),
        factory::text_message_end("m1"),
        factory::run_finished("thread", "run"),
    ])
    .await;

    assert_eq!(out.len(), 6);
    assert!(out.iter().all(Result::is_ok));
    assert!(matches!(out[1], Ok(Event::Custom(_))));
    assert!(matches!(out[2], Ok(Event::Raw(_))));
}

#[tokio::test]
async fn state_related_events_are_allowed_during_a_run() {
    let out = collect(vec![
        factory::run_started("thread", "run"),
        state_snapshot(),
        factory::step_started("step1"),
        messages_snapshot(),
        factory::step_finished("step1"),
        state_delta(),
        factory::run_finished("thread", "run"),
    ])
    .await;

    assert_eq!(out.len(), 7);
    assert!(out.iter().all(Result::is_ok));
    assert!(matches!(out[1], Ok(Event::StateSnapshot(_))));
    assert!(matches!(out[3], Ok(Event::MessagesSnapshot(_))));
    assert!(matches!(out[5], Ok(Event::StateDelta(_))));
}

#[tokio::test]
async fn state_snapshot_is_allowed_when_the_run_finishes_cleanly() {
    let out = collect(vec![
        factory::run_started("thread", "run"),
        state_snapshot(),
        factory::run_finished("thread", "run"),
    ])
    .await;

    assert_eq!(out.len(), 3);
    assert!(out.iter().all(Result::is_ok));
}

#[tokio::test]
async fn allows_complex_sequence_with_messages_steps_and_tool_calls() {
    let out = collect(vec![
        factory::run_started("thread", "run"),
        factory::step_started("step1"),
        factory::text_message_start("msg1"),
        factory::text_message_content("msg1", "msg1 content"),
        factory::text_message_end("msg1"),
        factory::tool_call_start("t1", "test-tool"),
        factory::tool_call_args("t1", "t1 args"),
        factory::tool_call_end("t1"),
        factory::text_message_start("msg2"),
        factory::text_message_content("msg2", "msg2 content"),
        factory::text_message_end("msg2"),
        factory::step_finished("step1"),
        factory::run_finished("thread", "run"),
    ])
    .await;

    assert_eq!(out.len(), 13);
    assert!(out.iter().all(Result::is_ok));
}
