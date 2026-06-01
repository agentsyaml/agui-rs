//! Mapping between `agui_rs_core::Event` and the protobuf `schema::Event`.

use agui_rs_core::types::{
    AssistantMessage, DeveloperMessage, FunctionCall, InputContent, InputContentSource,
    ReasoningMessage, SystemMessage, ToolMessage, UserMessage, UserMessageContent,
};
use agui_rs_core::{
    AgUiError, BaseEventFields, Event, Interrupt, Message, Result, RunFinishedOutcome, ToolCall,
    ToolCallKind,
};
use prost::Message as _;

use crate::schema as pb;
use crate::value::{json_opt_to_proto, json_to_proto, proto_to_json};

/// Encodes an AG-UI event into its protobuf binary representation.
///
/// Returns [`AgUiError::Unsupported`] for event variants that are not part of
/// the canonical `events.proto` schema (reasoning / activity / thinking).
pub fn encode(event: &Event) -> Result<Vec<u8>> {
    let inner = to_proto_event(event)?;
    let message = pb::Event { event: Some(inner) };
    Ok(message.encode_to_vec())
}

/// Decodes a protobuf binary representation back into an AG-UI event.
pub fn decode(data: &[u8]) -> Result<Event> {
    let message = pb::Event::decode(data)
        .map_err(|err| AgUiError::protocol(format!("protobuf decode failed: {err}")))?;
    let inner = message
        .event
        .ok_or_else(|| AgUiError::protocol("protobuf Event has no variant set"))?;
    from_proto_event(inner)
}

// ----- base event -----

fn base_to_proto(base: &BaseEventFields, ty: pb::EventType) -> pb::BaseEvent {
    pb::BaseEvent {
        r#type: ty as i32,
        timestamp: base.timestamp,
        raw_event: base.raw_event.as_ref().map(json_to_proto),
    }
}

fn base_from_proto(base: Option<pb::BaseEvent>) -> BaseEventFields {
    match base {
        Some(base) => BaseEventFields {
            timestamp: base.timestamp,
            raw_event: base.raw_event.as_ref().map(proto_to_json),
        },
        None => BaseEventFields::default(),
    }
}

// ----- tool calls -----

fn tool_call_to_proto(tc: &ToolCall) -> pb::ToolCall {
    pb::ToolCall {
        id: tc.id.clone(),
        r#type: "function".to_string(),
        function: Some(pb::ToolCallFunction {
            name: tc.function.name.clone(),
            arguments: tc.function.arguments.clone(),
        }),
    }
}

fn tool_call_from_proto(tc: pb::ToolCall) -> ToolCall {
    let function = tc.function.unwrap_or_default();
    ToolCall {
        id: tc.id,
        kind: ToolCallKind::Function,
        function: FunctionCall {
            name: function.name,
            arguments: function.arguments,
        },
        encrypted_value: None,
    }
}

// ----- input content (multimodal) -----

fn source_to_proto(source: &InputContentSource) -> pb::InputContentSource {
    let inner = match source {
        InputContentSource::Data { value, mime_type } => {
            pb::input_content_source::Source::Data(pb::InputContentDataSource {
                value: value.clone(),
                mime_type: mime_type.clone(),
            })
        }
        InputContentSource::Url { value, mime_type } => {
            pb::input_content_source::Source::Url(pb::InputContentUrlSource {
                value: value.clone(),
                mime_type: mime_type.clone(),
            })
        }
    };
    pb::InputContentSource {
        source: Some(inner),
    }
}

fn source_from_proto(source: Option<pb::InputContentSource>) -> Option<InputContentSource> {
    match source.and_then(|s| s.source) {
        Some(pb::input_content_source::Source::Data(data)) => Some(InputContentSource::Data {
            value: data.value,
            mime_type: data.mime_type,
        }),
        Some(pb::input_content_source::Source::Url(url)) => Some(InputContentSource::Url {
            value: url.value,
            mime_type: url.mime_type,
        }),
        None => None,
    }
}

fn content_part_to_proto(part: &InputContent) -> Option<pb::InputContent> {
    let inner = match part {
        InputContent::Text { text } => {
            pb::input_content::Part::Text(pb::TextInputPart { text: text.clone() })
        }
        InputContent::Image { source, metadata } => {
            pb::input_content::Part::Image(media(source, metadata))
        }
        InputContent::Audio { source, metadata } => {
            pb::input_content::Part::Audio(media(source, metadata))
        }
        InputContent::Video { source, metadata } => {
            pb::input_content::Part::Video(media(source, metadata))
        }
        InputContent::Document { source, metadata } => {
            pb::input_content::Part::Document(media(source, metadata))
        }
        // Legacy binary parts map onto Document with metadata, mirroring TS.
        InputContent::Binary { content } => {
            let source = if let Some(data) = &content.data {
                pb::InputContentSource {
                    source: Some(pb::input_content_source::Source::Data(
                        pb::InputContentDataSource {
                            value: data.clone(),
                            mime_type: content.mime_type.clone(),
                        },
                    )),
                }
            } else if let Some(url) = content.url.as_ref().or(content.id.as_ref()) {
                pb::InputContentSource {
                    source: Some(pb::input_content_source::Source::Url(
                        pb::InputContentUrlSource {
                            value: url.clone(),
                            mime_type: Some(content.mime_type.clone()),
                        },
                    )),
                }
            } else {
                return None;
            };
            let mut meta = serde_json::Map::new();
            meta.insert("legacyBinary".into(), serde_json::Value::Bool(true));
            if let Some(filename) = &content.filename {
                meta.insert(
                    "filename".into(),
                    serde_json::Value::String(filename.clone()),
                );
            }
            if let Some(id) = &content.id {
                meta.insert("id".into(), serde_json::Value::String(id.clone()));
            }
            pb::input_content::Part::Document(pb::MediaInputPart {
                source: Some(source),
                metadata: Some(json_to_proto(&serde_json::Value::Object(meta))),
            })
        }
    };
    Some(pb::InputContent { part: Some(inner) })
}

fn media(source: &InputContentSource, metadata: &Option<serde_json::Value>) -> pb::MediaInputPart {
    pb::MediaInputPart {
        source: Some(source_to_proto(source)),
        metadata: json_opt_to_proto(metadata.as_ref()),
    }
}

fn content_part_from_proto(part: pb::InputContent) -> Option<InputContent> {
    match part.part? {
        pb::input_content::Part::Text(t) => Some(InputContent::Text { text: t.text }),
        pb::input_content::Part::Image(m) => Some(InputContent::Image {
            source: source_from_proto(m.source)?,
            metadata: m.metadata.as_ref().map(proto_to_json),
        }),
        pb::input_content::Part::Audio(m) => Some(InputContent::Audio {
            source: source_from_proto(m.source)?,
            metadata: m.metadata.as_ref().map(proto_to_json),
        }),
        pb::input_content::Part::Video(m) => Some(InputContent::Video {
            source: source_from_proto(m.source)?,
            metadata: m.metadata.as_ref().map(proto_to_json),
        }),
        pb::input_content::Part::Document(m) => Some(InputContent::Document {
            source: source_from_proto(m.source)?,
            metadata: m.metadata.as_ref().map(proto_to_json),
        }),
    }
}

// ----- messages -----

fn message_to_proto(message: &Message) -> pb::ProtoMessage {
    let mut proto = pb::ProtoMessage {
        id: message.id().to_string(),
        role: role_str(message).to_string(),
        ..Default::default()
    };
    match message {
        Message::Developer(m) => proto.content = Some(m.content.clone()),
        Message::System(m) => proto.content = Some(m.content.clone()),
        Message::Assistant(m) => {
            proto.content = m.content.clone();
            proto.name = m.name.clone();
            if let Some(tool_calls) = &m.tool_calls {
                proto.tool_calls = tool_calls.iter().map(tool_call_to_proto).collect();
            }
        }
        Message::User(m) => match &m.content {
            UserMessageContent::Text(text) => proto.content = Some(text.clone()),
            UserMessageContent::Parts(parts) => {
                proto.content_parts = parts.iter().filter_map(content_part_to_proto).collect();
            }
        },
        Message::Tool(m) => {
            proto.content = Some(m.content.clone());
            proto.tool_call_id = Some(m.tool_call_id.clone());
            proto.error = m.error.clone();
        }
        Message::Reasoning(m) => proto.content = Some(m.content.clone()),
        Message::Activity(_) => {}
    }
    proto
}

fn role_str(message: &Message) -> &'static str {
    match message {
        Message::Developer(_) => "developer",
        Message::System(_) => "system",
        Message::Assistant(_) => "assistant",
        Message::User(_) => "user",
        Message::Tool(_) => "tool",
        Message::Reasoning(_) => "reasoning",
        Message::Activity(_) => "activity",
    }
}

fn message_from_proto(proto: pb::ProtoMessage) -> Result<Message> {
    let id = proto.id;
    let message = match proto.role.as_str() {
        "developer" => Message::Developer(DeveloperMessage {
            id,
            content: proto.content.unwrap_or_default(),
            name: proto.name,
            encrypted_value: None,
        }),
        "system" => Message::System(SystemMessage {
            id,
            content: proto.content.unwrap_or_default(),
            name: proto.name,
            encrypted_value: None,
        }),
        "assistant" => Message::Assistant(AssistantMessage {
            id,
            content: proto.content,
            name: proto.name,
            tool_calls: if proto.tool_calls.is_empty() {
                None
            } else {
                Some(
                    proto
                        .tool_calls
                        .into_iter()
                        .map(tool_call_from_proto)
                        .collect(),
                )
            },
            encrypted_value: None,
        }),
        "user" => {
            let content = if !proto.content_parts.is_empty() {
                let parts: Vec<InputContent> = proto
                    .content_parts
                    .into_iter()
                    .filter_map(content_part_from_proto)
                    .collect();
                UserMessageContent::Parts(parts)
            } else {
                UserMessageContent::Text(proto.content.unwrap_or_default())
            };
            Message::User(UserMessage {
                id,
                content,
                name: proto.name,
                encrypted_value: None,
            })
        }
        "tool" => Message::Tool(ToolMessage {
            id,
            content: proto.content.unwrap_or_default(),
            tool_call_id: proto.tool_call_id.unwrap_or_default(),
            error: proto.error,
            encrypted_value: None,
        }),
        "reasoning" => Message::Reasoning(ReasoningMessage {
            id,
            content: proto.content.unwrap_or_default(),
            encrypted_value: None,
        }),
        other => {
            return Err(AgUiError::protocol(format!(
                "protobuf message has unknown role '{other}'"
            )))
        }
    };
    Ok(message)
}

// ----- interrupts -----

fn interrupt_to_proto(interrupt: &Interrupt) -> pb::Interrupt {
    pb::Interrupt {
        id: interrupt.id.clone(),
        reason: interrupt.reason.clone(),
        message: interrupt.message.clone(),
        tool_call_id: interrupt.tool_call_id.clone(),
        response_schema: interrupt
            .response_schema
            .as_ref()
            .map(|m| json_to_proto(&serde_json::Value::Object(m.clone()))),
        expires_at: interrupt.expires_at.clone(),
        metadata: interrupt
            .metadata
            .as_ref()
            .map(|m| json_to_proto(&serde_json::Value::Object(m.clone()))),
    }
}

fn interrupt_from_proto(proto: pb::Interrupt) -> Interrupt {
    fn as_object(
        value: Option<&prost_types::Value>,
    ) -> Option<serde_json::Map<String, serde_json::Value>> {
        match value.map(proto_to_json) {
            Some(serde_json::Value::Object(map)) => Some(map),
            _ => None,
        }
    }
    Interrupt {
        id: proto.id,
        reason: proto.reason,
        message: proto.message,
        tool_call_id: proto.tool_call_id,
        response_schema: as_object(proto.response_schema.as_ref()),
        expires_at: proto.expires_at,
        metadata: as_object(proto.metadata.as_ref()),
    }
}

// ----- json patch -----

fn patch_op_to_proto(op: &serde_json::Value) -> Result<pb::JsonPatchOperation> {
    let obj = op
        .as_object()
        .ok_or_else(|| AgUiError::protocol("state delta op is not an object"))?;
    let op_str = obj
        .get("op")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AgUiError::protocol("state delta op missing 'op'"))?;
    let op_enum = match op_str {
        "add" => pb::JsonPatchOperationType::Add,
        "remove" => pb::JsonPatchOperationType::Remove,
        "replace" => pb::JsonPatchOperationType::Replace,
        "move" => pb::JsonPatchOperationType::Move,
        "copy" => pb::JsonPatchOperationType::Copy,
        "test" => pb::JsonPatchOperationType::Test,
        other => {
            return Err(AgUiError::protocol(format!(
                "unknown JSON patch op '{other}'"
            )))
        }
    };
    Ok(pb::JsonPatchOperation {
        op: op_enum as i32,
        path: obj
            .get("path")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string(),
        from: obj.get("from").and_then(|v| v.as_str()).map(String::from),
        value: obj.get("value").map(json_to_proto),
    })
}

fn patch_op_from_proto(op: pb::JsonPatchOperation) -> serde_json::Value {
    let op_str = match pb::JsonPatchOperationType::try_from(op.op) {
        Ok(pb::JsonPatchOperationType::Add) => "add",
        Ok(pb::JsonPatchOperationType::Remove) => "remove",
        Ok(pb::JsonPatchOperationType::Replace) => "replace",
        Ok(pb::JsonPatchOperationType::Move) => "move",
        Ok(pb::JsonPatchOperationType::Copy) => "copy",
        Ok(pb::JsonPatchOperationType::Test) => "test",
        Err(_) => "add",
    };
    let mut map = serde_json::Map::new();
    map.insert("op".into(), serde_json::Value::String(op_str.into()));
    map.insert("path".into(), serde_json::Value::String(op.path));
    if let Some(from) = op.from {
        map.insert("from".into(), serde_json::Value::String(from));
    }
    if let Some(value) = op.value {
        map.insert("value".into(), proto_to_json(&value));
    }
    serde_json::Value::Object(map)
}

// ----- event dispatch -----

fn to_proto_event(event: &Event) -> Result<pb::event::Event> {
    use pb::event::Event as PE;
    let e = match event {
        Event::TextMessageStart(ev) => PE::TextMessageStart(pb::TextMessageStartEvent {
            base_event: Some(base_to_proto(&ev.base, pb::EventType::TextMessageStart)),
            message_id: ev.message_id.clone(),
            role: Some(format!("{:?}", ev.role).to_lowercase()),
            name: ev.name.clone(),
        }),
        Event::TextMessageContent(ev) => PE::TextMessageContent(pb::TextMessageContentEvent {
            base_event: Some(base_to_proto(&ev.base, pb::EventType::TextMessageContent)),
            message_id: ev.message_id.clone(),
            delta: ev.delta.clone(),
        }),
        Event::TextMessageEnd(ev) => PE::TextMessageEnd(pb::TextMessageEndEvent {
            base_event: Some(base_to_proto(&ev.base, pb::EventType::TextMessageEnd)),
            message_id: ev.message_id.clone(),
        }),
        Event::TextMessageChunk(ev) => PE::TextMessageChunk(pb::TextMessageChunkEvent {
            base_event: Some(base_to_proto(&ev.base, pb::EventType::TextMessageStart)),
            message_id: ev.message_id.clone(),
            role: ev.role.map(|r| format!("{r:?}").to_lowercase()),
            delta: ev.delta.clone(),
            name: ev.name.clone(),
        }),
        Event::ToolCallStart(ev) => PE::ToolCallStart(pb::ToolCallStartEvent {
            base_event: Some(base_to_proto(&ev.base, pb::EventType::ToolCallStart)),
            tool_call_id: ev.tool_call_id.clone(),
            tool_call_name: ev.tool_call_name.clone(),
            parent_message_id: ev.parent_message_id.clone(),
        }),
        Event::ToolCallArgs(ev) => PE::ToolCallArgs(pb::ToolCallArgsEvent {
            base_event: Some(base_to_proto(&ev.base, pb::EventType::ToolCallArgs)),
            tool_call_id: ev.tool_call_id.clone(),
            delta: ev.delta.clone(),
        }),
        Event::ToolCallEnd(ev) => PE::ToolCallEnd(pb::ToolCallEndEvent {
            base_event: Some(base_to_proto(&ev.base, pb::EventType::ToolCallEnd)),
            tool_call_id: ev.tool_call_id.clone(),
        }),
        Event::ToolCallChunk(ev) => PE::ToolCallChunk(pb::ToolCallChunkEvent {
            base_event: Some(base_to_proto(&ev.base, pb::EventType::ToolCallStart)),
            tool_call_id: ev.tool_call_id.clone(),
            tool_call_name: ev.tool_call_name.clone(),
            parent_message_id: ev.parent_message_id.clone(),
            delta: ev.delta.clone(),
        }),
        Event::StateSnapshot(ev) => PE::StateSnapshot(pb::StateSnapshotEvent {
            base_event: Some(base_to_proto(&ev.base, pb::EventType::StateSnapshot)),
            snapshot: Some(json_to_proto(&ev.snapshot)),
        }),
        Event::StateDelta(ev) => PE::StateDelta(pb::StateDeltaEvent {
            base_event: Some(base_to_proto(&ev.base, pb::EventType::StateDelta)),
            delta: ev
                .delta
                .iter()
                .map(patch_op_to_proto)
                .collect::<Result<_>>()?,
        }),
        Event::MessagesSnapshot(ev) => PE::MessagesSnapshot(pb::MessagesSnapshotEvent {
            base_event: Some(base_to_proto(&ev.base, pb::EventType::MessagesSnapshot)),
            messages: ev.messages.iter().map(message_to_proto).collect(),
        }),
        Event::Raw(ev) => PE::Raw(pb::RawEvent {
            base_event: Some(base_to_proto(&ev.base, pb::EventType::Raw)),
            event: Some(json_to_proto(&ev.event)),
            source: ev.source.clone(),
        }),
        Event::Custom(ev) => PE::Custom(pb::CustomEvent {
            base_event: Some(base_to_proto(&ev.base, pb::EventType::Custom)),
            name: ev.name.clone(),
            value: Some(json_to_proto(&ev.value)),
        }),
        Event::RunStarted(ev) => PE::RunStarted(pb::RunStartedEvent {
            base_event: Some(base_to_proto(&ev.base, pb::EventType::RunStarted)),
            thread_id: ev.thread_id.clone(),
            run_id: ev.run_id.clone(),
        }),
        Event::RunFinished(ev) => {
            let (outcome, interrupts) = match &ev.outcome {
                None => (String::new(), Vec::new()),
                Some(RunFinishedOutcome::Success) => ("success".to_string(), Vec::new()),
                Some(RunFinishedOutcome::Interrupt { interrupts }) => (
                    "interrupt".to_string(),
                    interrupts.iter().map(interrupt_to_proto).collect(),
                ),
            };
            PE::RunFinished(pb::RunFinishedEvent {
                base_event: Some(base_to_proto(&ev.base, pb::EventType::RunFinished)),
                thread_id: ev.thread_id.clone(),
                run_id: ev.run_id.clone(),
                result: ev.result.as_ref().map(json_to_proto),
                outcome,
                interrupts,
            })
        }
        Event::RunError(ev) => PE::RunError(pb::RunErrorEvent {
            base_event: Some(base_to_proto(&ev.base, pb::EventType::RunError)),
            code: ev.code.clone(),
            message: ev.message.clone(),
        }),
        Event::StepStarted(ev) => PE::StepStarted(pb::StepStartedEvent {
            base_event: Some(base_to_proto(&ev.base, pb::EventType::StepStarted)),
            step_name: ev.step_name.clone(),
        }),
        Event::StepFinished(ev) => PE::StepFinished(pb::StepFinishedEvent {
            base_event: Some(base_to_proto(&ev.base, pb::EventType::StepFinished)),
            step_name: ev.step_name.clone(),
        }),
        // Not part of the canonical protobuf schema.
        Event::ActivitySnapshot(_)
        | Event::ActivityDelta(_)
        | Event::ToolCallResult(_)
        | Event::ReasoningStart(_)
        | Event::ReasoningMessageStart(_)
        | Event::ReasoningMessageContent(_)
        | Event::ReasoningMessageEnd(_)
        | Event::ReasoningMessageChunk(_)
        | Event::ReasoningEnd(_)
        | Event::ReasoningEncryptedValue(_)
        | Event::ThinkingStart(_)
        | Event::ThinkingEnd(_)
        | Event::ThinkingTextMessageStart(_)
        | Event::ThinkingTextMessageContent(_)
        | Event::ThinkingTextMessageEnd(_) => {
            return Err(AgUiError::unsupported(format!(
                "event type {:?} is not part of the AG-UI protobuf schema",
                event.event_type()
            )))
        }
    };
    Ok(e)
}

fn from_proto_event(event: pb::event::Event) -> Result<Event> {
    use agui_rs_core::{
        CustomEvent, RawEvent, RunErrorEvent, RunFinishedEvent, RunStartedEvent, StateDeltaEvent,
        StateSnapshotEvent, StepFinishedEvent, StepStartedEvent, TextMessageChunkEvent,
        TextMessageContentEvent, TextMessageEndEvent, TextMessageStartEvent, ToolCallArgsEvent,
        ToolCallChunkEvent, ToolCallEndEvent, ToolCallStartEvent,
    };
    use pb::event::Event as PE;
    let event = match event {
        PE::TextMessageStart(ev) => Event::TextMessageStart(TextMessageStartEvent {
            message_id: ev.message_id,
            role: parse_text_role(ev.role.as_deref()),
            name: ev.name,
            base: base_from_proto(ev.base_event),
        }),
        PE::TextMessageContent(ev) => Event::TextMessageContent(TextMessageContentEvent {
            message_id: ev.message_id,
            delta: ev.delta,
            base: base_from_proto(ev.base_event),
        }),
        PE::TextMessageEnd(ev) => Event::TextMessageEnd(TextMessageEndEvent {
            message_id: ev.message_id,
            base: base_from_proto(ev.base_event),
        }),
        PE::TextMessageChunk(ev) => Event::TextMessageChunk(TextMessageChunkEvent {
            message_id: ev.message_id,
            role: ev.role.as_deref().map(|r| parse_text_role(Some(r))),
            delta: ev.delta,
            name: ev.name,
            base: base_from_proto(ev.base_event),
        }),
        PE::ToolCallStart(ev) => Event::ToolCallStart(ToolCallStartEvent {
            tool_call_id: ev.tool_call_id,
            tool_call_name: ev.tool_call_name,
            parent_message_id: ev.parent_message_id,
            base: base_from_proto(ev.base_event),
        }),
        PE::ToolCallArgs(ev) => Event::ToolCallArgs(ToolCallArgsEvent {
            tool_call_id: ev.tool_call_id,
            delta: ev.delta,
            base: base_from_proto(ev.base_event),
        }),
        PE::ToolCallEnd(ev) => Event::ToolCallEnd(ToolCallEndEvent {
            tool_call_id: ev.tool_call_id,
            base: base_from_proto(ev.base_event),
        }),
        PE::ToolCallChunk(ev) => Event::ToolCallChunk(ToolCallChunkEvent {
            tool_call_id: ev.tool_call_id,
            tool_call_name: ev.tool_call_name,
            parent_message_id: ev.parent_message_id,
            delta: ev.delta,
            base: base_from_proto(ev.base_event),
        }),
        PE::StateSnapshot(ev) => Event::StateSnapshot(StateSnapshotEvent {
            snapshot: ev
                .snapshot
                .as_ref()
                .map(proto_to_json)
                .unwrap_or(serde_json::Value::Null),
            base: base_from_proto(ev.base_event),
        }),
        PE::StateDelta(ev) => Event::StateDelta(StateDeltaEvent {
            delta: ev.delta.into_iter().map(patch_op_from_proto).collect(),
            base: base_from_proto(ev.base_event),
        }),
        PE::MessagesSnapshot(ev) => {
            let messages = ev
                .messages
                .into_iter()
                .map(message_from_proto)
                .collect::<Result<Vec<_>>>()?;
            Event::MessagesSnapshot(agui_rs_core::MessagesSnapshotEvent {
                messages,
                base: base_from_proto(ev.base_event),
            })
        }
        PE::Raw(ev) => Event::Raw(RawEvent {
            event: ev
                .event
                .as_ref()
                .map(proto_to_json)
                .unwrap_or(serde_json::Value::Null),
            source: ev.source,
            base: base_from_proto(ev.base_event),
        }),
        PE::Custom(ev) => Event::Custom(CustomEvent {
            name: ev.name,
            value: ev
                .value
                .as_ref()
                .map(proto_to_json)
                .unwrap_or(serde_json::Value::Null),
            base: base_from_proto(ev.base_event),
        }),
        PE::RunStarted(ev) => Event::RunStarted(RunStartedEvent {
            thread_id: ev.thread_id,
            run_id: ev.run_id,
            parent_run_id: None,
            input: None,
            base: base_from_proto(ev.base_event),
        }),
        PE::RunFinished(ev) => {
            let outcome = match ev.outcome.as_str() {
                "interrupt" => Some(RunFinishedOutcome::Interrupt {
                    interrupts: ev
                        .interrupts
                        .into_iter()
                        .map(interrupt_from_proto)
                        .collect(),
                }),
                "success" => Some(RunFinishedOutcome::Success),
                _ => None,
            };
            Event::RunFinished(RunFinishedEvent {
                thread_id: ev.thread_id,
                run_id: ev.run_id,
                result: ev.result.as_ref().map(proto_to_json),
                outcome,
                base: base_from_proto(ev.base_event),
            })
        }
        PE::RunError(ev) => Event::RunError(RunErrorEvent {
            message: ev.message,
            code: ev.code,
            base: base_from_proto(ev.base_event),
        }),
        PE::StepStarted(ev) => Event::StepStarted(StepStartedEvent {
            step_name: ev.step_name,
            base: base_from_proto(ev.base_event),
        }),
        PE::StepFinished(ev) => Event::StepFinished(StepFinishedEvent {
            step_name: ev.step_name,
            base: base_from_proto(ev.base_event),
        }),
    };
    Ok(event)
}

fn parse_text_role(role: Option<&str>) -> agui_rs_core::TextMessageRole {
    use agui_rs_core::TextMessageRole;
    match role {
        Some("developer") => TextMessageRole::Developer,
        Some("system") => TextMessageRole::System,
        Some("user") => TextMessageRole::User,
        _ => TextMessageRole::Assistant,
    }
}
