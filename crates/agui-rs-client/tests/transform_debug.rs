// SKIPPED: no debug logs when logger is undefined: Rust parse_sse_stream exposes no logger parameter and debug logging is not wired into transform parsing.
// SKIPPED: with events+verbose: logs full JSON of each parsed SSE event with [SSE] prefix: Rust DebugLogger is environment-gated and not integrated with parse_sse_stream.
// SKIPPED: with events only (no verbose): logs { type } summary: Rust DebugLogger has no verbose/events mode split and is not integrated with parse_sse_stream.
// SKIPPED: no debug logs when logger is undefined: Rust HttpAgent/transform pipeline exposes no logger parameter and debug logging is not wired into HTTP stream transformation.
// SKIPPED: lifecycle log: [HTTP] Stream format detected: with contentType and parser type: Rust HttpAgent does not emit structured lifecycle debug records for stream-format detection.
// SKIPPED: event validation log: [HTTP] Event validated: with type and valid:true on success: Rust HttpAgent does not perform or log per-event validation at the HTTP transform layer.
// SKIPPED: event invalid log: [HTTP] Event invalid: on schema parse failure: Rust transform layer returns AgUiError values but does not expose debug-log hooks for invalid-event reporting.
