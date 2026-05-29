#![allow(dead_code)]
#![allow(unused_imports)]

use agui_rs_client::DebugLogger;
use agui_rs_core::{BaseEventFields, Event, RunStartedEvent};
use std::sync::{Mutex, OnceLock};

fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn run_event() -> Event {
    Event::RunStarted(RunStartedEvent {
        thread_id: "thread-1".into(),
        run_id: "run-1".into(),
        parent_run_id: None,
        input: None,
        base: BaseEventFields::default(),
    })
}

#[test]
fn debug_logger_respects_env_var_and_renders_without_panicking() {
    let _guard = env_lock().lock().expect("env lock");
    let original = std::env::var_os("AG_UI_DEBUG");

    std::env::remove_var("AG_UI_DEBUG");
    let disabled = DebugLogger::new("agent");
    assert!(!disabled.enabled());
    assert_eq!(disabled.prefix(), "agent");

    for truthy in ["1", "true", "yes", "on", "debug"] {
        std::env::set_var("AG_UI_DEBUG", truthy);
        assert!(
            DebugLogger::new("agent").enabled(),
            "expected {truthy} to enable logging"
        );
    }

    for falsey in ["", "0", "false", "off", "no"] {
        std::env::set_var("AG_UI_DEBUG", falsey);
        assert!(
            !DebugLogger::new("agent").enabled(),
            "expected {falsey:?} to disable logging"
        );
    }

    std::env::set_var("AG_UI_DEBUG", "1");
    let enabled = DebugLogger::new("agent");
    enabled.log("hello");
    enabled.log_event(&run_event());
    enabled.log_error(&agui_rs_core::AgUiError::other("boom"));

    if let Some(value) = original {
        std::env::set_var("AG_UI_DEBUG", value);
    } else {
        std::env::remove_var("AG_UI_DEBUG");
    }
}
