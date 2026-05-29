#![allow(dead_code)]
#![allow(unused_imports)]

#[path = "../src/middleware.rs"]
mod middleware;

use agui_rs_core::types::UserMessage;
use agui_rs_core::{
    BinaryInputContent, InputContent, InputContentSource, Message, RunAgentInput,
    UserMessageContent,
};
use futures::stream;
use middleware::Middleware as _;
use serde_json::Value;
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct CaptureTerminal {
    input: Arc<Mutex<Option<middleware::MiddlewareInput>>>,
}

impl CaptureTerminal {
    fn terminal(&self) -> middleware::TerminalFn {
        let input = Arc::clone(&self.input);
        Arc::new(move |middleware_input| {
            let input = Arc::clone(&input);
            Box::pin(async move {
                *input.lock().expect("input lock") = Some(middleware_input);
                Ok(Box::pin(stream::empty()) as middleware::EventStream)
            })
        })
    }
}

async fn run_and_capture(message: Message) -> Message {
    let capture = CaptureTerminal::default();
    let middleware = middleware::backward_compat::BackwardCompat0_0_47;
    let input = middleware::MiddlewareInput::from(RunAgentInput {
        messages: vec![message],
        ..RunAgentInput::new("thread-1", "run-1")
    });

    let _stream = middleware
        .run(input, capture.terminal())
        .await
        .expect("middleware should succeed");

    let captured = capture
        .input
        .lock()
        .expect("input lock")
        .clone()
        .expect("captured input")
        .run_agent_input
        .messages;

    captured.into_iter().next().expect("captured message")
}

#[tokio::test]
async fn passes_through_plain_string_content_unchanged() {
    let message = Message::User(UserMessage {
        id: "msg-1".into(),
        content: UserMessageContent::Text("hello world".into()),
        name: None,
        encrypted_value: None,
    });

    let captured = run_and_capture(message.clone()).await;
    assert_eq!(captured, message);
}

#[tokio::test]
async fn passes_through_text_parts_unchanged() {
    let message = Message::User(UserMessage {
        id: "msg-1".into(),
        content: UserMessageContent::Parts(vec![InputContent::Text {
            text: "hello".into(),
        }]),
        name: None,
        encrypted_value: None,
    });

    let captured = run_and_capture(message.clone()).await;
    assert_eq!(captured, message);
}

#[tokio::test]
async fn converts_binary_parts_into_new_content_types() {
    let image = run_and_capture(Message::User(UserMessage {
        id: "img".into(),
        content: UserMessageContent::Parts(vec![InputContent::Binary {
            content: BinaryInputContent {
                mime_type: "image/png".into(),
                id: None,
                url: None,
                data: Some("iVBORw0KGgo=".into()),
                filename: None,
            },
        }]),
        name: None,
        encrypted_value: None,
    }))
    .await;
    let audio = run_and_capture(Message::User(UserMessage {
        id: "audio".into(),
        content: UserMessageContent::Parts(vec![InputContent::Binary {
            content: BinaryInputContent {
                mime_type: "audio/mp3".into(),
                id: None,
                url: Some("https://example.com/audio.mp3".into()),
                data: None,
                filename: None,
            },
        }]),
        name: None,
        encrypted_value: None,
    }))
    .await;
    let video = run_and_capture(Message::User(UserMessage {
        id: "video".into(),
        content: UserMessageContent::Parts(vec![InputContent::Binary {
            content: BinaryInputContent {
                mime_type: "video/mp4".into(),
                id: None,
                url: None,
                data: Some("AAAA".into()),
                filename: None,
            },
        }]),
        name: None,
        encrypted_value: None,
    }))
    .await;
    let document = run_and_capture(Message::User(UserMessage {
        id: "doc".into(),
        content: UserMessageContent::Parts(vec![InputContent::Binary {
            content: BinaryInputContent {
                mime_type: "application/pdf".into(),
                id: None,
                url: Some("https://example.com/doc.pdf".into()),
                data: None,
                filename: None,
            },
        }]),
        name: None,
        encrypted_value: None,
    }))
    .await;

    assert!(
        matches!(image, Message::User(UserMessage { content: UserMessageContent::Parts(parts), .. }) if matches!(&parts[0], InputContent::Image { source: InputContentSource::Data { value, mime_type }, metadata } if value == "iVBORw0KGgo=" && mime_type == "image/png" && metadata.is_none()))
    );
    assert!(
        matches!(audio, Message::User(UserMessage { content: UserMessageContent::Parts(parts), .. }) if matches!(&parts[0], InputContent::Audio { source: InputContentSource::Url { value, mime_type }, metadata } if value == "https://example.com/audio.mp3" && mime_type.as_deref() == Some("audio/mp3") && metadata.is_none()))
    );
    assert!(
        matches!(video, Message::User(UserMessage { content: UserMessageContent::Parts(parts), .. }) if matches!(&parts[0], InputContent::Video { source: InputContentSource::Data { value, mime_type }, metadata } if value == "AAAA" && mime_type == "video/mp4" && metadata.is_none()))
    );
    assert!(
        matches!(document, Message::User(UserMessage { content: UserMessageContent::Parts(parts), .. }) if matches!(&parts[0], InputContent::Document { source: InputContentSource::Url { value, mime_type }, metadata } if value == "https://example.com/doc.pdf" && mime_type.as_deref() == Some("application/pdf") && metadata.is_none()))
    );
}

#[tokio::test]
async fn preserves_filename_and_prefers_data_over_url() {
    let captured = run_and_capture(Message::User(UserMessage {
        id: "msg-1".into(),
        content: UserMessageContent::Parts(vec![InputContent::Binary {
            content: BinaryInputContent {
                mime_type: "image/jpeg".into(),
                id: None,
                url: Some("https://example.com/img.jpg".into()),
                data: Some("base64data".into()),
                filename: Some("photo.jpg".into()),
            },
        }]),
        name: None,
        encrypted_value: None,
    }))
    .await;

    assert!(
        matches!(captured, Message::User(UserMessage { content: UserMessageContent::Parts(parts), .. }) if matches!(&parts[0], InputContent::Image { source: InputContentSource::Data { value, mime_type }, metadata: Some(Value::Object(meta)) } if value == "base64data" && mime_type == "image/jpeg" && meta.get("filename") == Some(&Value::String("photo.jpg".into()))))
    );
}

#[tokio::test]
async fn leaves_id_only_binary_and_mixed_parts_as_expected() {
    let id_only = run_and_capture(Message::User(UserMessage {
        id: "msg-id".into(),
        content: UserMessageContent::Parts(vec![InputContent::Binary {
            content: BinaryInputContent {
                mime_type: "image/png".into(),
                id: Some("file-123".into()),
                url: None,
                data: None,
                filename: None,
            },
        }]),
        name: None,
        encrypted_value: None,
    }))
    .await;
    let mixed = run_and_capture(Message::User(UserMessage {
        id: "mixed".into(),
        content: UserMessageContent::Parts(vec![
            InputContent::Text {
                text: "Look at this:".into(),
            },
            InputContent::Binary {
                content: BinaryInputContent {
                    mime_type: "image/png".into(),
                    id: None,
                    url: None,
                    data: Some("imgdata".into()),
                    filename: None,
                },
            },
            InputContent::Image {
                source: InputContentSource::Url {
                    value: "https://example.com/img.png".into(),
                    mime_type: None,
                },
                metadata: None,
            },
        ]),
        name: None,
        encrypted_value: None,
    }))
    .await;

    assert!(
        matches!(id_only, Message::User(UserMessage { content: UserMessageContent::Parts(parts), .. }) if matches!(&parts[0], InputContent::Binary { content } if content.id.as_deref() == Some("file-123")))
    );
    assert!(
        matches!(mixed, Message::User(UserMessage { content: UserMessageContent::Parts(parts), .. }) if parts.len() == 3 && matches!(&parts[0], InputContent::Text { text } if text == "Look at this:") && matches!(&parts[1], InputContent::Image { source: InputContentSource::Data { value, mime_type }, .. } if value == "imgdata" && mime_type == "image/png") && matches!(&parts[2], InputContent::Image { source: InputContentSource::Url { value, .. }, .. } if value == "https://example.com/img.png"))
    );
}

// SKIPPED: automatically transforms when maxVersion <= 0.0.47: Rust tests target the middleware submodule directly; there is no auto-insertion layer to exercise here.
