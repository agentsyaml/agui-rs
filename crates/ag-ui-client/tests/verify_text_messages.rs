use ag_ui_client::verify_events;
use ag_ui_core::*;
use futures::{stream, StreamExt};
use serde_json::json;

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

#[tokio::test]
async fn text_message_content_before_start_errors() {
    let out = collect(vec![
        factory::run_started("thread", "run"),
        factory::text_message_content("m1", "hello"),
    ])
    .await;

    assert!(out[0].is_ok());
    assert_validation(&out[1], "Cannot send 'TEXT_MESSAGE_CONTENT' event: No active text message found with ID 'm1'");
}

#[tokio::test]
async fn text_message_end_before_start_errors() {
    let out = collect(vec![
        factory::run_started("thread", "run"),
        factory::text_message_end("m1"),
    ])
    .await;

    assert!(out[0].is_ok());
    assert_validation(&out[1], "Cannot send 'TEXT_MESSAGE_END' event: No active text message found with ID 'm1'");
}

#[tokio::test]
async fn second_text_message_start_while_active_errors() {
    let out = collect(vec![
        factory::run_started("thread", "run"),
        Event::TextMessageStart(TextMessageStartEvent {
            message_id: "m1".into(),
            role: TextMessageRole::Assistant,
            name: None,
            base: BaseEventFields::default(),
        }),
        Event::TextMessageStart(TextMessageStartEvent {
            message_id: "m2".into(),
            role: TextMessageRole::Assistant,
            name: None,
            base: BaseEventFields::default(),
        }),
    ])
    .await;

    assert!(out[0].is_ok());
    assert!(out[1].is_ok());
    assert_validation(&out[2], "A text message with ID 'm1' is already in progress");
}

#[tokio::test]
async fn mismatched_text_message_content_id_errors() {
    let out = collect(vec![
        factory::run_started("thread", "run"),
        factory::text_message_start("m1"),
        factory::text_message_content("m2", "hello"),
    ])
    .await;

    assert_validation(&out[2], "Cannot send 'TEXT_MESSAGE_CONTENT' event: No active text message found with ID 'm2'");
}

#[tokio::test]
async fn text_message_content_with_raw_custom_state_and_messages_snapshot_passes() {
    let out = collect(vec![
        factory::run_started("thread", "run"),
        factory::text_message_start("m1"),
        factory::text_message_content("m1", "hello"),
        raw_event(),
        custom_event(),
        factory::state_snapshot(json!({})),
        factory::state_delta(vec![json!({ "op": "add", "path": "/result", "value": "success" })]),
        messages_snapshot_event(),
        factory::text_message_end("m1"),
        factory::run_finished("thread", "run"),
    ])
    .await;

    assert_eq!(out.len(), 10);
    assert!(out.iter().all(|item| item.is_ok()));
}

#[tokio::test]
async fn lifecycle_events_during_text_message_pass() {
    let out = collect(vec![
        factory::run_started("thread", "run"),
        factory::text_message_start("m1"),
        factory::step_started("test-step"),
        factory::text_message_content("m1", "working"),
        factory::step_finished("test-step"),
        factory::text_message_end("m1"),
        factory::run_finished("thread", "run"),
    ])
    .await;

    assert_eq!(out.len(), 7);
    assert!(out.iter().all(|item| item.is_ok()));
}

#[tokio::test]
async fn tool_call_can_run_inside_active_text_message() {
    let out = collect(vec![
        factory::run_started("thread", "run"),
        factory::text_message_start("m1"),
        factory::text_message_content("m1", "Starting search..."),
        tool_call_start_with_parent("tc1", "m1"),
        factory::tool_call_args("tc1", "{\"query\":\"test\"}"),
        factory::tool_call_end("tc1"),
        factory::text_message_content("m1", "Search completed."),
        factory::text_message_end("m1"),
        factory::run_finished("thread", "run"),
    ])
    .await;

    assert_eq!(out.len(), 9);
    assert!(out.iter().all(|item| item.is_ok()));
}

#[tokio::test]
async fn multiple_sequential_text_messages_pass() {
    let out = collect(vec![
        factory::run_started("thread", "run"),
        factory::text_message_start("m1"),
        factory::text_message_content("m1", "content 1"),
        factory::text_message_end("m1"),
        factory::text_message_start("m2"),
        factory::text_message_content("m2", "content 2"),
        factory::text_message_end("m2"),
        factory::run_finished("thread", "run"),
    ])
    .await;

    assert_eq!(out.len(), 8);
    assert!(out.iter().all(|item| item.is_ok()));
}

#[tokio::test]
async fn text_message_at_run_boundaries_passes() {
    let out = collect(vec![
        factory::run_started("thread", "run"),
        factory::text_message_start("m1"),
        factory::text_message_content("m1", "content 1"),
        factory::text_message_end("m1"),
        factory::run_finished("thread", "run"),
    ])
    .await;

    assert_eq!(out.len(), 5);
    assert!(out.iter().all(|item| item.is_ok()));
}

#[tokio::test]
async fn starting_text_message_before_run_started_errors() {
    let out = collect(vec![factory::text_message_start("m1")]).await;

    assert_eq!(out.len(), 1);
    assert_validation(&out[0], "First event must be 'RUN_STARTED'");
}

#[tokio::test]
async fn run_finished_with_active_text_message_errors() {
    let out = collect(vec![
        factory::run_started("thread", "run"),
        factory::text_message_start("m1"),
        factory::run_finished("thread", "run"),
    ])
    .await;

    assert_validation(&out[2], "Cannot send 'RUN_FINISHED' while text message 'm1' is still active");
}
