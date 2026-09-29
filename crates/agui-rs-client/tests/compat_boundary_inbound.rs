//! The always-on inbound compatibility boundary, ported from the TypeScript
//! `CompatibilityBoundary` inbound THINKING_* conversions.
//!
//! The conversions are not version-gated: a `THINKING_*` event arriving is
//! itself the proof the peer is old. They run on the RAW stream, before typed
//! deserialization, because the 1.0 event set has no `THINKING_*` variants and
//! serde would otherwise reject such a frame before anything could see it.

use agui_rs_client::{parse_sse_stream, Event};
use bytes::Bytes;
use futures::{stream, StreamExt};

/// One SSE frame per line of `json`, all delivered in a single chunk.
fn sse(json: &str) -> Vec<Result<Bytes, &'static str>> {
    let frames = json
        .lines()
        .map(|line| format!("data: {line}\n\n"))
        .collect::<String>();
    vec![Ok(Bytes::from(frames))]
}

async fn parse(chunks: Vec<Result<Bytes, &'static str>>) -> Vec<Event> {
    parse_sse_stream(stream::iter(chunks))
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .map(|event| event.expect("event should parse"))
        .collect()
}

#[tokio::test]
async fn maps_inbound_thinking_events_to_reasoning_without_any_opt_in() {
    let events = parse(sse(r#"{"type":"THINKING_START","title":"plan"}
{"type":"THINKING_TEXT_MESSAGE_START","role":"assistant"}
{"type":"THINKING_TEXT_MESSAGE_CONTENT","delta":"hello"}
{"type":"THINKING_TEXT_MESSAGE_END"}
{"type":"THINKING_END"}"#))
    .await;

    assert_eq!(events.len(), 5);
    let (Event::ReasoningStart(start), Event::ReasoningMessageStart(message_start)) =
        (&events[0], &events[1])
    else {
        panic!("expected reasoning events, got {events:?}");
    };
    let Event::ReasoningMessageContent(content) = &events[2] else {
        panic!("expected reasoning message content, got {events:?}");
    };
    let Event::ReasoningMessageEnd(message_end) = &events[3] else {
        panic!("expected reasoning message end, got {events:?}");
    };
    let Event::ReasoningEnd(end) = &events[4] else {
        panic!("expected reasoning end, got {events:?}");
    };

    assert_ne!(start.message_id, message_start.message_id);
    assert_eq!(content.message_id, message_start.message_id);
    assert_eq!(content.delta, "hello");
    assert_eq!(message_end.message_id, message_start.message_id);
    assert_eq!(end.message_id, start.message_id);
    // The schema pins this role; the old translation said "assistant".
    assert_eq!(
        message_start.role,
        agui_rs_core::ReasoningMessageRole::Reasoning
    );
}

#[tokio::test]
async fn drops_the_thinking_start_title_that_reasoning_cannot_carry() {
    // Nothing in the typed event holds the span's label: REASONING_START has
    // no title field, so it is gone once the frame is parsed.
    let events = parse(sse(r#"{"type":"THINKING_START","title":"plan"}"#)).await;
    assert!(matches!(&events[0], Event::ReasoningStart(_)));
}

#[tokio::test]
async fn mints_an_id_for_a_thinking_continuation_with_no_opener() {
    let events = parse(sse(
        r#"{"type":"THINKING_TEXT_MESSAGE_CONTENT","delta":"orphan"}"#,
    ))
    .await;

    match &events[0] {
        Event::ReasoningMessageContent(content) => {
            assert!(!content.message_id.is_empty());
            assert_eq!(content.delta, "orphan");
        }
        other => panic!("expected reasoning message content, got {other:?}"),
    }
}

#[tokio::test]
async fn leaves_a_modern_stream_untouched() {
    let events = parse(sse(concat!(
        r#"{"type":"RUN_STARTED","threadId":"t1","runId":"r1"}"#,
        "\n",
        r#"{"type":"TEXT_MESSAGE_START","messageId":"m1","role":"assistant"}"#,
        "\n",
        r#"{"type":"TEXT_MESSAGE_CONTENT","messageId":"m1","delta":"hi"}"#,
        "\n",
        r#"{"type":"TEXT_MESSAGE_END","messageId":"m1"}"#,
    )))
    .await;

    assert!(matches!(&events[0], Event::RunStarted(_)));
    match &events[1] {
        Event::TextMessageStart(start) => assert_eq!(start.message_id, "m1"),
        other => panic!("expected text message start, got {other:?}"),
    }
    assert!(matches!(&events[2], Event::TextMessageContent(_)));
    assert!(matches!(&events[3], Event::TextMessageEnd(_)));
}
