//! Ported from TypeScript `transform/__tests__/proto.test.ts`.
//!
//! Protobuf decoding is now implemented (`agui-rs-proto` + `parse_proto_stream`),
//! so these exercise real length-prefixed frame decoding, including events
//! batched into one chunk and split across chunks.

use agui_rs_client::{parse_proto_stream, AGUI_MEDIA_TYPE_PROTOBUF};
use agui_rs_core::{factory, BaseEventFields, Event, MessagesSnapshotEvent, StateDeltaEvent};
use agui_rs_core::types::AssistantMessage;
use agui_rs_core::Message;
use agui_rs_encoder::EventEncoder;
use futures::{stream, StreamExt};
use serde_json::json;

fn proto_encoder() -> EventEncoder {
    EventEncoder::with_accept(Some(AGUI_MEDIA_TYPE_PROTOBUF))
}

/// Encodes events to length-prefixed protobuf frames concatenated together.
fn encode_all(events: &[Event]) -> Vec<u8> {
    let encoder = proto_encoder();
    let mut bytes = Vec::new();
    for event in events {
        bytes.extend_from_slice(&encoder.encode_protobuf(event).expect("encode"));
    }
    bytes
}

async fn decode_bytes(chunks: Vec<Vec<u8>>) -> Vec<Result<Event, agui_rs_core::AgUiError>> {
    let items = chunks
        .into_iter()
        .map(|c| Ok::<bytes::Bytes, std::convert::Infallible>(bytes::Bytes::from(c)))
        .collect::<Vec<_>>();
    parse_proto_stream(stream::iter(items))
        .collect::<Vec<_>>()
        .await
}

#[tokio::test]
async fn decodes_a_single_protobuf_event() {
    let events = vec![factory::run_started("t1", "r1")];
    let out = decode_bytes(vec![encode_all(&events)]).await;
    assert_eq!(out.len(), 1);
    assert!(matches!(out[0], Ok(Event::RunStarted(_))));
}

#[tokio::test]
async fn handles_multiple_protobuf_events_in_a_single_chunk() {
    let events = vec![
        factory::run_started("t1", "r1"),
        factory::text_message_start("m1"),
        factory::run_finished("t1", "r1"),
    ];
    let out = decode_bytes(vec![encode_all(&events)]).await;
    assert_eq!(out.len(), 3);
    assert!(matches!(out[0], Ok(Event::RunStarted(_))));
    assert!(matches!(out[1], Ok(Event::TextMessageStart(_))));
    assert!(matches!(out[2], Ok(Event::RunFinished(_))));
}

#[tokio::test]
async fn handles_protobuf_event_split_across_chunks() {
    let events = vec![factory::run_started("thread-1", "run-1")];
    let full = encode_all(&events);
    // Split the single frame across two byte chunks.
    let mid = full.len() / 2;
    let out = decode_bytes(vec![full[..mid].to_vec(), full[mid..].to_vec()]).await;
    assert_eq!(out.len(), 1);
    match &out[0] {
        Ok(Event::RunStarted(e)) => {
            assert_eq!(e.thread_id, "thread-1");
            assert_eq!(e.run_id, "run-1");
        }
        other => panic!("unexpected: {other:?}"),
    }
}

#[tokio::test]
async fn round_trips_state_delta_with_json_patch_operations() {
    let event = Event::StateDelta(StateDeltaEvent {
        delta: vec![
            json!({"op": "replace", "path": "/count", "value": 2}),
            json!({"op": "add", "path": "/items/-", "value": "x"}),
        ],
        base: BaseEventFields::default(),
    });
    let out = decode_bytes(vec![encode_all(&[event])]).await;
    assert_eq!(out.len(), 1);
    // protobuf `google.protobuf.Value` represents every number as a double, so
    // integers round-trip as floats (2 -> 2.0). This matches the canonical TS
    // proto behaviour, which uses the same Struct/Value representation.
    let expected = Event::StateDelta(StateDeltaEvent {
        delta: vec![
            json!({"op": "replace", "path": "/count", "value": 2.0}),
            json!({"op": "add", "path": "/items/-", "value": "x"}),
        ],
        base: BaseEventFields::default(),
    });
    assert_eq!(out[0].as_ref().unwrap(), &expected);
}

#[tokio::test]
async fn round_trips_messages_snapshot() {
    let event = Event::MessagesSnapshot(MessagesSnapshotEvent {
        messages: vec![Message::Assistant(AssistantMessage {
            id: "a1".into(),
            content: Some("hi".into()),
            name: None,
            tool_calls: None,
            encrypted_value: None,
        })],
        base: BaseEventFields::default(),
    });
    let out = decode_bytes(vec![encode_all(std::slice::from_ref(&event))]).await;
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].as_ref().unwrap(), &event);
}

#[tokio::test]
async fn emits_error_on_invalid_protobuf_frame() {
    // A 4-byte length header claiming 3 bytes, followed by junk that is not a
    // valid Event message.
    let mut bad = vec![0u8, 0, 0, 3];
    bad.extend_from_slice(&[0xff, 0xff, 0xff]);
    let out = decode_bytes(vec![bad]).await;
    assert_eq!(out.len(), 1);
    assert!(out[0].is_err(), "invalid protobuf should yield an error");
}
