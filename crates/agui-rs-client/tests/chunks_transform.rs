use agui_rs_client::expand_chunks;
use agui_rs_core::{
    event_factories::{
        create_raw_event, create_text_message_chunk_event, create_tool_call_chunk_event,
    },
    factory, AgUiError, AttributableFields, BaseEventFields, Event, RawEvent,
    ReasoningMessageChunkEvent, TextMessageChunkEvent, TextMessageContentEvent,
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
            assert!(
                message.contains(expected),
                "expected '{expected}' in '{message}'"
            );
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
        attributable: AttributableFields::default(),
    })
}

fn text_content(message_id: &str, delta: &str) -> Event {
    Event::TextMessageContent(TextMessageContentEvent {
        message_id: message_id.to_string(),
        delta: delta.to_string(),
        base: BaseEventFields::default(),
        attributable: AttributableFields::default(),
    })
}

fn text_end(message_id: &str) -> Event {
    Event::TextMessageEnd(TextMessageEndEvent {
        message_id: message_id.to_string(),
        base: BaseEventFields::default(),
        attributable: AttributableFields::default(),
    })
}

fn tool_start(tool_call_id: &str, tool_call_name: &str, parent_message_id: Option<&str>) -> Event {
    Event::ToolCallStart(ToolCallStartEvent {
        tool_call_id: tool_call_id.to_string(),
        tool_call_name: tool_call_name.to_string(),
        parent_message_id: parent_message_id.map(str::to_string),
        base: BaseEventFields::default(),
        attributable: AttributableFields::default(),
    })
}

fn tool_args(tool_call_id: &str, delta: &str) -> Event {
    Event::ToolCallArgs(ToolCallArgsEvent {
        tool_call_id: tool_call_id.to_string(),
        delta: delta.to_string(),
        base: BaseEventFields::default(),
        attributable: AttributableFields::default(),
    })
}

fn tool_end(tool_call_id: &str) -> Event {
    Event::ToolCallEnd(ToolCallEndEvent {
        tool_call_id: tool_call_id.to_string(),
        base: BaseEventFields::default(),
        attributable: AttributableFields::default(),
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
    assert_validation(
        &events[0],
        "first TEXT_MESSAGE_CHUNK must include message_id",
    );
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
    assert_validation(
        &events[0],
        "first TOOL_CALL_CHUNK must include tool_call_id",
    );
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
    assert_validation(
        &events[0],
        "first TOOL_CALL_CHUNK must include tool_call_name",
    );
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
    let raw_event = create_raw_event(
        json!({ "some": "data" }),
        Some("test-source".into()),
        Some(1),
        None,
    );
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
        create_text_message_chunk_event(Some("msg-123".into()), None, None, None, Some(1), None),
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
        text_start(
            "msg-123",
            TextMessageRole::Assistant,
            Some("research-agent")
        )
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
        attributable: AttributableFields::default(),
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

    // The RAW event does not close the pending message, and a stream that ends
    // without a run terminal synthesizes no END (TS `finalize` discards the
    // events it builds — transform.ts:951-958).
    assert_eq!(
        events,
        vec![
            text_start("msg-123", TextMessageRole::Assistant, None),
            text_content("msg-123", "Hello"),
            raw_event,
        ]
    );
}

// Faithfulness regression: in TS `transformChunks`, CUSTOM and TOOL_CALL_RESULT
// belong to the group that closes a pending chunk stream first (only RAW,
// ACTIVITY_SNAPSHOT, ACTIVITY_DELTA and REASONING_ENCRYPTED_VALUE pass through
// without closing). These two tests lock that behaviour in.
#[tokio::test]
async fn custom_event_closes_pending_text_message() {
    let custom_event = Event::Custom(agui_rs_core::CustomEvent {
        name: "mark".into(),
        value: json!({ "x": 1 }),
        base: BaseEventFields::default(),
        attributable: AttributableFields::default(),
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
        custom_event.clone(),
    ])
    .await;

    assert_eq!(
        events,
        vec![
            text_start("msg-123", TextMessageRole::Assistant, None),
            text_content("msg-123", "Hello"),
            text_end("msg-123"),
            custom_event,
        ]
    );
}

#[tokio::test]
async fn tool_call_result_closes_pending_text_message() {
    let result_event = Event::ToolCallResult(agui_rs_core::ToolCallResultEvent {
        message_id: "tool-msg-1".into(),
        tool_call_id: "tc-1".into(),
        content: "done".into(),
        attributable: AttributableFields::default(),
        role: None,
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
        result_event.clone(),
    ])
    .await;

    assert_eq!(
        events,
        vec![
            text_start("msg-123", TextMessageRole::Assistant, None),
            text_content("msg-123", "Hello"),
            text_end("msg-123"),
            result_event,
        ]
    );
}

// === subagentRunId propagation (TS chunks/__tests__/subagent-chunks.test.ts) ===

fn chunk_attributable(subagent_run_id: Option<&str>) -> AttributableFields {
    AttributableFields {
        subagent_run_id: subagent_run_id.map(str::to_string),
    }
}

fn attributed_text_chunk(
    message_id: Option<&str>,
    delta: &str,
    subagent_run_id: Option<&str>,
) -> Event {
    Event::TextMessageChunk(TextMessageChunkEvent {
        message_id: message_id.map(str::to_string),
        role: None,
        attributable: chunk_attributable(subagent_run_id),
        delta: Some(delta.to_string()),
        name: None,
        base: BaseEventFields::default(),
    })
}

fn text_start_owned(message_id: &str, subagent_run_id: Option<&str>) -> Event {
    Event::TextMessageStart(TextMessageStartEvent {
        message_id: message_id.to_string(),
        role: TextMessageRole::Assistant,
        name: None,
        base: BaseEventFields::default(),
        attributable: chunk_attributable(subagent_run_id),
    })
}

fn text_content_owned(message_id: &str, delta: &str, subagent_run_id: Option<&str>) -> Event {
    Event::TextMessageContent(TextMessageContentEvent {
        message_id: message_id.to_string(),
        delta: delta.to_string(),
        base: BaseEventFields::default(),
        attributable: chunk_attributable(subagent_run_id),
    })
}

fn text_end_owned(message_id: &str, subagent_run_id: Option<&str>) -> Event {
    Event::TextMessageEnd(TextMessageEndEvent {
        message_id: message_id.to_string(),
        base: BaseEventFields::default(),
        attributable: chunk_attributable(subagent_run_id),
    })
}

#[tokio::test]
async fn propagates_subagent_run_id_to_synthesized_start_content_and_end() {
    // TS subagent-chunks.test.ts "propagate subagentRunId from TEXT_MESSAGE_CHUNK
    // to synthesized TEXT_MESSAGE_START" + "carry the opener's subagentRunId onto
    // the synthesized END" + "carry the incoming chunk's subagentRunId onto
    // synthesized CONTENT".
    let close_event = factory::run_finished("t", "r");
    let events = collect_ok(vec![
        attributed_text_chunk(Some("m1"), "hi", Some("sub-1")),
        close_event,
    ])
    .await;

    assert_eq!(
        events,
        vec![
            text_start_owned("m1", Some("sub-1")),
            text_content_owned("m1", "hi", Some("sub-1")),
            text_end_owned("m1", Some("sub-1")),
            factory::run_finished("t", "r"),
        ]
    );
}

#[tokio::test]
async fn keeps_interleaved_subagents_open_with_per_lane_assembly() {
    // TS chunk-lanes.test.ts "keeps both subagents' text messages open when their
    // chunks interleave": m2's opener must not close m1, and an id-less, tagged
    // continuation lands back on its own lane.
    let close_event = factory::run_finished("t", "r");
    let events = collect_ok(vec![
        attributed_text_chunk(Some("m1"), "A", Some("s1")),
        attributed_text_chunk(Some("m2"), "B", Some("s2")),
        attributed_text_chunk(None, "C", Some("s1")),
        close_event,
    ])
    .await;

    let contents: Vec<_> = events
        .iter()
        .filter(|e| matches!(e, Event::TextMessageContent(_)))
        .collect();
    assert_eq!(
        contents,
        vec![
            &text_content_owned("m1", "A", Some("s1")),
            &text_content_owned("m2", "B", Some("s2")),
            &text_content_owned("m1", "C", Some("s1")),
        ]
    );
    let ends = events
        .iter()
        .filter(|e| matches!(e, Event::TextMessageEnd(_)))
        .count();
    assert_eq!(ends, 2);
}

#[tokio::test]
async fn untagged_continuation_prefers_the_parent_lane() {
    // TS chunk-lanes.test.ts "keeps the parent's own stream separate from a
    // subagent's": a chunk with no tag belongs to the parent even while a
    // subagent's stream is open.
    let close_event = factory::run_finished("t", "r");
    let events = collect_ok(vec![
        attributed_text_chunk(Some("p1"), "P", None),
        attributed_text_chunk(Some("m1"), "A", Some("s1")),
        attributed_text_chunk(None, "Q", None),
        close_event,
    ])
    .await;

    let contents: Vec<_> = events
        .iter()
        .filter(|e| matches!(e, Event::TextMessageContent(_)))
        .collect();
    assert_eq!(
        contents,
        vec![
            &text_content_owned("p1", "P", None),
            &text_content_owned("m1", "A", Some("s1")),
            &text_content_owned("p1", "Q", None),
        ]
    );
}

#[tokio::test]
async fn rejects_a_disagreeing_owner_on_an_open_stream() {
    // TS subagent-chunks.test.ts "reject an owner change on the same chunk
    // stream" and "reject an owner change even when the chunk carries no delta".
    let conflicting = Event::TextMessageChunk(TextMessageChunkEvent {
        message_id: Some("m1".into()),
        role: None,
        attributable: chunk_attributable(Some("s2")),
        delta: None,
        name: None,
        base: BaseEventFields::default(),
    });
    let events = collect(vec![
        attributed_text_chunk(Some("m1"), "A", Some("s1")),
        conflicting,
    ])
    .await;
    assert_validation(
        &events[events.len() - 1],
        "does not match the open stream's subagent",
    );
}

#[tokio::test]
async fn closes_only_the_finishing_subagents_lane() {
    // TS chunk-lanes.test.ts "closes only the finishing subagent's lane":
    // s1's terminal closes s1's message before the terminal; s2 is untouched.
    let close_event = factory::run_finished("t", "r");
    let finished = Event::SubagentFinished(agui_rs_core::SubagentFinishedEvent {
        subagent_run_id: "s1".into(),
        result: None,
        outcome: None,
        base: BaseEventFields::default(),
    });
    let events = collect_ok(vec![
        attributed_text_chunk(Some("m1"), "A", Some("s1")),
        attributed_text_chunk(Some("m2"), "B", Some("s2")),
        finished.clone(),
        attributed_text_chunk(None, "C", Some("s2")),
        close_event,
    ])
    .await;

    let ends: Vec<_> = events
        .iter()
        .filter(|e| matches!(e, Event::TextMessageEnd(_)))
        .collect();
    assert_eq!(
        ends,
        vec![
            &text_end_owned("m1", Some("s1")),
            &text_end_owned("m2", Some("s2"))
        ]
    );
    // s1's END lands before its terminal.
    let end_pos = events
        .iter()
        .position(|e| matches!(e, Event::TextMessageEnd(_)));
    let terminal_pos = events
        .iter()
        .position(|e| matches!(e, Event::SubagentFinished(_)));
    assert!(end_pos.unwrap() < terminal_pos.unwrap());
}

#[tokio::test]
async fn does_not_close_a_subagents_lane_on_an_untagged_own_lane_closer() {
    // TS chunk-lanes.test.ts "does not close a subagent's lane on an untagged
    // TOOL_CALL_RESULT / STEP_FINISHED": an untagged closer names the parent
    // lane, so a subagent's stream survives it.
    let close_event = factory::run_finished("t", "r");
    let step = factory::step_finished("work");
    let events = collect_ok(vec![
        attributed_text_chunk(Some("m1"), "A", Some("s1")),
        step,
        attributed_text_chunk(None, "B", Some("s1")),
        close_event,
    ])
    .await;

    let contents: Vec<_> = events
        .iter()
        .filter(|e| matches!(e, Event::TextMessageContent(_)))
        .collect();
    assert_eq!(
        contents,
        vec![
            &text_content_owned("m1", "A", Some("s1")),
            &text_content_owned("m1", "B", Some("s1")),
        ]
    );
    let ends = events
        .iter()
        .filter(|e| matches!(e, Event::TextMessageEnd(_)))
        .count();
    assert_eq!(ends, 1);
}

#[tokio::test]
async fn closes_every_lane_at_run_finished_in_the_order_they_opened() {
    // TS chunk-lanes.test.ts "closes every open lane at RUN_FINISHED, in the
    // order they opened".
    let close_event = factory::run_finished("t", "r");
    let events = collect_ok(vec![
        attributed_text_chunk(Some("m2"), "A", Some("s2")),
        attributed_text_chunk(Some("p1"), "B", None),
        attributed_text_chunk(Some("m1"), "C", Some("s1")),
        close_event.clone(),
    ])
    .await;

    let ends: Vec<_> = events
        .iter()
        .filter(|e| matches!(e, Event::TextMessageEnd(_)))
        .collect();
    assert_eq!(
        ends,
        vec![
            &text_end_owned("m2", Some("s2")),
            &text_end_owned("p1", None),
            &text_end_owned("m1", Some("s1")),
        ]
    );
    assert_eq!(*events.last().unwrap(), close_event);
}

#[tokio::test]
async fn passes_through_subagent_lifecycle_events_untouched() {
    // TS subagent-chunks.test.ts "pass through SUBAGENT_STARTED events
    // unchanged" — and STARTED must not close any lane.
    let started = Event::SubagentStarted(agui_rs_core::SubagentStartedEvent {
        subagent_run_id: "s1".into(),
        name: "research-agent".into(),
        description: None,
        parent_subagent_run_id: None,
        parent_tool_call_id: None,
        parent_message_id: None,
        base: BaseEventFields::default(),
    });
    let close_event = factory::run_finished("t", "r");
    let events = collect_ok(vec![
        attributed_text_chunk(Some("m1"), "A", Some("s1")),
        started.clone(),
        attributed_text_chunk(None, "B", Some("s1")),
        close_event,
    ])
    .await;

    let contents: Vec<_> = events
        .iter()
        .filter(|e| matches!(e, Event::TextMessageContent(_)))
        .collect();
    assert_eq!(
        contents,
        vec![
            &text_content_owned("m1", "A", Some("s1")),
            &text_content_owned("m1", "B", Some("s1")),
        ]
    );
    assert!(events.contains(&started));
}

// ---------------------------------------------------------------------------
// Chunk metadata / rawEvent propagation — the Rust port of
// `transform-metadata.test.ts` and the rawEvent-only case in
// `opener-agreement.test.ts` ("transformChunks rawEvent-only chunks").
// ---------------------------------------------------------------------------

fn metadata_chunk(
    message_id: Option<&str>,
    delta: Option<&str>,
    role: Option<TextMessageRole>,
    metadata: serde_json::Value,
) -> Event {
    Event::TextMessageChunk(TextMessageChunkEvent {
        message_id: message_id.map(str::to_string),
        role,
        delta: delta.map(str::to_string),
        name: None,
        base: BaseEventFields {
            timestamp: None,
            raw_event: None,
            metadata: Some(metadata),
        },
        attributable: AttributableFields::default(),
    })
}

#[tokio::test]
async fn stamps_text_chunk_metadata_onto_both_synthesized_events() {
    // transform-metadata.test.ts "stamps a text chunk's metadata onto both
    // synthesized events".
    let events = collect_ok(vec![metadata_chunk(
        Some("m1"),
        Some("Hello"),
        None,
        json!({"source": "openai"}),
    )])
    .await;

    assert_eq!(events.len(), 2);
    match (&events[0], &events[1]) {
        (Event::TextMessageStart(start), Event::TextMessageContent(content)) => {
            assert_eq!(start.base.metadata, Some(json!({"source": "openai"})));
            assert_eq!(content.base.metadata, Some(json!({"source": "openai"})));
            // The START deliberately claims no rawEvent (withChunkMetadata);
            // only the CONTENT carries it (withChunkOrigin).
            assert_eq!(start.base.raw_event, None);
        }
        other => panic!("unexpected events {other:?}"),
    }
}

#[tokio::test]
async fn final_metadata_only_text_chunk_reaches_the_reducer_as_zero_delta_content() {
    // transform-metadata.test.ts "preserves metadata from a text chunk that
    // carries no delta" — the regression test for S2: token usage and finish
    // reason ride the LAST chunk, which is often delta-less.
    let events = collect_ok(vec![
        create_text_message_chunk_event(
            Some("m1".into()),
            None,
            Some("Hello".into()),
            None,
            None,
            None,
        ),
        metadata_chunk(
            Some("m1"),
            None,
            None,
            json!({"usage": {"output": 340}, "finishReason": "stop"}),
        ),
        factory::run_finished("t", "r"),
    ])
    .await;

    // START("Hello") CONTENT("Hello") CONTENT("") END RUN_FINISHED — the
    // zero-delta carrier sits just before the synthetic END.
    let last = &events[events.len() - 3];
    match last {
        Event::TextMessageContent(content) => {
            assert_eq!(content.delta, "");
            assert_eq!(content.message_id, "m1");
            assert_eq!(
                content.base.metadata,
                Some(json!({"usage": {"output": 340}, "finishReason": "stop"}))
            );
        }
        other => panic!("expected zero-delta CONTENT, got {other:?}"),
    }
}

#[tokio::test]
async fn mid_stream_chunk_metadata_is_preserved() {
    // transform-metadata.test.ts "does not carry a metadata-only chunk's
    // metadata onto the next message" — m1's metadata stays with m1.
    let events = collect_ok(vec![
        create_text_message_chunk_event(
            Some("m1".into()),
            None,
            Some("one".into()),
            None,
            None,
            None,
        ),
        metadata_chunk(Some("m1"), None, None, json!({"belongsTo": "m1"})),
        create_text_message_chunk_event(
            Some("m2".into()),
            None,
            Some("two".into()),
            None,
            None,
            None,
        ),
    ])
    .await;

    let m2_events: Vec<_> = events
        .iter()
        .filter(|e| {
            matches!(
                e,
                Event::TextMessageStart(TextMessageStartEvent { message_id, .. })
                    | Event::TextMessageContent(TextMessageContentEvent { message_id, .. })
                    if message_id == "m2"
            )
        })
        .collect();
    assert!(!m2_events.is_empty());
    for e in &m2_events {
        let base = match e {
            Event::TextMessageStart(s) => &s.base,
            Event::TextMessageContent(c) => &c.base,
            _ => unreachable!(),
        };
        assert_eq!(base.metadata, None, "m2 must not inherit m1's metadata");
    }
}

#[tokio::test]
async fn synthetic_end_never_carries_chunk_metadata() {
    // transform-metadata.test.ts "does not put the new chunk's metadata on the
    // END that closes the previous message" and "does not leak across a switch
    // from a text chunk to a tool call chunk".
    let events = collect_ok(vec![
        create_text_message_chunk_event(
            Some("m1".into()),
            None,
            Some("first".into()),
            None,
            None,
            None,
        ),
        metadata_chunk(Some("m1"), None, None, json!({"belongsTo": "m1"})),
        create_text_message_chunk_event(
            Some("m2".into()),
            None,
            Some("second".into()),
            None,
            None,
            None,
        ),
        factory::run_finished("t", "r"),
    ])
    .await;

    for e in &events {
        if let Event::TextMessageEnd(end) = e {
            assert_eq!(end.base.metadata, None, "synthetic END is built bare");
        }
    }
}

#[tokio::test]
async fn tool_and_reasoning_metadata_only_chunks_emit_zero_delta_events() {
    // transform-metadata.test.ts "preserves metadata from a tool call chunk
    // that carries no delta" and "...from a reasoning chunk that carries no
    // delta".
    let events = collect_ok(vec![
        create_tool_call_chunk_event(
            Some("tc1".into()),
            Some("search".into()),
            None,
            Some("{}".into()),
            None,
            None,
        ),
        Event::ToolCallChunk(agui_rs_core::ToolCallChunkEvent {
            tool_call_id: Some("tc1".into()),
            tool_call_name: None,
            parent_message_id: None,
            delta: None,
            base: BaseEventFields {
                timestamp: None,
                raw_event: None,
                metadata: Some(json!({"latencyMs": 12})),
            },
            attributable: AttributableFields::default(),
        }),
        factory::run_finished("t", "r"),
    ])
    .await;
    // START ARGS("{}") ARGS("") END RUN_FINISHED — the zero-delta carrier
    // follows the real args.
    match &events[2] {
        Event::ToolCallArgs(args) => {
            assert_eq!(args.delta, "");
            assert_eq!(args.base.metadata, Some(json!({"latencyMs": 12})));
        }
        other => panic!("expected zero-delta ARGS, got {other:?}"),
    }

    let events = collect_ok(vec![
        Event::ReasoningMessageChunk(ReasoningMessageChunkEvent {
            message_id: Some("r1".into()),
            delta: Some("thinking".into()),
            base: BaseEventFields::default(),
            attributable: AttributableFields::default(),
        }),
        Event::ReasoningMessageChunk(ReasoningMessageChunkEvent {
            message_id: Some("r1".into()),
            delta: None,
            base: BaseEventFields {
                timestamp: None,
                raw_event: None,
                metadata: Some(json!({"tokens": 7})),
            },
            attributable: AttributableFields::default(),
        }),
        factory::run_finished("t", "r"),
    ])
    .await;
    // START CONTENT CONTENT("") END RUN_FINISHED — index 2 is the carrier.
    match &events[2] {
        Event::ReasoningMessageContent(content) => {
            assert_eq!(content.delta, "");
            assert_eq!(content.base.metadata, Some(json!({"tokens": 7})));
        }
        other => panic!("expected zero-delta CONTENT, got {other:?}"),
    }
}

#[tokio::test]
async fn raw_event_only_first_chunk_rides_a_zero_delta_content_event() {
    // opener-agreement.test.ts "carries a first chunk's rawEvent on a
    // zero-delta content event": the synthesized START claims no rawEvent, so
    // a payload-only first chunk needs the CONTENT as its carrier.
    let chunk = create_text_message_chunk_event(
        Some("msg-1".into()),
        None,
        None,
        None,
        None,
        Some(json!({"provider": "payload"})),
    );
    let events = collect_ok(vec![chunk, factory::run_finished("t", "r")]).await;

    assert!(matches!(events[0], Event::TextMessageStart(_)));
    match &events[1] {
        Event::TextMessageContent(content) => {
            assert_eq!(content.delta, "");
            assert_eq!(content.base.raw_event, Some(json!({"provider": "payload"})));
        }
        other => panic!("expected CONTENT carrier, got {other:?}"),
    }
}

#[tokio::test]
async fn continuation_role_assistant_agrees_with_a_roleless_opener() {
    // opener-agreement.test.ts "accepts a continuation repeating the assistant
    // role the opener defaulted to": the open stream stores the RESOLVED
    // role, so `role:"assistant"` after a role-less opener agrees rather than
    // hard-failing the whole run.
    let events = collect_ok(vec![
        create_text_message_chunk_event(
            Some("m1".into()),
            None,
            Some("a".into()),
            None,
            None,
            None,
        ),
        create_text_message_chunk_event(
            Some("m1".into()),
            Some(TextMessageRole::Assistant),
            Some("b".into()),
            None,
            None,
            None,
        ),
        factory::run_finished("t", "r"),
    ])
    .await;
    assert!(matches!(events[0], Event::TextMessageStart(_)));
    assert_eq!(events.len(), 5); // start, content, content, end, run_finished

    // And a genuinely conflicting role still errors.
    let results = collect(vec![
        create_text_message_chunk_event(
            Some("m2".into()),
            None,
            Some("a".into()),
            None,
            None,
            None,
        ),
        create_text_message_chunk_event(
            Some("m2".into()),
            Some(TextMessageRole::User),
            Some("b".into()),
            None,
            None,
            None,
        ),
    ])
    .await;
    // [START, CONTENT, Err] — the error lands on the second chunk.
    assert_validation(
        &results[2],
        "chunk role 'user' does not match the open stream's role 'assistant'",
    );
}
