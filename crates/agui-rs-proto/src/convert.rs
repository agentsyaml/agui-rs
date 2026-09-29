//! Mapping between `agui_rs_core::Event` and the protobuf `schema::Event`.

use agui_rs_core::types::{
    ActivityMessage, AssistantMessage, ContentPart, Context, DeveloperMessage, FunctionCall,
    PartSource, ReasoningMessage, ResumeEntry, ResumeStatus, RunAgentInput, SystemMessage, Tool,
    ToolCallKind, ToolMessage, ToolResultContent, UserMessage, UserMessageContent,
};
use agui_rs_core::{
    AgUiError, AttributableFields, BaseEventFields, Event, Interrupt, Message,
    ReasoningEncryptedValueSubtype, ReasoningMessageRole, Result, RunFinishedOutcome,
    SubagentFinishedOutcome, TextMessageRole, TokenUsage, ToolCall, ToolResultRole,
};
use prost::Message as _;

use crate::schema as pb;
use crate::value::{json_to_proto, json_to_struct, proto_to_json, struct_to_json};

/// Encodes an AG-UI event into its protobuf binary representation.
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
        metadata: base.metadata.as_ref().map(json_to_struct),
    }
}

fn base_from_proto(base: Option<pb::BaseEvent>) -> BaseEventFields {
    match base {
        Some(base) => BaseEventFields {
            timestamp: base.timestamp,
            raw_event: base.raw_event.as_ref().map(proto_to_json),
            metadata: base.metadata.as_ref().map(struct_to_json),
        },
        None => BaseEventFields::default(),
    }
}

/// `Attributable` is one field in the schema (`subagent_run_id`) and one field on
/// the core side, so both directions are a field move.
fn attributable_from_proto(id: Option<String>) -> AttributableFields {
    AttributableFields {
        subagent_run_id: id,
    }
}

fn subagent_id(a: &AttributableFields) -> Option<String> {
    a.subagent_run_id.clone()
}

// ----- token usage -----

/// Upstream types every token count `optional int64`; core types them `Option<u64>`.
/// A negative count has no meaning as a token count, so it decodes to absent
/// rather than wrapping around to a huge number.
fn to_u64(value: Option<i64>) -> Option<u64> {
    value.and_then(|v| u64::try_from(v).ok())
}

fn to_i64(value: Option<u64>) -> Option<i64> {
    value.and_then(|v| i64::try_from(v).ok())
}

fn usage_to_proto(usage: &[TokenUsage]) -> Vec<pb::Usage> {
    usage
        .iter()
        .map(|u| pb::Usage {
            provider: u.provider.clone(),
            model: u.model.clone(),
            input_tokens: to_i64(u.input_tokens),
            output_tokens: to_i64(u.output_tokens),
            total_tokens: to_i64(u.total_tokens),
            reasoning_tokens: to_i64(u.reasoning_tokens),
            cached_input_tokens: to_i64(u.cached_input_tokens),
            cache_write_input_tokens: to_i64(u.cache_write_input_tokens),
        })
        .collect()
}

fn usage_from_proto(usage: Vec<pb::Usage>) -> Vec<TokenUsage> {
    usage
        .into_iter()
        .map(|u| TokenUsage {
            provider: u.provider,
            model: u.model,
            input_tokens: to_u64(u.input_tokens),
            output_tokens: to_u64(u.output_tokens),
            total_tokens: to_u64(u.total_tokens),
            reasoning_tokens: to_u64(u.reasoning_tokens),
            cached_input_tokens: to_u64(u.cached_input_tokens),
            cache_write_input_tokens: to_u64(u.cache_write_input_tokens),
        })
        .collect()
}

// ----- tool calls -----

fn tool_call_to_proto(tc: &ToolCall) -> pb::ToolCall {
    pb::ToolCall {
        id: tc.id.clone(),
        r#type: kind_str(tc.kind).to_string(),
        function: Some(pb::ToolCallFunction {
            name: tc.function.name.clone(),
            arguments: tc.function.arguments.clone(),
        }),
        metadata: tc
            .metadata
            .as_ref()
            .map(|m| json_to_struct(&serde_json::Value::Object(m.clone()))),
        encrypted_value: tc.encrypted_value.clone(),
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
        encrypted_value: tc.encrypted_value,
        metadata: tc.metadata.as_ref().map(|s| match struct_to_json(s) {
            serde_json::Value::Object(map) => map,
            _ => serde_json::Map::new(),
        }),
    }
}

fn kind_str(kind: ToolCallKind) -> &'static str {
    match kind {
        ToolCallKind::Function => "function",
    }
}

// ----- input content (multimodal) -----

fn source_to_proto(source: &PartSource) -> pb::InputContentSource {
    let inner = match source {
        PartSource::Data { value, mime_type } => {
            pb::input_content_source::Source::Data(pb::InputContentDataSource {
                value: value.clone(),
                mime_type: mime_type.clone(),
            })
        }
        PartSource::Url { value, mime_type } => {
            pb::input_content_source::Source::Url(pb::InputContentUrlSource {
                value: value.clone(),
                mime_type: mime_type.clone(),
            })
        }
        PartSource::File {
            value,
            provider,
            mime_type,
        } => pb::input_content_source::Source::File(pb::InputContentFileSource {
            value: value.clone(),
            provider: provider.clone(),
            mime_type: mime_type.clone(),
        }),
    };
    pb::InputContentSource {
        source: Some(inner),
    }
}

fn source_from_proto(source: Option<pb::InputContentSource>) -> PartSource {
    match source.and_then(|s| s.source) {
        Some(pb::input_content_source::Source::Data(data)) => PartSource::Data {
            value: data.value,
            mime_type: data.mime_type,
        },
        Some(pb::input_content_source::Source::Url(url)) => PartSource::Url {
            value: url.value,
            mime_type: url.mime_type,
        },
        // ponytail: a `file` source with no payload is not representable in core,
        // so it decodes to an empty url source rather than erroring the stream.
        Some(pb::input_content_source::Source::File(file)) => PartSource::File {
            value: file.value,
            provider: file.provider,
            mime_type: file.mime_type,
        },
        None => PartSource::Data {
            value: String::new(),
            mime_type: String::new(),
        },
    }
}

fn media_to_proto(
    id: &Option<String>,
    source: &PartSource,
    metadata: &Option<serde_json::Value>,
) -> pb::MediaInputPart {
    pb::MediaInputPart {
        source: Some(source_to_proto(source)),
        metadata: metadata.as_ref().map(json_to_proto),
        id: id.clone(),
    }
}

fn content_part_to_proto(part: &ContentPart) -> pb::InputContent {
    let inner = match part {
        ContentPart::Text { id, text, metadata } => {
            pb::input_content::Part::Text(pb::TextInputPart {
                text: text.clone(),
                id: id.clone(),
                metadata: metadata.as_ref().map(json_to_proto),
            })
        }
        ContentPart::Image {
            id,
            source,
            metadata,
        } => pb::input_content::Part::Image(media_to_proto(id, source, metadata)),
        ContentPart::Audio {
            id,
            source,
            metadata,
        } => pb::input_content::Part::Audio(media_to_proto(id, source, metadata)),
        ContentPart::Video {
            id,
            source,
            metadata,
        } => pb::input_content::Part::Video(media_to_proto(id, source, metadata)),
        ContentPart::Document {
            id,
            source,
            metadata,
        } => pb::input_content::Part::Document(media_to_proto(id, source, metadata)),
    };
    pb::InputContent { part: Some(inner) }
}

fn content_part_from_proto(part: pb::InputContent) -> ContentPart {
    // ponytail: a set-less `InputContent` decodes as an empty text part rather
    // than failing the whole stream.
    match part
        .part
        .unwrap_or(pb::input_content::Part::Text(pb::TextInputPart::default()))
    {
        pb::input_content::Part::Text(t) => ContentPart::Text {
            id: t.id,
            text: t.text,
            metadata: t.metadata.as_ref().map(proto_to_json),
        },
        pb::input_content::Part::Image(m) => media_from_proto(m, ContentPartKind::Image),
        pb::input_content::Part::Audio(m) => media_from_proto(m, ContentPartKind::Audio),
        pb::input_content::Part::Video(m) => media_from_proto(m, ContentPartKind::Video),
        pb::input_content::Part::Document(m) => media_from_proto(m, ContentPartKind::Document),
    }
}

enum ContentPartKind {
    Image,
    Audio,
    Video,
    Document,
}

fn media_from_proto(m: pb::MediaInputPart, kind: ContentPartKind) -> ContentPart {
    let source = source_from_proto(m.source);
    let metadata = m.metadata.as_ref().map(proto_to_json);
    match kind {
        ContentPartKind::Image => ContentPart::Image {
            id: m.id,
            source,
            metadata,
        },
        ContentPartKind::Audio => ContentPart::Audio {
            id: m.id,
            source,
            metadata,
        },
        ContentPartKind::Video => ContentPart::Video {
            id: m.id,
            source,
            metadata,
        },
        ContentPartKind::Document => ContentPart::Document {
            id: m.id,
            source,
            metadata,
        },
    }
}

/// The `content` / `content_parts` pair is a union in the schema: a body is
/// either a bare string or a part list, and only one of the two slots is set.
enum ProtoBody {
    Text(String),
    Parts(Vec<ContentPart>),
}

fn body_from_proto(content: Option<String>, parts: Vec<pb::InputContent>) -> ProtoBody {
    if parts.is_empty() {
        ProtoBody::Text(content.unwrap_or_default())
    } else {
        ProtoBody::Parts(parts.into_iter().map(content_part_from_proto).collect())
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
        Message::Developer(m) => {
            proto.content = Some(m.content.clone());
            proto.name = m.name.clone();
            proto.encrypted_value = m.encrypted_value.clone();
            proto.subagent_run_id = m.subagent_run_id.clone();
            proto.metadata = m.metadata.as_ref().map(struct_of);
        }
        Message::System(m) => {
            proto.content = Some(m.content.clone());
            proto.name = m.name.clone();
            proto.encrypted_value = m.encrypted_value.clone();
            proto.subagent_run_id = m.subagent_run_id.clone();
            proto.metadata = m.metadata.as_ref().map(struct_of);
        }
        Message::Assistant(m) => {
            proto.content = m.content.clone();
            proto.name = m.name.clone();
            if let Some(tool_calls) = &m.tool_calls {
                proto.tool_calls = tool_calls.iter().map(tool_call_to_proto).collect();
            }
            proto.encrypted_value = m.encrypted_value.clone();
            proto.subagent_run_id = m.subagent_run_id.clone();
            proto.metadata = m.metadata.as_ref().map(struct_of);
        }
        Message::User(m) => {
            match &m.content {
                UserMessageContent::Text(text) => proto.content = Some(text.clone()),
                UserMessageContent::Parts(parts) => {
                    proto.content_parts = parts.iter().map(content_part_to_proto).collect();
                }
            }
            proto.name = m.name.clone();
            proto.encrypted_value = m.encrypted_value.clone();
            proto.subagent_run_id = m.subagent_run_id.clone();
            proto.metadata = m.metadata.as_ref().map(struct_of);
        }
        Message::Tool(m) => {
            match &m.content {
                ToolResultContent::Text(text) => proto.content = Some(text.clone()),
                ToolResultContent::Parts(parts) => {
                    proto.content_parts = parts.iter().map(content_part_to_proto).collect();
                }
            }
            proto.tool_call_id = Some(m.tool_call_id.clone());
            proto.error = m.error.clone();
            proto.encrypted_value = m.encrypted_value.clone();
            proto.subagent_run_id = m.subagent_run_id.clone();
            proto.metadata = m.metadata.as_ref().map(struct_of);
        }
        Message::Reasoning(m) => {
            proto.content = Some(m.content.clone());
            proto.encrypted_value = m.encrypted_value.clone();
            proto.subagent_run_id = m.subagent_run_id.clone();
            proto.metadata = m.metadata.as_ref().map(struct_of);
        }
        Message::Activity(m) => {
            proto.activity_type = Some(m.activity_type.clone());
            proto.activity_content = Some(json_to_struct(&serde_json::Value::Object(
                m.content.clone(),
            )));
            proto.subagent_run_id = m.subagent_run_id.clone();
            proto.metadata = m.metadata.as_ref().map(struct_of);
        }
    }
    proto
}

fn struct_of(map: &serde_json::Map<String, serde_json::Value>) -> prost_types::Struct {
    json_to_struct(&serde_json::Value::Object(map.clone()))
}

fn map_of(structure: &prost_types::Struct) -> serde_json::Map<String, serde_json::Value> {
    match struct_to_json(structure) {
        serde_json::Value::Object(map) => map,
        _ => serde_json::Map::new(),
    }
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
    let subagent_run_id = proto.subagent_run_id;
    let encrypted_value = proto.encrypted_value;
    let metadata = proto.metadata.as_ref().map(map_of);
    let body = body_from_proto(proto.content, proto.content_parts);
    let message = match proto.role.as_str() {
        "developer" => Message::Developer(DeveloperMessage {
            id,
            content: text_of(body),
            name: proto.name,
            encrypted_value,
            subagent_run_id,
            metadata,
        }),
        "system" => Message::System(SystemMessage {
            id,
            content: text_of(body),
            name: proto.name,
            encrypted_value,
            subagent_run_id,
            metadata,
        }),
        "assistant" => Message::Assistant(AssistantMessage {
            id,
            content: match body {
                ProtoBody::Text(text) => Some(text),
                ProtoBody::Parts(_) => None,
            },
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
            encrypted_value,
            subagent_run_id,
            metadata,
        }),
        "user" => {
            let content = match body {
                ProtoBody::Text(text) => UserMessageContent::Text(text),
                ProtoBody::Parts(parts) => UserMessageContent::Parts(parts),
            };
            Message::User(UserMessage {
                id,
                content,
                name: proto.name,
                encrypted_value,
                subagent_run_id,
                metadata,
            })
        }
        "tool" => {
            let content = match body {
                ProtoBody::Text(text) => ToolResultContent::Text(text),
                ProtoBody::Parts(parts) => ToolResultContent::Parts(parts),
            };
            Message::Tool(ToolMessage {
                id,
                content,
                tool_call_id: proto.tool_call_id.unwrap_or_default(),
                error: proto.error,
                encrypted_value,
                subagent_run_id,
                metadata,
            })
        }
        "reasoning" => Message::Reasoning(ReasoningMessage {
            id,
            content: text_of(body),
            encrypted_value,
            subagent_run_id,
            metadata,
        }),
        "activity" => {
            let content = proto
                .activity_content
                .as_ref()
                .map(map_of)
                .unwrap_or_default();
            Message::Activity(ActivityMessage {
                id,
                activity_type: proto.activity_type.unwrap_or_default(),
                content,
                subagent_run_id,
                metadata,
            })
        }
        other => {
            return Err(AgUiError::protocol(format!(
                "protobuf message has unknown role '{other}'"
            )))
        }
    };
    Ok(message)
}

/// `developer`, `system` and `reasoning` carry a bare string only, so a part
/// list on the wire is flattened to its text.
fn text_of(body: ProtoBody) -> String {
    match body {
        ProtoBody::Text(text) => text,
        ProtoBody::Parts(parts) => parts
            .iter()
            .filter_map(|p| match p {
                ContentPart::Text { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .concat(),
    }
}

// ----- run agent input -----

fn run_input_to_proto(input: &RunAgentInput) -> pb::RunAgentInput {
    pb::RunAgentInput {
        thread_id: input.thread_id.clone(),
        run_id: input.run_id.clone(),
        parent_run_id: input.parent_run_id.clone(),
        state: input.state.as_ref().map(json_to_proto),
        messages: input.messages.iter().map(message_to_proto).collect(),
        tools: input.tools.iter().map(tool_to_proto).collect(),
        context: input.context.iter().map(context_to_proto).collect(),
        forwarded_props: input.forwarded_props.as_ref().map(json_to_proto),
        resume: input.resume.iter().flatten().map(resume_to_proto).collect(),
        protocol_version: input.protocol_version.clone(),
    }
}

fn tool_to_proto(tool: &Tool) -> pb::Tool {
    pb::Tool {
        name: tool.name.clone(),
        description: tool.description.clone(),
        parameters: Some(json_to_proto(&tool.parameters)),
        metadata: tool.metadata.as_ref().map(struct_of),
    }
}

fn context_to_proto(context: &Context) -> pb::Context {
    pb::Context {
        description: context.description.clone(),
        value: context.value.clone(),
    }
}

fn resume_to_proto(entry: &ResumeEntry) -> pb::ResumeEntry {
    pb::ResumeEntry {
        interrupt_id: entry.interrupt_id.clone(),
        status: resume_status_str(entry.status).to_string(),
        payload: entry.payload.as_ref().map(json_to_proto),
        metadata: entry.metadata.as_ref().map(struct_of),
    }
}

fn resume_status_str(status: ResumeStatus) -> &'static str {
    match status {
        ResumeStatus::Resolved => "resolved",
        ResumeStatus::Cancelled => "cancelled",
    }
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
        subagent_run_id: interrupt.subagent_run_id.clone(),
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
        subagent_run_id: proto.subagent_run_id,
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
    #[allow(deprecated)]
    let e = match event {
        Event::TextMessageStart(ev) => PE::TextMessageStart(pb::TextMessageStartEvent {
            base_event: Some(base_to_proto(&ev.base, pb::EventType::TextMessageStart)),
            message_id: ev.message_id.clone(),
            role: Some(text_role_str(ev.role).to_string()),
            name: ev.name.clone(),
            subagent_run_id: subagent_id(&ev.attributable),
        }),
        Event::TextMessageContent(ev) => PE::TextMessageContent(pb::TextMessageContentEvent {
            base_event: Some(base_to_proto(&ev.base, pb::EventType::TextMessageContent)),
            message_id: ev.message_id.clone(),
            delta: ev.delta.clone(),
            subagent_run_id: subagent_id(&ev.attributable),
        }),
        Event::TextMessageEnd(ev) => PE::TextMessageEnd(pb::TextMessageEndEvent {
            base_event: Some(base_to_proto(&ev.base, pb::EventType::TextMessageEnd)),
            message_id: ev.message_id.clone(),
            subagent_run_id: subagent_id(&ev.attributable),
        }),
        Event::TextMessageChunk(ev) => PE::TextMessageChunk(pb::TextMessageChunkEvent {
            base_event: Some(base_to_proto(&ev.base, pb::EventType::TextMessageChunk)),
            message_id: ev.message_id.clone(),
            role: ev.role.map(|r| text_role_str(r).to_string()),
            delta: ev.delta.clone(),
            name: ev.name.clone(),
            subagent_run_id: subagent_id(&ev.attributable),
        }),
        Event::ToolCallStart(ev) => PE::ToolCallStart(pb::ToolCallStartEvent {
            base_event: Some(base_to_proto(&ev.base, pb::EventType::ToolCallStart)),
            tool_call_id: ev.tool_call_id.clone(),
            tool_call_name: ev.tool_call_name.clone(),
            parent_message_id: ev.parent_message_id.clone(),
            subagent_run_id: subagent_id(&ev.attributable),
        }),
        Event::ToolCallArgs(ev) => PE::ToolCallArgs(pb::ToolCallArgsEvent {
            base_event: Some(base_to_proto(&ev.base, pb::EventType::ToolCallArgs)),
            tool_call_id: ev.tool_call_id.clone(),
            delta: ev.delta.clone(),
            subagent_run_id: subagent_id(&ev.attributable),
        }),
        Event::ToolCallEnd(ev) => PE::ToolCallEnd(pb::ToolCallEndEvent {
            base_event: Some(base_to_proto(&ev.base, pb::EventType::ToolCallEnd)),
            tool_call_id: ev.tool_call_id.clone(),
            subagent_run_id: subagent_id(&ev.attributable),
        }),
        Event::ToolCallChunk(ev) => PE::ToolCallChunk(pb::ToolCallChunkEvent {
            base_event: Some(base_to_proto(&ev.base, pb::EventType::ToolCallChunk)),
            tool_call_id: ev.tool_call_id.clone(),
            tool_call_name: ev.tool_call_name.clone(),
            parent_message_id: ev.parent_message_id.clone(),
            delta: ev.delta.clone(),
            subagent_run_id: subagent_id(&ev.attributable),
        }),
        Event::ToolCallResult(ev) => PE::ToolCallResult(pb::ToolCallResultEvent {
            base_event: Some(base_to_proto(&ev.base, pb::EventType::ToolCallResult)),
            subagent_run_id: subagent_id(&ev.attributable),
            message_id: ev.message_id.clone(),
            tool_call_id: ev.tool_call_id.clone(),
            content: match &ev.content {
                ToolResultContent::Text(text) => Some(text.clone()),
                ToolResultContent::Parts(_) => None,
            },
            role: ev.role.map(|_| "tool".to_string()),
            content_parts: match &ev.content {
                ToolResultContent::Parts(parts) => {
                    parts.iter().map(content_part_to_proto).collect()
                }
                ToolResultContent::Text(_) => Vec::new(),
            },
        }),
        Event::StateSnapshot(ev) => PE::StateSnapshot(pb::StateSnapshotEvent {
            base_event: Some(base_to_proto(&ev.base, pb::EventType::StateSnapshot)),
            snapshot: Some(json_to_proto(&ev.snapshot)),
            subagent_run_id: subagent_id(&ev.attributable),
        }),
        Event::StateDelta(ev) => PE::StateDelta(pb::StateDeltaEvent {
            base_event: Some(base_to_proto(&ev.base, pb::EventType::StateDelta)),
            delta: ev
                .delta
                .iter()
                .map(patch_op_to_proto)
                .collect::<Result<_>>()?,
            subagent_run_id: subagent_id(&ev.attributable),
        }),
        Event::MessagesSnapshot(ev) => PE::MessagesSnapshot(pb::MessagesSnapshotEvent {
            base_event: Some(base_to_proto(&ev.base, pb::EventType::MessagesSnapshot)),
            messages: ev.messages.iter().map(message_to_proto).collect(),
        }),
        Event::ActivitySnapshot(ev) => PE::ActivitySnapshot(pb::ActivitySnapshotEvent {
            base_event: Some(base_to_proto(&ev.base, pb::EventType::ActivitySnapshot)),
            subagent_run_id: subagent_id(&ev.attributable),
            message_id: ev.message_id.clone(),
            activity_type: ev.activity_type.clone(),
            content: Some(json_to_struct(&serde_json::Value::Object(
                ev.content.clone(),
            ))),
            replace: ev.replace,
        }),
        Event::ActivityDelta(ev) => PE::ActivityDelta(pb::ActivityDeltaEvent {
            base_event: Some(base_to_proto(&ev.base, pb::EventType::ActivityDelta)),
            subagent_run_id: subagent_id(&ev.attributable),
            message_id: ev.message_id.clone(),
            activity_type: ev.activity_type.clone(),
            patch: ev
                .patch
                .iter()
                .map(patch_op_to_proto)
                .collect::<Result<_>>()?,
        }),
        Event::Raw(ev) => PE::Raw(pb::RawEvent {
            base_event: Some(base_to_proto(&ev.base, pb::EventType::Raw)),
            event: Some(json_to_proto(&ev.event)),
            source: ev.source.clone(),
            subagent_run_id: subagent_id(&ev.attributable),
        }),
        Event::Custom(ev) => PE::Custom(pb::CustomEvent {
            base_event: Some(base_to_proto(&ev.base, pb::EventType::Custom)),
            name: ev.name.clone(),
            value: Some(json_to_proto(&ev.value)),
            subagent_run_id: subagent_id(&ev.attributable),
        }),
        Event::RunStarted(ev) => PE::RunStarted(pb::RunStartedEvent {
            base_event: Some(base_to_proto(&ev.base, pb::EventType::RunStarted)),
            thread_id: ev.thread_id.clone(),
            run_id: ev.run_id.clone(),
            parent_run_id: ev.parent_run_id.clone(),
            input: ev.input.as_ref().map(run_input_to_proto),
            protocol_version: ev.protocol_version.clone(),
        }),
        Event::RunFinished(ev) => {
            let (outcome, interrupts, pending_tool_call_ids) = match &ev.outcome {
                None => (String::new(), Vec::new(), Vec::new()),
                Some(RunFinishedOutcome::Success {
                    pending_tool_call_ids,
                }) => (
                    "success".to_string(),
                    Vec::new(),
                    pending_tool_call_ids.clone().unwrap_or_default(),
                ),
                Some(RunFinishedOutcome::Interrupt { interrupts }) => (
                    "interrupt".to_string(),
                    interrupts.iter().map(interrupt_to_proto).collect(),
                    Vec::new(),
                ),
                Some(RunFinishedOutcome::Cancelled) => {
                    ("cancelled".to_string(), Vec::new(), Vec::new())
                }
            };
            PE::RunFinished(pb::RunFinishedEvent {
                base_event: Some(base_to_proto(&ev.base, pb::EventType::RunFinished)),
                thread_id: ev.thread_id.clone(),
                run_id: ev.run_id.clone(),
                result: ev.result.as_ref().map(json_to_proto),
                outcome,
                interrupts,
                usage: usage_to_proto(&ev.usage),
                pending_tool_call_ids,
            })
        }
        Event::RunError(ev) => PE::RunError(pb::RunErrorEvent {
            base_event: Some(base_to_proto(&ev.base, pb::EventType::RunError)),
            code: ev.code.clone(),
            message: ev.message.clone(),
            usage: usage_to_proto(&ev.usage),
        }),
        Event::StepStarted(ev) => PE::StepStarted(pb::StepStartedEvent {
            base_event: Some(base_to_proto(&ev.base, pb::EventType::StepStarted)),
            step_name: ev.step_name.clone(),
            subagent_run_id: subagent_id(&ev.attributable),
        }),
        Event::StepFinished(ev) => PE::StepFinished(pb::StepFinishedEvent {
            base_event: Some(base_to_proto(&ev.base, pb::EventType::StepFinished)),
            step_name: ev.step_name.clone(),
            subagent_run_id: subagent_id(&ev.attributable),
        }),
        Event::ReasoningStart(ev) => PE::ReasoningStart(pb::ReasoningStartEvent {
            base_event: Some(base_to_proto(&ev.base, pb::EventType::ReasoningStart)),
            subagent_run_id: subagent_id(&ev.attributable),
            message_id: ev.message_id.clone(),
        }),
        Event::ReasoningMessageStart(ev) => {
            PE::ReasoningMessageStart(pb::ReasoningMessageStartEvent {
                base_event: Some(base_to_proto(
                    &ev.base,
                    pb::EventType::ReasoningMessageStart,
                )),
                subagent_run_id: subagent_id(&ev.attributable),
                message_id: ev.message_id.clone(),
                role: "reasoning".to_string(),
            })
        }
        Event::ReasoningMessageContent(ev) => {
            PE::ReasoningMessageContent(pb::ReasoningMessageContentEvent {
                base_event: Some(base_to_proto(
                    &ev.base,
                    pb::EventType::ReasoningMessageContent,
                )),
                subagent_run_id: subagent_id(&ev.attributable),
                message_id: ev.message_id.clone(),
                delta: ev.delta.clone(),
            })
        }
        Event::ReasoningMessageEnd(ev) => PE::ReasoningMessageEnd(pb::ReasoningMessageEndEvent {
            base_event: Some(base_to_proto(&ev.base, pb::EventType::ReasoningMessageEnd)),
            subagent_run_id: subagent_id(&ev.attributable),
            message_id: ev.message_id.clone(),
        }),
        Event::ReasoningMessageChunk(ev) => {
            PE::ReasoningMessageChunk(pb::ReasoningMessageChunkEvent {
                base_event: Some(base_to_proto(
                    &ev.base,
                    pb::EventType::ReasoningMessageChunk,
                )),
                subagent_run_id: subagent_id(&ev.attributable),
                message_id: ev.message_id.clone(),
                delta: ev.delta.clone(),
            })
        }
        Event::ReasoningEnd(ev) => PE::ReasoningEnd(pb::ReasoningEndEvent {
            base_event: Some(base_to_proto(&ev.base, pb::EventType::ReasoningEnd)),
            subagent_run_id: subagent_id(&ev.attributable),
            message_id: ev.message_id.clone(),
        }),
        Event::ReasoningEncryptedValue(ev) => {
            PE::ReasoningEncryptedValue(pb::ReasoningEncryptedValueEvent {
                base_event: Some(base_to_proto(
                    &ev.base,
                    pb::EventType::ReasoningEncryptedValue,
                )),
                subagent_run_id: subagent_id(&ev.attributable),
                subtype: match ev.subtype {
                    ReasoningEncryptedValueSubtype::ToolCall => "toolCall",
                    ReasoningEncryptedValueSubtype::Message => "message",
                }
                .to_string(),
                entity_id: ev.entity_id.clone(),
                encrypted_value: ev.encrypted_value.clone(),
            })
        }
        Event::SubagentStarted(ev) => PE::SubagentStarted(pb::SubagentStartedEvent {
            base_event: Some(base_to_proto(&ev.base, pb::EventType::SubagentStarted)),
            subagent_run_id: ev.subagent_run_id.clone(),
            name: ev.name.clone(),
            description: ev.description.clone(),
            parent_subagent_run_id: ev.parent_subagent_run_id.clone(),
            parent_tool_call_id: ev.parent_tool_call_id.clone(),
            parent_message_id: ev.parent_message_id.clone(),
        }),
        Event::SubagentFinished(ev) => {
            let outcome = match &ev.outcome {
                None => String::new(),
                Some(SubagentFinishedOutcome::Success) => "success".to_string(),
                Some(SubagentFinishedOutcome::Suspended { .. }) => "suspended".to_string(),
            };
            let interrupt_ids = match &ev.outcome {
                Some(SubagentFinishedOutcome::Suspended { interrupt_ids }) => interrupt_ids.clone(),
                _ => Vec::new(),
            };
            PE::SubagentFinished(pb::SubagentFinishedEvent {
                base_event: Some(base_to_proto(&ev.base, pb::EventType::SubagentFinished)),
                subagent_run_id: ev.subagent_run_id.clone(),
                result: ev.result.as_ref().map(json_to_proto),
                outcome,
                interrupt_ids,
            })
        }
        Event::SubagentError(ev) => PE::SubagentError(pb::SubagentErrorEvent {
            base_event: Some(base_to_proto(&ev.base, pb::EventType::SubagentError)),
            subagent_run_id: ev.subagent_run_id.clone(),
            message: ev.message.clone(),
            code: ev.code.clone(),
        }),
    };
    Ok(e)
}

fn from_proto_event(event: pb::event::Event) -> Result<Event> {
    use agui_rs_core::{
        ActivityDeltaEvent, ActivitySnapshotEvent, CustomEvent, MessagesSnapshotEvent, RawEvent,
        ReasoningEncryptedValueEvent, ReasoningEndEvent, ReasoningMessageChunkEvent,
        ReasoningMessageContentEvent, ReasoningMessageEndEvent, ReasoningMessageStartEvent,
        ReasoningStartEvent, RunErrorEvent, RunFinishedEvent, RunStartedEvent, StateDeltaEvent,
        StateSnapshotEvent, StepFinishedEvent, StepStartedEvent, SubagentErrorEvent,
        SubagentFinishedEvent, SubagentStartedEvent, TextMessageChunkEvent,
        TextMessageContentEvent, TextMessageEndEvent, TextMessageStartEvent, ToolCallArgsEvent,
        ToolCallChunkEvent, ToolCallEndEvent, ToolCallResultEvent, ToolCallStartEvent,
    };
    use pb::event::Event as PE;
    let event = match event {
        PE::TextMessageStart(ev) => Event::TextMessageStart(TextMessageStartEvent {
            message_id: ev.message_id,
            role: parse_text_role(ev.role.as_deref()),
            name: ev.name,
            base: base_from_proto(ev.base_event),
            attributable: attributable_from_proto(ev.subagent_run_id),
        }),
        PE::TextMessageContent(ev) => Event::TextMessageContent(TextMessageContentEvent {
            message_id: ev.message_id,
            delta: ev.delta,
            base: base_from_proto(ev.base_event),
            attributable: attributable_from_proto(ev.subagent_run_id),
        }),
        PE::TextMessageEnd(ev) => Event::TextMessageEnd(TextMessageEndEvent {
            message_id: ev.message_id,
            base: base_from_proto(ev.base_event),
            attributable: attributable_from_proto(ev.subagent_run_id),
        }),
        PE::TextMessageChunk(ev) => Event::TextMessageChunk(TextMessageChunkEvent {
            message_id: ev.message_id,
            role: ev.role.as_deref().map(|r| parse_text_role(Some(r))),
            delta: ev.delta,
            name: ev.name,
            base: base_from_proto(ev.base_event),
            attributable: attributable_from_proto(ev.subagent_run_id),
        }),
        PE::ToolCallStart(ev) => Event::ToolCallStart(ToolCallStartEvent {
            tool_call_id: ev.tool_call_id,
            tool_call_name: ev.tool_call_name,
            parent_message_id: ev.parent_message_id,
            base: base_from_proto(ev.base_event),
            attributable: attributable_from_proto(ev.subagent_run_id),
        }),
        PE::ToolCallArgs(ev) => Event::ToolCallArgs(ToolCallArgsEvent {
            tool_call_id: ev.tool_call_id,
            delta: ev.delta,
            base: base_from_proto(ev.base_event),
            attributable: attributable_from_proto(ev.subagent_run_id),
        }),
        PE::ToolCallEnd(ev) => Event::ToolCallEnd(ToolCallEndEvent {
            tool_call_id: ev.tool_call_id,
            base: base_from_proto(ev.base_event),
            attributable: attributable_from_proto(ev.subagent_run_id),
        }),
        PE::ToolCallChunk(ev) => Event::ToolCallChunk(ToolCallChunkEvent {
            tool_call_id: ev.tool_call_id,
            tool_call_name: ev.tool_call_name,
            parent_message_id: ev.parent_message_id,
            delta: ev.delta,
            base: base_from_proto(ev.base_event),
            attributable: attributable_from_proto(ev.subagent_run_id),
        }),
        PE::ToolCallResult(ev) => {
            let content = match body_from_proto(ev.content, ev.content_parts) {
                ProtoBody::Text(text) => ToolResultContent::Text(text),
                ProtoBody::Parts(parts) => ToolResultContent::Parts(parts),
            };
            Event::ToolCallResult(ToolCallResultEvent {
                message_id: ev.message_id,
                tool_call_id: ev.tool_call_id,
                content,
                role: ev.role.as_deref().map(|_| ToolResultRole::Tool),
                base: base_from_proto(ev.base_event),
                attributable: attributable_from_proto(ev.subagent_run_id),
            })
        }
        PE::StateSnapshot(ev) => Event::StateSnapshot(StateSnapshotEvent {
            snapshot: ev
                .snapshot
                .as_ref()
                .map(proto_to_json)
                .unwrap_or(serde_json::Value::Null),
            base: base_from_proto(ev.base_event),
            attributable: attributable_from_proto(ev.subagent_run_id),
        }),
        PE::StateDelta(ev) => Event::StateDelta(StateDeltaEvent {
            delta: ev.delta.into_iter().map(patch_op_from_proto).collect(),
            base: base_from_proto(ev.base_event),
            attributable: attributable_from_proto(ev.subagent_run_id),
        }),
        PE::MessagesSnapshot(ev) => {
            let messages = ev
                .messages
                .into_iter()
                .map(message_from_proto)
                .collect::<Result<Vec<_>>>()?;
            Event::MessagesSnapshot(MessagesSnapshotEvent {
                messages,
                base: base_from_proto(ev.base_event),
            })
        }
        PE::ActivitySnapshot(ev) => Event::ActivitySnapshot(ActivitySnapshotEvent {
            message_id: ev.message_id,
            activity_type: ev.activity_type,
            content: ev.content.as_ref().map(map_of).unwrap_or_default(),
            replace: ev.replace,
            base: base_from_proto(ev.base_event),
            attributable: attributable_from_proto(ev.subagent_run_id),
        }),
        PE::ActivityDelta(ev) => Event::ActivityDelta(ActivityDeltaEvent {
            message_id: ev.message_id,
            activity_type: ev.activity_type,
            patch: ev.patch.into_iter().map(patch_op_from_proto).collect(),
            base: base_from_proto(ev.base_event),
            attributable: attributable_from_proto(ev.subagent_run_id),
        }),
        PE::Raw(ev) => Event::Raw(RawEvent {
            event: ev
                .event
                .as_ref()
                .map(proto_to_json)
                .unwrap_or(serde_json::Value::Null),
            source: ev.source,
            base: base_from_proto(ev.base_event),
            attributable: attributable_from_proto(ev.subagent_run_id),
        }),
        PE::Custom(ev) => Event::Custom(CustomEvent {
            name: ev.name,
            value: ev
                .value
                .as_ref()
                .map(proto_to_json)
                .unwrap_or(serde_json::Value::Null),
            base: base_from_proto(ev.base_event),
            attributable: attributable_from_proto(ev.subagent_run_id),
        }),
        PE::RunStarted(ev) => Event::RunStarted(RunStartedEvent {
            thread_id: ev.thread_id,
            run_id: ev.run_id,
            protocol_version: ev.protocol_version,
            parent_run_id: ev.parent_run_id,
            input: ev.input.map(run_input_from_proto),
            base: base_from_proto(ev.base_event),
        }),
        PE::RunFinished(ev) => {
            let outcome = match ev.outcome.as_str() {
                "success" => Some(RunFinishedOutcome::Success {
                    pending_tool_call_ids: (!ev.pending_tool_call_ids.is_empty())
                        .then(|| ev.pending_tool_call_ids.clone()),
                }),
                "interrupt" => Some(RunFinishedOutcome::Interrupt {
                    interrupts: ev
                        .interrupts
                        .into_iter()
                        .map(interrupt_from_proto)
                        .collect(),
                }),
                "cancelled" => Some(RunFinishedOutcome::Cancelled),
                _ => None,
            };
            Event::RunFinished(RunFinishedEvent {
                thread_id: ev.thread_id,
                run_id: ev.run_id,
                result: ev.result.as_ref().map(proto_to_json),
                outcome,
                usage: usage_from_proto(ev.usage),
                base: base_from_proto(ev.base_event),
            })
        }
        PE::RunError(ev) => Event::RunError(RunErrorEvent {
            message: ev.message,
            code: ev.code,
            usage: usage_from_proto(ev.usage),
            base: base_from_proto(ev.base_event),
        }),
        PE::StepStarted(ev) => Event::StepStarted(StepStartedEvent {
            step_name: ev.step_name,
            base: base_from_proto(ev.base_event),
            attributable: attributable_from_proto(ev.subagent_run_id),
        }),
        PE::StepFinished(ev) => Event::StepFinished(StepFinishedEvent {
            step_name: ev.step_name,
            base: base_from_proto(ev.base_event),
            attributable: attributable_from_proto(ev.subagent_run_id),
        }),
        PE::ReasoningStart(ev) => Event::ReasoningStart(ReasoningStartEvent {
            message_id: ev.message_id,
            base: base_from_proto(ev.base_event),
            attributable: attributable_from_proto(ev.subagent_run_id),
        }),
        PE::ReasoningMessageStart(ev) => Event::ReasoningMessageStart(ReasoningMessageStartEvent {
            message_id: ev.message_id,
            role: ReasoningMessageRole::Reasoning,
            base: base_from_proto(ev.base_event),
            attributable: attributable_from_proto(ev.subagent_run_id),
        }),
        PE::ReasoningMessageContent(ev) => {
            Event::ReasoningMessageContent(ReasoningMessageContentEvent {
                message_id: ev.message_id,
                delta: ev.delta,
                base: base_from_proto(ev.base_event),
                attributable: attributable_from_proto(ev.subagent_run_id),
            })
        }
        PE::ReasoningMessageEnd(ev) => Event::ReasoningMessageEnd(ReasoningMessageEndEvent {
            message_id: ev.message_id,
            base: base_from_proto(ev.base_event),
            attributable: attributable_from_proto(ev.subagent_run_id),
        }),
        PE::ReasoningMessageChunk(ev) => Event::ReasoningMessageChunk(ReasoningMessageChunkEvent {
            message_id: ev.message_id,
            delta: ev.delta,
            base: base_from_proto(ev.base_event),
            attributable: attributable_from_proto(ev.subagent_run_id),
        }),
        PE::ReasoningEnd(ev) => Event::ReasoningEnd(ReasoningEndEvent {
            message_id: ev.message_id,
            base: base_from_proto(ev.base_event),
            attributable: attributable_from_proto(ev.subagent_run_id),
        }),
        PE::ReasoningEncryptedValue(ev) => {
            Event::ReasoningEncryptedValue(ReasoningEncryptedValueEvent {
                subtype: match ev.subtype.as_str() {
                    "message" => ReasoningEncryptedValueSubtype::Message,
                    _ => ReasoningEncryptedValueSubtype::ToolCall,
                },
                entity_id: ev.entity_id,
                encrypted_value: ev.encrypted_value,
                base: base_from_proto(ev.base_event),
                attributable: attributable_from_proto(ev.subagent_run_id),
            })
        }
        PE::SubagentStarted(ev) => Event::SubagentStarted(SubagentStartedEvent {
            subagent_run_id: ev.subagent_run_id,
            name: ev.name,
            description: ev.description,
            parent_subagent_run_id: ev.parent_subagent_run_id,
            parent_tool_call_id: ev.parent_tool_call_id,
            parent_message_id: ev.parent_message_id,
            base: base_from_proto(ev.base_event),
        }),
        PE::SubagentFinished(ev) => {
            let outcome = match ev.outcome.as_str() {
                "success" => Some(SubagentFinishedOutcome::Success),
                "suspended" => Some(SubagentFinishedOutcome::Suspended {
                    interrupt_ids: ev.interrupt_ids.clone(),
                }),
                _ => None,
            };
            Event::SubagentFinished(SubagentFinishedEvent {
                subagent_run_id: ev.subagent_run_id,
                result: ev.result.as_ref().map(proto_to_json),
                outcome,
                base: base_from_proto(ev.base_event),
            })
        }
        PE::SubagentError(ev) => Event::SubagentError(SubagentErrorEvent {
            subagent_run_id: ev.subagent_run_id,
            message: ev.message,
            code: ev.code,
            base: base_from_proto(ev.base_event),
        }),
    };
    Ok(event)
}

fn run_input_from_proto(proto: pb::RunAgentInput) -> RunAgentInput {
    RunAgentInput {
        thread_id: proto.thread_id,
        run_id: proto.run_id,
        protocol_version: proto.protocol_version,
        parent_run_id: proto.parent_run_id,
        state: proto.state.as_ref().map(proto_to_json),
        messages: proto
            .messages
            .into_iter()
            .filter_map(|m| message_from_proto(m).ok())
            .collect(),
        tools: proto
            .tools
            .into_iter()
            .map(|t| Tool {
                name: t.name,
                description: t.description,
                parameters: t
                    .parameters
                    .as_ref()
                    .map(proto_to_json)
                    .unwrap_or(serde_json::Value::Null),
                metadata: t.metadata.as_ref().map(map_of),
            })
            .collect(),
        context: proto
            .context
            .into_iter()
            .map(|c| Context {
                description: c.description,
                value: c.value,
            })
            .collect(),
        forwarded_props: proto.forwarded_props.as_ref().map(proto_to_json),
        resume: {
            let resume: Vec<ResumeEntry> = proto
                .resume
                .into_iter()
                .map(|r| ResumeEntry {
                    interrupt_id: r.interrupt_id,
                    status: match r.status.as_str() {
                        "cancelled" => ResumeStatus::Cancelled,
                        _ => ResumeStatus::Resolved,
                    },
                    payload: r.payload.as_ref().map(proto_to_json),
                    metadata: r.metadata.as_ref().map(map_of),
                })
                .collect();
            // `None` and `Some([])` are the same thing on the wire, and core
            // distinguishes them, so an empty list goes back as absent.
            (!resume.is_empty()).then_some(resume)
        },
    }
}

fn text_role_str(role: TextMessageRole) -> &'static str {
    match role {
        TextMessageRole::Developer => "developer",
        TextMessageRole::System => "system",
        TextMessageRole::Assistant => "assistant",
        TextMessageRole::User => "user",
    }
}

fn parse_text_role(role: Option<&str>) -> TextMessageRole {
    match role {
        Some("developer") => TextMessageRole::Developer,
        Some("system") => TextMessageRole::System,
        Some("user") => TextMessageRole::User,
        _ => TextMessageRole::Assistant,
    }
}
