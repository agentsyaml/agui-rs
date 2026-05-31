use agui_rs_client::{
    detect_stream_format, StreamFormat, AGUI_MEDIA_TYPE_PROTOBUF, AGUI_MEDIA_TYPE_SSE,
};

#[test]
fn detects_protobuf_content_type() {
    assert_eq!(
        detect_stream_format(Some(AGUI_MEDIA_TYPE_PROTOBUF)),
        StreamFormat::Protobuf
    );
    assert_eq!(
        detect_stream_format(Some(&format!("{AGUI_MEDIA_TYPE_PROTOBUF}; charset=binary"))),
        StreamFormat::Protobuf
    );
}

#[test]
fn defaults_to_sse_for_missing_and_non_protobuf_content_types() {
    assert_eq!(detect_stream_format(None), StreamFormat::Sse);
    assert_eq!(
        detect_stream_format(Some(AGUI_MEDIA_TYPE_SSE)),
        StreamFormat::Sse
    );
    assert_eq!(
        detect_stream_format(Some("application/json")),
        StreamFormat::Sse
    );
}

// IMPLEMENTED: protobuf event transform is now supported — `HttpAgent` dispatches
// protobuf responses to `parse_proto_stream`, which decodes length-prefixed
// frames and surfaces malformed frames as `AgUiError`. See transform_proto.rs
// for decode round-trips and error propagation.
// SKIPPED: should emit RUN_ERROR and complete on AbortError without erroring: Rust HttpAgent surfaces cancellation as AgUiError::Cancelled instead of synthesizing a RUN_ERROR event.
// SKIPPED: should error if DATA received before HEADERS: Rust transform API starts after HTTP headers are already resolved, so there is no public headerless DATA state to exercise.
