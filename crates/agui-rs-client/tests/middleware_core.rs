#![allow(dead_code)]
#![allow(unused_imports)]

#[path = "../src/agent.rs"]
mod agent;
#[path = "../src/apply.rs"]
mod apply;
#[path = "../src/chunks.rs"]
mod chunks;
#[path = "../src/debug_logger.rs"]
mod debug_logger;
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
use serde_json::json;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
struct FakeAgent {
    events: Vec<Event>,
    seen_inputs: Arc<Mutex<Vec<RunAgentInput>>>,
}

#[async_trait]
impl agent::Agent for FakeAgent {
    async fn run(&self, input: RunAgentInput) -> agui_rs_core::Result<agent::EventStream> {
        self.seen_inputs
            .lock()
            .expect("seen inputs lock")
            .push(input);
        Ok(Box::pin(stream::iter(
            self.events.clone().into_iter().map(Ok),
        )))
    }
}

struct TransformingMiddleware {
    emitted: Arc<Mutex<Vec<Event>>>,
}

#[async_trait]
impl middleware::Middleware for TransformingMiddleware {
    async fn run(
        &self,
        input: middleware::MiddlewareInput,
        next: middleware::NextFn,
    ) -> std::result::Result<middleware::EventStream, agui_rs_core::AgUiError> {
        let mut upstream = next(input).await?;
        let emitted = Arc::clone(&self.emitted);

        let output = try_stream! {
            while let Some(item) = upstream.next().await {
                let event = item?;
                let transformed = match event {
                    Event::RunStarted(mut started) => {
                        started.base.raw_event = Some(json!({"middleware": true}));
                        Event::RunStarted(started)
                    }
                    other => other,
                };

                emitted
                    .lock()
                    .expect("emitted lock")
                    .push(transformed.clone());
                yield transformed;
            }
        };

        Ok(Box::pin(output))
    }
}

#[tokio::test]
async fn middleware_can_modify_the_event_stream() {
    let emitted = Arc::new(Mutex::new(Vec::new()));
    let seen_inputs = Arc::new(Mutex::new(Vec::new()));

    let agent = FakeAgent {
        events: vec![
            factory::run_started("test-thread", "test-run"),
            Event::RunFinished(RunFinishedEvent {
                thread_id: "test-thread".into(),
                run_id: "test-run".into(),
                result: Some(json!({"success": true})),
                outcome: Some(RunFinishedOutcome::Success),
                base: BaseEventFields::default(),
            }),
        ],
        seen_inputs: Arc::clone(&seen_inputs),
    };

    let mut chain = middleware::MiddlewareChain::new();
    chain.push(TransformingMiddleware {
        emitted: Arc::clone(&emitted),
    });

    let mut runner =
        agent::AgentRunner::new(agent, agent::AgentConfig::default()).with_middleware(chain);
    let result = runner
        .run_agent(agent::RunAgentParameters {
            run_id: Some("test-run".into()),
            ..Default::default()
        })
        .await
        .expect("run should succeed");

    assert!(result.new_messages.is_empty());
    assert_eq!(result.outcome, Some(RunFinishedOutcome::Success));

    let seen_inputs = seen_inputs.lock().expect("seen inputs lock");
    assert_eq!(seen_inputs.len(), 1);
    assert!(seen_inputs[0].thread_id.starts_with("thread-"));
    assert_eq!(seen_inputs[0].run_id, "test-run");

    let emitted = emitted.lock().expect("emitted lock");
    assert_eq!(emitted.len(), 2);
    match &emitted[0] {
        Event::RunStarted(started) => {
            assert_eq!(started.thread_id, result.thread_id);
            assert_eq!(started.run_id, "test-run");
            assert_eq!(started.base.raw_event, Some(json!({"middleware": true})));
        }
        other => panic!("expected run started event, got {other:?}"),
    }
    match &emitted[1] {
        Event::RunFinished(finished) => {
            assert_eq!(finished.result, Some(json!({"success": true})));
        }
        other => panic!("expected run finished event, got {other:?}"),
    }
}
