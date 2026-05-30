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
    BaseEventFields, Event, Message, RunAgentInput, RunFinishedEvent, TextMessageChunkEvent,
    TextMessageRole,
};
use async_trait::async_trait;
use futures::{stream, StreamExt};
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

struct CapturingMiddleware {
    captured: Arc<Mutex<Vec<Message>>>,
}

#[async_trait]
impl middleware::Middleware for CapturingMiddleware {
    async fn run(
        &self,
        input: middleware::MiddlewareInput,
        next: middleware::NextFn,
    ) -> std::result::Result<middleware::EventStream, agui_rs_core::AgUiError> {
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
        .collect::<agui_rs_core::Result<Vec<_>>>()?;

        if let Some(last) = applied.last() {
            *self.captured.lock().expect("captured lock") = last.messages.clone();
        }

        let events = applied
            .into_iter()
            .map(|applied| Ok(applied.event))
            .collect::<Vec<_>>();
        Ok(Box::pin(stream::iter(events)))
    }
}

#[tokio::test]
async fn outer_and_inner_state_tracking_middlewares_both_capture_messages() {
    let seen_inputs = Arc::new(Mutex::new(Vec::new()));
    let inner_captured = Arc::new(Mutex::new(Vec::new()));
    let outer_captured = Arc::new(Mutex::new(Vec::new()));

    let agent = FakeAgent {
        events: vec![
            agui_rs_core::factory::run_started("test-thread", "test-run"),
            Event::TextMessageChunk(TextMessageChunkEvent {
                message_id: Some("message-1".into()),
                role: Some(TextMessageRole::Assistant),
                delta: Some("Hello".into()),
                name: None,
                base: BaseEventFields::default(),
            }),
            Event::RunFinished(RunFinishedEvent {
                thread_id: "test-thread".into(),
                run_id: "test-run".into(),
                result: None,
                outcome: None,
                base: BaseEventFields::default(),
            }),
        ],
        seen_inputs: Arc::clone(&seen_inputs),
    };

    let mut chain = middleware::MiddlewareChain::new();
    chain.push(CapturingMiddleware {
        captured: Arc::clone(&outer_captured),
    });
    chain.push(CapturingMiddleware {
        captured: Arc::clone(&inner_captured),
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

    for captured in [&inner_captured, &outer_captured] {
        let captured = captured.lock().expect("captured lock");
        assert_eq!(captured.len(), 1);
        match &captured[0] {
            Message::Assistant(message) => {
                assert_eq!(message.content.as_deref(), Some("Hello"));
            }
            other => panic!("expected assistant message, got {other:?}"),
        }
    }

    assert_eq!(result.new_messages.len(), 1);
    let seen_inputs = seen_inputs.lock().expect("seen inputs lock");
    assert_eq!(seen_inputs.len(), 1);
    assert_eq!(seen_inputs[0].run_id, "test-run");
}
