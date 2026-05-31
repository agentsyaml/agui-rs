use agui_rs_core::{AgUiError, Event};
use std::env;

#[derive(Debug, Clone)]
pub struct DebugLogger {
    enabled: bool,
    prefix: String,
}

impl DebugLogger {
    pub fn new(prefix: impl Into<String>) -> Self {
        Self {
            enabled: env_debug_enabled(),
            prefix: prefix.into(),
        }
    }

    /// Creates a logger that is always enabled, regardless of environment.
    /// Used when the caller explicitly opts into debug logging
    /// (e.g. `AgentConfig { debug: true, .. }`), mirroring TS `debug: true`.
    pub fn forced(prefix: impl Into<String>) -> Self {
        Self {
            enabled: true,
            prefix: prefix.into(),
        }
    }

    pub fn log(&self, msg: &str) {
        if self.enabled {
            eprintln!("{}", self.render_log(msg));
        }
    }

    pub fn log_event(&self, event: &Event) {
        if self.enabled {
            eprintln!("{}", self.render_event(event));
        }
    }

    pub fn log_error(&self, err: &AgUiError) {
        if self.enabled {
            eprintln!("{}", self.render_error(err));
        }
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn prefix(&self) -> &str {
        &self.prefix
    }

    fn render_log(&self, msg: &str) -> String {
        format!("[{}] {}", self.prefix, msg)
    }

    fn render_event(&self, event: &Event) -> String {
        format!("[{}] {}", self.prefix, format_event(event))
    }

    fn render_error(&self, err: &AgUiError) -> String {
        format!("[{}] error: {}", self.prefix, err)
    }
}

fn env_debug_enabled() -> bool {
    match env::var("AG_UI_DEBUG") {
        Ok(value) => {
            let value = value.trim().to_ascii_lowercase();
            !matches!(value.as_str(), "" | "0" | "false" | "off" | "no")
        }
        Err(_) => false,
    }
}

fn format_event(event: &Event) -> String {
    serde_json::to_string(event).unwrap_or_else(|_| format!("{:?}", event))
}

#[cfg(test)]
mod debug_logger_tests {
    use super::*;
    use agui_rs_core::{BaseEventFields, EventType, RunStartedEvent};
    use std::sync::{Mutex, OnceLock};

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn run_event() -> Event {
        Event::RunStarted(RunStartedEvent {
            thread_id: "thread-1".to_string(),
            run_id: "run-1".to_string(),
            parent_run_id: None,
            input: None,
            base: BaseEventFields::default(),
        })
    }

    fn error_event() -> AgUiError {
        AgUiError::other("boom")
    }

    #[test]
    fn disabled_by_default() {
        let _guard = env_lock().lock().unwrap();
        std::env::remove_var("AG_UI_DEBUG");
        let logger = DebugLogger::new("p");
        assert!(!logger.enabled());
    }

    #[test]
    fn enabled_when_env_is_truthy() {
        let _guard = env_lock().lock().unwrap();
        std::env::set_var("AG_UI_DEBUG", "1");
        let logger = DebugLogger::new("p");
        assert!(logger.enabled());
        std::env::remove_var("AG_UI_DEBUG");
    }

    #[test]
    fn disabled_when_env_is_falsey() {
        let _guard = env_lock().lock().unwrap();
        std::env::set_var("AG_UI_DEBUG", "false");
        let logger = DebugLogger::new("p");
        assert!(!logger.enabled());
        std::env::remove_var("AG_UI_DEBUG");
    }

    #[test]
    fn prefix_is_preserved() {
        let _guard = env_lock().lock().unwrap();
        std::env::remove_var("AG_UI_DEBUG");
        let logger = DebugLogger::new("agent");
        assert_eq!(logger.prefix(), "agent");
    }

    #[test]
    fn formats_event_payload() {
        let event = run_event();
        let rendered = format_event(&event);
        assert!(rendered.contains("RUN_STARTED"));
        assert!(rendered.contains("thread-1"));
        assert_eq!(event.event_type(), EventType::RunStarted);
    }

    #[test]
    fn formats_error_payload() {
        let err = error_event();
        let logger = DebugLogger {
            enabled: true,
            prefix: "agent".to_string(),
        };
        assert_eq!(logger.render_error(&err), "[agent] error: boom");
    }

    #[test]
    fn log_methods_do_not_panic_when_enabled() {
        let logger = DebugLogger {
            enabled: true,
            prefix: "agent".to_string(),
        };
        logger.log("hello");
        logger.log_event(&run_event());
        logger.log_error(&AgUiError::other("boom"));
        assert_eq!(logger.render_log("hello"), "[agent] hello");
        assert!(logger.render_event(&run_event()).contains("RUN_STARTED"));
    }

    #[test]
    fn error_display_uses_message() {
        let logger = DebugLogger {
            enabled: true,
            prefix: "agent".to_string(),
        };
        logger.log_error(&AgUiError::other("boom"));
        let rendered = logger.render_error(&AgUiError::other("boom"));
        assert_eq!(rendered, "[agent] error: boom");
    }

    #[test]
    fn render_event_is_prefixed() {
        let logger = DebugLogger {
            enabled: true,
            prefix: "agent".to_string(),
        };
        let rendered = logger.render_event(&run_event());
        assert!(rendered.starts_with("[agent] "));
        assert!(rendered.contains("thread-1"));
    }

    #[test]
    fn disabled_loggers_still_render() {
        let logger = DebugLogger {
            enabled: false,
            prefix: "agent".to_string(),
        };
        assert_eq!(logger.render_log("hello"), "[agent] hello");
        assert!(logger.render_event(&run_event()).contains("RUN_STARTED"));
        assert_eq!(
            logger.render_error(&AgUiError::other("boom")),
            "[agent] error: boom"
        );
    }
}
