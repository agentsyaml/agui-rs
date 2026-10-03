use agui_rs_core::{
    AgUiError, AttributableFields, BaseEventFields, Event, ReasoningMessageChunkEvent,
    ReasoningMessageContentEvent, ReasoningMessageEndEvent, ReasoningMessageRole,
    ReasoningMessageStartEvent, Result, TextMessageChunkEvent, TextMessageContentEvent,
    TextMessageEndEvent, TextMessageRole, TextMessageStartEvent, ToolCallArgsEvent,
    ToolCallChunkEvent, ToolCallEndEvent, ToolCallStartEvent,
};

use async_stream::try_stream;
use futures::{stream::BoxStream, Stream, StreamExt};

/// One chunk stream currently being assembled. A lane holds at most one,
/// because the chunk shorthand identifies a continuation only by "the same as
/// before". Kept fields let a continuation chunk that repeats one be judged
/// against what the stream already said.
#[derive(Debug, Clone)]
enum OpenChunk {
    Text {
        message_id: String,
        role: Option<TextMessageRole>,
        name: Option<String>,
    },
    Tool {
        tool_call_id: String,
        tool_call_name: String,
        parent_message_id: Option<String>,
    },
    Reasoning {
        message_id: String,
    },
}

impl OpenChunk {
    /// Produces the `*_END` event that closes this open chunk stream. The END
    /// carries the opener's subagent owner (`owner` lives on the lane).
    fn close(self, owner: AttributableFields) -> Event {
        match self {
            OpenChunk::Text { message_id, .. } => Event::TextMessageEnd(TextMessageEndEvent {
                message_id,
                base: BaseEventFields::default(),
                attributable: owner,
            }),
            OpenChunk::Tool { tool_call_id, .. } => Event::ToolCallEnd(ToolCallEndEvent {
                tool_call_id,
                base: BaseEventFields::default(),
                attributable: owner,
            }),
            OpenChunk::Reasoning { message_id } => {
                Event::ReasoningMessageEnd(ReasoningMessageEndEvent {
                    message_id,
                    base: BaseEventFields::default(),
                    attributable: owner,
                })
            }
        }
    }
}

type Lanes = Vec<(Option<String>, OpenChunk)>;

fn is_text(pending: &OpenChunk) -> bool {
    matches!(pending, OpenChunk::Text { .. })
}

fn is_tool(pending: &OpenChunk) -> bool {
    matches!(pending, OpenChunk::Tool { .. })
}

fn is_reasoning(pending: &OpenChunk) -> bool {
    matches!(pending, OpenChunk::Reasoning { .. })
}

/// The entity id a pending stream is keyed by, whichever kind it is.
fn pending_entity_id(pending: &OpenChunk) -> &str {
    match pending {
        OpenChunk::Text { message_id, .. } | OpenChunk::Reasoning { message_id } => message_id,
        OpenChunk::Tool { tool_call_id, .. } => tool_call_id,
    }
}

fn attributable(subagent_run_id: Option<&str>) -> AttributableFields {
    AttributableFields {
        subagent_run_id: subagent_run_id.map(str::to_string),
    }
}

/// The lane (owner) whose pending stream satisfies `kind` and holds
/// `entity_id`, if any.
fn lane_holding(
    lanes: &Lanes,
    kind: fn(&OpenChunk) -> bool,
    entity_id: &str,
) -> Option<Option<String>> {
    lanes
        .iter()
        .find(|(_, pending)| kind(pending) && pending_entity_id(pending) == entity_id)
        .map(|(owner, _)| owner.clone())
}

/// Decide which lane a chunk belongs to, mirroring the TypeScript
/// `resolveLane` in `chunks/transform.ts`. Every chunk carries its own
/// `subagentRunId`, which is what makes per-lane assembly possible at all —
/// but the shorthand lets a continuation omit both the id and the tag, so the
/// lane has to be inferred.
fn resolve_lane(
    lanes: &Lanes,
    kind: fn(&OpenChunk) -> bool,
    kind_label: &'static str,
    chunk_type: &'static str,
    entity_id: Option<&str>,
    tag: Option<&str>,
) -> Result<Option<String>> {
    let Some(entity_id) = entity_id else {
        // Continuation shorthand. A tag names its lane outright.
        if let Some(tag) = tag {
            return Ok(Some(tag.to_string()));
        }

        // Untagged means the parent agent, so prefer the parent's own open stream.
        if matches!(lane_get(lanes, None), Some(ref pending) if kind(pending)) {
            return Ok(None);
        }

        // Otherwise fall back to the sole open stream of this kind, so producers
        // that attribute only the opening chunk keep working. An id-less chunk can
        // never OPEN a stream (a first chunk must carry its id — the caller throws),
        // so when the parent has no stream of this kind to continue, the sole open
        // stream is the chunk's only possible referent.
        let candidates: Vec<Option<String>> = lanes
            .iter()
            .filter(|(_, pending)| kind(pending))
            .map(|(owner, _)| owner.clone())
            .collect();
        return match candidates.as_slice() {
            [only] => Ok(only.clone()),
            [] => Ok(None),
            _ => Err(AgUiError::validation(format!(
                "Ambiguous {chunk_type}: it carries neither an id nor a subagentRunId, but {} lanes have an open {kind_label}. Attribute the chunk to the subagent it belongs to.",
                candidates.len()
            ))),
        };
    };

    // A named id continues wherever it is already open, regardless of who sends
    // it, so the id remains the strongest signal. A tag that disagrees with
    // that lane is the contradiction the continuation-owner rule forbids:
    // rejected here rather than left to verifyEvents, because a chunk carrying
    // attribution but no delta synthesizes nothing, so the disagreement would
    // never reach the verifier.
    if let Some(holder) = lane_holding(lanes, kind, entity_id) {
        if let Some(tag) = tag {
            if Some(tag) != holder.as_deref() {
                let holder_desc = holder
                    .as_deref()
                    .map(|s| format!("'{s}'"))
                    .unwrap_or_else(|| "(the parent agent)".to_string());
                return Err(AgUiError::validation(format!(
                    "Cannot continue {kind_label} '{entity_id}': chunk subagentRunId '{tag}' does not match the open stream's subagent {holder_desc}."
                )));
            }
        }
        return Ok(holder);
    }

    // An id nobody holds opens a new stream, in the lane its own tag names.
    Ok(tag.map(str::to_string))
}

/// Emit the END for whatever `owner` has open, and clear the lane.
fn close_lane(lanes: &mut Lanes, owner: Option<&str>) -> Vec<Event> {
    if let Some(index) = lanes
        .iter()
        .position(|(key, _)| *key == owner.map(str::to_string))
    {
        let (_, pending) = lanes.remove(index);
        vec![pending.close(attributable(owner))]
    } else {
        Vec::new()
    }
}

/// Close every lane, in the order they opened. Used by the run-level events,
/// which describe the run as a whole rather than any one producer within it.
fn close_all_lanes(lanes: &mut Lanes) -> Vec<Event> {
    let mut events = Vec::new();
    while !lanes.is_empty() {
        let owner = lanes[0].0.clone();
        events.extend(close_lane(lanes, owner.as_deref()));
    }
    events
}

/// The subagent owner an explicit (non-chunk) event names, via its flattened
/// `AttributableFields`; absent reads as the parent lane.
fn event_owner(event: &Event) -> Option<&str> {
    match event {
        Event::TextMessageStart(e) => e.attributable.subagent_run_id.as_deref(),
        Event::TextMessageContent(e) => e.attributable.subagent_run_id.as_deref(),
        Event::TextMessageEnd(e) => e.attributable.subagent_run_id.as_deref(),
        Event::ToolCallStart(e) => e.attributable.subagent_run_id.as_deref(),
        Event::ToolCallArgs(e) => e.attributable.subagent_run_id.as_deref(),
        Event::ToolCallEnd(e) => e.attributable.subagent_run_id.as_deref(),
        Event::ToolCallResult(e) => e.attributable.subagent_run_id.as_deref(),
        Event::StateSnapshot(e) => e.attributable.subagent_run_id.as_deref(),
        Event::StateDelta(e) => e.attributable.subagent_run_id.as_deref(),
        Event::Custom(e) => e.attributable.subagent_run_id.as_deref(),
        Event::StepStarted(e) => e.attributable.subagent_run_id.as_deref(),
        Event::StepFinished(e) => e.attributable.subagent_run_id.as_deref(),
        Event::ReasoningStart(e) => e.attributable.subagent_run_id.as_deref(),
        Event::ReasoningMessageStart(e) => e.attributable.subagent_run_id.as_deref(),
        Event::ReasoningMessageContent(e) => e.attributable.subagent_run_id.as_deref(),
        Event::ReasoningMessageEnd(e) => e.attributable.subagent_run_id.as_deref(),
        _ => None,
    }
}

fn lane_get(lanes: &Lanes, owner: Option<&str>) -> Option<OpenChunk> {
    lanes
        .iter()
        .find(|(key, _)| *key == owner.map(str::to_string))
        .map(|(_, pending)| pending.clone())
}

fn lane_insert(lanes: &mut Lanes, owner: Option<String>, pending: OpenChunk) {
    lanes.retain(|(key, _)| *key != owner);
    lanes.push((owner, pending));
}

pub fn expand_chunks<S>(stream: S) -> BoxStream<'static, Result<Event>>
where
    S: Stream<Item = Result<Event>> + Send + 'static,
{
    Box::pin(try_stream! {
        let mut stream = stream.boxed();
        let mut lanes: Lanes = Vec::new();

        while let Some(item) = stream.next().await {
            let event = item?;
            match event {
                Event::TextMessageChunk(chunk) => {
                    for event in expand_text_chunk(&mut lanes, chunk)? {
                        yield event;
                    }
                }
                Event::ToolCallChunk(chunk) => {
                    for event in expand_tool_chunk(&mut lanes, chunk)? {
                        yield event;
                    }
                }
                Event::ReasoningMessageChunk(chunk) => {
                    for event in expand_reasoning_chunk(&mut lanes, chunk)? {
                        yield event;
                    }
                }
                // These events pass through without disturbing any open chunk
                // stream, matching the dedicated passthrough arm in TS.
                Event::Raw(_)
                | Event::ActivitySnapshot(_)
                | Event::ActivityDelta(_)
                | Event::ReasoningEncryptedValue(_)
                | Event::SubagentStarted(_) => yield event,
                // A subagent's terminal event closes any stream still being
                // assembled from chunks — its own lane only. A terminal with no
                // id is malformed and must not be read as closing the parent
                // lane (our typed surface has no null owner at all).
                Event::SubagentFinished(ref done) => {
                    for event in close_lane(&mut lanes, Some(done.subagent_run_id.as_str())) {
                        yield event;
                    }
                    yield event;
                }
                Event::SubagentError(ref err) => {
                    for event in close_lane(&mut lanes, Some(err.subagent_run_id.as_str())) {
                        yield event;
                    }
                    yield event;
                }
                // Run-level events describe the run as a whole rather than any
                // one producer within it, so every lane closes — otherwise a
                // subagent's chunk stream would outlive the run that carried
                // it. MESSAGES_SNAPSHOT restates the entire conversation and
                // belongs here too.
                Event::RunStarted(_)
                | Event::RunFinished(_)
                | Event::RunError(_)
                | Event::MessagesSnapshot(_) => {
                    for event in close_all_lanes(&mut lanes) {
                        yield event;
                    }
                    yield event;
                }
                // Every other structured event closes only ITS OWN lane's
                // pending stream first. Events that carry no tag read as the
                // parent lane, which is what they are.
                other => {
                    let owner = event_owner(&other);
                    for event in close_lane(&mut lanes, owner) {
                        yield event;
                    }
                    yield other;
                }
            }
        }

        // In TS `finalize` clears the lanes but DISCARDS the END events it
        // builds — a stream that ends without a run terminal therefore has no
        // synthesized END. Same here: only state is cleared.
        lanes.clear();
    })
}

/// A continuation chunk MAY repeat a field its opener established, but only
/// with the same value. A conflicting repeat is fatal — rejected here rather
/// than left to verifyEvents because the repeated field never survives
/// expansion: the synthesized content event does not carry it, so downstream
/// stages would never see the disagreement.
fn require_agreement(
    entity_kind: &str,
    entity_id: &str,
    field: &str,
    incoming: Option<&str>,
    established: Option<&str>,
) -> Result<()> {
    if let Some(incoming) = incoming {
        if Some(incoming) != established {
            let established_desc = established
                .map(|e| format!("'{e}'"))
                .unwrap_or_else(|| "(absent)".to_string());
            return Err(AgUiError::validation(format!(
                "Cannot continue {entity_kind} '{entity_id}': chunk {field} '{incoming}' does not match the open stream's {field} {established_desc}."
            )));
        }
    }
    Ok(())
}

fn role_str(role: &TextMessageRole) -> &'static str {
    match role {
        TextMessageRole::Assistant => "assistant",
        TextMessageRole::User => "user",
        TextMessageRole::Developer => "developer",
        TextMessageRole::System => "system",
    }
}

fn expand_text_chunk(lanes: &mut Lanes, chunk: TextMessageChunkEvent) -> Result<Vec<Event>> {
    let mut events = Vec::new();
    let tag = chunk.attributable.subagent_run_id.clone();
    let lane = resolve_lane(
        lanes,
        is_text,
        "text message",
        "TEXT_MESSAGE_CHUNK",
        chunk.message_id.as_deref(),
        tag.as_deref(),
    )?;

    let lane_slot = lane.clone();
    let continuation = match lane_get(lanes, lane_slot.as_deref()) {
        Some(OpenChunk::Text {
            message_id,
            role,
            name,
        }) => {
            // An absent id continues; a present one must be the same message.
            if let Some(incoming_id) = chunk.message_id.as_deref() {
                if incoming_id != message_id.as_str() {
                    false
                } else {
                    require_agreement(
                        "text message",
                        &message_id,
                        "role",
                        chunk.role.as_ref().map(|r| role_str(r)),
                        role.as_ref().map(|r| role_str(r)),
                    )?;
                    require_agreement(
                        "text message",
                        &message_id,
                        "name",
                        chunk.name.as_deref(),
                        name.as_deref(),
                    )?;
                    true
                }
            } else {
                true
            }
        }
        _ => false,
    };

    if !continuation {
        // Whatever else this lane had open ends before the new stream begins.
        events.extend(close_lane(lanes, lane.as_deref()));

        let message_id = chunk.message_id.clone().ok_or_else(|| {
            AgUiError::validation("first TEXT_MESSAGE_CHUNK must include message_id")
        })?;
        let role = chunk.role.unwrap_or(TextMessageRole::Assistant);

        lane_insert(
            lanes,
            lane_slot.clone(),
            OpenChunk::Text {
                message_id: message_id.clone(),
                role: chunk.role,
                name: chunk.name.clone(),
            },
        );

        events.push(Event::TextMessageStart(TextMessageStartEvent {
            message_id,
            role,
            name: chunk.name.clone(),
            base: BaseEventFields::default(),
            attributable: attributable(tag.as_deref()),
        }));
    }

    // A content event is emitted when the chunk carries a delta. Prefer the
    // INCOMING chunk's tag over the opener's, so a producer that attributes
    // every chunk sees its own attribution on the output rather than a value
    // this transform remembered.
    if let Some(delta) = chunk.delta {
        let Some(OpenChunk::Text { message_id, .. }) = lane_get(lanes, lane_slot.as_deref()) else {
            unreachable!("text chunk just opened or continued a text message");
        };
        let owner = tag.clone().or_else(|| {
            // The opener's owner is this lane's key itself when present.
            lane_slot.clone()
        });
        events.push(Event::TextMessageContent(TextMessageContentEvent {
            message_id: message_id.clone(),
            delta,
            base: BaseEventFields::default(),
            attributable: attributable(owner.as_deref()),
        }));
    }

    Ok(events)
}

fn expand_tool_chunk(lanes: &mut Lanes, chunk: ToolCallChunkEvent) -> Result<Vec<Event>> {
    let mut events = Vec::new();
    let tag = chunk.attributable.subagent_run_id.clone();
    let lane = resolve_lane(
        lanes,
        is_tool,
        "tool call",
        "TOOL_CALL_CHUNK",
        chunk.tool_call_id.as_deref(),
        tag.as_deref(),
    )?;

    let lane_slot = lane.clone();
    let mut continuation = false;
    if let Some(OpenChunk::Tool {
        tool_call_id,
        tool_call_name,
        parent_message_id,
    }) = lane_get(lanes, lane_slot.as_deref())
    {
        if let Some(incoming_id) = chunk.tool_call_id.as_deref() {
            if incoming_id == tool_call_id {
                require_agreement(
                    "tool call",
                    &tool_call_id,
                    "toolCallName",
                    chunk.tool_call_name.as_deref(),
                    Some(tool_call_name.as_str()),
                )?;
                require_agreement(
                    "tool call",
                    &tool_call_id,
                    "parentMessageId",
                    chunk.parent_message_id.as_deref(),
                    parent_message_id.as_deref(),
                )?;
                continuation = true;
            }
        } else {
            continuation = true;
        }
    }

    if !continuation {
        // Whatever else this lane had open ends before the new stream begins.
        events.extend(close_lane(lanes, lane.as_deref()));

        let tool_call_id = chunk.tool_call_id.clone().ok_or_else(|| {
            AgUiError::validation("first TOOL_CALL_CHUNK must include tool_call_id")
        })?;
        let tool_call_name = chunk.tool_call_name.clone().ok_or_else(|| {
            AgUiError::validation("first TOOL_CALL_CHUNK must include tool_call_name")
        })?;

        lane_insert(
            lanes,
            lane_slot.clone(),
            OpenChunk::Tool {
                tool_call_id: tool_call_id.clone(),
                tool_call_name: tool_call_name.clone(),
                parent_message_id: chunk.parent_message_id.clone(),
            },
        );

        events.push(Event::ToolCallStart(ToolCallStartEvent {
            tool_call_id,
            tool_call_name,
            parent_message_id: chunk.parent_message_id.clone(),
            base: BaseEventFields::default(),
            attributable: attributable(tag.as_deref()),
        }));
    }

    if let Some(delta) = chunk.delta {
        let Some(OpenChunk::Tool { tool_call_id, .. }) = lane_get(lanes, lane_slot.as_deref())
        else {
            unreachable!("tool chunk just opened or continued a tool call");
        };
        let owner = tag.clone().or_else(|| lane_slot.clone());
        events.push(Event::ToolCallArgs(ToolCallArgsEvent {
            tool_call_id: tool_call_id.clone(),
            delta,
            base: BaseEventFields::default(),
            attributable: attributable(owner.as_deref()),
        }));
    }

    Ok(events)
}

fn expand_reasoning_chunk(
    lanes: &mut Lanes,
    chunk: ReasoningMessageChunkEvent,
) -> Result<Vec<Event>> {
    let mut events = Vec::new();
    let tag = chunk.attributable.subagent_run_id.clone();
    let lane = resolve_lane(
        lanes,
        is_reasoning,
        "reasoning message",
        "REASONING_MESSAGE_CHUNK",
        chunk.message_id.as_deref(),
        tag.as_deref(),
    )?;

    let lane_slot = lane.clone();
    let mut continuation = false;
    if let Some(OpenChunk::Reasoning { message_id }) = lane_get(lanes, lane_slot.as_deref()) {
        // An absent id continues; a present one must be the same message. An
        // explicitly empty id is a present id that denotes a NEW stream.
        match chunk.message_id.as_deref() {
            None => continuation = true,
            Some("") => {}
            Some(incoming_id) => continuation = incoming_id == message_id,
        }
    }

    if !continuation {
        // Whatever else this lane had open ends before the new stream begins.
        events.extend(close_lane(lanes, lane.as_deref()));

        let message_id = chunk.message_id.clone().ok_or_else(|| {
            AgUiError::validation("first REASONING_MESSAGE_CHUNK must include message_id")
        })?;

        lane_insert(
            lanes,
            lane_slot.clone(),
            OpenChunk::Reasoning {
                message_id: message_id.clone(),
            },
        );

        events.push(Event::ReasoningMessageStart(ReasoningMessageStartEvent {
            message_id,
            role: ReasoningMessageRole::Reasoning,
            base: BaseEventFields::default(),
            attributable: attributable(tag.as_deref()),
        }));
    }

    if let Some(delta) = chunk.delta {
        let Some(OpenChunk::Reasoning { message_id }) = lane_get(lanes, lane_slot.as_deref())
        else {
            unreachable!("reasoning chunk just opened or continued a reasoning message");
        };
        let owner = tag.clone().or_else(|| lane_slot.clone());
        events.push(Event::ReasoningMessageContent(
            ReasoningMessageContentEvent {
                message_id: message_id.clone(),
                delta,
                base: BaseEventFields::default(),
                attributable: attributable(owner.as_deref()),
            },
        ));
    }

    Ok(events)
}

#[cfg(test)]
mod tests {
    use super::*;
    use agui_rs_core::{
        factory, ReasoningEncryptedValueEvent, ReasoningEncryptedValueSubtype,
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
        let events = collect_ok(vec![
            Event::TextMessageChunk(TextMessageChunkEvent {
                message_id: Some("m1".into()),
                role: Some(TextMessageRole::Assistant),
                attributable: AttributableFields::default(),
                delta: Some("hello".into()),
                name: None,
                base: BaseEventFields::default(),
            }),
            factory::run_finished("t", "r"),
        ])
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
                attributable: AttributableFields::default(),
                delta: Some("hel".into()),
                name: None,
                base: BaseEventFields::default(),
            }),
            Event::TextMessageChunk(TextMessageChunkEvent {
                message_id: None,
                role: None,
                attributable: AttributableFields::default(),
                delta: Some("lo".into()),
                name: None,
                base: BaseEventFields::default(),
            }),
            factory::run_finished("t", "r"),
        ])
        .await;

        assert_eq!(events.len(), 5);
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
                attributable: AttributableFields::default(),
                delta: Some("one".into()),
                name: None,
                base: BaseEventFields::default(),
            }),
            Event::TextMessageChunk(TextMessageChunkEvent {
                message_id: Some("m2".into()),
                role: Some(TextMessageRole::Assistant),
                attributable: AttributableFields::default(),
                delta: Some("two".into()),
                name: None,
                base: BaseEventFields::default(),
            }),
            factory::run_finished("t", "r"),
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
        let events = collect_ok(vec![
            Event::ToolCallChunk(ToolCallChunkEvent {
                tool_call_id: Some("tc1".into()),
                tool_call_name: Some("search".into()),
                parent_message_id: Some("m1".into()),
                delta: Some("{".into()),
                base: BaseEventFields::default(),
                attributable: AttributableFields::default(),
            }),
            factory::run_finished("t", "r"),
        ])
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
                attributable: AttributableFields::default(),
            }),
            Event::ToolCallChunk(ToolCallChunkEvent {
                tool_call_id: None,
                tool_call_name: None,
                parent_message_id: None,
                delta: Some("rust\"}".into()),
                base: BaseEventFields::default(),
                attributable: AttributableFields::default(),
            }),
            factory::run_finished("t", "r"),
        ])
        .await;

        assert_eq!(events.len(), 5);
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
            let events = collect_ok(vec![
                Event::ReasoningMessageChunk(ReasoningMessageChunkEvent {
                    message_id: Some("r1".into()),
                    delta: Some("plan".into()),
                    base: BaseEventFields::default(),
                    attributable: AttributableFields::default(),
                }),
                factory::run_finished("t", "r"),
            ])
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
                    attributable: AttributableFields::default(),
                }),
                Event::ReasoningMessageChunk(ReasoningMessageChunkEvent {
                    message_id: None,
                    delta: Some("n".into()),
                    base: BaseEventFields::default(),
                    attributable: AttributableFields::default(),
                }),
                factory::run_finished("t", "r"),
            ])
            .await;

            assert_eq!(events.len(), 5);
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
                    attributable: AttributableFields::default(),
                }),
                Event::ReasoningMessageChunk(ReasoningMessageChunkEvent {
                    message_id: Some("r2".into()),
                    delta: Some("two".into()),
                    base: BaseEventFields::default(),
                    attributable: AttributableFields::default(),
                }),
                factory::run_finished("t", "r"),
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
                    attributable: AttributableFields::default(),
                }),
                Event::ReasoningEncryptedValue(ReasoningEncryptedValueEvent {
                    subtype: ReasoningEncryptedValueSubtype::Message,
                    entity_id: "r1".into(),
                    encrypted_value: "secret".into(),
                    attributable: AttributableFields::default(),
                    base: BaseEventFields::default(),
                }),
                Event::ReasoningMessageChunk(ReasoningMessageChunkEvent {
                    message_id: None,
                    delta: Some("b".into()),
                    base: BaseEventFields::default(),
                    attributable: AttributableFields::default(),
                }),
                factory::run_finished("t", "r"),
            ])
            .await;

            assert_eq!(events.len(), 6);
            assert!(matches!(events[0], Event::ReasoningMessageStart(_)));
            assert!(matches!(events[1], Event::ReasoningMessageContent(_)));
            assert!(matches!(events[2], Event::ReasoningEncryptedValue(_)));
            assert!(matches!(events[3], Event::ReasoningMessageContent(_)));
            assert!(matches!(events[4], Event::ReasoningMessageEnd(_)));
        }
    }
}
