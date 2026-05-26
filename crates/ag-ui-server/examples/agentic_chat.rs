//! Agentic chat example, ported from
//! `ag-ui-protocol/integrations/server-starter-all-features/python/.../agentic_chat.py`.
//!
//! Branches on the last user message:
//! - Tool result (last message is `Message::Tool`) → emits a short assistant
//!   confirmation ("background changed ✓").
//! - Text `"tool"` → emits a `change_background` tool call.
//! - Text `"backend_tool"` → emits a `lookup_weather` tool call wrapped in a
//!   `MessagesSnapshot`.
//! - Anything else → streams a 10→1 countdown as `TextMessage*` events with
//!   a 300 ms gap between deltas.
//!
//! Run with:
//!   cargo run -p ag-ui-server --example agentic_chat
//!
//! Then POST to `http://127.0.0.1:8000/`.

use ag_ui_core::types::{AssistantMessage, ToolMessage};
use ag_ui_core::{
    factory, AgUiError, Event, FunctionCall, Message, MessagesSnapshotEvent, Result, RunAgentInput,
    ToolCall, ToolCallKind, UserMessageContent,
};
use ag_ui_server::{agui_router, channel, EventEmitter, RunHandler};
use futures::stream::BoxStream;
use serde_json::json;
use std::time::Duration;

struct AgenticChatHandler;

#[async_trait::async_trait]
impl RunHandler for AgenticChatHandler {
    async fn handle(&self, input: RunAgentInput) -> Result<BoxStream<'static, Result<Event>>> {
        let (emitter, stream) = channel(64);

        let thread_id = input.thread_id.clone();
        let run_id = input.run_id.clone();

        let last = input.messages.last().cloned();
        let last_user_text = match &last {
            Some(Message::User(user)) => match &user.content {
                UserMessageContent::Text(text) => Some(text.clone()),
                _ => None,
            },
            _ => None,
        };
        let last_is_tool = matches!(last, Some(Message::Tool(_)));

        let history = input.messages.clone();

        tokio::spawn(async move {
            let _ = emitter.run_started(&thread_id, &run_id).await;

            if last_is_tool {
                send_tool_result_message(&emitter, &run_id).await;
            } else if last_user_text.as_deref() == Some("tool") {
                send_tool_call(&emitter, &run_id).await;
            } else if last_user_text.as_deref() == Some("backend_tool") {
                send_backend_tool_call(&emitter, &run_id, history).await;
            } else {
                send_countdown(&emitter, &run_id).await;
            }

            let _ = emitter
                .emit(factory::run_finished(&thread_id, &run_id))
                .await;
        });

        Ok(stream)
    }
}

async fn send_countdown(emitter: &EventEmitter, run_id: &str) {
    let message_id = format!("msg-{run_id}-countdown");
    let _ = emitter
        .emit(factory::text_message_start(message_id.clone()))
        .await;
    let _ = emitter
        .emit(factory::text_message_content(
            message_id.clone(),
            "counting down: ",
        ))
        .await;
    for count in (1..=10).rev() {
        let _ = emitter
            .emit(factory::text_message_content(
                message_id.clone(),
                format!("{count}  "),
            ))
            .await;
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
    let _ = emitter
        .emit(factory::text_message_content(message_id.clone(), "✓"))
        .await;
    let _ = emitter.emit(factory::text_message_end(message_id)).await;
}

async fn send_tool_result_message(emitter: &EventEmitter, run_id: &str) {
    let message_id = format!("msg-{run_id}-tool-ack");
    let _ = emitter
        .emit(factory::text_message_start(message_id.clone()))
        .await;
    let _ = emitter
        .emit(factory::text_message_content(
            message_id.clone(),
            "background changed ✓",
        ))
        .await;
    let _ = emitter.emit(factory::text_message_end(message_id)).await;
}

async fn send_tool_call(emitter: &EventEmitter, run_id: &str) {
    let tool_call_id = format!("tc-{run_id}-bg");
    let args = json!({
        "background": "linear-gradient(135deg, #667eea 0%, #764ba2 100%)"
    })
    .to_string();
    let _ = emitter
        .emit(factory::tool_call_start(
            tool_call_id.clone(),
            "change_background",
        ))
        .await;
    let _ = emitter
        .emit(factory::tool_call_args(tool_call_id.clone(), args))
        .await;
    let _ = emitter.emit(factory::tool_call_end(tool_call_id)).await;
}

async fn send_backend_tool_call(emitter: &EventEmitter, run_id: &str, history: Vec<Message>) {
    let tool_call_id = format!("tc-{run_id}-weather");
    let assistant = Message::Assistant(AssistantMessage {
        id: format!("msg-{run_id}-assistant"),
        content: None,
        name: None,
        tool_calls: Some(vec![ToolCall {
            id: tool_call_id.clone(),
            kind: ToolCallKind::Function,
            function: FunctionCall {
                name: "lookup_weather".into(),
                arguments: json!({"city": "San Francisco", "weather": "sunny"}).to_string(),
            },
            encrypted_value: None,
        }]),
        encrypted_value: None,
    });
    let tool_result = Message::Tool(ToolMessage {
        id: format!("msg-{run_id}-tool"),
        content: "The weather in San Francisco is sunny.".into(),
        tool_call_id,
        error: None,
        encrypted_value: None,
    });

    let mut all = history;
    all.push(assistant);
    all.push(tool_result);

    let _ = emitter
        .emit(Event::MessagesSnapshot(MessagesSnapshotEvent {
            messages: all,
            base: Default::default(),
        }))
        .await;
}

#[tokio::main]
async fn main() -> Result<()> {
    let app = agui_router(AgenticChatHandler);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:8000")
        .await
        .map_err(|error| AgUiError::other(error.to_string()))?;

    println!("agentic_chat listening on http://127.0.0.1:8000/");

    ::axum::serve(listener, app)
        .await
        .map_err(|error| AgUiError::other(error.to_string()))?;

    Ok(())
}
