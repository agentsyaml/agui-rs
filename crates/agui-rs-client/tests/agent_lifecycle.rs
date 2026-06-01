//! Ported from TypeScript `agent/__tests__/agent-clone.test.ts` plus the
//! `connect()` / `getCapabilities()` surface (`AbstractAgent` lifecycle).

use agui_rs_client::{Agent, AgentConfig, AgentRunner, RunAgentParameters};
use agui_rs_core::types::AssistantMessage;
use agui_rs_core::{
    factory, AgentCapabilities, Event, HumanInTheLoopCapabilities, Message, Result, RunAgentInput,
};
use async_trait::async_trait;
use futures::{stream, stream::BoxStream};
use serde_json::json;

/// Agent that supports neither connect nor capabilities (defaults).
struct PlainAgent;

#[async_trait]
impl Agent for PlainAgent {
    async fn run(&self, _input: RunAgentInput) -> Result<BoxStream<'static, Result<Event>>> {
        Ok(Box::pin(stream::iter(vec![
            Ok(factory::run_started("thread-1", "run-1")),
            Ok(factory::run_finished("thread-1", "run-1")),
        ])))
    }
}

/// Agent that implements connect() and capabilities().
struct ConnectingAgent;

#[async_trait]
impl Agent for ConnectingAgent {
    async fn run(&self, _input: RunAgentInput) -> Result<BoxStream<'static, Result<Event>>> {
        Ok(Box::pin(stream::iter(Vec::new())))
    }

    async fn connect(&self, _input: RunAgentInput) -> Result<BoxStream<'static, Result<Event>>> {
        Ok(Box::pin(stream::iter(vec![
            Ok(factory::run_started("thread-1", "run-1")),
            Ok(factory::run_finished("thread-1", "run-1")),
        ])))
    }

    async fn capabilities(&self) -> Option<AgentCapabilities> {
        Some(AgentCapabilities {
            human_in_the_loop: Some(HumanInTheLoopCapabilities {
                supported: Some(true),
                interrupts: Some(true),
                ..Default::default()
            }),
            ..Default::default()
        })
    }
}

fn config_with_state() -> AgentConfig {
    AgentConfig {
        agent_id: Some("test-agent".into()),
        description: Some("cloneable".into()),
        thread_id: Some("thread-test".into()),
        initial_messages: vec![Message::Assistant(AssistantMessage {
            id: "msg-1".into(),
            content: Some("Hello world".into()),
            name: None,
            tool_calls: None,
            encrypted_value: None,
        })],
        initial_state: json!({"stage": "initial"}),
        debug: false,
    }
}

#[tokio::test]
async fn clone_runner_copies_messages_state_and_thread_id() {
    let runner = AgentRunner::new(PlainAgent, config_with_state());
    let cloned = runner.clone_runner();

    assert_eq!(cloned.thread_id(), runner.thread_id());
    assert_eq!(cloned.messages(), runner.messages());
    assert_eq!(cloned.state(), runner.state());
}

#[tokio::test]
async fn cloned_runner_has_independent_message_state() {
    let mut runner = AgentRunner::new(PlainAgent, config_with_state());
    let mut cloned = runner.clone_runner();

    // Mutating the clone must not affect the original.
    cloned.set_state(json!({"stage": "changed"})).await;

    assert_eq!(runner.state(), &json!({"stage": "initial"}));
    assert_eq!(cloned.state(), &json!({"stage": "changed"}));

    // And vice-versa.
    runner
        .add_message(Message::Assistant(AssistantMessage {
            id: "msg-2".into(),
            content: Some("only in original".into()),
            name: None,
            tool_calls: None,
            encrypted_value: None,
        }))
        .await;
    assert_eq!(runner.messages().len(), 2);
    assert_eq!(cloned.messages().len(), 1);
}

#[tokio::test]
async fn connect_agent_uses_connect_stream() {
    let mut runner = AgentRunner::new(ConnectingAgent, AgentConfig::default());
    let result = runner
        .connect_agent(RunAgentParameters::default())
        .await
        .expect("connect should succeed");
    assert_eq!(
        result.outcome,
        Some(agui_rs_core::RunFinishedOutcome::Success)
    );
}

#[tokio::test]
async fn connect_agent_on_plain_agent_completes_without_error() {
    // PlainAgent does not override connect(), so connect() returns
    // ConnectNotImplemented, which connect_agent swallows into an empty result.
    let mut runner = AgentRunner::new(PlainAgent, AgentConfig::default());
    let result = runner
        .connect_agent(RunAgentParameters::default())
        .await
        .expect("connect_agent swallows ConnectNotImplemented");
    assert!(result.new_messages.is_empty());
    assert_eq!(result.outcome, None);
}

#[tokio::test]
async fn capabilities_returns_declared_surface() {
    let runner = AgentRunner::new(ConnectingAgent, AgentConfig::default());
    let caps = runner.capabilities().await.expect("capabilities declared");
    let hitl = caps.human_in_the_loop.expect("hitl present");
    assert_eq!(hitl.supported, Some(true));
    assert_eq!(hitl.interrupts, Some(true));
}

#[tokio::test]
async fn capabilities_default_is_none() {
    let runner = AgentRunner::new(PlainAgent, AgentConfig::default());
    assert!(runner.capabilities().await.is_none());
}

#[tokio::test]
async fn debug_config_runs_with_lifecycle_logging_enabled() {
    // With debug: true the runner attaches a forced DebugLogger and logs
    // lifecycle events. We assert the run completes cleanly (the logging path is
    // exercised); stderr capture is not asserted (no console.debug equivalent).
    let config = AgentConfig {
        debug: true,
        ..AgentConfig::default()
    };
    let mut runner = AgentRunner::new(PlainAgent, config);
    let result = runner
        .run_agent(RunAgentParameters::default())
        .await
        .expect("debug run should succeed");
    assert_eq!(
        result.outcome,
        Some(agui_rs_core::RunFinishedOutcome::Success)
    );
}

#[tokio::test]
async fn debug_logger_forced_is_enabled_regardless_of_env() {
    use agui_rs_client::DebugLogger;
    let logger = DebugLogger::forced("LIFECYCLE");
    assert!(logger.enabled());
    assert_eq!(logger.prefix(), "LIFECYCLE");
}

// SKIPPED: HttpAgent.abortController clone semantics — the Rust HttpAgent uses a
// per-run AbortHandle rather than a persistent AbortController field, so there
// is no controller state to preserve across a clone.
// SKIPPED: per-stream-stage debug logging ([VERIFY]/[SSE]/[TRANSFORM]/[CHUNK]
// prefixes) is a documented divergence — Rust threads no logger through the pure
// stream functions; lifecycle logging is done at the runner level instead.
