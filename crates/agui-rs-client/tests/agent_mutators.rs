//! Ported from TypeScript `agent/__tests__/agent-mutations.test.ts` — the
//! `addMessage` / `addMessages` / `setMessages` / `setState` mutator API on the
//! agent, with subscriber notifications.
//!
//! Difference from TS: Rust mutators are `async` and await notifications inline
//! (no event-loop tick), so there is no `waitForAsyncNotifications` shim.

use std::sync::{Arc, Mutex};

use agui_rs_client::{
    Agent, AgentConfig, AgentRunner, AgentSubscriber, NewMessageContext, NewToolCallContext,
    RunContext,
};
use agui_rs_core::types::AssistantMessage;
use agui_rs_core::{
    Event, FunctionCall, Message, Result, RunAgentInput, ToolCall, ToolCallKind,
};
use agui_rs_core::types::UserMessage;
use agui_rs_core::UserMessageContent;
use async_trait::async_trait;
use futures::{stream, stream::BoxStream};

struct IdleAgent;

#[async_trait]
impl Agent for IdleAgent {
    async fn run(&self, _input: RunAgentInput) -> Result<BoxStream<'static, Result<Event>>> {
        Ok(Box::pin(stream::iter(Vec::new())))
    }
}

#[derive(Default)]
struct RecordingSubscriber {
    new_messages: Mutex<Vec<String>>,
    new_tool_calls: Mutex<Vec<String>>,
    messages_changed: Mutex<usize>,
    state_changed: Mutex<usize>,
    order: Option<(&'static str, Arc<Mutex<Vec<String>>>)>,
}

impl RecordingSubscriber {
    fn with_order(label: &'static str, log: Arc<Mutex<Vec<String>>>) -> Self {
        Self {
            order: Some((label, log)),
            ..Default::default()
        }
    }
}

#[async_trait]
impl AgentSubscriber for RecordingSubscriber {
    async fn on_new_message(
        &self,
        ctx: &NewMessageContext<'_>,
    ) -> Result<Option<Message>> {
        self.new_messages
            .lock()
            .unwrap()
            .push(ctx.message.id().to_string());
        if let Some((label, log)) = &self.order {
            log.lock().unwrap().push(format!("{label}-newMessage"));
        }
        Ok(None)
    }

    async fn on_new_tool_call(
        &self,
        ctx: &NewToolCallContext<'_>,
    ) -> Result<Option<ToolCall>> {
        self.new_tool_calls
            .lock()
            .unwrap()
            .push(ctx.tool_call.id.clone());
        Ok(None)
    }

    async fn on_messages_changed(&self, _ctx: &RunContext, _messages: &[Message]) {
        *self.messages_changed.lock().unwrap() += 1;
        if let Some((label, log)) = &self.order {
            log.lock().unwrap().push(format!("{label}-messagesChanged"));
        }
    }

    async fn on_state_changed(&self, _ctx: &RunContext, _state: &agui_rs_core::State) {
        *self.state_changed.lock().unwrap() += 1;
    }
}

fn user_message(id: &str, content: &str) -> Message {
    Message::User(UserMessage {
        id: id.into(),
        content: UserMessageContent::Text(content.into()),
        name: None,
        encrypted_value: None,
    })
}

fn assistant_with_tool_calls(id: &str, tool_calls: Vec<ToolCall>) -> Message {
    Message::Assistant(AssistantMessage {
        id: id.into(),
        content: Some("...".into()),
        name: None,
        tool_calls: Some(tool_calls),
        encrypted_value: None,
    })
}

fn tool_call(id: &str, name: &str) -> ToolCall {
    ToolCall {
        id: id.into(),
        kind: ToolCallKind::Function,
        function: FunctionCall {
            name: name.into(),
            arguments: "{}".into(),
        },
        encrypted_value: None,
    }
}

fn runner_with(sub: Arc<RecordingSubscriber>) -> AgentRunner<IdleAgent> {
    let runner = AgentRunner::new(IdleAgent, AgentConfig::default());
    runner.subscribe(sub);
    runner
}

#[tokio::test]
async fn add_message_fires_new_message_and_messages_changed() {
    let sub = Arc::new(RecordingSubscriber::default());
    let mut runner = runner_with(sub.clone());

    runner.add_message(user_message("user-msg-1", "Hello world")).await;

    assert_eq!(runner.messages().len(), 1);
    assert_eq!(*sub.new_messages.lock().unwrap(), vec!["user-msg-1"]);
    assert_eq!(*sub.messages_changed.lock().unwrap(), 1);
    assert!(sub.new_tool_calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn add_assistant_message_without_tool_calls_does_not_fire_new_tool_call() {
    let sub = Arc::new(RecordingSubscriber::default());
    let mut runner = runner_with(sub.clone());

    runner
        .add_message(Message::Assistant(AssistantMessage {
            id: "assistant-msg-1".into(),
            content: Some("How can I help you?".into()),
            name: None,
            tool_calls: None,
            encrypted_value: None,
        }))
        .await;

    assert_eq!(*sub.new_messages.lock().unwrap(), vec!["assistant-msg-1"]);
    assert_eq!(*sub.messages_changed.lock().unwrap(), 1);
    assert!(sub.new_tool_calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn add_assistant_message_with_tool_calls_fires_new_tool_call_per_call() {
    let sub = Arc::new(RecordingSubscriber::default());
    let mut runner = runner_with(sub.clone());

    runner
        .add_message(assistant_with_tool_calls(
            "assistant-msg-2",
            vec![tool_call("call-1", "get_weather"), tool_call("call-2", "search_web")],
        ))
        .await;

    assert_eq!(*sub.new_messages.lock().unwrap(), vec!["assistant-msg-2"]);
    assert_eq!(
        *sub.new_tool_calls.lock().unwrap(),
        vec!["call-1", "call-2"]
    );
    assert_eq!(*sub.messages_changed.lock().unwrap(), 1);
}

#[tokio::test]
async fn add_messages_fires_per_message_then_changed_once() {
    let sub = Arc::new(RecordingSubscriber::default());
    let mut runner = runner_with(sub.clone());

    runner
        .add_messages(vec![
            user_message("msg-1", "First"),
            assistant_with_tool_calls("msg-2", vec![tool_call("call-1", "test_tool")]),
            user_message("msg-3", "Third"),
        ])
        .await;

    assert_eq!(runner.messages().len(), 3);
    assert_eq!(sub.new_messages.lock().unwrap().len(), 3);
    assert_eq!(*sub.new_tool_calls.lock().unwrap(), vec!["call-1"]);
    assert_eq!(*sub.messages_changed.lock().unwrap(), 1);
}

#[tokio::test]
async fn add_messages_empty_still_fires_changed_once() {
    let sub = Arc::new(RecordingSubscriber::default());
    let mut runner = runner_with(sub.clone());

    runner.add_messages(Vec::new()).await;

    assert_eq!(runner.messages().len(), 0);
    assert_eq!(*sub.messages_changed.lock().unwrap(), 1);
    assert!(sub.new_messages.lock().unwrap().is_empty());
    assert!(sub.new_tool_calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn set_messages_replaces_and_fires_changed_only() {
    let sub = Arc::new(RecordingSubscriber::default());
    let mut runner = runner_with(sub.clone());
    runner.add_message(user_message("initial", "first")).await;
    // Reset counters by reading and asserting baseline.
    let baseline_changed = *sub.messages_changed.lock().unwrap();

    runner
        .set_messages(vec![
            user_message("new-msg-1", "New start"),
            assistant_with_tool_calls("new-msg-2", vec![tool_call("call-1", "some_tool")]),
        ])
        .await;

    assert_eq!(runner.messages().len(), 2);
    assert_eq!(*sub.messages_changed.lock().unwrap(), baseline_changed + 1);
    // set_messages must not fire new_message / new_tool_call for the replacement set.
    assert_eq!(sub.new_messages.lock().unwrap().len(), 1); // only the earlier add_message
    assert!(sub.new_tool_calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn set_state_replaces_and_fires_state_changed_only() {
    let sub = Arc::new(RecordingSubscriber::default());
    let mut runner = runner_with(sub.clone());

    runner
        .set_state(serde_json::json!({"counter": 100, "isActive": true}))
        .await;

    assert_eq!(runner.state(), &serde_json::json!({"counter": 100, "isActive": true}));
    assert_eq!(*sub.state_changed.lock().unwrap(), 1);
    assert_eq!(*sub.messages_changed.lock().unwrap(), 0);
    assert!(sub.new_messages.lock().unwrap().is_empty());
}

#[tokio::test]
async fn notifications_run_in_registration_order() {
    let log = Arc::new(Mutex::new(Vec::new()));
    let mut runner = AgentRunner::new(IdleAgent, AgentConfig::default());
    runner.subscribe(Arc::new(RecordingSubscriber::with_order("first", log.clone())));
    runner.subscribe(Arc::new(RecordingSubscriber::with_order("second", log.clone())));

    runner.add_message(user_message("test-msg", "Test")).await;

    assert_eq!(
        *log.lock().unwrap(),
        vec![
            "first-newMessage",
            "second-newMessage",
            "first-messagesChanged",
            "second-messagesChanged",
        ]
    );
}

#[tokio::test]
async fn all_subscribers_notified_for_each_event() {
    let a = Arc::new(RecordingSubscriber::default());
    let b = Arc::new(RecordingSubscriber::default());
    let mut runner = AgentRunner::new(IdleAgent, AgentConfig::default());
    runner.subscribe(a.clone());
    runner.subscribe(b.clone());

    runner
        .add_message(assistant_with_tool_calls(
            "test-msg",
            vec![tool_call("call-1", "test")],
        ))
        .await;

    for sub in [&a, &b] {
        assert_eq!(*sub.new_messages.lock().unwrap(), vec!["test-msg"]);
        assert_eq!(*sub.new_tool_calls.lock().unwrap(), vec!["call-1"]);
        assert_eq!(*sub.messages_changed.lock().unwrap(), 1);
    }
}
