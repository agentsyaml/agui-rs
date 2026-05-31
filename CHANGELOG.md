# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## Upstream tracking

The **single source of truth** for this SDK is the official TypeScript SDK in the
[`ag-ui-protocol/ag-ui`](https://github.com/ag-ui-protocol/ag-ui) monorepo
(`sdks/typescript/packages/{core,client,encoder}`). All protocol types, events,
wire format, and runtime behaviour are aligned to it. Rust-specific ergonomics
are layered on top without diverging from that contract.

| Tracked upstream | Value |
| ---------------- | ----- |
| TypeScript SDK packages | `@ag-ui/core`, `@ag-ui/client`, `@ag-ui/encoder` `0.0.54` |
| Monorepo commit | `f30021b9dddb97f827f463ab36bddd34ac6bb764` (2026-05-29) |
| Reviewed | 2026-05-30 |

## [0.1.1] - 2026-05-31

### Fixed (faithfulness, 2026-05-31)
- **`compact_events` no longer compacts reasoning.** The canonical TS
  `compactEvents` only compacts text messages, tool calls, and state; reasoning
  events are not in its streaming set and pass through unchanged. The Rust port
  had added a reasoning-compaction path (concatenating
  `REASONING_MESSAGE_CONTENT` deltas) that diverged from upstream — it has been
  removed so reasoning events pass through verbatim, matching TS exactly. Also
  simplifies the module (drops the `PendingReasoning` accumulator + its flush).
- **`expand_chunks` close-set corrected.** TS `transformChunks` closes a pending
  chunk stream before `TOOL_CALL_RESULT` and `CUSTOM` (only `RAW`,
  `ACTIVITY_SNAPSHOT`, `ACTIVITY_DELTA`, `REASONING_ENCRYPTED_VALUE` pass through
  without closing). The Rust port wrongly treated `CUSTOM` and `TOOL_CALL_RESULT`
  as non-closing. Fixed, with regression tests
  (`custom_event_closes_pending_text_message`,
  `tool_call_result_closes_pending_text_message`). Also removed a spurious
  close-on-empty-chunk branch that had no TS counterpart.

### Changed (elegance, 2026-05-31)
- `expand_chunks` rewritten around a single `OpenChunk` enum
  (`Text` / `Tool` / `Reasoning`), mirroring the TS single-`mode` state machine,
  replacing three parallel `Option`s and the duplicated "close the other two"
  logic in every handler. At most one chunk stream is open at a time, as in TS.

### Added
- **`agui-rs-proto` crate**: full protobuf binary encoding/decoding for the 18
  events in the canonical `events.proto` schema, using hand-written `prost`
  messages (no `protoc` build dependency). Wired into
  `EventEncoder::encode_protobuf` (4-byte big-endian length-prefixed framing)
  and `agui_rs_client::parse_proto_stream` (length-prefixed frame reassembly +
  decode). Exposed on the facade crate behind the `proto` feature.
- `Agent::connect` / `Agent::connect_cancellable` + `AgentRunner::connect_agent`
  / `connect_agent_with` — connect-style runs, with `ConnectNotImplemented`
  swallowed into an empty result (mirrors TS `connectAgent`).
- `Agent::capabilities` + `AgentRunner::capabilities` — optional declared
  `AgentCapabilities` (mirrors TS `getCapabilities?()`).
- `AgentRunner::clone_runner` — deep-copies messages/state/pending interrupts,
  shares the agent/middleware/subscribers (mirrors TS `clone()`).
- `AgentRunner::with_max_version` + `AgentConfig::debug` — version-gated
  auto-insertion of backward-compat middleware (mirrors the TS `AbstractAgent`
  constructor) and runner-level lifecycle debug logging via `DebugLogger`
  (`forced` constructor for explicit opt-in).
- Multi-subscriber support on `AgentRunner`: `subscribe()` (returns a
  `Subscription` with `unsubscribe()`), `subscriber_count()`, and a one-shot
  per-run subscriber via `run_agent_with(params, subscriber)`. Subscribers are
  invoked in registration order; replacements from `on_new_message` /
  `on_new_tool_call` chain through subscribers. Mirrors TS
  `AbstractAgent.subscribe` + `runAgent(params, subscriber)`.
- Agent mutator API: `AgentRunner::{add_message, add_messages, set_messages,
  set_state}` plus `messages()` / `state()` / `thread_id()` accessors, with
  subscriber notifications (`on_new_message`, `on_new_tool_call`,
  `on_messages_changed`, `on_state_changed`). Mirrors TS `AbstractAgent`.
- `AgentRunner::pending_interrupts()` and `with_now_fn()` — the stateful runner
  now tracks unresolved interrupts emitted by a `RUN_FINISHED` interrupt
  outcome, matching TypeScript `AbstractAgent.pendingInterrupts`.
- `interrupts::ensure_resume_covers()` and `interrupts::interrupt_is_expired()`
  helpers, ported from TS `AbstractAgent.onInitialize` enforcement and
  `isInterruptExpired`.
- `docs/typescript-alignment.md`: full field-by-field alignment audit against
  TypeScript SDK `0.0.54`.
- **Server protobuf responses**: `agui-rs-server` now content-negotiates
  protobuf. When a request's `Accept` header selects
  `application/vnd.ag-ui.event+proto`, the route streams length-prefixed
  protobuf frames via the new `proto_body` helper instead of returning
  `406 Not Acceptable`. SSE remains the default.
- Public re-exports aligning the client crate's middleware surface with the TS
  `@ag-ui/client` middleware exports: `FilterToolCallsMiddleware`,
  `FilterToolCallsConfig`, and `BackwardCompat0_0_{39,45,47}`.

### Changed
- Aligned the SDK against the TypeScript SDK `0.0.54` (commit `f30021b9`), now
  the **single source of truth**. See `docs/typescript-alignment.md`.
- `AgentRunner` now enforces interrupt resumption before a run: a run that
  follows an interrupt outcome must address every open interrupt via `resume`
  (else `AgUiError::Validation`) and reject any expired interrupt — mirroring
  the TypeScript client. Successful outcomes clear the pending set.
- `verify_events` now supports **multiple sequential runs**, matching the
  TypeScript `verifyEvents` state machine: a `RUN_STARTED` after `RUN_FINISHED`
  resets per-run state and starts a fresh run; `RUN_ERROR` is permanently
  terminal; `RUN_ERROR` is accepted as the first event; the previous
  end-of-stream "must finish" enforcement (a Rust-only addition) was removed to
  match TS, which imposes no such requirement.
- **`AgentCapabilities` realigned to canonical.** A full export-by-export
  re-audit found the `capabilities` module had a stale, divergent shape and a
  duplicate stub in `types.rs`. Rewrote `capabilities.rs` to mirror
  `core/src/capabilities.ts` exactly (`identity / transport / tools / output /
  state / multiAgent / reasoning / multimodal / execution / humanInTheLoop /
  custom` + `SubAgentInfo`); interrupt flags now live on
  `HumanInTheLoopCapabilities`. `Capabilities` remains as a type alias.
  **Breaking** for any code using the old capability field names.
- **`compact_events` now performs state compaction.** Previously it merged only
  text/tool-call/reasoning deltas and silently dropped `STATE_SNAPSHOT` /
  `STATE_DELTA` reduction — a blind spot found by the full test-case
  reconciliation (no `// SKIPPED:` marker existed). It now collects state events
  per run and flushes a single reduced `STATE_SNAPSHOT` (JSON-Patch applied) at
  `RUN_STARTED` (pre-/inter-run), `RUN_FINISHED` / `RUN_ERROR`, and end —
  matching canonical `compact.ts`.

### Changed (cleanup round, 2026-05-31)
- Removed all dead code masked by `#[allow(dead_code)]`: the
  `FilterToolCallsMiddleware::should_filter_tool`/`should_filter_tool_name`
  helpers (the `Middleware::run` impl filters inline) were deleted, and the
  `#[allow(dead_code)]` annotations on the `filter_tool_calls` and
  `backward_compat` modules were removed — both modules are now reachable
  through public re-exports (TS-aligned) and `with_max_version`. No source file
  carries any `#[allow(...)]` attribute.
- Refreshed stale `// SKIPPED:` markers that no longer reflect implemented
  behaviour: `interrupts_lifecycle.rs` (clone interrupt preservation is now a
  real test via `clone_runner`), `transform_http.rs` (protobuf transform is
  implemented, not `Unsupported`), and `agent_debug.rs` (`AgentConfig::debug`
  forced-logger construction is now a real test).
- `docs/typescript-alignment.md` §2/§5 corrected: protobuf encode/decode is no
  longer described as a stubbed gap.

### Added (docs/tooling)
- `docs/test-reconciliation.md`: per-file, per-case TS⇄Rust reconciliation with
  classification of every file where Rust coverage is below the TS case count,
  plus a reproducible (ripgrep-based) counting method.

### Notes
- `agui-rs-core` / `agui-rs-client` remain free of any date dependency; interrupt
  expiry uses an injectable clock (`AgentRunner::with_now_fn`), defaulting to a
  never-expire clock for deterministic, opt-in behaviour.
- Ported tests now passing (previously `// SKIPPED:`):
  `verify/__tests__/verify.multiple-runs.test.ts` (6),
  `verify/__tests__/verify.lifecycle.test.ts` (2),
  `agent/__tests__/interrupts-lifecycle.test.ts` (6),
  `core/__tests__/capabilities-interrupts.test.ts` (rewritten for canonical shape),
  `compact/__tests__/compact.test.ts` "State Compaction" (13),
  `agent/__tests__/subscriber.test.ts` registry/temporary cases (6, in
  `tests/subscriber_registry.rs`),
  `agent/__tests__/agent-mutations.test.ts` (9, in `tests/agent_mutators.rs`),
  `agent/__tests__/agent-clone.test.ts` + connect/capabilities (8, in
  `tests/agent_lifecycle.rs`),
  `middleware/__tests__/backward-compatibility-*` auto-insertion (5, in
  `tests/middleware_auto_insertion.rs`),
  `transform/__tests__/proto.test.ts` (6, in `tests/transform_proto.rs`) +
  `agui-rs-proto` round-trips (9).
- Remaining divergences are architectural / JS-runtime-only (subscriber
  `stopPropagation` mutation-object model, `events$`/`detachActiveRun`,
  per-stream-stage debug logging, frozen-input/ESM cases). See
  `docs/typescript-alignment.md` §4.
- Verified: `cargo build --workspace --examples`, `cargo clippy --workspace
  --all-targets` (zero warnings), `cargo test --workspace` (1308 tests passing).

## [0.1.0] - 2026-05-29

### Added
- Initial public release on crates.io.
- `agui-rs-core`: protocol types, 33 event variants, factory helpers.
- `agui-rs-encoder`: SSE encoder + protobuf media-type negotiation surface.
- `agui-rs-client`: `HttpAgent`, `AgentRunner`, subscriber hooks, middleware chain.
- `agui-rs-server`: `axum` route builder, `RunHandler`, channel `EventEmitter`.
- `agui-rs`: facade crate with `core` / `encoder` / `client` / `server` / `full` features.

[0.1.0]: https://github.com/agentsyaml/agui-rs/releases/tag/v0.1.0
