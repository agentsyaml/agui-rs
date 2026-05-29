# agui-rs-server

Server SDK for the AG-UI protocol — axum-based.

Provides the `RunHandler` trait, a channel-based `EventEmitter`, and an `agui_router`
helper for building axum routes that serve AG-UI event streams.

```rust
use agui_rs_core::{Message, Result, RunAgentInput, UserMessageContent};
use agui_rs_server::{agui_router, channel, RunHandler};
use async_trait::async_trait;
use futures::stream::BoxStream;

struct EchoHandler;

#[async_trait]
impl RunHandler for EchoHandler {
    async fn handle(&self, input: RunAgentInput) -> Result<BoxStream<'static, Result<agui_rs_core::Event>>> {
        let (emitter, stream) = channel(32);
        // ... emit events ...
        Ok(stream)
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let app = agui_router(EchoHandler);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:8000").await.unwrap();
    axum::serve(listener, app).await.unwrap();
    Ok(())
}
```

## License

Apache-2.0
