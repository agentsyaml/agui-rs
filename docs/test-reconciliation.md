# TS ⇄ Rust Test Reconciliation

A per-file reconciliation of the canonical TypeScript test suite against this
SDK's tests. The goal is to make "are these really all the gaps?" **verifiable**
rather than asserted.

- **Reconciled against:** upstream `ag-ui-protocol/ag-ui` **1.0.0**, monorepo
  commit `024332cbb71e03e6a6bc055bed5af9c5c504471a` (2026-09-28), local
  checkout `/tmp/agui`.
- **Protocol ground truth:** this repo's byte-for-byte copy of the 1.0 spec —
  `crates/agui-rs-proto/upstream-spec/schema.json` + `proto-freeze.txt` +
  the three `.proto` files.
- **Last refreshed:** 2026-09-29. This is a **one-shot snapshot at one upstream
  commit**. Any upstream bump invalidates the counts below; re-run "How to
  reproduce" before trusting them.

Method: enumerate every TS `it()/test()` case per file, map each TS test file
to its Rust counterpart(s) (integration test + relevant `src/` unit tests), and
compare `TsCases` against `RustCases`. Any file where Rust coverage is below the
TS count is inspected and classified.

## Counting conventions (口径)

Read this before comparing any two numbers in this document.

**TS side — "case" = one `it(` / `test(` call site.**

```
rg -c "\b(it|test)\s*\(" <file>
```

- `it.each(...)` is **not** matched (the regex requires `(` right after the
  keyword) and counts as **1**. There are 49 `it.each` blocks in scope
  (core 7, client 35, encoder 1, proto 10); the "incl. `it.each`" column below
  counts each as its own case. Both are given.
- `describe()` blocks are not cases.
- This is the same regex the pre-1.0 revision of this document used, so the two
  revisions are method-comparable even though the totals are not.

**Rust side — two different numbers, both real, do not mix them.**

| Number | Meaning | Value |
| ------ | ------- | ----- |
| `cargo test` executed | what CI reports | **1326 passed / 0 failed** |
| distinct test fns | `#[test]` / `#[tokio::test]` defined once in source | **637** |

The gap is **689 duplicate executions**, entirely inside `agui-rs-client`.

### The `#[path]` duplication problem

`crates/agui-rs-client/tests/middleware_*.rs`, `subscriber.rs` and
`legacy_bridged.rs` pull lib modules into integration targets with
`#[path = "../src/…"] mod …;`. That re-runs the lib's `#[cfg(test)]` unit tests
inside each integration binary. Measured, per target:

| Integration target | executed | own | duplicated |
| ------------------ | ------- | --- | ---------- |
| `middleware_chained_integration` | 157 | 17 | 140 |
| `middleware_chained_run_next_with_state` | 141 | 1 | 140 |
| `middleware_core` | 141 | 1 | 140 |
| `middleware_function` | 141 | 1 | 140 |
| `middleware_with_state` | 90 | 1 | 89 |
| `subscriber` | 16 | 3 | 13 |
| `middleware_filter_tool_calls` | 13 | 2 | 11 |
| `middleware_live_events` | 12 | 1 | 11 |
| `legacy_bridged` | 9 | 4 | 5 |
| **total** | **720** | **31** | **689** |

**This document uses the distinct-test-fn count (637) for every per-file Rust
figure**, and quotes 1326 only as the `cargo test` headline. A raw
`rg -c '#\[test\]' crates/` sweep (608) *under*-counts by 29 because
`agui-rs-core/src/event_factories.rs` generates 30 tests from one `#[test]`
inside a `factory_test!` macro; 608 − 1 + 30 = 637.

## Totals

### TS 1.0.0 — in scope

Counted with the original regex; "incl. each" adds `it.each` as its own case.

| TS package | `.test.ts` files | cases | incl. `it.each` | Rust crate | Rust test fns | `cargo test` executed |
| ---------- | ---------------- | ----- | --------------- | ---------- | ------------- | --------------------- |
| `core` | 23 | 213 | 219 | `agui-rs-core` | 163 | 163 |
| `client` | 94 | 938 | 971 | `agui-rs-client` | 407 | 1096 |
| `encoder` | 2 | 5 | 6 | `agui-rs-encoder` | 22 | 22 |
| `proto` | 13 | 155 | 164 | `agui-rs-proto` | 31 | 31 |
| **total** | **132** | **1311** | **1360** | (+ `agui-rs-server`) | **637** (+14) | **1326** |

`agui-rs-server` (14 tests) is Rust-only — upstream has no server package.

**Out of scope:** `cli` (2 files, 5 cases) and `a2ui-toolkit` (3 files, 112
cases). Neither has a Rust counterpart. All 6 packages = 137 `.test.ts` files;
the 5 excluded files are exactly this delta.

## How to reproduce

Every command below was run and produced the number quoted above.

1. **TS cases per file** (the counting convention, verbatim):
   ```sh
   cd /tmp/agui/sdks/typescript/packages
   for p in core client encoder proto; do
     find "$p" \( -name "*.test.ts" -o -name "*.spec.ts" \) -not -path "*/node_modules/*" -print0 \
       | xargs -0 rg -c --no-filename "\b(it|test)\s*\(" \
       | awk -v p="$p" '{s+=$1} END{printf "%-8s %d\n", p, s}'
   done
   ```
2. **TS `it.each` blocks** (the "incl. each" column): same loop with
   `rg -c --no-filename "^\s*(it|test)(\.\w+)?\s*\("`.
3. **Rust `cargo test` total**:
   ```sh
   cargo test --workspace 2>&1 \
     | rg -o 'test result: ok\. \d+ passed' | rg -o '\d+' | awk '{s+=$1}END{print s}'
   # → 1326
   ```
4. **Rust distinct test fns per crate**:
   ```sh
   for c in agui-rs-core agui-rs-client agui-rs-encoder agui-rs-proto agui-rs-server; do
     printf '%-20s ' "$c"
     rg -c --no-filename '#\[(tokio::)?test\]' "crates/$c/src" "crates/$c/tests" \
       | awk '{s+=$1}END{print s}'
   done
   ```
   For `agui-rs-core` add the macro delta: `rg -c '#\[test\]' crates/agui-rs-core/src`
   = 65, of which one is the `factory_test!` body expanding 30 times →
   `65 − 1 + 30 = 94` lib tests, `+ 69` integration = **163**.
5. **The `#[path]` duplication table**: `cargo test --workspace 2>&1` and pair
   each `Running tests/<name>.rs` line with its `test result:` line, then
   subtract `rg -c '#\[(tokio::)?test\]' crates/agui-rs-client/tests/<name>.rs`.

## Files where Rust coverage < TS case count — classified

Rust counts are **distinct test fns** in the mapped units (a unit whose
`cargo test` count differs is footnoted). Classification vocabulary is
`ported` / `partial` / `skipped` / `TS-only`.

| TS file | TS | Rust (unit, fns) | Class | Basis |
| ------- | -- | ---------------- | ----- | ----- |
| `verify/subagent-verify` | 79 | `client/src/verify.rs` 35 | **partial** | `verify.rs:145` treats all three `Subagent*` events as pass-through (explicit `ponytail:` comment). None of the 79 owner-tracking rules — duplicate `SUBAGENT_STARTED`, `parentSubagentRunId` not started, owner mismatch on `TOOL_CALL_ARGS` / encrypted values, `RUN_FINISHED` with an open subagent — is implemented. **Largest single gap.** |
| `chunks/chunk-lanes` | 39 | `client/src/chunks.rs` 10 + `tests/chunks_transform.rs` 22 | **partial** | `chunks.rs` contains **zero** `subagent_run_id` references; TS `chunks/transform.ts` uses it 57×. No lane routing, no per-subagent close. |
| `chunks/subagent-chunks` | 15 | same 32 | **partial** | Same root cause: `subagentRunId` propagation through expansion is absent. |
| `agent/subscriber` | 43 | `src/subscriber.rs` 13 + `tests/subscriber.rs` 3 + `tests/subscriber_registry.rs` 6 = 22 | **partial** | Registry, ordering, temporary subscribers and replacement chaining are done. The JS `AgentStateMutation` + `stopPropagation` contract is not adopted — Rust hooks return `Result` / `Option<replacement>`. |
| `enforce/enforce` | 19 | `client/src/enforce.rs` 5 | **partial** | `enforce_event` drops unknown types, strips unknown own-keys and defers malformed known fields to serde. The 19 TS cases also cover union-slot wrong types, `stripUnknown` reporting, and the zod-generated-schema surface. |
| `enforce/strip` | 11 | `enforce.rs` 5 | **partial** | **The table is the event's own keys only.** Upstream descends into `messages`, `tools` and the RFC 6902 `patch` array; that needs a general JSON Schema evaluator, which `enforce.rs:29-33` explicitly declines. See *Residual gaps*. |
| `enforce/cross-zod-copy` | 4 | 0 | **TS-only** | Asserts `stripUnknown` behaves identically across two installed copies of zod. No Rust analogue. |
| `enforce/literal-value-across-zod-3-25` | 2 | 0 | **TS-only** | Pinned zod 3.25 literal handling. |
| `enforce/optional-null` | 1 | 0 | **TS-only** | Tests `enforceOutgoingInput` (outgoing `RunAgentInput` null omission). No Rust counterpart — `rg enforce_outgoing crates/` returns nothing. |
| `transform/transport-parity` | 20 | 0 | **TS-only** | Differential test running the *same* corpus through the TS SSE and protobuf builds and asserting identical outcomes. Needs two independent implementations in one process; Rust has one. |
| `apply/default.metadata` | 18 | `client/src/apply.rs` 33 † | **partial** | Rust applies no `mergeMetadata` on event metadata. `apply.rs` only *reads* `@ag-ui/client` metadata for activity authority (`:355`); nothing merges `event.metadata` onto the target message. †33 is the whole module, not this file's share. |
| `apply/default.activity` | 37 | `src/apply.rs` 33 + `core/tests/activity_events.rs` 4 | **ported** | Snapshot/delta/`replace:false`/RFC 6902 patch/stale-patch/authority-declaration all present (`apply.rs:823-978`, `:1599-1856`). |
| `apply/default.reasoning` | 28 | `src/apply.rs` 33 † | **ported** | Full reasoning lifecycle + `REASONING_ENCRYPTED_VALUE` → `encryptedValue` on tool-call / message / reasoning message. †module-wide. |
| `agent/protocol-version` | 13 | `src/version.rs` 3 + `src/compat.rs` 3 = 6 | **partial** | Comparator + grammar + producer-declaration warning + THINKING→REASONING boundary all present. The in-band `RUN_STARTED` echo version and the deprecated `maxVersion` alias cycle guard are not. |
| `agent/agent-peer-ceiling` | 5 | 0 | **partial** | `AgentRunner::with_max_protocol_version` exists (`agent.rs:332`) and the deprecation note is wired (`:341`). What is missing is the JS-specific defect detection: reading `maxVersion` / `maxProtocolVersion` off the *instance* rather than the prototype. JS-runtime only. |
| `agent/agent-pending-tool-calls` | 7 | 0 | **partial** | Unanswered-tool-call tallying (derive from stream, trust producer's list, fresh tally per run, not reported on interrupt) is not implemented as such; `RunFinishedOutcome::pending_tool_calls` is carried but never tallied. |
| `agent/agent-detach` | 2 | 0 | **skipped** | RxJS `takeUntil(activeRunDetach$)` background-run detachment. No `futures::Stream` analogue. `// SKIPPED:` marker at `tests/agent_result.rs:377`. |
| `agent/subscriber-errors` | 3 | `src/subscriber.rs` 13 † | **ported** | Hook errors surface as `Err` rather than a thrown exception. †module-wide. |
| `activity-history` | 4 | `src/apply.rs` 1 (`authoritative_activity_types` path) | **partial** | `authoritativeActivityTypes` is ported and tested (`apply.rs:978`). `withAuthoritativeActivityTypes` — the projector-scope union that stamps `@ag-ui/client` onto an unmarked transcript — is not (`rg with_authoritative crates/` → nothing). |
| `apply/run-started-input` | 7 | `src/agent.rs` + `src/apply.rs` | **ported** | Input-list seeding and echo handled. |
| `conformance/streams` | 2 | `src/transform.rs` 3 | **ported** | |
| `core/token-usage` | 22 | `core/src/events.rs` 1 (`token_usage_all_fields_round_trip`) | **partial** | The `TokenUsage` *type* round-trips. The three exported helpers TS tests here — `tokenUsageFromAiSdkUsage`, `tokenUsageFromLangChainMetadata`, `aggregateTokenUsage` — have no Rust counterpart (`rg -i 'langchain\|aggregate' crates/` → nothing). |
| `core/token-usage-warnings` | 7 | 0 | **skipped** | Dev-time warnings for a non-numeric / NaN / negative count. Rust has no token-usage coercion layer to warn in. |
| `core/metadata` | 17 | `core/src/types.rs` 15 † | **partial** | Metadata is carried and round-trips on every message kind. `mergeMetadata` (key-replace-outright, never recursive, never mutating) is **not ported** — `rg merge_metadata crates/` → nothing. †`types.rs` is module-wide. |
| `core/event-factories` | 27 | `src/event_factories.rs` **35** + `tests/event_factories.rs` 7 = 42 | **ported** | Note the count inverts here: the `factory_test!` macro expands 30 single-line cases. |
| `core/main-entry-zod-free` | 5 | 0 | **TS-only** | Asserts the published bundle contains no zod import. Build-pipeline property, not runtime. |
| `core/index` | 1 | 0 | **TS-only** | Package smoke test. `// SKIPPED:` marker at `tests/index.rs:1`. |
| `apply/esm-interop` | 1 | 0 | **TS-only** | JS module-interop smoke test. |

† = the unit's total test-fn count, which covers several TS files. Quoted as an
upper bound for that TS file, not as a per-file attribution.

## Gaps closed since the 0.0.57 baseline

1. **`agui-rs-proto` crate** — hand-written `prost` messages, no `protoc`
   build dependency. Full 1.0 schema (31 events), mechanically drift-checked
   against the vendored `upstream-spec/` `.proto` files. 31 test fns. This
   replaced the previous state where `encode` rejected reasoning / activity /
   thinking events as `Unsupported`.
2. **The always-on compatibility boundary** — `client/src/compat.rs` maps the
   retired THINKING_* family onto REASONING_* on raw JSON before typed
   deserialization. Upstream 1.0 has no `BackwardCompatibility_0_0_{39,45,47}`
   middleware; those were removed and replaced by this permanent boundary
   (6 test fns across `compat.rs` + `tests/compat_boundary_inbound.rs`).
3. **Protocol version comparator** — `client/src/version.rs`
   (`compare_declared_protocol`, `compare_versions`, published-grammar check,
   producer-declaration warning). 3 test fns.
4. **Activity-history authority** — `apply.rs::authoritative_activity_types`
   plus the `MESSAGES_SNAPSHOT` activity-preservation rules. 8 test fns.
5. **`enforce` stage** — `client/src/enforce.rs` reads the frozen 1.0
   `schema.json` at compile time to build its known-shape table; no
   hand-maintained duplicate. 5 test fns.
6. **Encoder content negotiation** — `EventEncoder::with_accept` rewritten to
   mirror `media-type.ts` (`*/*` and `application/*` select protobuf, `q=0`
   vetoes, specificity outranks `q`). 8 test fns.

## Confirmed-equivalent buckets (no action)

Rust mapped-unit count ≥ TS count, inspected and left alone:

- **core:** `activity-events`, `backwards-compatibility`, `capabilities-interrupts`,
  `compat-types`, `events-role-defaults`, `interrupts`, `multimodal-messages`,
  `run-agent-input-lists`, `run-event-usage`, `run-finished-event`,
  `serialization`, `subagent-attribution`, `subagent-events`,
  `subagent-lifecycle-events`, `tool-call-events`, `tool-result-content`.
- **client:** `apply/default.state`, `apply/default.text-message`,
  `apply/default.tool-calls`, `apply/apply-debug`, `agent/agent-clone`,
  `agent/agent-concurrent`, `agent/agent-debug`, `agent/agent-multiple-runs`,
  `agent/agent-mutations`, `agent/agent-result`, `agent/agent-text-roles`,
  `agent/agent-version`, `agent/http`, `agent/http-fetch-binding`,
  `agent/legacy-bridged`, `chunks/expansion-never-repairs`,
  `chunks/transform-roles`, `compact/compact`, `compact/compact.state-window`,
  `interrupts/helpers`, `interrupts-lifecycle`, `legacy/convert.*`,
  `middleware/filter-tool-calls*`, `middleware/middleware*`,
  `middleware/function-middleware`, `run/http-request`, `transform/http*`,
  `transform/proto*`, `transform/sse`, `verify/verify.*` (except
  `subagent-verify`), `*debug` (documented logger divergence below).
- **encoder:** `encoder/encoder`, `encoder/null-omission`.
- **proto:** all 13 files.

## Residual gaps (the honest list)

Verified against the 1.0.0 checkout. Ordered by size.

1. **Subagent lifecycle verification — 79 TS cases, 0 implemented.**
   `client/src/verify/verify.ts` maintains `owners.{message,reasoning,toolCall,activity}`
   keyed by `subagentRunId`, seeded from the `RUN_STARTED` input echo and
   re-seeded by `MESSAGES_SNAPSHOT`. Rust `verify.rs:145-149` passes all three
   `Subagent*` events through with a `ponytail:` comment. Every owner-mismatch
   rejection in `subagent-verify.test.ts` is therefore absent. **This is a real
   behavioural gap, not an architectural divergence** — unlike the two items
   the previous revision of this document called out.
2. **Subagent chunk lanes — 54 TS cases (`chunk-lanes` 39 + `subagent-chunks`
   15), 0 implemented.** `chunks/transform.ts` reads `subagentRunId` 57 times;
   `chunks.rs` reads it zero times. Concurrent subagents' interleaved chunks
   are not routed to separate lanes and a chunk cannot close a *different*
   subagent's pending message.
3. **Enforcement does not recurse.** `enforce_event` strips unknown keys at the
   event's own level only. Upstream `stripUnknown` descends into `messages`,
   `tools` and the RFC 6902 `patch` array, so an unknown member nested one
   level down is dropped upstream but **silently ignored by serde** here.
   Declined at `enforce.rs:29-33`; a fix needs a general JSON Schema evaluator.
4. **`REASONING_ENCRYPTED_VALUE` owner check absent in `verify`.** TS
   `verify.ts:773+` resolves `entityId` against `owners.toolCall` (subtype
   `tool-call`) or `owners.{message,reasoning}` (subtype `message`) and
   rejects a value that disagrees with the opener. Rust `verify.rs` has no arm
   for the event at all — it falls through to `_ => Ok(())` (`:150`).
5. **Enforcement runs at a different pipeline position.** Upstream composes
   `enforceEvents(...)` **after** the middleware chain, before `transformChunks`
   (`agent.ts:368`, with the reasoning in the adjacent comment). Rust applies
   it inside `transform.rs::parse_sse_stream` / `parse_proto_stream` — the
   innermost wire layer, before any middleware can see a raw event. Same
   enforcement, different position; a middleware that expects to normalise a
   shape before validation cannot.
6. **`mergeMetadata` not ported.** `core/src/metadata.ts` exports it and
   `apply/default.ts:131` calls it on every event to merge `event.metadata`
   onto the target message. No Rust equivalent (`rg merge_metadata crates/` →
   nothing), so event-level metadata never reaches the message list.
7. **Token-usage helpers not ported.** `tokenUsageFromAiSdkUsage`,
   `tokenUsageFromLangChainMetadata` and `aggregateTokenUsage` are exported
   from `@ag-ui/core` and tested across 29 TS cases. Rust only models the
   `TokenUsage` *type*.
8. **Activity-history projector scope.** `withAuthoritativeActivityTypes` (the
   scope union that stamps `@ag-ui/client` onto an unmarked transcript) is not
   ported; only the read side is.
9. **Peer-ceiling defect detection.** Rust has the ceiling and the deprecation
   note but not the JS instance-field override diagnostic (5 TS cases,
   JS-runtime only).
10. **Subscriber mutation model** — `stopPropagation` + `AgentStateMutation`
    chaining. Rust hooks return `Result` / `Option<replacement>`. Registry,
    ordering, temporary subscribers and replacement chaining *are* implemented;
    only the JS mutation-object contract is intentionally not adopted.
11. **`events$` replay subject / `detachActiveRun()`** — RxJS-specific; no
    `futures::Stream` analogue. Cancellation is covered by `AbortHandle`.
12. **Per-stream-stage `DebugLogger` logging** — `[VERIFY]`/`[SSE]`/
    `[TRANSFORM]`/`[CHUNK]` console capture. Lifecycle logging
    (    `AgentConfig::debug`) is done; 39 `// SKIPPED:` markers across
    `chunks_transform_debug.rs` (10), `verify_debug.rs` (7),
    `transform_debug.rs` (7), `subscriber.rs` (3), `agent_debug.rs`,
    `agent_http.rs`, `agent_lifecycle.rs`, `agent_result.rs`,
    `agent_concurrent.rs`, `transform_http.rs`, `legacy_bridged.rs`
    (2 each) record it.
13. **JS-runtime / zod-runtime-only cases** — frozen inputs, `process
    undefined`, ESM interop, bundle-has-no-zod, two-zod-copies,
    zod-3.25 literals, and the 20-case SSE-vs-protobuf transport-parity
    differential.
14. **The compatibility boundary's OUTBOUND half.** Upstream
    `CompatibilityBoundary.run` rewrites the `RunAgentInput` it is about to
    send (`compatibility-boundary.ts:177`, `input.messages =
    input.messages.map(upgradeMessageContent)`) and the module doc notes
    *"Outgoing legacy binary attachments are also upgraded before the transport
    validates or sends them"*. Rust `compat.rs` only translates INBOUND raw
    events. An outgoing legacy binary attachment is not upgraded on the way
    out. In practice a Rust caller cannot build one: `ContentPart` has no
    `Binary` variant, and the type stays a `Json` value in `RunAgentInput`,
    so the shape has to be hand-written JSON. The inbound direction — the one
    that killed a run outright — is implemented.
15. **`normalizeLegacyRunAgentInput` for non-event request parsing.** Upstream
    marks it `@internal` and states the scope itself
    (`compatibility-boundary.ts:71-78`): *"Used internally for
    `RUN_STARTED.input`; direct server request parsers do not pass through this
    event boundary and must handle compatibility locally."* Rust
    `agui-rs-server` parses requests straight from the wire, so it is the
    "handle compatibility locally" case, not a client gap. `RUN_STARTED.input`
    is covered.

Everything else is implemented. Each remaining item is recorded in
`docs/typescript-alignment.md` §4 (intentional divergences) and, where a
per-case stub exists, as `// SKIPPED:` markers in the Rust tests (48 markers
total: 45 in `agui-rs-client`, 3 in `agui-rs-core`).

## `[未核实]`

- **Per-case mapping inside each file.** This reconciliation compares *counts*
  per file and then reads the test names on both sides to classify. It is not a
  case-by-case diff. Items 1–3 above were confirmed by reading the TS
  `describe`/`it` titles against the Rust module, not by walking all 79 / 54
  cases individually. A file listed as **ported** has a Rust unit whose count
  meets or exceeds the TS count *and* whose test names were sampled to match —
  it has not been proven exhaustively equivalent.
- **Whether any Rust test asserts something the TS suite contradicts.** Not
  checked; the audit is one-directional (TS → Rust).
- **`a2ui-toolkit` (112 cases) and `cli` (5 cases).** Excluded on the grounds
  that no Rust crate exists for either. If that exclusion is wrong, the gap
  table is incomplete for those 117 cases.
