//! Tool-based generative UI example, ported from
//! `ag-ui-protocol/integrations/server-starter-all-features/python/.../tool_based_generative_ui.py`.
//!
//! On every run the agent emits a single assistant message that carries one
//! `generate_haiku` tool call, then snapshots the full message history.
//! When the most recent user message is the literal string `"thanks"`, the
//! agent instead replies with a plain text confirmation.
//!
//! Run with:
//!   cargo run -p agui-rs-server --example tool_based_generative_ui
//!
//! Then POST to `http://127.0.0.1:8000/`.

use agui_rs_core::types::{AssistantMessage, ToolMessage};
use agui_rs_core::{
    factory, AgUiError, Event, FunctionCall, Message, MessagesSnapshotEvent, Result, RunAgentInput,
    ToolCall, ToolCallKind, UserMessageContent,
};
use agui_rs_server::{agui_router, channel, RunHandler};
use futures::stream::BoxStream;
use serde_json::json;

struct ToolBasedGenerativeUiHandler;

#[async_trait::async_trait]
impl RunHandler for ToolBasedGenerativeUiHandler {
    async fn handle(&self, input: RunAgentInput) -> Result<BoxStream<'static, Result<Event>>> {
        let (emitter, stream) = channel(32);

        let last_user_text = input
            .messages
            .iter()
            .rev()
            .find_map(|message| match message {
                Message::User(user) => match &user.content {
                    UserMessageContent::Text(text) => Some(text.clone()),
                    _ => None,
                },
                _ => None,
            });

        let thread_id = input.thread_id.clone();
        let run_id = input.run_id.clone();
        let history = input.messages.clone();

        tokio::spawn(async move {
            let _ = emitter.run_started(&thread_id, &run_id).await;

            let mut all_messages = history;

            if matches!(last_user_text.as_deref(), Some("thanks")) {
                let assistant = Message::Assistant(AssistantMessage {
                    id: format!("msg-{}-assistant", run_id),
                    content: Some("Haiku created".into()),
                    name: None,
                    tool_calls: None,
                    encrypted_value: None,
                });
                all_messages.push(assistant);
            } else {
                let tool_call_id = format!("tc-{}-haiku", run_id);
                let assistant = Message::Assistant(AssistantMessage {
                    id: format!("msg-{}-assistant", run_id),
                    content: None,
                    name: None,
                    tool_calls: Some(vec![ToolCall {
                        id: tool_call_id.clone(),
                        kind: ToolCallKind::Function,
                        function: FunctionCall {
                            name: "generate_haiku".into(),
                            arguments: json!({
                                "japanese": ["エーアイの", "橋つなぐ道", "コパキット"],
                                "english": [
                                    "From AI's realm",
                                    "A bridge-road linking us-",
                                    "CopilotKit."
                                ]
                            })
                            .to_string(),
                        },
                        encrypted_value: None,
                    }]),
                    encrypted_value: None,
                });
                let tool_result = Message::Tool(ToolMessage {
                    id: format!("msg-{}-tool", run_id),
                    content: "Haiku created".into(),
                    tool_call_id,
                    error: None,
                    encrypted_value: None,
                });
                all_messages.push(assistant);
                all_messages.push(tool_result);
            }

            let _ = emitter
                .emit(Event::MessagesSnapshot(MessagesSnapshotEvent {
                    messages: all_messages,
                    base: Default::default(),
                }))
                .await;

            let _ = emitter
                .emit(factory::run_finished(&thread_id, &run_id))
                .await;
        });

        Ok(stream)
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let app = agui_router(ToolBasedGenerativeUiHandler);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:8000")
        .await
        .map_err(|error| AgUiError::other(error.to_string()))?;

    println!("tool_based_generative_ui listening on http://127.0.0.1:8000/");

    ::axum::serve(listener, app)
        .await
        .map_err(|error| AgUiError::other(error.to_string()))?;

    Ok(())
}
