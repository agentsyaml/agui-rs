use agui_rs_core::types::{BinaryInputContent, Message, UserMessage};
use agui_rs_core::{InputContent, InputContentSource, RunAgentInput, UserMessageContent};
use serde_json::{json, Value};

fn parse_user_message_content(value: Value) -> UserMessageContent {
    serde_json::from_value(value).expect("deserialize user message content")
}

fn parse_input_content(value: Value) -> InputContent {
    serde_json::from_value(value).expect("deserialize input content")
}

fn parse_source(value: Value) -> InputContentSource {
    serde_json::from_value(value).expect("deserialize source")
}

#[test]
fn user_message_parses_content_array() {
    let content = parse_user_message_content(json!([
        { "type": "text", "text": "Check this out" },
        {
            "type": "image",
            "source": {
                "type": "url",
                "value": "https://example.com/image.png",
                "mimeType": "image/png"
            }
        }
    ]));

    match content {
        UserMessageContent::Parts(parts) => {
            assert_eq!(parts.len(), 2);
            assert!(matches!(&parts[0], InputContent::Text { text } if text == "Check this out"));
            assert!(
                matches!(&parts[1], InputContent::Image { source: InputContentSource::Url { value, .. }, .. } if value == "https://example.com/image.png")
            );
        }
        other => panic!("expected parts, got {other:?}"),
    }
}

#[test]
fn image_part_parses_inline_data_source() {
    let part = parse_input_content(json!({
        "type": "image",
        "source": {
            "type": "data",
            "value": "base64-value",
            "mimeType": "image/png"
        },
        "metadata": { "detail": "high" }
    }));

    match part {
        InputContent::Image { source, metadata } => {
            assert!(
                matches!(source, InputContentSource::Data { mime_type, .. } if mime_type == "image/png")
            );
            assert_eq!(metadata, Some(json!({ "detail": "high" })));
        }
        other => panic!("expected image part, got {other:?}"),
    }
}

#[test]
fn url_source_parses_without_mime_type() {
    let source = parse_source(json!({
        "type": "url",
        "value": "https://example.com/file.pdf"
    }));

    assert!(
        matches!(source, InputContentSource::Url { value, mime_type: None } if value == "https://example.com/file.pdf")
    );
}

#[test]
fn data_source_parses_with_mime_type() {
    let source = parse_source(json!({
        "type": "data",
        "value": "Zm9v",
        "mimeType": "application/pdf"
    }));

    assert!(
        matches!(source, InputContentSource::Data { mime_type, .. } if mime_type == "application/pdf")
    );
}

#[test]
fn binary_content_without_payload_source_is_rejected_by_validation() {
    let input = RunAgentInput {
        thread_id: "thread-1".into(),
        run_id: "run-1".into(),
        parent_run_id: None,
        state: json!({}),
        messages: vec![Message::User(UserMessage {
            id: "user_invalid".into(),
            content: UserMessageContent::Parts(vec![InputContent::Binary {
                content: BinaryInputContent {
                    mime_type: "image/png".into(),
                    id: None,
                    url: None,
                    data: None,
                    filename: None,
                },
            }]),
            name: None,
            encrypted_value: None,
        })],
        tools: vec![],
        context: vec![],
        forwarded_props: json!({}),
        resume: None,
    };

    let error = input
        .validate()
        .expect_err("missing binary payload should fail");
    assert!(error.to_string().contains("id, url, or data"));
}

#[test]
fn binary_input_parses_with_embedded_data() {
    let binary = parse_input_content(json!({
        "type": "binary",
        "mimeType": "image/png",
        "data": "base64"
    }));

    assert!(
        matches!(binary, InputContent::Binary { content } if content.data.as_deref() == Some("base64"))
    );
}

#[test]
fn binary_input_requires_payload_source() {
    let binary: BinaryInputContent = serde_json::from_value(json!({
        "mimeType": "image/png"
    }))
    .expect("deserialize binary content without payload");

    let error = binary.validate().expect_err("missing payload should fail");
    assert!(error.to_string().contains("id, url, or data"));
}

fn modality_shape_round_trip(modality: &str, mime_type: &str) {
    let url_with_metadata = parse_input_content(json!({
        "type": modality,
        "source": {
            "type": "url",
            "value": format!("https://example.com/{modality}"),
            "mimeType": mime_type
        },
        "metadata": { "providerHint": "high" }
    }));

    let data_without_metadata = parse_input_content(json!({
        "type": modality,
        "source": {
            "type": "data",
            "value": "Zm9v",
            "mimeType": mime_type
        }
    }));

    let url_without_mime = parse_input_content(json!({
        "type": modality,
        "source": {
            "type": "url",
            "value": format!("https://example.com/{modality}/raw")
        }
    }));

    let data_missing_mime = serde_json::from_value::<InputContent>(json!({
        "type": modality,
        "source": {
            "type": "data",
            "value": "Zm9v"
        }
    }))
    .expect_err("data source without mime type should fail");

    let missing_source = serde_json::from_value::<InputContent>(json!({
        "type": modality
    }))
    .expect_err("missing source should fail");

    let invalid_source = serde_json::from_value::<InputContent>(json!({
        "type": modality,
        "source": {
            "type": "file",
            "value": "abc"
        }
    }))
    .expect_err("invalid source discriminator should fail");

    match url_with_metadata {
        InputContent::Image { source, metadata }
        | InputContent::Audio { source, metadata }
        | InputContent::Video { source, metadata }
        | InputContent::Document { source, metadata } => {
            assert!(matches!(source, InputContentSource::Url { .. }));
            assert_eq!(metadata, Some(json!({ "providerHint": "high" })));
        }
        other => panic!("expected multimodal content, got {other:?}"),
    }

    match data_without_metadata {
        InputContent::Image { source, metadata }
        | InputContent::Audio { source, metadata }
        | InputContent::Video { source, metadata }
        | InputContent::Document { source, metadata } => {
            assert!(
                matches!(source, InputContentSource::Data { mime_type: found, .. } if found == mime_type)
            );
            assert_eq!(metadata, None);
        }
        other => panic!("expected multimodal content, got {other:?}"),
    }

    match url_without_mime {
        InputContent::Image { source, .. }
        | InputContent::Audio { source, .. }
        | InputContent::Video { source, .. }
        | InputContent::Document { source, .. } => {
            assert!(matches!(
                source,
                InputContentSource::Url {
                    mime_type: None,
                    ..
                }
            ));
        }
        other => panic!("expected multimodal content, got {other:?}"),
    }

    assert!(data_missing_mime.to_string().contains("mimeType"));
    assert!(missing_source.to_string().contains("source"));
    assert!(
        invalid_source.to_string().contains("file")
            || invalid_source.to_string().contains("unknown variant")
    );
}

#[test]
fn image_audio_video_and_document_support_url_and_data_sources() {
    for (modality, mime_type) in [
        ("image", "image/png"),
        ("audio", "audio/wav"),
        ("video", "video/mp4"),
        ("document", "application/pdf"),
    ] {
        modality_shape_round_trip(modality, mime_type);
    }
}

#[test]
fn user_message_accepts_all_supported_modalities() {
    let content = parse_user_message_content(json!([
        { "type": "text", "text": "Process all inputs" },
        { "type": "image", "source": { "type": "url", "value": "https://example.com/image.png" } },
        { "type": "audio", "source": { "type": "data", "value": "Zm9v", "mimeType": "audio/wav" } },
        { "type": "video", "source": { "type": "url", "value": "https://example.com/video.mp4" } },
        { "type": "document", "source": { "type": "data", "value": "YmFy", "mimeType": "application/pdf" } },
        { "type": "binary", "mimeType": "application/octet-stream", "id": "blob-1" }
    ]));

    match content {
        UserMessageContent::Parts(parts) => {
            let serialized = serde_json::to_value(parts).expect("serialize parts");
            assert_eq!(
                serialized,
                json!([
                    { "type": "text", "text": "Process all inputs" },
                    { "type": "image", "source": { "type": "url", "value": "https://example.com/image.png" } },
                    { "type": "audio", "source": { "type": "data", "value": "Zm9v", "mimeType": "audio/wav" } },
                    { "type": "video", "source": { "type": "url", "value": "https://example.com/video.mp4" } },
                    { "type": "document", "source": { "type": "data", "value": "YmFy", "mimeType": "application/pdf" } },
                    { "type": "binary", "mimeType": "application/octet-stream", "id": "blob-1" }
                ])
            );
        }
        other => panic!("expected parts, got {other:?}"),
    }
}
