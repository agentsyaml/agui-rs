use agui_rs_core::{
    AttributableFields, BaseEventFields, CustomEvent, Event, RunErrorEvent, StateSnapshotEvent,
    TextMessageEndEvent, TextMessageRole, TextMessageStartEvent, ToolCallArgsEvent,
    ToolCallEndEvent, ToolCallResultEvent, ToolCallStartEvent,
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
}

/// Converts legacy protocol events into current AG-UI events.
pub fn convert_legacy_events<S>(s: S) -> impl Stream<Item = Result<Event>>
where
    S: Stream<Item = LegacyEvent> + Send + 'static,
{
    try_stream! {
        let mut stream = s.boxed();
        while let Some(event) = stream.next().await {
            match event {
                LegacyEvent::TextMessageStart(event) => {
                    yield Event::TextMessageStart(TextMessageStartEvent {
                        message_id: event.message_id,
                        role: parse_text_role(event.role.as_deref()),
                        name: None,
                        base: BaseEventFields::default(),
    attributable: AttributableFields::default(),
                    });
                }
                LegacyEvent::TextMessageContent(event) => {
                    yield Event::TextMessageContent(agui_rs_core::TextMessageContentEvent {
                        message_id: event.message_id,
                        delta: event.content,
                        base: BaseEventFields::default(),
    attributable: AttributableFields::default(),
                    });
                }
                LegacyEvent::TextMessageEnd(event) => {
                    yield Event::TextMessageEnd(TextMessageEndEvent {
                        message_id: event.message_id,
                        base: BaseEventFields::default(),
    attributable: AttributableFields::default(),
                    });
                }
                LegacyEvent::ActionExecutionStart(event) => {
                    yield Event::ToolCallStart(ToolCallStartEvent {
                        tool_call_id: event.action_execution_id,
                        tool_call_name: event.action_name,
                        parent_message_id: event.parent_message_id,
                        base: BaseEventFields::default(),
    attributable: AttributableFields::default(),
                    });
                }
                LegacyEvent::ActionExecutionArgs(event) => {
                    yield Event::ToolCallArgs(ToolCallArgsEvent {
                        tool_call_id: event.action_execution_id,
                        delta: event.args,
                        base: BaseEventFields::default(),
    attributable: AttributableFields::default(),
                    });
                }
                LegacyEvent::ActionExecutionEnd(event) => {
                    yield Event::ToolCallEnd(ToolCallEndEvent {
                        tool_call_id: event.action_execution_id,
                        base: BaseEventFields::default(),
    attributable: AttributableFields::default(),
                    });
                }
                LegacyEvent::ActionExecutionResult(event) => {
                    yield Event::ToolCallResult(ToolCallResultEvent {
                        message_id: format!("legacy-tool-result-{}", event.action_execution_id),
                        tool_call_id: event.action_execution_id,
                        content: event.result.into(),
                        role: None,
                        base: BaseEventFields::default(),
    attributable: AttributableFields::default(),
                    });
                }
                LegacyEvent::AgentStateMessage(event) => {
                    let snapshot = serde_json::from_str(&event.state)?;
                    yield Event::StateSnapshot(StateSnapshotEvent {
                        snapshot,
                        base: BaseEventFields::default(),
    attributable: AttributableFields::default(),
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
    attributable: AttributableFields::default(),
                    });
                }
                LegacyEvent::RunError(event) => {
                    yield Event::RunError(RunErrorEvent {
                        message: event.message,
                        code: event.code,
                        usage: Vec::new(),
                        base: BaseEventFields::default(),
                    });
                }
            }
        }
    }
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
    use agui_rs_core::AttributableFields;
    use agui_rs_core::BaseEventFields;
    use futures::{stream, StreamExt};
    use serde_json::json;

    use super::{
        convert_legacy_events, LegacyActionExecutionArgs, LegacyActionExecutionEnd,
        LegacyActionExecutionResult, LegacyActionExecutionStart, LegacyAgentStateMessage,
        LegacyEvent, LegacyMetaEvent, LegacyMetaEventName, LegacyRunError,
        LegacyTextMessageContent, LegacyTextMessageEnd, LegacyTextMessageStart,
    };

    async fn collect(events: Vec<LegacyEvent>) -> Vec<crate::Result<agui_rs_core::Event>> {
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
            matches!(&events[0], Ok(event) if *event == agui_rs_core::factory::text_message_start("m1"))
        );
        assert!(
            matches!(&events[1], Ok(event) if *event == agui_rs_core::factory::text_message_content("m1", "hello"))
        );
        assert!(
            matches!(&events[2], Ok(event) if *event == agui_rs_core::factory::text_message_end("m1"))
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
            matches!(&events[0], Ok(event) if *event == agui_rs_core::Event::ToolCallStart(agui_rs_core::ToolCallStartEvent {
                tool_call_id: "tc1".into(),
                tool_call_name: "search".into(),
                parent_message_id: Some("m1".into()),
                base: BaseEventFields::default(),
                attributable: AttributableFields::default(),
            }))
        );
        assert!(
            matches!(&events[1], Ok(event) if *event == agui_rs_core::factory::tool_call_args("tc1", "{}"))
        );
        assert!(
            matches!(&events[2], Ok(event) if *event == agui_rs_core::factory::tool_call_end("tc1"))
        );
        assert!(
            matches!(&events[3], Ok(agui_rs_core::Event::ToolCallResult(event)) if event.tool_call_id == "tc1" && event.content == agui_rs_core::ToolResultContent::Text("ok".into()))
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
            matches!(&events[0], Ok(agui_rs_core::Event::StateSnapshot(event)) if event.snapshot == json!({"count": 1}))
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
            matches!(&events[0], Ok(agui_rs_core::Event::Custom(event)) if event.name == "PredictState")
        );
        assert!(
            matches!(&events[1], Ok(agui_rs_core::Event::RunError(event)) if event.message == "boom" && event.code.as_deref() == Some("E_BOOM"))
        );
    }
}
