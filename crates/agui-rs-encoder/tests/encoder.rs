use agui_rs_core::factory;
use agui_rs_encoder::{EventEncoder, AGUI_MEDIA_TYPE_PROTOBUF, AGUI_MEDIA_TYPE_SSE};

fn sample_event() -> agui_rs_core::Event {
    factory::run_started("thread-1", "run-1")
}

#[test]
fn encode_sse_wraps_serialized_event_in_data_frame() {
    let encoder = EventEncoder::new();
    let event = sample_event();

    let frame = encoder
        .encode_sse(&event)
        .expect("sse encoding should succeed");

    assert_eq!(
        frame,
        format!(
            "data: {}\n\n",
            serde_json::to_string(&event).expect("event should serialize")
        )
    );
}

#[test]
fn encode_binary_uses_sse_when_accept_header_is_missing() {
    let encoder = EventEncoder::new();

    let bytes = encoder
        .encode_binary(&sample_event())
        .expect("binary encoding should fall back to sse");

    assert_eq!(encoder.content_type(), AGUI_MEDIA_TYPE_SSE);
    assert_eq!(
        String::from_utf8(bytes).expect("frame should be valid utf-8"),
        encoder
            .encode_sse(&sample_event())
            .expect("sse encoding should succeed")
    );
}

#[test]
fn encode_binary_uses_sse_when_accept_header_excludes_protobuf() {
    let encoder = EventEncoder::with_accept(Some(AGUI_MEDIA_TYPE_SSE));

    let bytes = encoder
        .encode_binary(&sample_event())
        .expect("binary encoding should fall back to sse");

    assert!(!encoder.accepts_protobuf());
    assert_eq!(encoder.content_type(), AGUI_MEDIA_TYPE_SSE);
    assert!(String::from_utf8(bytes)
        .expect("frame should be valid utf-8")
        .starts_with("data: {"));
}

#[test]
fn protobuf_accept_header_switches_content_negotiation() {
    let encoder = EventEncoder::with_accept(Some(&format!(
        "text/event-stream, {AGUI_MEDIA_TYPE_PROTOBUF}"
    )));

    assert!(encoder.accepts_protobuf());
    assert_eq!(encoder.content_type(), AGUI_MEDIA_TYPE_PROTOBUF);
}

#[test]
fn encode_binary_returns_length_prefixed_protobuf_when_requested() {
    let encoder = EventEncoder::with_accept(Some(AGUI_MEDIA_TYPE_PROTOBUF));

    let bytes = encoder
        .encode_binary(&sample_event())
        .expect("protobuf encoding should succeed");

    assert!(encoder.accepts_protobuf());
    // 4-byte big-endian length prefix followed by a body of exactly that length.
    assert!(bytes.len() > 4);
    let len = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
    assert_eq!(len, bytes.len() - 4);
}

#[test]
fn encode_protobuf_round_trips_through_proto_decode() {
    let encoder = EventEncoder::with_accept(Some(AGUI_MEDIA_TYPE_PROTOBUF));
    let event = sample_event();

    let framed = encoder
        .encode_protobuf(&event)
        .expect("protobuf encoding should succeed");
    // Strip the 4-byte length prefix before decoding the message body.
    let body = &framed[4..];
    let decoded = agui_rs_proto::decode(body).expect("decode should succeed");
    assert_eq!(decoded, event);
}
