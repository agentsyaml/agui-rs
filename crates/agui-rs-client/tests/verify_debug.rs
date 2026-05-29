// SKIPPED: no console.debug calls when debugLogger is undefined: ts uses VITEST debug log capture, no Rust equivalent
// SKIPPED: no console.debug calls when debugLogger is false: verify_events has no public debug logger parameter in Rust
// SKIPPED: no console.debug calls when debugLogger is null: verify_events has no public debug logger parameter in Rust
// SKIPPED: logs full JSON of each event: ts debug logger integration is not exposed by Rust verify_events
// SKIPPED: logs only { type } summary: ts debug logger integration is not exposed by Rust verify_events
// SKIPPED: uses [VERIFY] prefix: ts debug logger integration is not exposed by Rust verify_events
// SKIPPED: each event in the sequence produces exactly one debug log: ts debug logger integration is not exposed by Rust verify_events
