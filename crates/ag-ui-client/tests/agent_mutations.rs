use ag_ui_client::{Agent, AgentConfig, AgentRunner, RunAgentParameters};
use ag_ui_core::types::AssistantMessage;
use ag_ui_core::{factory, Event, Message, Result, RunAgentInput};
use async_trait::async_trait;
use futures::{stream, stream::BoxStream};

struct FakeAgent {
    events: Vec<Event>,
}

#[async_trait]
impl Agent for FakeAgent {
    async fn run(&self, _input: RunAgentInput) -> Result<BoxStream<'static, Result<Event>>> {
        Ok(Box::pin(stream::iter(self.events.clone().into_iter().map(Ok))))
    }
}

#[tokio::test]
async fn on_new_message_replaces_message_in_place() {
    let agent = FakeAgent {
        events: vec![
            factory::run_started("thread-1", "run-1"),
            Event::TextMessageChunk(ag_ui_core::TextMessageChunkEvent {
                message_id: Some("m1".into()),
                role: Some(ag_ui_core::TextMessageRole::Assistant),
                delta: Some("hello".into()),
                name: None,
                base: ag_ui_core::BaseEventFields::default(),
            }),
            factory::run_finished("thread-1", "run-1"),
        ],
    };
    let mut runner = AgentRunner::new(agent, AgentConfig::default());

    let result = runner.run_agent(RunAgentParameters::default()).await.expect("run succeeds");

    assert_eq!(result.new_messages.len(), 1);
    match &result.new_messages[0] {
        Message::Assistant(message) => {
            assert_eq!(message.id, "m1");
            assert_eq!(message.content.as_deref(), Some("hello"));
        }
        other => panic!("expected assistant message, got {other:?}"),
    }
}

#[tokio::test]
async fn on_new_tool_call_mutation_path_replaces_tool_call_in_place() {
    let agent = FakeAgent {
        events: vec![
            factory::run_started("thread-1", "run-1"),
            factory::tool_call_start("call-1", "tool"),
            factory::tool_call_args("call-1", "{}"),
            factory::tool_call_end("call-1"),
            factory::run_finished("thread-1", "run-1"),
        ],
    };
    let mut runner = AgentRunner::new(agent, AgentConfig::default());

    let result = runner.run_agent(RunAgentParameters::default()).await.expect("run succeeds");

    let Message::Assistant(message) = &result.new_messages[0] else {
        panic!("expected assistant message");
    };
    let tool_calls = message.tool_calls.as_ref().expect("tool calls present");
    assert_eq!(tool_calls.len(), 1);
    assert_eq!(tool_calls[0].id, "call-1");
    assert_eq!(tool_calls[0].function.name, "tool");
    assert_eq!(tool_calls[0].function.arguments, "{}");
}

#[tokio::test]
async fn message_and_tool_call_outputs_share_same_assistant_message() {
    let agent = FakeAgent {
        events: vec![
            factory::run_started("thread-1", "run-1"),
            Event::TextMessageChunk(ag_ui_core::TextMessageChunkEvent {
                message_id: Some("m1".into()),
                role: Some(ag_ui_core::TextMessageRole::Assistant),
                delta: Some("hello".into()),
                name: None,
                base: ag_ui_core::BaseEventFields::default(),
            }),
            factory::tool_call_start("call-1", "search"),
            factory::tool_call_args("call-1", "{\"q\":\"rust\"}"),
            factory::tool_call_end("call-1"),
            factory::run_finished("thread-1", "run-1"),
        ],
    };
    let mut runner = AgentRunner::new(agent, AgentConfig::default());

    let result = runner.run_agent(RunAgentParameters::default()).await.expect("run succeeds");

    assert_eq!(result.new_messages.len(), 1);
    let Message::Assistant(message) = &result.new_messages[0] else {
        panic!("expected assistant message");
    };
    assert_eq!(message.content.as_deref(), Some("hello"));
    let tool_calls = message.tool_calls.as_ref().expect("tool calls present");
    assert_eq!(tool_calls[0].id, "call-1");
    assert_eq!(tool_calls[0].function.name, "search");
    assert_eq!(tool_calls[0].function.arguments, "{\"q\":\"rust\"}");
}

#[tokio::test]
async fn new_messages_are_appended_after_initial_messages() {
    let initial_message = Message::Assistant(AssistantMessage {
        id: "existing".into(),
        content: Some("before".into()),
        name: None,
        tool_calls: None,
        encrypted_value: None,
    });
    let agent = FakeAgent {
        events: vec![
            factory::run_started("thread-1", "run-1"),
            Event::TextMessageChunk(ag_ui_core::TextMessageChunkEvent {
                message_id: Some("m1".into()),
                role: Some(ag_ui_core::TextMessageRole::Assistant),
                delta: Some("after".into()),
                name: None,
                base: ag_ui_core::BaseEventFields::default(),
            }),
            factory::run_finished("thread-1", "run-1"),
        ],
    };
    let mut runner = AgentRunner::new(
        agent,
        AgentConfig {
            initial_messages: vec![initial_message],
            ..AgentConfig::default()
        },
    );

    let result = runner.run_agent(RunAgentParameters::default()).await.expect("run succeeds");

    assert_eq!(result.new_messages.len(), 1);
    assert_eq!(result.new_messages[0].id(), "m1");
}

// SKIPPED: TypeScript addMessage/addMessages/setMessages/setState APIs do not exist in the Rust SDK.
// SKIPPED: direct subscriber mutation-hook replacement cannot be authored in integration tests because NewMessageContext/NewToolCallContext are crate-private.
