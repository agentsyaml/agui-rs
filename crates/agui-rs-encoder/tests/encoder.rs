use agui_rs_core::{factory, AgUiError};
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
fn encode_binary_returns_unsupported_when_protobuf_is_requested() {
    let encoder = EventEncoder::with_accept(Some(AGUI_MEDIA_TYPE_PROTOBUF));

    let err = encoder
        .encode_binary(&sample_event())
        .expect_err("protobuf encoding is not implemented");

    match err {
        AgUiError::Unsupported(message) => {
            assert!(message.contains("protobuf encoding is not implemented"));
        }
        other => panic!("expected unsupported error, got {other:?}"),
    }
}

#[test]
fn encode_protobuf_returns_unsupported_error() {
    let encoder = EventEncoder::new();

    let err = encoder
        .encode_protobuf(&sample_event())
        .expect_err("protobuf encoding is not implemented");

    match err {
        AgUiError::Unsupported(message) => {
            assert!(message.contains("protobuf encoding is not implemented"));
        }
        other => panic!("expected unsupported error, got {other:?}"),
    }
}

// SKIPPED: should return protobuf encoded data when accept header includes protobuf media type: Rust encoder currently negotiates protobuf content type but returns AgUiError::Unsupported because protobuf encoding is not implemented in this build.
// SKIPPED: should encode event as protobuf with length prefix: Rust encoder currently returns AgUiError::Unsupported from encode_protobuf instead of emitting length-prefixed protobuf bytes.
