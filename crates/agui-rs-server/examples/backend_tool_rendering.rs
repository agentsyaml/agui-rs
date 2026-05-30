//! Backend tool rendering example, ported from
//! `ag-ui-protocol/integrations/server-starter-all-features/python/.../backend_tool_rendering.py`.
//!
//! Branches on the last message role:
//! - Last message is `Message::Tool` → streams a short `TextMessage*`
//!   ("Retrieved weather information!").
//! - Otherwise → constructs an `AssistantMessage` with a `get_weather` tool
//!   call plus a synthetic `ToolMessage` result and emits the full history
//!   as a single `MessagesSnapshot`.
//!
//! Run with:
//!   cargo run -p agui-rs-server --example backend_tool_rendering

use agui_rs_core::types::{AssistantMessage, ToolMessage};
use agui_rs_core::{
    factory, Event, FunctionCall, Message, MessagesSnapshotEvent, Result, RunAgentInput,
    ToolCall, ToolCallKind,
};
use agui_rs_server::{agui_router, channel, serve, EventEmitter, RunHandler};
use futures::stream::BoxStream;
use serde_json::json;

struct BackendToolRenderingHandler;

#[async_trait::async_trait]
impl RunHandler for BackendToolRenderingHandler {
    async fn handle(&self, input: RunAgentInput) -> Result<BoxStream<'static, Result<Event>>> {
        let (emitter, stream) = channel(32);

        let thread_id = input.thread_id.clone();
        let run_id = input.run_id.clone();
        let last_is_tool = matches!(input.messages.last(), Some(Message::Tool(_)));
        let history = input.messages.clone();

        tokio::spawn(async move {
            let _ = emitter.run_started(&thread_id, &run_id).await;

            if last_is_tool {
                send_tool_result_message(&emitter, &run_id).await;
            } else {
                send_backend_tool_call(&emitter, &run_id, history).await;
            }

            let _ = emitter
                .emit(factory::run_finished(&thread_id, &run_id))
                .await;
        });

        Ok(stream)
    }
}

async fn send_tool_result_message(emitter: &EventEmitter, run_id: &str) {
    let message_id = format!("msg-{run_id}-ack");
    let _ = emitter
        .emit(factory::text_message_start(message_id.clone()))
        .await;
    let _ = emitter
        .emit(factory::text_message_content(
            message_id.clone(),
            "Retrieved weather information!",
        ))
        .await;
    let _ = emitter.emit(factory::text_message_end(message_id)).await;
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
                name: "get_weather".into(),
                arguments: json!({"city": "San Francisco"}).to_string(),
            },
            encrypted_value: None,
        }]),
        encrypted_value: None,
    });

    let tool_result = Message::Tool(ToolMessage {
        id: format!("msg-{run_id}-tool"),
        content: json!({
            "city": "San Francisco",
            "conditions": "sunny",
            "wind_speed": "10",
            "temperature": "20",
            "humidity": "60"
        })
        .to_string(),
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
    let app = agui_router(BackendToolRenderingHandler);
    println!("backend_tool_rendering listening on http://127.0.0.1:8000/");
    serve("127.0.0.1:8000", app).await
}
