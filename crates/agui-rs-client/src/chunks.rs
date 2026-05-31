use agui_rs_core::{
    AgUiError, BaseEventFields, Event, ReasoningMessageChunkEvent, ReasoningMessageContentEvent,
    ReasoningMessageEndEvent, ReasoningMessageRole, ReasoningMessageStartEvent, Result,
    TextMessageChunkEvent, TextMessageContentEvent, TextMessageEndEvent, TextMessageRole,
    TextMessageStartEvent, ToolCallArgsEvent, ToolCallChunkEvent, ToolCallEndEvent,
    ToolCallStartEvent,
};
use async_stream::try_stream;
use futures::{stream::BoxStream, Stream, StreamExt};

/// The single chunk stream currently being expanded.
///
/// Mirrors the `mode` state machine in the TypeScript `transformChunks`: at
/// most one of text / tool-call / reasoning is open at a time, and opening a
/// new one (or any structured non-chunk event) first closes the active one.
#[derive(Debug, Clone)]
enum OpenChunk {
    Text { message_id: String },
    Tool { tool_call_id: String },
    Reasoning { message_id: String },
}

impl OpenChunk {
    /// Produces the `*_END` event that closes this open chunk stream.
    fn close(self) -> Event {
        match self {
            OpenChunk::Text { message_id } => Event::TextMessageEnd(TextMessageEndEvent {
                message_id,
                base: BaseEventFields::default(),
            }),
            OpenChunk::Tool { tool_call_id } => Event::ToolCallEnd(ToolCallEndEvent {
                tool_call_id,
                base: BaseEventFields::default(),
            }),
            OpenChunk::Reasoning { message_id } => {
                Event::ReasoningMessageEnd(ReasoningMessageEndEvent {
                    message_id,
                    base: BaseEventFields::default(),
                })
            }
        }
    }
}

pub fn expand_chunks<S>(stream: S) -> BoxStream<'static, Result<Event>>
where
    S: Stream<Item = Result<Event>> + Send + 'static,
{
    Box::pin(try_stream! {
        let mut stream = stream.boxed();
        let mut open: Option<OpenChunk> = None;

        while let Some(item) = stream.next().await {
            let event = item?;
            match event {
                Event::TextMessageChunk(chunk) => {
                    for event in expand_text_chunk(&mut open, chunk)? {
                        yield event;
                    }
                }
                Event::ToolCallChunk(chunk) => {
                    for event in expand_tool_chunk(&mut open, chunk)? {
                        yield event;
                    }
                }
                Event::ReasoningMessageChunk(chunk) => {
                    for event in expand_reasoning_chunk(&mut open, chunk)? {
                        yield event;
                    }
                }
                // These events pass through without disturbing an open chunk
                // stream, matching the dedicated passthrough arm in TS.
                Event::Raw(_)
                | Event::ActivitySnapshot(_)
                | Event::ActivityDelta(_)
                | Event::ReasoningEncryptedValue(_) => yield event,
                // Every other structured event (including TOOL_CALL_RESULT and
                // CUSTOM) closes any pending chunk stream first.
                other => {
                    if let Some(pending) = open.take() {
                        yield pending.close();
                    }
                    yield other;
                }
            }
        }

        if let Some(pending) = open.take() {
            yield pending.close();
        }
    })
}

fn expand_text_chunk(
    open: &mut Option<OpenChunk>,
    chunk: TextMessageChunkEvent,
) -> Result<Vec<Event>> {
    let mut events = Vec::new();

    // Close the active stream when it isn't this text message, or when a
    // different message id arrives. A same-id / id-less chunk continues it.
    let id_switch = matches!(
        (open.as_ref(), chunk.message_id.as_deref()),
        (Some(OpenChunk::Text { message_id }), Some(incoming)) if message_id != incoming
    );
    if !matches!(open, Some(OpenChunk::Text { .. })) || id_switch {
        if let Some(pending) = open.take() {
            events.push(pending.close());
        }
    }

    if !matches!(open, Some(OpenChunk::Text { .. })) {
        let message_id = chunk
            .message_id
            .ok_or_else(|| AgUiError::validation("first TEXT_MESSAGE_CHUNK must include message_id"))?;
        *open = Some(OpenChunk::Text {
            message_id: message_id.clone(),
        });
        events.push(Event::TextMessageStart(TextMessageStartEvent {
            message_id,
            role: chunk.role.unwrap_or(TextMessageRole::Assistant),
            name: chunk.name,
            base: BaseEventFields::default(),
        }));
    }

    if let Some(delta) = chunk.delta {
        let Some(OpenChunk::Text { message_id }) = open.as_ref() else {
            unreachable!("text chunk just opened or continued a text message");
        };
        events.push(Event::TextMessageContent(TextMessageContentEvent {
            message_id: message_id.clone(),
            delta,
            base: BaseEventFields::default(),
        }));
    }

    Ok(events)
}

fn expand_tool_chunk(
    open: &mut Option<OpenChunk>,
    chunk: ToolCallChunkEvent,
) -> Result<Vec<Event>> {
    let mut events = Vec::new();

    let id_switch = matches!(
        (open.as_ref(), chunk.tool_call_id.as_deref()),
        (Some(OpenChunk::Tool { tool_call_id }), Some(incoming)) if tool_call_id != incoming
    );
    if !matches!(open, Some(OpenChunk::Tool { .. })) || id_switch {
        if let Some(pending) = open.take() {
            events.push(pending.close());
        }
    }

    if !matches!(open, Some(OpenChunk::Tool { .. })) {
        let tool_call_id = chunk
            .tool_call_id
            .ok_or_else(|| AgUiError::validation("first TOOL_CALL_CHUNK must include tool_call_id"))?;
        let tool_call_name = chunk.tool_call_name.ok_or_else(|| {
            AgUiError::validation("first TOOL_CALL_CHUNK must include tool_call_name")
        })?;
        *open = Some(OpenChunk::Tool {
            tool_call_id: tool_call_id.clone(),
        });
        events.push(Event::ToolCallStart(ToolCallStartEvent {
            tool_call_id,
            tool_call_name,
            parent_message_id: chunk.parent_message_id,
            base: BaseEventFields::default(),
        }));
    }

    if let Some(delta) = chunk.delta {
        let Some(OpenChunk::Tool { tool_call_id }) = open.as_ref() else {
            unreachable!("tool chunk just opened or continued a tool call");
        };
        events.push(Event::ToolCallArgs(ToolCallArgsEvent {
            tool_call_id: tool_call_id.clone(),
            delta,
            base: BaseEventFields::default(),
        }));
    }

    Ok(events)
}

fn expand_reasoning_chunk(
    open: &mut Option<OpenChunk>,
    chunk: ReasoningMessageChunkEvent,
) -> Result<Vec<Event>> {
    let mut events = Vec::new();

    let id_switch = matches!(
        (open.as_ref(), chunk.message_id.as_deref()),
        (Some(OpenChunk::Reasoning { message_id }), Some(incoming)) if message_id != incoming
    );
    if !matches!(open, Some(OpenChunk::Reasoning { .. })) || id_switch {
        if let Some(pending) = open.take() {
            events.push(pending.close());
        }
    }

    if !matches!(open, Some(OpenChunk::Reasoning { .. })) {
        let message_id = chunk.message_id.ok_or_else(|| {
            AgUiError::validation("first REASONING_MESSAGE_CHUNK must include message_id")
        })?;
        *open = Some(OpenChunk::Reasoning {
            message_id: message_id.clone(),
        });
        events.push(Event::ReasoningMessageStart(ReasoningMessageStartEvent {
            message_id,
            role: ReasoningMessageRole::Reasoning,
            base: BaseEventFields::default(),
        }));
    }

    if let Some(delta) = chunk.delta {
        let Some(OpenChunk::Reasoning { message_id }) = open.as_ref() else {
            unreachable!("reasoning chunk just opened or continued a reasoning message");
        };
        events.push(Event::ReasoningMessageContent(ReasoningMessageContentEvent {
            message_id: message_id.clone(),
            delta,
            base: BaseEventFields::default(),
        }));
    }

    Ok(events)
}

#[cfg(test)]
mod tests {
    use super::*;
    use agui_rs_core::{
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
            let events = collect_ok(vec![Event::ReasoningMessageChunk(
                ReasoningMessageChunkEvent {
                    message_id: Some("r1".into()),
                    delta: Some("plan".into()),
                    base: BaseEventFields::default(),
                },
            )])
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
