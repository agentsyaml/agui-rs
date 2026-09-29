use crate::AgUiError;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Developer,
    System,
    Assistant,
    User,
    Tool,
    Activity,
    Reasoning,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TextMessageRole {
    Developer,
    System,
    #[default]
    Assistant,
    User,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FunctionCall {
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
// A tool call carries no subagent attribution of its own: several calls can
// share one parent, so it inherits its containing message's.
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: ToolCallKind,
    pub function: FunctionCall,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub encrypted_value: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub metadata: Option<serde_json::Map<String, Value>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ToolCallKind {
    Function,
}

/// `PartSource`: where a media part's bytes come from — carried inline,
/// referenced by URL, or already at the provider under a handle it issued.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum PartSource {
    Data {
        value: String,
        #[serde(rename = "mimeType")]
        mime_type: String,
    },
    Url {
        value: String,
        #[serde(rename = "mimeType", skip_serializing_if = "Option::is_none", default)]
        mime_type: Option<String>,
    },
    /// The handle, exactly as the provider issued it. Opaque: a consumer must
    /// not fetch, parse or read a scheme out of it.
    File {
        value: String,
        #[serde(skip_serializing_if = "Option::is_none", default)]
        provider: Option<String>,
        #[serde(rename = "mimeType", skip_serializing_if = "Option::is_none", default)]
        mime_type: Option<String>,
    },
}

/// The retired 0.x `{ type: "binary" }` attachment. No 1.0 message shape
/// carries it and `RunAgentInput` validation rejects it; the always-on
/// compatibility boundary converts what arrives into the media parts above,
/// and upgrades outgoing legacy attachments before the transport sends them.
/// Kept so an adapter written against 0.x still has the type to name.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BinaryInputContent {
    #[serde(rename = "mimeType")]
    pub mime_type: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub data: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub filename: Option<String>,
}

impl BinaryInputContent {
    /// Validates binary input content payload requirements.
    pub fn validate(&self) -> Result<(), AgUiError> {
        let has_payload = self
            .id
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty())
            || self
                .url
                .as_deref()
                .is_some_and(|value| !value.trim().is_empty())
            || self
                .data
                .as_deref()
                .is_some_and(|value| !value.trim().is_empty());

        if has_payload {
            Ok(())
        } else {
            Err(AgUiError::validation(
                "BinaryInputContent requires at least one of id, url, or data.",
            ))
        }
    }
}

/// `ContentPart`: one part of a message body — what a person sends in a user
/// message, or what a tool returns in a tool message. Five variants, no more.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ContentPart {
    Text {
        #[serde(skip_serializing_if = "Option::is_none", default)]
        id: Option<String>,
        text: String,
        #[serde(skip_serializing_if = "Option::is_none", default)]
        metadata: Option<Value>,
    },
    Image {
        #[serde(skip_serializing_if = "Option::is_none", default)]
        id: Option<String>,
        source: PartSource,
        #[serde(skip_serializing_if = "Option::is_none", default)]
        metadata: Option<Value>,
    },
    Audio {
        #[serde(skip_serializing_if = "Option::is_none", default)]
        id: Option<String>,
        source: PartSource,
        #[serde(skip_serializing_if = "Option::is_none", default)]
        metadata: Option<Value>,
    },
    Video {
        #[serde(skip_serializing_if = "Option::is_none", default)]
        id: Option<String>,
        source: PartSource,
        #[serde(skip_serializing_if = "Option::is_none", default)]
        metadata: Option<Value>,
    },
    Document {
        #[serde(skip_serializing_if = "Option::is_none", default)]
        id: Option<String>,
        source: PartSource,
        #[serde(skip_serializing_if = "Option::is_none", default)]
        metadata: Option<Value>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum UserMessageContent {
    Text(String),
    Parts(Vec<ContentPart>),
}

/// What a tool returned, on `TOOL_CALL_RESULT.content`: either plain text, or
/// an ordered list of parts. Mirrors [`UserMessageContent`], which carries the
/// same union on a `User_MESSAGE`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ToolResultContent {
    Text(String),
    Parts(Vec<ContentPart>),
}

impl From<String> for ToolResultContent {
    fn from(text: String) -> Self {
        Self::Text(text)
    }
}

impl From<&str> for ToolResultContent {
    fn from(text: &str) -> Self {
        Self::Text(text.to_string())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeveloperMessage {
    pub id: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub encrypted_value: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub subagent_run_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub metadata: Option<serde_json::Map<String, Value>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemMessage {
    pub id: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub encrypted_value: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub subagent_run_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub metadata: Option<serde_json::Map<String, Value>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssistantMessage {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub encrypted_value: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub subagent_run_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub metadata: Option<serde_json::Map<String, Value>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserMessage {
    pub id: String,
    pub content: UserMessageContent,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub encrypted_value: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub subagent_run_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub metadata: Option<serde_json::Map<String, Value>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolMessage {
    pub id: String,
    pub content: ToolResultContent,
    pub tool_call_id: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub encrypted_value: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub subagent_run_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub metadata: Option<serde_json::Map<String, Value>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityMessage {
    pub id: String,
    pub activity_type: String,
    pub content: serde_json::Map<String, Value>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub subagent_run_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub metadata: Option<serde_json::Map<String, Value>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReasoningMessage {
    pub id: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub encrypted_value: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub subagent_run_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub metadata: Option<serde_json::Map<String, Value>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "role", rename_all = "lowercase")]
pub enum Message {
    Developer(DeveloperMessage),
    System(SystemMessage),
    Assistant(AssistantMessage),
    User(UserMessage),
    Tool(ToolMessage),
    Activity(ActivityMessage),
    Reasoning(ReasoningMessage),
}

impl Message {
    pub fn id(&self) -> &str {
        match self {
            Self::Developer(m) => &m.id,
            Self::System(m) => &m.id,
            Self::Assistant(m) => &m.id,
            Self::User(m) => &m.id,
            Self::Tool(m) => &m.id,
            Self::Activity(m) => &m.id,
            Self::Reasoning(m) => &m.id,
        }
    }

    pub fn role(&self) -> Role {
        match self {
            Self::Developer(_) => Role::Developer,
            Self::System(_) => Role::System,
            Self::Assistant(_) => Role::Assistant,
            Self::User(_) => Role::User,
            Self::Tool(_) => Role::Tool,
            Self::Activity(_) => Role::Activity,
            Self::Reasoning(_) => Role::Reasoning,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Context {
    pub description: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tool {
    pub name: String,
    pub description: String,
    pub parameters: Value,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub metadata: Option<serde_json::Map<String, Value>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Interrupt {
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub subagent_run_id: Option<String>,
    pub id: String,
    pub reason: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub response_schema: Option<serde_json::Map<String, Value>>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub expires_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub metadata: Option<serde_json::Map<String, Value>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ResumeStatus {
    Resolved,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResumeEntry {
    pub interrupt_id: String,
    pub status: ResumeStatus,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub payload: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub metadata: Option<serde_json::Map<String, Value>>,
}

pub type State = Value;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunAgentInput {
    pub thread_id: String,
    pub run_id: String,
    /// The protocol version this consumer speaks, such as `"1.0"`. Sent
    /// in-band rather than by the transport, so a recorded exchange stays
    /// self-describing.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub protocol_version: Option<String>,
    #[serde(
        rename = "parentRunId",
        skip_serializing_if = "Option::is_none",
        default
    )]
    pub parent_run_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub state: Option<Value>,
    pub messages: Vec<Message>,
    pub tools: Vec<Tool>,
    pub context: Vec<Context>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub forwarded_props: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub resume: Option<Vec<ResumeEntry>>,
}

impl RunAgentInput {
    /// Creates a new run agent input.
    pub fn new(thread_id: impl Into<String>, run_id: impl Into<String>) -> Self {
        Self {
            thread_id: thread_id.into(),
            run_id: run_id.into(),
            protocol_version: None,
            parent_run_id: None,
            state: None,
            messages: Vec::new(),
            tools: Vec::new(),
            context: Vec::new(),
            forwarded_props: None,
            resume: None,
        }
    }

    /// Validates run input identifiers and message references.
    pub fn validate(&self) -> Result<(), AgUiError> {
        if self.thread_id.trim().is_empty() {
            return Err(AgUiError::validation(
                "RunAgentInput requires a non-empty thread_id.",
            ));
        }

        if self.run_id.trim().is_empty() {
            return Err(AgUiError::validation(
                "RunAgentInput requires a non-empty run_id.",
            ));
        }

        let mut message_ids = HashSet::new();
        for message in &self.messages {
            let message_id = message.id();
            if !message_ids.insert(message_id) {
                return Err(AgUiError::validation(format!(
                    "RunAgentInput contains duplicate message id '{message_id}'."
                )));
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod validate_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn binary_input_validate_accepts_id() {
        let binary = BinaryInputContent {
            mime_type: "application/octet-stream".into(),
            id: Some("blob-1".into()),
            url: None,
            data: None,
            filename: None,
        };

        assert!(binary.validate().is_ok());
    }

    #[test]
    fn binary_input_validate_accepts_url() {
        let binary = BinaryInputContent {
            mime_type: "application/octet-stream".into(),
            id: None,
            url: Some("https://example.com/blob".into()),
            data: None,
            filename: None,
        };

        assert!(binary.validate().is_ok());
    }

    #[test]
    fn binary_input_validate_accepts_data() {
        let binary = BinaryInputContent {
            mime_type: "application/octet-stream".into(),
            id: None,
            url: None,
            data: Some("Zm9v".into()),
            filename: None,
        };

        assert!(binary.validate().is_ok());
    }

    #[test]
    fn binary_input_validate_rejects_missing_payload() {
        let binary = BinaryInputContent {
            mime_type: "application/octet-stream".into(),
            id: None,
            url: None,
            data: None,
            filename: None,
        };

        let error = binary.validate().expect_err("missing payload should fail");
        assert!(matches!(error, AgUiError::Validation(_)));
        assert_eq!(
            error.to_string(),
            "event validation failed: BinaryInputContent requires at least one of id, url, or data."
        );
    }

    #[test]
    fn binary_input_validate_rejects_blank_payload_fields() {
        let binary = BinaryInputContent {
            mime_type: "application/octet-stream".into(),
            id: Some("   ".into()),
            url: Some("".into()),
            data: None,
            filename: None,
        };

        assert!(binary.validate().is_err());
    }

    #[test]
    fn run_agent_input_validate_accepts_valid_input() {
        let input = RunAgentInput {
            thread_id: "thread-1".into(),
            run_id: "run-1".into(),
            protocol_version: Some("1.0".into()),
            parent_run_id: Some("parent-1".into()),
            state: Some(json!({"count": 1})),
            messages: vec![Message::User(UserMessage {
                id: "user-1".into(),
                content: UserMessageContent::Text("hello".into()),
                name: None,
                encrypted_value: None,
                subagent_run_id: None,
                metadata: None,
            })],
            tools: Vec::new(),
            context: Vec::new(),
            forwarded_props: None,
            resume: None,
        };

        assert!(input.validate().is_ok());
    }

    #[test]
    fn run_agent_input_validate_rejects_blank_thread_id() {
        let input = RunAgentInput {
            thread_id: "  ".into(),
            ..RunAgentInput::new("thread-1", "run-1")
        };

        let error = input.validate().expect_err("blank thread id should fail");
        assert_eq!(
            error.to_string(),
            "event validation failed: RunAgentInput requires a non-empty thread_id."
        );
    }

    #[test]
    fn run_agent_input_validate_rejects_blank_run_id() {
        let input = RunAgentInput {
            run_id: "".into(),
            ..RunAgentInput::new("thread-1", "run-1")
        };

        let error = input.validate().expect_err("blank run id should fail");
        assert_eq!(
            error.to_string(),
            "event validation failed: RunAgentInput requires a non-empty run_id."
        );
    }

    #[test]
    fn run_agent_input_validate_rejects_duplicate_message_ids() {
        let input = RunAgentInput {
            thread_id: "thread-1".into(),
            run_id: "run-1".into(),
            protocol_version: None,
            parent_run_id: None,
            state: None,
            messages: vec![
                Message::User(UserMessage {
                    id: "dup".into(),
                    content: UserMessageContent::Text("hello".into()),
                    name: None,
                    encrypted_value: None,
                    subagent_run_id: None,
                    metadata: None,
                }),
                Message::Assistant(AssistantMessage {
                    id: "dup".into(),
                    content: Some("hi".into()),
                    name: None,
                    tool_calls: None,
                    encrypted_value: None,
                    subagent_run_id: None,
                    metadata: None,
                }),
            ],
            tools: Vec::new(),
            context: Vec::new(),
            forwarded_props: None,
            resume: None,
        };

        let error = input
            .validate()
            .expect_err("duplicate message ids should fail");
        assert_eq!(
            error.to_string(),
            "event validation failed: RunAgentInput contains duplicate message id 'dup'."
        );
    }

    #[test]
    fn run_agent_input_serializes_parent_run_id_in_camel_case() {
        let input = RunAgentInput {
            parent_run_id: Some("parent-1".into()),
            ..RunAgentInput::new("thread-1", "run-1")
        };

        let value = serde_json::to_value(input).expect("serialize run agent input");
        assert_eq!(value["parentRunId"], "parent-1");
    }

    #[test]
    fn run_agent_input_omits_absent_optionals_rather_than_writing_null() {
        let value =
            serde_json::to_value(RunAgentInput::new("thread-1", "run-1")).expect("serialize");

        // 1.0 makes null illegal: an absent field is spelled as an absence.
        for key in [
            "protocolVersion",
            "parentRunId",
            "state",
            "forwardedProps",
            "resume",
        ] {
            assert!(value.get(key).is_none(), "{key} was written as null");
        }
    }
}

#[cfg(test)]
mod message_fields_tests {
    use super::*;
    use serde_json::json;

    fn metadata() -> serde_json::Map<String, Value> {
        serde_json::Map::from_iter([("source".to_string(), json!("test"))])
    }

    /// Every message type composes `BaseMessage`, so all seven carry
    /// `metadata` and `subagentRunId`.
    fn attributed_messages() -> Vec<(&'static str, Message)> {
        vec![
            (
                "developer",
                Message::Developer(DeveloperMessage {
                    id: "m1".into(),
                    content: "rules".into(),
                    name: None,
                    encrypted_value: None,
                    subagent_run_id: Some("sub-1".into()),
                    metadata: Some(metadata()),
                }),
            ),
            (
                "system",
                Message::System(SystemMessage {
                    id: "m2".into(),
                    content: "rules".into(),
                    name: None,
                    encrypted_value: None,
                    subagent_run_id: Some("sub-1".into()),
                    metadata: Some(metadata()),
                }),
            ),
            (
                "assistant",
                Message::Assistant(AssistantMessage {
                    id: "m3".into(),
                    content: Some("hi".into()),
                    name: None,
                    tool_calls: None,
                    encrypted_value: None,
                    subagent_run_id: Some("sub-1".into()),
                    metadata: Some(metadata()),
                }),
            ),
            (
                "user",
                Message::User(UserMessage {
                    id: "m4".into(),
                    content: UserMessageContent::Text("hi".into()),
                    name: None,
                    encrypted_value: None,
                    subagent_run_id: Some("sub-1".into()),
                    metadata: Some(metadata()),
                }),
            ),
            (
                "tool",
                Message::Tool(ToolMessage {
                    id: "m5".into(),
                    content: ToolResultContent::Text("ok".into()),
                    tool_call_id: "tc-1".into(),
                    error: None,
                    encrypted_value: None,
                    subagent_run_id: Some("sub-1".into()),
                    metadata: Some(metadata()),
                }),
            ),
            (
                "activity",
                Message::Activity(ActivityMessage {
                    id: "m6".into(),
                    activity_type: "PLAN".into(),
                    content: serde_json::Map::new(),
                    subagent_run_id: Some("sub-1".into()),
                    metadata: Some(metadata()),
                }),
            ),
            (
                "reasoning",
                Message::Reasoning(ReasoningMessage {
                    id: "m7".into(),
                    content: "think".into(),
                    encrypted_value: None,
                    subagent_run_id: Some("sub-1".into()),
                    metadata: Some(metadata()),
                }),
            ),
        ]
    }

    #[test]
    fn every_message_type_round_trips_metadata_and_attribution() {
        for (role, message) in attributed_messages() {
            let value = serde_json::to_value(&message).expect("serialize");
            assert_eq!(value["role"], role);
            assert_eq!(value["subagentRunId"], "sub-1", "{role} lost attribution");
            assert_eq!(value["metadata"], json!({ "source": "test" }), "{role}");

            let back: Message = serde_json::from_value(value).expect("deserialize");
            assert_eq!(back, message, "{role} round-trip mismatch");
        }
    }

    #[test]
    fn absent_message_metadata_and_attribution_are_omitted() {
        let message = Message::Reasoning(ReasoningMessage {
            id: "m1".into(),
            content: "think".into(),
            encrypted_value: None,
            subagent_run_id: None,
            metadata: None,
        });

        assert_eq!(
            serde_json::to_value(&message).expect("serialize"),
            json!({ "id": "m1", "role": "reasoning", "content": "think" })
        );
    }

    #[test]
    fn tool_call_round_trips_metadata_and_stays_unattributed() {
        let call = ToolCall {
            id: "tc-1".into(),
            kind: ToolCallKind::Function,
            function: FunctionCall {
                name: "search".into(),
                arguments: "{}".into(),
            },
            encrypted_value: Some("cipher".into()),
            metadata: Some(metadata()),
        };

        let value = serde_json::to_value(&call).expect("serialize");
        assert_eq!(
            value,
            json!({
                "id": "tc-1",
                "type": "function",
                "function": { "name": "search", "arguments": "{}" },
                "encryptedValue": "cipher",
                "metadata": { "source": "test" }
            })
        );
        let back: ToolCall = serde_json::from_value(value).expect("deserialize");
        assert_eq!(back, call);
    }

    #[test]
    fn message_metadata_is_preserved_through_a_message_list() {
        let input = RunAgentInput {
            messages: attributed_messages().into_iter().map(|(_, m)| m).collect(),
            ..RunAgentInput::new("t-1", "r-1")
        };

        let value = serde_json::to_value(&input).expect("serialize");
        let back: RunAgentInput = serde_json::from_value(value).expect("deserialize");
        assert_eq!(back.messages, input.messages);
    }
}
