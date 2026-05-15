use ag_ui_core::{
    BaseEventFields, CustomEvent, Event, ReasoningEndEvent, ReasoningMessageContentEvent,
    ReasoningMessageEndEvent, ReasoningMessageRole, ReasoningMessageStartEvent,
    ReasoningStartEvent, RunErrorEvent, StateSnapshotEvent, TextMessageEndEvent, TextMessageRole,
    TextMessageStartEvent, ToolCallArgsEvent, ToolCallEndEvent, ToolCallResultEvent,
    ToolCallStartEvent,
};
use async_stream::try_stream;
use futures::{Stream, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::Result;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LegacyMetaEventName {
    LangGraphInterruptEvent,
    PredictState,
    Exit,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyTextMessageStart {
    pub message_id: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub parent_message_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub role: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyTextMessageContent {
    pub message_id: String,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyTextMessageEnd {
    pub message_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyActionExecutionStart {
    pub action_execution_id: String,
    pub action_name: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub parent_message_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyActionExecutionArgs {
    pub action_execution_id: String,
    pub args: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyActionExecutionEnd {
    pub action_execution_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyActionExecutionResult {
    pub action_name: String,
    pub action_execution_id: String,
    pub result: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyAgentStateMessage {
    pub state: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyMetaEvent {
    pub name: LegacyMetaEventName,
    pub value: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyRunError {
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub code: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyThinkingStart {
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub title: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct LegacyThinkingTextMessageStart {}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyThinkingTextMessageContent {
    pub delta: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct LegacyThinkingTextMessageEnd {}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct LegacyThinkingEnd {}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum LegacyEvent {
    #[serde(rename = "TextMessageStart")]
    TextMessageStart(LegacyTextMessageStart),
    #[serde(rename = "TextMessageContent")]
    TextMessageContent(LegacyTextMessageContent),
    #[serde(rename = "TextMessageEnd")]
    TextMessageEnd(LegacyTextMessageEnd),
    #[serde(rename = "ActionExecutionStart")]
    ActionExecutionStart(LegacyActionExecutionStart),
    #[serde(rename = "ActionExecutionArgs")]
    ActionExecutionArgs(LegacyActionExecutionArgs),
    #[serde(rename = "ActionExecutionEnd")]
    ActionExecutionEnd(LegacyActionExecutionEnd),
    #[serde(rename = "ActionExecutionResult")]
    ActionExecutionResult(LegacyActionExecutionResult),
    #[serde(rename = "AgentStateMessage")]
    AgentStateMessage(LegacyAgentStateMessage),
    #[serde(rename = "MetaEvent")]
    MetaEvent(LegacyMetaEvent),
    #[serde(rename = "RunError")]
    RunError(LegacyRunError),
    #[serde(rename = "THINKING_START")]
    ThinkingStart(LegacyThinkingStart),
    #[serde(rename = "THINKING_TEXT_MESSAGE_START")]
    ThinkingTextMessageStart(LegacyThinkingTextMessageStart),
    #[serde(rename = "THINKING_TEXT_MESSAGE_CONTENT")]
    ThinkingTextMessageContent(LegacyThinkingTextMessageContent),
    #[serde(rename = "THINKING_TEXT_MESSAGE_END")]
    ThinkingTextMessageEnd(LegacyThinkingTextMessageEnd),
    #[serde(rename = "THINKING_END")]
    ThinkingEnd(LegacyThinkingEnd),
}

/// Converts legacy protocol events into current AG-UI events.
pub fn convert_legacy_events<S>(s: S) -> impl Stream<Item = Result<Event>>
where
    S: Stream<Item = LegacyEvent> + Send + 'static,
{
    try_stream! {
        let mut stream = s.boxed();
        let mut next_generated_id = 0usize;
        let mut current_reasoning_id: Option<String> = None;
        let mut current_reasoning_message_id: Option<String> = None;

        while let Some(event) = stream.next().await {
            match event {
                LegacyEvent::TextMessageStart(event) => {
                    yield Event::TextMessageStart(TextMessageStartEvent {
                        message_id: event.message_id,
                        role: parse_text_role(event.role.as_deref()),
                        name: None,
                        base: BaseEventFields::default(),
                    });
                }
                LegacyEvent::TextMessageContent(event) => {
                    yield Event::TextMessageContent(ag_ui_core::TextMessageContentEvent {
                        message_id: event.message_id,
                        delta: event.content,
                        base: BaseEventFields::default(),
                    });
                }
                LegacyEvent::TextMessageEnd(event) => {
                    yield Event::TextMessageEnd(TextMessageEndEvent {
                        message_id: event.message_id,
                        base: BaseEventFields::default(),
                    });
                }
                LegacyEvent::ActionExecutionStart(event) => {
                    yield Event::ToolCallStart(ToolCallStartEvent {
                        tool_call_id: event.action_execution_id,
                        tool_call_name: event.action_name,
                        parent_message_id: event.parent_message_id,
                        base: BaseEventFields::default(),
                    });
                }
                LegacyEvent::ActionExecutionArgs(event) => {
                    yield Event::ToolCallArgs(ToolCallArgsEvent {
                        tool_call_id: event.action_execution_id,
                        delta: event.args,
                        base: BaseEventFields::default(),
                    });
                }
                LegacyEvent::ActionExecutionEnd(event) => {
                    yield Event::ToolCallEnd(ToolCallEndEvent {
                        tool_call_id: event.action_execution_id,
                        base: BaseEventFields::default(),
                    });
                }
                LegacyEvent::ActionExecutionResult(event) => {
                    yield Event::ToolCallResult(ToolCallResultEvent {
                        message_id: format!("legacy-tool-result-{}", event.action_execution_id),
                        tool_call_id: event.action_execution_id,
                        content: event.result,
                        role: None,
                        base: BaseEventFields::default(),
                    });
                }
                LegacyEvent::AgentStateMessage(event) => {
                    let snapshot = serde_json::from_str(&event.state)?;
                    yield Event::StateSnapshot(StateSnapshotEvent {
                        snapshot,
                        base: BaseEventFields::default(),
                    });
                }
                LegacyEvent::MetaEvent(event) => {
                    yield Event::Custom(CustomEvent {
                        name: match event.name {
                            LegacyMetaEventName::LangGraphInterruptEvent => "LangGraphInterruptEvent",
                            LegacyMetaEventName::PredictState => "PredictState",
                            LegacyMetaEventName::Exit => "Exit",
                        }
                        .to_string(),
                        value: event.value,
                        base: BaseEventFields::default(),
                    });
                }
                LegacyEvent::RunError(event) => {
                    yield Event::RunError(RunErrorEvent {
                        message: event.message,
                        code: event.code,
                        base: BaseEventFields::default(),
                    });
                }
                LegacyEvent::ThinkingStart(_) => {
                    let reasoning_id = generated_id(&mut next_generated_id, "legacy-reasoning");
                    current_reasoning_id = Some(reasoning_id.clone());
                    yield Event::ReasoningStart(ReasoningStartEvent {
                        message_id: reasoning_id,
                        base: BaseEventFields::default(),
                    });
                }
                LegacyEvent::ThinkingTextMessageStart(_) => {
                    let message_id = generated_id(&mut next_generated_id, "legacy-reasoning-message");
                    current_reasoning_message_id = Some(message_id.clone());
                    yield Event::ReasoningMessageStart(ReasoningMessageStartEvent {
                        message_id,
                        role: ReasoningMessageRole::Reasoning,
                        base: BaseEventFields::default(),
                    });
                }
                LegacyEvent::ThinkingTextMessageContent(event) => {
                    let message_id = current_reasoning_message_id
                        .clone()
                        .unwrap_or_else(|| generated_id(&mut next_generated_id, "legacy-reasoning-message"));
                    current_reasoning_message_id = Some(message_id.clone());
                    yield Event::ReasoningMessageContent(ReasoningMessageContentEvent {
                        message_id,
                        delta: event.delta,
                        base: BaseEventFields::default(),
                    });
                }
                LegacyEvent::ThinkingTextMessageEnd(_) => {
                    let message_id = current_reasoning_message_id
                        .clone()
                        .unwrap_or_else(|| generated_id(&mut next_generated_id, "legacy-reasoning-message"));
                    current_reasoning_message_id = Some(message_id.clone());
                    yield Event::ReasoningMessageEnd(ReasoningMessageEndEvent {
                        message_id,
                        base: BaseEventFields::default(),
                    });
                }
                LegacyEvent::ThinkingEnd(_) => {
                    let message_id = current_reasoning_id
                        .clone()
                        .unwrap_or_else(|| generated_id(&mut next_generated_id, "legacy-reasoning"));
                    current_reasoning_id = None;
                    current_reasoning_message_id = None;
                    yield Event::ReasoningEnd(ReasoningEndEvent {
                        message_id,
                        base: BaseEventFields::default(),
                    });
                }
            }
        }
    }
}

fn generated_id(counter: &mut usize, prefix: &str) -> String {
    *counter += 1;
    format!("{prefix}-{}", *counter)
}

fn parse_text_role(role: Option<&str>) -> TextMessageRole {
    match role.unwrap_or("assistant") {
        "developer" => TextMessageRole::Developer,
        "system" => TextMessageRole::System,
        "user" => TextMessageRole::User,
        _ => TextMessageRole::Assistant,
    }
}

#[cfg(test)]
mod tests {
    use ag_ui_core::BaseEventFields;
    use futures::{stream, StreamExt};
    use serde_json::json;

    use super::{
        convert_legacy_events, LegacyActionExecutionArgs, LegacyActionExecutionEnd,
        LegacyActionExecutionResult, LegacyActionExecutionStart, LegacyAgentStateMessage,
        LegacyEvent, LegacyMetaEvent, LegacyMetaEventName, LegacyRunError,
        LegacyTextMessageContent, LegacyTextMessageEnd, LegacyTextMessageStart, LegacyThinkingEnd,
        LegacyThinkingStart, LegacyThinkingTextMessageContent, LegacyThinkingTextMessageEnd,
        LegacyThinkingTextMessageStart,
    };

    async fn collect(events: Vec<LegacyEvent>) -> Vec<crate::Result<ag_ui_core::Event>> {
        convert_legacy_events(stream::iter(events)).collect().await
    }

    #[tokio::test]
    async fn converts_legacy_text_messages() {
        let events = collect(vec![
            LegacyEvent::TextMessageStart(LegacyTextMessageStart {
                message_id: "m1".into(),
                parent_message_id: None,
                role: Some("assistant".into()),
            }),
            LegacyEvent::TextMessageContent(LegacyTextMessageContent {
                message_id: "m1".into(),
                content: "hello".into(),
            }),
            LegacyEvent::TextMessageEnd(LegacyTextMessageEnd {
                message_id: "m1".into(),
            }),
        ])
        .await;

        assert!(
            matches!(&events[0], Ok(event) if *event == ag_ui_core::factory::text_message_start("m1"))
        );
        assert!(
            matches!(&events[1], Ok(event) if *event == ag_ui_core::factory::text_message_content("m1", "hello"))
        );
        assert!(
            matches!(&events[2], Ok(event) if *event == ag_ui_core::factory::text_message_end("m1"))
        );
    }

    #[tokio::test]
    async fn converts_legacy_tool_call_events() {
        let events = collect(vec![
            LegacyEvent::ActionExecutionStart(LegacyActionExecutionStart {
                action_execution_id: "tc1".into(),
                action_name: "search".into(),
                parent_message_id: Some("m1".into()),
            }),
            LegacyEvent::ActionExecutionArgs(LegacyActionExecutionArgs {
                action_execution_id: "tc1".into(),
                args: "{}".into(),
            }),
            LegacyEvent::ActionExecutionEnd(LegacyActionExecutionEnd {
                action_execution_id: "tc1".into(),
            }),
            LegacyEvent::ActionExecutionResult(LegacyActionExecutionResult {
                action_name: "search".into(),
                action_execution_id: "tc1".into(),
                result: "ok".into(),
            }),
        ])
        .await;

        assert!(
            matches!(&events[0], Ok(event) if *event == ag_ui_core::Event::ToolCallStart(ag_ui_core::ToolCallStartEvent {
                tool_call_id: "tc1".into(),
                tool_call_name: "search".into(),
                parent_message_id: Some("m1".into()),
                base: BaseEventFields::default(),
            }))
        );
        assert!(
            matches!(&events[1], Ok(event) if *event == ag_ui_core::factory::tool_call_args("tc1", "{}"))
        );
        assert!(
            matches!(&events[2], Ok(event) if *event == ag_ui_core::factory::tool_call_end("tc1"))
        );
        assert!(
            matches!(&events[3], Ok(ag_ui_core::Event::ToolCallResult(event)) if event.tool_call_id == "tc1" && event.content == "ok")
        );
    }

    #[tokio::test]
    async fn converts_thinking_events_to_reasoning() {
        let events = collect(vec![
            LegacyEvent::ThinkingStart(LegacyThinkingStart::default()),
            LegacyEvent::ThinkingTextMessageStart(LegacyThinkingTextMessageStart::default()),
            LegacyEvent::ThinkingTextMessageContent(LegacyThinkingTextMessageContent {
                delta: "plan".into(),
            }),
            LegacyEvent::ThinkingTextMessageEnd(LegacyThinkingTextMessageEnd::default()),
            LegacyEvent::ThinkingEnd(LegacyThinkingEnd::default()),
        ])
        .await;

        let reasoning_start_id = match &events[0] {
            Ok(ag_ui_core::Event::ReasoningStart(event)) => event.message_id.clone(),
            other => panic!("unexpected event: {other:?}"),
        };
        let reasoning_message_id = match &events[1] {
            Ok(ag_ui_core::Event::ReasoningMessageStart(event)) => event.message_id.clone(),
            other => panic!("unexpected event: {other:?}"),
        };

        assert_ne!(reasoning_start_id, reasoning_message_id);
        assert!(
            matches!(&events[2], Ok(ag_ui_core::Event::ReasoningMessageContent(event)) if event.message_id == reasoning_message_id && event.delta == "plan")
        );
        assert!(
            matches!(&events[3], Ok(ag_ui_core::Event::ReasoningMessageEnd(event)) if event.message_id == reasoning_message_id)
        );
        assert!(
            matches!(&events[4], Ok(ag_ui_core::Event::ReasoningEnd(event)) if event.message_id == reasoning_start_id)
        );
    }

    #[tokio::test]
    async fn generates_missing_reasoning_ids_on_content_only() {
        let events = collect(vec![LegacyEvent::ThinkingTextMessageContent(
            LegacyThinkingTextMessageContent {
                delta: "solo".into(),
            },
        )])
        .await;

        assert!(
            matches!(&events[0], Ok(ag_ui_core::Event::ReasoningMessageContent(event)) if event.message_id.starts_with("legacy-reasoning-message-"))
        );
    }

    #[tokio::test]
    async fn converts_state_messages_to_snapshots() {
        let events = collect(vec![LegacyEvent::AgentStateMessage(
            LegacyAgentStateMessage {
                state: json!({"count": 1}).to_string(),
            },
        )])
        .await;

        assert!(
            matches!(&events[0], Ok(ag_ui_core::Event::StateSnapshot(event)) if event.snapshot == json!({"count": 1}))
        );
    }

    #[tokio::test]
    async fn invalid_state_message_yields_error() {
        let events = collect(vec![LegacyEvent::AgentStateMessage(
            LegacyAgentStateMessage {
                state: "not-json".into(),
            },
        )])
        .await;

        assert!(events[0].is_err());
    }

    #[tokio::test]
    async fn converts_meta_events_and_run_errors() {
        let events = collect(vec![
            LegacyEvent::MetaEvent(LegacyMetaEvent {
                name: LegacyMetaEventName::PredictState,
                value: json!({"tool": "search"}),
            }),
            LegacyEvent::RunError(LegacyRunError {
                message: "boom".into(),
                code: Some("E_BOOM".into()),
            }),
        ])
        .await;

        assert!(
            matches!(&events[0], Ok(ag_ui_core::Event::Custom(event)) if event.name == "PredictState")
        );
        assert!(
            matches!(&events[1], Ok(ag_ui_core::Event::RunError(event)) if event.message == "boom" && event.code.as_deref() == Some("E_BOOM"))
        );
    }
}
