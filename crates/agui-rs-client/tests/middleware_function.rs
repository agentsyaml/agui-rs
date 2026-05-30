#![allow(dead_code)]
#![allow(unused_imports)]

#[path = "../src/agent.rs"]
mod agent;
#[path = "../src/apply.rs"]
mod apply;
#[path = "../src/chunks.rs"]
mod chunks;
#[path = "../src/interrupts.rs"]
mod interrupts;
#[path = "../src/middleware.rs"]
mod middleware;
#[path = "../src/subscriber.rs"]
mod subscriber;
#[path = "../src/verify.rs"]
mod verify;

use agui_rs_core::{
    factory, BaseEventFields, Event, RunAgentInput, RunFinishedEvent, RunFinishedOutcome,
};
use async_stream::try_stream;
use async_trait::async_trait;
use futures::{stream, StreamExt};
use middleware::Middleware as _;
use serde_json::json;
use std::sync::Arc;

#[derive(Clone)]
struct FakeAgent {
    events: Vec<Event>,
}

#[async_trait]
impl agent::Agent for FakeAgent {
    async fn run(&self, _input: RunAgentInput) -> agui_rs_core::Result<agent::EventStream> {
        Ok(Box::pin(stream::iter(
            self.events.clone().into_iter().map(Ok),
        )))
    }
}

type MiddlewareFn = dyn Fn(
        middleware::MiddlewareInput,
        middleware::NextFn,
    ) -> futures::future::BoxFuture<
        'static,
        std::result::Result<middleware::EventStream, agui_rs_core::AgUiError>,
    > + Send
    + Sync;

struct FunctionMiddleware {
    handler: Box<MiddlewareFn>,
}

#[async_trait]
impl middleware::Middleware for FunctionMiddleware {
    async fn run(
        &self,
        input: middleware::MiddlewareInput,
        next: middleware::NextFn,
    ) -> std::result::Result<middleware::EventStream, agui_rs_core::AgUiError> {
        (self.handler)(input, next).await
    }
}

#[tokio::test]
async fn function_based_middleware_can_intercept_events() {
    let middleware = FunctionMiddleware {
        handler: Box::new(|input, next| {
            Box::pin(async move {
                let mut upstream = next(input).await?;
                let output = try_stream! {
                    while let Some(item) = upstream.next().await {
                        let event = item?;
                        match event {
                            Event::RunStarted(mut started) => {
                                started.base.raw_event = Some(json!({"fromMiddleware": true}));
                                yield Event::RunStarted(started);
                            }
                            Event::RunFinished(mut finished) => {
                                finished.result = Some(json!({"success": true}));
                                yield Event::RunFinished(finished);
                            }
                            other => yield other,
                        }
                    }
                };
                Ok(Box::pin(output) as middleware::EventStream)
            })
        }),
    };

    let terminal: middleware::TerminalFn = Arc::new(|_input| {
        Box::pin(async move {
            Ok(Box::pin(stream::iter(vec![
                Ok(factory::run_started("test-thread", "test-run")),
                Ok(Event::RunFinished(RunFinishedEvent {
                    thread_id: "test-thread".into(),
                    run_id: "test-run".into(),
                    result: None,
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
        .collect::<agui_rs_core::Result<Vec<_>>>()
        .expect("stream should succeed");

    assert_eq!(events.len(), 2);
    match &events[0] {
        Event::RunStarted(started) => {
            assert_eq!(
                started.base.raw_event,
                Some(json!({"fromMiddleware": true}))
            );
        }
        other => panic!("expected run started event, got {other:?}"),
    }
    match &events[1] {
        Event::RunFinished(finished) => {
            assert_eq!(finished.result, Some(json!({"success": true})));
        }
        other => panic!("expected run finished event, got {other:?}"),
    }
}
