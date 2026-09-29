use agui_rs_core::{AgUiError, Event};
use async_stream::stream;
use futures::{Stream, StreamExt};
use std::collections::HashSet;

type VerifyResult<T> = std::result::Result<T, AgUiError>;

#[derive(Debug, Default)]
struct VerifierState {
    first_event_received: bool,
    run_started: bool,
    run_finished: bool,
    active_text_message_id: Option<String>,
    active_tool_call_id: Option<String>,
    active_tool_call_parent_message_id: Option<String>,
    // Reasoning has TWO bracketed entities, not one. A SPAN is opened by
    // REASONING_START and closed by REASONING_END; a reasoning MESSAGE is
    // opened by REASONING_MESSAGE_START and closed by REASONING_MESSAGE_END
    // (upstream `verify/verify.ts:22-38`). The specification says the span's
    // identifier "namespaces nothing" and the messages inside carry their own
    // ids, so the two are tracked separately — which also means one id may
    // legitimately name both. Reasoning was the one streaming entity whose
    // open/close discipline went unverified: a content event with no opener, a
    // message never closed, and a span closed without being opened all passed.
    active_reasoning_spans: HashSet<String>,
    active_reasoning_messages: HashSet<String>,
    run_errored: bool,
    active_step_names: HashSet<String>,
}

pub fn verify_events<S>(stream: S) -> impl Stream<Item = VerifyResult<Event>>
where
    S: Stream<Item = VerifyResult<Event>> + Send + 'static,
{
    stream! {
        let mut stream = stream.boxed();
        let mut state = VerifierState::default();

        while let Some(item) = stream.next().await {
            let event = match item {
                Ok(event) => event,
                Err(err) => {
                    yield Err(err);
                    return;
                }
            };

            if let Err(err) = state.validate_event(&event) {
                yield Err(err);
                return;
            }

            yield Ok(event);
        }
    }
}

impl VerifierState {
    /// Resets per-run state so a new `RUN_STARTED` can begin a fresh run after a
    /// previous `RUN_FINISHED`. Mirrors `resetRunState()` in the TypeScript
    /// `verifyEvents`. Note: `run_errored` is intentionally NOT reset — once a
    /// run errors, the whole stream is permanently terminal.
    fn reset_run_state(&mut self) {
        self.run_finished = false;
        self.run_started = true;
        self.clear_active_state();
    }

    fn validate_event(&mut self, event: &Event) -> VerifyResult<()> {
        // RUN_ERROR is permanently terminal: nothing (not even a new run) may
        // follow it.
        if self.run_errored {
            return Err(AgUiError::validation(format!(
                "Cannot send event type '{}': The run has already errored with 'RUN_ERROR'. No further events can be sent.",
                event_name(event)
            )));
        }

        // After RUN_FINISHED only RUN_ERROR or a new RUN_STARTED may follow.
        if self.run_finished
            && !matches!(event, Event::RunError(_))
            && !matches!(event, Event::RunStarted(_))
        {
            return Err(AgUiError::validation(format!(
                "Cannot send event type '{}': The run has already finished with 'RUN_FINISHED'. Start a new run with 'RUN_STARTED'.",
                event_name(event)
            )));
        }

        // First-event requirement: must be RUN_STARTED or RUN_ERROR.
        if !self.first_event_received {
            self.first_event_received = true;
            if !matches!(event, Event::RunStarted(_) | Event::RunError(_)) {
                return Err(AgUiError::validation("First event must be 'RUN_STARTED'"));
            }
        } else if matches!(event, Event::RunStarted(_)) {
            // A RUN_STARTED mid-stream is only valid as the start of a new run
            // after the previous one finished.
            if self.run_started && !self.run_finished {
                return Err(AgUiError::validation(
                    "Cannot send 'RUN_STARTED' while a run is still active. The previous run must be finished with 'RUN_FINISHED' before starting a new run.",
                ));
            }
            if self.run_finished {
                self.reset_run_state();
            }
        }

        match event {
            Event::RunStarted(_) => {
                self.run_started = true;
                Ok(())
            }
            Event::RunFinished(_) => self.finish_run(),
            Event::RunError(_) => {
                self.run_errored = true;
                self.clear_active_state();
                Ok(())
            }
            Event::TextMessageStart(event) => self.start_text_message(&event.message_id),
            Event::TextMessageContent(event) => self.text_message_content(&event.message_id),
            Event::TextMessageChunk(event) => self.text_message_chunk(event.message_id.as_deref()),
            Event::TextMessageEnd(event) => self.end_text_message(&event.message_id),
            Event::ToolCallStart(event) => {
                self.start_tool_call(&event.tool_call_id, event.parent_message_id.as_deref())
            }
            Event::ToolCallArgs(event) => self.tool_call_args(&event.tool_call_id),
            Event::ToolCallChunk(event) => self.tool_call_chunk(
                event.tool_call_id.as_deref(),
                event.parent_message_id.as_deref(),
            ),
            Event::ToolCallEnd(event) => self.end_tool_call(&event.tool_call_id),
            Event::ReasoningStart(event) => self.start_reasoning(&event.message_id),
            Event::ReasoningMessageStart(event) => self.start_reasoning_message(&event.message_id),
            Event::ReasoningMessageContent(event) => {
                self.continue_reasoning_message(&event.message_id, "REASONING_MESSAGE_CONTENT")
            }
            Event::ReasoningMessageChunk(event) => {
                self.reasoning_chunk(event.message_id.as_deref())
            }
            Event::ReasoningMessageEnd(event) => self.end_reasoning_message(&event.message_id),
            Event::ReasoningEnd(event) => self.end_reasoning(&event.message_id),
            Event::StepStarted(event) => self.start_step(&event.step_name),
            Event::StepFinished(event) => self.finish_step(&event.step_name),
            // ponytail: subagent events are pass-through (outcome/interruptIds
            // attribution already enforced by core types); ordering follows the
            // run terminal rules above.
            Event::SubagentStarted(_) | Event::SubagentFinished(_) | Event::SubagentError(_) => {
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn finish_run(&mut self) -> VerifyResult<()> {
        if !self.active_step_names.is_empty() {
            let mut steps = self.active_step_names.iter().cloned().collect::<Vec<_>>();
            steps.sort();
            return Err(AgUiError::validation(format!(
                "Cannot send 'RUN_FINISHED' while steps are still active: {}",
                steps.join(", ")
            )));
        }

        if let Some(message_id) = self.active_text_message_id.as_deref() {
            return Err(AgUiError::validation(format!(
                "Cannot send 'RUN_FINISHED' while text messages are still active: {message_id}"
            )));
        }

        if !self.active_reasoning_messages.is_empty() {
            let mut open = self
                .active_reasoning_messages
                .iter()
                .cloned()
                .collect::<Vec<_>>();
            open.sort();
            return Err(AgUiError::validation(format!(
                "Cannot send 'RUN_FINISHED' while reasoning messages are still active: {}",
                open.join(", ")
            )));
        }

        if !self.active_reasoning_spans.is_empty() {
            let mut open = self
                .active_reasoning_spans
                .iter()
                .cloned()
                .collect::<Vec<_>>();
            open.sort();
            return Err(AgUiError::validation(format!(
                "Cannot send 'RUN_FINISHED' while reasoning spans are still active: {}",
                open.join(", ")
            )));
        }

        if let Some(tool_call_id) = self.active_tool_call_id.as_deref() {
            return Err(AgUiError::validation(format!(
                "Cannot send 'RUN_FINISHED' while tool calls are still active: {tool_call_id}"
            )));
        }

        self.run_finished = true;
        Ok(())
    }

    fn start_text_message(&mut self, message_id: &str) -> VerifyResult<()> {
        if let Some(active) = self.active_text_message_id.as_deref() {
            return Err(AgUiError::validation(format!(
                "Cannot send 'TEXT_MESSAGE_START' event: A text message with ID '{}' is already in progress. Complete it with 'TEXT_MESSAGE_END' first.",
                active
            )));
        }

        self.active_text_message_id = Some(message_id.to_owned());
        Ok(())
    }

    fn text_message_content(&self, message_id: &str) -> VerifyResult<()> {
        match self.active_text_message_id.as_deref() {
            Some(active) if active == message_id => Ok(()),
            _ => Err(AgUiError::validation(format!(
                "Cannot send 'TEXT_MESSAGE_CONTENT' event: No active text message found with ID '{}'. Start a text message with 'TEXT_MESSAGE_START' first.",
                message_id
            ))),
        }
    }

    fn text_message_chunk(&self, message_id: Option<&str>) -> VerifyResult<()> {
        match (self.active_text_message_id.as_deref(), message_id) {
            (Some(active), Some(message_id)) if active == message_id => Ok(()),
            (Some(_), None) => Ok(()),
            (Some(active), Some(_)) => Err(AgUiError::validation(format!(
                "Cannot send 'TEXT_MESSAGE_CHUNK' event: The active text message ID is '{}'.",
                active
            ))),
            (_, Some(message_id)) => Err(AgUiError::validation(format!(
                "Cannot send 'TEXT_MESSAGE_CHUNK' event: No active text message found with ID '{}'. Start a text message with 'TEXT_MESSAGE_START' first.",
                message_id
            ))),
            (None, None) => Err(AgUiError::validation(
                "Cannot send 'TEXT_MESSAGE_CHUNK' event: No active text message found. Start a text message with 'TEXT_MESSAGE_START' first.",
            )),
        }
    }

    fn end_text_message(&mut self, message_id: &str) -> VerifyResult<()> {
        match self.active_text_message_id.as_deref() {
            Some(active) if active == message_id => {
                self.active_text_message_id = None;
                Ok(())
            }
            _ => Err(AgUiError::validation(format!(
                "Cannot send 'TEXT_MESSAGE_END' event: No active text message found with ID '{}'. A 'TEXT_MESSAGE_START' event must be sent first.",
                message_id
            ))),
        }
    }

    fn start_tool_call(
        &mut self,
        tool_call_id: &str,
        parent_message_id: Option<&str>,
    ) -> VerifyResult<()> {
        if let Some(active) = self.active_tool_call_id.as_deref() {
            return Err(AgUiError::validation(format!(
                "Cannot send 'TOOL_CALL_START' event: A tool call with ID '{}' is already in progress. Complete it with 'TOOL_CALL_END' first.",
                active
            )));
        }

        if let Some(parent_message_id) = parent_message_id {
            match self.active_text_message_id.as_deref() {
                Some(active) if active == parent_message_id => {}
                Some(active) => {
                    return Err(AgUiError::validation(format!(
                        "Cannot send 'TOOL_CALL_START' event: Parent message ID '{}' must match the active text message ID '{}'.",
                        parent_message_id, active
                    )));
                }
                None => {
                    return Err(AgUiError::validation(format!(
                        "Cannot send 'TOOL_CALL_START' event: Parent message ID '{}' does not reference an active text message. Start a text message with 'TEXT_MESSAGE_START' first.",
                        parent_message_id
                    )));
                }
            }
        }

        self.active_tool_call_id = Some(tool_call_id.to_owned());
        self.active_tool_call_parent_message_id = parent_message_id.map(ToOwned::to_owned);
        Ok(())
    }

    fn tool_call_args(&self, tool_call_id: &str) -> VerifyResult<()> {
        match self.active_tool_call_id.as_deref() {
            Some(active) if active == tool_call_id => Ok(()),
            _ => Err(AgUiError::validation(format!(
                "Cannot send 'TOOL_CALL_ARGS' event: No active tool call found with ID '{}'. Start a tool call with 'TOOL_CALL_START' first.",
                tool_call_id
            ))),
        }
    }

    fn tool_call_chunk(
        &self,
        tool_call_id: Option<&str>,
        parent_message_id: Option<&str>,
    ) -> VerifyResult<()> {
        match (self.active_tool_call_id.as_deref(), tool_call_id) {
            (Some(active), Some(tool_call_id)) if active != tool_call_id => {
                return Err(AgUiError::validation(format!(
                    "Cannot send 'TOOL_CALL_CHUNK' event: No active tool call found with ID '{}'. Start a tool call with 'TOOL_CALL_START' first.",
                    tool_call_id
                )));
            }
            (None, Some(tool_call_id)) => {
                return Err(AgUiError::validation(format!(
                    "Cannot send 'TOOL_CALL_CHUNK' event: No active tool call found with ID '{}'. Start a tool call with 'TOOL_CALL_START' first.",
                    tool_call_id
                )));
            }
            (None, None) => {
                return Err(AgUiError::validation(
                    "Cannot send 'TOOL_CALL_CHUNK' event: No active tool call found. Start a tool call with 'TOOL_CALL_START' first.",
                ));
            }
            _ => {}
        }

        if let Some(parent_message_id) = parent_message_id {
            match self.active_tool_call_parent_message_id.as_deref() {
                Some(active) if active == parent_message_id => {}
                Some(active) => {
                    return Err(AgUiError::validation(format!(
                        "Cannot send 'TOOL_CALL_CHUNK' event: Parent message ID '{}' must match the active tool call parent message ID '{}'.",
                        parent_message_id, active
                    )));
                }
                None => {
                    return Err(AgUiError::validation(format!(
                        "Cannot send 'TOOL_CALL_CHUNK' event: Parent message ID '{}' is not valid for the active tool call.",
                        parent_message_id
                    )));
                }
            }
        }

        Ok(())
    }

    fn end_tool_call(&mut self, tool_call_id: &str) -> VerifyResult<()> {
        match self.active_tool_call_id.as_deref() {
            Some(active) if active == tool_call_id => {
                self.active_tool_call_id = None;
                self.active_tool_call_parent_message_id = None;
                Ok(())
            }
            _ => Err(AgUiError::validation(format!(
                "Cannot send 'TOOL_CALL_END' event: No active tool call found with ID '{}'. A 'TOOL_CALL_START' event must be sent first.",
                tool_call_id
            ))),
        }
    }

    /// Opens a reasoning SPAN. Checked against the opener's OWN set: a span
    /// and the message inside it may share an id, so one set for both would
    /// reject the canonical shape (upstream `verify/verify.ts:689-732`).
    fn start_reasoning(&mut self, message_id: &str) -> VerifyResult<()> {
        if !self.active_reasoning_spans.insert(message_id.to_owned()) {
            return Err(AgUiError::validation(format!(
                "Cannot send 'REASONING_START' event: A reasoning span with ID '{message_id}' is already in progress. Complete it with 'REASONING_END' first."
            )));
        }
        Ok(())
    }

    /// Opens a reasoning MESSAGE. It requires no enclosing span — the span's
    /// identifier "namespaces nothing" — so only a duplicate open is rejected.
    fn start_reasoning_message(&mut self, message_id: &str) -> VerifyResult<()> {
        if !self.active_reasoning_messages.insert(message_id.to_owned()) {
            return Err(AgUiError::validation(format!(
                "Cannot send 'REASONING_MESSAGE_START' event: A reasoning message with ID '{message_id}' is already in progress. Complete it with 'REASONING_MESSAGE_END' first."
            )));
        }
        Ok(())
    }

    /// A continuation must name something that is open. Content does not close;
    /// only the matching `*_END` drops the OPEN flag.
    fn continue_reasoning_message(&self, message_id: &str, event_name: &str) -> VerifyResult<()> {
        if self.active_reasoning_messages.contains(message_id) {
            return Ok(());
        }
        Err(AgUiError::validation(format!(
            "Cannot send '{event_name}' event: No active reasoning message found with ID '{message_id}'. Start a reasoning message with 'REASONING_MESSAGE_START' first."
        )))
    }

    fn end_reasoning_message(&mut self, message_id: &str) -> VerifyResult<()> {
        if !self.active_reasoning_messages.remove(message_id) {
            return Err(AgUiError::validation(format!(
                "Cannot send 'REASONING_MESSAGE_END' event: No active reasoning message found with ID '{message_id}'. A 'REASONING_MESSAGE_START' event must be sent first."
            )));
        }
        Ok(())
    }

    /// Closes a reasoning SPAN, independently of any message it bracketed: a
    /// span may end while its message is still open, and the message's own
    /// `REASONING_MESSAGE_END` remains owed (upstream `verify/verify.ts:733-771`).
    fn end_reasoning(&mut self, message_id: &str) -> VerifyResult<()> {
        if !self.active_reasoning_spans.remove(message_id) {
            return Err(AgUiError::validation(format!(
                "Cannot send 'REASONING_END' event: No active reasoning span found with ID '{message_id}'. A 'REASONING_START' event must be sent first."
            )));
        }
        Ok(())
    }

    /// A chunk is a continuation like content, so it must name an open message.
    ///
    /// ponytail: upstream has no `REASONING_MESSAGE_CHUNK` case at all
    /// (`verify/verify.ts` switches on CONTENT/END, never CHUNK) — chunks are
    /// expanded upstream of verification, so it is unobservable there. We keep
    /// the check, rebased on the message set.
    fn reasoning_chunk(&self, message_id: Option<&str>) -> VerifyResult<()> {
        match message_id {
            Some(message_id) => self.continue_reasoning_message(message_id, "REASONING_MESSAGE_CHUNK"),
            None => Err(AgUiError::validation(
                "Cannot send 'REASONING_MESSAGE_CHUNK' event: No active reasoning message found. Start a reasoning message with 'REASONING_MESSAGE_START' first.",
            )),
        }
    }

    fn start_step(&mut self, step_name: &str) -> VerifyResult<()> {
        if !self.active_step_names.insert(step_name.to_owned()) {
            return Err(AgUiError::validation(format!(
                "Step \"{}\" is already active for 'STEP_STARTED'",
                step_name
            )));
        }

        Ok(())
    }

    fn finish_step(&mut self, step_name: &str) -> VerifyResult<()> {
        if !self.active_step_names.remove(step_name) {
            return Err(AgUiError::validation(format!(
                "Cannot send 'STEP_FINISHED' for step \"{}\" that was not started",
                step_name
            )));
        }

        Ok(())
    }

    fn clear_active_state(&mut self) {
        self.active_text_message_id = None;
        self.active_tool_call_id = None;
        self.active_tool_call_parent_message_id = None;
        self.active_reasoning_spans.clear();
        self.active_reasoning_messages.clear();
        self.active_step_names.clear();
    }
}

fn event_name(event: &Event) -> &'static str {
    match event {
        Event::TextMessageStart(_) => "TEXT_MESSAGE_START",
        Event::TextMessageContent(_) => "TEXT_MESSAGE_CONTENT",
        Event::TextMessageEnd(_) => "TEXT_MESSAGE_END",
        Event::TextMessageChunk(_) => "TEXT_MESSAGE_CHUNK",
        Event::ToolCallStart(_) => "TOOL_CALL_START",
        Event::ToolCallArgs(_) => "TOOL_CALL_ARGS",
        Event::ToolCallEnd(_) => "TOOL_CALL_END",
        Event::ToolCallChunk(_) => "TOOL_CALL_CHUNK",
        Event::ToolCallResult(_) => "TOOL_CALL_RESULT",
        Event::StateSnapshot(_) => "STATE_SNAPSHOT",
        Event::StateDelta(_) => "STATE_DELTA",
        Event::MessagesSnapshot(_) => "MESSAGES_SNAPSHOT",
        Event::ActivitySnapshot(_) => "ACTIVITY_SNAPSHOT",
        Event::ActivityDelta(_) => "ACTIVITY_DELTA",
        Event::Raw(_) => "RAW",
        Event::Custom(_) => "CUSTOM",
        Event::RunStarted(_) => "RUN_STARTED",
        Event::RunFinished(_) => "RUN_FINISHED",
        Event::RunError(_) => "RUN_ERROR",
        Event::StepStarted(_) => "STEP_STARTED",
        Event::StepFinished(_) => "STEP_FINISHED",
        Event::ReasoningStart(_) => "REASONING_START",
        Event::ReasoningMessageStart(_) => "REASONING_MESSAGE_START",
        Event::ReasoningMessageContent(_) => "REASONING_MESSAGE_CONTENT",
        Event::ReasoningMessageEnd(_) => "REASONING_MESSAGE_END",
        Event::ReasoningMessageChunk(_) => "REASONING_MESSAGE_CHUNK",
        Event::ReasoningEnd(_) => "REASONING_END",
        Event::ReasoningEncryptedValue(_) => "REASONING_ENCRYPTED_VALUE",
        Event::SubagentStarted(_) => "SUBAGENT_STARTED",
        Event::SubagentFinished(_) => "SUBAGENT_FINISHED",
        Event::SubagentError(_) => "SUBAGENT_ERROR",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agui_rs_core::AttributableFields;
    use agui_rs_core::{
        factory, BaseEventFields, Event, ReasoningEndEvent, ReasoningMessageChunkEvent,
        ReasoningMessageContentEvent, ReasoningMessageEndEvent, ReasoningMessageRole,
        ReasoningMessageStartEvent, ReasoningStartEvent, ToolCallChunkEvent, ToolCallStartEvent,
    };
    use futures::stream;

    async fn collect(events: Vec<Event>) -> Vec<VerifyResult<Event>> {
        verify_events(stream::iter(events.into_iter().map(Ok)))
            .collect::<Vec<_>>()
            .await
    }

    fn assert_validation(result: &VerifyResult<Event>, expected: &str) {
        match result {
            Err(AgUiError::Validation(message)) => assert!(
                message.contains(expected),
                "expected '{expected}' in '{message}'"
            ),
            other => panic!("expected validation error, got {other:?}"),
        }
    }

    fn tool_call_start_with_parent(tool_call_id: &str, parent_message_id: &str) -> Event {
        Event::ToolCallStart(ToolCallStartEvent {
            tool_call_id: tool_call_id.into(),
            tool_call_name: "search".into(),
            parent_message_id: Some(parent_message_id.into()),
            base: BaseEventFields::default(),
            attributable: AttributableFields::default(),
        })
    }

    fn tool_call_chunk(tool_call_id: Option<&str>, parent_message_id: Option<&str>) -> Event {
        Event::ToolCallChunk(ToolCallChunkEvent {
            tool_call_id: tool_call_id.map(str::to_owned),
            tool_call_name: None,
            parent_message_id: parent_message_id.map(str::to_owned),
            delta: Some("{}".into()),
            base: BaseEventFields::default(),
            attributable: AttributableFields::default(),
        })
    }

    fn reasoning_start(message_id: &str) -> Event {
        Event::ReasoningStart(ReasoningStartEvent {
            message_id: message_id.into(),
            base: BaseEventFields::default(),
            attributable: AttributableFields::default(),
        })
    }

    fn reasoning_message_start(message_id: &str) -> Event {
        Event::ReasoningMessageStart(ReasoningMessageStartEvent {
            message_id: message_id.into(),
            role: ReasoningMessageRole::Reasoning,
            attributable: AttributableFields::default(),
            base: BaseEventFields::default(),
        })
    }

    fn reasoning_message_content(message_id: &str, delta: &str) -> Event {
        Event::ReasoningMessageContent(ReasoningMessageContentEvent {
            message_id: message_id.into(),
            delta: delta.into(),
            base: BaseEventFields::default(),
            attributable: AttributableFields::default(),
        })
    }

    fn reasoning_message_chunk(message_id: Option<&str>, delta: &str) -> Event {
        Event::ReasoningMessageChunk(ReasoningMessageChunkEvent {
            message_id: message_id.map(str::to_owned),
            delta: Some(delta.into()),
            base: BaseEventFields::default(),
            attributable: AttributableFields::default(),
        })
    }

    fn reasoning_message_end(message_id: &str) -> Event {
        Event::ReasoningMessageEnd(ReasoningMessageEndEvent {
            message_id: message_id.into(),
            base: BaseEventFields::default(),
            attributable: AttributableFields::default(),
        })
    }

    fn reasoning_end(message_id: &str) -> Event {
        Event::ReasoningEnd(ReasoningEndEvent {
            message_id: message_id.into(),
            base: BaseEventFields::default(),
            attributable: AttributableFields::default(),
        })
    }

    mod run_lifecycle {
        use super::*;

        #[tokio::test]
        async fn rejects_non_run_started_first_event() {
            let items = collect(vec![factory::text_message_start("m1")]).await;
            assert_validation(&items[0], "First event must be 'RUN_STARTED'");
        }

        #[tokio::test]
        async fn rejects_second_run_started_during_active_run() {
            let items = collect(vec![
                factory::run_started("thread", "run"),
                factory::run_started("thread", "run-2"),
            ])
            .await;
            assert!(items[0].is_ok());
            assert_validation(
                &items[1],
                "Cannot send 'RUN_STARTED' while a run is still active",
            );
        }

        #[tokio::test]
        async fn rejects_events_after_run_finished() {
            let items = collect(vec![
                factory::run_started("thread", "run"),
                factory::run_finished("thread", "run"),
                factory::text_message_start("m1"),
            ])
            .await;
            assert_validation(
                &items[2],
                "The run has already finished with 'RUN_FINISHED'",
            );
        }

        #[tokio::test]
        async fn rejects_events_after_run_error() {
            let items = collect(vec![
                factory::run_started("thread", "run"),
                factory::run_error("boom"),
                factory::text_message_start("m1"),
            ])
            .await;
            assert_validation(&items[2], "The run has already errored with 'RUN_ERROR'");
        }

        #[tokio::test]
        async fn accepts_valid_run_lifecycle() {
            let items = collect(vec![
                factory::run_started("thread", "run"),
                factory::text_message_start("m1"),
                factory::text_message_end("m1"),
                factory::run_finished("thread", "run"),
            ])
            .await;
            assert!(items.iter().all(VerifyResult::is_ok));
        }
    }

    mod text_messages {
        use super::*;

        #[tokio::test]
        async fn rejects_content_before_start() {
            let items = collect(vec![
                factory::run_started("thread", "run"),
                factory::text_message_content("m1", "hello"),
            ])
            .await;
            assert_validation(&items[1], "Cannot send 'TEXT_MESSAGE_CONTENT' event: No active text message found with ID 'm1'");
        }

        #[tokio::test]
        async fn rejects_end_before_start() {
            let items = collect(vec![
                factory::run_started("thread", "run"),
                factory::text_message_end("m1"),
            ])
            .await;
            assert_validation(
                &items[1],
                "Cannot send 'TEXT_MESSAGE_END' event: No active text message found with ID 'm1'",
            );
        }

        #[tokio::test]
        async fn rejects_second_text_message_start_while_active() {
            let items = collect(vec![
                factory::run_started("thread", "run"),
                factory::text_message_start("m1"),
                factory::text_message_start("m2"),
            ])
            .await;
            assert_validation(
                &items[2],
                "A text message with ID 'm1' is already in progress",
            );
        }

        #[tokio::test]
        async fn rejects_mismatched_content_id() {
            let items = collect(vec![
                factory::run_started("thread", "run"),
                factory::text_message_start("m1"),
                factory::text_message_content("m2", "hello"),
            ])
            .await;
            assert_validation(&items[2], "No active text message found with ID 'm2'");
        }

        #[tokio::test]
        async fn allows_valid_text_message_sequence() {
            let items = collect(vec![
                factory::run_started("thread", "run"),
                factory::text_message_start("m1"),
                factory::text_message_content("m1", "hello"),
                factory::text_message_content("m1", " world"),
                factory::text_message_end("m1"),
                factory::run_finished("thread", "run"),
            ])
            .await;
            assert!(items.iter().all(VerifyResult::is_ok));
        }
    }

    mod tool_calls {
        use super::*;

        #[tokio::test]
        async fn rejects_args_before_start() {
            let items = collect(vec![
                factory::run_started("thread", "run"),
                factory::tool_call_args("tc1", "{}"),
            ])
            .await;
            assert_validation(
                &items[1],
                "Cannot send 'TOOL_CALL_ARGS' event: No active tool call found with ID 'tc1'",
            );
        }

        #[tokio::test]
        async fn rejects_end_before_start() {
            let items = collect(vec![
                factory::run_started("thread", "run"),
                factory::tool_call_end("tc1"),
            ])
            .await;
            assert_validation(
                &items[1],
                "Cannot send 'TOOL_CALL_END' event: No active tool call found with ID 'tc1'",
            );
        }

        #[tokio::test]
        async fn rejects_tool_call_start_with_non_active_parent_message() {
            let items = collect(vec![
                factory::run_started("thread", "run"),
                tool_call_start_with_parent("tc1", "m1"),
            ])
            .await;
            assert_validation(
                &items[1],
                "Parent message ID 'm1' does not reference an active text message",
            );
        }

        #[tokio::test]
        async fn rejects_second_tool_call_start_while_active() {
            let items = collect(vec![
                factory::run_started("thread", "run"),
                factory::tool_call_start("tc1", "search"),
                factory::tool_call_start("tc2", "search"),
            ])
            .await;
            assert_validation(
                &items[2],
                "A tool call with ID 'tc1' is already in progress",
            );
        }

        #[tokio::test]
        async fn allows_tool_call_sequence_with_parent_message() {
            let items = collect(vec![
                factory::run_started("thread", "run"),
                factory::text_message_start("m1"),
                tool_call_start_with_parent("tc1", "m1"),
                factory::tool_call_args("tc1", "{"),
                tool_call_chunk(Some("tc1"), Some("m1")),
                factory::tool_call_end("tc1"),
                factory::text_message_end("m1"),
                factory::run_finished("thread", "run"),
            ])
            .await;
            assert!(items.iter().all(VerifyResult::is_ok));
        }
    }

    mod reasoning {
        use super::*;

        #[tokio::test]
        async fn allows_reasoning_message_start_without_an_enclosing_span() {
            // The span's identifier "namespaces nothing" and the messages inside
            // carry their own ids, so a message opener requires no span.
            let items = collect(vec![
                factory::run_started("thread", "run"),
                reasoning_message_start("r1"),
                reasoning_message_content("r1", "thinking"),
                reasoning_message_end("r1"),
                factory::run_finished("thread", "run"),
            ])
            .await;
            assert!(items.iter().all(VerifyResult::is_ok));
        }

        #[tokio::test]
        async fn rejects_a_second_reasoning_message_start_with_the_same_id() {
            let items = collect(vec![
                factory::run_started("thread", "run"),
                reasoning_message_start("r1"),
                reasoning_message_start("r1"),
            ])
            .await;
            assert_validation(
                &items[2],
                "A reasoning message with ID 'r1' is already in progress. Complete it with 'REASONING_MESSAGE_END' first.",
            );
        }

        #[tokio::test]
        async fn rejects_a_second_reasoning_start_with_the_same_id() {
            let items = collect(vec![
                factory::run_started("thread", "run"),
                reasoning_start("r1"),
                reasoning_start("r1"),
            ])
            .await;
            assert_validation(
                &items[2],
                "A reasoning span with ID 'r1' is already in progress. Complete it with 'REASONING_END' first.",
            );
        }

        #[tokio::test]
        async fn rejects_reasoning_content_before_reasoning_message_start() {
            let items = collect(vec![
                factory::run_started("thread", "run"),
                reasoning_start("r1"),
                reasoning_message_content("r1", "thinking"),
            ])
            .await;
            assert_validation(&items[2], "No active reasoning message found with ID 'r1'");
        }

        #[tokio::test]
        async fn rejects_reasoning_message_end_with_mismatched_id() {
            let items = collect(vec![
                factory::run_started("thread", "run"),
                reasoning_start("r1"),
                reasoning_message_start("r1"),
                reasoning_message_end("r2"),
            ])
            .await;
            assert_validation(&items[3], "No active reasoning message found with ID 'r2'");
        }

        #[tokio::test]
        async fn closes_only_its_own_entity() {
            // REASONING_END drops the SPAN's open flag and nothing else; the
            // message's own REASONING_MESSAGE_END is still owed.
            let items = collect(vec![
                factory::run_started("thread", "run"),
                reasoning_start("span"),
                reasoning_message_start("msg"),
                reasoning_end("span"),
                reasoning_message_end("msg"),
                factory::run_finished("thread", "run"),
            ])
            .await;
            assert!(items.iter().all(VerifyResult::is_ok));
        }

        #[tokio::test]
        async fn rejects_reasoning_end_for_a_span_nothing_opened() {
            let items = collect(vec![
                factory::run_started("thread", "run"),
                reasoning_end("r1"),
            ])
            .await;
            assert_validation(
                &items[1],
                "Cannot send 'REASONING_END' event: No active reasoning span found with ID 'r1'.",
            );
        }

        #[tokio::test]
        async fn lets_one_id_name_both_a_span_and_its_message() {
            let items = collect(vec![
                factory::run_started("thread", "run"),
                reasoning_start("r1"),
                reasoning_message_start("r1"),
                reasoning_message_content("r1", "a"),
                reasoning_message_end("r1"),
                reasoning_end("r1"),
                factory::run_finished("thread", "run"),
            ])
            .await;
            assert!(items.iter().all(VerifyResult::is_ok));
        }

        #[tokio::test]
        async fn allows_complete_reasoning_sequence() {
            let items = collect(vec![
                factory::run_started("thread", "run"),
                reasoning_start("r1"),
                reasoning_message_start("r1"),
                reasoning_message_content("r1", "a"),
                reasoning_message_chunk(Some("r1"), "b"),
                reasoning_message_end("r1"),
                reasoning_end("r1"),
                factory::run_finished("thread", "run"),
            ])
            .await;
            assert!(items.iter().all(VerifyResult::is_ok));
        }
    }

    mod steps {
        use super::*;

        #[tokio::test]
        async fn rejects_duplicate_step_start() {
            let items = collect(vec![
                factory::run_started("thread", "run"),
                factory::step_started("plan"),
                factory::step_started("plan"),
            ])
            .await;
            assert_validation(
                &items[2],
                "Step \"plan\" is already active for 'STEP_STARTED'",
            );
        }

        #[tokio::test]
        async fn rejects_step_finish_without_start() {
            let items = collect(vec![
                factory::run_started("thread", "run"),
                factory::step_finished("plan"),
            ])
            .await;
            assert_validation(
                &items[1],
                "Cannot send 'STEP_FINISHED' for step \"plan\" that was not started",
            );
        }

        #[tokio::test]
        async fn rejects_run_finished_with_active_step() {
            let items = collect(vec![
                factory::run_started("thread", "run"),
                factory::step_started("plan"),
                factory::run_finished("thread", "run"),
            ])
            .await;
            assert_validation(
                &items[2],
                "Cannot send 'RUN_FINISHED' while steps are still active: plan",
            );
        }

        #[tokio::test]
        async fn allows_multiple_distinct_steps() {
            let items = collect(vec![
                factory::run_started("thread", "run"),
                factory::step_started("plan"),
                factory::step_started("execute"),
                factory::step_finished("execute"),
                factory::step_finished("plan"),
                factory::run_finished("thread", "run"),
            ])
            .await;
            assert!(items.iter().all(VerifyResult::is_ok));
        }

        #[tokio::test]
        async fn allows_reusing_step_name_after_finish() {
            let items = collect(vec![
                factory::run_started("thread", "run"),
                factory::step_started("plan"),
                factory::step_finished("plan"),
                factory::step_started("plan"),
                factory::step_finished("plan"),
                factory::run_finished("thread", "run"),
            ])
            .await;
            assert!(items.iter().all(VerifyResult::is_ok));
        }
    }

    mod sequencing {
        use super::*;

        #[tokio::test]
        async fn rejects_run_finished_with_active_text_message() {
            let items = collect(vec![
                factory::run_started("thread", "run"),
                factory::text_message_start("m1"),
                factory::run_finished("thread", "run"),
            ])
            .await;
            assert_validation(
                &items[2],
                "Cannot send 'RUN_FINISHED' while text messages are still active: m1",
            );
        }

        #[tokio::test]
        async fn rejects_run_finished_with_active_tool_call() {
            let items = collect(vec![
                factory::run_started("thread", "run"),
                factory::tool_call_start("tc1", "search"),
                factory::run_finished("thread", "run"),
            ])
            .await;
            assert_validation(
                &items[2],
                "Cannot send 'RUN_FINISHED' while tool calls are still active: tc1",
            );
        }

        #[tokio::test]
        async fn rejects_run_finished_with_active_reasoning_span() {
            let items = collect(vec![
                factory::run_started("thread", "run"),
                reasoning_start("r1"),
                factory::run_finished("thread", "run"),
            ])
            .await;
            assert_validation(
                &items[2],
                "Cannot send 'RUN_FINISHED' while reasoning spans are still active: r1",
            );
        }

        #[tokio::test]
        async fn rejects_run_finished_with_active_reasoning_message() {
            let items = collect(vec![
                factory::run_started("thread", "run"),
                reasoning_start("r1"),
                reasoning_message_start("r1"),
                reasoning_end("r1"),
                factory::run_finished("thread", "run"),
            ])
            .await;
            // The span closed; the message inside it did not, and the two are
            // tracked separately.
            assert_validation(
                &items[4],
                "Cannot send 'RUN_FINISHED' while reasoning messages are still active: r1",
            );
        }

        #[tokio::test]
        async fn rejects_mismatched_tool_call_chunk_id() {
            let items = collect(vec![
                factory::run_started("thread", "run"),
                factory::tool_call_start("tc1", "search"),
                tool_call_chunk(Some("tc2"), None),
            ])
            .await;
            assert_validation(
                &items[2],
                "Cannot send 'TOOL_CALL_CHUNK' event: No active tool call found with ID 'tc2'",
            );
        }

        #[tokio::test]
        async fn allows_mixed_full_sequence() {
            let items = collect(vec![
                factory::run_started("thread", "run"),
                factory::step_started("plan"),
                factory::text_message_start("m1"),
                factory::text_message_content("m1", "Preparing"),
                tool_call_start_with_parent("tc1", "m1"),
                factory::tool_call_args("tc1", "{}"),
                factory::tool_call_end("tc1"),
                factory::text_message_end("m1"),
                reasoning_start("r1"),
                reasoning_message_start("r1"),
                reasoning_message_content("r1", "thought"),
                reasoning_message_end("r1"),
                reasoning_end("r1"),
                factory::step_finished("plan"),
                factory::run_finished("thread", "run"),
            ])
            .await;
            assert_eq!(items.len(), 15);
            assert!(items.iter().all(VerifyResult::is_ok));
        }
    }
}
