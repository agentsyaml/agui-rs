use agui_rs_core::types::UserMessage;
use agui_rs_core::{
    AgUiError, BinaryInputContent, Event, InputContent, InputContentSource, Message,
    ReasoningEndEvent, ReasoningMessageContentEvent, ReasoningMessageEndEvent,
    ReasoningMessageRole, ReasoningMessageStartEvent, ReasoningStartEvent, RunAgentInput,
    ThinkingEndEvent, ThinkingStartEvent, ThinkingTextMessageContentEvent,
    ThinkingTextMessageEndEvent, ThinkingTextMessageStartEvent, UserMessageContent,
};
use async_stream::try_stream;
use async_trait::async_trait;
use futures::{future::BoxFuture, stream::BoxStream, StreamExt};
use serde_json::{Map, Value};
use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

pub type EventStream = BoxStream<'static, std::result::Result<Event, AgUiError>>;
pub type NextFn = Arc<
    dyn Fn(MiddlewareInput) -> BoxFuture<'static, std::result::Result<EventStream, AgUiError>>
        + Send
        + Sync,
>;
pub type TerminalFn = NextFn;

#[derive(Debug, Clone, PartialEq)]
pub struct MiddlewareInput {
    pub run_agent_input: RunAgentInput,
    pub extras: Value,
}

impl MiddlewareInput {
    pub fn new(run_agent_input: RunAgentInput, extras: Value) -> Self {
        Self {
            run_agent_input,
            extras,
        }
    }
}

impl From<RunAgentInput> for MiddlewareInput {
    fn from(run_agent_input: RunAgentInput) -> Self {
        Self {
            run_agent_input,
            extras: Value::Object(Map::new()),
        }
    }
}

#[async_trait]
pub trait Middleware: Send + Sync {
    async fn run(
        &self,
        input: MiddlewareInput,
        next: NextFn,
    ) -> std::result::Result<EventStream, AgUiError>;
}

pub fn chain_middlewares(
    middlewares: Vec<Arc<dyn Middleware>>,
    terminal: TerminalFn,
) -> TerminalFn {
    middlewares
        .into_iter()
        .rev()
        .fold(terminal, |next, middleware| {
            Arc::new(move |input: MiddlewareInput| {
                let middleware = Arc::clone(&middleware);
                let next = Arc::clone(&next);
                Box::pin(async move { middleware.run(input, next).await })
            })
        })
}

#[derive(Default, Clone)]
pub struct MiddlewareChain {
    middlewares: Vec<Arc<dyn Middleware>>,
}

impl MiddlewareChain {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push<M>(&mut self, middleware: M)
    where
        M: Middleware + 'static,
    {
        self.middlewares.push(Arc::new(middleware));
    }

    pub fn push_arc(&mut self, middleware: Arc<dyn Middleware>) {
        self.middlewares.push(middleware);
    }

    /// Appends all middlewares from `other` to the end of this chain,
    /// preserving order. Used to compose auto-inserted middleware with
    /// user-provided middleware.
    pub fn extend(&mut self, other: MiddlewareChain) {
        self.middlewares.extend(other.middlewares);
    }

    pub async fn run(
        &self,
        input: MiddlewareInput,
        terminal: TerminalFn,
    ) -> std::result::Result<EventStream, AgUiError> {
        chain_middlewares(self.middlewares.clone(), terminal)(input).await
    }

    pub async fn apply(
        &self,
        input: MiddlewareInput,
        terminal: TerminalFn,
    ) -> std::result::Result<EventStream, AgUiError> {
        self.run(input, terminal).await
    }

    pub fn is_empty(&self) -> bool {
        self.middlewares.is_empty()
    }

    /// Number of middlewares in the chain.
    pub fn len(&self) -> usize {
        self.middlewares.len()
    }
}

/// `filterToolCalls` middleware port: filters tool calls by allow/deny lists.
pub mod filter_tool_calls {
    use super::*;

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct FilterToolCallsConfig {
        allowed_tool_calls: Option<Vec<String>>,
        disallowed_tool_calls: Option<Vec<String>>,
    }

    impl FilterToolCallsConfig {
        pub fn new(
            allowed_tool_calls: Option<Vec<String>>,
            disallowed_tool_calls: Option<Vec<String>>,
        ) -> std::result::Result<Self, AgUiError> {
            if allowed_tool_calls.is_some() && disallowed_tool_calls.is_some() {
                return Err(AgUiError::validation(
                    "Cannot specify both allowedToolCalls and disallowedToolCalls",
                ));
            }

            if allowed_tool_calls.is_none() && disallowed_tool_calls.is_none() {
                return Err(AgUiError::validation(
                    "Must specify either allowedToolCalls or disallowedToolCalls",
                ));
            }

            Ok(Self {
                allowed_tool_calls,
                disallowed_tool_calls,
            })
        }

        pub fn allowed(tool_calls: Vec<String>) -> Self {
            Self {
                allowed_tool_calls: Some(tool_calls),
                disallowed_tool_calls: None,
            }
        }

        pub fn disallowed(tool_calls: Vec<String>) -> Self {
            Self {
                allowed_tool_calls: None,
                disallowed_tool_calls: Some(tool_calls),
            }
        }
    }

    #[derive(Debug, Clone)]
    pub struct FilterToolCallsMiddleware {
        allowed_tools: Option<HashSet<String>>,
        disallowed_tools: Option<HashSet<String>>,
    }

    impl FilterToolCallsMiddleware {
        pub fn new(config: FilterToolCallsConfig) -> Self {
            Self {
                allowed_tools: config
                    .allowed_tool_calls
                    .map(|tools| tools.into_iter().collect()),
                disallowed_tools: config
                    .disallowed_tool_calls
                    .map(|tools| tools.into_iter().collect()),
            }
        }
    }

    #[async_trait]
    impl Middleware for FilterToolCallsMiddleware {
        async fn run(
            &self,
            input: MiddlewareInput,
            next: NextFn,
        ) -> std::result::Result<EventStream, AgUiError> {
            let mut upstream = next(input).await?;
            let allowed_tools = self.allowed_tools.clone();
            let disallowed_tools = self.disallowed_tools.clone();

            let output = try_stream! {
                let mut blocked_tool_call_ids = HashSet::new();

                while let Some(item) = upstream.next().await {
                    let event = item?;

                    match event {
                        Event::ToolCallStart(tool_call_start_event) => {
                            let should_filter = if let Some(allowed_tools) = &allowed_tools {
                                !allowed_tools.contains(tool_call_start_event.tool_call_name.as_str())
                            } else if let Some(disallowed_tools) = &disallowed_tools {
                                disallowed_tools.contains(tool_call_start_event.tool_call_name.as_str())
                            } else {
                                false
                            };

                            if should_filter {
                                blocked_tool_call_ids.insert(tool_call_start_event.tool_call_id);
                            } else {
                                yield Event::ToolCallStart(tool_call_start_event);
                            }
                        }
                        Event::ToolCallArgs(tool_call_args_event) => {
                            if !blocked_tool_call_ids.contains(tool_call_args_event.tool_call_id.as_str()) {
                                yield Event::ToolCallArgs(tool_call_args_event);
                            }
                        }
                        Event::ToolCallEnd(tool_call_end_event) => {
                            if !blocked_tool_call_ids.contains(tool_call_end_event.tool_call_id.as_str()) {
                                yield Event::ToolCallEnd(tool_call_end_event);
                            }
                        }
                        Event::ToolCallResult(tool_call_result_event) => {
                            if blocked_tool_call_ids.remove(tool_call_result_event.tool_call_id.as_str()) {
                                continue;
                            }

                            yield Event::ToolCallResult(tool_call_result_event);
                        }
                        other => yield other,
                    }
                }
            };

            Ok(Box::pin(output))
        }
    }
}

/// Backward-compatibility middlewares mirroring the TS
/// `BackwardCompatibility_0_0_*` exports. Auto-inserted by
/// [`AgentRunner::with_max_version`](crate::AgentRunner::with_max_version).
pub mod backward_compat {
    use super::*;

    #[derive(Debug, Clone, Default)]
    pub struct BackwardCompat0_0_39;

    #[async_trait]
    impl Middleware for BackwardCompat0_0_39 {
        async fn run(
            &self,
            input: MiddlewareInput,
            next: NextFn,
        ) -> std::result::Result<EventStream, AgUiError> {
            let MiddlewareInput {
                run_agent_input,
                extras,
            } = input;

            let sanitized_input = MiddlewareInput {
                run_agent_input: sanitize_run_agent_input_0_0_39(run_agent_input),
                extras,
            };

            next(sanitized_input).await
        }
    }

    #[derive(Debug, Clone, Default)]
    pub struct BackwardCompat0_0_45;

    #[async_trait]
    impl Middleware for BackwardCompat0_0_45 {
        async fn run(
            &self,
            input: MiddlewareInput,
            next: NextFn,
        ) -> std::result::Result<EventStream, AgUiError> {
            let mut upstream = next(input).await?;

            let output = try_stream! {
                let mut current_reasoning_id: Option<String> = None;
                let mut current_message_id: Option<String> = None;

                while let Some(item) = upstream.next().await {
                    let event = item?;

                    let transformed = match event {
                        Event::ThinkingStart(ThinkingStartEvent { base, .. }) => {
                            let reasoning_id = generate_transformation_id("reasoning");
                            current_reasoning_id = Some(reasoning_id.clone());
                            warn_about_transformation("THINKING_START", "REASONING_START");
                            Event::ReasoningStart(ReasoningStartEvent {
                                message_id: reasoning_id,
                                base,
                            })
                        }
                        Event::ThinkingTextMessageStart(ThinkingTextMessageStartEvent { base }) => {
                            let message_id = generate_transformation_id("reasoning-message");
                            current_message_id = Some(message_id.clone());
                            warn_about_transformation(
                                "THINKING_TEXT_MESSAGE_START",
                                "REASONING_MESSAGE_START",
                            );
                            Event::ReasoningMessageStart(ReasoningMessageStartEvent {
                                message_id,
                                role: ReasoningMessageRole::Reasoning,
                                base,
                            })
                        }
                        Event::ThinkingTextMessageContent(ThinkingTextMessageContentEvent { delta, base }) => {
                            let message_id = current_message_id
                                .clone()
                                .unwrap_or_else(|| generate_transformation_id("reasoning-message"));
                            warn_about_transformation(
                                "THINKING_TEXT_MESSAGE_CONTENT",
                                "REASONING_MESSAGE_CONTENT",
                            );
                            Event::ReasoningMessageContent(ReasoningMessageContentEvent {
                                message_id,
                                delta,
                                base,
                            })
                        }
                        Event::ThinkingTextMessageEnd(ThinkingTextMessageEndEvent { base }) => {
                            let message_id = current_message_id
                                .clone()
                                .unwrap_or_else(|| generate_transformation_id("reasoning-message"));
                            warn_about_transformation(
                                "THINKING_TEXT_MESSAGE_END",
                                "REASONING_MESSAGE_END",
                            );
                            Event::ReasoningMessageEnd(ReasoningMessageEndEvent { message_id, base })
                        }
                        Event::ThinkingEnd(ThinkingEndEvent { base }) => {
                            let reasoning_id = current_reasoning_id
                                .clone()
                                .unwrap_or_else(|| generate_transformation_id("reasoning"));
                            warn_about_transformation("THINKING_END", "REASONING_END");
                            Event::ReasoningEnd(ReasoningEndEvent {
                                message_id: reasoning_id,
                                base,
                            })
                        }
                        other => other,
                    };

                    yield transformed;
                }
            };

            Ok(Box::pin(output))
        }
    }

    #[derive(Debug, Clone, Default)]
    pub struct BackwardCompat0_0_47;

    #[async_trait]
    impl Middleware for BackwardCompat0_0_47 {
        async fn run(
            &self,
            input: MiddlewareInput,
            next: NextFn,
        ) -> std::result::Result<EventStream, AgUiError> {
            let MiddlewareInput {
                run_agent_input,
                extras,
            } = input;

            let upgraded_input = MiddlewareInput {
                run_agent_input: upgrade_run_agent_input_0_0_47(run_agent_input),
                extras,
            };

            next(upgraded_input).await
        }
    }

    fn sanitize_run_agent_input_0_0_39(input: RunAgentInput) -> RunAgentInput {
        RunAgentInput {
            parent_run_id: None,
            messages: input
                .messages
                .into_iter()
                .map(sanitize_message_content_0_0_39)
                .collect(),
            ..input
        }
    }

    fn sanitize_message_content_0_0_39(message: Message) -> Message {
        match message {
            Message::User(UserMessage {
                id,
                content: UserMessageContent::Parts(parts),
                name,
                encrypted_value,
            }) => {
                let concatenated_content = parts
                    .into_iter()
                    .filter_map(|part| match part {
                        InputContent::Text { text } => Some(text),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("");

                Message::User(UserMessage {
                    id,
                    content: UserMessageContent::Text(concatenated_content),
                    name,
                    encrypted_value,
                })
            }
            other => other,
        }
    }

    fn upgrade_run_agent_input_0_0_47(input: RunAgentInput) -> RunAgentInput {
        RunAgentInput {
            messages: input
                .messages
                .into_iter()
                .map(upgrade_message_content_0_0_47)
                .collect(),
            ..input
        }
    }

    fn upgrade_message_content_0_0_47(message: Message) -> Message {
        match message {
            Message::User(UserMessage {
                id,
                content: UserMessageContent::Parts(parts),
                name,
                encrypted_value,
            }) => Message::User(UserMessage {
                id,
                content: UserMessageContent::Parts(
                    parts
                        .into_iter()
                        .map(convert_legacy_binary_to_new_format_0_0_47)
                        .collect(),
                ),
                name,
                encrypted_value,
            }),
            other => other,
        }
    }

    fn convert_legacy_binary_to_new_format_0_0_47(part: InputContent) -> InputContent {
        match part {
            InputContent::Binary {
                content:
                    BinaryInputContent {
                        mime_type,
                        id,
                        url,
                        data,
                        filename,
                    },
            } => {
                if let Some(data) = data {
                    return convert_new_content_part_0_0_47(
                        mime_type.clone(),
                        InputContentSource::Data {
                            value: data,
                            mime_type: mime_type.clone(),
                        },
                        filename,
                    );
                }

                if let Some(url) = url {
                    return convert_new_content_part_0_0_47(
                        mime_type.clone(),
                        InputContentSource::Url {
                            value: url,
                            mime_type: Some(mime_type.clone()),
                        },
                        filename,
                    );
                }

                InputContent::Binary {
                    content: BinaryInputContent {
                        mime_type,
                        id,
                        url: None,
                        data: None,
                        filename,
                    },
                }
            }
            other => other,
        }
    }

    fn convert_new_content_part_0_0_47(
        mime_type: String,
        source: InputContentSource,
        filename: Option<String>,
    ) -> InputContent {
        let metadata = filename.map(|filename| {
            Value::Object(Map::from_iter([(
                "filename".to_string(),
                Value::String(filename),
            )]))
        });

        if mime_type.starts_with("image/") {
            InputContent::Image { source, metadata }
        } else if mime_type.starts_with("audio/") {
            InputContent::Audio { source, metadata }
        } else if mime_type.starts_with("video/") {
            InputContent::Video { source, metadata }
        } else {
            InputContent::Document { source, metadata }
        }
    }

    fn warn_about_transformation(from: &str, to: &str) {
        if std::env::var_os("SUPPRESS_TRANSFORMATION_WARNINGS").is_some() {
            return;
        }

        eprintln!(
            "AG-UI is converting {from} to {to}. To remove this warning, upgrade your AG-UI integration package. To suppress it, set SUPPRESS_TRANSFORMATION_WARNINGS=true in your environment.",
        );
    }
}

static TRANSFORMATION_COUNTER: AtomicU64 = AtomicU64::new(0);

fn generate_transformation_id(prefix: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let counter = TRANSFORMATION_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{prefix}-{nanos}-{counter}")
}

#[cfg(test)]
mod chain_tests {
    use super::*;
    use agui_rs_core::{
        factory, BaseEventFields, ToolCallArgsEvent, ToolCallEndEvent, ToolCallResultEvent,
        ToolCallStartEvent,
    };
    use futures::stream;
    use std::sync::Mutex;

    #[derive(Debug)]
    struct RecordingMiddleware {
        label: &'static str,
        log: Arc<Mutex<Vec<String>>>,
    }

    #[async_trait]
    impl Middleware for RecordingMiddleware {
        async fn run(
            &self,
            input: MiddlewareInput,
            next: NextFn,
        ) -> std::result::Result<EventStream, AgUiError> {
            self.log
                .lock()
                .expect("log lock")
                .push(format!("{}:before", self.label));
            let stream = next(input).await?;
            self.log
                .lock()
                .expect("log lock")
                .push(format!("{}:after", self.label));
            Ok(stream)
        }
    }

    struct ShortCircuitMiddleware;

    #[async_trait]
    impl Middleware for ShortCircuitMiddleware {
        async fn run(
            &self,
            _input: MiddlewareInput,
            _next: NextFn,
        ) -> std::result::Result<EventStream, AgUiError> {
            Ok(Box::pin(stream::iter(vec![Ok(factory::run_started(
                "thread-short",
                "run-short",
            ))])))
        }
    }

    struct ErrorMiddleware;

    #[async_trait]
    impl Middleware for ErrorMiddleware {
        async fn run(
            &self,
            _input: MiddlewareInput,
            _next: NextFn,
        ) -> std::result::Result<EventStream, AgUiError> {
            Err(AgUiError::other("middleware boom"))
        }
    }

    fn base_input() -> MiddlewareInput {
        MiddlewareInput::from(RunAgentInput::new("thread-1", "run-1"))
    }

    async fn collect_events(mut stream: EventStream) -> Vec<Event> {
        let mut events = Vec::new();
        while let Some(item) = stream.next().await {
            events.push(item.expect("stream item"));
        }
        events
    }

    #[tokio::test]
    async fn ordering_wraps_right_to_left() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let first: Arc<dyn Middleware> = Arc::new(RecordingMiddleware {
            label: "first",
            log: Arc::clone(&log),
        });
        let second: Arc<dyn Middleware> = Arc::new(RecordingMiddleware {
            label: "second",
            log: Arc::clone(&log),
        });
        let terminal_log = Arc::clone(&log);
        let terminal: TerminalFn = Arc::new(move |_input| {
            let terminal_log = Arc::clone(&terminal_log);
            Box::pin(async move {
                terminal_log
                    .lock()
                    .expect("log lock")
                    .push("terminal".into());
                Ok(Box::pin(stream::iter(vec![Ok(factory::run_started(
                    "thread-order",
                    "run-order",
                ))])) as EventStream)
            })
        });

        let chain = chain_middlewares(vec![first, second], terminal);
        let _ = collect_events(chain(base_input()).await.expect("chain result")).await;

        assert_eq!(
            log.lock().expect("log lock").clone(),
            vec![
                "first:before",
                "second:before",
                "terminal",
                "second:after",
                "first:after"
            ]
        );
    }

    #[tokio::test]
    async fn terminal_error_propagates() {
        let terminal: TerminalFn =
            Arc::new(|_input| Box::pin(async { Err(AgUiError::other("terminal boom")) }));
        let chain = chain_middlewares(Vec::new(), terminal);

        let error = match chain(base_input()).await {
            Ok(_) => panic!("terminal should fail"),
            Err(error) => error,
        };
        assert_eq!(error.to_string(), "terminal boom");
    }

    #[tokio::test]
    async fn middleware_error_propagates_without_terminal() {
        let terminal_called = Arc::new(Mutex::new(false));
        let terminal_flag = Arc::clone(&terminal_called);
        let terminal: TerminalFn = Arc::new(move |_input| {
            let terminal_flag = Arc::clone(&terminal_flag);
            Box::pin(async move {
                *terminal_flag.lock().expect("flag lock") = true;
                Ok(Box::pin(stream::empty()) as EventStream)
            })
        });

        let chain = chain_middlewares(vec![Arc::new(ErrorMiddleware)], terminal);
        let error = match chain(base_input()).await {
            Ok(_) => panic!("middleware should fail"),
            Err(error) => error,
        };

        assert_eq!(error.to_string(), "middleware boom");
        assert!(!*terminal_called.lock().expect("flag lock"));
    }

    #[tokio::test]
    async fn short_circuit_skips_remaining_chain() {
        let terminal_called = Arc::new(Mutex::new(false));
        let terminal_flag = Arc::clone(&terminal_called);
        let terminal: TerminalFn = Arc::new(move |_input| {
            let terminal_flag = Arc::clone(&terminal_flag);
            Box::pin(async move {
                *terminal_flag.lock().expect("flag lock") = true;
                Ok(Box::pin(stream::empty()) as EventStream)
            })
        });

        let chain = chain_middlewares(vec![Arc::new(ShortCircuitMiddleware)], terminal);
        let events = collect_events(chain(base_input()).await.expect("chain result")).await;

        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], Event::RunStarted(_)));
        assert!(!*terminal_called.lock().expect("flag lock"));
    }

    #[tokio::test]
    async fn middleware_chain_run_uses_composed_terminal() {
        let mut chain = MiddlewareChain::new();
        let middleware: Arc<dyn Middleware> = Arc::new(RecordingMiddleware {
            label: "only",
            log: Arc::new(Mutex::new(Vec::new())),
        });
        chain.push_arc(middleware);

        let terminal: TerminalFn = Arc::new(|_input| {
            Box::pin(async move {
                Ok(Box::pin(stream::iter(vec![
                    Ok(Event::ToolCallStart(ToolCallStartEvent {
                        tool_call_id: "call-1".into(),
                        tool_call_name: "tool".into(),
                        parent_message_id: None,
                        base: BaseEventFields::default(),
                    })),
                    Ok(Event::ToolCallArgs(ToolCallArgsEvent {
                        tool_call_id: "call-1".into(),
                        delta: "{}".into(),
                        base: BaseEventFields::default(),
                    })),
                    Ok(Event::ToolCallEnd(ToolCallEndEvent {
                        tool_call_id: "call-1".into(),
                        base: BaseEventFields::default(),
                    })),
                    Ok(Event::ToolCallResult(ToolCallResultEvent {
                        message_id: "msg-1".into(),
                        tool_call_id: "call-1".into(),
                        content: "ok".into(),
                        role: None,
                        base: BaseEventFields::default(),
                    })),
                ])) as EventStream)
            })
        });

        let events = collect_events(
            chain
                .run(base_input(), terminal)
                .await
                .expect("chain result"),
        )
        .await;
        assert_eq!(events.len(), 4);
    }
}

#[cfg(test)]
mod filter_tool_calls_tests {
    use super::filter_tool_calls::{FilterToolCallsConfig, FilterToolCallsMiddleware};
    use super::*;
    use agui_rs_core::{
        factory, BaseEventFields, ToolCallArgsEvent, ToolCallEndEvent, ToolCallResultEvent,
        ToolCallStartEvent,
    };
    use futures::stream;

    fn base_input() -> MiddlewareInput {
        MiddlewareInput::from(RunAgentInput::new("thread-filter", "run-filter"))
    }

    fn tool_events(tool_call_id: &str, tool_name: &str) -> Vec<Event> {
        vec![
            Event::ToolCallStart(ToolCallStartEvent {
                tool_call_id: tool_call_id.into(),
                tool_call_name: tool_name.into(),
                parent_message_id: None,
                base: BaseEventFields::default(),
            }),
            Event::ToolCallArgs(ToolCallArgsEvent {
                tool_call_id: tool_call_id.into(),
                delta: "{\"x\":1}".into(),
                base: BaseEventFields::default(),
            }),
            Event::ToolCallEnd(ToolCallEndEvent {
                tool_call_id: tool_call_id.into(),
                base: BaseEventFields::default(),
            }),
            Event::ToolCallResult(ToolCallResultEvent {
                message_id: format!("msg-{tool_call_id}"),
                tool_call_id: tool_call_id.into(),
                content: format!("result-{tool_name}"),
                role: None,
                base: BaseEventFields::default(),
            }),
        ]
    }

    async fn collect_events(mut stream: EventStream) -> Vec<Event> {
        let mut events = Vec::new();
        while let Some(item) = stream.next().await {
            events.push(item.expect("stream item"));
        }
        events
    }

    async fn run_filter(middleware: FilterToolCallsMiddleware, events: Vec<Event>) -> Vec<Event> {
        let terminal: TerminalFn = Arc::new(move |_input| {
            let events = events.clone();
            Box::pin(async move {
                Ok(Box::pin(stream::iter(events.into_iter().map(Ok))) as EventStream)
            })
        });
        let chain = chain_middlewares(vec![Arc::new(middleware)], terminal);
        collect_events(chain(base_input()).await.expect("filtered stream")).await
    }

    #[tokio::test]
    async fn drops_disallowed_tool_calls_and_followups() {
        let middleware = FilterToolCallsMiddleware::new(FilterToolCallsConfig::disallowed(vec![
            "blocked".into(),
        ]));
        let mut events = vec![factory::run_started("thread-filter", "run-filter")];
        events.extend(tool_events("blocked-1", "blocked"));
        events.extend(tool_events("allowed-1", "allowed"));

        let filtered = run_filter(middleware, events).await;

        assert_eq!(filtered.len(), 5);
        assert!(filtered.iter().all(|event| {
            !matches!(event, Event::ToolCallStart(e) if e.tool_call_name == "blocked")
                && !matches!(event, Event::ToolCallArgs(e) if e.tool_call_id == "blocked-1")
                && !matches!(event, Event::ToolCallEnd(e) if e.tool_call_id == "blocked-1")
                && !matches!(event, Event::ToolCallResult(e) if e.tool_call_id == "blocked-1")
        }));
    }

    #[tokio::test]
    async fn passes_unmatched_disallowed_tool_calls() {
        let middleware =
            FilterToolCallsMiddleware::new(FilterToolCallsConfig::disallowed(vec!["other".into()]));
        let filtered = run_filter(middleware, tool_events("allowed-1", "allowed")).await;

        assert_eq!(filtered.len(), 4);
    }

    #[tokio::test]
    async fn allowed_list_filters_out_non_members() {
        let middleware =
            FilterToolCallsMiddleware::new(FilterToolCallsConfig::allowed(vec!["keep".into()]));
        let mut events = tool_events("keep-1", "keep");
        events.extend(tool_events("drop-1", "drop"));

        let filtered = run_filter(middleware, events).await;

        assert_eq!(filtered.len(), 4);
        assert!(filtered
            .iter()
            .all(|event| !matches!(event, Event::ToolCallStart(e) if e.tool_call_name == "drop")));
    }

    #[tokio::test]
    async fn regex_like_patterns_are_treated_literally() {
        let middleware = FilterToolCallsMiddleware::new(FilterToolCallsConfig::disallowed(vec![
            "calc_.*".into(),
        ]));
        let filtered = run_filter(middleware, tool_events("calc-1", "calc_sum")).await;

        assert_eq!(filtered.len(), 4);
    }

    #[tokio::test]
    async fn empty_disallowed_filter_keeps_all_events() {
        let middleware =
            FilterToolCallsMiddleware::new(FilterToolCallsConfig::disallowed(Vec::new()));
        let filtered = run_filter(middleware, tool_events("allowed-1", "allowed")).await;

        assert_eq!(filtered.len(), 4);
    }

    #[test]
    fn config_validation_rejects_both_or_neither_lists() {
        let both = FilterToolCallsConfig::new(Some(vec!["a".into()]), Some(vec!["b".into()]));
        let neither = FilterToolCallsConfig::new(None, None);

        assert!(both.is_err());
        assert!(neither.is_err());
    }
}

#[cfg(test)]
mod backward_compat_tests {
    use super::backward_compat::{
        BackwardCompat0_0_39, BackwardCompat0_0_45, BackwardCompat0_0_47,
    };
    use super::*;
    use agui_rs_core::types::{AssistantMessage, DeveloperMessage, SystemMessage, UserMessage};
    use agui_rs_core::{
        factory, BaseEventFields, BinaryInputContent, InputContent, InputContentSource, Message,
        ReasoningMessageRole, ThinkingEndEvent, ThinkingStartEvent,
        ThinkingTextMessageContentEvent, ThinkingTextMessageEndEvent,
        ThinkingTextMessageStartEvent, UserMessageContent,
    };
    use futures::stream;
    use std::sync::Mutex;

    fn base_input() -> MiddlewareInput {
        MiddlewareInput::new(
            RunAgentInput::new("thread-backward", "run-backward"),
            Value::Object(Map::new()),
        )
    }

    async fn collect_events(mut stream: EventStream) -> Vec<Event> {
        let mut events = Vec::new();
        while let Some(item) = stream.next().await {
            events.push(item.expect("stream item"));
        }
        events
    }

    fn capture_terminal(captured: Arc<Mutex<Option<MiddlewareInput>>>) -> TerminalFn {
        Arc::new(move |input| {
            let captured = Arc::clone(&captured);
            Box::pin(async move {
                *captured.lock().expect("capture lock") = Some(input);
                Ok(Box::pin(stream::empty()) as EventStream)
            })
        })
    }

    fn events_terminal(events: Vec<Event>) -> TerminalFn {
        Arc::new(move |_input| {
            let events = events.clone();
            Box::pin(async move {
                Ok(Box::pin(stream::iter(events.into_iter().map(Ok))) as EventStream)
            })
        })
    }

    #[tokio::test]
    async fn backward_compat_0_0_39_removes_parent_run_id() {
        let captured = Arc::new(Mutex::new(None));
        let middleware = Arc::new(BackwardCompat0_0_39);
        let chain = chain_middlewares(vec![middleware], capture_terminal(Arc::clone(&captured)));
        let mut input = base_input();
        input.run_agent_input.parent_run_id = Some("parent-1".into());

        let _ = chain(input).await.expect("run should succeed");

        assert_eq!(
            captured
                .lock()
                .expect("capture lock")
                .clone()
                .expect("captured input")
                .run_agent_input
                .parent_run_id,
            None
        );
    }

    #[tokio::test]
    async fn backward_compat_0_0_39_concatenates_text_parts() {
        let captured = Arc::new(Mutex::new(None));
        let middleware = Arc::new(BackwardCompat0_0_39);
        let chain = chain_middlewares(vec![middleware], capture_terminal(Arc::clone(&captured)));
        let mut input = base_input();
        input.run_agent_input.messages = vec![Message::User(UserMessage {
            id: "user-1".into(),
            content: UserMessageContent::Parts(vec![
                InputContent::Text {
                    text: "hello".into(),
                },
                InputContent::Image {
                    source: InputContentSource::Url {
                        value: "https://example.com/image.png".into(),
                        mime_type: Some("image/png".into()),
                    },
                    metadata: None,
                },
                InputContent::Text {
                    text: " world".into(),
                },
            ]),
            name: None,
            encrypted_value: None,
        })];

        let _ = chain(input).await.expect("run should succeed");

        let captured = captured
            .lock()
            .expect("capture lock")
            .clone()
            .expect("captured input");
        match &captured.run_agent_input.messages[0] {
            Message::User(UserMessage {
                content: UserMessageContent::Text(text),
                ..
            }) => assert_eq!(text, "hello world"),
            other => panic!("expected sanitized user message, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn backward_compat_0_0_39_non_text_parts_become_empty_string() {
        let captured = Arc::new(Mutex::new(None));
        let middleware = Arc::new(BackwardCompat0_0_39);
        let chain = chain_middlewares(vec![middleware], capture_terminal(Arc::clone(&captured)));
        let mut input = base_input();
        input.run_agent_input.messages = vec![Message::User(UserMessage {
            id: "user-2".into(),
            content: UserMessageContent::Parts(vec![InputContent::Document {
                source: InputContentSource::Url {
                    value: "https://example.com/file.pdf".into(),
                    mime_type: Some("application/pdf".into()),
                },
                metadata: None,
            }]),
            name: None,
            encrypted_value: None,
        })];

        let _ = chain(input).await.expect("run should succeed");

        let captured = captured
            .lock()
            .expect("capture lock")
            .clone()
            .expect("captured input");
        match &captured.run_agent_input.messages[0] {
            Message::User(UserMessage {
                content: UserMessageContent::Text(text),
                ..
            }) => assert!(text.is_empty()),
            other => panic!("expected sanitized user message, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn backward_compat_0_0_45_maps_thinking_start_and_end() {
        let middleware = Arc::new(BackwardCompat0_0_45);
        let chain = chain_middlewares(
            vec![middleware],
            events_terminal(vec![
                Event::ThinkingStart(ThinkingStartEvent {
                    title: Some("legacy".into()),
                    base: BaseEventFields::default(),
                }),
                Event::ThinkingEnd(ThinkingEndEvent {
                    base: BaseEventFields::default(),
                }),
            ]),
        );

        let events = collect_events(chain(base_input()).await.expect("run should succeed")).await;

        match (&events[0], &events[1]) {
            (Event::ReasoningStart(start), Event::ReasoningEnd(end)) => {
                assert!(!start.message_id.is_empty());
                assert_eq!(start.message_id, end.message_id);
            }
            other => panic!("expected reasoning events, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn backward_compat_0_0_45_maps_thinking_message_lifecycle() {
        let middleware = Arc::new(BackwardCompat0_0_45);
        let chain = chain_middlewares(
            vec![middleware],
            events_terminal(vec![
                Event::ThinkingTextMessageStart(ThinkingTextMessageStartEvent {
                    base: BaseEventFields::default(),
                }),
                Event::ThinkingTextMessageContent(ThinkingTextMessageContentEvent {
                    delta: "hello".into(),
                    base: BaseEventFields::default(),
                }),
                Event::ThinkingTextMessageEnd(ThinkingTextMessageEndEvent {
                    base: BaseEventFields::default(),
                }),
            ]),
        );

        let events = collect_events(chain(base_input()).await.expect("run should succeed")).await;

        match (&events[0], &events[1], &events[2]) {
            (
                Event::ReasoningMessageStart(start),
                Event::ReasoningMessageContent(content),
                Event::ReasoningMessageEnd(end),
            ) => {
                assert_eq!(start.role, ReasoningMessageRole::Reasoning);
                assert_eq!(start.message_id, content.message_id);
                assert_eq!(content.message_id, end.message_id);
                assert_eq!(content.delta, "hello");
            }
            other => panic!("expected reasoning message events, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn backward_compat_0_0_45_generates_message_id_without_start() {
        let middleware = Arc::new(BackwardCompat0_0_45);
        let chain = chain_middlewares(
            vec![middleware],
            events_terminal(vec![Event::ThinkingTextMessageContent(
                ThinkingTextMessageContentEvent {
                    delta: "orphan".into(),
                    base: BaseEventFields::default(),
                },
            )]),
        );

        let events = collect_events(chain(base_input()).await.expect("run should succeed")).await;

        match &events[0] {
            Event::ReasoningMessageContent(content) => {
                assert_eq!(content.delta, "orphan");
                assert!(!content.message_id.is_empty());
            }
            other => panic!("expected reasoning message content, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn backward_compat_0_0_45_passthrough_non_legacy_events() {
        let middleware = Arc::new(BackwardCompat0_0_45);
        let original = factory::text_message_start("msg-1");
        let chain = chain_middlewares(vec![middleware], events_terminal(vec![original.clone()]));

        let events = collect_events(chain(base_input()).await.expect("run should succeed")).await;
        assert_eq!(events, vec![original]);
    }

    #[tokio::test]
    async fn backward_compat_0_0_47_converts_binary_data_to_image() {
        let captured = Arc::new(Mutex::new(None));
        let middleware = Arc::new(BackwardCompat0_0_47);
        let chain = chain_middlewares(vec![middleware], capture_terminal(Arc::clone(&captured)));
        let mut input = base_input();
        input.run_agent_input.messages = vec![Message::User(UserMessage {
            id: "user-image".into(),
            content: UserMessageContent::Parts(vec![InputContent::Binary {
                content: BinaryInputContent {
                    mime_type: "image/png".into(),
                    id: None,
                    url: None,
                    data: Some("Zm9v".into()),
                    filename: Some("image.png".into()),
                },
            }]),
            name: None,
            encrypted_value: None,
        })];

        let _ = chain(input).await.expect("run should succeed");

        let captured = captured
            .lock()
            .expect("capture lock")
            .clone()
            .expect("captured input");
        match &captured.run_agent_input.messages[0] {
            Message::User(UserMessage {
                content: UserMessageContent::Parts(parts),
                ..
            }) => match &parts[0] {
                InputContent::Image { source, metadata } => {
                    assert_eq!(
                        source,
                        &InputContentSource::Data {
                            value: "Zm9v".into(),
                            mime_type: "image/png".into(),
                        }
                    );
                    assert_eq!(
                        metadata,
                        &Some(Value::Object(Map::from_iter([(
                            "filename".to_string(),
                            Value::String("image.png".into()),
                        )])))
                    );
                }
                other => panic!("expected image content, got {other:?}"),
            },
            other => panic!("expected user message, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn backward_compat_0_0_47_converts_binary_url_to_audio() {
        let captured = Arc::new(Mutex::new(None));
        let middleware = Arc::new(BackwardCompat0_0_47);
        let chain = chain_middlewares(vec![middleware], capture_terminal(Arc::clone(&captured)));
        let mut input = base_input();
        input.run_agent_input.messages = vec![Message::User(UserMessage {
            id: "user-audio".into(),
            content: UserMessageContent::Parts(vec![InputContent::Binary {
                content: BinaryInputContent {
                    mime_type: "audio/mpeg".into(),
                    id: None,
                    url: Some("https://example.com/audio.mp3".into()),
                    data: None,
                    filename: None,
                },
            }]),
            name: None,
            encrypted_value: None,
        })];

        let _ = chain(input).await.expect("run should succeed");

        let captured = captured
            .lock()
            .expect("capture lock")
            .clone()
            .expect("captured input");
        match &captured.run_agent_input.messages[0] {
            Message::User(UserMessage {
                content: UserMessageContent::Parts(parts),
                ..
            }) => match &parts[0] {
                InputContent::Audio { source, metadata } => {
                    assert_eq!(
                        source,
                        &InputContentSource::Url {
                            value: "https://example.com/audio.mp3".into(),
                            mime_type: Some("audio/mpeg".into()),
                        }
                    );
                    assert_eq!(metadata, &None);
                }
                other => panic!("expected audio content, got {other:?}"),
            },
            other => panic!("expected user message, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn backward_compat_0_0_47_converts_unknown_binary_to_document() {
        let captured = Arc::new(Mutex::new(None));
        let middleware = Arc::new(BackwardCompat0_0_47);
        let chain = chain_middlewares(vec![middleware], capture_terminal(Arc::clone(&captured)));
        let mut input = base_input();
        input.run_agent_input.messages = vec![Message::User(UserMessage {
            id: "user-doc".into(),
            content: UserMessageContent::Parts(vec![InputContent::Binary {
                content: BinaryInputContent {
                    mime_type: "application/pdf".into(),
                    id: None,
                    url: None,
                    data: Some("cGRm".into()),
                    filename: None,
                },
            }]),
            name: None,
            encrypted_value: None,
        })];

        let _ = chain(input).await.expect("run should succeed");

        let captured = captured
            .lock()
            .expect("capture lock")
            .clone()
            .expect("captured input");
        match &captured.run_agent_input.messages[0] {
            Message::User(UserMessage {
                content: UserMessageContent::Parts(parts),
                ..
            }) => assert!(matches!(parts[0], InputContent::Document { .. })),
            other => panic!("expected user message, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn backward_compat_0_0_47_keeps_id_only_binary_as_is() {
        let captured = Arc::new(Mutex::new(None));
        let middleware = Arc::new(BackwardCompat0_0_47);
        let chain = chain_middlewares(vec![middleware], capture_terminal(Arc::clone(&captured)));
        let mut input = base_input();
        input.run_agent_input.messages = vec![
            Message::Developer(DeveloperMessage {
                id: "dev-1".into(),
                content: "system".into(),
                name: None,
                encrypted_value: None,
            }),
            Message::System(SystemMessage {
                id: "sys-1".into(),
                content: "prompt".into(),
                name: None,
                encrypted_value: None,
            }),
            Message::Assistant(AssistantMessage {
                id: "assistant-1".into(),
                content: Some("ok".into()),
                name: None,
                tool_calls: None,
                encrypted_value: None,
            }),
            Message::User(UserMessage {
                id: "user-id-only".into(),
                content: UserMessageContent::Parts(vec![InputContent::Binary {
                    content: BinaryInputContent {
                        mime_type: "application/octet-stream".into(),
                        id: Some("blob-1".into()),
                        url: None,
                        data: None,
                        filename: Some("blob.bin".into()),
                    },
                }]),
                name: None,
                encrypted_value: None,
            }),
        ];

        let _ = chain(input).await.expect("run should succeed");

        let captured = captured
            .lock()
            .expect("capture lock")
            .clone()
            .expect("captured input");
        assert!(matches!(
            captured.run_agent_input.messages[0],
            Message::Developer(_)
        ));
        assert!(matches!(
            captured.run_agent_input.messages[1],
            Message::System(_)
        ));
        assert!(matches!(
            captured.run_agent_input.messages[2],
            Message::Assistant(_)
        ));
        match &captured.run_agent_input.messages[3] {
            Message::User(UserMessage {
                content: UserMessageContent::Parts(parts),
                ..
            }) => match &parts[0] {
                InputContent::Binary { content } => {
                    assert_eq!(content.id.as_deref(), Some("blob-1"));
                    assert_eq!(content.filename.as_deref(), Some("blob.bin"));
                }
                other => panic!("expected binary content, got {other:?}"),
            },
            other => panic!("expected user message, got {other:?}"),
        }
    }
}
