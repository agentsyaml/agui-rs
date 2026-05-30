use crate::types::{Interrupt, Message, RunAgentInput, State, TextMessageRole};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct BaseEventFields {
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub timestamp: Option<i64>,
    #[serde(rename = "rawEvent", skip_serializing_if = "Option::is_none", default)]
    pub raw_event: Option<Value>,
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

    ThinkingStart,
    ThinkingEnd,
    ThinkingTextMessageStart,
    ThinkingTextMessageContent,
    ThinkingTextMessageEnd,
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
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextMessageContentEvent {
    pub message_id: String,
    pub delta: String,
    #[serde(flatten)]
    pub base: BaseEventFields,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextMessageEndEvent {
    pub message_id: String,
    #[serde(flatten)]
    pub base: BaseEventFields,
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
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCallArgsEvent {
    pub tool_call_id: String,
    pub delta: String,
    #[serde(flatten)]
    pub base: BaseEventFields,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCallEndEvent {
    pub tool_call_id: String,
    #[serde(flatten)]
    pub base: BaseEventFields,
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
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub role: Option<ToolResultRole>,
    #[serde(flatten)]
    pub base: BaseEventFields,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StateSnapshotEvent {
    pub snapshot: State,
    #[serde(flatten)]
    pub base: BaseEventFields,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StateDeltaEvent {
    pub delta: Vec<Value>,
    #[serde(flatten)]
    pub base: BaseEventFields,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessagesSnapshotEvent {
    pub messages: Vec<Message>,
    #[serde(flatten)]
    pub base: BaseEventFields,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivitySnapshotEvent {
    pub message_id: String,
    pub activity_type: String,
    pub content: serde_json::Map<String, Value>,
    #[serde(default = "default_true")]
    pub replace: bool,
    #[serde(flatten)]
    pub base: BaseEventFields,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityDeltaEvent {
    pub message_id: String,
    pub activity_type: String,
    pub patch: Vec<Value>,
    #[serde(flatten)]
    pub base: BaseEventFields,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawEvent {
    pub event: Value,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub source: Option<String>,
    #[serde(flatten)]
    pub base: BaseEventFields,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomEvent {
    pub name: String,
    pub value: Value,
    #[serde(flatten)]
    pub base: BaseEventFields,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunStartedEvent {
    pub thread_id: String,
    pub run_id: String,
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
    Success,
    Interrupt { interrupts: Vec<Interrupt> },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunFinishedEvent {
    pub thread_id: String,
    pub run_id: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub outcome: Option<RunFinishedOutcome>,
    #[serde(flatten)]
    pub base: BaseEventFields,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunErrorEvent {
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub code: Option<String>,
    #[serde(flatten)]
    pub base: BaseEventFields,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StepStartedEvent {
    pub step_name: String,
    #[serde(flatten)]
    pub base: BaseEventFields,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StepFinishedEvent {
    pub step_name: String,
    #[serde(flatten)]
    pub base: BaseEventFields,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReasoningStartEvent {
    pub message_id: String,
    #[serde(flatten)]
    pub base: BaseEventFields,
}

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
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReasoningMessageContentEvent {
    pub message_id: String,
    pub delta: String,
    #[serde(flatten)]
    pub base: BaseEventFields,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReasoningMessageEndEvent {
    pub message_id: String,
    #[serde(flatten)]
    pub base: BaseEventFields,
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
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReasoningEndEvent {
    pub message_id: String,
    #[serde(flatten)]
    pub base: BaseEventFields,
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
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThinkingStartEvent {
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub title: Option<String>,
    #[serde(flatten)]
    pub base: BaseEventFields,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThinkingEndEvent {
    #[serde(flatten)]
    pub base: BaseEventFields,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThinkingTextMessageStartEvent {
    #[serde(flatten)]
    pub base: BaseEventFields,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThinkingTextMessageContentEvent {
    pub delta: String,
    #[serde(flatten)]
    pub base: BaseEventFields,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThinkingTextMessageEndEvent {
    #[serde(flatten)]
    pub base: BaseEventFields,
}

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

    ThinkingStart(ThinkingStartEvent),
    ThinkingEnd(ThinkingEndEvent),
    ThinkingTextMessageStart(ThinkingTextMessageStartEvent),
    ThinkingTextMessageContent(ThinkingTextMessageContentEvent),
    ThinkingTextMessageEnd(ThinkingTextMessageEndEvent),
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
    ThinkingStart,
    ThinkingEnd,
    ThinkingTextMessageStart,
    ThinkingTextMessageContent,
    ThinkingTextMessageEnd,
);

pub mod factory {
    use super::*;

    pub fn run_started(thread_id: impl Into<String>, run_id: impl Into<String>) -> Event {
        Event::RunStarted(RunStartedEvent {
            thread_id: thread_id.into(),
            run_id: run_id.into(),
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
            outcome: Some(RunFinishedOutcome::Success),
            base: BaseEventFields::default(),
        })
    }

    pub fn run_error(message: impl Into<String>) -> Event {
        Event::RunError(RunErrorEvent {
            message: message.into(),
            code: None,
            base: BaseEventFields::default(),
        })
    }

    pub fn text_message_start(message_id: impl Into<String>) -> Event {
        Event::TextMessageStart(TextMessageStartEvent {
            message_id: message_id.into(),
            role: TextMessageRole::Assistant,
            name: None,
            base: BaseEventFields::default(),
        })
    }

    pub fn text_message_content(message_id: impl Into<String>, delta: impl Into<String>) -> Event {
        Event::TextMessageContent(TextMessageContentEvent {
            message_id: message_id.into(),
            delta: delta.into(),
            base: BaseEventFields::default(),
        })
    }

    pub fn text_message_end(message_id: impl Into<String>) -> Event {
        Event::TextMessageEnd(TextMessageEndEvent {
            message_id: message_id.into(),
            base: BaseEventFields::default(),
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
        })
    }

    pub fn tool_call_args(tool_call_id: impl Into<String>, delta: impl Into<String>) -> Event {
        Event::ToolCallArgs(ToolCallArgsEvent {
            tool_call_id: tool_call_id.into(),
            delta: delta.into(),
            base: BaseEventFields::default(),
        })
    }

    pub fn tool_call_end(tool_call_id: impl Into<String>) -> Event {
        Event::ToolCallEnd(ToolCallEndEvent {
            tool_call_id: tool_call_id.into(),
            base: BaseEventFields::default(),
        })
    }

    pub fn state_snapshot(snapshot: State) -> Event {
        Event::StateSnapshot(StateSnapshotEvent {
            snapshot,
            base: BaseEventFields::default(),
        })
    }

    pub fn state_delta(delta: Vec<Value>) -> Event {
        Event::StateDelta(StateDeltaEvent {
            delta,
            base: BaseEventFields::default(),
        })
    }

    pub fn step_started(step_name: impl Into<String>) -> Event {
        Event::StepStarted(StepStartedEvent {
            step_name: step_name.into(),
            base: BaseEventFields::default(),
        })
    }

    pub fn step_finished(step_name: impl Into<String>) -> Event {
        Event::StepFinished(StepFinishedEvent {
            step_name: step_name.into(),
            base: BaseEventFields::default(),
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
    fn tool_call_result_round_trip() {
        let event = Event::ToolCallResult(ToolCallResultEvent {
            message_id: "m1".into(),
            tool_call_id: "tc1".into(),
            content: "ok".into(),
            role: Some(ToolResultRole::Tool),
            base: BaseEventFields::default(),
        });
        round_trip(&event);
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["role"], "tool");
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
    fn run_finished_interrupt_outcome() {
        let event = Event::RunFinished(RunFinishedEvent {
            thread_id: "t1".into(),
            run_id: "r1".into(),
            result: None,
            outcome: Some(RunFinishedOutcome::Interrupt {
                interrupts: vec![Interrupt {
                    id: "i1".into(),
                    reason: "needs_human".into(),
                    message: None,
                    tool_call_id: None,
                    response_schema: None,
                    expires_at: None,
                    metadata: None,
                }],
            }),
            base: BaseEventFields::default(),
        });
        round_trip(&event);
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["outcome"]["type"], "interrupt");
        assert_eq!(json["outcome"]["interrupts"][0]["id"], "i1");
    }

    #[test]
    fn reasoning_chain_round_trip() {
        round_trip(&Event::ReasoningStart(ReasoningStartEvent {
            message_id: "r1".into(),
            base: BaseEventFields::default(),
        }));
        round_trip(&Event::ReasoningMessageStart(ReasoningMessageStartEvent {
            message_id: "r1".into(),
            role: ReasoningMessageRole::Reasoning,
            base: BaseEventFields::default(),
        }));
        round_trip(&Event::ReasoningMessageContent(
            ReasoningMessageContentEvent {
                message_id: "r1".into(),
                delta: "thinking...".into(),
                base: BaseEventFields::default(),
            },
        ));
        round_trip(&Event::ReasoningEncryptedValue(
            ReasoningEncryptedValueEvent {
                subtype: ReasoningEncryptedValueSubtype::ToolCall,
                entity_id: "tc1".into(),
                encrypted_value: "abc".into(),
                base: BaseEventFields::default(),
            },
        ));
    }

    #[test]
    fn reasoning_encrypted_value_subtype_serializes_kebab_case() {
        let event = Event::ReasoningEncryptedValue(ReasoningEncryptedValueEvent {
            subtype: ReasoningEncryptedValueSubtype::ToolCall,
            entity_id: "x".into(),
            encrypted_value: "y".into(),
            base: BaseEventFields::default(),
        });
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["subtype"], "tool-call");
    }

    #[test]
    fn deprecated_thinking_events_round_trip() {
        round_trip(&Event::ThinkingStart(ThinkingStartEvent {
            title: Some("planning".into()),
            base: BaseEventFields::default(),
        }));
        round_trip(&Event::ThinkingEnd(ThinkingEndEvent::default()));
        round_trip(&Event::ThinkingTextMessageStart(
            ThinkingTextMessageStartEvent::default(),
        ));
        round_trip(&Event::ThinkingTextMessageContent(
            ThinkingTextMessageContentEvent {
                delta: "x".into(),
                base: BaseEventFields::default(),
            },
        ));
        round_trip(&Event::ThinkingTextMessageEnd(
            ThinkingTextMessageEndEvent::default(),
        ));
    }

    #[test]
    fn activity_events_round_trip() {
        let mut content = serde_json::Map::new();
        content.insert("step".into(), json!("search"));
        round_trip(&Event::ActivitySnapshot(ActivitySnapshotEvent {
            message_id: "a1".into(),
            activity_type: "plan".into(),
            content,
            replace: true,
            base: BaseEventFields::default(),
        }));
        round_trip(&Event::ActivityDelta(ActivityDeltaEvent {
            message_id: "a1".into(),
            activity_type: "plan".into(),
            patch: vec![json!({"op": "add", "path": "/steps/0", "value": "x"})],
            base: BaseEventFields::default(),
        }));
    }

    #[test]
    fn raw_and_custom_round_trip() {
        round_trip(&Event::Raw(RawEvent {
            event: json!({"any": "thing"}),
            source: Some("openai".into()),
            base: BaseEventFields::default(),
        }));
        round_trip(&Event::Custom(CustomEvent {
            name: "my-event".into(),
            value: json!({"x": 1}),
            base: BaseEventFields::default(),
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
            },
        });
        round_trip(&event);
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["timestamp"], 123);
        assert_eq!(json["rawEvent"], json!({"orig": true}));
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
}
