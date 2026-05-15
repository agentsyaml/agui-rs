use ag_ui_client::expand_chunks;
use ag_ui_core::{
    event_factories::{
        create_raw_event, create_text_message_chunk_event, create_tool_call_chunk_event,
    },
    factory, AgUiError, BaseEventFields, Event, RawEvent, TextMessageContentEvent,
    TextMessageEndEvent, TextMessageRole, TextMessageStartEvent, ToolCallArgsEvent,
    ToolCallEndEvent, ToolCallStartEvent,
};
use futures::{stream, StreamExt};
use serde_json::json;

async fn collect(events: Vec<Event>) -> Vec<Result<Event, AgUiError>> {
    let input = stream::iter(events.into_iter().map(Ok::<_, AgUiError>));
    expand_chunks(Box::pin(input)).collect::<Vec<_>>().await
}

async fn collect_ok(events: Vec<Event>) -> Vec<Event> {
    collect(events)
        .await
        .into_iter()
        .collect::<Result<Vec<_>, _>>()
        .expect("chunk expansion should succeed")
}

fn assert_validation(result: &Result<Event, AgUiError>, expected: &str) {
    match result {
        Err(AgUiError::Validation(message)) => {
            assert!(message.contains(expected), "expected '{expected}' in '{message}'");
        }
        other => panic!("expected validation error, got {other:?}"),
    }
}

fn text_start(message_id: &str, role: TextMessageRole, name: Option<&str>) -> Event {
    Event::TextMessageStart(TextMessageStartEvent {
        message_id: message_id.to_string(),
        role,
        name: name.map(str::to_string),
        base: BaseEventFields::default(),
    })
}

fn text_content(message_id: &str, delta: &str) -> Event {
    Event::TextMessageContent(TextMessageContentEvent {
        message_id: message_id.to_string(),
        delta: delta.to_string(),
        base: BaseEventFields::default(),
    })
}

fn text_end(message_id: &str) -> Event {
    Event::TextMessageEnd(TextMessageEndEvent {
        message_id: message_id.to_string(),
        base: BaseEventFields::default(),
    })
}

fn tool_start(tool_call_id: &str, tool_call_name: &str, parent_message_id: Option<&str>) -> Event {
    Event::ToolCallStart(ToolCallStartEvent {
        tool_call_id: tool_call_id.to_string(),
        tool_call_name: tool_call_name.to_string(),
        parent_message_id: parent_message_id.map(str::to_string),
        base: BaseEventFields::default(),
    })
}

fn tool_args(tool_call_id: &str, delta: &str) -> Event {
    Event::ToolCallArgs(ToolCallArgsEvent {
        tool_call_id: tool_call_id.to_string(),
        delta: delta.to_string(),
        base: BaseEventFields::default(),
    })
}

fn tool_end(tool_call_id: &str) -> Event {
    Event::ToolCallEnd(ToolCallEndEvent {
        tool_call_id: tool_call_id.to_string(),
        base: BaseEventFields::default(),
    })
}

#[tokio::test]
async fn transforms_single_text_message_chunk_into_start_content_end() {
    let close_event = factory::run_finished("thread-123", "run-123");
    let events = collect_ok(vec![
        create_text_message_chunk_event(
            Some("msg-123".into()),
            None,
            Some("Hello, world!".into()),
            None,
            Some(1),
            None,
        ),
        close_event.clone(),
    ])
    .await;

    assert_eq!(
        events,
        vec![
            text_start("msg-123", TextMessageRole::Assistant, None),
            text_content("msg-123", "Hello, world!"),
            text_end("msg-123"),
            close_event,
        ]
    );
}

#[tokio::test]
async fn transforms_multiple_text_message_chunks_with_same_id() {
    let close_event = factory::run_finished("thread-123", "run-123");
    let events = collect_ok(vec![
        create_text_message_chunk_event(
            Some("msg-123".into()),
            None,
            Some("Hello".into()),
            None,
            Some(1),
            None,
        ),
        create_text_message_chunk_event(
            Some("msg-123".into()),
            None,
            Some(", world!".into()),
            None,
            Some(2),
            None,
        ),
        close_event.clone(),
    ])
    .await;

    assert_eq!(
        events,
        vec![
            text_start("msg-123", TextMessageRole::Assistant, None),
            text_content("msg-123", "Hello"),
            text_content("msg-123", ", world!"),
            text_end("msg-123"),
            close_event,
        ]
    );
}

#[tokio::test]
async fn transforms_single_tool_call_chunk_into_start_args_end() {
    let close_event = factory::run_finished("thread-123", "run-123");
    let events = collect_ok(vec![
        create_tool_call_chunk_event(
            Some("tool-123".into()),
            Some("testTool".into()),
            None,
            Some(r#"{"arg1": "value1"}"#.into()),
            Some(1),
            None,
        ),
        close_event.clone(),
    ])
    .await;

    assert_eq!(
        events,
        vec![
            tool_start("tool-123", "testTool", None),
            tool_args("tool-123", r#"{"arg1": "value1"}"#),
            tool_end("tool-123"),
            close_event,
        ]
    );
}

#[tokio::test]
async fn closes_text_message_when_switching_to_tool_call() {
    let close_event = factory::run_finished("thread-123", "run-123");
    let events = collect_ok(vec![
        create_text_message_chunk_event(
            Some("msg-123".into()),
            None,
            Some("Hello".into()),
            None,
            Some(1),
            None,
        ),
        create_tool_call_chunk_event(
            Some("tool-123".into()),
            Some("testTool".into()),
            None,
            Some(r#"{"arg1": "value1"}"#.into()),
            Some(2),
            None,
        ),
        close_event.clone(),
    ])
    .await;

    assert_eq!(
        events,
        vec![
            text_start("msg-123", TextMessageRole::Assistant, None),
            text_content("msg-123", "Hello"),
            text_end("msg-123"),
            tool_start("tool-123", "testTool", None),
            tool_args("tool-123", r#"{"arg1": "value1"}"#),
            tool_end("tool-123"),
            close_event,
        ]
    );
}

#[tokio::test]
async fn passes_through_non_chunk_events() {
    let run_start = factory::run_started("thread-123", "run-123");
    let events = collect_ok(vec![run_start.clone()]).await;

    assert_eq!(events, vec![run_start]);
}

#[tokio::test]
async fn closes_current_message_when_non_chunk_event_arrives() {
    let run_start = factory::run_started("thread-123", "run-123");
    let events = collect_ok(vec![
        create_text_message_chunk_event(
            Some("msg-123".into()),
            None,
            Some("Hello".into()),
            None,
            Some(1),
            None,
        ),
        run_start.clone(),
    ])
    .await;

    assert_eq!(
        events,
        vec![
            text_start("msg-123", TextMessageRole::Assistant, None),
            text_content("msg-123", "Hello"),
            text_end("msg-123"),
            run_start,
        ]
    );
}

#[tokio::test]
async fn closes_previous_text_message_when_message_id_changes() {
    let close_event = factory::run_finished("thread-123", "run-123");
    let events = collect_ok(vec![
        create_text_message_chunk_event(
            Some("msg-123".into()),
            None,
            Some("Hello".into()),
            None,
            Some(1),
            None,
        ),
        create_text_message_chunk_event(
            Some("msg-456".into()),
            None,
            Some("Different message".into()),
            None,
            Some(2),
            None,
        ),
        close_event.clone(),
    ])
    .await;

    assert_eq!(
        events,
        vec![
            text_start("msg-123", TextMessageRole::Assistant, None),
            text_content("msg-123", "Hello"),
            text_end("msg-123"),
            text_start("msg-456", TextMessageRole::Assistant, None),
            text_content("msg-456", "Different message"),
            text_end("msg-456"),
            close_event,
        ]
    );
}

#[tokio::test]
async fn errors_when_first_text_message_chunk_has_no_id() {
    let events = collect(vec![create_text_message_chunk_event(
        None,
        None,
        Some("This will fail".into()),
        None,
        Some(1),
        None,
    )])
    .await;

    assert_eq!(events.len(), 1);
    assert_validation(&events[0], "first TEXT_MESSAGE_CHUNK must include message_id");
}

#[tokio::test]
async fn errors_when_first_tool_call_chunk_has_no_id() {
    let events = collect(vec![create_tool_call_chunk_event(
        None,
        Some("testTool".into()),
        None,
        Some("This will fail".into()),
        Some(1),
        None,
    )])
    .await;

    assert_eq!(events.len(), 1);
    assert_validation(&events[0], "first TOOL_CALL_CHUNK must include tool_call_id");
}

#[tokio::test]
async fn errors_when_first_tool_call_chunk_has_no_name() {
    let events = collect(vec![create_tool_call_chunk_event(
        Some("tool-123".into()),
        None,
        None,
        Some("This will fail".into()),
        Some(1),
        None,
    )])
    .await;

    assert_eq!(events.len(), 1);
    assert_validation(&events[0], "first TOOL_CALL_CHUNK must include tool_call_name");
}

#[tokio::test]
async fn preserves_tool_call_parent_message_id() {
    let close_event = factory::run_finished("thread-123", "run-123");
    let events = collect_ok(vec![
        create_tool_call_chunk_event(
            Some("tool-123".into()),
            Some("testTool".into()),
            Some("parent-msg-123".into()),
            Some(r#"{"arg1": "value1"}"#.into()),
            Some(1),
            None,
        ),
        close_event.clone(),
    ])
    .await;

    assert_eq!(
        events,
        vec![
            tool_start("tool-123", "testTool", Some("parent-msg-123")),
            tool_args("tool-123", r#"{"arg1": "value1"}"#),
            tool_end("tool-123"),
            close_event,
        ]
    );
}

#[tokio::test]
async fn passes_through_raw_events_without_transformation() {
    let raw_event = create_raw_event(json!({ "some": "data" }), Some("test-source".into()), Some(1), None);
    let events = collect_ok(vec![raw_event.clone()]).await;

    assert_eq!(events, vec![raw_event]);
}

#[tokio::test]
async fn handles_a_complex_sequence_of_mixed_events() {
    let close_event = factory::run_finished("thread-123", "run-123");
    let events = collect_ok(vec![
        create_text_message_chunk_event(
            Some("msg-123".into()),
            None,
            Some("Hello".into()),
            None,
            Some(1),
            None,
        ),
        create_tool_call_chunk_event(
            Some("tool-123".into()),
            Some("testTool".into()),
            None,
            Some(r#"{"arg1": "value1"}"#.into()),
            Some(2),
            None,
        ),
        create_text_message_chunk_event(
            Some("msg-456".into()),
            None,
            Some("After tool call".into()),
            None,
            Some(3),
            None,
        ),
        close_event.clone(),
    ])
    .await;

    assert_eq!(
        events,
        vec![
            text_start("msg-123", TextMessageRole::Assistant, None),
            text_content("msg-123", "Hello"),
            text_end("msg-123"),
            tool_start("tool-123", "testTool", None),
            tool_args("tool-123", r#"{"arg1": "value1"}"#),
            tool_end("tool-123"),
            text_start("msg-456", TextMessageRole::Assistant, None),
            text_content("msg-456", "After tool call"),
            text_end("msg-456"),
            close_event,
        ]
    );
}

#[tokio::test]
async fn omits_text_content_event_when_chunk_has_no_delta() {
    let close_event = factory::run_finished("thread-123", "run-123");
    let events = collect_ok(vec![
        create_text_message_chunk_event(
            Some("msg-123".into()),
            None,
            None,
            None,
            Some(1),
            None,
        ),
        close_event.clone(),
    ])
    .await;

    assert_eq!(
        events,
        vec![
            text_start("msg-123", TextMessageRole::Assistant, None),
            text_end("msg-123"),
            close_event,
        ]
    );
}

#[tokio::test]
async fn omits_tool_args_event_when_chunk_has_no_delta() {
    let close_event = factory::run_finished("thread-123", "run-123");
    let events = collect_ok(vec![
        create_tool_call_chunk_event(
            Some("tool-123".into()),
            Some("testTool".into()),
            None,
            None,
            Some(1),
            None,
        ),
        close_event.clone(),
    ])
    .await;

    assert_eq!(
        events,
        vec![
            tool_start("tool-123", "testTool", None),
            tool_end("tool-123"),
            close_event,
        ]
    );
}

#[tokio::test]
async fn emits_exactly_one_text_start_and_end_for_multiple_chunks_with_same_id() {
    let close_event = factory::run_finished("thread-123", "run-123");
    let events = collect_ok(vec![
        create_text_message_chunk_event(
            Some("msg-123".into()),
            None,
            Some("First part".into()),
            None,
            Some(1),
            None,
        ),
        create_text_message_chunk_event(
            Some("msg-123".into()),
            None,
            Some("Second part".into()),
            None,
            Some(2),
            None,
        ),
        create_text_message_chunk_event(
            Some("msg-123".into()),
            None,
            Some("Third part".into()),
            None,
            Some(3),
            None,
        ),
        create_text_message_chunk_event(
            Some("msg-123".into()),
            None,
            Some("Fourth part".into()),
            None,
            Some(4),
            None,
        ),
        close_event.clone(),
    ])
    .await;

    assert_eq!(
        events[0],
        text_start("msg-123", TextMessageRole::Assistant, None)
    );
    assert_eq!(events[5], text_end("msg-123"));
    assert_eq!(events[6], close_event);
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, Event::TextMessageStart(_)))
            .count(),
        1
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, Event::TextMessageEnd(_)))
            .count(),
        1
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, Event::TextMessageContent(_)))
            .count(),
        4
    );
    assert!(events[1..5].iter().all(|event| matches!(
        event,
        Event::TextMessageContent(TextMessageContentEvent { message_id, .. }) if message_id == "msg-123"
    )));
}

#[tokio::test]
async fn passes_name_from_text_message_chunk_to_start_event() {
    let close_event = factory::run_finished("thread-123", "run-123");
    let events = collect_ok(vec![
        create_text_message_chunk_event(
            Some("msg-123".into()),
            None,
            Some("Hello".into()),
            Some("research-agent".into()),
            Some(1),
            None,
        ),
        close_event,
    ])
    .await;

    assert_eq!(
        events[0],
        text_start("msg-123", TextMessageRole::Assistant, Some("research-agent"))
    );
}

#[tokio::test]
async fn omits_name_on_text_message_start_when_chunk_has_no_name() {
    let close_event = factory::run_finished("thread-123", "run-123");
    let events = collect_ok(vec![
        create_text_message_chunk_event(
            Some("msg-123".into()),
            None,
            Some("Hello".into()),
            None,
            Some(1),
            None,
        ),
        close_event,
    ])
    .await;

    assert_eq!(
        events[0],
        text_start("msg-123", TextMessageRole::Assistant, None)
    );
}

#[tokio::test]
async fn handles_interleaved_text_and_tool_chunks_as_separate_open_sequences() {
    let close_event = factory::run_finished("thread-123", "run-123");
    let events = collect_ok(vec![
        create_text_message_chunk_event(
            Some("msg-1".into()),
            None,
            Some("First message part 1".into()),
            None,
            Some(1),
            None,
        ),
        create_tool_call_chunk_event(
            Some("tool-1".into()),
            Some("firstTool".into()),
            None,
            Some(r#"{"arg1": "value1"}"#.into()),
            Some(2),
            None,
        ),
        create_text_message_chunk_event(
            Some("msg-1".into()),
            None,
            Some("First message part 2".into()),
            None,
            Some(3),
            None,
        ),
        create_text_message_chunk_event(
            Some("msg-2".into()),
            None,
            Some("Second message".into()),
            None,
            Some(4),
            None,
        ),
        create_tool_call_chunk_event(
            Some("tool-2".into()),
            Some("secondTool".into()),
            None,
            Some(r#"{"arg2": "value2"}"#.into()),
            Some(5),
            None,
        ),
        create_tool_call_chunk_event(
            Some("tool-1".into()),
            Some("firstTool".into()),
            None,
            Some(r#",\"arg1_more\": \"more data\"}"#.into()),
            Some(6),
            None,
        ),
        close_event,
    ])
    .await;

    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, Event::TextMessageStart(_)))
            .count(),
        3
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, Event::TextMessageEnd(_)))
            .count(),
        3
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, Event::ToolCallStart(_)))
            .count(),
        3
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, Event::ToolCallEnd(_)))
            .count(),
        3
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, Event::TextMessageContent(_)))
            .count(),
        3
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, Event::ToolCallArgs(_)))
            .count(),
        3
    );
    assert_eq!(events.len(), 19);

    let msg_1_start_count = events
        .iter()
        .filter(|event| matches!(
            event,
            Event::TextMessageStart(TextMessageStartEvent { message_id, .. }) if message_id == "msg-1"
        ))
        .count();
    let msg_1_end_count = events
        .iter()
        .filter(|event| matches!(
            event,
            Event::TextMessageEnd(TextMessageEndEvent { message_id, .. }) if message_id == "msg-1"
        ))
        .count();
    let msg_2_start_count = events
        .iter()
        .filter(|event| matches!(
            event,
            Event::TextMessageStart(TextMessageStartEvent { message_id, .. }) if message_id == "msg-2"
        ))
        .count();
    let msg_2_end_count = events
        .iter()
        .filter(|event| matches!(
            event,
            Event::TextMessageEnd(TextMessageEndEvent { message_id, .. }) if message_id == "msg-2"
        ))
        .count();

    assert_eq!(msg_1_start_count, 2);
    assert_eq!(msg_1_end_count, 2);
    assert_eq!(msg_2_start_count, 1);
    assert_eq!(msg_2_end_count, 1);
}

#[tokio::test]
async fn raw_event_does_not_close_pending_text_message() {
    let raw_event = Event::Raw(RawEvent {
        event: json!({ "provider": "test" }),
        source: Some("raw-source".into()),
        base: BaseEventFields::default(),
    });
    let events = collect_ok(vec![
        create_text_message_chunk_event(
            Some("msg-123".into()),
            None,
            Some("Hello".into()),
            None,
            Some(1),
            None,
        ),
        raw_event.clone(),
    ])
    .await;

    assert_eq!(
        events,
        vec![
            text_start("msg-123", TextMessageRole::Assistant, None),
            text_content("msg-123", "Hello"),
            raw_event,
            text_end("msg-123"),
        ]
    );
}
