// IMPLEMENTED: `AbstractAgent.clone()` is now `AgentRunner::clone_runner()` —
// covered in tests/agent_lifecycle.rs (clone copies messages/state/thread_id
// with independent ownership).
//
// SKIPPED: TypeScript HttpAgent.clone()/AbortController cloning behavior — the
// Rust HttpAgent uses a per-run AbortHandle, not a persistent AbortController
// field, so there is no controller state to clone (see agent_lifecycle.rs).
