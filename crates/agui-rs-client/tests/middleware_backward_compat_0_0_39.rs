#![allow(dead_code)]
#![allow(unused_imports)]

#[path = "../src/middleware.rs"]
mod middleware;

use agui_rs_core::types::UserMessage;
use agui_rs_core::{InputContent, InputContentSource, Message, RunAgentInput, UserMessageContent};
use futures::stream;
use futures::StreamExt;
use middleware::Middleware as _;
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

#[tokio::test]
async fn strips_parent_run_id_and_flattens_user_content_for_legacy_compatibility() {
    let capture = CaptureTerminal::default();
    let middleware = middleware::backward_compat::BackwardCompat0_0_39;
    let input = middleware::MiddlewareInput::from(RunAgentInput {
        parent_run_id: Some("legacy-parent".into()),
        messages: vec![
            Message::User(UserMessage {
                id: "msg-1".into(),
                content: UserMessageContent::Parts(vec![
                    InputContent::Text {
                        text: "Hello ".into(),
                    },
                    InputContent::Text {
                        text: "world!".into(),
                    },
                    InputContent::Document {
                        source: InputContentSource::Url {
                            value: "https://example.com/file.txt".into(),
                            mime_type: Some("text/plain".into()),
                        },
                        metadata: None,
                    },
                ]),
                name: None,
                encrypted_value: None,
            }),
            Message::User(UserMessage {
                id: "msg-2".into(),
                content: UserMessageContent::Parts(vec![InputContent::Document {
                    source: InputContentSource::Url {
                        value: "https://example.com/ignored.pdf".into(),
                        mime_type: Some("application/pdf".into()),
                    },
                    metadata: None,
                }]),
                name: None,
                encrypted_value: None,
            }),
        ],
        ..RunAgentInput::new("thread-1", "run-1")
    });

    let mut stream = middleware
        .run(input, capture.terminal())
        .await
        .expect("middleware should succeed");
    while stream.next().await.is_some() {}

    let captured = capture
        .input
        .lock()
        .expect("input lock")
        .clone()
        .expect("captured input")
        .run_agent_input;

    assert_eq!(captured.parent_run_id, None);
    match &captured.messages[0] {
        Message::User(UserMessage {
            content: UserMessageContent::Text(text),
            ..
        }) => assert_eq!(text, "Hello world!"),
        other => panic!("expected flattened user message, got {other:?}"),
    }
    match &captured.messages[1] {
        Message::User(UserMessage {
            content: UserMessageContent::Text(text),
            ..
        }) => assert!(text.is_empty()),
        other => panic!("expected flattened user message, got {other:?}"),
    }
}

// SKIPPED: automatically strips parentRunId when maxVersion <= 0.0.39: Rust tests target the middleware submodule directly; there is no auto-insertion layer to exercise here.
