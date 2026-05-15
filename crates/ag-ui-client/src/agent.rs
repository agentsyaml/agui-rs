use crate::apply::default_apply_events;
use crate::chunks::expand_chunks;
use crate::middleware::{MiddlewareChain, MiddlewareInput};
use crate::subscriber::{AgentSubscriber, NewMessageContext, NewToolCallContext, RunContext};
use crate::verify::verify_events;
use ag_ui_core::{
    AgUiError, Context, Event, Message, Result, RunAgentInput, RunFinishedOutcome, State, Tool,
    ToolCall,
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

#[derive(Debug)]
struct AbortState {
    aborted: AtomicBool,
    notify: Notify,
}

impl Default for AbortState {
    fn default() -> Self {
        Self {
            aborted: AtomicBool::new(false),
            notify: Notify::new(),
        }
    }
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
    pub resume: Vec<ag_ui_core::ResumeEntry>,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::middleware::{Middleware, NextFn};
    use crate::subscriber::AgentSubscriber;
    use ag_ui_core::types::AssistantMessage;
    use ag_ui_core::{factory, BaseEventFields, FunctionCall, RunFinishedEvent, ToolCallKind};
    use futures::stream;
    use std::sync::Mutex;

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

    #[tokio::test]
    async fn run_agent_end_to_end_applies_pipeline() {
        let agent = FakeAgent {
            events: vec![
                factory::run_started("thread-1", "run-1"),
                Event::TextMessageChunk(ag_ui_core::TextMessageChunkEvent {
                    message_id: Some("m1".into()),
                    role: Some(ag_ui_core::TextMessageRole::Assistant),
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
                Event::TextMessageChunk(ag_ui_core::TextMessageChunkEvent {
                    message_id: Some("m1".into()),
                    role: Some(ag_ui_core::TextMessageRole::Assistant),
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
                Event::TextMessageChunk(ag_ui_core::TextMessageChunkEvent {
                    message_id: Some("m1".into()),
                    role: Some(ag_ui_core::TextMessageRole::Assistant),
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
}
