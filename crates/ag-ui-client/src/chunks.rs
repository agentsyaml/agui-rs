use ag_ui_core::{
    BaseEventFields, Event, ReasoningMessageChunkEvent, ReasoningMessageContentEvent,
    ReasoningMessageEndEvent, ReasoningMessageRole, ReasoningMessageStartEvent, Result,
    TextMessageChunkEvent, TextMessageContentEvent, TextMessageEndEvent, TextMessageRole,
    TextMessageStartEvent, ToolCallArgsEvent, ToolCallChunkEvent, ToolCallEndEvent,
    ToolCallStartEvent,
};
use async_stream::try_stream;
use futures::{stream::BoxStream, Stream, StreamExt};

#[derive(Debug, Clone)]
struct OpenTextMessage {
    message_id: String,
}

#[derive(Debug, Clone)]
struct OpenToolCall {
    tool_call_id: String,
}

#[derive(Debug, Clone)]
struct OpenReasoningMessage {
    message_id: String,
}

pub fn expand_chunks<S>(stream: S) -> BoxStream<'static, Result<Event>>
where
    S: Stream<Item = Result<Event>> + Send + 'static,
{
    Box::pin(try_stream! {
        let mut stream = stream.boxed();
        let mut open_text: Option<OpenTextMessage> = None;
        let mut open_tool: Option<OpenToolCall> = None;
        let mut open_reasoning: Option<OpenReasoningMessage> = None;

        while let Some(item) = stream.next().await {
            let event = item?;
            match event {
                Event::TextMessageChunk(chunk) => {
                    for event in expand_text_chunk(
                        &mut open_text,
                        &mut open_tool,
                        &mut open_reasoning,
                        chunk,
                    )? {
                        yield event;
                    }
                }
                Event::ToolCallChunk(chunk) => {
                    for event in expand_tool_chunk(
                        &mut open_text,
                        &mut open_tool,
                        &mut open_reasoning,
                        chunk,
                    )? {
                        yield event;
                    }
                }
                Event::ReasoningMessageChunk(chunk) => {
                    for event in expand_reasoning_chunk(
                        &mut open_text,
                        &mut open_tool,
                        &mut open_reasoning,
                        chunk,
                    )? {
                        yield event;
                    }
                }
                other => {
                    if let Some(end) = close_pending_for_non_chunk(
                        &mut open_text,
                        &mut open_tool,
                        &mut open_reasoning,
                        &other,
                    ) {
                        yield end;
                    }
                    yield other;
                }
            }
        }

        if let Some(text) = open_text.take() {
            yield Event::TextMessageEnd(TextMessageEndEvent {
                message_id: text.message_id,
                base: BaseEventFields::default(),
            });
        }
        if let Some(tool) = open_tool.take() {
            yield Event::ToolCallEnd(ToolCallEndEvent {
                tool_call_id: tool.tool_call_id,
                base: BaseEventFields::default(),
            });
        }
        if let Some(reasoning) = open_reasoning.take() {
            yield Event::ReasoningMessageEnd(ReasoningMessageEndEvent {
                message_id: reasoning.message_id,
                base: BaseEventFields::default(),
            });
        }
    })
}

fn close_pending_for_non_chunk(
    open_text: &mut Option<OpenTextMessage>,
    open_tool: &mut Option<OpenToolCall>,
    open_reasoning: &mut Option<OpenReasoningMessage>,
    event: &Event,
) -> Option<Event> {
    match event {
        Event::Raw(_)
        | Event::Custom(_)
        | Event::ToolCallResult(_)
        | Event::ActivitySnapshot(_)
        | Event::ActivityDelta(_)
        | Event::ReasoningEncryptedValue(_) => None,
        _ => {
            if let Some(text) = open_text.take() {
                return Some(Event::TextMessageEnd(TextMessageEndEvent {
                    message_id: text.message_id,
                    base: BaseEventFields::default(),
                }));
            }
            if let Some(tool) = open_tool.take() {
                return Some(Event::ToolCallEnd(ToolCallEndEvent {
                    tool_call_id: tool.tool_call_id,
                    base: BaseEventFields::default(),
                }));
            }
            if let Some(reasoning) = open_reasoning.take() {
                return Some(Event::ReasoningMessageEnd(ReasoningMessageEndEvent {
                    message_id: reasoning.message_id,
                    base: BaseEventFields::default(),
                }));
            }
            None
        }
    }
}

fn expand_text_chunk(
    open_text: &mut Option<OpenTextMessage>,
    open_tool: &mut Option<OpenToolCall>,
    open_reasoning: &mut Option<OpenReasoningMessage>,
    chunk: TextMessageChunkEvent,
) -> Result<Vec<Event>> {
    let mut events = Vec::new();

    if open_tool.is_some() {
        let tool = open_tool.take().ok_or_else(|| ag_ui_core::AgUiError::protocol("tool call state missing"))?;
        events.push(Event::ToolCallEnd(ToolCallEndEvent {
            tool_call_id: tool.tool_call_id,
            base: BaseEventFields::default(),
        }));
    }
    if open_reasoning.is_some() {
        let reasoning = open_reasoning
            .take()
            .ok_or_else(|| ag_ui_core::AgUiError::protocol("reasoning message state missing"))?;
        events.push(Event::ReasoningMessageEnd(ReasoningMessageEndEvent {
            message_id: reasoning.message_id,
            base: BaseEventFields::default(),
        }));
    }

    let message_id = chunk.message_id.clone();
    let delta = chunk.delta.clone();

    let should_switch_message = match (&open_text, message_id.as_deref()) {
        (Some(open), Some(message_id)) if open.message_id != message_id => true,
        (Some(_), None) if delta.is_none() => true,
        _ => false,
    };

    if should_switch_message {
        let current = open_text.take().ok_or_else(|| ag_ui_core::AgUiError::protocol("text message state missing"))?;
        events.push(Event::TextMessageEnd(TextMessageEndEvent {
            message_id: current.message_id,
            base: BaseEventFields::default(),
        }));
    }

    if open_text.is_none() {
        if let Some(message_id) = message_id {
            open_text.replace(OpenTextMessage {
                message_id: message_id.clone(),
            });
            events.push(Event::TextMessageStart(TextMessageStartEvent {
                message_id,
                role: chunk.role.unwrap_or(TextMessageRole::Assistant),
                name: chunk.name,
                base: BaseEventFields::default(),
            }));
        }
    }

    if let Some(delta) = delta {
        let message_id = open_text
            .as_ref()
            .map(|open| open.message_id.clone())
            .ok_or_else(|| ag_ui_core::AgUiError::validation("first TEXT_MESSAGE_CHUNK must include message_id"))?;
        events.push(Event::TextMessageContent(TextMessageContentEvent {
            message_id,
            delta,
            base: BaseEventFields::default(),
        }));
    }

    Ok(events)
}

fn expand_tool_chunk(
    open_text: &mut Option<OpenTextMessage>,
    open_tool: &mut Option<OpenToolCall>,
    open_reasoning: &mut Option<OpenReasoningMessage>,
    chunk: ToolCallChunkEvent,
) -> Result<Vec<Event>> {
    let mut events = Vec::new();

    if open_text.is_some() {
        let text = open_text.take().ok_or_else(|| ag_ui_core::AgUiError::protocol("text message state missing"))?;
        events.push(Event::TextMessageEnd(TextMessageEndEvent {
            message_id: text.message_id,
            base: BaseEventFields::default(),
        }));
    }
    if open_reasoning.is_some() {
        let reasoning = open_reasoning
            .take()
            .ok_or_else(|| ag_ui_core::AgUiError::protocol("reasoning message state missing"))?;
        events.push(Event::ReasoningMessageEnd(ReasoningMessageEndEvent {
            message_id: reasoning.message_id,
            base: BaseEventFields::default(),
        }));
    }

    let tool_call_id = chunk.tool_call_id.clone();
    let delta = chunk.delta.clone();

    let should_switch_tool = match (&open_tool, tool_call_id.as_deref()) {
        (Some(open), Some(tool_call_id)) if open.tool_call_id != tool_call_id => true,
        (Some(_), None) if delta.is_none() => true,
        _ => false,
    };

    if should_switch_tool {
        let current = open_tool.take().ok_or_else(|| ag_ui_core::AgUiError::protocol("tool call state missing"))?;
        events.push(Event::ToolCallEnd(ToolCallEndEvent {
            tool_call_id: current.tool_call_id,
            base: BaseEventFields::default(),
        }));
    }

    if open_tool.is_none() {
        let tool_call_id = tool_call_id
            .ok_or_else(|| ag_ui_core::AgUiError::validation("first TOOL_CALL_CHUNK must include tool_call_id"))?;
        let tool_call_name = chunk
            .tool_call_name
            .clone()
            .ok_or_else(|| ag_ui_core::AgUiError::validation("first TOOL_CALL_CHUNK must include tool_call_name"))?;
        open_tool.replace(OpenToolCall {
            tool_call_id: tool_call_id.clone(),
        });
        events.push(Event::ToolCallStart(ToolCallStartEvent {
            tool_call_id,
            tool_call_name,
            parent_message_id: chunk.parent_message_id,
            base: BaseEventFields::default(),
        }));
    }

    if let Some(delta) = delta {
        let tool_call_id = open_tool
            .as_ref()
            .map(|open| open.tool_call_id.clone())
            .ok_or_else(|| ag_ui_core::AgUiError::validation("TOOL_CALL_CHUNK received without active tool call"))?;
        events.push(Event::ToolCallArgs(ToolCallArgsEvent {
            tool_call_id,
            delta,
            base: BaseEventFields::default(),
        }));
    }

    Ok(events)
}

fn expand_reasoning_chunk(
    open_text: &mut Option<OpenTextMessage>,
    open_tool: &mut Option<OpenToolCall>,
    open_reasoning: &mut Option<OpenReasoningMessage>,
    chunk: ReasoningMessageChunkEvent,
) -> Result<Vec<Event>> {
    let mut events = Vec::new();

    if open_text.is_some() {
        let text = open_text
            .take()
            .ok_or_else(|| ag_ui_core::AgUiError::protocol("text message state missing"))?;
        events.push(Event::TextMessageEnd(TextMessageEndEvent {
            message_id: text.message_id,
            base: BaseEventFields::default(),
        }));
    }
    if open_tool.is_some() {
        let tool = open_tool
            .take()
            .ok_or_else(|| ag_ui_core::AgUiError::protocol("tool call state missing"))?;
        events.push(Event::ToolCallEnd(ToolCallEndEvent {
            tool_call_id: tool.tool_call_id,
            base: BaseEventFields::default(),
        }));
    }

    let message_id = chunk.message_id.clone();
    let delta = chunk.delta.clone();

    let should_switch_message = match (&open_reasoning, message_id.as_deref()) {
        (Some(open), Some(message_id)) if open.message_id != message_id => true,
        (Some(_), None) if delta.is_none() => true,
        _ => false,
    };

    if should_switch_message {
        let current = open_reasoning
            .take()
            .ok_or_else(|| ag_ui_core::AgUiError::protocol("reasoning message state missing"))?;
        events.push(Event::ReasoningMessageEnd(ReasoningMessageEndEvent {
            message_id: current.message_id,
            base: BaseEventFields::default(),
        }));
    }

    if open_reasoning.is_none() {
        if let Some(message_id) = message_id {
            open_reasoning.replace(OpenReasoningMessage {
                message_id: message_id.clone(),
            });
            events.push(Event::ReasoningMessageStart(ReasoningMessageStartEvent {
                message_id,
                role: ReasoningMessageRole::Reasoning,
                base: BaseEventFields::default(),
            }));
        }
    }

    if let Some(delta) = delta {
        let message_id = open_reasoning
            .as_ref()
            .map(|open| open.message_id.clone())
            .ok_or_else(|| {
                ag_ui_core::AgUiError::validation(
                    "first REASONING_MESSAGE_CHUNK must include message_id",
                )
            })?;
        events.push(Event::ReasoningMessageContent(ReasoningMessageContentEvent {
            message_id,
            delta,
            base: BaseEventFields::default(),
        }));
    }

    Ok(events)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ag_ui_core::{
        factory, BaseEventFields, ReasoningEncryptedValueEvent, ReasoningEncryptedValueSubtype,
        ReasoningMessageChunkEvent, ToolCallChunkEvent,
    };
    use futures::stream;

    async fn collect_ok(events: Vec<Event>) -> Vec<Event> {
        expand_chunks(stream::iter(events.into_iter().map(Ok)))
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .collect::<Result<Vec<_>>>()
            .expect("chunk expansion should succeed")
    }

    #[tokio::test]
    async fn expands_single_text_message_chunk() {
        let events = collect_ok(vec![Event::TextMessageChunk(TextMessageChunkEvent {
            message_id: Some("m1".into()),
            role: Some(TextMessageRole::Assistant),
            delta: Some("hello".into()),
            name: None,
            base: BaseEventFields::default(),
        })])
        .await;

        assert!(matches!(events[0], Event::TextMessageStart(_)));
        assert!(matches!(events[1], Event::TextMessageContent(_)));
        assert!(matches!(events[2], Event::TextMessageEnd(_)));
    }

    #[tokio::test]
    async fn expands_multiple_text_message_chunks() {
        let events = collect_ok(vec![
            Event::TextMessageChunk(TextMessageChunkEvent {
                message_id: Some("m1".into()),
                role: Some(TextMessageRole::Assistant),
                delta: Some("hel".into()),
                name: None,
                base: BaseEventFields::default(),
            }),
            Event::TextMessageChunk(TextMessageChunkEvent {
                message_id: None,
                role: None,
                delta: Some("lo".into()),
                name: None,
                base: BaseEventFields::default(),
            }),
        ])
        .await;

        assert_eq!(events.len(), 4);
        assert!(matches!(events[0], Event::TextMessageStart(_)));
        assert!(matches!(events[1], Event::TextMessageContent(_)));
        assert!(matches!(events[2], Event::TextMessageContent(_)));
        assert!(matches!(events[3], Event::TextMessageEnd(_)));
    }

    #[tokio::test]
    async fn expands_two_text_messages_back_to_back() {
        let events = collect_ok(vec![
            Event::TextMessageChunk(TextMessageChunkEvent {
                message_id: Some("m1".into()),
                role: Some(TextMessageRole::Assistant),
                delta: Some("one".into()),
                name: None,
                base: BaseEventFields::default(),
            }),
            Event::TextMessageChunk(TextMessageChunkEvent {
                message_id: Some("m2".into()),
                role: Some(TextMessageRole::Assistant),
                delta: Some("two".into()),
                name: None,
                base: BaseEventFields::default(),
            }),
        ])
        .await;

        assert!(matches!(events[0], Event::TextMessageStart(_)));
        assert!(matches!(events[1], Event::TextMessageContent(_)));
        assert!(matches!(events[2], Event::TextMessageEnd(_)));
        assert!(matches!(events[3], Event::TextMessageStart(_)));
        assert!(matches!(events[4], Event::TextMessageContent(_)));
        assert!(matches!(events[5], Event::TextMessageEnd(_)));
    }

    #[tokio::test]
    async fn expands_single_tool_call_chunk() {
        let events = collect_ok(vec![Event::ToolCallChunk(ToolCallChunkEvent {
            tool_call_id: Some("tc1".into()),
            tool_call_name: Some("search".into()),
            parent_message_id: Some("m1".into()),
            delta: Some("{".into()),
            base: BaseEventFields::default(),
        })])
        .await;

        assert!(matches!(events[0], Event::ToolCallStart(_)));
        assert!(matches!(events[1], Event::ToolCallArgs(_)));
        assert!(matches!(events[2], Event::ToolCallEnd(_)));
    }

    #[tokio::test]
    async fn expands_multiple_tool_call_chunks() {
        let events = collect_ok(vec![
            Event::ToolCallChunk(ToolCallChunkEvent {
                tool_call_id: Some("tc1".into()),
                tool_call_name: Some("search".into()),
                parent_message_id: None,
                delta: Some("{\"q\":\"".into()),
                base: BaseEventFields::default(),
            }),
            Event::ToolCallChunk(ToolCallChunkEvent {
                tool_call_id: None,
                tool_call_name: None,
                parent_message_id: None,
                delta: Some("rust\"}".into()),
                base: BaseEventFields::default(),
            }),
        ])
        .await;

        assert_eq!(events.len(), 4);
        assert!(matches!(events[0], Event::ToolCallStart(_)));
        assert!(matches!(events[1], Event::ToolCallArgs(_)));
        assert!(matches!(events[2], Event::ToolCallArgs(_)));
        assert!(matches!(events[3], Event::ToolCallEnd(_)));
    }

    #[tokio::test]
    async fn passes_through_non_chunk_events() {
        let events = collect_ok(vec![factory::run_started("thread", "run")]).await;
        assert_eq!(events, vec![factory::run_started("thread", "run")]);
    }

    mod reasoning_chunks {
        use super::*;

        #[tokio::test]
        async fn expands_single_reasoning_chunk() {
            let events = collect_ok(vec![Event::ReasoningMessageChunk(ReasoningMessageChunkEvent {
                message_id: Some("r1".into()),
                delta: Some("plan".into()),
                base: BaseEventFields::default(),
            })])
            .await;

            assert!(matches!(events[0], Event::ReasoningMessageStart(_)));
            assert!(matches!(events[1], Event::ReasoningMessageContent(_)));
            assert!(matches!(events[2], Event::ReasoningMessageEnd(_)));
        }

        #[tokio::test]
        async fn expands_multiple_reasoning_chunks() {
            let events = collect_ok(vec![
                Event::ReasoningMessageChunk(ReasoningMessageChunkEvent {
                    message_id: Some("r1".into()),
                    delta: Some("pla".into()),
                    base: BaseEventFields::default(),
                }),
                Event::ReasoningMessageChunk(ReasoningMessageChunkEvent {
                    message_id: None,
                    delta: Some("n".into()),
                    base: BaseEventFields::default(),
                }),
            ])
            .await;

            assert_eq!(events.len(), 4);
            assert!(matches!(events[0], Event::ReasoningMessageStart(_)));
            assert!(matches!(events[1], Event::ReasoningMessageContent(_)));
            assert!(matches!(events[2], Event::ReasoningMessageContent(_)));
            assert!(matches!(events[3], Event::ReasoningMessageEnd(_)));
        }

        #[tokio::test]
        async fn closes_previous_reasoning_message_when_message_id_changes() {
            let events = collect_ok(vec![
                Event::ReasoningMessageChunk(ReasoningMessageChunkEvent {
                    message_id: Some("r1".into()),
                    delta: Some("one".into()),
                    base: BaseEventFields::default(),
                }),
                Event::ReasoningMessageChunk(ReasoningMessageChunkEvent {
                    message_id: Some("r2".into()),
                    delta: Some("two".into()),
                    base: BaseEventFields::default(),
                }),
            ])
            .await;

            assert!(matches!(events[0], Event::ReasoningMessageStart(_)));
            assert!(matches!(events[1], Event::ReasoningMessageContent(_)));
            assert!(matches!(events[2], Event::ReasoningMessageEnd(_)));
            assert!(matches!(events[3], Event::ReasoningMessageStart(_)));
            assert!(matches!(events[4], Event::ReasoningMessageContent(_)));
            assert!(matches!(events[5], Event::ReasoningMessageEnd(_)));
        }

        #[tokio::test]
        async fn reasoning_encrypted_value_does_not_close_open_reasoning_message() {
            let events = collect_ok(vec![
                Event::ReasoningMessageChunk(ReasoningMessageChunkEvent {
                    message_id: Some("r1".into()),
                    delta: Some("a".into()),
                    base: BaseEventFields::default(),
                }),
                Event::ReasoningEncryptedValue(ReasoningEncryptedValueEvent {
                    subtype: ReasoningEncryptedValueSubtype::Message,
                    entity_id: "r1".into(),
                    encrypted_value: "secret".into(),
                    base: BaseEventFields::default(),
                }),
                Event::ReasoningMessageChunk(ReasoningMessageChunkEvent {
                    message_id: None,
                    delta: Some("b".into()),
                    base: BaseEventFields::default(),
                }),
            ])
            .await;

            assert_eq!(events.len(), 5);
            assert!(matches!(events[0], Event::ReasoningMessageStart(_)));
            assert!(matches!(events[1], Event::ReasoningMessageContent(_)));
            assert!(matches!(events[2], Event::ReasoningEncryptedValue(_)));
            assert!(matches!(events[3], Event::ReasoningMessageContent(_)));
            assert!(matches!(events[4], Event::ReasoningMessageEnd(_)));
        }
    }
}
