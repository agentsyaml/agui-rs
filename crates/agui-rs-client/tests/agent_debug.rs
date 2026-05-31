use agui_rs_client::DebugLogger;
use std::sync::{Mutex, OnceLock};

fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

#[test]
fn debug_logger_is_undefined_equivalent_when_debug_env_is_missing() {
    let _guard = env_lock().lock().expect("env lock");
    let original = std::env::var_os("AG_UI_DEBUG");
    std::env::remove_var("AG_UI_DEBUG");

    let logger = DebugLogger::new("agent");
    assert!(!logger.enabled());

    if let Some(value) = original {
        std::env::set_var("AG_UI_DEBUG", value);
    }
}

#[test]
fn debug_logger_is_enabled_for_truthy_env_values() {
    let _guard = env_lock().lock().expect("env lock");
    let original = std::env::var_os("AG_UI_DEBUG");

    for value in ["1", "true", "yes", "on", "debug"] {
        std::env::set_var("AG_UI_DEBUG", value);
        assert!(
            DebugLogger::new("agent").enabled(),
            "expected {value} to enable logging"
        );
    }

    if let Some(value) = original {
        std::env::set_var("AG_UI_DEBUG", value);
    } else {
        std::env::remove_var("AG_UI_DEBUG");
    }
}

#[test]
fn debug_logger_preserves_prefix() {
    let logger = DebugLogger::new("test-agent");
    assert_eq!(logger.prefix(), "test-agent");
}

#[test]
fn forced_logger_is_enabled_regardless_of_env() {
    let _guard = env_lock().lock().expect("env lock");
    let original = std::env::var_os("AG_UI_DEBUG");
    std::env::remove_var("AG_UI_DEBUG");

    // `AgentConfig { debug: true }` wires a forced logger that ignores the env
    // gate, mirroring TS `debug: true`.
    let logger = DebugLogger::forced("LIFECYCLE");
    assert!(logger.enabled());
    assert_eq!(logger.prefix(), "LIFECYCLE");

    if let Some(value) = original {
        std::env::set_var("AG_UI_DEBUG", value);
    }
}

// IMPLEMENTED: TS `debug: true` construction maps to `AgentConfig { debug: true }`,
// which installs a forced (env-independent) lifecycle `DebugLogger` on the runner
// — see `forced_logger_is_enabled_regardless_of_env` and the runner Run
// started/finished lifecycle logs.
// SKIPPED: TypeScript per-event console.debug capture (stream-stage [SSE]/[CHUNK]/
// [VERIFY] prefixes) is a documented divergence — Rust pure stream fns take no
// logger; only runner-level lifecycle logging is wired. See docs/typescript-alignment.md §4.
// SKIPPED: asserting exact console.debug call counts relies on Vitest spy capture,
// which has no Rust equivalent (logs go to stderr through DebugLogger).
