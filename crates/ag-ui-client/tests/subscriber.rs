#![allow(dead_code)]
#![allow(unused_imports)]

#[path = "../src/subscriber.rs"]
mod subscriber_impl;

use ag_ui_core::types::{ActivityMessage, AssistantMessage};
use ag_ui_core::{
    BaseEventFields, CustomEvent, Event, FunctionCall, Interrupt, Message, RawEvent,
    ReasoningEncryptedValueEvent, ReasoningEncryptedValueSubtype, ReasoningEndEvent,
    ReasoningMessageChunkEvent, ReasoningMessageContentEvent, ReasoningMessageEndEvent,
    ReasoningMessageRole, ReasoningMessageStartEvent, ReasoningStartEvent, RunErrorEvent,
    RunFinishedEvent, RunFinishedOutcome, RunStartedEvent, State, StateDeltaEvent,
    StateSnapshotEvent, StepFinishedEvent, StepStartedEvent, TextMessageChunkEvent,
    TextMessageContentEvent, TextMessageEndEvent, TextMessageRole, TextMessageStartEvent,
    ThinkingEndEvent, ThinkingStartEvent, ToolCall, ToolCallArgsEvent, ToolCallChunkEvent,
    ToolCallEndEvent, ToolCallKind, ToolCallResultEvent, ToolCallStartEvent,
};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use subscriber_impl::{
    ActivityDeltaContext, ActivitySnapshotContext, AgentSubscriber, EventContext,
    InterruptContext, NewMessageContext, NewToolCallContext, ReasoningContentContext,
    ReasoningEndContext, RunContext, RunFailedContext, RunFinishedContext,
    TextContentContext, TextEndContext, ToolCallArgsContext, ToolCallEndContext,
    ToolCallResultContext,
};

fn run_context() -> RunContext {
    RunContext {
        run_id: "run-1".into(),
        thread_id: "thread-1".into(),
        messages: vec![Message::Assistant(AssistantMessage {
            id: "m-1".into(),
            content: Some("hello".into()),
            name: None,
            tool_calls: Some(vec![tool_call()]),
            encrypted_value: None,
        })],
        state: json!({"count": 1}),
    }
}

fn tool_call() -> ToolCall {
    ToolCall {
        id: "call-1".into(),
        kind: ToolCallKind::Function,
        function: FunctionCall {
            name: "search".into(),
            arguments: "{\"q\":\"rust\"}".into(),
        },
        encrypted_value: None,
    }
}

fn run_started_event() -> RunStartedEvent {
    RunStartedEvent {
        thread_id: "thread-1".into(),
        run_id: "run-1".into(),
        parent_run_id: None,
        input: None,
        base: BaseEventFields::default(),
    }
}

fn run_finished_event() -> RunFinishedEvent {
    RunFinishedEvent {
        thread_id: "thread-1".into(),
        run_id: "run-1".into(),
        result: Some(json!({"ok": true})),
        outcome: Some(RunFinishedOutcome::Success),
        base: BaseEventFields::default(),
    }
}

fn run_error_event() -> RunErrorEvent {
    RunErrorEvent {
        message: "boom".into(),
        code: Some("E_FAIL".into()),
        base: BaseEventFields::default(),
    }
}

fn step_started_event() -> StepStartedEvent {
    StepStartedEvent {
        step_name: "plan".into(),
        base: BaseEventFields::default(),
    }
}

fn step_finished_event() -> StepFinishedEvent {
    StepFinishedEvent {
        step_name: "plan".into(),
        base: BaseEventFields::default(),
    }
}

fn text_start_event() -> TextMessageStartEvent {
    TextMessageStartEvent {
        message_id: "msg-1".into(),
        role: TextMessageRole::Assistant,
        name: None,
        base: BaseEventFields::default(),
    }
}

fn text_content_event() -> TextMessageContentEvent {
    TextMessageContentEvent {
        message_id: "msg-1".into(),
        delta: " world".into(),
        base: BaseEventFields::default(),
    }
}

fn text_end_event() -> TextMessageEndEvent {
    TextMessageEndEvent {
        message_id: "msg-1".into(),
        base: BaseEventFields::default(),
    }
}

fn text_chunk_event() -> TextMessageChunkEvent {
    TextMessageChunkEvent {
        message_id: Some("msg-1".into()),
        role: Some(TextMessageRole::Assistant),
        delta: Some(" world".into()),
        name: None,
        base: BaseEventFields::default(),
    }
}

fn tool_start_event() -> ToolCallStartEvent {
    ToolCallStartEvent {
        tool_call_id: "call-1".into(),
        tool_call_name: "search".into(),
        parent_message_id: Some("m-1".into()),
        base: BaseEventFields::default(),
    }
}

fn tool_args_event() -> ToolCallArgsEvent {
    ToolCallArgsEvent {
        tool_call_id: "call-1".into(),
        delta: "{\"q\":\"ru".into(),
        base: BaseEventFields::default(),
    }
}

fn tool_end_event() -> ToolCallEndEvent {
    ToolCallEndEvent {
        tool_call_id: "call-1".into(),
        base: BaseEventFields::default(),
    }
}

fn tool_chunk_event() -> ToolCallChunkEvent {
    ToolCallChunkEvent {
        tool_call_id: Some("call-1".into()),
        tool_call_name: Some("search".into()),
        parent_message_id: Some("m-1".into()),
        delta: Some("st\"}".into()),
        base: BaseEventFields::default(),
    }
}

fn tool_result_event() -> ToolCallResultEvent {
    ToolCallResultEvent {
        message_id: "tool-msg-1".into(),
        tool_call_id: "call-1".into(),
        content: "done".into(),
        role: None,
        base: BaseEventFields::default(),
    }
}

fn reasoning_start_event() -> ReasoningStartEvent {
    ReasoningStartEvent {
        message_id: "reason-1".into(),
        base: BaseEventFields::default(),
    }
}

fn reasoning_message_start_event() -> ReasoningMessageStartEvent {
    ReasoningMessageStartEvent {
        message_id: "reason-1".into(),
        role: ReasoningMessageRole::Reasoning,
        base: BaseEventFields::default(),
    }
}

fn reasoning_content_event() -> ReasoningMessageContentEvent {
    ReasoningMessageContentEvent {
        message_id: "reason-1".into(),
        delta: "ink".into(),
        base: BaseEventFields::default(),
    }
}

fn reasoning_message_end_event() -> ReasoningMessageEndEvent {
    ReasoningMessageEndEvent {
        message_id: "reason-1".into(),
        base: BaseEventFields::default(),
    }
}

fn reasoning_chunk_event() -> ReasoningMessageChunkEvent {
    ReasoningMessageChunkEvent {
        message_id: Some("reason-1".into()),
        delta: Some("ink".into()),
        base: BaseEventFields::default(),
    }
}

fn reasoning_end_event() -> ReasoningEndEvent {
    ReasoningEndEvent {
        message_id: "reason-1".into(),
        base: BaseEventFields::default(),
    }
}

fn reasoning_encrypted_value_event() -> ReasoningEncryptedValueEvent {
    ReasoningEncryptedValueEvent {
        subtype: ReasoningEncryptedValueSubtype::Message,
        entity_id: "reason-1".into(),
        encrypted_value: "secret".into(),
        base: BaseEventFields::default(),
    }
}

fn thinking_start_event() -> ThinkingStartEvent {
    ThinkingStartEvent {
        title: Some("plan".into()),
        base: BaseEventFields::default(),
    }
}

fn thinking_end_event() -> ThinkingEndEvent {
    ThinkingEndEvent {
        base: BaseEventFields::default(),
    }
}

fn state_snapshot_event() -> StateSnapshotEvent {
    StateSnapshotEvent {
        snapshot: json!({"count": 2}),
        base: BaseEventFields::default(),
    }
}

fn state_delta_event() -> StateDeltaEvent {
    StateDeltaEvent {
        delta: vec![json!({"op": "replace", "path": "/count", "value": 2})],
        base: BaseEventFields::default(),
    }
}

fn activity_snapshot_event() -> ag_ui_core::ActivitySnapshotEvent {
    ag_ui_core::ActivitySnapshotEvent {
        message_id: "activity-1".into(),
        activity_type: "plan".into(),
        content: serde_json::Map::new(),
        replace: true,
        base: BaseEventFields::default(),
    }
}

fn activity_delta_event() -> ag_ui_core::ActivityDeltaEvent {
    ag_ui_core::ActivityDeltaEvent {
        message_id: "activity-1".into(),
        activity_type: "plan".into(),
        patch: vec![json!({"op": "add", "path": "/steps/0", "value": "x"})],
        base: BaseEventFields::default(),
    }
}

fn raw_event() -> RawEvent {
    RawEvent {
        event: json!({"provider": "openai"}),
        source: Some("openai".into()),
        base: BaseEventFields::default(),
    }
}

fn custom_event() -> CustomEvent {
    CustomEvent {
        name: "custom".into(),
        value: json!({"ok": true}),
        base: BaseEventFields::default(),
    }
}

fn interrupt() -> Interrupt {
    Interrupt {
        id: "interrupt-1".into(),
        reason: "needs_human".into(),
        message: None,
        tool_call_id: None,
        response_schema: None,
        expires_at: None,
        metadata: None,
    }
}

struct DefaultSubscriber;

#[async_trait]
impl AgentSubscriber for DefaultSubscriber {}

#[derive(Default)]
struct RecordingSubscriber {
    calls: Arc<Mutex<Vec<String>>>,
}

impl RecordingSubscriber {
    fn record(&self, call: &str) {
        self.calls.lock().expect("calls lock").push(call.into());
    }

    fn calls(&self) -> Vec<String> {
        self.calls.lock().expect("calls lock").clone()
    }
}

#[async_trait]
impl AgentSubscriber for RecordingSubscriber {
    async fn on_run_started_event(
        &self,
        ctx: &EventContext<'_, RunStartedEvent>,
    ) -> std::result::Result<(), ag_ui_core::AgUiError> {
        assert_eq!(ctx.run.run_id, "run-1");
        assert_eq!(ctx.event.run_id, "run-1");
        self.record("run_started");
        Ok(())
    }

    async fn on_text_message_content_event(
        &self,
        ctx: &TextContentContext<'_>,
    ) -> std::result::Result<(), ag_ui_core::AgUiError> {
        assert_eq!(ctx.text_message_buffer, "hello");
        assert_eq!(ctx.event.delta, " world");
        self.record("text_content");
        Ok(())
    }

    async fn on_tool_call_end_event(
        &self,
        ctx: &ToolCallEndContext<'_>,
    ) -> std::result::Result<(), ag_ui_core::AgUiError> {
        assert_eq!(ctx.tool_call_name, "search");
        assert_eq!(ctx.tool_call_args, &json!({"q": "rust"}));
        self.record("tool_end");
        Ok(())
    }

    async fn on_custom_event(
        &self,
        ctx: &EventContext<'_, CustomEvent>,
    ) -> std::result::Result<(), ag_ui_core::AgUiError> {
        assert_eq!(ctx.event.name, "custom");
        self.record("custom");
        Ok(())
    }

    async fn on_new_message(
        &self,
        ctx: &NewMessageContext<'_>,
    ) -> std::result::Result<Option<Message>, ag_ui_core::AgUiError> {
        self.record("new_message");
        Ok(Some(ctx.message.clone()))
    }

    async fn on_new_tool_call(
        &self,
        ctx: &NewToolCallContext<'_>,
    ) -> std::result::Result<Option<ToolCall>, ag_ui_core::AgUiError> {
        self.record("new_tool_call");
        Ok(Some(ctx.tool_call.clone()))
    }
}

#[tokio::test]
async fn default_trait_methods_cover_all_public_callbacks() {
    let subscriber = DefaultSubscriber;
    let run = run_context();
    let finished = run_finished_event();
    let interrupt_outcome = RunFinishedEvent {
        outcome: Some(RunFinishedOutcome::Interrupt {
            interrupts: vec![interrupt()],
        }),
        ..finished.clone()
    };
    let tool_call = tool_call();
    let message = Message::Assistant(AssistantMessage {
        id: "m-2".into(),
        content: Some("new".into()),
        name: None,
        tool_calls: None,
        encrypted_value: None,
    });
    let activity = ActivityMessage {
        id: "activity-1".into(),
        activity_type: "plan".into(),
        content: serde_json::Map::new(),
    };
    let error = ag_ui_core::AgUiError::other("boom");
    let partial_args = json!({"q": "ru"});
    let final_args = json!({"q": "rust"});
    let custom = custom_event();
    let event = Event::Custom(custom.clone());
    let run_started = run_started_event();
    let run_error = run_error_event();
    let step_started = step_started_event();
    let step_finished = step_finished_event();
    let text_start = text_start_event();
    let text_content = text_content_event();
    let text_end = text_end_event();
    let text_chunk = text_chunk_event();
    let tool_start = tool_start_event();
    let tool_args = tool_args_event();
    let tool_end = tool_end_event();
    let tool_chunk = tool_chunk_event();
    let tool_result = tool_result_event();
    let reasoning_started = reasoning_start_event();
    let reasoning_start = reasoning_message_start_event();
    let reasoning_content = reasoning_content_event();
    let reasoning_end = reasoning_message_end_event();
    let reasoning_chunk = reasoning_chunk_event();
    let reasoning_finished = reasoning_end_event();
    let reasoning_encrypted = reasoning_encrypted_value_event();
    let thinking_start = thinking_start_event();
    let thinking_end = thinking_end_event();
    let state_snapshot = state_snapshot_event();
    let state_delta = state_delta_event();
    let messages_snapshot = ag_ui_core::MessagesSnapshotEvent {
        messages: vec![message.clone()],
        base: BaseEventFields::default(),
    };
    let activity_snapshot = activity_snapshot_event();
    let activity_delta = activity_delta_event();
    let raw = raw_event();

    subscriber.on_run_initialized(&run).await;
    subscriber.on_run_finalized(&run).await;
    subscriber.on_messages_changed(&run, &run.messages).await;
    subscriber.on_state_changed(&run, &run.state).await;
    subscriber.on_run_started(&run).await;
    subscriber.on_run_finished(&run).await;
    subscriber.on_run_failed(&run, &error).await;
    subscriber.on_event(&run, &event).await;
    subscriber.on_text_message_start(&run, "msg-1").await;
    subscriber.on_text_message_content(&run, "msg-1", "hello").await;
    subscriber.on_text_message_end(&run, "msg-1").await;
    subscriber.on_tool_call_start(&run, "call-1", "search").await;
    subscriber.on_tool_call_args(&run, "call-1", "{}").await;
    subscriber.on_tool_call_end(&run, "call-1").await;
    subscriber.on_tool_call_result(&run, "call-1", "done").await;

    assert!(subscriber
        .on_run_started_event(&EventContext {
            run: &run,
            event: &run_started,
        })
        .await
        .is_ok());
    assert!(subscriber
        .on_run_finished_event(&RunFinishedContext {
            run: &run,
            event: &finished,
            result: finished.result.as_ref(),
            outcome: finished.outcome.as_ref(),
        })
        .await
        .is_ok());
    assert!(subscriber
        .on_run_error(&EventContext {
            run: &run,
            event: &run_error,
        })
        .await
        .is_ok());
    assert!(subscriber
        .on_run_failed_event(&RunFailedContext {
            run: &run,
            error: &error,
        })
        .await
        .is_ok());
    assert!(subscriber
        .on_step_started(&EventContext {
            run: &run,
            event: &step_started,
        })
        .await
        .is_ok());
    assert!(subscriber
        .on_step_finished(&EventContext {
            run: &run,
            event: &step_finished,
        })
        .await
        .is_ok());
    assert!(subscriber
        .on_text_message_start_event(&EventContext {
            run: &run,
            event: &text_start,
        })
        .await
        .is_ok());
    assert!(subscriber
        .on_text_message_content_event(&TextContentContext {
            run: &run,
            event: &text_content,
            text_message_buffer: "hello",
        })
        .await
        .is_ok());
    assert!(subscriber
        .on_text_message_end_event(&TextEndContext {
            run: &run,
            event: &text_end,
            text_message_buffer: "hello world",
        })
        .await
        .is_ok());
    assert!(subscriber
        .on_text_message_chunk(&EventContext {
            run: &run,
            event: &text_chunk,
        })
        .await
        .is_ok());
    assert!(subscriber
        .on_tool_call_start_event(&EventContext {
            run: &run,
            event: &tool_start,
        })
        .await
        .is_ok());
    assert!(subscriber
        .on_tool_call_args_event(&ToolCallArgsContext {
            run: &run,
            event: &tool_args,
            tool_call_buffer: "{\"q\":\"ru",
            tool_call_name: "search",
            partial_tool_call_args: &partial_args,
        })
        .await
        .is_ok());
    assert!(subscriber
        .on_tool_call_end_event(&ToolCallEndContext {
            run: &run,
            event: &tool_end,
            tool_call_name: "search",
            tool_call_args: &final_args,
        })
        .await
        .is_ok());
    assert!(subscriber
        .on_tool_call_chunk(&EventContext {
            run: &run,
            event: &tool_chunk,
        })
        .await
        .is_ok());
    assert!(subscriber
        .on_tool_call_result_event(&ToolCallResultContext {
            run: &run,
            event: &tool_result,
        })
        .await
        .is_ok());
    assert!(subscriber
        .on_reasoning_started(&EventContext {
            run: &run,
            event: &reasoning_started,
        })
        .await
        .is_ok());
    assert!(subscriber
        .on_reasoning_start(&EventContext {
            run: &run,
            event: &reasoning_start,
        })
        .await
        .is_ok());
    assert!(subscriber
        .on_reasoning_content(&ReasoningContentContext {
            run: &run,
            event: &reasoning_content,
            reasoning_message_buffer: "th",
        })
        .await
        .is_ok());
    assert!(subscriber
        .on_reasoning_end(&ReasoningEndContext {
            run: &run,
            event: &reasoning_end,
            reasoning_message_buffer: "think",
        })
        .await
        .is_ok());
    assert!(subscriber
        .on_reasoning_chunk(&EventContext {
            run: &run,
            event: &reasoning_chunk,
        })
        .await
        .is_ok());
    assert!(subscriber
        .on_reasoning_finished(&EventContext {
            run: &run,
            event: &reasoning_finished,
        })
        .await
        .is_ok());
    assert!(subscriber
        .on_reasoning_encrypted_value(&EventContext {
            run: &run,
            event: &reasoning_encrypted,
        })
        .await
        .is_ok());
    assert!(subscriber
        .on_thinking_start(&EventContext {
            run: &run,
            event: &thinking_start,
        })
        .await
        .is_ok());
    assert!(subscriber
        .on_thinking_end(&EventContext {
            run: &run,
            event: &thinking_end,
        })
        .await
        .is_ok());
    assert!(subscriber
        .on_state_snapshot(&EventContext {
            run: &run,
            event: &state_snapshot,
        })
        .await
        .is_ok());
    assert!(subscriber
        .on_state_delta(&EventContext {
            run: &run,
            event: &state_delta,
        })
        .await
        .is_ok());
    assert!(subscriber
        .on_messages_snapshot(&EventContext {
            run: &run,
            event: &messages_snapshot,
        })
        .await
        .is_ok());
    assert!(subscriber
        .on_activity_snapshot(&ActivitySnapshotContext {
            run: &run,
            event: &activity_snapshot,
            activity_message: Some(&activity),
            existing_message: run.messages.first(),
        })
        .await
        .is_ok());
    assert!(subscriber
        .on_activity_delta(&ActivityDeltaContext {
            run: &run,
            event: &activity_delta,
            activity_message: Some(&activity),
        })
        .await
        .is_ok());
    assert!(subscriber
        .on_raw_event(&EventContext {
            run: &run,
            event: &raw,
        })
        .await
        .is_ok());
    assert!(subscriber
        .on_custom_event(&EventContext {
            run: &run,
            event: &custom,
        })
        .await
        .is_ok());
    assert!(subscriber
        .on_interrupt(&InterruptContext {
            run: &run,
            event: &interrupt_outcome,
            interrupts: match interrupt_outcome.outcome.as_ref() {
                Some(RunFinishedOutcome::Interrupt { interrupts }) => interrupts,
                _ => &[],
            },
        })
        .await
        .is_ok());
    assert!(subscriber
        .on_event_received(&EventContext {
            run: &run,
            event: &event,
        })
        .await
        .is_ok());
    assert!(subscriber
        .on_new_message(&NewMessageContext {
            run: &run,
            message: &message,
        })
        .await
        .expect("new message hook succeeds")
        .is_none());
    assert!(subscriber
        .on_new_tool_call(&NewToolCallContext {
            run: &run,
            tool_call: &tool_call,
        })
        .await
        .expect("new tool call hook succeeds")
        .is_none());
}

#[tokio::test]
async fn custom_subscriber_receives_expected_context_payloads() {
    let subscriber = RecordingSubscriber::default();
    let run = run_context();
    let partial_args = json!({"q": "ru"});
    let final_args = json!({"q": "rust"});
    let message = Message::Assistant(AssistantMessage {
        id: "m-2".into(),
        content: Some("x".into()),
        name: None,
        tool_calls: None,
        encrypted_value: None,
    });
    let call = tool_call();
    let custom = custom_event();

    subscriber
        .on_run_started_event(&EventContext {
            run: &run,
            event: &run_started_event(),
        })
        .await
        .expect("run started hook");
    subscriber
        .on_text_message_content_event(&TextContentContext {
            run: &run,
            event: &text_content_event(),
            text_message_buffer: "hello",
        })
        .await
        .expect("text content hook");
    subscriber
        .on_tool_call_args_event(&ToolCallArgsContext {
            run: &run,
            event: &tool_args_event(),
            tool_call_buffer: "{\"q\":\"ru",
            tool_call_name: "search",
            partial_tool_call_args: &partial_args,
        })
        .await
        .expect("tool args hook");
    subscriber
        .on_tool_call_end_event(&ToolCallEndContext {
            run: &run,
            event: &tool_end_event(),
            tool_call_name: "search",
            tool_call_args: &final_args,
        })
        .await
        .expect("tool end hook");
    subscriber
        .on_custom_event(&EventContext {
            run: &run,
            event: &custom,
        })
        .await
        .expect("custom hook");
    assert!(subscriber
        .on_new_message(&NewMessageContext {
            run: &run,
            message: &message,
        })
        .await
        .expect("new message hook")
        .is_some());
    assert!(subscriber
        .on_new_tool_call(&NewToolCallContext {
            run: &run,
            tool_call: &call,
        })
        .await
        .expect("new tool call hook")
        .is_some());

    assert_eq!(
        subscriber.calls(),
        vec![
            "run_started",
            "text_content",
            "tool_end",
            "custom",
            "new_message",
            "new_tool_call",
        ]
    );
}

#[tokio::test]
async fn run_context_clone_preserves_messages_and_state() {
    let run = run_context();
    let cloned = run.clone();

    assert_eq!(cloned, run);
    assert_eq!(cloned.messages[0].id(), "m-1");
    assert_eq!(cloned.state, json!({"count": 1}));
}

// SKIPPED: stopPropagation chaining from TypeScript does not exist in the Rust AgentSubscriber API.
// SKIPPED: runSubscribersWithMutation isolation contract is a TypeScript-only helper and has no Rust equivalent.
// SKIPPED: browser-process freeze behavior is TypeScript runtime-specific and not modeled in Rust.
