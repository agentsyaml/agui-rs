//! Agentic generative UI example, ported from
//! `ag-ui-protocol/integrations/server-starter-all-features/python/.../agentic_generative_ui.py`.
//!
//! Emits an initial `STATE_SNAPSHOT` carrying a 10-step plan (all pending),
//! then walks the plan one step at a time, marking each as `completed` via
//! a JSON-Patch `STATE_DELTA` (RFC 6902) with a 1 s gap between updates, and
//! finally emits a final `STATE_SNAPSHOT`.
//!
//! Run with:
//!   cargo run -p agui-rs-server --example agentic_generative_ui

use agui_rs_core::{factory, AgUiError, Event, Result, RunAgentInput};
use agui_rs_server::{agui_router, channel, RunHandler};
use futures::stream::BoxStream;
use serde_json::{json, Value};
use std::time::Duration;

struct AgenticGenerativeUiHandler;

#[async_trait::async_trait]
impl RunHandler for AgenticGenerativeUiHandler {
    async fn handle(&self, input: RunAgentInput) -> Result<BoxStream<'static, Result<Event>>> {
        let (emitter, stream) = channel(64);

        let thread_id = input.thread_id.clone();
        let run_id = input.run_id.clone();

        tokio::spawn(async move {
            let _ = emitter.run_started(&thread_id, &run_id).await;

            let steps: Vec<Value> = (0..10)
                .map(|i| json!({"description": format!("Step {}", i + 1), "status": "pending"}))
                .collect();
            let snapshot = json!({"steps": steps});

            let _ = emitter
                .emit(factory::state_snapshot(snapshot.clone()))
                .await;
            tokio::time::sleep(Duration::from_secs(1)).await;

            for i in 0..10 {
                let patch = vec![json!({
                    "op": "replace",
                    "path": format!("/steps/{i}/status"),
                    "value": "completed"
                })];
                let _ = emitter.emit(factory::state_delta(patch)).await;
                tokio::time::sleep(Duration::from_secs(1)).await;
            }

            let final_steps: Vec<Value> = (0..10)
                .map(|i| json!({"description": format!("Step {}", i + 1), "status": "completed"}))
                .collect();
            let final_snapshot = json!({"steps": final_steps});
            let _ = emitter.emit(factory::state_snapshot(final_snapshot)).await;

            let _ = emitter
                .emit(factory::run_finished(&thread_id, &run_id))
                .await;
        });

        Ok(stream)
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let app = agui_router(AgenticGenerativeUiHandler);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:8000")
        .await
        .map_err(|error| AgUiError::other(error.to_string()))?;

    println!("agentic_generative_ui listening on http://127.0.0.1:8000/");

    ::axum::serve(listener, app)
        .await
        .map_err(|error| AgUiError::other(error.to_string()))?;

    Ok(())
}
