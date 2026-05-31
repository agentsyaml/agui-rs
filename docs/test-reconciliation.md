# TS ⇄ Rust Test Reconciliation

A per-file, per-case reconciliation of the canonical TypeScript test suite
(`sdks/typescript/packages/{core,client,encoder}` at `0.0.54`, commit
`f30021b9`) against this SDK's tests. The goal is to make "are these really all
the gaps?" **verifiable** rather than asserted.

Method: enumerate every TS `it()/test()` case per file and map each TS test file
to its Rust counterpart(s) (integration test + relevant `src/` unit tests),
comparing `TsCases` against `Ported + Skipped`. Any file where Rust coverage is
below the TS count is then inspected case-by-case and classified below.

- TS packages in scope: **core, client, encoder** (`proto`, `a2ui-toolkit`,
  `cli` are out of scope — `proto` is the stubbed protobuf impl).
- TS cases in scope: **568**. Rust ported: **499**. Rust SKIPPED markers: **76**.
- Last refreshed: 2026-05-30 (after closing capabilities, verify multi-run,
  compact state-compaction, multi-subscriber registry, and agent mutator gaps).

## How to reproduce

1. Count TS cases per file: `rg -c "\b(it|test)\s*\(" <ts-test-file>` (or
   enumerate titles with `rg "\b(it|test)\s*\(" -o`).
2. Count Rust coverage per file: `rg -c "#\[(tokio::)?test\]"` for ported tests
   and `rg -c "// SKIPPED:"` for explicitly-skipped cases in the corresponding
   `tests/*.rs` / `src/*.rs`.
3. For any file where ported + skipped < TS cases, compare case titles and
   classify (table below).

## Files where Rust coverage < TS case count — classified

Every file below was inspected case-by-case. "Behaviour present?" answers
whether the *production behaviour* exists in Rust regardless of test count.

| TS file | TS | Rust (port+skip) | Behaviour present? | Classification |
| ------- | -- | ---------------- | ------------------ | -------------- |
| `agent/subscriber` | 43 | 25 | Partial | **Architectural divergence (narrowed 2026-05-30).** `subscribe`/`unsubscribe`, multi-subscriber registration + ordering, and temporary per-run subscribers are now implemented (`tests/subscriber_registry.rs`). The remaining shortfall is `AgentStateMutation` + `stopPropagation` chaining, dev-mode frozen-input checks, and `runSubscribersWithMutation`/`process undefined` — Rust uses return-based hooks, not mutation objects. |
| `client/debug-logger` | 29 | 11 | Partial | **Lifecycle logging implemented 2026-05-30** (`AgentConfig::debug` + runner lifecycle logs). Per-stream-stage `console.debug` capture cases (`[VERIFY]`/`[SSE]`/`[TRANSFORM]`/`[CHUNK]`) remain a documented divergence — pure stream fns take no logger param. |
| `agent/agent-debug` | 12 | 8 | Partial | **`debug` config implemented 2026-05-30** (`tests/agent_lifecycle.rs`). Remaining cases assert stderr capture / per-stage prefixes (no `console.debug` equivalent). |
| `agent/agent-result` | 21 | 7 | Partial | Result/newMessages tracking is present and tested. The remaining shortfall is `detachActiveRun()` (4 cases) — no Rust equivalent (RxJS background-run detachment; cancellation is covered by `AbortHandle`). |
| `agent/agent-clone` | 2 | 1 | Yes | **Gap closed 2026-05-30.** `AgentRunner::clone_runner` (`tests/agent_lifecycle.rs`). The HttpAgent AbortController-clone case is N/A (per-run `AbortHandle`). |
| `agent/agent-version` | 1 | 1 | Yes | **Gap closed 2026-05-30.** `AgentRunner::with_max_version` (`tests/middleware_auto_insertion.rs`). |
| `agent/agent-mutations` | 11 | 11 | Yes | **Gap closed 2026-05-30.** `add_message`/`add_messages`/`set_messages`/`set_state` implemented on `AgentRunner` with subscriber notifications (`tests/agent_mutators.rs`, 9 cases covering all 11 TS cases incl. registration order + multi-subscriber). |
| `agent/agent-concurrent` | 7 | 4 | Yes | Concurrency is exercised through the apply/verify pipeline tests; remaining TS cases are finer-grained pipeline permutations of the same behaviour. |
| `apply/default.reasoning` | 28 | 17 | Yes | All reasoning behaviour (full lifecycle, `REASONING_ENCRYPTED_VALUE` → `encryptedValue` on tool-call/message, non-existent-id warnings, activity rejection) is implemented and tested. The 11-case shortfall is entirely `stopPropagation`/mutation-object variants. |
| `middleware/backward-compatibility-0-0-{39,45,47}` | 32 | ~18+auto | Yes | Transform logic implemented and tested; auto-insertion now via `with_max_version` (`tests/middleware_auto_insertion.rs`, 2026-05-30). Remaining shortfall = JS `process undefined` warning suppression. |
| `core/event-factories` | 25 | 14 | Yes | All factory constructors exist (`factory::*` + `event_factories::create_*`). TS splits each event family into separate cases; Rust groups them. No missing factory. |
| `core/multimodal-messages` | 14 | 9 | Yes | All multimodal parse/validation behaviour present (data/url/binary sources, mimeType requirements, all-modalities). TS has extra negative-parse permutations covered by fewer Rust cases. |
| `verify/verify.events` | 13 | 8 | Yes | All ID-matching / context rules implemented; Rust consolidates several TS cases per test (also covered by 36 `verify.rs` unit tests). |
| `verify/verify.text-messages` | 13 | 11 | Yes | Same — text-message ordering rules fully implemented. |
| `compact/compact` | 25 | 22 | Yes | **Gap closed 2026-05-30** (state compaction added). **2026-05-31:** reasoning de-compaction — TS `compactEvents` only compacts text/tool/state, so reasoning now passes through unchanged. Remaining −3 are message/tool-call permutations merged into equivalent Rust tests. |
| `transform/proto` | 6 | 6 | Yes | **Gap closed 2026-05-30.** Protobuf decode via `agui-rs-proto` + `parse_proto_stream` (`tests/transform_proto.rs`). |
| `apply/esm-interop` | 1 | 0 | N/A | JS/ESM module-interop smoke test; not meaningful in Rust. |

## Gaps closed during the reconciliation effort

1. **`compact_events` state compaction** — silently dropped `STATE_SNAPSHOT` /
   `STATE_DELTA` per-run reduction; a true blind spot (no `// SKIPPED:` marker).
   Implemented + 13 ported tests.
2. **Multi-subscriber registry** — `subscribe()`/`unsubscribe()`, registration
   order, temporary per-run subscribers. 6 ported tests.
3. **Agent mutator API** — `add_message`/`add_messages`/`set_messages`/`set_state`.
   9 ported tests.
4. **protobuf encode/decode** — new `agui-rs-proto` crate (hand-written `prost`,
   no `protoc`); wired into encoder + client transform. 6 + 9 tests.
5. **`connect()` / `connectAgent()`, `getCapabilities()`, `clone()`** — agent
   lifecycle surface. 8 ported tests.
6. **`maxVersion` + version-gated backward-compat middleware auto-insertion** —
   `with_max_version`. 5 tests.
7. **`DebugLogger` lifecycle logging** — `AgentConfig::debug` + runner logs.

(Earlier: `AgentCapabilities` schema realignment, `verify_events` multi-run
support, `pendingInterrupts` enforcement.)

## Confirmed-equivalent buckets (no action)

These TS files have Rust coverage ≥ TS count and need no further review:
all `verify.*` (except the two consolidated above), `transform.sse`,
`chunks.transform`, `legacy.*`, `interrupts.helpers`, `middleware-chained-*`,
`core/activity-events`, `core/interrupts`, `core/events-role-defaults`,
`core/run-finished-event`, `core/backwards-compatibility`, `encoder`.

## Residual gaps (the honest list)

After the case-by-case audit, the remaining divergences are **architectural or
explicitly-deferred**, not silent protocol gaps:

1. **Subscriber mutation model** — `stopPropagation` + `AgentStateMutation`
   chaining. Rust subscriber hooks return `Result`/`Option<replacement>`, not
   mutation objects. (Multi-subscriber registry, ordering, temporary
   subscribers, and replacement chaining are implemented — only the
   JS mutation-object + `stopPropagation` contract is intentionally not adopted.)
2. **`events$` replay subject / `detachActiveRun()`** — RxJS-specific; no
   `futures::Stream` analogue. Cancellation is covered by `AbortHandle`.
3. **Per-stream-stage `DebugLogger` logging** — `[VERIFY]`/`[SSE]`/`[TRANSFORM]`/
   `[CHUNK]` console capture. Lifecycle logging (`AgentConfig::debug`) is done;
   stage logging would use `tracing`.
4. **protobuf reasoning/activity/thinking events** — not in the canonical
   `events.proto` (18 variants); `encode` rejects them with `Unsupported`.
5. **JS-runtime-only cases** — frozen inputs, `process undefined`, ESM interop.

Everything else is implemented. Each remaining item is recorded in
`docs/typescript-alignment.md` §4 (intentional divergences) and, where a
per-case stub exists, as `// SKIPPED:` markers in the Rust tests.

### Closed this round (previously residual)

`connect()`/`connectAgent()`, `getCapabilities()`, `clone()`, `maxVersion` +
version-gated middleware auto-insertion, agent mutator API, multi-subscriber
registry, protobuf encode/decode, and `DebugLogger` lifecycle logging are all
now implemented and tested.
