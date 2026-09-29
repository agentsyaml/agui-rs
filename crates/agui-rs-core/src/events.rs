use crate::types::{Interrupt, Message, RunAgentInput, State, TextMessageRole, ToolResultContent};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

// ponytail: null -> default so old streams with explicit null don't fail.
fn null_as_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
}

/// `BaseEvent`: the fields every event carries, whatever its type.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct BaseEventFields {
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub timestamp: Option<i64>,
    #[serde(rename = "rawEvent", skip_serializing_if = "Option::is_none", default)]
    pub raw_event: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub metadata: Option<Value>,
}

/// `Attributable`: composed into everything that can belong to a subagent's
/// work. Run-scoped and conversation-wide events omit it, as do the
/// SUBAGENT_* events, which name the subagent in a required field of their own
/// instead of attributing themselves to one.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttributableFields {
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub subagent_run_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EventType {
    TextMessageStart,
    TextMessageContent,
    TextMessageEnd,
    TextMessageChunk,

    ToolCallStart,
    ToolCallArgs,
    ToolCallEnd,
    ToolCallChunk,
    ToolCallResult,

    StateSnapshot,
    StateDelta,
    MessagesSnapshot,

    ActivitySnapshot,
    ActivityDelta,

    Raw,
    Custom,

    RunStarted,
    RunFinished,
    RunError,
    StepStarted,
    StepFinished,

    ReasoningStart,
    ReasoningMessageStart,
    ReasoningMessageContent,
    ReasoningMessageEnd,
    ReasoningMessageChunk,
    ReasoningEnd,
    ReasoningEncryptedValue,

    SubagentStarted,
    SubagentFinished,
    SubagentError,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextMessageStartEvent {
    pub message_id: String,
    #[serde(default)]
    pub role: TextMessageRole,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub name: Option<String>,
    #[serde(flatten)]
    pub base: BaseEventFields,
    #[serde(flatten)]
    pub attributable: AttributableFields,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextMessageContentEvent {
    pub message_id: String,
    pub delta: String,
    #[serde(flatten)]
    pub base: BaseEventFields,
    #[serde(flatten)]
    pub attributable: AttributableFields,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextMessageEndEvent {
    pub message_id: String,
    #[serde(flatten)]
    pub base: BaseEventFields,
    #[serde(flatten)]
    pub attributable: AttributableFields,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextMessageChunkEvent {
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub message_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub role: Option<TextMessageRole>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub delta: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub name: Option<String>,
    #[serde(flatten)]
    pub base: BaseEventFields,
    #[serde(flatten)]
    pub attributable: AttributableFields,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCallStartEvent {
    pub tool_call_id: String,
    pub tool_call_name: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub parent_message_id: Option<String>,
    #[serde(flatten)]
    pub base: BaseEventFields,
    #[serde(flatten)]
    pub attributable: AttributableFields,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCallArgsEvent {
    pub tool_call_id: String,
    pub delta: String,
    #[serde(flatten)]
    pub base: BaseEventFields,
    #[serde(flatten)]
    pub attributable: AttributableFields,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCallEndEvent {
    pub tool_call_id: String,
    #[serde(flatten)]
    pub base: BaseEventFields,
    #[serde(flatten)]
    pub attributable: AttributableFields,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCallChunkEvent {
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub tool_call_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub parent_message_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub delta: Option<String>,
    #[serde(flatten)]
    pub base: BaseEventFields,
    #[serde(flatten)]
    pub attributable: AttributableFields,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ToolResultRole {
    Tool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCallResultEvent {
    pub message_id: String,
    pub tool_call_id: String,
    pub content: ToolResultContent,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub role: Option<ToolResultRole>,
    #[serde(flatten)]
    pub base: BaseEventFields,
    #[serde(flatten)]
    pub attributable: AttributableFields,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StateSnapshotEvent {
    pub snapshot: State,
    #[serde(flatten)]
    pub base: BaseEventFields,
    #[serde(flatten)]
    pub attributable: AttributableFields,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StateDeltaEvent {
    pub delta: Vec<Value>,
    #[serde(flatten)]
    pub base: BaseEventFields,
    #[serde(flatten)]
    pub attributable: AttributableFields,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessagesSnapshotEvent {
    pub messages: Vec<Message>,
    #[serde(flatten)]
    pub base: BaseEventFields,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivitySnapshotEvent {
    pub message_id: String,
    pub activity_type: String,
    pub content: serde_json::Map<String, Value>,
    /// Absent means the snapshot overwrites the activity's existing content;
    /// only an explicit `false` asks a consumer to leave what is there.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub replace: Option<bool>,
    #[serde(flatten)]
    pub base: BaseEventFields,
    #[serde(flatten)]
    pub attributable: AttributableFields,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityDeltaEvent {
    pub message_id: String,
    pub activity_type: String,
    pub patch: Vec<Value>,
    #[serde(flatten)]
    pub base: BaseEventFields,
    #[serde(flatten)]
    pub attributable: AttributableFields,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawEvent {
    pub event: Value,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub source: Option<String>,
    #[serde(flatten)]
    pub base: BaseEventFields,
    #[serde(flatten)]
    pub attributable: AttributableFields,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomEvent {
    pub name: String,
    pub value: Value,
    #[serde(flatten)]
    pub base: BaseEventFields,
    #[serde(flatten)]
    pub attributable: AttributableFields,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunStartedEvent {
    pub thread_id: String,
    pub run_id: String,
    /// The protocol version this producer speaks, such as `"1.0"`. Not an echo
    /// of the input's: each side declares itself, so a consumer sees a
    /// downgrade the moment it happens.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub protocol_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub parent_run_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub input: Option<RunAgentInput>,
    #[serde(flatten)]
    pub base: BaseEventFields,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum RunFinishedOutcome {
    Success {
        /// Tool calls this run started and left unanswered, for the application
        /// to answer in the next input's messages. Absent or empty means the
        /// producer named none and a consumer derives the list from the stream.
        #[serde(
            rename = "pendingToolCallIds",
            skip_serializing_if = "Option::is_none",
            default
        )]
        pending_tool_call_ids: Option<Vec<String>>,
    },
    Interrupt {
        interrupts: Vec<Interrupt>,
    },
    /// Stopped before completing, by whoever was running it, without failing.
    /// Named here because an outcome a consumer does not recognise is stripped
    /// and read as success.
    Cancelled,
}

/// Token counts for one provider and model, in the protocol's own accounting:
/// `input_tokens`/`output_tokens` are the totals and `total_tokens` is their
/// sum, while `reasoning_tokens`, `cached_input_tokens` and
/// `cache_write_input_tokens` are parts of those totals — never additions.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenUsage {
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub input_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub output_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub total_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub reasoning_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub cached_input_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub cache_write_input_tokens: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunFinishedEvent {
    pub thread_id: String,
    pub run_id: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub result: Option<Value>,
    /// Absent means success.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub outcome: Option<RunFinishedOutcome>,
    #[serde(
        default,
        deserialize_with = "null_as_default",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub usage: Vec<TokenUsage>,
    #[serde(flatten)]
    pub base: BaseEventFields,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunErrorEvent {
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub code: Option<String>,
    #[serde(
        default,
        deserialize_with = "null_as_default",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub usage: Vec<TokenUsage>,
    #[serde(flatten)]
    pub base: BaseEventFields,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StepStartedEvent {
    pub step_name: String,
    #[serde(flatten)]
    pub base: BaseEventFields,
    #[serde(flatten)]
    pub attributable: AttributableFields,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StepFinishedEvent {
    pub step_name: String,
    #[serde(flatten)]
    pub base: BaseEventFields,
    #[serde(flatten)]
    pub attributable: AttributableFields,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReasoningStartEvent {
    pub message_id: String,
    #[serde(flatten)]
    pub base: BaseEventFields,
    #[serde(flatten)]
    pub attributable: AttributableFields,
}

/// `role` on `ReasoningMessageStartEvent` is fixed at `"reasoning"`, so the
/// wire value is not a choice a producer gets to make.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReasoningMessageRole {
    Reasoning,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReasoningMessageStartEvent {
    pub message_id: String,
    pub role: ReasoningMessageRole,
    #[serde(flatten)]
    pub base: BaseEventFields,
    #[serde(flatten)]
    pub attributable: AttributableFields,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReasoningMessageContentEvent {
    pub message_id: String,
    pub delta: String,
    #[serde(flatten)]
    pub base: BaseEventFields,
    #[serde(flatten)]
    pub attributable: AttributableFields,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReasoningMessageEndEvent {
    pub message_id: String,
    #[serde(flatten)]
    pub base: BaseEventFields,
    #[serde(flatten)]
    pub attributable: AttributableFields,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReasoningMessageChunkEvent {
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub message_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub delta: Option<String>,
    #[serde(flatten)]
    pub base: BaseEventFields,
    #[serde(flatten)]
    pub attributable: AttributableFields,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReasoningEndEvent {
    pub message_id: String,
    #[serde(flatten)]
    pub base: BaseEventFields,
    #[serde(flatten)]
    pub attributable: AttributableFields,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReasoningEncryptedValueSubtype {
    ToolCall,
    Message,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReasoningEncryptedValueEvent {
    pub subtype: ReasoningEncryptedValueSubtype,
    pub entity_id: String,
    pub encrypted_value: String,
    #[serde(flatten)]
    pub base: BaseEventFields,
    #[serde(flatten)]
    pub attributable: AttributableFields,
}

// ponytail: large_enum_variant is allowed on purpose — RUN_STARTED embeds
// RunAgentInput by value, so boxing it to satisfy the size ratio would cost an
// allocation per run and an indirection in every match for no real saving.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Event {
    TextMessageStart(TextMessageStartEvent),
    TextMessageContent(TextMessageContentEvent),
    TextMessageEnd(TextMessageEndEvent),
    TextMessageChunk(TextMessageChunkEvent),

    ToolCallStart(ToolCallStartEvent),
    ToolCallArgs(ToolCallArgsEvent),
    ToolCallEnd(ToolCallEndEvent),
    ToolCallChunk(ToolCallChunkEvent),
    ToolCallResult(ToolCallResultEvent),

    StateSnapshot(StateSnapshotEvent),
    StateDelta(StateDeltaEvent),
    MessagesSnapshot(MessagesSnapshotEvent),

    ActivitySnapshot(ActivitySnapshotEvent),
    ActivityDelta(ActivityDeltaEvent),

    Raw(RawEvent),
    Custom(CustomEvent),

    RunStarted(RunStartedEvent),
    RunFinished(RunFinishedEvent),
    RunError(RunErrorEvent),
    StepStarted(StepStartedEvent),
    StepFinished(StepFinishedEvent),

    ReasoningStart(ReasoningStartEvent),
    ReasoningMessageStart(ReasoningMessageStartEvent),
    ReasoningMessageContent(ReasoningMessageContentEvent),
    ReasoningMessageEnd(ReasoningMessageEndEvent),
    ReasoningMessageChunk(ReasoningMessageChunkEvent),
    ReasoningEnd(ReasoningEndEvent),
    ReasoningEncryptedValue(ReasoningEncryptedValueEvent),

    SubagentStarted(SubagentStartedEvent),
    SubagentFinished(SubagentFinishedEvent),
    SubagentError(SubagentErrorEvent),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum SubagentFinishedOutcome {
    Success,
    Suspended {
        #[serde(
            rename = "interruptIds",
            default,
            deserialize_with = "null_as_default",
            skip_serializing_if = "Vec::is_empty"
        )]
        interrupt_ids: Vec<String>,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubagentStartedEvent {
    pub subagent_run_id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub parent_subagent_run_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub parent_tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub parent_message_id: Option<String>,
    #[serde(flatten)]
    pub base: BaseEventFields,
}

/// `interrupt_ids` live only nested inside `outcome.suspended`; there is no
/// top-level ids field.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubagentFinishedEvent {
    pub subagent_run_id: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub outcome: Option<SubagentFinishedOutcome>,
    #[serde(flatten)]
    pub base: BaseEventFields,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubagentErrorEvent {
    pub subagent_run_id: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub code: Option<String>,
    #[serde(flatten)]
    pub base: BaseEventFields,
}

/// Generates [`Event::event_type`] and [`Event::base`] from a single variant
/// list, keeping the two dispatch tables in lockstep as variants are added.
macro_rules! impl_event_dispatch {
    ($($variant:ident),+ $(,)?) => {
        impl Event {
            /// Returns the [`EventType`] discriminant for this event.
            pub fn event_type(&self) -> EventType {
                match self {
                    $(Self::$variant(_) => EventType::$variant,)+
                }
            }

            /// Returns the shared [`BaseEventFields`] carried by every event.
            pub fn base(&self) -> &BaseEventFields {
                match self {
                    $(Self::$variant(e) => &e.base,)+
                }
            }
        }
    };
}

impl_event_dispatch!(
    TextMessageStart,
    TextMessageContent,
    TextMessageEnd,
    TextMessageChunk,
    ToolCallStart,
    ToolCallArgs,
    ToolCallEnd,
    ToolCallChunk,
    ToolCallResult,
    StateSnapshot,
    StateDelta,
    MessagesSnapshot,
    ActivitySnapshot,
    ActivityDelta,
    Raw,
    Custom,
    RunStarted,
    RunFinished,
    RunError,
    StepStarted,
    StepFinished,
    ReasoningStart,
    ReasoningMessageStart,
    ReasoningMessageContent,
    ReasoningMessageEnd,
    ReasoningMessageChunk,
    ReasoningEnd,
    ReasoningEncryptedValue,
    SubagentStarted,
    SubagentFinished,
    SubagentError,
);

pub mod factory {
    use super::*;

    pub fn run_started(thread_id: impl Into<String>, run_id: impl Into<String>) -> Event {
        Event::RunStarted(RunStartedEvent {
            thread_id: thread_id.into(),
            run_id: run_id.into(),
            protocol_version: None,
            parent_run_id: None,
            input: None,
            base: BaseEventFields::default(),
        })
    }

    pub fn run_finished(thread_id: impl Into<String>, run_id: impl Into<String>) -> Event {
        Event::RunFinished(RunFinishedEvent {
            thread_id: thread_id.into(),
            run_id: run_id.into(),
            result: None,
            outcome: Some(RunFinishedOutcome::Success {
                pending_tool_call_ids: None,
            }),
            usage: Vec::new(),
            base: BaseEventFields::default(),
        })
    }

    pub fn run_error(message: impl Into<String>) -> Event {
        Event::RunError(RunErrorEvent {
            message: message.into(),
            code: None,
            usage: Vec::new(),
            base: BaseEventFields::default(),
        })
    }

    pub fn text_message_start(message_id: impl Into<String>) -> Event {
        Event::TextMessageStart(TextMessageStartEvent {
            message_id: message_id.into(),
            role: TextMessageRole::Assistant,
            name: None,
            base: BaseEventFields::default(),
            attributable: AttributableFields::default(),
        })
    }

    pub fn text_message_content(message_id: impl Into<String>, delta: impl Into<String>) -> Event {
        Event::TextMessageContent(TextMessageContentEvent {
            message_id: message_id.into(),
            delta: delta.into(),
            base: BaseEventFields::default(),
            attributable: AttributableFields::default(),
        })
    }

    pub fn text_message_end(message_id: impl Into<String>) -> Event {
        Event::TextMessageEnd(TextMessageEndEvent {
            message_id: message_id.into(),
            base: BaseEventFields::default(),
            attributable: AttributableFields::default(),
        })
    }

    pub fn tool_call_start(
        tool_call_id: impl Into<String>,
        tool_call_name: impl Into<String>,
    ) -> Event {
        Event::ToolCallStart(ToolCallStartEvent {
            tool_call_id: tool_call_id.into(),
            tool_call_name: tool_call_name.into(),
            parent_message_id: None,
            base: BaseEventFields::default(),
            attributable: AttributableFields::default(),
        })
    }

    pub fn tool_call_args(tool_call_id: impl Into<String>, delta: impl Into<String>) -> Event {
        Event::ToolCallArgs(ToolCallArgsEvent {
            tool_call_id: tool_call_id.into(),
            delta: delta.into(),
            base: BaseEventFields::default(),
            attributable: AttributableFields::default(),
        })
    }

    pub fn tool_call_end(tool_call_id: impl Into<String>) -> Event {
        Event::ToolCallEnd(ToolCallEndEvent {
            tool_call_id: tool_call_id.into(),
            base: BaseEventFields::default(),
            attributable: AttributableFields::default(),
        })
    }

    pub fn state_snapshot(snapshot: State) -> Event {
        Event::StateSnapshot(StateSnapshotEvent {
            snapshot,
            base: BaseEventFields::default(),
            attributable: AttributableFields::default(),
        })
    }

    pub fn state_delta(delta: Vec<Value>) -> Event {
        Event::StateDelta(StateDeltaEvent {
            delta,
            base: BaseEventFields::default(),
            attributable: AttributableFields::default(),
        })
    }

    pub fn step_started(step_name: impl Into<String>) -> Event {
        Event::StepStarted(StepStartedEvent {
            step_name: step_name.into(),
            base: BaseEventFields::default(),
            attributable: AttributableFields::default(),
        })
    }

    pub fn step_finished(step_name: impl Into<String>) -> Event {
        Event::StepFinished(StepFinishedEvent {
            step_name: step_name.into(),
            base: BaseEventFields::default(),
            attributable: AttributableFields::default(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::UserMessageContent;
    use serde_json::json;

    fn round_trip(event: &Event) {
        let json = serde_json::to_value(event).expect("serialize");
        let back: Event = serde_json::from_value(json.clone()).expect("deserialize");
        assert_eq!(event, &back, "round-trip mismatch for {json}");
    }

    #[test]
    fn run_started_round_trip() {
        let event = factory::run_started("t1", "r1");
        round_trip(&event);
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["type"], "RUN_STARTED");
        assert_eq!(json["threadId"], "t1");
        assert_eq!(json["runId"], "r1");
        assert!(json.get("protocolVersion").is_none());
    }

    #[test]
    fn text_message_chain_round_trip() {
        round_trip(&factory::text_message_start("m1"));
        round_trip(&factory::text_message_content("m1", "hello"));
        round_trip(&factory::text_message_end("m1"));
    }

    #[test]
    fn text_message_start_default_role_is_assistant() {
        let event = factory::text_message_start("m1");
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["role"], "assistant");
    }

    #[test]
    fn tool_call_chain_round_trip() {
        round_trip(&factory::tool_call_start("tc1", "search"));
        round_trip(&factory::tool_call_args("tc1", "{\"q\":"));
        round_trip(&factory::tool_call_end("tc1"));
    }

    #[test]
    fn tool_call_parent_message_id_accepts_null() {
        // Cross-language back-compat (TS 0.0.58+ main, commit 2fa33a0e): the
        // .NET Microsoft Agent Framework adapter serializes the optional
        // `parentMessageId` as JSON `null` rather than omitting it. `Option`
        // fields must treat null as "field omitted", not fail validation.
        for json in [
            json!({"type": "TOOL_CALL_START", "toolCallId": "tc-1", "toolCallName": "get_weather", "parentMessageId": null}),
            json!({"type": "TOOL_CALL_CHUNK", "toolCallId": "tc-1", "toolCallName": "get_weather", "parentMessageId": null, "delta": "x"}),
        ] {
            let event: Event =
                serde_json::from_value(json).expect("null parentMessageId should parse");
            match &event {
                Event::ToolCallStart(e) => assert!(e.parent_message_id.is_none()),
                Event::ToolCallChunk(e) => assert!(e.parent_message_id.is_none()),
                _ => panic!("unexpected event: {event:?}"),
            }
        }
    }

    #[test]
    fn tool_call_parent_message_id_round_trips_string() {
        let json = json!({"type": "TOOL_CALL_START", "toolCallId": "tc-1", "toolCallName": "get_weather", "parentMessageId": "msg-1"});
        let event: Event =
            serde_json::from_value(json).expect("string parentMessageId should parse");
        match &event {
            Event::ToolCallStart(e) => assert_eq!(e.parent_message_id.as_deref(), Some("msg-1")),
            _ => panic!("unexpected event: {event:?}"),
        }
    }

    #[test]
    fn tool_call_result_round_trip() {
        let event = Event::ToolCallResult(ToolCallResultEvent {
            message_id: "m1".into(),
            tool_call_id: "tc1".into(),
            content: "ok".into(),
            role: Some(ToolResultRole::Tool),
            base: BaseEventFields::default(),
            attributable: AttributableFields::default(),
        });
        round_trip(&event);
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["role"], "tool");
        assert_eq!(json["content"], "ok");
    }

    #[test]
    fn tool_call_result_content_carries_parts() {
        let event = Event::ToolCallResult(ToolCallResultEvent {
            message_id: "m1".into(),
            tool_call_id: "tc1".into(),
            content: ToolResultContent::Parts(vec![crate::ContentPart::Text {
                id: None,
                text: "hello".into(),
                metadata: None,
            }]),
            role: None,
            base: BaseEventFields::default(),
            attributable: AttributableFields::default(),
        });
        round_trip(&event);
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(
            json["content"],
            json!([{ "type": "text", "text": "hello" }])
        );
        assert!(json.get("role").is_none());
    }

    #[test]
    fn state_snapshot_and_delta_round_trip() {
        round_trip(&factory::state_snapshot(json!({"counter": 1})));
        round_trip(&factory::state_delta(vec![
            json!({"op": "replace", "path": "/counter", "value": 2}),
        ]));
    }

    #[test]
    fn run_finished_success_outcome() {
        let event = factory::run_finished("t1", "r1");
        round_trip(&event);
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["outcome"]["type"], "success");
    }

    #[test]
    fn run_finished_success_outcome_carries_pending_tool_calls() {
        let event = Event::RunFinished(RunFinishedEvent {
            thread_id: "t1".into(),
            run_id: "r1".into(),
            result: None,
            outcome: Some(RunFinishedOutcome::Success {
                pending_tool_call_ids: Some(vec!["tc1".into(), "tc2".into()]),
            }),
            usage: Vec::new(),
            base: BaseEventFields::default(),
        });
        round_trip(&event);
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["outcome"]["pendingToolCallIds"], json!(["tc1", "tc2"]));
    }

    #[test]
    fn run_finished_cancelled_outcome() {
        let event = Event::RunFinished(RunFinishedEvent {
            thread_id: "t1".into(),
            run_id: "r1".into(),
            result: None,
            outcome: Some(RunFinishedOutcome::Cancelled),
            usage: Vec::new(),
            base: BaseEventFields::default(),
        });
        round_trip(&event);
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["outcome"], json!({ "type": "cancelled" }));
    }

    #[test]
    fn run_finished_interrupt_outcome() {
        let event = Event::RunFinished(RunFinishedEvent {
            thread_id: "t1".into(),
            run_id: "r1".into(),
            result: None,
            outcome: Some(RunFinishedOutcome::Interrupt {
                interrupts: vec![Interrupt {
                    subagent_run_id: Some("sub-1".into()),
                    id: "i1".into(),
                    reason: "needs_human".into(),
                    message: None,
                    tool_call_id: None,
                    response_schema: None,
                    expires_at: None,
                    metadata: None,
                }],
            }),
            usage: Vec::new(),
            base: BaseEventFields::default(),
        });
        round_trip(&event);
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["outcome"]["type"], "interrupt");
        assert_eq!(json["outcome"]["interrupts"][0]["id"], "i1");
        assert_eq!(json["outcome"]["interrupts"][0]["subagentRunId"], "sub-1");
    }

    #[test]
    fn reasoning_chain_round_trip() {
        round_trip(&Event::ReasoningStart(ReasoningStartEvent {
            message_id: "r1".into(),
            base: BaseEventFields::default(),
            attributable: AttributableFields::default(),
        }));
        round_trip(&Event::ReasoningMessageStart(ReasoningMessageStartEvent {
            message_id: "r1".into(),
            role: ReasoningMessageRole::Reasoning,
            base: BaseEventFields::default(),
            attributable: AttributableFields::default(),
        }));
        round_trip(&Event::ReasoningMessageContent(
            ReasoningMessageContentEvent {
                message_id: "r1".into(),
                delta: "thinking...".into(),
                base: BaseEventFields::default(),
                attributable: AttributableFields::default(),
            },
        ));
        round_trip(&Event::ReasoningEncryptedValue(
            ReasoningEncryptedValueEvent {
                subtype: ReasoningEncryptedValueSubtype::ToolCall,
                entity_id: "tc1".into(),
                encrypted_value: "abc".into(),
                base: BaseEventFields::default(),
                attributable: AttributableFields::default(),
            },
        ));
    }

    #[test]
    fn reasoning_message_start_requires_fixed_role() {
        let error = serde_json::from_value::<Event>(json!({
            "type": "REASONING_MESSAGE_START",
            "messageId": "r1"
        }))
        .expect_err("role is required, not defaulted");
        assert!(error.to_string().contains("role"));

        let event: Event = serde_json::from_value(json!({
            "type": "REASONING_MESSAGE_START",
            "messageId": "r1",
            "role": "reasoning"
        }))
        .expect("fixed role should parse");
        assert_eq!(event.event_type(), EventType::ReasoningMessageStart);
    }

    #[test]
    fn reasoning_encrypted_value_subtype_serializes_kebab_case() {
        let event = Event::ReasoningEncryptedValue(ReasoningEncryptedValueEvent {
            subtype: ReasoningEncryptedValueSubtype::ToolCall,
            entity_id: "x".into(),
            encrypted_value: "y".into(),
            base: BaseEventFields::default(),
            attributable: AttributableFields::default(),
        });
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["subtype"], "tool-call");
    }

    #[test]
    fn activity_events_round_trip() {
        let mut content = serde_json::Map::new();
        content.insert("step".into(), json!("search"));
        round_trip(&Event::ActivitySnapshot(ActivitySnapshotEvent {
            message_id: "a1".into(),
            activity_type: "plan".into(),
            content,
            replace: None,
            base: BaseEventFields::default(),
            attributable: AttributableFields::default(),
        }));
        round_trip(&Event::ActivityDelta(ActivityDeltaEvent {
            message_id: "a1".into(),
            activity_type: "plan".into(),
            patch: vec![json!({"op": "add", "path": "/steps/0", "value": "x"})],
            base: BaseEventFields::default(),
            attributable: AttributableFields::default(),
        }));
    }

    #[test]
    fn activity_snapshot_omits_absent_replace() {
        let event = Event::ActivitySnapshot(ActivitySnapshotEvent {
            message_id: "a1".into(),
            activity_type: "plan".into(),
            content: serde_json::Map::new(),
            replace: None,
            base: BaseEventFields::default(),
            attributable: AttributableFields::default(),
        });
        let json = serde_json::to_value(&event).unwrap();
        assert!(json.get("replace").is_none());
    }

    #[test]
    fn raw_and_custom_round_trip() {
        round_trip(&Event::Raw(RawEvent {
            event: json!({"any": "thing"}),
            source: Some("openai".into()),
            base: BaseEventFields::default(),
            attributable: AttributableFields::default(),
        }));
        round_trip(&Event::Custom(CustomEvent {
            name: "my-event".into(),
            value: json!({"x": 1}),
            base: BaseEventFields::default(),
            attributable: AttributableFields::default(),
        }));
    }

    #[test]
    fn step_events_round_trip() {
        round_trip(&factory::step_started("plan"));
        round_trip(&factory::step_finished("plan"));
    }

    #[test]
    fn messages_snapshot_round_trip_user_message() {
        let user = Message::User(crate::types::UserMessage {
            id: "u1".into(),
            content: UserMessageContent::Text("hi".into()),
            name: None,
            encrypted_value: None,
            subagent_run_id: None,
            metadata: None,
        });
        round_trip(&Event::MessagesSnapshot(MessagesSnapshotEvent {
            messages: vec![user],
            base: BaseEventFields::default(),
        }));
    }

    #[test]
    fn run_error_with_code_round_trip() {
        let event = Event::RunError(RunErrorEvent {
            message: "boom".into(),
            code: Some("E_BOOM".into()),
            usage: Vec::new(),
            base: BaseEventFields::default(),
        });
        round_trip(&event);
    }

    #[test]
    fn base_event_fields_round_trip_through_flatten() {
        let event = Event::TextMessageContent(TextMessageContentEvent {
            message_id: "m1".into(),
            delta: "hi".into(),
            base: BaseEventFields {
                timestamp: Some(123),
                raw_event: Some(json!({"orig": true})),
                metadata: None,
            },
            attributable: AttributableFields::default(),
        });
        round_trip(&event);
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["timestamp"], 123);
        assert_eq!(json["rawEvent"], json!({"orig": true}));
    }

    #[test]
    fn attributable_flattens_subagent_run_id() {
        let event = Event::TextMessageContent(TextMessageContentEvent {
            message_id: "m1".into(),
            delta: "hi".into(),
            base: BaseEventFields::default(),
            attributable: AttributableFields {
                subagent_run_id: Some("sub-1".into()),
            },
        });
        round_trip(&event);
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["subagentRunId"], "sub-1");
    }

    #[test]
    fn run_scoped_events_cannot_carry_attribution() {
        for json in [
            json!({"type": "RUN_STARTED", "threadId": "t1", "runId": "r1"}),
            json!({"type": "RUN_FINISHED", "threadId": "t1", "runId": "r1"}),
            json!({"type": "RUN_ERROR", "message": "boom"}),
            json!({"type": "MESSAGES_SNAPSHOT", "messages": []}),
            json!({"type": "SUBAGENT_STARTED", "subagentRunId": "sub-1", "name": "r"}),
        ] {
            let event: Event = serde_json::from_value(json.clone()).expect("deserialize");
            let back = serde_json::to_value(&event).expect("serialize");
            assert!(
                !back.as_object().unwrap().contains_key("subagentRunId")
                    || back["type"] == "SUBAGENT_STARTED",
                "{json} gained attribution"
            );
        }
    }

    #[test]
    fn parses_assistant_message_with_tool_calls() {
        use crate::types::*;
        let raw = json!({
            "role": "assistant",
            "id": "m1",
            "content": "calling",
            "toolCalls": [{
                "id": "tc1",
                "type": "function",
                "function": {"name": "search", "arguments": "{}"}
            }]
        });
        let msg: Message = serde_json::from_value(raw).unwrap();
        match msg {
            Message::Assistant(AssistantMessage { tool_calls, .. }) => {
                let tc = tool_calls.expect("tool_calls present");
                assert_eq!(tc.len(), 1);
                assert_eq!(tc[0].id, "tc1");
                assert_eq!(tc[0].function.name, "search");
            }
            _ => panic!("expected assistant"),
        }
    }

    #[test]
    fn user_message_content_accepts_string_or_parts() {
        use crate::types::*;
        let s: UserMessageContent = serde_json::from_value(json!("plain text")).unwrap();
        assert!(matches!(s, UserMessageContent::Text(_)));
        let p: UserMessageContent = serde_json::from_value(json!([
            {"type": "text", "text": "hi"}
        ]))
        .unwrap();
        assert!(matches!(p, UserMessageContent::Parts(_)));
    }

    #[test]
    fn subagent_finished_suspended_outcome_uses_camel_case_ids() {
        let event = Event::SubagentFinished(SubagentFinishedEvent {
            subagent_run_id: "sub-1".into(),
            result: None,
            outcome: Some(SubagentFinishedOutcome::Suspended {
                interrupt_ids: vec!["a".into(), "b".into()],
            }),
            base: BaseEventFields::default(),
        });
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["subagentRunId"], "sub-1");
        assert_eq!(
            json["outcome"]["interruptIds"],
            serde_json::json!(["a", "b"])
        );
        assert!(json["outcome"].get("interrupt_ids").is_none());
        assert!(json.get("interruptIds").is_none());
        round_trip(&event);
    }

    #[test]
    fn subagent_started_full_fields_round_trip() {
        let event = Event::SubagentStarted(SubagentStartedEvent {
            subagent_run_id: "sub-1".into(),
            name: "researcher".into(),
            description: Some("deep dive".into()),
            parent_subagent_run_id: Some("sub-0".into()),
            parent_tool_call_id: Some("tc-1".into()),
            parent_message_id: Some("m-1".into()),
            base: BaseEventFields::default(),
        });
        round_trip(&event);
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["subagentRunId"], "sub-1");
        assert_eq!(json["name"], "researcher");
        assert_eq!(json["description"], "deep dive");
        assert_eq!(json["parentSubagentRunId"], "sub-0");
        assert_eq!(json["parentToolCallId"], "tc-1");
        assert_eq!(json["parentMessageId"], "m-1");
    }

    #[test]
    fn token_usage_all_fields_round_trip() {
        let event = Event::RunFinished(RunFinishedEvent {
            thread_id: "t1".into(),
            run_id: "r1".into(),
            result: None,
            outcome: Some(RunFinishedOutcome::Success {
                pending_tool_call_ids: None,
            }),
            usage: vec![TokenUsage {
                provider: Some("openai".into()),
                model: Some("gpt-5".into()),
                input_tokens: Some(10),
                output_tokens: Some(20),
                total_tokens: Some(30),
                reasoning_tokens: Some(5),
                cached_input_tokens: Some(2),
                cache_write_input_tokens: Some(3),
            }],
            base: BaseEventFields::default(),
        });
        round_trip(&event);
        let json = serde_json::to_value(&event).unwrap();
        let u = &json["usage"][0];
        assert_eq!(u["provider"], "openai");
        assert_eq!(u["model"], "gpt-5");
        assert_eq!(u["inputTokens"], 10);
        assert_eq!(u["outputTokens"], 20);
        assert_eq!(u["totalTokens"], 30);
        assert_eq!(u["reasoningTokens"], 5);
        assert_eq!(u["cachedInputTokens"], 2);
        assert_eq!(u["cacheWriteInputTokens"], 3);
    }

    #[test]
    fn deserializes_canonical_text_message_content_payload() {
        let raw = json!({
            "type": "TEXT_MESSAGE_CONTENT",
            "messageId": "m1",
            "delta": "hi",
            "timestamp": 42
        });
        let event: Event = serde_json::from_value(raw).unwrap();
        assert_eq!(event.event_type(), EventType::TextMessageContent);
        match event {
            Event::TextMessageContent(e) => {
                assert_eq!(e.message_id, "m1");
                assert_eq!(e.delta, "hi");
                assert_eq!(e.base.timestamp, Some(42));
            }
            _ => panic!("expected TextMessageContent"),
        }
    }

    #[test]
    fn event_type_covers_the_thirty_one_spec_events() {
        let all = [
            EventType::TextMessageStart,
            EventType::TextMessageContent,
            EventType::TextMessageEnd,
            EventType::TextMessageChunk,
            EventType::ToolCallStart,
            EventType::ToolCallArgs,
            EventType::ToolCallEnd,
            EventType::ToolCallChunk,
            EventType::ToolCallResult,
            EventType::StateSnapshot,
            EventType::StateDelta,
            EventType::MessagesSnapshot,
            EventType::ActivitySnapshot,
            EventType::ActivityDelta,
            EventType::Raw,
            EventType::Custom,
            EventType::RunStarted,
            EventType::RunFinished,
            EventType::RunError,
            EventType::StepStarted,
            EventType::StepFinished,
            EventType::ReasoningStart,
            EventType::ReasoningMessageStart,
            EventType::ReasoningMessageContent,
            EventType::ReasoningMessageEnd,
            EventType::ReasoningMessageChunk,
            EventType::ReasoningEnd,
            EventType::ReasoningEncryptedValue,
            EventType::SubagentStarted,
            EventType::SubagentFinished,
            EventType::SubagentError,
        ];
        assert_eq!(all.len(), 31);
        for ty in all {
            let json = serde_json::to_value(ty).unwrap();
            let back: EventType = serde_json::from_value(json).unwrap();
            assert_eq!(back, ty);
        }
    }
}
