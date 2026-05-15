use ag_ui_client::DebugLogger;
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

// SKIPPED: TypeScript AbstractAgent debug=true/false/object construction API does not exist in the Rust public client API.
// SKIPPED: TypeScript lifecycle/event console.debug capture is not exposed by the Rust public client API.
// SKIPPED: TypeScript runAgent debug error logging on agent execution is not configurable through the Rust public client API.
