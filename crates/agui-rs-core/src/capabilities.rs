use crate::Tool;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// Describes message handling capabilities.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MessagesCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supported: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub multipart: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshots: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encrypted: Option<bool>,
}

/// Describes context ingestion capabilities.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ContextCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supported: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entries: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub forwarded_props: Option<bool>,
}

/// Describes tool calling capabilities.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ToolsCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supported: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub items: Option<Vec<Tool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parallel_calls: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_provided: Option<bool>,
}

/// Describes state management capabilities.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct StateCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supported: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshots: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deltas: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub persistent_state: Option<bool>,
}

/// Describes event transport capabilities.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct EventsCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub streaming: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub websocket: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http_binary: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub push_notifications: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resumable: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub custom: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamps: Option<bool>,
}

/// Describes activity event capabilities.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ActivityCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supported: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshots: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deltas: Option<bool>,
}

/// Describes reasoning stream capabilities.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ReasoningCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supported: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub streaming: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encrypted: Option<bool>,
}

/// Describes interrupt handling capabilities.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct InterruptsCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supported: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub approvals: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interventions: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub feedback: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resume: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub approve_with_edits: Option<bool>,
}

/// Describes the agent capability surface.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Capabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub messages: Option<MessagesCapabilities>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<ContextCapabilities>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<ToolsCapabilities>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<StateCapabilities>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub events: Option<EventsCapabilities>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub activity: Option<ActivityCapabilities>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<ReasoningCapabilities>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interrupts: Option<InterruptsCapabilities>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[cfg(test)]
mod capabilities_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn capabilities_default_is_empty() {
        let capabilities = Capabilities::default();
        let value = serde_json::to_value(capabilities).expect("serialize capabilities");
        assert_eq!(value, json!({}));
    }

    #[test]
    fn capabilities_serializes_camel_case_fields() {
        let capabilities = Capabilities {
            context: Some(ContextCapabilities {
                supported: Some(true),
                entries: Some(true),
                forwarded_props: Some(true),
            }),
            tools: Some(ToolsCapabilities {
                supported: Some(true),
                items: Some(vec![Tool {
                    name: "search".into(),
                    description: "Searches the web".into(),
                    parameters: json!({"type": "object"}),
                    metadata: None,
                }]),
                parallel_calls: Some(true),
                client_provided: Some(false),
            }),
            state: Some(StateCapabilities {
                supported: Some(true),
                snapshots: Some(true),
                deltas: Some(true),
                memory: Some(false),
                persistent_state: Some(true),
            }),
            events: Some(EventsCapabilities {
                streaming: Some(true),
                websocket: Some(false),
                http_binary: Some(true),
                push_notifications: Some(false),
                resumable: Some(true),
                raw: Some(true),
                custom: Some(true),
                timestamps: Some(true),
            }),
            ..Capabilities::default()
        };

        let value = serde_json::to_value(capabilities).expect("serialize capabilities");
        assert_eq!(value["context"]["forwardedProps"], true);
        assert_eq!(value["tools"]["parallelCalls"], true);
        assert_eq!(value["state"]["persistentState"], true);
        assert_eq!(value["events"]["httpBinary"], true);
    }

    #[test]
    fn capabilities_round_trip_full_structure() {
        let mut extra = Map::new();
        extra.insert("customCapability".into(), json!("value"));

        let capabilities = Capabilities {
            messages: Some(MessagesCapabilities {
                supported: Some(true),
                multipart: Some(true),
                tool_calls: Some(true),
                snapshots: Some(true),
                encrypted: Some(false),
            }),
            context: Some(ContextCapabilities {
                supported: Some(true),
                entries: Some(true),
                forwarded_props: Some(true),
            }),
            tools: Some(ToolsCapabilities {
                supported: Some(true),
                items: None,
                parallel_calls: Some(true),
                client_provided: Some(true),
            }),
            state: Some(StateCapabilities {
                supported: Some(true),
                snapshots: Some(true),
                deltas: Some(true),
                memory: Some(true),
                persistent_state: Some(true),
            }),
            events: Some(EventsCapabilities {
                streaming: Some(true),
                websocket: Some(false),
                http_binary: Some(false),
                push_notifications: Some(false),
                resumable: Some(true),
                raw: Some(true),
                custom: Some(true),
                timestamps: Some(true),
            }),
            activity: Some(ActivityCapabilities {
                supported: Some(true),
                snapshots: Some(true),
                deltas: Some(true),
            }),
            reasoning: Some(ReasoningCapabilities {
                supported: Some(true),
                streaming: Some(true),
                encrypted: Some(true),
            }),
            interrupts: Some(InterruptsCapabilities {
                supported: Some(true),
                approvals: Some(true),
                interventions: Some(true),
                feedback: Some(true),
                resume: Some(true),
                approve_with_edits: Some(true),
            }),
            extra,
        };

        let value = serde_json::to_value(&capabilities).expect("serialize capabilities");
        let round_trip: Capabilities =
            serde_json::from_value(value.clone()).expect("deserialize capabilities");
        assert_eq!(round_trip, capabilities);
        assert_eq!(value["customCapability"], "value");
    }

    #[test]
    fn capabilities_skips_absent_nested_values() {
        let capabilities = Capabilities {
            reasoning: Some(ReasoningCapabilities {
                supported: Some(true),
                streaming: None,
                encrypted: None,
            }),
            ..Capabilities::default()
        };

        let value = serde_json::to_value(capabilities).expect("serialize capabilities");
        assert_eq!(value, json!({"reasoning": {"supported": true}}));
    }

    #[test]
    fn capabilities_deserializes_nested_camel_case_payload() {
        let raw = json!({
            "messages": {"multipart": true, "toolCalls": true},
            "interrupts": {"approveWithEdits": true, "resume": true}
        });

        let capabilities: Capabilities =
            serde_json::from_value(raw).expect("deserialize capabilities");

        assert_eq!(capabilities.messages.unwrap().tool_calls, Some(true));
        assert_eq!(
            capabilities.interrupts.unwrap().approve_with_edits,
            Some(true)
        );
    }
}
