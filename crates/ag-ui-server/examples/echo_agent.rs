use ag_ui_core::{Event, Message, Result, RunAgentInput, UserMessageContent};
use ag_ui_server::{agui_router, channel, RunHandler};
use futures::stream::BoxStream;

struct EchoHandler;

#[async_trait::async_trait]
impl RunHandler for EchoHandler {
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
            })
            .unwrap_or_else(|| "(no input)".to_string());

        let thread_id = input.thread_id.clone();
        let run_id = input.run_id.clone();

        tokio::spawn(async move {
            let _ = emitter.run_started(&thread_id, &run_id).await;
            let _ = emitter
                .text_message("msg-1", &format!("echo: {last_user_text}"))
                .await;
            let _ = emitter.run_finished_success(&thread_id, &run_id).await;
        });

        Ok(stream)
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let app = agui_router(EchoHandler);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:8000")
        .await
        .map_err(|error| ag_ui_core::AgUiError::other(error.to_string()))?;

    println!("listening on http://127.0.0.1:8000/");

    ::axum::serve(listener, app)
        .await
        .map_err(|error| ag_ui_core::AgUiError::other(error.to_string()))?;

    Ok(())
}
