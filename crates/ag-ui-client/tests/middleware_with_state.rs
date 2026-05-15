#![allow(dead_code)]
#![allow(unused_imports)]

#[path = "../src/apply.rs"]
mod apply;
#[path = "../src/chunks.rs"]
mod chunks;
#[path = "../src/verify.rs"]
mod verify;
#[path = "../src/middleware.rs"]
mod middleware;

use ag_ui_core::{BaseEventFields, Event, RunFinishedEvent, RunAgentInput, RunFinishedOutcome, TextMessageChunkEvent, TextMessageRole};
use async_trait::async_trait;
use futures::{stream, StreamExt};
use middleware::Middleware as _;
use serde_json::Value;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, PartialEq)]
struct EventWithState {
    event: Event,
    messages: Vec<ag_ui_core::Message>,
    state: Value,
}

struct StateTrackingMiddleware {
    captured: Arc<Mutex<Vec<EventWithState>>>,
}

#[async_trait]
impl middleware::Middleware for StateTrackingMiddleware {
    async fn run(
        &self,
        input: middleware::MiddlewareInput,
        next: middleware::NextFn,
    ) -> std::result::Result<middleware::EventStream, ag_ui_core::AgUiError> {
        let initial_messages = input.run_agent_input.messages.clone();
        let initial_state = input.run_agent_input.state.clone();
        let applied = apply::default_apply_events(
            verify::verify_events(chunks::expand_chunks(next(input).await?)),
            initial_messages,
            initial_state,
        )
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .collect::<ag_ui_core::Result<Vec<_>>>()?;

        *self.captured.lock().expect("captured lock") = applied
            .iter()
            .map(|applied| EventWithState {
                event: applied.event.clone(),
                messages: applied.messages.clone(),
                state: applied.state.clone(),
            })
            .collect();

        let events = applied.into_iter().map(|applied| Ok(applied.event)).collect::<Vec<_>>();
        Ok(Box::pin(stream::iter(events)))
    }
}

#[tokio::test]
async fn captures_state_after_each_expanded_event() {
    let captured = Arc::new(Mutex::new(Vec::new()));
    let middleware = StateTrackingMiddleware {
        captured: Arc::clone(&captured),
    };

    let terminal: middleware::TerminalFn = Arc::new(|input| {
        Box::pin(async move {
            Ok(Box::pin(stream::iter(vec![
                Ok(ag_ui_core::factory::run_started(&input.run_agent_input.thread_id, &input.run_agent_input.run_id)),
                Ok(Event::TextMessageChunk(TextMessageChunkEvent {
                    message_id: Some("message-1".into()),
                    role: Some(TextMessageRole::Assistant),
                    delta: Some("Hello".into()),
                    name: None,
                    base: BaseEventFields::default(),
                })),
                Ok(Event::RunFinished(RunFinishedEvent {
                    thread_id: input.run_agent_input.thread_id,
                    run_id: input.run_agent_input.run_id,
                    result: Some(serde_json::json!({"success": true})),
                    outcome: Some(RunFinishedOutcome::Success),
                    base: BaseEventFields::default(),
                })),
            ])) as middleware::EventStream)
        })
    });

    let stream = middleware
        .run(
            middleware::MiddlewareInput::from(RunAgentInput::new("test-thread", "test-run")),
            terminal,
        )
        .await
        .expect("middleware should succeed");

    let events = stream
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .collect::<ag_ui_core::Result<Vec<_>>>()
        .expect("stream should succeed");
    assert_eq!(events.len(), 5);
    assert!(matches!(events[0], Event::RunStarted(_)));
    assert!(matches!(events[1], Event::TextMessageStart(_)));
    assert!(matches!(events[2], Event::TextMessageContent(_)));
    assert!(matches!(events[3], Event::TextMessageEnd(_)));
    assert!(matches!(events[4], Event::RunFinished(_)));

    let captured = captured.lock().expect("captured lock");
    assert_eq!(captured.len(), 5);
    assert_eq!(captured[0].messages.len(), 0);
    match &captured[1].messages[0] {
        ag_ui_core::Message::Assistant(message) => {
            assert_eq!(message.content.as_deref(), Some(""));
        }
        other => panic!("expected assistant message, got {other:?}"),
    }
    match &captured[2].messages[0] {
        ag_ui_core::Message::Assistant(message) => {
            assert_eq!(message.content.as_deref(), Some("Hello"));
        }
        other => panic!("expected assistant message, got {other:?}"),
    }
}
