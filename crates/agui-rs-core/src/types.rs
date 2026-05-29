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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TextMessageRole {
    Developer,
    System,
    Assistant,
    User,
}

impl Default for TextMessageRole {
    fn default() -> Self {
        Self::Assistant
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FunctionCall {
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: ToolCallKind,
    pub function: FunctionCall,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub encrypted_value: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ToolCallKind {
    Function,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum InputContentSource {
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
}

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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum InputContent {
    Text {
        text: String,
    },
    Image {
        source: InputContentSource,
        #[serde(skip_serializing_if = "Option::is_none", default)]
        metadata: Option<Value>,
    },
    Audio {
        source: InputContentSource,
        #[serde(skip_serializing_if = "Option::is_none", default)]
        metadata: Option<Value>,
    },
    Video {
        source: InputContentSource,
        #[serde(skip_serializing_if = "Option::is_none", default)]
        metadata: Option<Value>,
    },
    Document {
        source: InputContentSource,
        #[serde(skip_serializing_if = "Option::is_none", default)]
        metadata: Option<Value>,
    },
    Binary {
        #[serde(flatten)]
        content: BinaryInputContent,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum UserMessageContent {
    Text(String),
    Parts(Vec<InputContent>),
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
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolMessage {
    pub id: String,
    pub content: String,
    pub tool_call_id: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub encrypted_value: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityMessage {
    pub id: String,
    pub activity_type: String,
    pub content: serde_json::Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReasoningMessage {
    pub id: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub encrypted_value: Option<String>,
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
}

pub type State = Value;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunAgentInput {
    pub thread_id: String,
    pub run_id: String,
    #[serde(rename = "parentRunId", skip_serializing_if = "Option::is_none")]
    pub parent_run_id: Option<String>,
    #[serde(default)]
    pub state: Value,
    pub messages: Vec<Message>,
    pub tools: Vec<Tool>,
    pub context: Vec<Context>,
    #[serde(default)]
    pub forwarded_props: Value,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub resume: Option<Vec<ResumeEntry>>,
}

impl RunAgentInput {
    /// Creates a new run agent input.
    pub fn new(thread_id: impl Into<String>, run_id: impl Into<String>) -> Self {
        Self {
            thread_id: thread_id.into(),
            run_id: run_id.into(),
            parent_run_id: None,
            state: Value::Null,
            messages: Vec::new(),
            tools: Vec::new(),
            context: Vec::new(),
            forwarded_props: Value::Null,
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

            if let Message::User(UserMessage {
                content: UserMessageContent::Parts(parts),
                ..
            }) = message
            {
                for part in parts {
                    if let InputContent::Binary { content } = part {
                        content.validate()?;
                    }
                }
            }
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AgentCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub messages: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interrupts: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub activity: Option<Value>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, Value>,
}

#[cfg(test)]
mod validate_tests {
    use super::*;
    use serde_json::json;

    fn user_message_with_parts(id: &str, parts: Vec<InputContent>) -> Message {
        Message::User(UserMessage {
            id: id.into(),
            content: UserMessageContent::Parts(parts),
            name: None,
            encrypted_value: None,
        })
    }

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
    fn binary_input_serializes_like_legacy_shape() {
        let part = InputContent::Binary {
            content: BinaryInputContent {
                mime_type: "application/octet-stream".into(),
                id: Some("blob-1".into()),
                url: None,
                data: None,
                filename: Some("blob.bin".into()),
            },
        };

        let value = serde_json::to_value(part).expect("serialize binary input");
        assert_eq!(value["type"], "binary");
        assert_eq!(value["mimeType"], "application/octet-stream");
        assert_eq!(value["id"], "blob-1");
        assert_eq!(value["filename"], "blob.bin");
    }

    #[test]
    fn run_agent_input_validate_accepts_valid_input() {
        let input = RunAgentInput {
            thread_id: "thread-1".into(),
            run_id: "run-1".into(),
            parent_run_id: Some("parent-1".into()),
            state: json!({"count": 1}),
            messages: vec![Message::User(UserMessage {
                id: "user-1".into(),
                content: UserMessageContent::Text("hello".into()),
                name: None,
                encrypted_value: None,
            })],
            tools: Vec::new(),
            context: Vec::new(),
            forwarded_props: Value::Null,
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
            parent_run_id: None,
            state: Value::Null,
            messages: vec![
                Message::User(UserMessage {
                    id: "dup".into(),
                    content: UserMessageContent::Text("hello".into()),
                    name: None,
                    encrypted_value: None,
                }),
                Message::Assistant(AssistantMessage {
                    id: "dup".into(),
                    content: Some("hi".into()),
                    name: None,
                    tool_calls: None,
                    encrypted_value: None,
                }),
            ],
            tools: Vec::new(),
            context: Vec::new(),
            forwarded_props: Value::Null,
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
    fn run_agent_input_validate_rejects_invalid_binary_part() {
        let input = RunAgentInput {
            thread_id: "thread-1".into(),
            run_id: "run-1".into(),
            parent_run_id: None,
            state: Value::Null,
            messages: vec![user_message_with_parts(
                "user-1",
                vec![InputContent::Binary {
                    content: BinaryInputContent {
                        mime_type: "application/octet-stream".into(),
                        id: None,
                        url: None,
                        data: None,
                        filename: None,
                    },
                }],
            )],
            tools: Vec::new(),
            context: Vec::new(),
            forwarded_props: Value::Null,
            resume: None,
        };

        assert!(input.validate().is_err());
    }

    #[test]
    fn run_agent_input_validate_accepts_binary_part_with_payload() {
        let input = RunAgentInput {
            thread_id: "thread-1".into(),
            run_id: "run-1".into(),
            parent_run_id: None,
            state: Value::Null,
            messages: vec![user_message_with_parts(
                "user-1",
                vec![InputContent::Binary {
                    content: BinaryInputContent {
                        mime_type: "application/octet-stream".into(),
                        id: Some("asset-1".into()),
                        url: None,
                        data: None,
                        filename: None,
                    },
                }],
            )],
            tools: Vec::new(),
            context: Vec::new(),
            forwarded_props: Value::Null,
            resume: None,
        };

        assert!(input.validate().is_ok());
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
}
