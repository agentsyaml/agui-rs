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

// SKIPPED: should correctly transform protocol buffer events: protobuf decoding intentionally unsupported in Rust SDK; covered by transform_proto.rs asserting AgUiError::Unsupported.
// SKIPPED: should emit RUN_ERROR and complete on AbortError without erroring: Rust HttpAgent surfaces cancellation as AgUiError::Cancelled instead of synthesizing a RUN_ERROR event.
// SKIPPED: should handle parseProtoStream errors: Rust SDK has no injectable parseProtoStream layer; protobuf content type short-circuits to AgUiError::Unsupported.
// SKIPPED: should error if DATA received before HEADERS: Rust transform API starts after HTTP headers are already resolved, so there is no public headerless DATA state to exercise.
