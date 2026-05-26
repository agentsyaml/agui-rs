//! Predictive state updates example, ported from
//! `ag-ui-protocol/integrations/server-starter-all-features/python/.../predictive_state_updates.py`.
//!
//! Branches on the last message role:
//! - Last message is `Message::Tool` → streams a short `"Ok!"` text message.
//! - Otherwise → emits a `PredictState` `CustomEvent`, then a streaming
//!   `write_document_local` tool call whose `arguments` is a JSON object
//!   `{"document": "..."}` delivered chunk-by-chunk (200 ms per token),
//!   followed by a no-arg `confirm_changes` tool call.
//!
//! The dog name is chosen deterministically by hashing `run_id` so the
//! example stays reproducible without pulling in the `rand` crate.
//!
//! Run with:
//!   cargo run -p ag-ui-server --example predictive_state_updates

use ag_ui_core::{factory, AgUiError, CustomEvent, Event, Message, Result, RunAgentInput};
use ag_ui_server::{agui_router, channel, EventEmitter, RunHandler};
use futures::stream::BoxStream;
use serde_json::json;
use std::time::Duration;

const DOG_NAMES: &[&str] = &["Rex", "Buddy", "Max", "Charlie"];

struct PredictiveStateUpdatesHandler;

#[async_trait::async_trait]
impl RunHandler for PredictiveStateUpdatesHandler {
    async fn handle(&self, input: RunAgentInput) -> Result<BoxStream<'static, Result<Event>>> {
        let (emitter, stream) = channel(64);

        let thread_id = input.thread_id.clone();
        let run_id = input.run_id.clone();
        let last_is_tool = matches!(input.messages.last(), Some(Message::Tool(_)));

        tokio::spawn(async move {
            let _ = emitter.run_started(&thread_id, &run_id).await;

            if last_is_tool {
                send_text_message(&emitter, &run_id).await;
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

async fn send_text_message(emitter: &EventEmitter, run_id: &str) {
    let message_id = format!("msg-{run_id}-ok");
    let _ = emitter
        .emit(factory::text_message_start(message_id.clone()))
        .await;
    let _ = emitter
        .emit(factory::text_message_content(message_id.clone(), "Ok!"))
        .await;
    let _ = emitter.emit(factory::text_message_end(message_id)).await;
}

fn pick_dog_name(run_id: &str) -> &'static str {
    let pick = run_id.bytes().fold(0u64, |a, b| a.wrapping_add(b as u64));
    DOG_NAMES[(pick as usize) % DOG_NAMES.len()]
}

async fn send_tool_call_events(emitter: &EventEmitter, run_id: &str) {
    let name = pick_dog_name(run_id);
    let story =
        format!("Once upon a time, there was a dog named {name}. {name} was a very good dog.");

    let _ = emitter
        .emit(Event::Custom(CustomEvent {
            name: "PredictState".into(),
            value: json!([{
                "state_key": "document",
                "tool": "write_document_local",
                "tool_argument": "document"
            }]),
            base: Default::default(),
        }))
        .await;

    let tool_call_id = format!("tc-{run_id}-write");
    let _ = emitter
        .emit(factory::tool_call_start(
            tool_call_id.clone(),
            "write_document_local",
        ))
        .await;
    let _ = emitter
        .emit(factory::tool_call_args(
            tool_call_id.clone(),
            "{\"document\":\"",
        ))
        .await;

    for chunk in story.split(' ') {
        let delta = format!("{chunk} ");
        let _ = emitter
            .emit(factory::tool_call_args(tool_call_id.clone(), delta))
            .await;
        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    let _ = emitter
        .emit(factory::tool_call_args(tool_call_id.clone(), "\"}"))
        .await;
    let _ = emitter.emit(factory::tool_call_end(tool_call_id)).await;

    let confirm_id = format!("tc-{run_id}-confirm");
    let _ = emitter
        .emit(factory::tool_call_start(
            confirm_id.clone(),
            "confirm_changes",
        ))
        .await;
    let _ = emitter
        .emit(factory::tool_call_args(confirm_id.clone(), "{}"))
        .await;
    let _ = emitter.emit(factory::tool_call_end(confirm_id)).await;
}

#[tokio::main]
async fn main() -> Result<()> {
    let app = agui_router(PredictiveStateUpdatesHandler);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:8000")
        .await
        .map_err(|error| AgUiError::other(error.to_string()))?;

    println!("predictive_state_updates listening on http://127.0.0.1:8000/");

    ::axum::serve(listener, app)
        .await
        .map_err(|error| AgUiError::other(error.to_string()))?;

    Ok(())
}
