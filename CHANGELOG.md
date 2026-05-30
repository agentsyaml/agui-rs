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

## [Unreleased]

### Added
- `AgentRunner::pending_interrupts()` and `with_now_fn()` — the stateful runner
  now tracks unresolved interrupts emitted by a `RUN_FINISHED` interrupt
  outcome, matching TypeScript `AbstractAgent.pendingInterrupts`.
- `interrupts::ensure_resume_covers()` and `interrupts::interrupt_is_expired()`
  helpers, ported from TS `AbstractAgent.onInitialize` enforcement and
  `isInterruptExpired`.
- `docs/typescript-alignment.md`: full field-by-field alignment audit against
  TypeScript SDK `0.0.54`.

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

### Added (docs/tooling)
- `docs/test-reconciliation.md`: per-file, per-case TS⇄Rust reconciliation with
  classification of every file where Rust coverage is below the TS case count.
- `scripts/reconcile.ps1` + `scripts/titles.ps1`: reproducible tooling that
  enumerates TS cases and maps them to Rust ported/skipped counts.

### Notes
- `agui-rs-core` / `agui-rs-client` remain free of any date dependency; interrupt
  expiry uses an injectable clock (`AgentRunner::with_now_fn`), defaulting to a
  never-expire clock for deterministic, opt-in behaviour.
- Ported tests now passing (previously `// SKIPPED:`):
  `verify/__tests__/verify.multiple-runs.test.ts` (6),
  `verify/__tests__/verify.lifecycle.test.ts` (2),
  `agent/__tests__/interrupts-lifecycle.test.ts` (6),
  `core/__tests__/capabilities-interrupts.test.ts` (rewritten for canonical shape),
  `compact/__tests__/compact.test.ts` "State Compaction" (13).
- Full per-file test reconciliation recorded in `docs/test-reconciliation.md`
  (568 TS cases in scope; 484 ported, 76 explicit `SKIPPED` markers; every
  below-coverage file classified).
- Verified: `cargo build --workspace --examples`, `cargo clippy --workspace
  --all-targets` (zero warnings), `cargo test --workspace` (1223 tests passing).

## [0.1.0] - 2026-05-29

### Added
- Initial public release on crates.io.
- `agui-rs-core`: protocol types, 33 event variants, factory helpers.
- `agui-rs-encoder`: SSE encoder + protobuf media-type negotiation surface.
- `agui-rs-client`: `HttpAgent`, `AgentRunner`, subscriber hooks, middleware chain.
- `agui-rs-server`: `axum` route builder, `RunHandler`, channel `EventEmitter`.
- `agui-rs`: facade crate with `core` / `encoder` / `client` / `server` / `full` features.

[0.1.0]: https://github.com/agentsyaml/agui-rs/releases/tag/v0.1.0
