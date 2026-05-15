use ag_ui_client::{parse_sse_stream, AgUiError, Event};
use ag_ui_core::TextMessageRole;
use bytes::Bytes;
use futures::{stream, StreamExt};
use tokio::time::{timeout, Duration};
use tokio_stream::wrappers::ReceiverStream;

type ByteResult = std::result::Result<Bytes, &'static str>;

fn byte_chunk(data: &str) -> ByteResult {
    Ok(Bytes::from(data.to_owned()))
}

#[tokio::test]
async fn emits_events_as_soon_as_complete_sse_events_arrive() {
    let (tx, rx) = tokio::sync::mpsc::channel::<ByteResult>(4);
    let mut events = parse_sse_stream(ReceiverStream::new(rx));

    tx.send(byte_chunk(
        "data: {\"type\":\"TEXT_MESSAGE_START\",\"messageId\":\"1\",\"role\":\"assistant\"}\n\n",
    ))
    .await
    .expect("send first chunk");

    let first = timeout(Duration::from_secs(1), events.next())
        .await
        .expect("first event should arrive")
        .expect("stream should yield first item")
        .expect("first event should parse");
    assert!(matches!(
        first,
        Event::TextMessageStart(ref event)
            if event.message_id == "1" && event.role == TextMessageRole::Assistant
    ));

    tx.send(byte_chunk(
        "data: {\"type\":\"TEXT_MESSAGE_CONTENT\",\"messageId\":\"1\",\"delta\":\"Hello\"}\n\n",
    ))
    .await
    .expect("send second chunk");
    drop(tx);

    let second = timeout(Duration::from_secs(1), events.next())
        .await
        .expect("second event should arrive")
        .expect("stream should yield second item")
        .expect("second event should parse");
    assert!(matches!(
        second,
        Event::TextMessageContent(ref event)
            if event.message_id == "1" && event.delta == "Hello"
    ));
}

#[tokio::test]
async fn handles_multiple_complete_sse_events_in_a_single_chunk() {
    let events = parse_sse_stream(stream::iter(vec![byte_chunk(concat!(
        "data: {\"type\":\"TEXT_MESSAGE_START\",\"messageId\":\"1\",\"role\":\"assistant\"}\n\n",
        "data: {\"type\":\"TEXT_MESSAGE_CONTENT\",\"messageId\":\"1\",\"delta\":\"Hello\"}\n\n"
    ))]))
    .collect::<Vec<_>>()
    .await;

    assert_eq!(events.len(), 2);
    assert!(matches!(
        &events[0],
        Ok(Event::TextMessageStart(event))
            if event.message_id == "1" && event.role == TextMessageRole::Assistant
    ));
    assert!(matches!(
        &events[1],
        Ok(Event::TextMessageContent(event))
            if event.message_id == "1" && event.delta == "Hello"
    ));
}

#[tokio::test]
async fn handles_split_sse_event_across_multiple_chunks() {
    let first = parse_sse_stream(stream::iter(vec![
        byte_chunk("data: {\"type\": \"TEXT_MESSAGE"),
        byte_chunk("_START\", \"messageId\": "),
        byte_chunk("\"1\", \"role\": \"assistant\"}\n\n"),
    ]))
    .next()
    .await
    .expect("event should be emitted")
    .expect("event should parse");

    assert!(matches!(
        first,
        Event::TextMessageStart(ref event)
            if event.message_id == "1" && event.role == TextMessageRole::Assistant
    ));
}

#[tokio::test]
async fn emits_error_for_invalid_json_in_sse_data() {
    let first = parse_sse_stream(stream::iter(vec![byte_chunk(
        "data: {\"type\": \"TEXT_MESSAGE_START\", \"messageId\": \"1\"\n\n",
    )]))
    .next()
    .await
    .expect("stream should yield parse error");

    assert!(matches!(first, Err(AgUiError::Json(_))));
}

#[tokio::test]
async fn handles_multiline_sse_data_fields() {
    let first = parse_sse_stream(stream::iter(vec![byte_chunk(concat!(
        "event: message\n",
        "id: 123\n",
        "data: {\n",
        "data: \"type\": \"TEXT_MESSAGE_CONTENT\",\n",
        "data: \"messageId\": \"1\",\n",
        "data: \"delta\": \"Hello World\"\n",
        "data: }\n\n"
    ))]))
    .next()
    .await
    .expect("event should be emitted")
    .expect("event should parse");

    assert!(matches!(
        first,
        Event::TextMessageContent(ref event)
            if event.message_id == "1" && event.delta == "Hello World"
    ));
}

#[tokio::test]
async fn handles_json_split_between_http_chunks_in_one_sse_event() {
    let first = parse_sse_stream(stream::iter(vec![
        byte_chunk("data: {\"type\": \"TEXT_MESSAGE_CONTENT\", \"messageId\": \"1\""),
        byte_chunk(", \"delta\": \"Hello "),
        byte_chunk("World\"}\n\n"),
    ]))
    .next()
    .await
    .expect("event should be emitted")
    .expect("event should parse");

    assert!(matches!(
        first,
        Event::TextMessageContent(ref event)
            if event.message_id == "1" && event.delta == "Hello World"
    ));
}

#[tokio::test]
async fn handles_data_prefix_split_from_json_content() {
    let first = parse_sse_stream(stream::iter(vec![
        byte_chunk("data: "),
        byte_chunk("{\"type\": \"TEXT_MESSAGE_CONTENT\""),
        byte_chunk(", \"messageId\": \"1\", \"delta\":"),
        byte_chunk(" \"Split JSON Test\"}\n\n"),
    ]))
    .next()
    .await
    .expect("event should be emitted")
    .expect("event should parse");

    assert!(matches!(
        first,
        Event::TextMessageContent(ref event)
            if event.message_id == "1" && event.delta == "Split JSON Test"
    ));
}
