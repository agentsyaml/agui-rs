//! Typed, categorized agent capability declarations.
//!
//! Mirrors the canonical TypeScript `@ag-ui/core` `capabilities.ts`
//! (`AgentCapabilities` and its category schemas). All fields are optional —
//! an agent only declares what it supports; an omitted field means "not
//! declared" (unknown), not "unsupported".

use crate::Tool;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// Describes a sub-agent that can be invoked by a parent agent.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SubAgentInfo {
    /// Unique name or identifier of the sub-agent.
    pub name: String,
    /// What this sub-agent specializes in.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Basic metadata about the agent (discovery UIs, marketplaces, debugging).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct IdentityCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub r#type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub documentation_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Map<String, Value>>,
}

/// Declares which transport mechanisms the agent supports.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TransportCapabilities {
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
}

/// Tool calling capabilities.
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

/// Output format support.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct OutputCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub structured_output: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supported_mime_types: Option<Vec<String>>,
}

/// State and memory management capabilities.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct StateCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshots: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deltas: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub persistent_state: Option<bool>,
}

/// Multi-agent coordination capabilities.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MultiAgentCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supported: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delegation: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub handoffs: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sub_agents: Option<Vec<SubAgentInfo>>,
}

/// Reasoning and thinking capabilities.
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

/// Modalities the agent can accept as input.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MultimodalInputCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub video: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pdf: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<bool>,
}

/// Modalities the agent can produce as output.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MultimodalOutputCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio: Option<bool>,
}

/// Multimodal input and output support.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MultimodalCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<MultimodalInputCapabilities>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<MultimodalOutputCapabilities>,
}

/// Execution control and limits.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ExecutionCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code_execution: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sandboxed: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_iterations: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_execution_time: Option<f64>,
}

/// Human-in-the-loop interaction support.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct HumanInTheLoopCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supported: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub approvals: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interventions: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub feedback: Option<bool>,
    /// Participates in the AG-UI interrupt protocol.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interrupts: Option<bool>,
    /// Tool-call interrupts accept `editedArgs` in the resume payload.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub approve_with_edits: Option<bool>,
}

/// A typed, categorized snapshot of an agent's current capabilities.
///
/// Returned by an agent's capability declaration; mirrors the canonical
/// `AgentCapabilities` schema. All fields are optional — `custom` is an escape
/// hatch for integration-specific capabilities.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AgentCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identity: Option<IdentityCapabilities>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transport: Option<TransportCapabilities>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<ToolsCapabilities>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<OutputCapabilities>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<StateCapabilities>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub multi_agent: Option<MultiAgentCapabilities>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<ReasoningCapabilities>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub multimodal: Option<MultimodalCapabilities>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub execution: Option<ExecutionCapabilities>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub human_in_the_loop: Option<HumanInTheLoopCapabilities>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub custom: Option<Map<String, Value>>,
}

/// Backwards-compatible alias. The canonical type name is [`AgentCapabilities`].
pub type Capabilities = AgentCapabilities;

#[cfg(test)]
mod capabilities_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn default_is_empty_object() {
        let value = serde_json::to_value(AgentCapabilities::default()).unwrap();
        assert_eq!(value, json!({}));
    }

    #[test]
    fn serializes_canonical_camel_case_fields() {
        let capabilities = AgentCapabilities {
            transport: Some(TransportCapabilities {
                streaming: Some(true),
                http_binary: Some(true),
                ..Default::default()
            }),
            state: Some(StateCapabilities {
                snapshots: Some(true),
                deltas: Some(true),
                persistent_state: Some(true),
                ..Default::default()
            }),
            execution: Some(ExecutionCapabilities {
                code_execution: Some(true),
                max_iterations: Some(10.0),
                ..Default::default()
            }),
            ..Default::default()
        };

        let value = serde_json::to_value(&capabilities).unwrap();
        assert_eq!(value["transport"]["httpBinary"], true);
        assert_eq!(value["state"]["persistentState"], true);
        assert_eq!(value["execution"]["codeExecution"], true);
        assert_eq!(value["execution"]["maxIterations"], 10.0);
    }

    #[test]
    fn human_in_the_loop_accepts_interrupt_flags() {
        let raw = json!({
            "supported": true,
            "approvals": true,
            "interrupts": true,
            "approveWithEdits": true
        });
        let hitl: HumanInTheLoopCapabilities = serde_json::from_value(raw).unwrap();
        assert_eq!(hitl.supported, Some(true));
        assert_eq!(hitl.approvals, Some(true));
        assert_eq!(hitl.interrupts, Some(true));
        assert_eq!(hitl.approve_with_edits, Some(true));
    }

    #[test]
    fn human_in_the_loop_leaves_new_flags_absent_when_omitted() {
        let hitl: HumanInTheLoopCapabilities =
            serde_json::from_value(json!({ "supported": true })).unwrap();
        assert_eq!(hitl.supported, Some(true));
        assert_eq!(hitl.interrupts, None);
        assert_eq!(hitl.approve_with_edits, None);
    }

    #[test]
    fn round_trips_full_structure_with_custom() {
        let mut custom = Map::new();
        custom.insert("myFlag".into(), json!("value"));

        let capabilities = AgentCapabilities {
            identity: Some(IdentityCapabilities {
                name: Some("demo".into()),
                r#type: Some("langgraph".into()),
                ..Default::default()
            }),
            multimodal: Some(MultimodalCapabilities {
                input: Some(MultimodalInputCapabilities {
                    image: Some(true),
                    pdf: Some(true),
                    ..Default::default()
                }),
                output: Some(MultimodalOutputCapabilities {
                    image: Some(true),
                    audio: Some(false),
                }),
            }),
            multi_agent: Some(MultiAgentCapabilities {
                supported: Some(true),
                sub_agents: Some(vec![SubAgentInfo {
                    name: "researcher".into(),
                    description: Some("web research".into()),
                }]),
                ..Default::default()
            }),
            human_in_the_loop: Some(HumanInTheLoopCapabilities {
                supported: Some(true),
                interrupts: Some(true),
                approve_with_edits: Some(true),
                ..Default::default()
            }),
            custom: Some(custom),
            ..Default::default()
        };

        let value = serde_json::to_value(&capabilities).unwrap();
        let round_trip: AgentCapabilities = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(round_trip, capabilities);
        assert_eq!(value["identity"]["type"], "langgraph");
        assert_eq!(value["multimodal"]["input"]["pdf"], true);
        assert_eq!(value["multiAgent"]["subAgents"][0]["name"], "researcher");
        assert_eq!(value["custom"]["myFlag"], "value");
    }
}
