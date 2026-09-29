//! Hand-written `prost` message definitions mirroring the canonical AG-UI
//! `.proto` schema (`events.proto`, `types.proto`, `patch.proto`).
//!
//! Written by hand rather than generated via `prost-build` so the crate has no
//! `protoc` build-time dependency. Field numbers and wire types match the
//! canonical schema exactly, so the binary encoding is interoperable.
//!
//! These definitions are a hand-written Rust mirror of the upstream generated
//! artifacts vendored in `upstream-spec/`, taken from `ag-ui-protocol/ag-ui`
//! commit `024332cb` (blob SHAs recorded in `upstream-spec/README.md`). Upstream's
//! source of truth is `schema.json`; wire numbers come from `proto-freeze.txt`.
//!
//! Any change to a field number, wire type, `oneof` tag, enum value or reserved
//! slot must be checked against the files in `upstream-spec/` first. On an
//! upstream upgrade, re-vendor those files before editing this one.
//!
//! `tests/proto_drift.rs` parses those vendored files and fails if anything here
//! stops matching them.

use prost::Message;
use prost_types::Struct as ProtoStruct;
use prost_types::Value as ProtoValue;

// ----- patch.proto -----

#[derive(Clone, Copy, Debug, PartialEq, Eq, prost::Enumeration)]
#[repr(i32)]
pub enum JsonPatchOperationType {
    Add = 0,
    Remove = 1,
    Replace = 2,
    Move = 3,
    Copy = 4,
    Test = 5,
}

#[derive(Clone, PartialEq, Message)]
pub struct JsonPatchOperation {
    #[prost(enumeration = "JsonPatchOperationType", tag = "1")]
    pub op: i32,
    #[prost(string, tag = "2")]
    pub path: String,
    #[prost(string, optional, tag = "3")]
    pub from: Option<String>,
    #[prost(message, optional, tag = "4")]
    pub value: Option<ProtoValue>,
}

// ----- types.proto -----

#[derive(Clone, PartialEq, Message)]
pub struct TextInputPart {
    #[prost(string, tag = "1")]
    pub text: String,
    #[prost(string, optional, tag = "2")]
    pub id: Option<String>,
    #[prost(message, optional, tag = "3")]
    pub metadata: Option<ProtoValue>,
}

#[derive(Clone, PartialEq, Message)]
pub struct InputContentDataSource {
    #[prost(string, tag = "1")]
    pub value: String,
    #[prost(string, tag = "2")]
    pub mime_type: String,
}

#[derive(Clone, PartialEq, Message)]
pub struct InputContentUrlSource {
    #[prost(string, tag = "1")]
    pub value: String,
    #[prost(string, optional, tag = "2")]
    pub mime_type: Option<String>,
}

#[derive(Clone, PartialEq, Message)]
pub struct InputContentFileSource {
    #[prost(string, tag = "1")]
    pub value: String,
    #[prost(string, optional, tag = "2")]
    pub provider: Option<String>,
    #[prost(string, optional, tag = "3")]
    pub mime_type: Option<String>,
}

#[derive(Clone, PartialEq, Message)]
pub struct InputContentSource {
    #[prost(oneof = "input_content_source::Source", tags = "1, 2, 3")]
    pub source: Option<input_content_source::Source>,
}

pub mod input_content_source {
    use super::{InputContentDataSource, InputContentFileSource, InputContentUrlSource};
    #[derive(Clone, PartialEq, prost::Oneof)]
    pub enum Source {
        #[prost(message, tag = "1")]
        Data(InputContentDataSource),
        #[prost(message, tag = "2")]
        Url(InputContentUrlSource),
        #[prost(message, tag = "3")]
        File(InputContentFileSource),
    }
}

/// One `MediaInputPart`. Upstream spells this out four times — `ImageInputPart`,
/// `AudioInputPart`, `VideoInputPart`, `DocumentInputPart` — with an identical
/// body each time, so they share one Rust type. `proto_drift.rs` maps all four
/// names here, and still fails if any one of them diverges upstream.
#[derive(Clone, PartialEq, Message)]
pub struct MediaInputPart {
    #[prost(message, optional, tag = "1")]
    pub source: Option<InputContentSource>,
    #[prost(message, optional, tag = "2")]
    pub metadata: Option<ProtoValue>,
    #[prost(string, optional, tag = "3")]
    pub id: Option<String>,
}

#[derive(Clone, PartialEq, Message)]
pub struct InputContent {
    #[prost(oneof = "input_content::Part", tags = "1, 2, 3, 4, 5")]
    pub part: Option<input_content::Part>,
}

pub mod input_content {
    use super::{MediaInputPart, TextInputPart};
    #[derive(Clone, PartialEq, prost::Oneof)]
    pub enum Part {
        #[prost(message, tag = "1")]
        Text(TextInputPart),
        #[prost(message, tag = "2")]
        Image(MediaInputPart),
        #[prost(message, tag = "3")]
        Audio(MediaInputPart),
        #[prost(message, tag = "4")]
        Video(MediaInputPart),
        #[prost(message, tag = "5")]
        Document(MediaInputPart),
    }
}

/// Upstream nests this inside `ToolCall` as `ToolCall.Function`. Nesting is
/// proto-sugar for a named type: the wire bytes of a nested message are those of
/// a top-level one, so this stays flat. `proto_drift.rs` maps the two names.
#[derive(Clone, PartialEq, Message)]
pub struct ToolCallFunction {
    #[prost(string, tag = "1")]
    pub name: String,
    #[prost(string, tag = "2")]
    pub arguments: String,
}

#[derive(Clone, PartialEq, Message)]
pub struct ToolCall {
    #[prost(string, tag = "1")]
    pub id: String,
    #[prost(string, tag = "2")]
    pub r#type: String,
    #[prost(message, optional, tag = "3")]
    pub function: Option<ToolCallFunction>,
    #[prost(message, optional, tag = "4")]
    pub metadata: Option<ProtoStruct>,
    #[prost(string, optional, tag = "5")]
    pub encrypted_value: Option<String>,
}

/// Upstream's `Message`. Named `ProtoMessage` here only because `prost::Message`
/// already owns the name; `proto_drift.rs` maps the two.
#[derive(Clone, PartialEq, Message)]
pub struct ProtoMessage {
    #[prost(string, tag = "1")]
    pub id: String,
    #[prost(string, tag = "2")]
    pub role: String,
    #[prost(string, optional, tag = "3")]
    pub content: Option<String>,
    #[prost(string, optional, tag = "4")]
    pub name: Option<String>,
    #[prost(message, repeated, tag = "5")]
    pub tool_calls: Vec<ToolCall>,
    #[prost(string, optional, tag = "6")]
    pub tool_call_id: Option<String>,
    #[prost(string, optional, tag = "7")]
    pub error: Option<String>,
    #[prost(message, repeated, tag = "8")]
    pub content_parts: Vec<InputContent>,
    #[prost(message, optional, tag = "9")]
    pub metadata: Option<ProtoStruct>,
    #[prost(string, optional, tag = "10")]
    pub subagent_run_id: Option<String>,
    #[prost(string, optional, tag = "11")]
    pub encrypted_value: Option<String>,
    #[prost(string, optional, tag = "12")]
    pub activity_type: Option<String>,
    #[prost(message, optional, tag = "13")]
    pub activity_content: Option<ProtoStruct>,
}

#[derive(Clone, PartialEq, Message)]
pub struct Tool {
    #[prost(string, tag = "1")]
    pub name: String,
    #[prost(string, tag = "2")]
    pub description: String,
    #[prost(message, optional, tag = "3")]
    pub parameters: Option<ProtoValue>,
    #[prost(message, optional, tag = "4")]
    pub metadata: Option<ProtoStruct>,
}

#[derive(Clone, PartialEq, Message)]
pub struct Context {
    #[prost(string, tag = "1")]
    pub description: String,
    #[prost(string, tag = "2")]
    pub value: String,
}

#[derive(Clone, PartialEq, Message)]
pub struct ResumeEntry {
    #[prost(string, tag = "1")]
    pub interrupt_id: String,
    #[prost(string, tag = "2")]
    pub status: String,
    #[prost(message, optional, tag = "3")]
    pub payload: Option<ProtoValue>,
    #[prost(message, optional, tag = "4")]
    pub metadata: Option<ProtoStruct>,
}

#[derive(Clone, PartialEq, Message)]
pub struct RunAgentInput {
    #[prost(string, tag = "1")]
    pub thread_id: String,
    #[prost(string, tag = "2")]
    pub run_id: String,
    #[prost(string, optional, tag = "3")]
    pub parent_run_id: Option<String>,
    #[prost(message, optional, tag = "4")]
    pub state: Option<ProtoValue>,
    #[prost(message, repeated, tag = "5")]
    pub messages: Vec<ProtoMessage>,
    #[prost(message, repeated, tag = "6")]
    pub tools: Vec<Tool>,
    #[prost(message, repeated, tag = "7")]
    pub context: Vec<Context>,
    #[prost(message, optional, tag = "8")]
    pub forwarded_props: Option<ProtoValue>,
    #[prost(message, repeated, tag = "9")]
    pub resume: Vec<ResumeEntry>,
    #[prost(string, optional, tag = "10")]
    pub protocol_version: Option<String>,
}

#[derive(Clone, PartialEq, Message)]
pub struct Interrupt {
    #[prost(string, tag = "1")]
    pub id: String,
    #[prost(string, tag = "2")]
    pub reason: String,
    #[prost(string, optional, tag = "3")]
    pub message: Option<String>,
    #[prost(string, optional, tag = "4")]
    pub tool_call_id: Option<String>,
    #[prost(message, optional, tag = "5")]
    pub response_schema: Option<ProtoValue>,
    #[prost(string, optional, tag = "6")]
    pub expires_at: Option<String>,
    #[prost(message, optional, tag = "7")]
    pub metadata: Option<ProtoValue>,
    #[prost(string, optional, tag = "8")]
    pub subagent_run_id: Option<String>,
}

// ----- events.proto -----

#[derive(Clone, Copy, Debug, PartialEq, Eq, prost::Enumeration)]
#[repr(i32)]
pub enum EventType {
    TextMessageStart = 0,
    TextMessageContent = 1,
    TextMessageEnd = 2,
    ToolCallStart = 3,
    ToolCallArgs = 4,
    ToolCallEnd = 5,
    StateSnapshot = 6,
    StateDelta = 7,
    MessagesSnapshot = 8,
    Raw = 9,
    Custom = 10,
    RunStarted = 11,
    RunFinished = 12,
    RunError = 13,
    StepStarted = 14,
    StepFinished = 15,
    SubagentStarted = 16,
    SubagentFinished = 17,
    SubagentError = 18,
    TextMessageChunk = 19,
    ToolCallChunk = 20,
    ToolCallResult = 21,
    ActivitySnapshot = 22,
    ActivityDelta = 23,
    ReasoningStart = 24,
    ReasoningMessageStart = 25,
    ReasoningMessageContent = 26,
    ReasoningMessageEnd = 27,
    ReasoningMessageChunk = 28,
    ReasoningEnd = 29,
    ReasoningEncryptedValue = 30,
}

#[derive(Clone, PartialEq, Message)]
pub struct BaseEvent {
    #[prost(enumeration = "EventType", tag = "1")]
    pub r#type: i32,
    #[prost(int64, optional, tag = "2")]
    pub timestamp: Option<i64>,
    #[prost(message, optional, tag = "3")]
    pub raw_event: Option<ProtoValue>,
    #[prost(message, optional, tag = "4")]
    pub metadata: Option<ProtoStruct>,
}

#[derive(Clone, PartialEq, Message)]
pub struct TextMessageStartEvent {
    #[prost(message, optional, tag = "1")]
    pub base_event: Option<BaseEvent>,
    #[prost(string, tag = "2")]
    pub message_id: String,
    #[prost(string, optional, tag = "3")]
    pub role: Option<String>,
    #[prost(string, optional, tag = "4")]
    pub name: Option<String>,
    #[prost(string, optional, tag = "5")]
    pub subagent_run_id: Option<String>,
}

#[derive(Clone, PartialEq, Message)]
pub struct TextMessageContentEvent {
    #[prost(message, optional, tag = "1")]
    pub base_event: Option<BaseEvent>,
    #[prost(string, tag = "2")]
    pub message_id: String,
    #[prost(string, tag = "3")]
    pub delta: String,
    #[prost(string, optional, tag = "4")]
    pub subagent_run_id: Option<String>,
}

#[derive(Clone, PartialEq, Message)]
pub struct TextMessageEndEvent {
    #[prost(message, optional, tag = "1")]
    pub base_event: Option<BaseEvent>,
    #[prost(string, tag = "2")]
    pub message_id: String,
    #[prost(string, optional, tag = "3")]
    pub subagent_run_id: Option<String>,
}

#[derive(Clone, PartialEq, Message)]
pub struct ToolCallStartEvent {
    #[prost(message, optional, tag = "1")]
    pub base_event: Option<BaseEvent>,
    #[prost(string, tag = "2")]
    pub tool_call_id: String,
    #[prost(string, tag = "3")]
    pub tool_call_name: String,
    #[prost(string, optional, tag = "4")]
    pub parent_message_id: Option<String>,
    #[prost(string, optional, tag = "5")]
    pub subagent_run_id: Option<String>,
}

#[derive(Clone, PartialEq, Message)]
pub struct ToolCallArgsEvent {
    #[prost(message, optional, tag = "1")]
    pub base_event: Option<BaseEvent>,
    #[prost(string, tag = "2")]
    pub tool_call_id: String,
    #[prost(string, tag = "3")]
    pub delta: String,
    #[prost(string, optional, tag = "4")]
    pub subagent_run_id: Option<String>,
}

#[derive(Clone, PartialEq, Message)]
pub struct ToolCallEndEvent {
    #[prost(message, optional, tag = "1")]
    pub base_event: Option<BaseEvent>,
    #[prost(string, tag = "2")]
    pub tool_call_id: String,
    #[prost(string, optional, tag = "3")]
    pub subagent_run_id: Option<String>,
}

#[derive(Clone, PartialEq, Message)]
pub struct ToolCallChunkEvent {
    #[prost(message, optional, tag = "1")]
    pub base_event: Option<BaseEvent>,
    #[prost(string, optional, tag = "2")]
    pub tool_call_id: Option<String>,
    #[prost(string, optional, tag = "3")]
    pub tool_call_name: Option<String>,
    #[prost(string, optional, tag = "4")]
    pub parent_message_id: Option<String>,
    #[prost(string, optional, tag = "5")]
    pub delta: Option<String>,
    #[prost(string, optional, tag = "6")]
    pub subagent_run_id: Option<String>,
}

#[derive(Clone, PartialEq, Message)]
pub struct TextMessageChunkEvent {
    #[prost(message, optional, tag = "1")]
    pub base_event: Option<BaseEvent>,
    #[prost(string, optional, tag = "2")]
    pub message_id: Option<String>,
    #[prost(string, optional, tag = "3")]
    pub role: Option<String>,
    #[prost(string, optional, tag = "4")]
    pub delta: Option<String>,
    #[prost(string, optional, tag = "5")]
    pub name: Option<String>,
    #[prost(string, optional, tag = "6")]
    pub subagent_run_id: Option<String>,
}

#[derive(Clone, PartialEq, Message)]
pub struct StateSnapshotEvent {
    #[prost(message, optional, tag = "1")]
    pub base_event: Option<BaseEvent>,
    #[prost(message, optional, tag = "2")]
    pub snapshot: Option<ProtoValue>,
    #[prost(string, optional, tag = "3")]
    pub subagent_run_id: Option<String>,
}

#[derive(Clone, PartialEq, Message)]
pub struct StateDeltaEvent {
    #[prost(message, optional, tag = "1")]
    pub base_event: Option<BaseEvent>,
    #[prost(message, repeated, tag = "2")]
    pub delta: Vec<JsonPatchOperation>,
    #[prost(string, optional, tag = "3")]
    pub subagent_run_id: Option<String>,
}

#[derive(Clone, PartialEq, Message)]
pub struct MessagesSnapshotEvent {
    #[prost(message, optional, tag = "1")]
    pub base_event: Option<BaseEvent>,
    #[prost(message, repeated, tag = "2")]
    pub messages: Vec<ProtoMessage>,
}

#[derive(Clone, PartialEq, Message)]
pub struct RawEvent {
    #[prost(message, optional, tag = "1")]
    pub base_event: Option<BaseEvent>,
    #[prost(message, optional, tag = "2")]
    pub event: Option<ProtoValue>,
    #[prost(string, optional, tag = "3")]
    pub source: Option<String>,
    #[prost(string, optional, tag = "4")]
    pub subagent_run_id: Option<String>,
}

#[derive(Clone, PartialEq, Message)]
pub struct CustomEvent {
    #[prost(message, optional, tag = "1")]
    pub base_event: Option<BaseEvent>,
    #[prost(string, tag = "2")]
    pub name: String,
    #[prost(message, optional, tag = "3")]
    pub value: Option<ProtoValue>,
    #[prost(string, optional, tag = "4")]
    pub subagent_run_id: Option<String>,
}

#[derive(Clone, PartialEq, Message)]
pub struct StepStartedEvent {
    #[prost(message, optional, tag = "1")]
    pub base_event: Option<BaseEvent>,
    #[prost(string, tag = "2")]
    pub step_name: String,
    #[prost(string, optional, tag = "3")]
    pub subagent_run_id: Option<String>,
}

#[derive(Clone, PartialEq, Message)]
pub struct StepFinishedEvent {
    #[prost(message, optional, tag = "1")]
    pub base_event: Option<BaseEvent>,
    #[prost(string, tag = "2")]
    pub step_name: String,
    #[prost(string, optional, tag = "3")]
    pub subagent_run_id: Option<String>,
}

#[derive(Clone, PartialEq, Message)]
pub struct RunStartedEvent {
    #[prost(message, optional, tag = "1")]
    pub base_event: Option<BaseEvent>,
    #[prost(string, tag = "2")]
    pub thread_id: String,
    #[prost(string, tag = "3")]
    pub run_id: String,
    #[prost(string, optional, tag = "4")]
    pub parent_run_id: Option<String>,
    #[prost(message, optional, tag = "5")]
    pub input: Option<RunAgentInput>,
    #[prost(string, optional, tag = "6")]
    pub protocol_version: Option<String>,
}

#[derive(Clone, PartialEq, Message)]
pub struct RunFinishedEvent {
    #[prost(message, optional, tag = "1")]
    pub base_event: Option<BaseEvent>,
    #[prost(string, tag = "2")]
    pub thread_id: String,
    #[prost(string, tag = "3")]
    pub run_id: String,
    #[prost(message, optional, tag = "4")]
    pub result: Option<ProtoValue>,
    #[prost(string, tag = "5")]
    pub outcome: String,
    #[prost(message, repeated, tag = "6")]
    pub interrupts: Vec<Interrupt>,
    #[prost(message, repeated, tag = "7")]
    pub usage: Vec<Usage>,
    #[prost(string, repeated, tag = "8")]
    pub pending_tool_call_ids: Vec<String>,
}

#[derive(Clone, PartialEq, Message)]
pub struct RunErrorEvent {
    #[prost(message, optional, tag = "1")]
    pub base_event: Option<BaseEvent>,
    #[prost(string, optional, tag = "2")]
    pub code: Option<String>,
    #[prost(string, tag = "3")]
    pub message: String,
    #[prost(message, repeated, tag = "4")]
    pub usage: Vec<Usage>,
}

#[derive(Clone, PartialEq, Message)]
pub struct SubagentStartedEvent {
    #[prost(message, optional, tag = "1")]
    pub base_event: Option<BaseEvent>,
    #[prost(string, tag = "2")]
    pub subagent_run_id: String,
    #[prost(string, tag = "3")]
    pub name: String,
    #[prost(string, optional, tag = "4")]
    pub description: Option<String>,
    #[prost(string, optional, tag = "5")]
    pub parent_subagent_run_id: Option<String>,
    #[prost(string, optional, tag = "6")]
    pub parent_tool_call_id: Option<String>,
    #[prost(string, optional, tag = "7")]
    pub parent_message_id: Option<String>,
}

#[derive(Clone, PartialEq, Message)]
pub struct SubagentFinishedEvent {
    #[prost(message, optional, tag = "1")]
    pub base_event: Option<BaseEvent>,
    #[prost(string, tag = "2")]
    pub subagent_run_id: String,
    #[prost(message, optional, tag = "3")]
    pub result: Option<ProtoValue>,
    #[prost(string, tag = "4")]
    pub outcome: String,
    #[prost(string, repeated, tag = "5")]
    pub interrupt_ids: Vec<String>,
}

#[derive(Clone, PartialEq, Message)]
pub struct SubagentErrorEvent {
    #[prost(message, optional, tag = "1")]
    pub base_event: Option<BaseEvent>,
    #[prost(string, tag = "2")]
    pub subagent_run_id: String,
    #[prost(string, tag = "3")]
    pub message: String,
    #[prost(string, optional, tag = "4")]
    pub code: Option<String>,
}

#[derive(Clone, PartialEq, Message)]
pub struct ToolCallResultEvent {
    #[prost(message, optional, tag = "1")]
    pub base_event: Option<BaseEvent>,
    #[prost(string, optional, tag = "2")]
    pub subagent_run_id: Option<String>,
    #[prost(string, tag = "3")]
    pub message_id: String,
    #[prost(string, tag = "4")]
    pub tool_call_id: String,
    #[prost(string, optional, tag = "5")]
    pub content: Option<String>,
    #[prost(string, optional, tag = "6")]
    pub role: Option<String>,
    #[prost(message, repeated, tag = "7")]
    pub content_parts: Vec<InputContent>,
}

#[derive(Clone, PartialEq, Message)]
pub struct ActivitySnapshotEvent {
    #[prost(message, optional, tag = "1")]
    pub base_event: Option<BaseEvent>,
    #[prost(string, optional, tag = "2")]
    pub subagent_run_id: Option<String>,
    #[prost(string, tag = "3")]
    pub message_id: String,
    #[prost(string, tag = "4")]
    pub activity_type: String,
    #[prost(message, optional, tag = "5")]
    pub content: Option<ProtoStruct>,
    #[prost(bool, optional, tag = "6")]
    pub replace: Option<bool>,
}

#[derive(Clone, PartialEq, Message)]
pub struct ActivityDeltaEvent {
    #[prost(message, optional, tag = "1")]
    pub base_event: Option<BaseEvent>,
    #[prost(string, optional, tag = "2")]
    pub subagent_run_id: Option<String>,
    #[prost(string, tag = "3")]
    pub message_id: String,
    #[prost(string, tag = "4")]
    pub activity_type: String,
    #[prost(message, repeated, tag = "5")]
    pub patch: Vec<JsonPatchOperation>,
}

#[derive(Clone, PartialEq, Message)]
pub struct ReasoningStartEvent {
    #[prost(message, optional, tag = "1")]
    pub base_event: Option<BaseEvent>,
    #[prost(string, optional, tag = "2")]
    pub subagent_run_id: Option<String>,
    #[prost(string, tag = "3")]
    pub message_id: String,
}

#[derive(Clone, PartialEq, Message)]
pub struct ReasoningMessageStartEvent {
    #[prost(message, optional, tag = "1")]
    pub base_event: Option<BaseEvent>,
    #[prost(string, optional, tag = "2")]
    pub subagent_run_id: Option<String>,
    #[prost(string, tag = "3")]
    pub message_id: String,
    #[prost(string, tag = "4")]
    pub role: String,
}

#[derive(Clone, PartialEq, Message)]
pub struct ReasoningMessageContentEvent {
    #[prost(message, optional, tag = "1")]
    pub base_event: Option<BaseEvent>,
    #[prost(string, optional, tag = "2")]
    pub subagent_run_id: Option<String>,
    #[prost(string, tag = "3")]
    pub message_id: String,
    #[prost(string, tag = "4")]
    pub delta: String,
}

#[derive(Clone, PartialEq, Message)]
pub struct ReasoningMessageEndEvent {
    #[prost(message, optional, tag = "1")]
    pub base_event: Option<BaseEvent>,
    #[prost(string, optional, tag = "2")]
    pub subagent_run_id: Option<String>,
    #[prost(string, tag = "3")]
    pub message_id: String,
}

#[derive(Clone, PartialEq, Message)]
pub struct ReasoningMessageChunkEvent {
    #[prost(message, optional, tag = "1")]
    pub base_event: Option<BaseEvent>,
    #[prost(string, optional, tag = "2")]
    pub subagent_run_id: Option<String>,
    #[prost(string, optional, tag = "3")]
    pub message_id: Option<String>,
    #[prost(string, optional, tag = "4")]
    pub delta: Option<String>,
}

#[derive(Clone, PartialEq, Message)]
pub struct ReasoningEndEvent {
    #[prost(message, optional, tag = "1")]
    pub base_event: Option<BaseEvent>,
    #[prost(string, optional, tag = "2")]
    pub subagent_run_id: Option<String>,
    #[prost(string, tag = "3")]
    pub message_id: String,
}

#[derive(Clone, PartialEq, Message)]
pub struct ReasoningEncryptedValueEvent {
    #[prost(message, optional, tag = "1")]
    pub base_event: Option<BaseEvent>,
    #[prost(string, optional, tag = "2")]
    pub subagent_run_id: Option<String>,
    #[prost(string, tag = "3")]
    pub subtype: String,
    #[prost(string, tag = "4")]
    pub entity_id: String,
    #[prost(string, tag = "5")]
    pub encrypted_value: String,
}

#[derive(Clone, PartialEq, Message)]
pub struct Usage {
    #[prost(string, optional, tag = "1")]
    pub provider: Option<String>,
    #[prost(string, optional, tag = "2")]
    pub model: Option<String>,
    #[prost(int64, optional, tag = "3")]
    pub input_tokens: Option<i64>,
    #[prost(int64, optional, tag = "4")]
    pub output_tokens: Option<i64>,
    #[prost(int64, optional, tag = "5")]
    pub total_tokens: Option<i64>,
    #[prost(int64, optional, tag = "6")]
    pub reasoning_tokens: Option<i64>,
    #[prost(int64, optional, tag = "7")]
    pub cached_input_tokens: Option<i64>,
    #[prost(int64, optional, tag = "8")]
    pub cache_write_input_tokens: Option<i64>,
}

#[derive(Clone, PartialEq, Message)]
pub struct Event {
    #[prost(
        oneof = "event::Event",
        tags = "1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31"
    )]
    pub event: Option<event::Event>,
}

pub mod event {
    use super::*;
    #[derive(Clone, PartialEq, prost::Oneof)]
    pub enum Event {
        #[prost(message, tag = "1")]
        TextMessageStart(TextMessageStartEvent),
        #[prost(message, tag = "2")]
        TextMessageContent(TextMessageContentEvent),
        #[prost(message, tag = "3")]
        TextMessageEnd(TextMessageEndEvent),
        #[prost(message, tag = "4")]
        ToolCallStart(ToolCallStartEvent),
        #[prost(message, tag = "5")]
        ToolCallArgs(ToolCallArgsEvent),
        #[prost(message, tag = "6")]
        ToolCallEnd(ToolCallEndEvent),
        #[prost(message, tag = "7")]
        StateSnapshot(StateSnapshotEvent),
        #[prost(message, tag = "8")]
        StateDelta(StateDeltaEvent),
        #[prost(message, tag = "9")]
        MessagesSnapshot(MessagesSnapshotEvent),
        #[prost(message, tag = "10")]
        Raw(RawEvent),
        #[prost(message, tag = "11")]
        Custom(CustomEvent),
        #[prost(message, tag = "12")]
        RunStarted(RunStartedEvent),
        #[prost(message, tag = "13")]
        RunFinished(RunFinishedEvent),
        #[prost(message, tag = "14")]
        RunError(RunErrorEvent),
        #[prost(message, tag = "15")]
        StepStarted(StepStartedEvent),
        #[prost(message, tag = "16")]
        StepFinished(StepFinishedEvent),
        #[prost(message, tag = "17")]
        TextMessageChunk(TextMessageChunkEvent),
        #[prost(message, tag = "18")]
        ToolCallChunk(ToolCallChunkEvent),
        #[prost(message, tag = "19")]
        SubagentStarted(SubagentStartedEvent),
        #[prost(message, tag = "20")]
        SubagentFinished(SubagentFinishedEvent),
        #[prost(message, tag = "21")]
        SubagentError(SubagentErrorEvent),
        #[prost(message, tag = "22")]
        ToolCallResult(ToolCallResultEvent),
        #[prost(message, tag = "23")]
        ActivitySnapshot(ActivitySnapshotEvent),
        #[prost(message, tag = "24")]
        ActivityDelta(ActivityDeltaEvent),
        #[prost(message, tag = "25")]
        ReasoningStart(ReasoningStartEvent),
        #[prost(message, tag = "26")]
        ReasoningMessageStart(ReasoningMessageStartEvent),
        #[prost(message, tag = "27")]
        ReasoningMessageContent(ReasoningMessageContentEvent),
        #[prost(message, tag = "28")]
        ReasoningMessageEnd(ReasoningMessageEndEvent),
        #[prost(message, tag = "29")]
        ReasoningMessageChunk(ReasoningMessageChunkEvent),
        #[prost(message, tag = "30")]
        ReasoningEnd(ReasoningEndEvent),
        #[prost(message, tag = "31")]
        ReasoningEncryptedValue(ReasoningEncryptedValueEvent),
    }
}
