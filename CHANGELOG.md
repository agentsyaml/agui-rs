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
| TypeScript SDK packages | `@ag-ui/core`, `@ag-ui/client`, `@ag-ui/encoder` `1.0.0` |
| Monorepo commit | `024332cbb71e03e6a6bc055bed5af9c5c504471a` (2026-09-28) |
| Reviewed | 2026-09-28 |

## [0.2.0] - 2026-09-29

**Breaking release.** Realigns every crate onto the official `ag-ui` **1.0.0**
protocol, pinned at monorepo commit `024332cb`.

### Decision — 100% protocol replication, no compatibility layer

Earlier releases tracked `0.0.5x`, whose semantics are now superseded. This
release adopts the 1.0.0 contract exactly and **keeps no shims for the old one**:
where a name, field, event, or behaviour changed, it is removed rather than
deprecated. A compatibility surface that upstream does not have is a divergence
from the source of truth, and a dual-path API is the thing most likely to drift
again.

The upstream wire artifacts are vendored byte-for-byte into
`crates/agui-rs-proto/upstream-spec/` (`schema.json`, `proto-freeze.txt`, and
the three generated `.proto` files, each with recorded blob SHAs), and
`crates/agui-rs-proto/tests/proto_drift.rs` parses the `.proto` files to
mechanically check `src/schema.rs` against them.

> Note: upstream 1.0.0 itself still ships `legacy/convert.ts` bridging 0.0.3x
> consumers. That is retained here as the `legacy` module. What was removed is
> strictly what upstream removed — the `THINKING_*` events.

### Removed (breaking)

- **`THINKING_*` events, entirely.** Upstream 1.0.0 dropped `THINKING_*` from
  the protocol. Gone: `EventType::{ThinkingStart, ThinkingEnd,
  ThinkingTextMessageStart, ThinkingTextMessageContent, ThinkingTextMessageEnd}`,
  their event structs and `Event` variants, the `create_thinking_*` factories,
  and all re-exports.
- **The whole self-invented compat layer.** The `BackwardCompat0_0_39`,
  `BackwardCompat0_0_45`, and `BackwardCompat0_0_47` modules and their crate-root
  re-exports; `AgentRunner::with_max_version` and its private `version_lte`;
  `AgentSubscriber::on_thinking_start` / `on_thinking_end` (and the
  `ThinkingStartCtx` / `ThinkingEndCtx` aliases); the five `LegacyThinking*`
  types and their `LegacyEvent` variants. The `legacy` conversion module and
  `FilterToolCallsMiddleware` remain.
- Deleted test files: `agent_version.rs`, `middleware_auto_insertion.rs`,
  `middleware_backward_compat_0_0_{39,45,47}.rs`.

### Changed (breaking) — types and fields

- New `AttributableFields`: 24 events carry an extra `attributable` object,
  flattened to an optional `subagentRunId`.
- `InputContent` → `ContentPart` (the `Binary` variant is gone; 5 variants
  remain, each gaining `id?`, plus `TextPart.metadata?`).
  `InputContentSource` → `PartSource`, which gains a `File` variant.
- `ToolCallResultEvent.content` and `ToolMessage.content` are now
  `ToolResultContent` (`string | ContentPart[]`).
- `RunFinishedOutcome` has three branches (new: `Cancelled`), and `Success` is
  now a struct carrying `pending_tool_call_ids`.
- `ActivitySnapshotEvent.replace` is `Option<bool>`; absent means "replace",
  which is the spec's semantics.
- `RunStartedEvent` gains `protocol_version`.
- `RunAgentInput.state` and `.forwarded_props` are now `Option<Value>`.
- `ExecutionCapabilities.{max_iterations, max_execution_time}`: `f64` → `u64`.
- Seven message types gain `metadata?` and `subagentRunId?`.
- `ToolCall` gains `metadata?`; `TokenUsage` gains
  `cache_write_input_tokens?`; `Interrupt` gains `subagent_run_id?`;
  `ResumeEntry` gains `metadata?`.
- `AgentSubscriber::on_tool_call_result` third argument: `&str` →
  `&ToolResultContent`.
- The 0.0.45 `THINKING`→`REASONING` rewrite is now a permanent raw-JSON boundary
  (`agui-rs-client/src/compat.rs`) rather than an opt-in middleware.

### Fixed — protobuf wire format

- Filled in `EventType` 19–30 and the `Event` oneof 22–31; the schema previously
  only covered up to 18/21.
- **Two wire bugs corrected.** `TextMessageChunk` and `ToolCallChunk` were
  emitting a `base.type` of `TextMessageStart` / `ToolCallStart` instead of
  their own chunk types. Seven `metadata` / `activity_content` fields were
  declared as `google.protobuf.Value` where upstream uses `Struct`, which made
  compliant peers **silently drop every metadata value**.
- `Usage` token fields: `uint64` → `int64`. `RunStartedEvent` fields 4/5/6
  (`parent_run_id`, `input`, `protocol_version`) were wrongly marked reserved
  and are live again. Added `RunFinishedEvent.pending_tool_call_ids = 8` and
  friends.

### Changed — encoder content negotiation

`Accept` selection now replicates upstream `media-type.ts` function by function
(replacing the previous `jshttp`/`negotiator` logic). **Behavioural change:**
`Accept: */*` and `Accept: application/*` now select **protobuf** instead of
falling back to SSE. `q=0` is honoured as a veto, and media-type specificity
outranks `q` (so `application/vnd.ag-ui.event+proto;q=0, */*` does *not* select
protobuf).

### Added — client behaviour

- An **enforcement** stage, schema-driven from
  `upstream-spec/schema.json`: unknown event types are dropped with a warning,
  unknown properties on known events are stripped with a warning carrying the
  path, and malformed values for protocol-described fields are fatal.
- `verify` gained reasoning double open/close discipline — the reasoning span and
  reasoning message are tracked as two independent sets, and `RUN_FINISHED`
  fails if either is non-empty.
- Outbound `protocolVersion` declaration (`PROTOCOL_VERSION = "1.0"`, gated by
  `max_protocol_version`) and a three-tier inbound warning arbitration.
- `MESSAGES_SNAPSHOT` now follows upstream HEAD semantics and gains
  `authoritative_activity_types`.
- `ActivityDelta`: a failed patch is now a warning, not fatal — the previous
  content is kept and `activityType` still advances.
- An expired interrupt is only allowed through if its resume entry is
  `cancelled`.
- `onInitialize` resume validation applies to `runAgent` only; `connectAgent` is
  exempt.

### Upgrading

- Replace `THINKING_*` events with the `REASONING_*` family; delete any
  `on_thinking_start` / `on_thinking_end` subscriber hooks in favour of the
  reasoning hooks.
- Rename `InputContent` → `ContentPart` (drop `Binary` handling) and
  `InputContentSource` → `PartSource`.
- Update `on_tool_call_result` handlers for `&ToolResultContent`.
- Add the `attributable` field to hand-written event literals; match the new
  `Success` struct form of `RunFinishedOutcome` (and handle `Cancelled`).
- Expect protobuf field-number changes and corrected chunk `base.type`; do not
  assume `metadata` is absent — it is now carried on the wire.
- Clients sending `Accept: */*` will now receive protobuf, not SSE. Send an
  explicit `Accept: text/event-stream` if you need SSE.
- Remove any use of `with_max_version`, `BackwardCompat0_0_*`, or the
  `LegacyThinking*` types.


## [0.1.3] - 2026-08-07

### Fixed (faithfulness, 2026-08-07)
- **Synced upstream monorepo `54f1341` → `27e5593a`.** No new stable npm
  release (still `0.0.57`); three client-side/core fixes ported from TS SDK
  main. Proto packages changed only by C#-only `csharp_namespace` options
  (no wire-format impact; not ported).
- **`TOOL_CALL_START` idempotency.** Applying a start event is now
  idempotent: the reducer dedupes the tool call id across all messages
  *before* resolving/creating the parent assistant message, so a replayed
  start (HITL `respond()` re-sync or dual-transport delivery) can no longer
  append a duplicate tool call or a stray empty assistant message when its
  `parentMessageId` is no longer in state. A start reusing an id under a
  different name updates the existing entry in place (`tracing::warn!`,
  mirroring TS `console.warn`) and never touches already-streamed
  `arguments`. Ported from TS commit `d6287260`.
- **`MESSAGES_SNAPSHOT` activity all-or-nothing.** When a snapshot carries
  any activity message, the backend is declaring the complete activity set:
  activity messages now follow the same source-of-truth replace semantics as
  reasoning (repeated entries replaced, omitted local entries dropped).
  Activity stays client-preserved only when the snapshot carries none.
  Ported from TS commits `ad24f70b` + `388e4c59`.
- **`parentMessageId: null` accepted on `TOOL_CALL_START`/`TOOL_CALL_CHUNK`.**
  Cross-language back-compat: producers that serialize the optional field as
  JSON `null` (e.g. the .NET Microsoft Agent Framework adapter) validate
  instead of aborting the run. Rust `Option` fields already normalize `null`
  to `None`; behaviour locked with tests. Ported from TS commit `2fa33a0e`.
- **10 new tests** mirroring the upstream TS cases: 4 tool-call idempotency,
  4 snapshot-activity, 2 core null-`parentMessageId` acceptance tests.

## [0.1.2] - 2026-06-24

### Fixed (faithfulness, 2026-06-24)
- **Synced upstream TS SDK 0.0.54 → 0.0.57.** Two client-side bug fixes ported
  from the TypeScript SDK; core/encoder/proto packages had zero code changes.
- **Tool-result ordering in event reducer.** When a `TOOL_CALL_RESULT` event
  arrives, the tool message is now inserted immediately after the owning
  assistant message (the one whose `tool_calls[].id` matches `tool_call_id`),
  skipping past any existing tool results for that assistant. Previously the
  tool message was appended to the end of `state.messages`, which could place
  it after trailing assistant text and violate the provider's message-ordering
  contract (tool result must immediately follow the assistant message that
  issued the tool call). Falls back to append when no owning assistant is
  found. Ported from TS commit `89a0c03` (0.0.55).
- **`MESSAGES_SNAPSHOT` reasoning dedup.** When a snapshot carries reasoning
  messages, streamed reasoning (with locally-generated ids) is now replaced by
  the snapshot's canonical copy instead of being preserved alongside it.
  Activity messages are always preserved. Local reasoning is preserved only
  when the snapshot does not contain reasoning. Ported from TS commit
  `5d9d1f2` (0.0.57).
- **5 new tests** mirroring the upstream TS test cases: tool-result ordering
  with trailing text, reasoning replacement, activity preservation, same-id
  reasoning update, and multi-turn reasoning convergence.

### Not ported (intentional)
- `0dc4c55` (0.0.57): HttpAgent default fetch binding — browser-only, N/A.
- `e395af5` + `748ad8c` (0.0.56): subscriber clone-cost / payloadExceeds /
  dev freeze guard — JS/browser-specific; Rust's ownership model and trait-based
  `AgentSubscriber` architecture already prevent these issues.

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
