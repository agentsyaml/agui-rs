//! Human-in-the-loop example, ported from
//! `ag-ui-protocol/integrations/server-starter-all-features/python/.../human_in_the_loop.py`.
//!
//! On the first turn the agent emits a `generate_task_steps` tool call whose
//! JSON arguments are streamed in 12 chunks (200 ms apart). When the user
//! returns a `tool` role message (the human's approval), the agent answers
//! with a short text confirmation instead.
//!
//! Run with:
//!   cargo run -p ag-ui-server --example human_in_the_loop

use std::time::Duration;

use ag_ui_core::{factory, AgUiError, Event, Message, Result, RunAgentInput};
use ag_ui_server::{agui_router, channel, EventEmitter, RunHandler};
use futures::stream::BoxStream;

struct HumanInTheLoopHandler;

async fn send_tool_call_events(emitter: &EventEmitter, run_id: &str) {
    let tool_call_id = format!("tc-{}-steps", run_id);
    let _ = emitter
        .emit(factory::tool_call_start(&tool_call_id, "generate_task_steps"))
        .await;
    let _ = emitter
        .emit(factory::tool_call_args(&tool_call_id, "{\"steps\":["))
        .await;

    for i in 0..10 {
        let comma = if i == 9 { "" } else { "," };
        let chunk = format!(
            "{{\"description\":\"Step {}\",\"status\":\"enabled\"}}{}",
            i + 1,
            comma
        );
        let _ = emitter.emit(factory::tool_call_args(&tool_call_id, chunk)).await;
        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    let _ = emitter.emit(factory::tool_call_args(&tool_call_id, "]}")).await;
    let _ = emitter.emit(factory::tool_call_end(&tool_call_id)).await;
}

async fn send_text_message_events(emitter: &EventEmitter, run_id: &str) {
    let message_id = format!("msg-{}-ack", run_id);
    let _ = emitter
        .text_message(&message_id, "Ok! I'm working on it.")
        .await;
}

#[async_trait::async_trait]
impl RunHandler for HumanInTheLoopHandler {
    async fn handle(&self, input: RunAgentInput) -> Result<BoxStream<'static, Result<Event>>> {
        let (emitter, stream) = channel(32);

        let thread_id = input.thread_id.clone();
        let run_id = input.run_id.clone();
        let last_was_tool = matches!(input.messages.last(), Some(Message::Tool(_)));

        tokio::spawn(async move {
            let _ = emitter.run_started(&thread_id, &run_id).await;

            if last_was_tool {
                send_text_message_events(&emitter, &run_id).await;
            } else {
                send_tool_call_events(&emitter, &run_id).await;
            }

            let _ = emitter
                .emit(factory::run_finished(&thread_id, &run_id))
                .await;
        });

        Ok(stream)
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let app = agui_router(HumanInTheLoopHandler);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:8000")
        .await
        .map_err(|error| AgUiError::other(error.to_string()))?;

    println!("human_in_the_loop listening on http://127.0.0.1:8000/");

    ::axum::serve(listener, app)
        .await
        .map_err(|error| AgUiError::other(error.to_string()))?;

    Ok(())
}
