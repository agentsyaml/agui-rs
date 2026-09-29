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

/// Mirrors the official TS `EventEncoder`: `preferredMediaTypes(accept,
/// [AGUI_MEDIA_TYPE]).includes(AGUI_MEDIA_TYPE)`. Table below is the
/// negotiation contract of ag-ui 1.0.0's `media-type.ts`.
#[test]
fn content_negotiation_matches_official_media_type_negotiator() {
    const P: &str = AGUI_MEDIA_TYPE_PROTOBUF;
    let cases: &[(&str, bool)] = &[
        // 1. absent / empty accept never negotiates
        ("", false),
        // 2. wildcards match the single offered type
        ("*/*", true),
        ("application/*", true),
        // 3. exact match
        (P, true),
        // 4. q=0 is filtered out by `spec.q > 0` (media-type.ts:36)
        ("*/*;q=0", false),
        ("application/*;q=0", false),
        (concat!("application/vnd.ag-ui.event+proto", ";q=0"), false),
        // 5. multi-entry: specificity outranks q and header order
        ("application/vnd.ag-ui.event+proto, */*;q=0", true),
        ("application/vnd.ag-ui.event+proto;q=0, */*", false),
        (
            "text/event-stream, application/vnd.ag-ui.event+proto;q=0.5",
            true,
        ),
        ("text/event-stream, */*;q=0.5", true),
        // 6. malformed q -> NaN -> rejected
        ("application/vnd.ag-ui.event+proto;q=bogus", false),
        // 7. params only collected before `q`
        ("application/vnd.ag-ui.event+proto;version=2", false),
        ("application/vnd.ag-ui.event+proto;q=0.5;version=2", true),
        // 8. case-insensitive type/subtype, non-matching entries dropped
        ("APPLICATION/VND.AG-UI.EVENT+PROTO", true),
        ("application/json", false),
        ("text/*", false),
    ];

    for (accept, expected) in cases {
        let enc = EventEncoder::with_accept(Some(accept));
        assert_eq!(
            enc.accepts_protobuf(),
            *expected,
            "accept={accept:?} expected protobuf={expected}"
        );
        assert_eq!(
            enc.content_type(),
            if *expected {
                AGUI_MEDIA_TYPE_PROTOBUF
            } else {
                AGUI_MEDIA_TYPE_SSE
            },
            "accept={accept:?}"
        );
    }
}

#[test]
fn encode_binary_uses_protobuf_for_wildcard_accept() {
    let encoder = EventEncoder::with_accept(Some("*/*"));

    let bytes = encoder
        .encode_binary(&sample_event())
        .expect("binary encoding should use protobuf");

    assert_eq!(encoder.content_type(), AGUI_MEDIA_TYPE_PROTOBUF);
    // Length-prefixed protobuf, not an SSE frame.
    let len = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
    assert_eq!(len, bytes.len() - 4);
    let decoded = agui_rs_proto::decode(&bytes[4..]).expect("decode should succeed");
    assert_eq!(decoded, sample_event());
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
