#![allow(dead_code)]
#![allow(unused_imports)]

#[path = "../src/middleware.rs"]
mod middleware;

use ag_ui_core::{BaseEventFields, Event, RunAgentInput, RunFinishedEvent, TextMessageChunkEvent};
use async_stream::try_stream;
use async_trait::async_trait;
use futures::{stream, StreamExt};
use middleware::Middleware as _;
use serde_json::json;
use std::sync::Arc;

struct CustomMiddleware;

#[async_trait]
impl middleware::Middleware for CustomMiddleware {
    async fn run(
        &self,
        input: middleware::MiddlewareInput,
        next: middleware::NextFn,
    ) -> std::result::Result<middleware::EventStream, ag_ui_core::AgUiError> {
        let mut upstream = next(input).await?;
        let output = try_stream! {
            while let Some(item) = upstream.next().await {
                let event = item?;
                match event {
                    Event::RunStarted(mut started) => {
                        started.base.raw_event = Some(json!({"custom": true}));
                        yield Event::RunStarted(started);
                    }
                    other => yield other,
                }
            }
        };
        Ok(Box::pin(output))
    }
}

#[tokio::test]
async fn middleware_can_modify_live_events_before_agent_pipeline() {
    let terminal: middleware::TerminalFn = Arc::new(|input| {
        Box::pin(async move {
            Ok(Box::pin(stream::iter(vec![
                Ok(ag_ui_core::factory::run_started(
                    &input.run_agent_input.thread_id,
                    &input.run_agent_input.run_id,
                )),
                Ok(Event::TextMessageChunk(TextMessageChunkEvent {
                    message_id: Some("message-1".into()),
                    role: Some(ag_ui_core::TextMessageRole::Assistant),
                    delta: Some("Hello".into()),
                    name: None,
                    base: BaseEventFields::default(),
                })),
                Ok(Event::RunFinished(RunFinishedEvent {
                    thread_id: input.run_agent_input.thread_id,
                    run_id: input.run_agent_input.run_id,
                    result: Some(json!({"success": true})),
                    outcome: None,
                    base: BaseEventFields::default(),
                })),
            ])) as middleware::EventStream)
        })
    });

    let stream = CustomMiddleware
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

    assert_eq!(events.len(), 3);
    match &events[0] {
        Event::RunStarted(started) => {
            assert_eq!(started.base.raw_event, Some(json!({"custom": true})));
        }
        other => panic!("expected run started event, got {other:?}"),
    }
    assert!(matches!(events[1], Event::TextMessageChunk(_)));
    assert!(matches!(events[2], Event::RunFinished(_)));
}
