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

use agui_rs_core::types::{AssistantMessage, UserMessage};
use agui_rs_core::{
    factory, BaseEventFields, Event, Message, RunAgentInput, StateDeltaEvent, StateSnapshotEvent,
    TextMessageChunkEvent, TextMessageContentEvent, TextMessageEndEvent, TextMessageRole,
    TextMessageStartEvent, ToolCallArgsEvent, ToolCallChunkEvent, ToolCallEndEvent,
    ToolCallResultEvent, ToolCallStartEvent, ToolResultRole, UserMessageContent,
};
use async_stream::try_stream;
use async_trait::async_trait;
use futures::{stream, StreamExt};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, PartialEq)]
struct EventWithState {
    event: Event,
    messages: Vec<Message>,
    state: Value,
}

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

struct CapturingMiddleware {
    captured_messages: Arc<Mutex<Vec<Message>>>,
    captured_state: Arc<Mutex<Value>>,
    captured_events: Arc<Mutex<Vec<EventWithState>>>,
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

        *self.captured_events.lock().expect("captured events lock") = applied
            .iter()
            .map(|applied| EventWithState {
                event: applied.event.clone(),
                messages: applied.messages.clone(),
                state: applied.state.clone(),
            })
            .collect();

        if let Some(last) = applied.last() {
            *self
                .captured_messages
                .lock()
                .expect("captured messages lock") = last.messages.clone();
            *self.captured_state.lock().expect("captured state lock") = last.state.clone();
        }

        Ok(Box::pin(stream::iter(
            applied
                .into_iter()
                .map(|applied| Ok(applied.event))
                .collect::<Vec<_>>(),
        )))
    }
}

struct PassThroughMiddleware {
    invoked: Arc<Mutex<bool>>,
}

#[async_trait]
impl middleware::Middleware for PassThroughMiddleware {
    async fn run(
        &self,
        input: middleware::MiddlewareInput,
        next: middleware::NextFn,
    ) -> std::result::Result<middleware::EventStream, agui_rs_core::AgUiError> {
        *self.invoked.lock().expect("invoked lock") = true;
        next(input).await
    }
}

struct EventInjectingMiddleware;

#[async_trait]
impl middleware::Middleware for EventInjectingMiddleware {
    async fn run(
        &self,
        input: middleware::MiddlewareInput,
        next: middleware::NextFn,
    ) -> std::result::Result<middleware::EventStream, agui_rs_core::AgUiError> {
        let mut upstream = next(input).await?;
        let output = try_stream! {
            while let Some(item) = upstream.next().await {
                let event = item?;
                let is_run_started = matches!(event, Event::RunStarted(_));
                yield event;
                if is_run_started {
                    yield Event::StateSnapshot(StateSnapshotEvent {
                        snapshot: json!({"injected": true}),
                        base: BaseEventFields::default(),
                    });
                }
            }
        };
        Ok(Box::pin(output))
    }
}

type MessageCapture = Arc<Mutex<Vec<Message>>>;
type StateCapture = Arc<Mutex<Value>>;
type EventCapture = Arc<Mutex<Vec<EventWithState>>>;

fn capturing_handles() -> (
    CapturingMiddleware,
    MessageCapture,
    StateCapture,
    EventCapture,
) {
    let messages = Arc::new(Mutex::new(Vec::new()));
    let state = Arc::new(Mutex::new(Value::Null));
    let events = Arc::new(Mutex::new(Vec::new()));
    (
        CapturingMiddleware {
            captured_messages: Arc::clone(&messages),
            captured_state: Arc::clone(&state),
            captured_events: Arc::clone(&events),
        },
        messages,
        state,
        events,
    )
}

async fn run_agent_with(
    events: Vec<Event>,
    chain: middleware::MiddlewareChain,
    config: agent::AgentConfig,
) -> agent::RunAgentResult {
    let agent = FakeAgent { events };
    let mut runner = agent::AgentRunner::new(agent, config).with_middleware(chain);
    runner
        .run_agent(agent::RunAgentParameters {
            run_id: Some("run-1".into()),
            ..Default::default()
        })
        .await
        .expect("run should succeed")
}

fn text_chunk_events() -> Vec<Event> {
    vec![
        factory::run_started("t1", "run-1"),
        Event::TextMessageChunk(TextMessageChunkEvent {
            message_id: Some("msg-1".into()),
            role: Some(TextMessageRole::Assistant),
            delta: Some("Hello from agent".into()),
            name: None,
            base: BaseEventFields::default(),
        }),
        factory::run_finished("t1", "run-1"),
    ]
}

fn full_text_events() -> Vec<Event> {
    vec![
        factory::run_started("t1", "run-1"),
        Event::TextMessageStart(TextMessageStartEvent {
            message_id: "msg-1".into(),
            role: TextMessageRole::Assistant,
            name: None,
            base: BaseEventFields::default(),
        }),
        Event::TextMessageContent(TextMessageContentEvent {
            message_id: "msg-1".into(),
            delta: "Full text".into(),
            base: BaseEventFields::default(),
        }),
        Event::TextMessageEnd(TextMessageEndEvent {
            message_id: "msg-1".into(),
            base: BaseEventFields::default(),
        }),
        factory::run_finished("t1", "run-1"),
    ]
}

fn tool_chunk_events() -> Vec<Event> {
    vec![
        factory::run_started("t1", "run-1"),
        Event::ToolCallChunk(ToolCallChunkEvent {
            tool_call_id: Some("tc-1".into()),
            tool_call_name: Some("get_weather".into()),
            parent_message_id: None,
            delta: Some("{\"city\":\"NYC\"}".into()),
            base: BaseEventFields::default(),
        }),
        Event::ToolCallResult(ToolCallResultEvent {
            message_id: "tool-result-1".into(),
            tool_call_id: "tc-1".into(),
            content: "72°F".into(),
            role: Some(ToolResultRole::Tool),
            base: BaseEventFields::default(),
        }),
        factory::run_finished("t1", "run-1"),
    ]
}

fn full_tool_call_events() -> Vec<Event> {
    vec![
        factory::run_started("t1", "run-1"),
        Event::ToolCallStart(ToolCallStartEvent {
            tool_call_id: "tc-1".into(),
            tool_call_name: "search".into(),
            parent_message_id: None,
            base: BaseEventFields::default(),
        }),
        Event::ToolCallArgs(ToolCallArgsEvent {
            tool_call_id: "tc-1".into(),
            delta: "{\"q\":\"test\"}".into(),
            base: BaseEventFields::default(),
        }),
        Event::ToolCallEnd(ToolCallEndEvent {
            tool_call_id: "tc-1".into(),
            base: BaseEventFields::default(),
        }),
        Event::ToolCallResult(ToolCallResultEvent {
            message_id: "tool-result-1".into(),
            tool_call_id: "tc-1".into(),
            content: "result".into(),
            role: Some(ToolResultRole::Tool),
            base: BaseEventFields::default(),
        }),
        factory::run_finished("t1", "run-1"),
    ]
}

fn text_and_state_events() -> Vec<Event> {
    vec![
        factory::run_started("t1", "run-1"),
        Event::StateSnapshot(StateSnapshotEvent {
            snapshot: json!({"temperature": 72}),
            base: BaseEventFields::default(),
        }),
        Event::TextMessageChunk(TextMessageChunkEvent {
            message_id: Some("msg-1".into()),
            role: Some(TextMessageRole::Assistant),
            delta: Some("Weather is nice".into()),
            name: None,
            base: BaseEventFields::default(),
        }),
        Event::StateDelta(StateDeltaEvent {
            delta: vec![json!({"op": "replace", "path": "/temperature", "value": 75})],
            base: BaseEventFields::default(),
        }),
        factory::run_finished("t1", "run-1"),
    ]
}

fn messages_snapshot_events() -> Vec<Event> {
    vec![
        factory::run_started("t1", "run-1"),
        Event::TextMessageStart(TextMessageStartEvent {
            message_id: "msg-1".into(),
            role: TextMessageRole::Assistant,
            name: None,
            base: BaseEventFields::default(),
        }),
        Event::TextMessageContent(TextMessageContentEvent {
            message_id: "msg-1".into(),
            delta: "original".into(),
            base: BaseEventFields::default(),
        }),
        Event::TextMessageEnd(TextMessageEndEvent {
            message_id: "msg-1".into(),
            base: BaseEventFields::default(),
        }),
        Event::MessagesSnapshot(agui_rs_core::MessagesSnapshotEvent {
            messages: vec![
                Message::User(UserMessage {
                    id: "snap-1".into(),
                    content: UserMessageContent::Text("question".into()),
                    name: None,
                    encrypted_value: None,
                }),
                Message::Assistant(AssistantMessage {
                    id: "snap-2".into(),
                    content: Some("answer".into()),
                    name: None,
                    tool_calls: None,
                    encrypted_value: None,
                }),
            ],
            base: BaseEventFields::default(),
        }),
        factory::run_finished("t1", "run-1"),
    ]
}

fn multi_message_events() -> Vec<Event> {
    vec![
        factory::run_started("t1", "run-1"),
        Event::TextMessageChunk(TextMessageChunkEvent {
            message_id: Some("msg-1".into()),
            role: Some(TextMessageRole::Assistant),
            delta: Some("First".into()),
            name: None,
            base: BaseEventFields::default(),
        }),
        Event::TextMessageChunk(TextMessageChunkEvent {
            message_id: Some("msg-2".into()),
            role: Some(TextMessageRole::Assistant),
            delta: Some("Second".into()),
            name: None,
            base: BaseEventFields::default(),
        }),
        factory::run_finished("t1", "run-1"),
    ]
}

fn text_then_tool_events() -> Vec<Event> {
    vec![
        factory::run_started("t1", "run-1"),
        Event::TextMessageChunk(TextMessageChunkEvent {
            message_id: Some("msg-1".into()),
            role: Some(TextMessageRole::Assistant),
            delta: Some("Let me search".into()),
            name: None,
            base: BaseEventFields::default(),
        }),
        Event::ToolCallStart(ToolCallStartEvent {
            tool_call_id: "tc-1".into(),
            tool_call_name: "search".into(),
            parent_message_id: None,
            base: BaseEventFields::default(),
        }),
        Event::ToolCallArgs(ToolCallArgsEvent {
            tool_call_id: "tc-1".into(),
            delta: "{\"q\":\"test\"}".into(),
            base: BaseEventFields::default(),
        }),
        Event::ToolCallEnd(ToolCallEndEvent {
            tool_call_id: "tc-1".into(),
            base: BaseEventFields::default(),
        }),
        Event::ToolCallResult(ToolCallResultEvent {
            message_id: "tool-result-1".into(),
            tool_call_id: "tc-1".into(),
            content: "found it".into(),
            role: Some(ToolResultRole::Tool),
            base: BaseEventFields::default(),
        }),
        factory::run_finished("t1", "run-1"),
    ]
}

#[tokio::test]
async fn two_capturing_middlewares_track_text_chunk_messages() {
    let (outer, outer_messages, _, _) = capturing_handles();
    let (inner, inner_messages, _, _) = capturing_handles();
    let mut chain = middleware::MiddlewareChain::new();
    chain.push(outer);
    chain.push(inner);

    let result = run_agent_with(text_chunk_events(), chain, agent::AgentConfig::default()).await;
    assert_eq!(result.new_messages.len(), 1);

    for captured in [&outer_messages, &inner_messages] {
        let captured = captured.lock().expect("messages lock");
        assert_eq!(captured.len(), 1);
        match &captured[0] {
            Message::Assistant(message) => {
                assert_eq!(message.content.as_deref(), Some("Hello from agent"))
            }
            other => panic!("expected assistant message, got {other:?}"),
        }
    }
}

#[tokio::test]
async fn three_capturing_middlewares_track_text_chunk_messages() {
    let (outer, outer_messages, _, _) = capturing_handles();
    let (middle, middle_messages, _, _) = capturing_handles();
    let (inner, inner_messages, _, _) = capturing_handles();
    let mut chain = middleware::MiddlewareChain::new();
    chain.push(outer);
    chain.push(middle);
    chain.push(inner);

    run_agent_with(text_chunk_events(), chain, agent::AgentConfig::default()).await;

    for captured in [&outer_messages, &middle_messages, &inner_messages] {
        let captured = captured.lock().expect("messages lock");
        assert_eq!(captured.len(), 1);
        match &captured[0] {
            Message::Assistant(message) => {
                assert_eq!(message.content.as_deref(), Some("Hello from agent"))
            }
            other => panic!("expected assistant message, got {other:?}"),
        }
    }
}

#[tokio::test]
async fn chained_middlewares_track_full_text_events() {
    let (outer, outer_messages, _, _) = capturing_handles();
    let (inner, inner_messages, _, _) = capturing_handles();
    let mut chain = middleware::MiddlewareChain::new();
    chain.push(outer);
    chain.push(inner);

    run_agent_with(full_text_events(), chain, agent::AgentConfig::default()).await;

    for captured in [&outer_messages, &inner_messages] {
        let captured = captured.lock().expect("messages lock");
        match &captured[0] {
            Message::Assistant(message) => {
                assert_eq!(message.content.as_deref(), Some("Full text"))
            }
            other => panic!("expected assistant message, got {other:?}"),
        }
    }
}

#[tokio::test]
async fn chained_middlewares_track_tool_call_messages_from_chunks() {
    let (outer, outer_messages, _, _) = capturing_handles();
    let (inner, inner_messages, _, _) = capturing_handles();
    let mut chain = middleware::MiddlewareChain::new();
    chain.push(outer);
    chain.push(inner);

    run_agent_with(tool_chunk_events(), chain, agent::AgentConfig::default()).await;

    for captured in [&outer_messages, &inner_messages] {
        let captured = captured.lock().expect("messages lock");
        assert_eq!(captured.len(), 2);
        match &captured[0] {
            Message::Assistant(message) => {
                let tool_calls = message.tool_calls.as_ref().expect("tool calls");
                assert_eq!(tool_calls[0].function.name, "get_weather");
                assert_eq!(tool_calls[0].function.arguments, "{\"city\":\"NYC\"}");
            }
            other => panic!("expected assistant message, got {other:?}"),
        }
        match &captured[1] {
            Message::Tool(message) => assert_eq!(message.content, "72°F"),
            other => panic!("expected tool message, got {other:?}"),
        }
    }
}

#[tokio::test]
async fn chained_middlewares_track_full_tool_call_events() {
    let (outer, outer_messages, _, _) = capturing_handles();
    let (inner, inner_messages, _, _) = capturing_handles();
    let mut chain = middleware::MiddlewareChain::new();
    chain.push(outer);
    chain.push(inner);

    run_agent_with(
        full_tool_call_events(),
        chain,
        agent::AgentConfig::default(),
    )
    .await;

    for captured in [&outer_messages, &inner_messages] {
        let captured = captured.lock().expect("messages lock");
        assert_eq!(captured.len(), 2);
        match &captured[0] {
            Message::Assistant(message) => {
                let tool_calls = message.tool_calls.as_ref().expect("tool calls");
                assert_eq!(tool_calls[0].function.name, "search");
                assert_eq!(tool_calls[0].function.arguments, "{\"q\":\"test\"}");
            }
            other => panic!("expected assistant message, got {other:?}"),
        }
    }
}

#[tokio::test]
async fn chained_middlewares_propagate_state_snapshot_and_delta() {
    let (outer, outer_messages, outer_state, _) = capturing_handles();
    let (inner, inner_messages, inner_state, _) = capturing_handles();
    let mut chain = middleware::MiddlewareChain::new();
    chain.push(outer);
    chain.push(inner);

    run_agent_with(
        text_and_state_events(),
        chain,
        agent::AgentConfig::default(),
    )
    .await;

    assert_eq!(
        *outer_state.lock().expect("outer state lock"),
        json!({"temperature": 75})
    );
    assert_eq!(
        *inner_state.lock().expect("inner state lock"),
        json!({"temperature": 75})
    );
    assert_eq!(outer_messages.lock().expect("outer messages lock").len(), 1);
    assert_eq!(inner_messages.lock().expect("inner messages lock").len(), 1);
}

#[tokio::test]
async fn state_evolves_incrementally_across_events() {
    let (capturing, _, _, captured_events) = capturing_handles();
    let mut chain = middleware::MiddlewareChain::new();
    chain.push(capturing);

    run_agent_with(
        text_and_state_events(),
        chain,
        agent::AgentConfig::default(),
    )
    .await;

    let captured = captured_events.lock().expect("events lock");
    let snapshot = captured
        .iter()
        .find(|entry| matches!(entry.event, Event::StateSnapshot(_)))
        .expect("state snapshot event");
    assert_eq!(snapshot.state, json!({"temperature": 72}));

    let delta = captured
        .iter()
        .find(|entry| matches!(entry.event, Event::StateDelta(_)))
        .expect("state delta event");
    assert_eq!(delta.state, json!({"temperature": 75}));
}

#[tokio::test]
async fn messages_snapshot_replaces_messages_for_all_layers() {
    let (outer, outer_messages, _, _) = capturing_handles();
    let (inner, inner_messages, _, _) = capturing_handles();
    let mut chain = middleware::MiddlewareChain::new();
    chain.push(outer);
    chain.push(inner);

    run_agent_with(
        messages_snapshot_events(),
        chain,
        agent::AgentConfig::default(),
    )
    .await;

    for captured in [&outer_messages, &inner_messages] {
        let captured = captured.lock().expect("messages lock");
        assert_eq!(captured.len(), 2);
        assert_eq!(captured[0].id(), "snap-1");
        assert_eq!(captured[1].id(), "snap-2");
    }
}

#[tokio::test]
async fn multiple_sequential_text_messages_are_tracked() {
    let (outer, outer_messages, _, _) = capturing_handles();
    let (inner, inner_messages, _, _) = capturing_handles();
    let mut chain = middleware::MiddlewareChain::new();
    chain.push(outer);
    chain.push(inner);

    run_agent_with(multi_message_events(), chain, agent::AgentConfig::default()).await;

    for captured in [&outer_messages, &inner_messages] {
        let captured = captured.lock().expect("messages lock");
        assert_eq!(captured.len(), 2);
        assert_eq!(captured[0].id(), "msg-1");
        assert_eq!(captured[1].id(), "msg-2");
    }
}

#[tokio::test]
async fn mixed_text_and_tool_call_events_attach_tool_call_to_existing_message() {
    let (outer, outer_messages, _, _) = capturing_handles();
    let (inner, inner_messages, _, _) = capturing_handles();
    let mut chain = middleware::MiddlewareChain::new();
    chain.push(outer);
    chain.push(inner);

    run_agent_with(
        text_then_tool_events(),
        chain,
        agent::AgentConfig::default(),
    )
    .await;

    for captured in [&outer_messages, &inner_messages] {
        let captured = captured.lock().expect("messages lock");
        let first = captured
            .iter()
            .find(|message| message.id() == "msg-1")
            .expect("assistant msg");
        match first {
            Message::Assistant(message) => {
                assert_eq!(message.content.as_deref(), Some("Let me search"));
                let tool_calls = message.tool_calls.as_ref().expect("tool calls");
                assert_eq!(tool_calls[0].function.name, "search");
            }
            other => panic!("expected assistant message, got {other:?}"),
        }
        let tool = captured
            .iter()
            .find(|message| matches!(message, Message::Tool(_)))
            .expect("tool msg");
        match tool {
            Message::Tool(message) => assert_eq!(message.content, "found it"),
            other => panic!("expected tool message, got {other:?}"),
        }
    }
}

#[tokio::test]
async fn capturing_then_passthrough_middleware_works() {
    let (capturing, captured_messages, _, _) = capturing_handles();
    let invoked = Arc::new(Mutex::new(false));
    let mut chain = middleware::MiddlewareChain::new();
    chain.push(capturing);
    chain.push(PassThroughMiddleware {
        invoked: Arc::clone(&invoked),
    });

    run_agent_with(text_chunk_events(), chain, agent::AgentConfig::default()).await;
    assert!(*invoked.lock().expect("invoked lock"));
    assert_eq!(captured_messages.lock().expect("messages lock").len(), 1);
}

#[tokio::test]
async fn passthrough_then_capturing_middleware_works() {
    let (capturing, captured_messages, _, _) = capturing_handles();
    let invoked = Arc::new(Mutex::new(false));
    let mut chain = middleware::MiddlewareChain::new();
    chain.push(PassThroughMiddleware {
        invoked: Arc::clone(&invoked),
    });
    chain.push(capturing);

    run_agent_with(text_chunk_events(), chain, agent::AgentConfig::default()).await;
    assert!(*invoked.lock().expect("invoked lock"));
    assert_eq!(captured_messages.lock().expect("messages lock").len(), 1);
}

#[tokio::test]
async fn event_injecting_middleware_exposes_injected_state_downstream() {
    let (capturing, captured_messages, captured_state, _) = capturing_handles();
    let mut chain = middleware::MiddlewareChain::new();
    chain.push(capturing);
    chain.push(EventInjectingMiddleware);

    run_agent_with(text_chunk_events(), chain, agent::AgentConfig::default()).await;
    assert_eq!(
        *captured_state.lock().expect("state lock"),
        json!({"injected": true})
    );
    assert_eq!(captured_messages.lock().expect("messages lock").len(), 1);
}

#[tokio::test]
async fn initial_messages_are_preserved_and_new_messages_accumulate() {
    let (outer, outer_messages, _, _) = capturing_handles();
    let (inner, inner_messages, _, _) = capturing_handles();
    let mut chain = middleware::MiddlewareChain::new();
    chain.push(outer);
    chain.push(inner);

    let config = agent::AgentConfig {
        initial_messages: vec![Message::User(UserMessage {
            id: "existing".into(),
            content: UserMessageContent::Text("Hi".into()),
            name: None,
            encrypted_value: None,
        })],
        ..Default::default()
    };

    let result = run_agent_with(text_chunk_events(), chain, config).await;
    assert_eq!(result.new_messages.len(), 1);

    for captured in [&outer_messages, &inner_messages] {
        let captured = captured.lock().expect("messages lock");
        assert_eq!(captured.len(), 2);
        assert_eq!(captured[0].id(), "existing");
        assert_eq!(captured[1].id(), "msg-1");
    }
}

#[tokio::test]
async fn initial_state_is_preserved_without_state_events() {
    let (capturing, _, captured_state, _) = capturing_handles();
    let mut chain = middleware::MiddlewareChain::new();
    chain.push(capturing);

    let config = agent::AgentConfig {
        initial_state: json!({"preserved": true}),
        ..Default::default()
    };

    run_agent_with(text_chunk_events(), chain, config).await;
    assert_eq!(
        *captured_state.lock().expect("state lock"),
        json!({"preserved": true})
    );
}

#[tokio::test]
async fn messages_accumulate_correctly_at_each_full_text_event() {
    let (capturing, _, _, captured_events) = capturing_handles();
    let mut chain = middleware::MiddlewareChain::new();
    chain.push(capturing);

    run_agent_with(full_text_events(), chain, agent::AgentConfig::default()).await;

    let captured = captured_events.lock().expect("events lock");
    let at_start = captured
        .iter()
        .find(|entry| matches!(entry.event, Event::TextMessageStart(_)))
        .expect("text start");
    let at_content = captured
        .iter()
        .find(|entry| matches!(entry.event, Event::TextMessageContent(_)))
        .expect("text content");
    let at_finished = captured
        .iter()
        .find(|entry| matches!(entry.event, Event::RunFinished(_)))
        .expect("run finished");

    match &at_start.messages[0] {
        Message::Assistant(message) => assert_eq!(message.content.as_deref(), Some("")),
        other => panic!("expected assistant message, got {other:?}"),
    }
    match &at_content.messages[0] {
        Message::Assistant(message) => assert_eq!(message.content.as_deref(), Some("Full text")),
        other => panic!("expected assistant message, got {other:?}"),
    }
    match &at_finished.messages[0] {
        Message::Assistant(message) => assert_eq!(message.content.as_deref(), Some("Full text")),
        other => panic!("expected assistant message, got {other:?}"),
    }
}

#[tokio::test]
async fn tool_call_messages_build_incrementally_across_events() {
    let (capturing, _, _, captured_events) = capturing_handles();
    let mut chain = middleware::MiddlewareChain::new();
    chain.push(capturing);

    run_agent_with(
        full_tool_call_events(),
        chain,
        agent::AgentConfig::default(),
    )
    .await;

    let captured = captured_events.lock().expect("events lock");
    let at_start = captured
        .iter()
        .find(|entry| matches!(entry.event, Event::ToolCallStart(_)))
        .expect("tool start");
    let at_args = captured
        .iter()
        .find(|entry| matches!(entry.event, Event::ToolCallArgs(_)))
        .expect("tool args");
    let at_result = captured
        .iter()
        .find(|entry| matches!(entry.event, Event::ToolCallResult(_)))
        .expect("tool result");

    match &at_start.messages[0] {
        Message::Assistant(message) => {
            let tool_calls = message.tool_calls.as_ref().expect("tool calls");
            assert_eq!(tool_calls[0].function.arguments, "");
        }
        other => panic!("expected assistant message, got {other:?}"),
    }
    match &at_args.messages[0] {
        Message::Assistant(message) => {
            let tool_calls = message.tool_calls.as_ref().expect("tool calls");
            assert_eq!(tool_calls[0].function.arguments, "{\"q\":\"test\"}");
        }
        other => panic!("expected assistant message, got {other:?}"),
    }
    assert_eq!(at_result.messages.len(), 2);
    assert!(matches!(at_result.messages[1], Message::Tool(_)));
}
