use agui_rs_client::{Agent, AgentConfig, AgentRunner, RunAgentParameters};
use agui_rs_core::types::AssistantMessage;
use agui_rs_core::{Event, Result, RunAgentInput, RunFinishedEvent};
use async_trait::async_trait;
use futures::{stream, stream::BoxStream};
use std::sync::Arc;

#[derive(Clone, Default)]
struct EchoAgent;

#[async_trait]
impl Agent for EchoAgent {
    async fn run(&self, input: RunAgentInput) -> Result<BoxStream<'static, Result<Event>>> {
        let thread_id = input.thread_id.clone();
        let run_id = input.run_id.clone();
        let message_id = format!("msg-{run_id}");
        let delta = format!("response for {run_id}");

        Ok(Box::pin(stream::iter(vec![
            Ok(agui_rs_core::factory::run_started(&thread_id, &run_id)),
            Ok(Event::TextMessageChunk(
                agui_rs_core::TextMessageChunkEvent {
                    message_id: Some(message_id),
                    role: Some(agui_rs_core::TextMessageRole::Assistant),
                    delta: Some(delta),
                    name: None,
                    base: agui_rs_core::BaseEventFields::default(),
                },
            )),
            Ok(Event::RunFinished(RunFinishedEvent {
                thread_id,
                run_id,
                result: None,
                outcome: Some(agui_rs_core::RunFinishedOutcome::Success),
                base: agui_rs_core::BaseEventFields::default(),
            })),
        ])))
    }
}

#[tokio::test]
async fn concurrent_runners_on_shared_agent_keep_messages_isolated() {
    let agent = EchoAgent;
    let mut runner_a = AgentRunner::new(
        agent.clone(),
        AgentConfig {
            thread_id: Some("thread-a".into()),
            ..AgentConfig::default()
        },
    );
    let mut runner_b = AgentRunner::new(
        agent,
        AgentConfig {
            thread_id: Some("thread-b".into()),
            ..AgentConfig::default()
        },
    );

    let (result_a, result_b) = tokio::join!(
        runner_a.run_agent(RunAgentParameters {
            run_id: Some("run-a".into()),
            ..RunAgentParameters::default()
        }),
        runner_b.run_agent(RunAgentParameters {
            run_id: Some("run-b".into()),
            ..RunAgentParameters::default()
        })
    );

    let result_a = result_a.expect("run a succeeds");
    let result_b = result_b.expect("run b succeeds");

    assert_eq!(result_a.thread_id, "thread-a");
    assert_eq!(result_b.thread_id, "thread-b");
    assert_eq!(result_a.run_id, "run-a");
    assert_eq!(result_b.run_id, "run-b");
    assert_eq!(result_a.new_messages[0].id(), "msg-run-a");
    assert_eq!(result_b.new_messages[0].id(), "msg-run-b");
}

#[tokio::test]
async fn concurrent_runs_preserve_per_run_message_content() {
    let shared_agent = Arc::new(EchoAgent);
    let mut runner_a = AgentRunner::new((*shared_agent).clone(), AgentConfig::default());
    let mut runner_b = AgentRunner::new((*shared_agent).clone(), AgentConfig::default());

    let (result_a, result_b) = tokio::join!(
        runner_a.run_agent(RunAgentParameters {
            run_id: Some("left".into()),
            ..RunAgentParameters::default()
        }),
        runner_b.run_agent(RunAgentParameters {
            run_id: Some("right".into()),
            ..RunAgentParameters::default()
        })
    );

    let result_a = result_a.expect("left succeeds");
    let result_b = result_b.expect("right succeeds");

    match &result_a.new_messages[0] {
        agui_rs_core::Message::Assistant(AssistantMessage { content, .. }) => {
            assert_eq!(content.as_deref(), Some("response for left"));
        }
        other => panic!("expected assistant message, got {other:?}"),
    }
    match &result_b.new_messages[0] {
        agui_rs_core::Message::Assistant(AssistantMessage { content, .. }) => {
            assert_eq!(content.as_deref(), Some("response for right"));
        }
        other => panic!("expected assistant message, got {other:?}"),
    }
}

// SKIPPED: TypeScript permits concurrent text messages and tool calls within one run stream, but Rust verify_events intentionally rejects that model.
// SKIPPED: run_cancellable concurrency cannot be integration-tested through the public crate API because AbortHandle is not publicly re-exported from agui_rs_client.
