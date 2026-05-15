# AG-UI Rust SDK

An idiomatic Rust implementation of the [AG-UI (Agent–User Interaction)
protocol](https://github.com/ag-ui-protocol/ag-ui) — the open protocol for
streaming structured events between AI agents and front-end / orchestration
clients.

This SDK is designed to be a peer of the official TypeScript and Python SDKs,
covering the full protocol surface: 33 event types, multimodal messages,
SSE-based HTTP transport, chunk expansion, event-ordering verification,
state/messages reduction, and an `axum`-based server.

> **Status**: pre-1.0. API may evolve before publication to crates.io.

---

## Workspace layout

| Crate            | Purpose                                                                 |
| ---------------- | ----------------------------------------------------------------------- |
| `ag-ui-core`     | Protocol data types, all 33 event payloads, `Event` discriminated enum. |
| `ag-ui-encoder`  | Wire-format encoder. SSE today; protobuf surface stubbed (`Unsupported`). |
| `ag-ui-client`   | `Agent` trait, `HttpAgent`, runner, subscriber, chunk/verify/apply pipeline. |
| `ag-ui-server`   | `RunHandler` trait, channel-based `EventEmitter`, `axum` route builder. |

All four crates share a workspace `Cargo.toml`; build everything with
`cargo build --workspace`.

---

## Quick start

### Server (echo agent)

```rust
use ag_ui_core::{Message, Result, RunAgentInput, UserMessageContent};
use ag_ui_server::{agui_router, channel, RunHandler};
use async_trait::async_trait;
use futures::stream::BoxStream;

struct EchoHandler;

#[async_trait]
impl RunHandler for EchoHandler {
    async fn handle(
        &self,
        input: RunAgentInput,
    ) -> Result<BoxStream<'static, Result<ag_ui_core::Event>>> {
        let (emitter, stream) = channel(32);

        let last_user_text = input.messages.iter().rev().find_map(|m| match m {
            Message::User(u) => match &u.content {
                UserMessageContent::Text(t) => Some(t.clone()),
                _ => None,
            },
            _ => None,
        }).unwrap_or_else(|| "(no input)".into());

        let (thread_id, run_id) = (input.thread_id.clone(), input.run_id.clone());

        tokio::spawn(async move {
            let _ = emitter.run_started(&thread_id, &run_id).await;
            let _ = emitter.text_message("msg-1", &format!("echo: {last_user_text}")).await;
            let _ = emitter.run_finished_success(&thread_id, &run_id).await;
        });

        Ok(stream)
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let app = agui_router(EchoHandler);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:8000").await
        .map_err(|e| ag_ui_core::AgUiError::other(e.to_string()))?;
    axum::serve(listener, app).await
        .map_err(|e| ag_ui_core::AgUiError::other(e.to_string()))?;
    Ok(())
}
```

Run it:

```sh
cargo run -p ag-ui-server --example echo_agent
```

Hit it with `curl`:

```sh
curl -N -X POST http://127.0.0.1:8000/ \
  -H 'Content-Type: application/json' \
  -H 'Accept: text/event-stream' \
  --data '{"threadId":"t1","runId":"r1","messages":[{"role":"user","id":"m1","content":"hello"}],"tools":[],"context":[],"state":{},"forwardedProps":null}'
```

You will see the SSE-framed event stream:

```
data: {"type":"RUN_STARTED","threadId":"t1","runId":"r1"}

data: {"type":"TEXT_MESSAGE_START","messageId":"msg-1","role":"assistant"}

data: {"type":"TEXT_MESSAGE_CONTENT","messageId":"msg-1","delta":"echo: hello"}

data: {"type":"TEXT_MESSAGE_END","messageId":"msg-1"}

data: {"type":"RUN_FINISHED","threadId":"t1","runId":"r1","outcome":{"type":"success"}}
```

### Client

```rust
use ag_ui_client::{AgentConfig, AgentRunner, HttpAgent, HttpAgentConfig, RunAgentParameters};

#[tokio::main]
async fn main() -> ag_ui_core::Result<()> {
    let agent = HttpAgent::new(HttpAgentConfig {
        url: "http://localhost:8000/".to_string(),
        headers: Default::default(),
        agent: AgentConfig::default(),
    });
    let mut runner = AgentRunner::new(agent, AgentConfig::default());
    let result = runner.run_agent(RunAgentParameters::default()).await?;
    println!("{} messages, state={:?}", result.new_messages.len(), result.new_state);
    Ok(())
}
```

Run with:

```sh
cargo run -p ag-ui-client --example basic_agent
```

---

## Examples

The workspace ships five runnable examples ported from the upstream
`integrations/server-starter-all-features` reference (Python & TypeScript) in
the [ag-ui-protocol/ag-ui](https://github.com/ag-ui-protocol/ag-ui) repo.

| Example                                  | Crate           | Mirrors (upstream)                                | Demonstrates                                                  |
| ---------------------------------------- | --------------- | ------------------------------------------------- | ------------------------------------------------------------- |
| `echo_agent`                             | `ag-ui-server`  | quick-start echo server                           | `RunHandler` + `EventEmitter` + `agui_router` end-to-end       |
| `tool_based_generative_ui`               | `ag-ui-server`  | `tool_based_generative_ui.py`                     | Assistant tool call (`generate_haiku`) + `MessagesSnapshot`    |
| `shared_state`                           | `ag-ui-server`  | `shared_state.py`                                 | `StateSnapshot` carrying a structured recipe payload           |
| `human_in_the_loop`                      | `ag-ui-server`  | `human_in_the_loop.py`                            | Streamed `tool_call_args` chunks (12 deltas, 200 ms each)      |
| `basic_agent` / `streaming_client`       | `ag-ui-client`  | TS `AgentSubscriber` demo                         | `HttpAgent` + `AgentRunner` + `AgentSubscriber::on_event`      |

Run a server, then in another terminal hit it:

```sh
# Terminal 1 — pick one server
cargo run -p ag-ui-server --example echo_agent
cargo run -p ag-ui-server --example tool_based_generative_ui
cargo run -p ag-ui-server --example shared_state
cargo run -p ag-ui-server --example human_in_the_loop

# Terminal 2 — either curl raw SSE
curl -N -X POST http://127.0.0.1:8000/ \
  -H 'Content-Type: application/json' \
  -H 'Accept: text/event-stream' \
  --data '{"threadId":"t1","runId":"r1","messages":[{"role":"user","id":"m1","content":"hello"}],"tools":[],"context":[],"state":{},"forwardedProps":null}'

# …or use the Rust client
cargo run -p ag-ui-client --example basic_agent       # one-shot
cargo run -p ag-ui-client --example streaming_client  # subscriber prints each event
```

All five examples are built by `cargo build --workspace --examples` in CI.

---

## Streaming model

The SDK is built around `futures::Stream<Item = Result<Event, AgUiError>>` as
its primary abstraction — not RxJS-style observables. This makes integration
with `tokio`, `axum`, `reqwest` and any other async-std-flavored Rust ecosystem
direct.

The client pipeline composes as:

```
HttpAgent (SSE bytes → Event)
    │
    ▼
expand_chunks   ── TEXT_MESSAGE_CHUNK / TOOL_CALL_CHUNK → Start+Content+End
    │
    ▼
verify_events   ── enforces protocol ordering
    │
    ▼
default_apply_events  ── builds messages + state via JSON Patch
    │
    ▼
AgentSubscriber on_* hooks  +  RunAgentResult
```

The server side is symmetric: the `axum` route negotiates content type, runs
the user’s `RunHandler`, and frames the resulting event stream as SSE.

---

## Protocol coverage

- **33 event types** (lifecycle, text message, tool call, state, step,
  reasoning, activity, raw, custom) plus the 5 deprecated `THINKING_*` events
  required for backward compatibility with older Python agents.
- **Multimodal user input**: text, image, audio, video, document, binary.
- **Tool calls** with chunked argument streaming.
- **State** via `STATE_SNAPSHOT` and `STATE_DELTA` (JSON Patch RFC 6902).
- **Resume** with `ResumeEntry` for interrupt continuations.
- **Content negotiation**: `text/event-stream` (default) and the protobuf
  media type `application/vnd.ag-ui.event+proto` (surface only — encoding /
  decoding returns `AgUiError::Unsupported` until a proto crate lands).

---

## Testing

```
cargo test --workspace          # 1112 tests (unit + ported TS integration)
cargo clippy --workspace --all-targets -- -D warnings
cargo build --workspace --examples
```

Test counts per crate:

| Crate           | Tests |
| --------------- | ----- |
| `ag-ui-core`    | 140   |
| `ag-ui-encoder` |  13   |
| `ag-ui-client`  | 946   |
| `ag-ui-server`  |  13   |

The `ag-ui-client` suite mirrors the TypeScript SDK's
`packages/client/__tests__` directory across `verify/`, `chunks/`,
`transform/`, `run/`, `middleware/`, `interrupts/`, and `agent/`. Cases that
depend on TypeScript-only API surface (e.g. `AbstractAgent.clone()`,
`pendingInterrupts`, `maxVersion`) are kept as `// SKIPPED:` markers in the
corresponding test files so the parity gap stays explicit.

End-to-end SSE round-trip is exercised by running the `echo_agent` example
and hitting it with `curl` (see *Quick start*).

---

## Mapping to the TypeScript SDK

| TypeScript                                 | Rust                                            |
| ------------------------------------------ | ----------------------------------------------- |
| `@ag-ui/core` (types, events)              | `ag-ui-core`                                    |
| `@ag-ui/encoder`                           | `ag-ui-encoder`                                 |
| `@ag-ui/client` `AbstractAgent`            | `ag_ui_client::Agent` + `AgentRunner`           |
| `HttpAgent`                                | `ag_ui_client::HttpAgent`                       |
| `AgentSubscriber`                          | `ag_ui_client::AgentSubscriber`                 |
| `Middleware`                               | `ag_ui_client::Middleware`                      |
| `chunks/transform.ts`                      | `ag_ui_client::chunks::expand_chunks`           |
| `verify/verify.ts`                         | `ag_ui_client::verify::verify_events`           |
| `apply/default.ts`                         | `ag_ui_client::apply::default_apply_events`     |
| `transform/sse.ts` + `transform/http.ts`   | `ag_ui_client::transform`                       |
| FastAPI server reference                   | `ag-ui-server` (`axum`-based)                   |

---

## Roadmap

- Protobuf encoder/decoder (currently stubbed via `AgUiError::Unsupported`).
- WebSocket transport.
- Connect-style secondary client API.
- crates.io publication.

---

## License

Apache-2.0. See the upstream
[ag-ui-protocol](https://github.com/ag-ui-protocol/ag-ui) repository for the
canonical specification.
