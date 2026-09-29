//! The always-on inbound compatibility boundary.
//!
//! Upstream 1.0 keeps exactly one unversioned shim: `THINKING_*` events sent by
//! a pre-1.0 peer become their `REASONING_*` equivalents. It is not
//! version-gated — a legacy-shaped event arriving is itself the proof the peer
//! is old, and on a modern stream every branch below is a no-op.
//!
//! It runs on the RAW stream, before the wire bytes become typed events: the
//! 1.0 event set has no `THINKING_*` variants, so serde would reject such a
//! frame before any event-level code could see it. Mirrors the upstream
//! `CompatibilityBoundary`, which likewise reads the raw stream rather than the
//! transformed one.

use serde_json::Value;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

const THINKING_START: &str = "THINKING_START";
const THINKING_END: &str = "THINKING_END";
const THINKING_TEXT_MESSAGE_START: &str = "THINKING_TEXT_MESSAGE_START";
const THINKING_TEXT_MESSAGE_CONTENT: &str = "THINKING_TEXT_MESSAGE_CONTENT";
const THINKING_TEXT_MESSAGE_END: &str = "THINKING_TEXT_MESSAGE_END";

/// Per-stream translation state: the ids a continuation names live here, put
/// there by the matching opener. A fresh boundary per stream keeps the state
/// per run.
#[derive(Debug, Default)]
pub(crate) struct CompatBoundary {
    current_reasoning_id: Option<String>,
    current_message_id: Option<String>,
}

/// Upgrades one raw JSON event. Non-objects and events of any other type are
/// returned unchanged.
pub(crate) fn map_raw_event(boundary: &mut CompatBoundary, event: Value) -> Value {
    let Value::Object(mut map) = event else {
        return event;
    };

    let Some(event_type) = map.get("type").and_then(Value::as_str) else {
        return Value::Object(map);
    };
    let event_type = event_type.to_owned();

    match event_type.as_str() {
        THINKING_START => {
            let message_id = mint_id("reasoning");
            boundary.current_reasoning_id = Some(message_id.clone());
            // REASONING_START has no `title`, so the span's label is dropped
            // here and nothing downstream can recover it. Named separately
            // from the conversion notice because it is a LOSS.
            if let Some(title) = map.remove("title") {
                warn_aside(&format!(
                    "Dropping {THINKING_START}.title {title}: REASONING_START has no title field, so the span's label cannot be carried and nothing downstream can recover it."
                ));
            }
            warn(THINKING_START, "REASONING_START");
            map.insert("type".to_string(), Value::String("REASONING_START".into()));
            map.insert("messageId".to_string(), Value::String(message_id));
        }
        THINKING_TEXT_MESSAGE_START => {
            let message_id = mint_id("reasoning-message");
            boundary.current_message_id = Some(message_id.clone());
            warn(THINKING_TEXT_MESSAGE_START, "REASONING_MESSAGE_START");
            map.insert(
                "type".to_string(),
                Value::String("REASONING_MESSAGE_START".into()),
            );
            map.insert("messageId".to_string(), Value::String(message_id));
            // The schema pins this role to "reasoning"; the old translation
            // said "assistant", which nothing validated.
            map.insert("role".to_string(), Value::String("reasoning".into()));
        }
        THINKING_TEXT_MESSAGE_CONTENT => {
            // A continuation keeps the opener's id; only the end closes it.
            let message_id =
                continuation_id(THINKING_TEXT_MESSAGE_CONTENT, &boundary.current_message_id);
            warn(THINKING_TEXT_MESSAGE_CONTENT, "REASONING_MESSAGE_CONTENT");
            map.insert(
                "type".to_string(),
                Value::String("REASONING_MESSAGE_CONTENT".into()),
            );
            map.insert("messageId".to_string(), Value::String(message_id));
        }
        THINKING_TEXT_MESSAGE_END => {
            let message_id = boundary
                .current_message_id
                .take()
                .unwrap_or_else(|| mint_continuation_id(THINKING_TEXT_MESSAGE_END));
            warn(THINKING_TEXT_MESSAGE_END, "REASONING_MESSAGE_END");
            map.insert(
                "type".to_string(),
                Value::String("REASONING_MESSAGE_END".into()),
            );
            map.insert("messageId".to_string(), Value::String(message_id));
        }
        THINKING_END => {
            let message_id = boundary
                .current_reasoning_id
                .take()
                .unwrap_or_else(|| mint_continuation_id(THINKING_END));
            warn(THINKING_END, "REASONING_END");
            map.insert("type".to_string(), Value::String("REASONING_END".into()));
            map.insert("messageId".to_string(), Value::String(message_id));
        }
        _ => return Value::Object(map),
    }

    Value::Object(map)
}

/// The id a continuation belongs to, or a freshly minted one when no opener
/// preceded it.
fn continuation_id(from: &str, established: &Option<String>) -> String {
    established
        .clone()
        .unwrap_or_else(|| mint_continuation_id(from))
}

/// A continuation with nothing to continue names nothing, and verification
/// rejects the translated event a few stages later — so the invention is
/// announced here, where it is still traceable to its cause.
fn mint_continuation_id(from: &str) -> String {
    let minted = mint_id("reasoning-message");
    warn_aside(&format!(
        "Minting a messageId ('{minted}') for {from}: no THINKING opener preceded it, so there was no id to continue. The id is this client's invention, not the producer's, and verification will reject the translated event for naming something nothing opened."
    ));
    minted
}

fn mint_id(prefix: &str) -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let counter = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{prefix}-{nanos}-{counter}")
}

fn suppressed() -> bool {
    std::env::var_os("SUPPRESS_TRANSFORMATION_WARNINGS").is_some()
}

fn warn(what: &str, replacement: &str) {
    if suppressed() {
        return;
    }
    eprintln!(
        "[ag-ui][compat] Converting deprecated {what} to {replacement}. The old shape leaves the protocol after its shim window — see the repo-root DEPRECATIONS.md. Set SUPPRESS_TRANSFORMATION_WARNINGS=true to silence."
    );
}

fn warn_aside(sentence: &str) {
    if suppressed() {
        return;
    }
    eprintln!("[ag-ui][compat] {sentence} Set SUPPRESS_TRANSFORMATION_WARNINGS=true to silence.");
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn run(events: &[Value]) -> Vec<Value> {
        let mut boundary = CompatBoundary::default();
        events
            .iter()
            .map(|event| map_raw_event(&mut boundary, event.clone()))
            .collect()
    }

    #[test]
    fn maps_full_thinking_sequence() {
        let mapped = run(&[
            json!({"type": THINKING_START, "title": "plan"}),
            json!({"type": THINKING_TEXT_MESSAGE_START, "role": "assistant"}),
            json!({"type": THINKING_TEXT_MESSAGE_CONTENT, "delta": "hello"}),
            json!({"type": THINKING_TEXT_MESSAGE_END}),
            json!({"type": THINKING_END}),
        ]);

        assert_eq!(mapped[0]["type"], "REASONING_START");
        assert!(mapped[0].get("title").is_none());
        assert_eq!(mapped[1]["type"], "REASONING_MESSAGE_START");
        assert_eq!(mapped[1]["role"], "reasoning");
        assert_eq!(mapped[2]["type"], "REASONING_MESSAGE_CONTENT");
        assert_eq!(mapped[2]["delta"], "hello");
        assert_eq!(mapped[3]["type"], "REASONING_MESSAGE_END");
        assert_eq!(mapped[4]["type"], "REASONING_END");

        let reasoning_id = mapped[0]["messageId"].as_str().unwrap();
        let message_id = mapped[1]["messageId"].as_str().unwrap();
        assert_ne!(reasoning_id, message_id);
        assert_eq!(mapped[2]["messageId"], message_id);
        assert_eq!(mapped[3]["messageId"], message_id);
        assert_eq!(mapped[4]["messageId"], reasoning_id);
    }

    #[test]
    fn mints_ids_without_an_opener() {
        let mapped = run(&[json!({"type": THINKING_TEXT_MESSAGE_CONTENT, "delta": "x"})]);
        assert!(mapped[0]["messageId"]
            .as_str()
            .is_some_and(|id| !id.is_empty()));
    }

    #[test]
    fn leaves_modern_events_untouched() {
        let event = json!({"type": "TEXT_MESSAGE_CONTENT", "messageId": "m1", "delta": "hi"});
        assert_eq!(&run(std::slice::from_ref(&event))[0], &event);
    }
}
