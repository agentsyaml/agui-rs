//! Hand-written `prost` message definitions mirroring the canonical AG-UI
//! `.proto` schema (`events.proto`, `types.proto`, `patch.proto`).
//!
//! Written by hand rather than generated via `prost-build` so the crate has no
//! `protoc` build-time dependency. Field numbers and wire types match the
//! canonical schema exactly, so the binary encoding is interoperable.

use prost::Message;
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
pub struct InputContentSource {
    #[prost(oneof = "input_content_source::Source", tags = "1, 2")]
    pub source: Option<input_content_source::Source>,
}

pub mod input_content_source {
    use super::{InputContentDataSource, InputContentUrlSource};
    #[derive(Clone, PartialEq, prost::Oneof)]
    pub enum Source {
        #[prost(message, tag = "1")]
        Data(InputContentDataSource),
        #[prost(message, tag = "2")]
        Url(InputContentUrlSource),
    }
}

#[derive(Clone, PartialEq, Message)]
pub struct TextInputPart {
    #[prost(string, tag = "1")]
    pub text: String,
}

#[derive(Clone, PartialEq, Message)]
pub struct MediaInputPart {
    #[prost(message, optional, tag = "1")]
    pub source: Option<InputContentSource>,
    #[prost(message, optional, tag = "2")]
    pub metadata: Option<ProtoValue>,
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
}

#[derive(Clone, PartialEq, Message)]
pub struct BaseEvent {
    #[prost(enumeration = "EventType", tag = "1")]
    pub r#type: i32,
    #[prost(int64, optional, tag = "2")]
    pub timestamp: Option<i64>,
    #[prost(message, optional, tag = "3")]
    pub raw_event: Option<ProtoValue>,
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
}

#[derive(Clone, PartialEq, Message)]
pub struct TextMessageContentEvent {
    #[prost(message, optional, tag = "1")]
    pub base_event: Option<BaseEvent>,
    #[prost(string, tag = "2")]
    pub message_id: String,
    #[prost(string, tag = "3")]
    pub delta: String,
}

#[derive(Clone, PartialEq, Message)]
pub struct TextMessageEndEvent {
    #[prost(message, optional, tag = "1")]
    pub base_event: Option<BaseEvent>,
    #[prost(string, tag = "2")]
    pub message_id: String,
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
}

#[derive(Clone, PartialEq, Message)]
pub struct ToolCallArgsEvent {
    #[prost(message, optional, tag = "1")]
    pub base_event: Option<BaseEvent>,
    #[prost(string, tag = "2")]
    pub tool_call_id: String,
    #[prost(string, tag = "3")]
    pub delta: String,
}

#[derive(Clone, PartialEq, Message)]
pub struct ToolCallEndEvent {
    #[prost(message, optional, tag = "1")]
    pub base_event: Option<BaseEvent>,
    #[prost(string, tag = "2")]
    pub tool_call_id: String,
}

#[derive(Clone, PartialEq, Message)]
pub struct StateSnapshotEvent {
    #[prost(message, optional, tag = "1")]
    pub base_event: Option<BaseEvent>,
    #[prost(message, optional, tag = "2")]
    pub snapshot: Option<ProtoValue>,
}

#[derive(Clone, PartialEq, Message)]
pub struct StateDeltaEvent {
    #[prost(message, optional, tag = "1")]
    pub base_event: Option<BaseEvent>,
    #[prost(message, repeated, tag = "2")]
    pub delta: Vec<JsonPatchOperation>,
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
}

#[derive(Clone, PartialEq, Message)]
pub struct CustomEvent {
    #[prost(message, optional, tag = "1")]
    pub base_event: Option<BaseEvent>,
    #[prost(string, tag = "2")]
    pub name: String,
    #[prost(message, optional, tag = "3")]
    pub value: Option<ProtoValue>,
}

#[derive(Clone, PartialEq, Message)]
pub struct RunStartedEvent {
    #[prost(message, optional, tag = "1")]
    pub base_event: Option<BaseEvent>,
    #[prost(string, tag = "2")]
    pub thread_id: String,
    #[prost(string, tag = "3")]
    pub run_id: String,
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
}

#[derive(Clone, PartialEq, Message)]
pub struct RunErrorEvent {
    #[prost(message, optional, tag = "1")]
    pub base_event: Option<BaseEvent>,
    #[prost(string, optional, tag = "2")]
    pub code: Option<String>,
    #[prost(string, tag = "3")]
    pub message: String,
}

#[derive(Clone, PartialEq, Message)]
pub struct StepStartedEvent {
    #[prost(message, optional, tag = "1")]
    pub base_event: Option<BaseEvent>,
    #[prost(string, tag = "2")]
    pub step_name: String,
}

#[derive(Clone, PartialEq, Message)]
pub struct StepFinishedEvent {
    #[prost(message, optional, tag = "1")]
    pub base_event: Option<BaseEvent>,
    #[prost(string, tag = "2")]
    pub step_name: String,
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
}

#[derive(Clone, PartialEq, Message)]
pub struct Event {
    #[prost(
        oneof = "event::Event",
        tags = "1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18"
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
    }
}
