// SKIPPED: no console.debug calls when debugLogger is undefined: Rust expand_chunks has no debug-logger parameter or console hook.
// SKIPPED: no console.debug calls when debugLogger is false: Rust expand_chunks has no debug-logger parameter or console hook.
// SKIPPED: no console.debug calls when debugLogger is null: Rust expand_chunks has no debug-logger parameter or console hook.
// SKIPPED: TEXT_MESSAGE_CHUNK produces debug logs for TEXT_MESSAGE_START, TEXT_MESSAGE_CONTENT, TEXT_MESSAGE_END with full JSON payloads: Rust chunk expansion emits events only and does not expose transform logging.
// SKIPPED: TOOL_CALL_CHUNK produces debug logs for TOOL_CALL_START, TOOL_CALL_ARGS, TOOL_CALL_END with full payloads: Rust chunk expansion emits events only and does not expose transform logging.
// SKIPPED: REASONING_MESSAGE_CHUNK produces corresponding debug logs: Rust chunk expansion emits events only and does not expose transform logging.
// SKIPPED: TEXT_MESSAGE_CHUNK logs include summary with messageId: Rust chunk expansion emits events only and does not expose summary-mode logging.
// SKIPPED: TOOL_CALL_CHUNK logs include summary with toolCallId/toolCallName: Rust chunk expansion emits events only and does not expose summary-mode logging.
// SKIPPED: REASONING_MESSAGE_CHUNK logs include summary with messageId: Rust chunk expansion emits events only and does not expose summary-mode logging.
// SKIPPED: uses [TRANSFORM] prefix: Rust chunk expansion does not emit prefixed debug output.
