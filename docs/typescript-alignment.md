# TypeScript SDK Alignment Audit

**Single source of truth:** the official TypeScript SDK in the
[`ag-ui-protocol/ag-ui`](https://github.com/ag-ui-protocol/ag-ui) monorepo,
`sdks/typescript/packages/{core,client,encoder}`.

| Tracked             | Value                                                         |
| ------------------- | ------------------------------------------------------------- |
| TS package versions | `@ag-ui/core` / `@ag-ui/client` / `@ag-ui/encoder` **0.0.54** |
| Monorepo commit     | `f30021b9dddb97f827f463ab36bddd34ac6bb764` (2026-05-29)       |
| Local checkout      | `../ag-ui`                                                    |
| Audited             | 2026-05-30                                                    |

This SDK aligns all protocol types, events, wire format, and runtime behaviour
to that reference. Rust-specific ergonomics are layered on top **without
diverging** from the contract.

## 1. Core types (`@ag-ui/core` → `agui-rs-core`)

Audited `core/src/types.ts` and `core/src/events.ts` field-by-field.

| TS construct                                                        | Rust                                           | Status                                        |
| ------------------------------------------------------------------- | ---------------------------------------------- | --------------------------------------------- |
| `EventType` (33 active + 5 deprecated `THINKING_*`)                 | `events::EventType`                            | ✅ identical set + wire names                  |
| `Event` discriminated union (`type` tag)                            | `events::Event` (`#[serde(tag = "type")]`)     | ✅ identical                                   |
| `Message` (developer/system/assistant/user/tool/activity/reasoning) | `types::Message`                               | ✅ identical                                   |
| `Role` (7 roles) / `TextMessageRole` (4 roles, default assistant)   | `types::Role` / `TextMessageRole`              | ✅ identical                                   |
| `ToolCall` / `FunctionCall`                                         | `types::ToolCall` / `FunctionCall`             | ✅ identical                                   |
| `InputContent` (text/image/audio/video/document + legacy binary)    | `types::InputContent`                          | ✅ identical (incl. binary payload validation) |
| `RunAgentInput` (+ optional `resume`)                               | `types::RunAgentInput`                         | ✅ identical                                   |
| `Interrupt` / `ResumeEntry` / `ResumeStatus`                        | `types::*`                                     | ✅ identical                                   |
| `State = any`                                                       | `type State = serde_json::Value`               | ✅ aligned (untyped, per canonical)            |
| `RunFinishedOutcome` (success / interrupt)                          | `events::RunFinishedOutcome`                   | ✅ identical                                   |
| `AgentCapabilities` (identity/transport/tools/output/state/multiAgent/reasoning/multimodal/execution/humanInTheLoop/custom) | `capabilities::AgentCapabilities` | ✅ realigned to canonical (2026-05-30) |
| `AGUIError` / `AGUIConnectNotImplementedError`                      | `error::AgUiError` (+ `ConnectNotImplemented`) | ✅ aligned                                     |

Wire format verified by exact-string round-trip tests
(`assert_eq!(json["type"], "RUN_STARTED")`, camelCase fields, `tool-call`
kebab subtype, etc.).

## 2. Encoder (`@ag-ui/encoder` → `agui-rs-encoder`)

| TS                                                      | Rust                        | Status                                        |
| ------------------------------------------------------- | --------------------------- | --------------------------------------------- |
| SSE media type `text/event-stream`                      | `AGUI_MEDIA_TYPE_SSE`       | ✅                                             |
| Protobuf media type `application/vnd.ag-ui.event+proto` | `AGUI_MEDIA_TYPE_PROTOBUF`  | ✅ constant; encode/decode `Unsupported`       |
| `EventEncoder` content negotiation                      | `EventEncoder::with_accept` | ✅ behaviourally aligned                       |
| protobuf encode/decode                                  | —                           | ⚠️ **gap**: stubbed (`AgUiError::Unsupported`) |

## 3. Client (`@ag-ui/client` → `agui-rs-client`)

| TS construct                                                                     | Rust                                                       | Status                                                     |
| -------------------------------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- |
| `transformChunks`                                                                | `chunks::expand_chunks`                                    | ✅                                                          |
| `verifyEvents`                                                                   | `verify::verify_events`                                    | ✅ aligned incl. multi-run streams (2026-05-30)             |
| `defaultApplyEvents`                                                             | `apply::default_apply_events`                              | ✅                                                          |
| `AgentSubscriber` hooks                                                          | `subscriber::AgentSubscriber`                              | ✅ behaviour; ⚠️ no `stopPropagation` mutation chaining      |
| `Middleware` / `FunctionMiddleware`                                              | `middleware::{Middleware, MiddlewareChain}`                | ✅                                                          |
| `BackwardCompatibility_0_0_{39,45,47}`                                           | `middleware::backward_compat::*`                           | ✅ logic; ⚠️ no version-gated auto-insertion                 |
| `FilterToolCallsMiddleware`                                                      | `middleware::filter_tool_calls`                            | ✅                                                          |
| `HttpAgent` (SSE)                                                                | `http::HttpAgent`                                          | ✅                                                          |
| `HttpAgentConfig.fetch` custom fetch                                             | `HttpAgentConfig::request_executor`                        | ✅ (Rust shape)                                             |
| `interrupts` helpers (`getRunOutcome`, `isInterruptExpired`, `buildResumeArray`) | `interrupts::*`                                            | ✅                                                          |
| **`AbstractAgent.pendingInterrupts` + resume enforcement**                       | `AgentRunner::pending_interrupts` + `ensure_resume_covers` | ✅ **newly aligned (2026-05-30)**                           |
| `convertToLegacyEvents`                                                          | `legacy::convert_legacy_events`                            | ✅                                                          |
| `compactEvents`                                                                  | `compact::compact_events`                                  | ✅                                                          |
| `DebugLogger`                                                                    | `debug_logger::DebugLogger`                                | ✅ type; ⚠️ not wired into transform/verify/chunks pipelines |

### Behavioural gap closed this round — `pendingInterrupts`

TS `AbstractAgent` tracks interrupts emitted by `RUN_FINISHED`
(`outcome.type === "interrupt"`) in `pendingInterrupts`, and on the next
`runAgent()`:
- throws if any pending interrupt id is not addressed by `resume`;
- throws if any pending interrupt has expired (`expiresAt <= now`);
- clears the set on a successful (non-interrupt) outcome.

The Rust stateful `AgentRunner` now mirrors this exactly:
- new field `pending_interrupts: Vec<Interrupt>` (+ `pending_interrupts()` accessor);
- populated/cleared on `RUN_FINISHED` outcome (matching `apply/default.ts`);
- enforced at the start of `run_agent_cancellable` via
  `interrupts::ensure_resume_covers(pending, &resume, now)`;
- expiry uses an injectable clock (`AgentRunner::with_now_fn`) to keep
  `agui-rs-core`/`-client` free of a date dependency. The default clock never
  expires (returns `""`), so behaviour is opt-in and deterministic.

Ported from `agent/__tests__/interrupts-lifecycle.test.ts`
(`tests/interrupts_lifecycle.rs`, 6 cases).

### Behavioural gap closed this round — `verifyEvents` multi-run streams

TS `verifyEvents` is a per-stream state machine that supports **multiple
sequential runs**: after `RUN_FINISHED`, a new `RUN_STARTED` resets per-run
state and a fresh run begins. It also treats `RUN_ERROR` as **permanently
terminal** (no event, not even a new run, may follow), allows `RUN_ERROR` as the
very first event, and imposes **no end-of-stream requirement** (a stream may
end mid-run).

The Rust `verify::verify_events` previously treated `RUN_FINISHED` as terminal
(one run per stream) and required the stream to end after a terminal event.
It now mirrors TS exactly:
- new `RUN_STARTED` after `RUN_FINISHED` resets run state and continues;
- `run_errored` is a separate, permanent terminal flag (never reset);
- first event may be `RUN_STARTED` or `RUN_ERROR`;
- the end-of-stream `finish()` enforcement was removed (TS has none).

Ported from `verify/__tests__/verify.multiple-runs.test.ts` (6 cases) and
`verify/__tests__/verify.lifecycle.test.ts` (2 previously-skipped cases:
`RUN_ERROR` after `RUN_FINISHED`, `RUN_ERROR` as first event).

### Type gap closed this round — `AgentCapabilities` schema

A full export-by-export re-audit (not relying on the `// SKIPPED:` markers)
found that the Rust `capabilities` module had a **stale, divergent shape**
(`messages/context/tools/state/events/activity/reasoning/interrupts/extra`) that
did **not** match the canonical `AgentCapabilities` schema. It also conflicted
with a second, even older `AgentCapabilities` stub in `types.rs`.

`capabilities.rs` was rewritten to mirror canonical `core/src/capabilities.ts`
exactly:
`identity / transport / tools / output / state / multiAgent / reasoning /
multimodal (input+output) / execution / humanInTheLoop / custom`, plus
`SubAgentInfo`. Interrupt flags now live on `HumanInTheLoopCapabilities`
(`interrupts`, `approveWithEdits`) as in canonical — not a standalone
`InterruptsCapabilities`. The stale `types.rs` stub was removed;
`Capabilities` is kept as a type alias for `AgentCapabilities`.

Ported from `core/src/__tests__/capabilities-interrupts.test.ts`
(`tests/capabilities_interrupts.rs`, rewritten to target
`HumanInTheLoopCapabilities`).

## 4. Intentional, documented divergences (Rust-idiomatic, non-protocol)

These do not change wire behaviour and are tracked as `// SKIPPED:` markers in
the corresponding test files:

| Area                                                       | Rationale                                                                 |
| ---------------------------------------------------------- | ------------------------------------------------------------------------- |
| `Observable`/RxJS → `futures::Stream`                      | Idiomatic async Rust; same event ordering.                                |
| `clone()` of the agent                                     | `AgentRunner` owns `Arc<A>` + boxed clock; not `Clone`.                   |
| `connect()` / `connectAgent()` / `events$`                 | Persistent-connection + replay-subject surface not yet implemented.       |
| `getCapabilities()` / `maxVersion`                         | Not yet exposed on the runner.                                            |
| Version-gated auto-insertion of backward-compat middleware | Rust exposes the middleware explicitly; no `maxVersion` field to gate on. |
| `DebugLogger` integration into pipelines                   | Logger type exists; per-stage hooks not wired.                            |
| Subscriber `stopPropagation` mutation chaining             | Rust subscriber hooks return `Result`, not mutation objects.              |
| protobuf encode/decode                                     | Stubbed (`Unsupported`) pending a proto crate.                            |

## 5. Rust-only additions (supersets, do not affect TS parity)

- `agui-rs-server` (axum `RunHandler` + `EventEmitter` + `serve`) — TS is client-only.
- Structured `AgUiError::{Http, Transport}` with `is_retryable()` / `is_user_input()`.
- `AbortHandle` / `run_cancellable` cooperative cancellation.
- `agui-rs` facade crate with Cargo features.
