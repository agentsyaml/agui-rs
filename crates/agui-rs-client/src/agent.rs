use crate::apply::default_apply_events;
use crate::chunks::expand_chunks;
use crate::interrupts::ensure_resume_covers;
use crate::middleware::{MiddlewareChain, MiddlewareInput};
use crate::subscriber::{
    ActivityDeltaContext, ActivitySnapshotContext, AgentSubscriber, EventContext, InterruptContext,
    NewMessageContext, NewToolCallContext, ReasoningContentContext, ReasoningEndContext,
    RunContext, RunFinishedContext, TextContentContext, TextEndContext, ToolCallArgsContext,
    ToolCallEndContext, ToolCallResultContext,
};
use crate::verify::verify_events;
use agui_rs_core::{
    AgUiError, Context, Event, Interrupt, Message, Result, RunAgentInput, RunFinishedOutcome,
    State, Tool, ToolCall,
};
use async_trait::async_trait;
use futures::{future::BoxFuture, stream::BoxStream, StreamExt};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::Notify;

pub type EventStream = BoxStream<'static, Result<Event>>;

#[derive(Debug, Default)]
struct AbortState {
    aborted: AtomicBool,
    notify: Notify,
}

#[derive(Debug, Clone, Default)]
pub struct AbortHandle {
    inner: Arc<AbortState>,
}

impl AbortHandle {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn abort(&self) {
        if !self.inner.aborted.swap(true, Ordering::SeqCst) {
            self.inner.notify.notify_waiters();
        }
    }

    pub fn is_aborted(&self) -> bool {
        self.inner.aborted.load(Ordering::SeqCst)
    }

    pub(crate) async fn cancelled(&self) {
        if self.is_aborted() {
            return;
        }

        let notified = self.inner.notify.notified();
        if self.is_aborted() {
            return;
        }
        notified.await;
    }
}

pub(crate) fn abortable_event_stream(mut stream: EventStream, abort: AbortHandle) -> EventStream {
    Box::pin(async_stream::stream! {
        loop {
            if abort.is_aborted() {
                yield Err(AgUiError::Cancelled);
                break;
            }

            let next_item = stream.next();
            tokio::pin!(next_item);

            let cancelled = abort.cancelled();
            tokio::pin!(cancelled);

            match tokio::select! {
                _ = &mut cancelled => Some(Err(AgUiError::Cancelled)),
                item = &mut next_item => item,
            } {
                Some(item) => yield item,
                None => break,
            }
        }
    })
}

#[async_trait]
pub trait Agent: Send + Sync {
    async fn run(&self, input: RunAgentInput) -> Result<EventStream>;

    async fn run_cancellable(
        &self,
        input: RunAgentInput,
        abort: AbortHandle,
    ) -> Result<EventStream> {
        if abort.is_aborted() {
            return Err(AgUiError::Cancelled);
        }

        Ok(abortable_event_stream(self.run(input).await?, abort))
    }
}

#[derive(Debug, Clone, Default)]
pub struct AgentConfig {
    pub agent_id: Option<String>,
    pub description: Option<String>,
    pub thread_id: Option<String>,
    pub initial_messages: Vec<Message>,
    pub initial_state: State,
}

#[derive(Debug, Clone, Default)]
pub struct RunAgentParameters {
    pub run_id: Option<String>,
    pub tools: Vec<Tool>,
    pub context: Vec<Context>,
    pub forwarded_props: Option<Value>,
    pub resume: Vec<agui_rs_core::ResumeEntry>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RunAgentResult {
    pub run_id: String,
    pub thread_id: String,
    pub new_messages: Vec<Message>,
    pub new_state: State,
    pub outcome: Option<RunFinishedOutcome>,
}

pub struct AgentRunner<A: Agent + 'static> {
    agent: Arc<A>,
    middleware: MiddlewareChain,
    subscriber: Option<Arc<dyn AgentSubscriber>>,
    messages: Vec<Message>,
    state: State,
    thread_id: String,
    pending_interrupts: Vec<Interrupt>,
    now_fn: Arc<dyn Fn() -> String + Send + Sync>,
}

/// Default "now" provider used for interrupt-expiry checks.
///
/// `agui-rs-core`/`-client` deliberately avoid a date dependency, so the
/// baseline clock returns an empty string — lexicographically less than any
/// real ISO-8601 timestamp, meaning no interrupt is ever considered expired
/// unless the caller injects a real clock via [`AgentRunner::with_now_fn`].
fn default_now() -> String {
    String::new()
}

impl<A: Agent + 'static> AgentRunner<A> {
    pub fn new(agent: A, config: AgentConfig) -> Self {
        let thread_id = config
            .thread_id
            .clone()
            .unwrap_or_else(|| generate_id("thread"));
        Self {
            agent: Arc::new(agent),
            middleware: MiddlewareChain::new(),
            messages: config.initial_messages.clone(),
            state: config.initial_state.clone(),
            thread_id,
            subscriber: None,
            pending_interrupts: Vec::new(),
            now_fn: Arc::new(default_now),
        }
    }

    pub fn with_subscriber(mut self, sub: Arc<dyn AgentSubscriber>) -> Self {
        self.subscriber = Some(sub);
        self
    }

    pub fn with_middleware(mut self, chain: MiddlewareChain) -> Self {
        self.middleware = chain;
        self
    }

    /// Injects a custom "now" provider (ISO-8601) used to evaluate interrupt
    /// expiry before a resuming run. Defaults to a clock that never expires
    /// interrupts (see [`default_now`]).
    pub fn with_now_fn<F>(mut self, now_fn: F) -> Self
    where
        F: Fn() -> String + Send + Sync + 'static,
    {
        self.now_fn = Arc::new(now_fn);
        self
    }

    /// Interrupts emitted by the most recent run that have not yet been
    /// resolved. Populated when `RUN_FINISHED` carries an interrupt outcome and
    /// cleared when a later run completes successfully — mirroring the
    /// TypeScript `AbstractAgent.pendingInterrupts` field.
    pub fn pending_interrupts(&self) -> &[Interrupt] {
        &self.pending_interrupts
    }

    pub async fn run_agent(&mut self, params: RunAgentParameters) -> Result<RunAgentResult> {
        self.run_agent_cancellable(params, AbortHandle::new()).await
    }

    pub async fn run_agent_cancellable(
        &mut self,
        params: RunAgentParameters,
        abort: AbortHandle,
    ) -> Result<RunAgentResult> {
        let starting_messages_len = self.messages.len();
        let run_id = params.run_id.clone().unwrap_or_else(|| generate_id("run"));

        // Mirror TS AbstractAgent.onInitialize: a run that follows interrupts
        // must address every still-open interrupt via `resume`, and none of
        // them may have expired.
        if !self.pending_interrupts.is_empty() {
            let now_iso = (self.now_fn)();
            ensure_resume_covers(&self.pending_interrupts, &params.resume, &now_iso)?;
        }

        let input = RunAgentInput {
            thread_id: self.thread_id.clone(),
            run_id: run_id.clone(),
            parent_run_id: None,
            state: self.state.clone(),
            messages: self.messages.clone(),
            tools: params.tools.clone(),
            context: params.context.clone(),
            forwarded_props: params.forwarded_props.unwrap_or(Value::Null),
            resume: (!params.resume.is_empty()).then_some(params.resume.clone()),
        };

        let mut context = RunContext {
            run_id: run_id.clone(),
            thread_id: self.thread_id.clone(),
            messages: self.messages.clone(),
            state: self.state.clone(),
        };

        if let Some(subscriber) = &self.subscriber {
            subscriber.on_run_initialized(&context).await;
            subscriber.on_run_started(&context).await;
        }

        if abort.is_aborted() {
            let err = AgUiError::Cancelled;
            if let Some(subscriber) = &self.subscriber {
                subscriber.on_run_failed(&context, &err).await;
            }
            return Err(err);
        }

        let agent_for_terminal = self.agent.clone();
        let abort_for_terminal = abort.clone();
        let terminal = Arc::new(move |mw_input: MiddlewareInput| {
            let agent = agent_for_terminal.clone();
            let abort = abort_for_terminal.clone();
            Box::pin(async move { agent.run_cancellable(mw_input.run_agent_input, abort).await })
                as BoxFuture<'static, Result<EventStream>>
        });

        let raw_stream = match self
            .middleware
            .run(MiddlewareInput::from(input), terminal)
            .await
        {
            Ok(stream) => stream,
            Err(err) => {
                if let Some(subscriber) = &self.subscriber {
                    subscriber.on_run_failed(&context, &err).await;
                }
                return Err(err);
            }
        };

        let pipeline = default_apply_events(
            verify_events(expand_chunks(raw_stream)),
            self.messages.clone(),
            self.state.clone(),
        );
        let mut pipeline = pipeline.boxed();
        let mut outcome = None;
        let mut announced_tool_call_ids = HashSet::new();
        let mut message_replacements = HashMap::<String, Message>::new();
        let mut tool_call_replacements = HashMap::<String, ToolCall>::new();
        let mut text_message_buffers: HashMap<String, String> = HashMap::new();
        let mut tool_call_buffers: HashMap<String, String> = HashMap::new();
        let mut tool_call_names: HashMap<String, String> = HashMap::new();
        let mut reasoning_message_buffers: HashMap<String, String> = HashMap::new();
        let mut tool_call_args_json: HashMap<String, Value> = HashMap::new();

        while let Some(item) = pipeline.next().await {
            let applied = match item {
                Ok(applied) => applied,
                Err(err) => {
                    if let Some(subscriber) = &self.subscriber {
                        subscriber.on_run_failed(&context, &err).await;
                    }
                    return Err(err);
                }
            };

            let previous_messages = context.messages.clone();
            let previous_state = context.state.clone();
            let previous_messages_len = previous_messages.len();
            context.messages = applied.messages.clone();
            context.state = applied.state.clone();
            apply_message_replacements(&mut context.messages, &message_replacements);
            apply_tool_call_replacements(&mut context.messages, &tool_call_replacements);

            if let Some(subscriber) = &self.subscriber {
                if context.messages.len() > previous_messages_len {
                    for idx in previous_messages_len..context.messages.len() {
                        let original_message_id = context.messages[idx].id().to_string();
                        let message_result = {
                            let message = &context.messages[idx];
                            let new_message_context = NewMessageContext {
                                run: &context,
                                message,
                            };
                            subscriber.on_new_message(&new_message_context).await
                        };

                        match message_result {
                            Ok(Some(replacement)) => {
                                message_replacements
                                    .insert(original_message_id, replacement.clone());
                                context.messages[idx] = replacement;
                            }
                            Ok(None) => {}
                            Err(err) => {
                                subscriber.on_run_failed(&context, &err).await;
                                return Err(err);
                            }
                        }
                    }
                }

                apply_tool_call_replacements(&mut context.messages, &tool_call_replacements);

                for message_idx in 0..context.messages.len() {
                    let tool_call_count = match &context.messages[message_idx] {
                        Message::Assistant(message) => {
                            message.tool_calls.as_ref().map_or(0, Vec::len)
                        }
                        _ => 0,
                    };

                    for tool_call_idx in 0..tool_call_count {
                        let original_tool_call_id = match &context.messages[message_idx] {
                            Message::Assistant(message) => message
                                .tool_calls
                                .as_ref()
                                .and_then(|tool_calls| tool_calls.get(tool_call_idx))
                                .map(|tool_call| tool_call.id.clone()),
                            _ => None,
                        };

                        let Some(original_tool_call_id) = original_tool_call_id else {
                            continue;
                        };

                        if announced_tool_call_ids.contains(&original_tool_call_id) {
                            continue;
                        }

                        let tool_call_result = {
                            let tool_call = match &context.messages[message_idx] {
                                Message::Assistant(message) => message
                                    .tool_calls
                                    .as_ref()
                                    .and_then(|tool_calls| tool_calls.get(tool_call_idx)),
                                _ => None,
                            };
                            let Some(tool_call) = tool_call else {
                                continue;
                            };
                            let new_tool_call_context = NewToolCallContext {
                                run: &context,
                                tool_call,
                            };
                            subscriber.on_new_tool_call(&new_tool_call_context).await
                        };

                        match tool_call_result {
                            Ok(Some(replacement)) => {
                                tool_call_replacements
                                    .insert(original_tool_call_id.clone(), replacement.clone());
                                if let Message::Assistant(message) =
                                    &mut context.messages[message_idx]
                                {
                                    if let Some(tool_calls) = &mut message.tool_calls {
                                        if let Some(tool_call) = tool_calls.get_mut(tool_call_idx) {
                                            *tool_call = replacement.clone();
                                        }
                                    }
                                }
                                announced_tool_call_ids.insert(original_tool_call_id);
                                announced_tool_call_ids.insert(replacement.id.clone());
                            }
                            Ok(None) => {
                                announced_tool_call_ids.insert(original_tool_call_id);
                            }
                            Err(err) => {
                                subscriber.on_run_failed(&context, &err).await;
                                return Err(err);
                            }
                        }
                    }
                }
            }

            match &applied.event {
                Event::RunStarted(event) => {
                    context.run_id = event.run_id.clone();
                    context.thread_id = event.thread_id.clone();
                    self.thread_id = event.thread_id.clone();
                }
                Event::RunFinished(event) => outcome = event.outcome.clone(),
                _ => {}
            }

            if let Some(subscriber) = &self.subscriber {
                macro_rules! try_subscriber_hook {
                    ($call:expr) => {
                        if let Err(err) = $call.await {
                            subscriber.on_run_failed(&context, &err).await;
                            return Err(err);
                        }
                    };
                }

                try_subscriber_hook!(subscriber.on_event_received(&EventContext {
                    run: &context,
                    event: &applied.event,
                }));
                subscriber.on_event(&context, &applied.event).await;

                match &applied.event {
                    Event::TextMessageStart(event) => {
                        subscriber
                            .on_text_message_start(&context, &event.message_id)
                            .await
                    }
                    Event::TextMessageContent(event) => {
                        subscriber
                            .on_text_message_content(&context, &event.message_id, &event.delta)
                            .await
                    }
                    Event::TextMessageEnd(event) => {
                        subscriber
                            .on_text_message_end(&context, &event.message_id)
                            .await
                    }
                    Event::ToolCallStart(event) => {
                        subscriber
                            .on_tool_call_start(
                                &context,
                                &event.tool_call_id,
                                &event.tool_call_name,
                            )
                            .await
                    }
                    Event::ToolCallArgs(event) => {
                        subscriber
                            .on_tool_call_args(&context, &event.tool_call_id, &event.delta)
                            .await
                    }
                    Event::ToolCallEnd(event) => {
                        subscriber
                            .on_tool_call_end(&context, &event.tool_call_id)
                            .await
                    }
                    Event::ToolCallResult(event) => {
                        subscriber
                            .on_tool_call_result(&context, &event.tool_call_id, &event.content)
                            .await
                    }
                    _ => {}
                }

                match &applied.event {
                    Event::RunStarted(event) => {
                        try_subscriber_hook!(subscriber.on_run_started_event(&EventContext {
                            run: &context,
                            event,
                        }));
                    }
                    Event::RunFinished(event) => {
                        try_subscriber_hook!(subscriber.on_run_finished_event(
                            &RunFinishedContext {
                                run: &context,
                                event,
                                result: event.result.as_ref(),
                                outcome: event.outcome.as_ref(),
                            }
                        ));

                        if let Some(RunFinishedOutcome::Interrupt { interrupts }) =
                            event.outcome.as_ref()
                        {
                            try_subscriber_hook!(subscriber.on_interrupt(&InterruptContext {
                                run: &context,
                                event,
                                interrupts,
                            }));
                        }
                    }
                    Event::RunError(event) => {
                        try_subscriber_hook!(subscriber.on_run_error(&EventContext {
                            run: &context,
                            event,
                        }));
                    }
                    Event::StepStarted(event) => {
                        try_subscriber_hook!(subscriber.on_step_started(&EventContext {
                            run: &context,
                            event,
                        }));
                    }
                    Event::StepFinished(event) => {
                        try_subscriber_hook!(subscriber.on_step_finished(&EventContext {
                            run: &context,
                            event,
                        }));
                    }
                    Event::TextMessageStart(event) => {
                        text_message_buffers.insert(event.message_id.clone(), String::new());
                        try_subscriber_hook!(subscriber.on_text_message_start_event(
                            &EventContext {
                                run: &context,
                                event,
                            }
                        ));
                    }
                    Event::TextMessageContent(event) => {
                        let buffer = text_message_buffers
                            .entry(event.message_id.clone())
                            .or_default();
                        buffer.push_str(&event.delta);
                        try_subscriber_hook!(subscriber.on_text_message_content_event(
                            &TextContentContext {
                                run: &context,
                                event,
                                text_message_buffer: buffer.as_str(),
                            }
                        ));
                    }
                    Event::TextMessageEnd(event) => {
                        let text_message_buffer = text_message_buffers
                            .get(&event.message_id)
                            .map(String::as_str)
                            .unwrap_or("");
                        try_subscriber_hook!(subscriber.on_text_message_end_event(
                            &TextEndContext {
                                run: &context,
                                event,
                                text_message_buffer,
                            }
                        ));
                        text_message_buffers.remove(&event.message_id);
                    }
                    Event::TextMessageChunk(event) => {
                        try_subscriber_hook!(subscriber.on_text_message_chunk(&EventContext {
                            run: &context,
                            event,
                        }));
                    }
                    Event::ToolCallStart(event) => {
                        tool_call_buffers.insert(event.tool_call_id.clone(), String::new());
                        tool_call_names
                            .insert(event.tool_call_id.clone(), event.tool_call_name.clone());
                        try_subscriber_hook!(subscriber.on_tool_call_start_event(&EventContext {
                            run: &context,
                            event,
                        }));
                    }
                    Event::ToolCallArgs(event) => {
                        let tool_call_name = tool_call_names
                            .get(&event.tool_call_id)
                            .map(String::as_str)
                            .unwrap_or("");
                        let buffer = tool_call_buffers
                            .entry(event.tool_call_id.clone())
                            .or_default();
                        buffer.push_str(&event.delta);
                        let partial_tool_call_args =
                            serde_json::from_str::<Value>(buffer.as_str()).unwrap_or(Value::Null);
                        tool_call_args_json
                            .insert(event.tool_call_id.clone(), partial_tool_call_args);
                        let partial_tool_call_args = tool_call_args_json
                            .get(&event.tool_call_id)
                            .unwrap_or(&Value::Null);
                        try_subscriber_hook!(subscriber.on_tool_call_args_event(
                            &ToolCallArgsContext {
                                run: &context,
                                event,
                                tool_call_buffer: buffer.as_str(),
                                tool_call_name,
                                partial_tool_call_args,
                            }
                        ));
                    }
                    Event::ToolCallEnd(event) => {
                        let tool_call_name = tool_call_names
                            .get(&event.tool_call_id)
                            .map(String::as_str)
                            .unwrap_or("");
                        let tool_call_args = tool_call_buffers
                            .get(&event.tool_call_id)
                            .and_then(|buffer| serde_json::from_str::<Value>(buffer).ok())
                            .unwrap_or(Value::Null);
                        try_subscriber_hook!(subscriber.on_tool_call_end_event(
                            &ToolCallEndContext {
                                run: &context,
                                event,
                                tool_call_name,
                                tool_call_args: &tool_call_args,
                            }
                        ));
                        tool_call_buffers.remove(&event.tool_call_id);
                        tool_call_names.remove(&event.tool_call_id);
                        tool_call_args_json.remove(&event.tool_call_id);
                    }
                    Event::ToolCallChunk(event) => {
                        try_subscriber_hook!(subscriber.on_tool_call_chunk(&EventContext {
                            run: &context,
                            event,
                        }));
                    }
                    Event::ToolCallResult(event) => {
                        try_subscriber_hook!(subscriber.on_tool_call_result_event(
                            &ToolCallResultContext {
                                run: &context,
                                event,
                            }
                        ));
                    }
                    Event::ReasoningStart(event) => {
                        try_subscriber_hook!(subscriber.on_reasoning_started(&EventContext {
                            run: &context,
                            event,
                        }));
                    }
                    Event::ReasoningMessageStart(event) => {
                        reasoning_message_buffers.insert(event.message_id.clone(), String::new());
                        try_subscriber_hook!(subscriber.on_reasoning_start(&EventContext {
                            run: &context,
                            event,
                        }));
                    }
                    Event::ReasoningMessageContent(event) => {
                        let buffer = reasoning_message_buffers
                            .entry(event.message_id.clone())
                            .or_default();
                        buffer.push_str(&event.delta);
                        try_subscriber_hook!(subscriber.on_reasoning_content(
                            &ReasoningContentContext {
                                run: &context,
                                event,
                                reasoning_message_buffer: buffer.as_str(),
                            }
                        ));
                    }
                    Event::ReasoningMessageEnd(event) => {
                        let reasoning_message_buffer = reasoning_message_buffers
                            .get(&event.message_id)
                            .map(String::as_str)
                            .unwrap_or("");
                        try_subscriber_hook!(subscriber.on_reasoning_end(&ReasoningEndContext {
                            run: &context,
                            event,
                            reasoning_message_buffer,
                        }));
                        reasoning_message_buffers.remove(&event.message_id);
                    }
                    Event::ReasoningMessageChunk(event) => {
                        try_subscriber_hook!(subscriber.on_reasoning_chunk(&EventContext {
                            run: &context,
                            event,
                        }));
                    }
                    Event::ReasoningEnd(event) => {
                        try_subscriber_hook!(subscriber.on_reasoning_finished(&EventContext {
                            run: &context,
                            event,
                        }));
                    }
                    Event::ReasoningEncryptedValue(event) => {
                        try_subscriber_hook!(subscriber.on_reasoning_encrypted_value(
                            &EventContext {
                                run: &context,
                                event,
                            }
                        ));
                    }
                    Event::ThinkingStart(event) => {
                        try_subscriber_hook!(subscriber.on_thinking_start(&EventContext {
                            run: &context,
                            event,
                        }));
                    }
                    Event::ThinkingEnd(event) => {
                        try_subscriber_hook!(subscriber.on_thinking_end(&EventContext {
                            run: &context,
                            event,
                        }));
                    }
                    Event::StateSnapshot(event) => {
                        try_subscriber_hook!(subscriber.on_state_snapshot(&EventContext {
                            run: &context,
                            event,
                        }));
                    }
                    Event::StateDelta(event) => {
                        try_subscriber_hook!(subscriber.on_state_delta(&EventContext {
                            run: &context,
                            event,
                        }));
                    }
                    Event::MessagesSnapshot(event) => {
                        try_subscriber_hook!(subscriber.on_messages_snapshot(&EventContext {
                            run: &context,
                            event,
                        }));
                    }
                    Event::ActivitySnapshot(event) => {
                        let activity_message =
                            find_activity_message(&context.messages, &event.message_id);
                        let existing_message = find_message(&context.messages, &event.message_id);
                        try_subscriber_hook!(subscriber.on_activity_snapshot(
                            &ActivitySnapshotContext {
                                run: &context,
                                event,
                                activity_message,
                                existing_message,
                            }
                        ));
                    }
                    Event::ActivityDelta(event) => {
                        let activity_message =
                            find_activity_message(&context.messages, &event.message_id);
                        try_subscriber_hook!(subscriber.on_activity_delta(&ActivityDeltaContext {
                            run: &context,
                            event,
                            activity_message,
                        }));
                    }
                    Event::Raw(event) => {
                        try_subscriber_hook!(subscriber.on_raw_event(&EventContext {
                            run: &context,
                            event,
                        }));
                    }
                    Event::Custom(event) => {
                        try_subscriber_hook!(subscriber.on_custom_event(&EventContext {
                            run: &context,
                            event,
                        }));
                    }
                    Event::ThinkingTextMessageStart(_)
                    | Event::ThinkingTextMessageContent(_)
                    | Event::ThinkingTextMessageEnd(_) => {}
                }

                if previous_messages != context.messages {
                    subscriber
                        .on_messages_changed(&context, &context.messages)
                        .await;
                }
                if previous_state != context.state {
                    subscriber.on_state_changed(&context, &context.state).await;
                }
            }
        }

        self.messages = context.messages.clone();
        self.state = context.state.clone();

        // Mirror TS apply RUN_FINISHED handling: track unresolved interrupts so
        // the next run can enforce that they are addressed; clear them on a
        // successful (non-interrupt) outcome.
        match &outcome {
            Some(RunFinishedOutcome::Interrupt { interrupts }) => {
                self.pending_interrupts = interrupts.clone();
            }
            _ => {
                self.pending_interrupts.clear();
            }
        }

        if let Some(subscriber) = &self.subscriber {
            subscriber.on_run_finished(&context).await;
        }

        Ok(RunAgentResult {
            run_id: context.run_id,
            thread_id: context.thread_id,
            new_messages: self.messages[starting_messages_len..].to_vec(),
            new_state: self.state.clone(),
            outcome,
        })
    }
}

fn generate_id(prefix: &str) -> String {
    let micros = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_micros())
        .unwrap_or_default();
    format!("{prefix}-{micros}")
}

fn apply_message_replacements(messages: &mut [Message], replacements: &HashMap<String, Message>) {
    for message in messages.iter_mut() {
        if let Some(replacement) = replacements.get(message.id()) {
            *message = replacement.clone();
        }
    }
}

fn apply_tool_call_replacements(
    messages: &mut [Message],
    replacements: &HashMap<String, ToolCall>,
) {
    for message in messages.iter_mut() {
        let Message::Assistant(message) = message else {
            continue;
        };
        let Some(tool_calls) = &mut message.tool_calls else {
            continue;
        };

        for tool_call in tool_calls.iter_mut() {
            if let Some(replacement) = replacements.get(&tool_call.id) {
                *tool_call = replacement.clone();
            }
        }
    }
}

fn find_message<'a>(messages: &'a [Message], message_id: &str) -> Option<&'a Message> {
    messages.iter().find(|message| message.id() == message_id)
}

fn find_activity_message<'a>(
    messages: &'a [Message],
    message_id: &str,
) -> Option<&'a agui_rs_core::types::ActivityMessage> {
    messages.iter().find_map(|message| match message {
        Message::Activity(activity) if activity.id == message_id => Some(activity),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::middleware::{Middleware, NextFn};
    use crate::subscriber::{
        AgentSubscriber, EventContext, RunFinishedContext, TextContentContext, TextEndContext,
        ToolCallArgsContext, ToolCallEndContext,
    };
    use agui_rs_core::types::AssistantMessage;
    use agui_rs_core::{factory, BaseEventFields, FunctionCall, RunFinishedEvent, ToolCallKind};
    use futures::stream;
    use serde_json::{json, Value};
    use std::sync::{Arc, Mutex};

    type TextRecord = (String, String);
    type ToolStartRecord = (String, String);
    type ToolArgsRecord = (String, String, String, Value);
    type ToolEndRecord = (String, String, Value);

    struct FakeAgent {
        events: Vec<Event>,
    }

    #[async_trait]
    impl Agent for FakeAgent {
        async fn run(&self, _input: RunAgentInput) -> Result<BoxStream<'static, Result<Event>>> {
            Ok(Box::pin(stream::iter(
                self.events.clone().into_iter().map(Ok),
            )))
        }
    }

    struct PassthroughMiddleware {
        invoked: Arc<Mutex<bool>>,
    }

    #[async_trait]
    impl Middleware for PassthroughMiddleware {
        async fn run(
            &self,
            input: MiddlewareInput,
            next: NextFn,
        ) -> std::result::Result<EventStream, AgUiError> {
            *self.invoked.lock().expect("invoked lock") = true;
            next(input).await
        }
    }

    struct ReplaceMessageSubscriber;

    #[async_trait]
    impl AgentSubscriber for ReplaceMessageSubscriber {
        async fn on_new_message(
            &self,
            _ctx: &NewMessageContext<'_>,
        ) -> std::result::Result<Option<Message>, AgUiError> {
            Ok(Some(Message::Assistant(AssistantMessage {
                id: "replaced-message".into(),
                content: Some("replaced".into()),
                name: None,
                tool_calls: None,
                encrypted_value: None,
            })))
        }
    }

    struct ReplaceToolCallSubscriber;

    #[async_trait]
    impl AgentSubscriber for ReplaceToolCallSubscriber {
        async fn on_new_tool_call(
            &self,
            _ctx: &NewToolCallContext<'_>,
        ) -> std::result::Result<Option<ToolCall>, AgUiError> {
            Ok(Some(ToolCall {
                id: "replaced-tool-call".into(),
                kind: ToolCallKind::Function,
                function: FunctionCall {
                    name: "replacement_tool".into(),
                    arguments: "{\"ok\":true}".into(),
                },
                encrypted_value: None,
            }))
        }
    }

    #[derive(Default)]
    struct TypedRecordingSubscriber {
        order: Arc<Mutex<Vec<String>>>,
        run_started_ids: Arc<Mutex<Vec<String>>>,
        run_finished_outcomes: Arc<Mutex<Vec<Option<RunFinishedOutcome>>>>,
        text_starts: Arc<Mutex<Vec<String>>>,
        text_contents: Arc<Mutex<Vec<TextRecord>>>,
        text_ends: Arc<Mutex<Vec<TextRecord>>>,
        tool_starts: Arc<Mutex<Vec<ToolStartRecord>>>,
        tool_args: Arc<Mutex<Vec<ToolArgsRecord>>>,
        tool_ends: Arc<Mutex<Vec<ToolEndRecord>>>,
        state_snapshots: Arc<Mutex<Vec<Value>>>,
        failures: Arc<Mutex<Vec<String>>>,
    }

    impl TypedRecordingSubscriber {
        fn push_order(&self, entry: impl Into<String>) {
            self.order.lock().expect("order lock").push(entry.into());
        }
    }

    #[async_trait]
    impl AgentSubscriber for TypedRecordingSubscriber {
        async fn on_event_received(
            &self,
            ctx: &EventContext<'_, Event>,
        ) -> std::result::Result<(), AgUiError> {
            self.push_order(format!("received:{:?}", ctx.event.event_type()));
            Ok(())
        }

        async fn on_run_started_event(
            &self,
            ctx: &EventContext<'_, agui_rs_core::RunStartedEvent>,
        ) -> std::result::Result<(), AgUiError> {
            self.push_order("typed:RunStarted");
            self.run_started_ids
                .lock()
                .expect("run_started_ids lock")
                .push(ctx.event.run_id.clone());
            Ok(())
        }

        async fn on_run_finished_event(
            &self,
            ctx: &RunFinishedContext<'_>,
        ) -> std::result::Result<(), AgUiError> {
            self.push_order("typed:RunFinished");
            self.run_finished_outcomes
                .lock()
                .expect("run_finished_outcomes lock")
                .push(ctx.outcome.cloned());
            Ok(())
        }

        async fn on_text_message_start_event(
            &self,
            ctx: &EventContext<'_, agui_rs_core::TextMessageStartEvent>,
        ) -> std::result::Result<(), AgUiError> {
            self.push_order("typed:TextMessageStart");
            self.text_starts
                .lock()
                .expect("text_starts lock")
                .push(ctx.event.message_id.clone());
            Ok(())
        }

        async fn on_text_message_content_event(
            &self,
            ctx: &TextContentContext<'_>,
        ) -> std::result::Result<(), AgUiError> {
            self.push_order("typed:TextMessageContent");
            self.text_contents
                .lock()
                .expect("text_contents lock")
                .push((
                    ctx.event.message_id.clone(),
                    ctx.text_message_buffer.to_string(),
                ));
            Ok(())
        }

        async fn on_text_message_end_event(
            &self,
            ctx: &TextEndContext<'_>,
        ) -> std::result::Result<(), AgUiError> {
            self.push_order("typed:TextMessageEnd");
            self.text_ends.lock().expect("text_ends lock").push((
                ctx.event.message_id.clone(),
                ctx.text_message_buffer.to_string(),
            ));
            Ok(())
        }

        async fn on_tool_call_start_event(
            &self,
            ctx: &EventContext<'_, agui_rs_core::ToolCallStartEvent>,
        ) -> std::result::Result<(), AgUiError> {
            self.push_order("typed:ToolCallStart");
            self.tool_starts.lock().expect("tool_starts lock").push((
                ctx.event.tool_call_id.clone(),
                ctx.event.tool_call_name.clone(),
            ));
            Ok(())
        }

        async fn on_tool_call_args_event(
            &self,
            ctx: &ToolCallArgsContext<'_>,
        ) -> std::result::Result<(), AgUiError> {
            self.push_order("typed:ToolCallArgs");
            self.tool_args.lock().expect("tool_args lock").push((
                ctx.event.tool_call_id.clone(),
                ctx.tool_call_name.to_string(),
                ctx.tool_call_buffer.to_string(),
                ctx.partial_tool_call_args.clone(),
            ));
            Ok(())
        }

        async fn on_tool_call_end_event(
            &self,
            ctx: &ToolCallEndContext<'_>,
        ) -> std::result::Result<(), AgUiError> {
            self.push_order("typed:ToolCallEnd");
            self.tool_ends.lock().expect("tool_ends lock").push((
                ctx.event.tool_call_id.clone(),
                ctx.tool_call_name.to_string(),
                ctx.tool_call_args.clone(),
            ));
            Ok(())
        }

        async fn on_state_snapshot(
            &self,
            ctx: &EventContext<'_, agui_rs_core::StateSnapshotEvent>,
        ) -> std::result::Result<(), AgUiError> {
            self.push_order("typed:StateSnapshot");
            self.state_snapshots
                .lock()
                .expect("state_snapshots lock")
                .push(ctx.event.snapshot.clone());
            Ok(())
        }

        async fn on_run_failed(&self, _ctx: &RunContext, err: &AgUiError) {
            self.failures
                .lock()
                .expect("failures lock")
                .push(err.to_string());
        }
    }

    struct FailingTypedSubscriber {
        failures: Arc<Mutex<Vec<String>>>,
    }

    #[async_trait]
    impl AgentSubscriber for FailingTypedSubscriber {
        async fn on_state_snapshot(
            &self,
            _ctx: &EventContext<'_, agui_rs_core::StateSnapshotEvent>,
        ) -> std::result::Result<(), AgUiError> {
            Err(AgUiError::other("typed hook failure"))
        }

        async fn on_run_failed(&self, _ctx: &RunContext, err: &AgUiError) {
            self.failures
                .lock()
                .expect("failures lock")
                .push(err.to_string());
        }
    }

    #[tokio::test]
    async fn run_agent_end_to_end_applies_pipeline() {
        let agent = FakeAgent {
            events: vec![
                factory::run_started("thread-1", "run-1"),
                Event::TextMessageChunk(agui_rs_core::TextMessageChunkEvent {
                    message_id: Some("m1".into()),
                    role: Some(agui_rs_core::TextMessageRole::Assistant),
                    delta: Some("hello".into()),
                    name: None,
                    base: BaseEventFields::default(),
                }),
                Event::RunFinished(RunFinishedEvent {
                    thread_id: "thread-1".into(),
                    run_id: "run-1".into(),
                    result: None,
                    outcome: Some(RunFinishedOutcome::Success),
                    base: BaseEventFields::default(),
                }),
            ],
        };

        let mut runner = AgentRunner::new(agent, AgentConfig::default());
        let result = runner
            .run_agent(RunAgentParameters::default())
            .await
            .expect("run should succeed");

        assert_eq!(result.thread_id, "thread-1");
        assert_eq!(result.run_id, "run-1");
        assert_eq!(result.new_messages.len(), 1);
        match &result.new_messages[0] {
            Message::Assistant(message) => assert_eq!(message.content.as_deref(), Some("hello")),
            _ => panic!("expected assistant message"),
        }
        assert_eq!(result.outcome, Some(RunFinishedOutcome::Success));
    }

    #[tokio::test]
    async fn middleware_chain_passes_input_through() {
        let invoked = Arc::new(Mutex::new(false));
        let mut chain = MiddlewareChain::new();
        chain.push(PassthroughMiddleware {
            invoked: Arc::clone(&invoked),
        });

        let agent = FakeAgent {
            events: vec![
                factory::run_started("thread-1", "run-1"),
                Event::TextMessageChunk(agui_rs_core::TextMessageChunkEvent {
                    message_id: Some("m1".into()),
                    role: Some(agui_rs_core::TextMessageRole::Assistant),
                    delta: Some("hello".into()),
                    name: None,
                    base: BaseEventFields::default(),
                }),
                Event::RunFinished(RunFinishedEvent {
                    thread_id: "thread-1".into(),
                    run_id: "run-1".into(),
                    result: None,
                    outcome: Some(RunFinishedOutcome::Success),
                    base: BaseEventFields::default(),
                }),
            ],
        };

        let mut runner = AgentRunner::new(agent, AgentConfig::default()).with_middleware(chain);
        let result = runner
            .run_agent(RunAgentParameters::default())
            .await
            .expect("run should succeed");

        assert!(*invoked.lock().expect("invoked lock"));
        assert_eq!(result.new_messages.len(), 1);
    }

    #[tokio::test]
    async fn on_new_message_mutation_replaces_message() {
        let agent = FakeAgent {
            events: vec![
                factory::run_started("thread-1", "run-1"),
                Event::TextMessageChunk(agui_rs_core::TextMessageChunkEvent {
                    message_id: Some("m1".into()),
                    role: Some(agui_rs_core::TextMessageRole::Assistant),
                    delta: Some("hello".into()),
                    name: None,
                    base: BaseEventFields::default(),
                }),
                Event::RunFinished(RunFinishedEvent {
                    thread_id: "thread-1".into(),
                    run_id: "run-1".into(),
                    result: None,
                    outcome: Some(RunFinishedOutcome::Success),
                    base: BaseEventFields::default(),
                }),
            ],
        };

        let subscriber: Arc<dyn AgentSubscriber> = Arc::new(ReplaceMessageSubscriber);
        let mut runner =
            AgentRunner::new(agent, AgentConfig::default()).with_subscriber(subscriber);
        let result = runner
            .run_agent(RunAgentParameters::default())
            .await
            .expect("run should succeed");

        match &result.new_messages[0] {
            Message::Assistant(message) => assert_eq!(message.content.as_deref(), Some("replaced")),
            _ => panic!("expected assistant message"),
        }
    }

    #[tokio::test]
    async fn on_new_tool_call_mutation_replaces_tool_call() {
        let agent = FakeAgent {
            events: vec![
                factory::run_started("thread-1", "run-1"),
                factory::tool_call_start("call-1", "tool"),
                factory::tool_call_args("call-1", "{}"),
                factory::tool_call_end("call-1"),
                Event::RunFinished(RunFinishedEvent {
                    thread_id: "thread-1".into(),
                    run_id: "run-1".into(),
                    result: None,
                    outcome: Some(RunFinishedOutcome::Success),
                    base: BaseEventFields::default(),
                }),
            ],
        };

        let subscriber: Arc<dyn AgentSubscriber> = Arc::new(ReplaceToolCallSubscriber);
        let mut runner =
            AgentRunner::new(agent, AgentConfig::default()).with_subscriber(subscriber);
        let result = runner
            .run_agent(RunAgentParameters::default())
            .await
            .expect("run should succeed");

        let Message::Assistant(message) = &result.new_messages[0] else {
            panic!("expected assistant message");
        };
        let tool_calls = message.tool_calls.as_ref().expect("tool calls");
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_calls[0].id, "replaced-tool-call");
        assert_eq!(tool_calls[0].function.name, "replacement_tool");
        assert_eq!(tool_calls[0].function.arguments, "{\"ok\":true}");
    }

    #[tokio::test]
    async fn on_event_received_fires_for_every_event_before_typed_dispatch() {
        let subscriber_impl = Arc::new(TypedRecordingSubscriber::default());
        let subscriber: Arc<dyn AgentSubscriber> = subscriber_impl.clone();
        let agent = FakeAgent {
            events: vec![
                factory::run_started("thread-1", "run-1"),
                factory::state_snapshot(json!({"count": 1})),
                factory::run_finished("thread-1", "run-1"),
            ],
        };

        let mut runner =
            AgentRunner::new(agent, AgentConfig::default()).with_subscriber(subscriber);
        runner
            .run_agent(RunAgentParameters::default())
            .await
            .expect("run should succeed");

        assert_eq!(
            subscriber_impl.order.lock().expect("order lock").clone(),
            vec![
                "received:RunStarted",
                "typed:RunStarted",
                "received:StateSnapshot",
                "typed:StateSnapshot",
                "received:RunFinished",
                "typed:RunFinished",
            ]
        );
    }

    #[tokio::test]
    async fn run_started_and_finished_typed_hooks_fire() {
        let subscriber_impl = Arc::new(TypedRecordingSubscriber::default());
        let subscriber: Arc<dyn AgentSubscriber> = subscriber_impl.clone();
        let agent = FakeAgent {
            events: vec![
                factory::run_started("thread-1", "run-1"),
                factory::run_finished("thread-1", "run-1"),
            ],
        };

        let mut runner =
            AgentRunner::new(agent, AgentConfig::default()).with_subscriber(subscriber);
        runner
            .run_agent(RunAgentParameters::default())
            .await
            .expect("run should succeed");

        assert_eq!(
            subscriber_impl
                .run_started_ids
                .lock()
                .expect("run_started_ids lock")
                .clone(),
            vec!["run-1".to_string()]
        );
        assert_eq!(
            subscriber_impl
                .run_finished_outcomes
                .lock()
                .expect("run_finished_outcomes lock")
                .clone(),
            vec![Some(RunFinishedOutcome::Success)]
        );
    }

    #[tokio::test]
    async fn text_message_typed_hooks_fire_with_correct_buffers() {
        let subscriber_impl = Arc::new(TypedRecordingSubscriber::default());
        let subscriber: Arc<dyn AgentSubscriber> = subscriber_impl.clone();
        let agent = FakeAgent {
            events: vec![
                factory::run_started("thread-1", "run-1"),
                factory::text_message_start("m1"),
                factory::text_message_content("m1", "he"),
                factory::text_message_content("m1", "llo"),
                factory::text_message_end("m1"),
                factory::run_finished("thread-1", "run-1"),
            ],
        };

        let mut runner =
            AgentRunner::new(agent, AgentConfig::default()).with_subscriber(subscriber);
        runner
            .run_agent(RunAgentParameters::default())
            .await
            .expect("run should succeed");

        assert_eq!(
            subscriber_impl
                .text_starts
                .lock()
                .expect("text_starts lock")
                .clone(),
            vec!["m1".to_string()]
        );
        assert_eq!(
            subscriber_impl
                .text_contents
                .lock()
                .expect("text_contents lock")
                .clone(),
            vec![
                ("m1".to_string(), "he".to_string()),
                ("m1".to_string(), "hello".to_string()),
            ]
        );
        assert_eq!(
            subscriber_impl
                .text_ends
                .lock()
                .expect("text_ends lock")
                .clone(),
            vec![("m1".to_string(), "hello".to_string())]
        );
    }

    #[tokio::test]
    async fn tool_call_typed_hooks_fire_with_correct_buffers_and_name() {
        let subscriber_impl = Arc::new(TypedRecordingSubscriber::default());
        let subscriber: Arc<dyn AgentSubscriber> = subscriber_impl.clone();
        let agent = FakeAgent {
            events: vec![
                factory::run_started("thread-1", "run-1"),
                factory::tool_call_start("call-1", "search"),
                factory::tool_call_args("call-1", "{\"q\":"),
                factory::tool_call_args("call-1", "\"rust\"}"),
                factory::tool_call_end("call-1"),
                factory::run_finished("thread-1", "run-1"),
            ],
        };

        let mut runner =
            AgentRunner::new(agent, AgentConfig::default()).with_subscriber(subscriber);
        runner
            .run_agent(RunAgentParameters::default())
            .await
            .expect("run should succeed");

        assert_eq!(
            subscriber_impl
                .tool_starts
                .lock()
                .expect("tool_starts lock")
                .clone(),
            vec![("call-1".to_string(), "search".to_string())]
        );
        assert_eq!(
            subscriber_impl
                .tool_args
                .lock()
                .expect("tool_args lock")
                .clone(),
            vec![
                (
                    "call-1".to_string(),
                    "search".to_string(),
                    "{\"q\":".to_string(),
                    Value::Null,
                ),
                (
                    "call-1".to_string(),
                    "search".to_string(),
                    "{\"q\":\"rust\"}".to_string(),
                    json!({"q": "rust"}),
                ),
            ]
        );
        assert_eq!(
            subscriber_impl
                .tool_ends
                .lock()
                .expect("tool_ends lock")
                .clone(),
            vec![(
                "call-1".to_string(),
                "search".to_string(),
                json!({"q": "rust"}),
            )]
        );
    }

    #[tokio::test]
    async fn state_snapshot_typed_hook_fires() {
        let subscriber_impl = Arc::new(TypedRecordingSubscriber::default());
        let subscriber: Arc<dyn AgentSubscriber> = subscriber_impl.clone();
        let agent = FakeAgent {
            events: vec![
                factory::run_started("thread-1", "run-1"),
                factory::state_snapshot(json!({"count": 3})),
                factory::run_finished("thread-1", "run-1"),
            ],
        };

        let mut runner =
            AgentRunner::new(agent, AgentConfig::default()).with_subscriber(subscriber);
        runner
            .run_agent(RunAgentParameters::default())
            .await
            .expect("run should succeed");

        assert_eq!(
            subscriber_impl
                .state_snapshots
                .lock()
                .expect("state_snapshots lock")
                .clone(),
            vec![json!({"count": 3})]
        );
    }

    #[tokio::test]
    async fn typed_hook_error_fails_run_and_triggers_on_run_failed() {
        let failures = Arc::new(Mutex::new(Vec::new()));
        let subscriber: Arc<dyn AgentSubscriber> = Arc::new(FailingTypedSubscriber {
            failures: Arc::clone(&failures),
        });
        let agent = FakeAgent {
            events: vec![
                factory::run_started("thread-1", "run-1"),
                factory::state_snapshot(json!({"count": 1})),
                factory::run_finished("thread-1", "run-1"),
            ],
        };

        let mut runner =
            AgentRunner::new(agent, AgentConfig::default()).with_subscriber(subscriber);
        let err = runner
            .run_agent(RunAgentParameters::default())
            .await
            .expect_err("run should fail");

        assert!(matches!(err, AgUiError::Other(message) if message == "typed hook failure"));
        assert_eq!(
            failures.lock().expect("failures lock").clone(),
            vec!["typed hook failure".to_string()]
        );
    }
}
