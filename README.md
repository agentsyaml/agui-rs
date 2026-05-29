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
| `agui-rs-core`     | Protocol data types, all 33 event payloads, `Event` discriminated enum. |
| `agui-rs-encoder`  | Wire-format encoder. SSE today; protobuf surface stubbed (`Unsupported`). |
| `agui-rs-client`   | `Agent` trait, `HttpAgent`, runner, subscriber, chunk/verify/apply pipeline. |
| `agui-rs-server`   | `RunHandler` trait, channel-based `EventEmitter`, `axum` route builder. |
| `agui-rs`          | Facade crate re-exporting all others via Cargo features.               |

All four crates share a workspace `Cargo.toml`; build everything with
`cargo build --workspace`.

---

## Quick start

### Server (echo agent)

```rust
use agui_rs_core::{Message, Result, RunAgentInput, UserMessageContent};
use agui_rs_server::{agui_router, channel, RunHandler};
use async_trait::async_trait;
use futures::stream::BoxStream;

struct EchoHandler;

#[async_trait]
impl RunHandler for EchoHandler {
    async fn handle(
        &self,
        input: RunAgentInput,
    ) -> Result<BoxStream<'static, Result<agui_rs_core::Event>>> {
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
        .map_err(|e| agui_rs_core::AgUiError::other(e.to_string()))?;
    axum::serve(listener, app).await
        .map_err(|e| agui_rs_core::AgUiError::other(e.to_string()))?;
    Ok(())
}
```

Run it:

```sh
cargo run -p agui-rs-server --example echo_agent
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
use agui_rs_client::{AgentConfig, AgentRunner, HttpAgent, HttpAgentConfig, RunAgentParameters};

#[tokio::main]
async fn main() -> agui_rs_core::Result<()> {
    let agent = HttpAgent::new(HttpAgentConfig {
        url: "http://localhost:8000/".to_string(),
        headers: Default::default(),
        agent: AgentConfig::default(),
        request_executor: None,
    });
    let mut runner = AgentRunner::new(agent, AgentConfig::default());
    let result = runner.run_agent(RunAgentParameters::default()).await?;
    println!("{} messages, state={:?}", result.new_messages.len(), result.new_state);
    Ok(())
}
```

Run with:

```sh
cargo run -p agui-rs-client --example basic_agent
```

---

## Examples

The workspace ships nine runnable examples ported from the upstream
`integrations/server-starter-all-features` reference (Python & TypeScript) in
the [ag-ui-protocol/ag-ui](https://github.com/ag-ui-protocol/ag-ui) repo.
The protocol/examples baseline is `42c57161` (`main`, 2026-05-17) /
tag `release/2026-05-15` (`35e7cac6`), with client updates reviewed through
`d74e2df` / tag `release/2026-05-26` (adds upstream `HttpAgent` custom fetch
parity, exposed here as `HttpAgentConfig::request_executor`).

| Example                       | Crate           | Mirrors (upstream)                  | Demonstrates                                                          |
| ----------------------------- | --------------- | ----------------------------------- | --------------------------------------------------------------------- |
| `echo_agent`                  | `agui-rs-server`  | quick-start echo server             | `RunHandler` + `EventEmitter` + `agui_router` end-to-end              |
| `agentic_chat`                | `agui-rs-server`  | `agentic_chat.py`                   | Branching countdown / `change_background` tool / weather snapshot     |
| `agentic_generative_ui`       | `agui-rs-server`  | `agentic_generative_ui.py`          | `STATE_SNAPSHOT` + JSON-Patch `STATE_DELTA` walking a 10-step plan    |
| `tool_based_generative_ui`    | `agui-rs-server`  | `tool_based_generative_ui.py`       | Assistant tool call (`generate_haiku`) + `MessagesSnapshot`           |
| `shared_state`                | `agui-rs-server`  | `shared_state.py`                   | `STATE_SNAPSHOT` carrying a structured recipe payload                 |
| `human_in_the_loop`           | `agui-rs-server`  | `human_in_the_loop.py`              | Streamed `tool_call_args` chunks (12 deltas, 200 ms each)             |
| `backend_tool_rendering`      | `agui-rs-server`  | `backend_tool_rendering.py`         | `get_weather` tool call + result via `MessagesSnapshot`               |
| `predictive_state_updates`    | `agui-rs-server`  | `predictive_state_updates.py`       | `PredictState` `CustomEvent` + streamed `write_document_local` args   |
| `basic_agent`                 | `agui-rs-client`  | TS one-shot agent demo              | `HttpAgent` + `AgentRunner::run_agent` (no subscriber)                |
| `streaming_client`            | `agui-rs-client`  | TS `AgentSubscriber` demo           | `HttpAgent` + `AgentSubscriber::on_event` printing each event         |

Run a server, then in another terminal hit it:

```sh
# Terminal 1 — pick one server
cargo run -p agui-rs-server --example echo_agent
cargo run -p agui-rs-server --example agentic_chat
cargo run -p agui-rs-server --example agentic_generative_ui
cargo run -p agui-rs-server --example tool_based_generative_ui
cargo run -p agui-rs-server --example shared_state
cargo run -p agui-rs-server --example human_in_the_loop
cargo run -p agui-rs-server --example backend_tool_rendering
cargo run -p agui-rs-server --example predictive_state_updates

# Terminal 2 — either curl raw SSE
curl -N -X POST http://127.0.0.1:8000/ \
  -H 'Content-Type: application/json' \
  -H 'Accept: text/event-stream' \
  --data '{"threadId":"t1","runId":"r1","messages":[{"role":"user","id":"m1","content":"hello"}],"tools":[],"context":[],"state":{},"forwardedProps":null}'

# …or use the Rust client
cargo run -p agui-rs-client --example basic_agent       # one-shot
cargo run -p agui-rs-client --example streaming_client  # subscriber prints each event
```

All nine examples are built green by `cargo build --workspace --examples`.

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
cargo test --workspace          # 1143 tests (unit + ported TS integration)
cargo clippy --workspace --all-targets -- -D warnings
cargo build --workspace --examples
```

Test counts per crate:

| Crate           | Tests |
| --------------- | ----- |
| `agui-rs-core`    | 140   |
| `agui-rs-encoder` |  13   |
| `agui-rs-client`  | 977   |
| `agui-rs-server`  |  13   |

The `agui-rs-client` suite mirrors the TypeScript SDK's
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
| `@ag-ui/core` (types, events)              | `agui-rs-core`                                    |
| `@ag-ui/encoder`                           | `agui-rs-encoder`                                 |
| `@ag-ui/client` `AbstractAgent`            | `agui_rs_client::Agent` + `AgentRunner`           |
| `HttpAgent`                                | `agui_rs_client::HttpAgent`                       |
| `AgentSubscriber`                          | `agui_rs_client::AgentSubscriber`                 |
| `Middleware`                               | `agui_rs_client::Middleware`                      |
| `chunks/transform.ts`                      | `agui_rs_client::chunks::expand_chunks`           |
| `verify/verify.ts`                         | `agui_rs_client::verify::verify_events`           |
| `apply/default.ts`                         | `agui_rs_client::apply::default_apply_events`     |
| `transform/sse.ts` + `transform/http.ts`   | `agui_rs_client::transform`                       |
| FastAPI server reference                   | `agui-rs-server` (`axum`-based)                   |

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
