use agui_rs_core::types::{BinaryInputContent, Message, UserMessage};
use agui_rs_core::{ContentPart, PartSource, RunAgentInput, ToolResultContent, UserMessageContent};
use serde_json::{json, Value};

fn parse_user_message_content(value: Value) -> UserMessageContent {
    serde_json::from_value(value).expect("deserialize user message content")
}

fn parse_content_part(value: Value) -> ContentPart {
    serde_json::from_value(value).expect("deserialize content part")
}

fn parse_source(value: Value) -> PartSource {
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
            assert!(
                matches!(&parts[0], ContentPart::Text { text, .. } if text == "Check this out")
            );
            assert!(
                matches!(&parts[1], ContentPart::Image { source: PartSource::Url { value, .. }, .. } if value == "https://example.com/image.png")
            );
        }
        other => panic!("expected parts, got {other:?}"),
    }
}

#[test]
fn content_part_round_trips_optional_id_and_metadata() {
    let part = parse_content_part(json!({
        "type": "text",
        "id": "part-1",
        "text": "a search hit",
        "metadata": { "title": "AG-UI", "source": "docs" }
    }));

    match &part {
        ContentPart::Text { id, text, metadata } => {
            assert_eq!(id.as_deref(), Some("part-1"));
            assert_eq!(text, "a search hit");
            assert_eq!(
                metadata.as_ref(),
                Some(&json!({ "title": "AG-UI", "source": "docs" }))
            );
        }
        other => panic!("expected text part, got {other:?}"),
    }
    let value = serde_json::to_value(&part).expect("serialize");
    assert_eq!(
        value,
        json!({
            "type": "text",
            "id": "part-1",
            "text": "a search hit",
            "metadata": { "title": "AG-UI", "source": "docs" }
        })
    );

    // An absent id and metadata stay absent rather than becoming nulls.
    let bare = parse_content_part(json!({ "type": "text", "text": "hi" }));
    let value = serde_json::to_value(&bare).expect("serialize");

    assert_eq!(value, json!({ "type": "text", "text": "hi" }));
}

#[test]
fn image_part_parses_inline_data_source() {
    let part = parse_content_part(json!({
        "type": "image",
        "source": {
            "type": "data",
            "value": "base64-value",
            "mimeType": "image/png"
        },
        "metadata": { "detail": "high" }
    }));

    match part {
        ContentPart::Image {
            source, metadata, ..
        } => {
            assert!(
                matches!(source, PartSource::Data { mime_type, .. } if mime_type == "image/png")
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
        matches!(source, PartSource::Url { value, mime_type: None } if value == "https://example.com/file.pdf")
    );
}

#[test]
fn data_source_parses_with_mime_type() {
    let source = parse_source(json!({
        "type": "data",
        "value": "Zm9v",
        "mimeType": "application/pdf"
    }));

    assert!(matches!(source, PartSource::Data { mime_type, .. } if mime_type == "application/pdf"));
}

#[test]
fn file_source_carries_an_opaque_provider_handle() {
    let source = parse_source(json!({
        "type": "file",
        "value": "files/abc123",
        "provider": "openai",
        "mimeType": "image/png"
    }));

    match source {
        PartSource::File {
            value,
            provider,
            mime_type,
        } => {
            assert_eq!(value, "files/abc123");
            assert_eq!(provider.as_deref(), Some("openai"));
            assert_eq!(mime_type.as_deref(), Some("image/png"));
        }
        other => panic!("expected file source, got {other:?}"),
    }
}

#[test]
fn file_source_accepts_bare_handle() {
    let source = parse_source(json!({ "type": "file", "value": "file_9" }));
    // provider and mimeType are optional, so an absent one is omitted.
    let value = serde_json::to_value(source).expect("serialize");
    assert_eq!(value, json!({ "type": "file", "value": "file_9" }));
}

#[test]
fn data_source_requires_a_mime_type() {
    let error = serde_json::from_value::<PartSource>(json!({ "type": "data", "value": "Zm9v" }))
        .expect_err("data source without mime type should fail");
    assert!(error.to_string().contains("mimeType"));
}

#[test]
fn binary_part_is_not_a_content_part() {
    // Retired in 1.0: the compatibility boundary converts what arrives into the
    // media parts, so no message shape carries `{ type: "binary" }` any more.
    let error = serde_json::from_value::<ContentPart>(json!({
        "type": "binary",
        "mimeType": "image/png",
        "data": "base64"
    }))
    .expect_err("binary is not a 1.0 content part");
    assert!(error.to_string().contains("binary") || error.to_string().contains("unknown variant"));
}

#[test]
fn legacy_binary_attachment_still_has_a_type_to_name() {
    let binary: BinaryInputContent = serde_json::from_value(json!({
        "mimeType": "image/png",
        "id": "blob-1"
    }))
    .expect("deserialize legacy binary attachment");
    assert!(binary.validate().is_ok());
}

fn modality_shape_round_trip(modality: &str, mime_type: &str) {
    let url_with_metadata = parse_content_part(json!({
        "type": modality,
        "id": "part-1",
        "source": {
            "type": "url",
            "value": format!("https://example.com/{modality}"),
            "mimeType": mime_type
        },
        "metadata": { "providerHint": "high" }
    }));

    let data_without_metadata = parse_content_part(json!({
        "type": modality,
        "source": {
            "type": "data",
            "value": "Zm9v",
            "mimeType": mime_type
        }
    }));

    let url_without_mime = parse_content_part(json!({
        "type": modality,
        "source": {
            "type": "url",
            "value": format!("https://example.com/{modality}/raw")
        }
    }));

    let file_source = parse_content_part(json!({
        "type": modality,
        "source": { "type": "file", "value": "handle-1" }
    }));

    let missing_source = serde_json::from_value::<ContentPart>(json!({ "type": modality }))
        .expect_err("missing source should fail");

    let data_missing_mime = serde_json::from_value::<ContentPart>(json!({
        "type": modality,
        "source": { "type": "data", "value": "Zm9v" }
    }))
    .expect_err("data source without mime type should fail");

    assert!(data_missing_mime.to_string().contains("mimeType"));
    assert!(missing_source.to_string().contains("source"));

    match url_with_metadata {
        ContentPart::Image {
            id,
            source,
            metadata,
        }
        | ContentPart::Audio {
            id,
            source,
            metadata,
        }
        | ContentPart::Video {
            id,
            source,
            metadata,
        }
        | ContentPart::Document {
            id,
            source,
            metadata,
        } => {
            assert_eq!(id.as_deref(), Some("part-1"));
            assert!(matches!(source, PartSource::Url { .. }));
            assert_eq!(metadata, Some(json!({ "providerHint": "high" })));
        }
        other => panic!("expected multimodal content, got {other:?}"),
    }

    match data_without_metadata {
        ContentPart::Image {
            id,
            source,
            metadata,
        }
        | ContentPart::Audio {
            id,
            source,
            metadata,
        }
        | ContentPart::Video {
            id,
            source,
            metadata,
        }
        | ContentPart::Document {
            id,
            source,
            metadata,
        } => {
            assert!(id.is_none());
            assert!(matches!(
                source,
                PartSource::Data { mime_type: found, .. } if found == mime_type
            ));
            assert_eq!(metadata, None);
        }
        other => panic!("expected multimodal content, got {other:?}"),
    }

    match url_without_mime {
        ContentPart::Image { source, .. }
        | ContentPart::Audio { source, .. }
        | ContentPart::Video { source, .. }
        | ContentPart::Document { source, .. } => {
            assert!(matches!(
                source,
                PartSource::Url {
                    mime_type: None,
                    ..
                }
            ));
        }
        other => panic!("expected multimodal content, got {other:?}"),
    }

    match file_source {
        ContentPart::Image { source, .. }
        | ContentPart::Audio { source, .. }
        | ContentPart::Video { source, .. }
        | ContentPart::Document { source, .. } => {
            assert!(matches!(source, PartSource::File { value, .. } if value == "handle-1"));
        }
        other => panic!("expected multimodal content, got {other:?}"),
    }
}

#[test]
fn image_audio_video_and_document_support_url_data_and_file_sources() {
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
        { "type": "image", "source": { "type": "file", "value": "file_1" } }
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
                    { "type": "image", "source": { "type": "file", "value": "file_1" } }
                ])
            );
        }
        other => panic!("expected parts, got {other:?}"),
    }
}

#[test]
fn tool_message_content_is_text_or_parts() {
    let text: Message = serde_json::from_value(json!({
        "id": "t1", "role": "tool", "content": "42", "toolCallId": "tc-1"
    }))
    .expect("deserialize text tool message");
    match &text {
        Message::Tool(tool) => {
            assert!(matches!(&tool.content, ToolResultContent::Text(v) if v == "42"))
        }
        other => panic!("expected tool message, got {other:?}"),
    }

    let parts: Message = serde_json::from_value(json!({
        "id": "t2",
        "role": "tool",
        "content": [
            { "type": "text", "text": "here" },
            { "type": "image", "source": { "type": "data", "value": "Zm9v", "mimeType": "image/png" } }
        ],
        "toolCallId": "tc-2",
        "error": "partial",
        "metadata": { "source": "tool" }
    }))
    .expect("deserialize multimodal tool message");
    match &parts {
        Message::Tool(tool) => {
            assert!(matches!(&tool.content, ToolResultContent::Parts(p) if p.len() == 2));
            assert_eq!(tool.error.as_deref(), Some("partial"));
            assert_eq!(
                tool.metadata,
                Some(serde_json::Map::from_iter([(
                    "source".to_string(),
                    json!("tool")
                )]))
            );
        }
        other => panic!("expected tool message, got {other:?}"),
    }
}

#[test]
fn run_agent_input_round_trips_every_modality_and_the_file_source() {
    let input = RunAgentInput {
        messages: vec![Message::User(UserMessage {
            id: "u1".into(),
            content: UserMessageContent::Parts(vec![ContentPart::Image {
                id: Some("part-1".into()),
                source: PartSource::File {
                    value: "file_1".into(),
                    provider: Some("anthropic".into()),
                    mime_type: None,
                },
                metadata: None,
            }]),
            name: None,
            encrypted_value: None,
            subagent_run_id: None,
            metadata: None,
        })],
        ..RunAgentInput::new("thread-1", "run-1")
    };

    input.validate().expect("input should validate");
    let value = serde_json::to_value(&input).expect("serialize");
    assert_eq!(
        value["messages"][0]["content"][0]["source"],
        json!({ "type": "file", "value": "file_1", "provider": "anthropic" })
    );
    let back: RunAgentInput = serde_json::from_value(value).expect("deserialize");
    assert_eq!(back.messages, input.messages);
}
