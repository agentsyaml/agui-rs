use ag_ui_core::{
    ActivityDeltaEvent, ActivitySnapshotEvent, AgUiError, CustomEvent, Event,
    Interrupt, Message, MessagesSnapshotEvent, RawEvent, ReasoningEncryptedValueEvent,
    ReasoningEndEvent, ReasoningMessageChunkEvent, ReasoningMessageContentEvent,
    ReasoningMessageEndEvent, ReasoningMessageStartEvent, ReasoningStartEvent, RunErrorEvent,
    RunFinishedEvent, RunFinishedOutcome, RunStartedEvent, State, StateDeltaEvent,
    StateSnapshotEvent, StepFinishedEvent, StepStartedEvent, TextMessageChunkEvent,
    TextMessageContentEvent, TextMessageEndEvent, TextMessageStartEvent, ThinkingEndEvent,
    ThinkingStartEvent, ToolCall, ToolCallArgsEvent, ToolCallChunkEvent, ToolCallEndEvent,
    ToolCallResultEvent, ToolCallStartEvent,
};
use ag_ui_core::types::ActivityMessage;
use async_trait::async_trait;

#[derive(Debug, Clone, PartialEq)]
pub struct RunContext {
    pub run_id: String,
    pub thread_id: String,
    pub messages: Vec<Message>,
    pub state: State,
}

#[derive(Debug, Clone, Copy)]
pub struct EventContext<'a, E> {
    pub run: &'a RunContext,
    pub event: &'a E,
}

#[derive(Debug, Clone, Copy)]
pub struct RunFinishedContext<'a> {
    pub run: &'a RunContext,
    pub event: &'a RunFinishedEvent,
    pub result: Option<&'a serde_json::Value>,
    pub outcome: Option<&'a RunFinishedOutcome>,
}

#[derive(Debug, Clone, Copy)]
pub struct RunFailedContext<'a> {
    pub run: &'a RunContext,
    pub error: &'a AgUiError,
}

#[derive(Debug, Clone, Copy)]
pub struct TextContentContext<'a> {
    pub run: &'a RunContext,
    pub event: &'a TextMessageContentEvent,
    pub text_message_buffer: &'a str,
}

#[derive(Debug, Clone, Copy)]
pub struct TextEndContext<'a> {
    pub run: &'a RunContext,
    pub event: &'a TextMessageEndEvent,
    pub text_message_buffer: &'a str,
}

#[derive(Debug, Clone, Copy)]
pub struct ToolCallArgsContext<'a> {
    pub run: &'a RunContext,
    pub event: &'a ToolCallArgsEvent,
    pub tool_call_buffer: &'a str,
    pub tool_call_name: &'a str,
    pub partial_tool_call_args: &'a serde_json::Value,
}

#[derive(Debug, Clone, Copy)]
pub struct ToolCallEndContext<'a> {
    pub run: &'a RunContext,
    pub event: &'a ToolCallEndEvent,
    pub tool_call_name: &'a str,
    pub tool_call_args: &'a serde_json::Value,
}

#[derive(Debug, Clone, Copy)]
pub struct ToolCallResultContext<'a> {
    pub run: &'a RunContext,
    pub event: &'a ToolCallResultEvent,
}

#[derive(Debug, Clone, Copy)]
pub struct ReasoningContentContext<'a> {
    pub run: &'a RunContext,
    pub event: &'a ReasoningMessageContentEvent,
    pub reasoning_message_buffer: &'a str,
}

#[derive(Debug, Clone, Copy)]
pub struct ReasoningEndContext<'a> {
    pub run: &'a RunContext,
    pub event: &'a ReasoningMessageEndEvent,
    pub reasoning_message_buffer: &'a str,
}

#[derive(Debug, Clone, Copy)]
pub struct ActivitySnapshotContext<'a> {
    pub run: &'a RunContext,
    pub event: &'a ActivitySnapshotEvent,
    pub activity_message: Option<&'a ActivityMessage>,
    pub existing_message: Option<&'a Message>,
}

#[derive(Debug, Clone, Copy)]
pub struct ActivityDeltaContext<'a> {
    pub run: &'a RunContext,
    pub event: &'a ActivityDeltaEvent,
    pub activity_message: Option<&'a ActivityMessage>,
}

#[derive(Debug, Clone, Copy)]
pub struct InterruptContext<'a> {
    pub run: &'a RunContext,
    pub event: &'a RunFinishedEvent,
    pub interrupts: &'a [Interrupt],
}

#[derive(Debug, Clone, Copy)]
pub struct NewMessageContext<'a> {
    pub run: &'a RunContext,
    pub message: &'a Message,
}

#[derive(Debug, Clone, Copy)]
pub struct NewToolCallContext<'a> {
    pub run: &'a RunContext,
    pub tool_call: &'a ToolCall,
}

#[async_trait]
pub trait AgentSubscriber: Send + Sync {
    async fn on_run_initialized(&self, _ctx: &RunContext) {}

    async fn on_run_finalized(&self, _ctx: &RunContext) {}

    async fn on_messages_changed(&self, _ctx: &RunContext, _messages: &[Message]) {}

    async fn on_state_changed(&self, _ctx: &RunContext, _state: &State) {}

    async fn on_run_started(&self, _ctx: &RunContext) {}

    async fn on_run_finished(&self, _ctx: &RunContext) {}

    async fn on_run_failed(&self, _ctx: &RunContext, _err: &AgUiError) {}

    async fn on_event(&self, _ctx: &RunContext, _event: &Event) {}

    async fn on_text_message_start(&self, _ctx: &RunContext, _message_id: &str) {}

    async fn on_text_message_content(&self, _ctx: &RunContext, _message_id: &str, _delta: &str) {}

    async fn on_text_message_end(&self, _ctx: &RunContext, _message_id: &str) {}

    async fn on_tool_call_start(&self, _ctx: &RunContext, _tool_call_id: &str, _tool_call_name: &str) {}

    async fn on_tool_call_args(&self, _ctx: &RunContext, _tool_call_id: &str, _delta: &str) {}

    async fn on_tool_call_end(&self, _ctx: &RunContext, _tool_call_id: &str) {}

    async fn on_tool_call_result(&self, _ctx: &RunContext, _tool_call_id: &str, _content: &str) {}

    async fn on_run_started_event(
        &self,
        _ctx: &EventContext<'_, RunStartedEvent>,
    ) -> std::result::Result<(), AgUiError> {
        std::result::Result::Ok(())
    }

    async fn on_run_finished_event(
        &self,
        _ctx: &RunFinishedContext<'_>,
    ) -> std::result::Result<(), AgUiError> {
        std::result::Result::Ok(())
    }

    async fn on_run_error(
        &self,
        _ctx: &EventContext<'_, RunErrorEvent>,
    ) -> std::result::Result<(), AgUiError> {
        std::result::Result::Ok(())
    }

    async fn on_run_failed_event(
        &self,
        _ctx: &RunFailedContext<'_>,
    ) -> std::result::Result<(), AgUiError> {
        std::result::Result::Ok(())
    }

    async fn on_step_started(
        &self,
        _ctx: &EventContext<'_, StepStartedEvent>,
    ) -> std::result::Result<(), AgUiError> {
        std::result::Result::Ok(())
    }

    async fn on_step_finished(
        &self,
        _ctx: &EventContext<'_, StepFinishedEvent>,
    ) -> std::result::Result<(), AgUiError> {
        std::result::Result::Ok(())
    }

    async fn on_text_message_start_event(
        &self,
        _ctx: &EventContext<'_, TextMessageStartEvent>,
    ) -> std::result::Result<(), AgUiError> {
        std::result::Result::Ok(())
    }

    async fn on_text_message_content_event(
        &self,
        _ctx: &TextContentContext<'_>,
    ) -> std::result::Result<(), AgUiError> {
        std::result::Result::Ok(())
    }

    async fn on_text_message_end_event(
        &self,
        _ctx: &TextEndContext<'_>,
    ) -> std::result::Result<(), AgUiError> {
        std::result::Result::Ok(())
    }

    async fn on_text_message_chunk(
        &self,
        _ctx: &EventContext<'_, TextMessageChunkEvent>,
    ) -> std::result::Result<(), AgUiError> {
        std::result::Result::Ok(())
    }

    async fn on_tool_call_start_event(
        &self,
        _ctx: &EventContext<'_, ToolCallStartEvent>,
    ) -> std::result::Result<(), AgUiError> {
        std::result::Result::Ok(())
    }

    async fn on_tool_call_args_event(
        &self,
        _ctx: &ToolCallArgsContext<'_>,
    ) -> std::result::Result<(), AgUiError> {
        std::result::Result::Ok(())
    }

    async fn on_tool_call_end_event(
        &self,
        _ctx: &ToolCallEndContext<'_>,
    ) -> std::result::Result<(), AgUiError> {
        std::result::Result::Ok(())
    }

    async fn on_tool_call_chunk(
        &self,
        _ctx: &EventContext<'_, ToolCallChunkEvent>,
    ) -> std::result::Result<(), AgUiError> {
        std::result::Result::Ok(())
    }

    async fn on_tool_call_result_event(
        &self,
        _ctx: &ToolCallResultContext<'_>,
    ) -> std::result::Result<(), AgUiError> {
        std::result::Result::Ok(())
    }

    async fn on_reasoning_started(
        &self,
        _ctx: &EventContext<'_, ReasoningStartEvent>,
    ) -> std::result::Result<(), AgUiError> {
        std::result::Result::Ok(())
    }

    async fn on_reasoning_start(
        &self,
        _ctx: &EventContext<'_, ReasoningMessageStartEvent>,
    ) -> std::result::Result<(), AgUiError> {
        std::result::Result::Ok(())
    }

    async fn on_reasoning_content(
        &self,
        _ctx: &ReasoningContentContext<'_>,
    ) -> std::result::Result<(), AgUiError> {
        std::result::Result::Ok(())
    }

    async fn on_reasoning_end(
        &self,
        _ctx: &ReasoningEndContext<'_>,
    ) -> std::result::Result<(), AgUiError> {
        std::result::Result::Ok(())
    }

    async fn on_reasoning_chunk(
        &self,
        _ctx: &EventContext<'_, ReasoningMessageChunkEvent>,
    ) -> std::result::Result<(), AgUiError> {
        std::result::Result::Ok(())
    }

    async fn on_reasoning_finished(
        &self,
        _ctx: &EventContext<'_, ReasoningEndEvent>,
    ) -> std::result::Result<(), AgUiError> {
        std::result::Result::Ok(())
    }

    async fn on_reasoning_encrypted_value(
        &self,
        _ctx: &EventContext<'_, ReasoningEncryptedValueEvent>,
    ) -> std::result::Result<(), AgUiError> {
        std::result::Result::Ok(())
    }

    async fn on_thinking_start(
        &self,
        _ctx: &EventContext<'_, ThinkingStartEvent>,
    ) -> std::result::Result<(), AgUiError> {
        std::result::Result::Ok(())
    }

    async fn on_thinking_end(
        &self,
        _ctx: &EventContext<'_, ThinkingEndEvent>,
    ) -> std::result::Result<(), AgUiError> {
        std::result::Result::Ok(())
    }

    async fn on_state_snapshot(
        &self,
        _ctx: &EventContext<'_, StateSnapshotEvent>,
    ) -> std::result::Result<(), AgUiError> {
        std::result::Result::Ok(())
    }

    async fn on_state_delta(
        &self,
        _ctx: &EventContext<'_, StateDeltaEvent>,
    ) -> std::result::Result<(), AgUiError> {
        std::result::Result::Ok(())
    }

    async fn on_messages_snapshot(
        &self,
        _ctx: &EventContext<'_, MessagesSnapshotEvent>,
    ) -> std::result::Result<(), AgUiError> {
        std::result::Result::Ok(())
    }

    async fn on_activity_snapshot(
        &self,
        _ctx: &ActivitySnapshotContext<'_>,
    ) -> std::result::Result<(), AgUiError> {
        std::result::Result::Ok(())
    }

    async fn on_activity_delta(
        &self,
        _ctx: &ActivityDeltaContext<'_>,
    ) -> std::result::Result<(), AgUiError> {
        std::result::Result::Ok(())
    }

    async fn on_raw_event(
        &self,
        _ctx: &EventContext<'_, RawEvent>,
    ) -> std::result::Result<(), AgUiError> {
        std::result::Result::Ok(())
    }

    async fn on_custom_event(
        &self,
        _ctx: &EventContext<'_, CustomEvent>,
    ) -> std::result::Result<(), AgUiError> {
        std::result::Result::Ok(())
    }

    async fn on_interrupt(
        &self,
        _ctx: &InterruptContext<'_>,
    ) -> std::result::Result<(), AgUiError> {
        std::result::Result::Ok(())
    }

    async fn on_event_received(
        &self,
        _ctx: &EventContext<'_, Event>,
    ) -> std::result::Result<(), AgUiError> {
        std::result::Result::Ok(())
    }

    async fn on_new_message(
        &self,
        _ctx: &NewMessageContext<'_>,
    ) -> std::result::Result<Option<Message>, AgUiError> {
        std::result::Result::Ok(None)
    }

    async fn on_new_tool_call(
        &self,
        _ctx: &NewToolCallContext<'_>,
    ) -> std::result::Result<Option<ToolCall>, AgUiError> {
        std::result::Result::Ok(None)
    }
}

#[cfg(test)]
mod subscriber_tests {
    use super::*;
    use ag_ui_core::{
        BaseEventFields, FunctionCall, ReasoningMessageRole, RunFinishedOutcome,
        TextMessageRole, ToolCallKind,
    };
    use ag_ui_core::types::AssistantMessage;
    use serde_json::json;
    use std::sync::{Arc, Mutex};

    fn run_context() -> RunContext {
        RunContext {
            run_id: "run-1".into(),
            thread_id: "thread-1".into(),
            messages: vec![assistant_message("m-1", Some("hello"), None)],
            state: json!({"count": 1}),
        }
    }

    fn assistant_message(
        id: &str,
        content: Option<&str>,
        tool_calls: Option<Vec<ToolCall>>,
    ) -> Message {
        Message::Assistant(AssistantMessage {
            id: id.into(),
            content: content.map(str::to_owned),
            name: None,
            tool_calls,
            encrypted_value: None,
        })
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
            parent_message_id: None,
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
            parent_message_id: None,
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
            delta: "think".into(),
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
            subtype: ag_ui_core::ReasoningEncryptedValueSubtype::Message,
            entity_id: "reason-1".into(),
            encrypted_value: "enc".into(),
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

    fn messages_snapshot_event() -> MessagesSnapshotEvent {
        MessagesSnapshotEvent {
            messages: vec![assistant_message("m-2", Some("new"), None)],
            base: BaseEventFields::default(),
        }
    }

    fn activity_snapshot_event() -> ActivitySnapshotEvent {
        ActivitySnapshotEvent {
            message_id: "activity-1".into(),
            activity_type: "plan".into(),
            content: serde_json::Map::new(),
            replace: true,
            base: BaseEventFields::default(),
        }
    }

    fn activity_delta_event() -> ActivityDeltaEvent {
        ActivityDeltaEvent {
            message_id: "activity-1".into(),
            activity_type: "plan".into(),
            patch: vec![json!({"op": "add", "path": "/steps/0", "value": "x"})],
            base: BaseEventFields::default(),
        }
    }

    fn activity_message() -> ActivityMessage {
        ActivityMessage {
            id: "activity-1".into(),
            activity_type: "plan".into(),
            content: serde_json::Map::new(),
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
        fn calls(&self) -> Vec<String> {
            self.calls.lock().expect("lock calls").clone()
        }

        fn record(&self, name: &str) {
            self.calls.lock().expect("lock calls").push(name.into());
        }
    }

    #[async_trait]
    impl AgentSubscriber for RecordingSubscriber {
        async fn on_run_started_event(
            &self,
            ctx: &EventContext<'_, RunStartedEvent>,
        ) -> std::result::Result<(), AgUiError> {
            assert_eq!(ctx.event.run_id, "run-1");
            self.record("run_started");
            std::result::Result::Ok(())
        }

        async fn on_text_message_content_event(
            &self,
            ctx: &TextContentContext<'_>,
        ) -> std::result::Result<(), AgUiError> {
            assert_eq!(ctx.text_message_buffer, "hello");
            self.record("text_content");
            std::result::Result::Ok(())
        }

        async fn on_tool_call_end_event(
            &self,
            ctx: &ToolCallEndContext<'_>,
        ) -> std::result::Result<(), AgUiError> {
            assert_eq!(ctx.tool_call_name, "search");
            self.record("tool_end");
            std::result::Result::Ok(())
        }

        async fn on_custom_event(
            &self,
            ctx: &EventContext<'_, CustomEvent>,
        ) -> std::result::Result<(), AgUiError> {
            assert_eq!(ctx.event.name, "custom");
            self.record("custom");
            std::result::Result::Ok(())
        }

        async fn on_new_message(
            &self,
            ctx: &NewMessageContext<'_>,
        ) -> std::result::Result<Option<Message>, AgUiError> {
            self.record("new_message");
            std::result::Result::Ok(Some(ctx.message.clone()))
        }

        async fn on_new_tool_call(
            &self,
            ctx: &NewToolCallContext<'_>,
        ) -> std::result::Result<Option<ToolCall>, AgUiError> {
            self.record("new_tool_call");
            std::result::Result::Ok(Some(ctx.tool_call.clone()))
        }
    }

    #[tokio::test]
    async fn default_lifecycle_and_compat_hooks_are_noops() {
        let subscriber = DefaultSubscriber;
        let run = run_context();
        let err = AgUiError::other("boom");
        let event = Event::RunStarted(run_started_event());

        subscriber.on_run_initialized(&run).await;
        subscriber.on_run_started(&run).await;
        subscriber.on_run_failed(&run, &err).await;
        subscriber.on_event(&run, &event).await;
        subscriber.on_run_finished(&run).await;
        subscriber.on_run_finalized(&run).await;
        subscriber.on_messages_changed(&run, &run.messages).await;
        subscriber.on_state_changed(&run, &run.state).await;
    }

    #[tokio::test]
    async fn default_event_hooks_return_ok() {
        let subscriber = DefaultSubscriber;
        let run = run_context();
        let run_started = run_started_event();
        let run_finished = run_finished_event();
        let run_error = run_error_event();
        let step_started = step_started_event();
        let step_finished = step_finished_event();

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
                event: &run_finished,
                result: run_finished.result.as_ref(),
                outcome: run_finished.outcome.as_ref(),
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
    }

    #[tokio::test]
    async fn default_text_hooks_return_ok() {
        let subscriber = DefaultSubscriber;
        let run = run_context();
        let start = text_start_event();
        let content = text_content_event();
        let end = text_end_event();
        let chunk = text_chunk_event();

        assert!(subscriber
            .on_text_message_start_event(&EventContext {
                run: &run,
                event: &start,
            })
            .await
            .is_ok());
        assert!(subscriber
            .on_text_message_content_event(&TextContentContext {
                run: &run,
                event: &content,
                text_message_buffer: "hello",
            })
            .await
            .is_ok());
        assert!(subscriber
            .on_text_message_end_event(&TextEndContext {
                run: &run,
                event: &end,
                text_message_buffer: "hello world",
            })
            .await
            .is_ok());
        assert!(subscriber
            .on_text_message_chunk(&EventContext {
                run: &run,
                event: &chunk,
            })
            .await
            .is_ok());
    }

    #[tokio::test]
    async fn default_tool_hooks_return_ok() {
        let subscriber = DefaultSubscriber;
        let run = run_context();
        let start = tool_start_event();
        let args = tool_args_event();
        let end = tool_end_event();
        let chunk = tool_chunk_event();
        let result = tool_result_event();
        let partial = json!("{\"q\":\"ru\"}");
        let final_args = json!({"q": "rust"});

        assert!(subscriber
            .on_tool_call_start_event(&EventContext {
                run: &run,
                event: &start,
            })
            .await
            .is_ok());
        assert!(subscriber
            .on_tool_call_args_event(&ToolCallArgsContext {
                run: &run,
                event: &args,
                tool_call_buffer: "{\"q\":\"ru",
                tool_call_name: "search",
                partial_tool_call_args: &partial,
            })
            .await
            .is_ok());
        assert!(subscriber
            .on_tool_call_end_event(&ToolCallEndContext {
                run: &run,
                event: &end,
                tool_call_name: "search",
                tool_call_args: &final_args,
            })
            .await
            .is_ok());
        assert!(subscriber
            .on_tool_call_chunk(&EventContext {
                run: &run,
                event: &chunk,
            })
            .await
            .is_ok());
        assert!(subscriber
            .on_tool_call_result_event(&ToolCallResultContext {
                run: &run,
                event: &result,
            })
            .await
            .is_ok());
    }

    #[tokio::test]
    async fn default_reasoning_hooks_return_ok() {
        let subscriber = DefaultSubscriber;
        let run = run_context();
        let started = reasoning_start_event();
        let start = reasoning_message_start_event();
        let content = reasoning_content_event();
        let end = reasoning_message_end_event();
        let chunk = reasoning_chunk_event();
        let finished = reasoning_end_event();
        let encrypted = reasoning_encrypted_value_event();

        assert!(subscriber
            .on_reasoning_started(&EventContext {
                run: &run,
                event: &started,
            })
            .await
            .is_ok());
        assert!(subscriber
            .on_reasoning_start(&EventContext {
                run: &run,
                event: &start,
            })
            .await
            .is_ok());
        assert!(subscriber
            .on_reasoning_content(&ReasoningContentContext {
                run: &run,
                event: &content,
                reasoning_message_buffer: "th",
            })
            .await
            .is_ok());
        assert!(subscriber
            .on_reasoning_end(&ReasoningEndContext {
                run: &run,
                event: &end,
                reasoning_message_buffer: "think",
            })
            .await
            .is_ok());
        assert!(subscriber
            .on_reasoning_chunk(&EventContext {
                run: &run,
                event: &chunk,
            })
            .await
            .is_ok());
        assert!(subscriber
            .on_reasoning_finished(&EventContext {
                run: &run,
                event: &finished,
            })
            .await
            .is_ok());
        assert!(subscriber
            .on_reasoning_encrypted_value(&EventContext {
                run: &run,
                event: &encrypted,
            })
            .await
            .is_ok());
    }

    #[tokio::test]
    async fn default_state_activity_and_misc_hooks_return_ok() {
        let subscriber = DefaultSubscriber;
        let run = run_context();
        let thinking_start = thinking_start_event();
        let thinking_end = thinking_end_event();
        let state_snapshot = state_snapshot_event();
        let state_delta = state_delta_event();
        let messages_snapshot = messages_snapshot_event();
        let activity_snapshot = activity_snapshot_event();
        let activity_delta = activity_delta_event();
        let raw = raw_event();
        let custom = custom_event();
        let finished = RunFinishedEvent {
            outcome: Some(RunFinishedOutcome::Interrupt {
                interrupts: vec![interrupt()],
            }),
            ..run_finished_event()
        };
        let activity = activity_message();

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
                event: &finished,
                interrupts: match finished.outcome.as_ref() {
                    Some(RunFinishedOutcome::Interrupt { interrupts }) => interrupts,
                    _ => &[],
                },
            })
            .await
            .is_ok());
        assert!(subscriber
            .on_event_received(&EventContext {
                run: &run,
                event: &Event::Custom(custom_event()),
            })
            .await
            .is_ok());
    }

    #[tokio::test]
    async fn default_new_message_returns_none() {
        let subscriber = DefaultSubscriber;
        let run = run_context();
        let message = assistant_message("m-2", Some("new"), None);

        let replacement = subscriber
            .on_new_message(&NewMessageContext {
                run: &run,
                message: &message,
            })
            .await
            .expect("default new message hook succeeds");

        assert!(replacement.is_none());
    }

    #[tokio::test]
    async fn default_new_tool_call_returns_none() {
        let subscriber = DefaultSubscriber;
        let run = run_context();
        let call = tool_call();

        let replacement = subscriber
            .on_new_tool_call(&NewToolCallContext {
                run: &run,
                tool_call: &call,
            })
            .await
            .expect("default new tool call hook succeeds");

        assert!(replacement.is_none());
    }

    #[tokio::test]
    async fn custom_new_message_can_replace_message() {
        struct MutatingSubscriber;

        #[async_trait]
        impl AgentSubscriber for MutatingSubscriber {
            async fn on_new_message(
                &self,
                ctx: &NewMessageContext<'_>,
            ) -> std::result::Result<Option<Message>, AgUiError> {
                std::result::Result::Ok(Some(assistant_message(
                    ctx.message.id(),
                    Some("rewritten"),
                    None,
                )))
            }
        }

        let subscriber = MutatingSubscriber;
        let run = run_context();
        let message = assistant_message("m-3", Some("before"), None);

        let replacement = subscriber
            .on_new_message(&NewMessageContext {
                run: &run,
                message: &message,
            })
            .await
            .expect("mutation succeeds")
            .expect("replacement present");

        assert_eq!(replacement, assistant_message("m-3", Some("rewritten"), None));
    }

    #[tokio::test]
    async fn custom_new_tool_call_can_replace_tool_call() {
        struct MutatingSubscriber;

        #[async_trait]
        impl AgentSubscriber for MutatingSubscriber {
            async fn on_new_tool_call(
                &self,
                ctx: &NewToolCallContext<'_>,
            ) -> std::result::Result<Option<ToolCall>, AgUiError> {
                std::result::Result::Ok(Some(ToolCall {
                    function: FunctionCall {
                        name: "lookup".into(),
                        arguments: ctx.tool_call.function.arguments.clone(),
                    },
                    ..ctx.tool_call.clone()
                }))
            }
        }

        let subscriber = MutatingSubscriber;
        let run = run_context();
        let call = tool_call();

        let replacement = subscriber
            .on_new_tool_call(&NewToolCallContext {
                run: &run,
                tool_call: &call,
            })
            .await
            .expect("mutation succeeds")
            .expect("replacement present");

        assert_eq!(replacement.function.name, "lookup");
    }

    #[tokio::test]
    async fn custom_hooks_record_invocations() {
        let subscriber = RecordingSubscriber::default();
        let run = run_context();
        let started = run_started_event();
        let content = text_content_event();
        let end = tool_end_event();
        let custom = custom_event();
        let partial = json!({"q": "ru"});
        let args = json!({"q": "rust"});
        let message = assistant_message("m-2", Some("x"), None);
        let call = tool_call();

        subscriber
            .on_run_started_event(&EventContext {
                run: &run,
                event: &started,
            })
            .await
            .expect("run started ok");
        subscriber
            .on_text_message_content_event(&TextContentContext {
                run: &run,
                event: &content,
                text_message_buffer: "hello",
            })
            .await
            .expect("text content ok");
        subscriber
            .on_tool_call_args_event(&ToolCallArgsContext {
                run: &run,
                event: &tool_args_event(),
                tool_call_buffer: "{\"q\":\"ru",
                tool_call_name: "search",
                partial_tool_call_args: &partial,
            })
            .await
            .expect("tool args ok");
        subscriber
            .on_tool_call_end_event(&ToolCallEndContext {
                run: &run,
                event: &end,
                tool_call_name: "search",
                tool_call_args: &args,
            })
            .await
            .expect("tool end ok");
        subscriber
            .on_custom_event(&EventContext {
                run: &run,
                event: &custom,
            })
            .await
            .expect("custom ok");
        assert!(subscriber
            .on_new_message(&NewMessageContext {
                run: &run,
                message: &message,
            })
            .await
            .expect("new message ok")
            .is_some());
        assert!(subscriber
            .on_new_tool_call(&NewToolCallContext {
                run: &run,
                tool_call: &call,
            })
            .await
            .expect("new tool call ok")
            .is_some());

        assert_eq!(
            subscriber.calls(),
            vec![
                "run_started",
                "text_content",
                "tool_end",
                "custom",
                "new_message",
                "new_tool_call"
            ]
        );
    }

    #[tokio::test]
    async fn run_failed_event_can_return_error() {
        struct FailingSubscriber;

        #[async_trait]
        impl AgentSubscriber for FailingSubscriber {
            async fn on_run_failed_event(
                &self,
                ctx: &RunFailedContext<'_>,
            ) -> std::result::Result<(), AgUiError> {
                std::result::Result::Err(AgUiError::other(format!("failed:{}", ctx.error)))
            }
        }

        let subscriber = FailingSubscriber;
        let run = run_context();
        let error = AgUiError::other("downstream");
        let result = subscriber
            .on_run_failed_event(&RunFailedContext {
                run: &run,
                error: &error,
            })
            .await;

        assert!(matches!(result, std::result::Result::Err(AgUiError::Other(_))));
    }

    #[tokio::test]
    async fn interrupt_context_exposes_interrupts() {
        struct InterruptSubscriber;

        #[async_trait]
        impl AgentSubscriber for InterruptSubscriber {
            async fn on_interrupt(
                &self,
                ctx: &InterruptContext<'_>,
            ) -> std::result::Result<(), AgUiError> {
                assert_eq!(ctx.interrupts.len(), 1);
                assert_eq!(ctx.interrupts[0].id, "interrupt-1");
                std::result::Result::Ok(())
            }
        }

        let subscriber = InterruptSubscriber;
        let run = run_context();
        let finished = RunFinishedEvent {
            outcome: Some(RunFinishedOutcome::Interrupt {
                interrupts: vec![interrupt()],
            }),
            ..run_finished_event()
        };
        let interrupts = match finished.outcome.as_ref() {
            Some(RunFinishedOutcome::Interrupt { interrupts }) => interrupts.as_slice(),
            _ => &[],
        };

        assert!(subscriber
            .on_interrupt(&InterruptContext {
                run: &run,
                event: &finished,
                interrupts,
            })
            .await
            .is_ok());
    }
}
