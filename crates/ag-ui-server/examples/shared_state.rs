//! Shared-state example, ported from
//! `ag-ui-protocol/integrations/server-starter-all-features/python/.../shared_state.py`.
//!
//! Emits a single `STATE_SNAPSHOT` carrying a recipe object, framing it
//! between `RUN_STARTED` and `RUN_FINISHED`. The client side then sees the
//! recipe via `default_apply_events` (state hydration).
//!
//! Run with:
//!   cargo run -p ag-ui-server --example shared_state

use ag_ui_core::{factory, AgUiError, Event, Result, RunAgentInput};
use ag_ui_server::{agui_router, channel, RunHandler};
use futures::stream::BoxStream;
use serde_json::json;

struct SharedStateHandler;

#[async_trait::async_trait]
impl RunHandler for SharedStateHandler {
    async fn handle(&self, input: RunAgentInput) -> Result<BoxStream<'static, Result<Event>>> {
        let (emitter, stream) = channel(16);

        let thread_id = input.thread_id.clone();
        let run_id = input.run_id.clone();

        tokio::spawn(async move {
            let _ = emitter.run_started(&thread_id, &run_id).await;

            let snapshot = json!({
                "recipe": {
                    "skillLevel": "Advanced",
                    "specialPreferences": ["Low Carb", "Spicy"],
                    "cookingTime": "15 min",
                    "ingredients": [
                        {"icon": "🍗", "name": "chicken breast", "amount": "1"},
                        {"icon": "🌶️", "name": "chili powder", "amount": "1 tsp"},
                        {"icon": "🧂", "name": "Salt", "amount": "a pinch"},
                        {"icon": "🥬", "name": "Lettuce leaves", "amount": "handful"}
                    ],
                    "instructions": [
                        "Season chicken with chili powder and salt.",
                        "Sear until fully cooked.",
                        "Slice and wrap in lettuce."
                    ]
                }
            });

            let _ = emitter.emit(factory::state_snapshot(snapshot)).await;
            let _ = emitter
                .emit(factory::run_finished(&thread_id, &run_id))
                .await;
        });

        Ok(stream)
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let app = agui_router(SharedStateHandler);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:8000")
        .await
        .map_err(|error| AgUiError::other(error.to_string()))?;

    println!("shared_state listening on http://127.0.0.1:8000/");

    ::axum::serve(listener, app)
        .await
        .map_err(|error| AgUiError::other(error.to_string()))?;

    Ok(())
}
