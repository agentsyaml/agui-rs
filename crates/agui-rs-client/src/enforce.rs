//! The enforcement stage.
//!
//! Upstream 1.0 guarantees that nothing unrecognised survives to
//! verification, subscribers or application code
//! (`sdks/typescript/packages/client/src/enforce/enforce.ts:50-69`):
//!
//! > - An event whose type nothing translated is dropped, with a warning.
//! > - Unknown properties and unrecognised union members on a known event are
//! >   stripped, each with a warning naming its path.
//! > - A malformed value on a field the protocol DOES describe stays fatal:
//! >   the generated validator throws, and the run errors.
//!
//! Placement follows the same order that comment asks for
//! (`agent/agent.ts:366-370`, *"Enforcement BEFORE expansion: a chunk is an
//! event of its own … Verification stays after expansion"*): this stage sits
//! after the always-on compatibility boundary, the innermost thing on the wire,
//! and before the bytes become typed events — hence before chunk expansion and
//! long before verification.
//!
//! # Known-shape table
//!
//! Upstream walks a generated zod schema (`enforce/strip.ts`). This crate has
//! no schema runtime, so the property table is read once out of the byte-for-byte
//! copy of the 1.0 JSON Schema that `agui-rs-proto` already vendors
//! (`upstream-spec/schema.json`): every `$defs` member of the `Event` union, its
//! own `properties`, plus everything its `allOf` pulls in (`BaseEvent`,
//! `Attributable`).
//!
//! ponytail: the table covers the event's OWN keys. Upstream also descends
//! into `messages`, `tools` and the RFC 6902 `patch` array; reproducing that
//! needs a general JSON Schema evaluator, which is not worth a client crate.
//! Unknown members nested inside a message or tool call stay silently ignored
//! by serde, exactly as they were before this stage existed.

use serde_json::{Map, Value};
use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

/// The frozen 1.0 schema. Same bytes `agui-rs-proto` vendors, read at compile
/// time rather than duplicated as a hand-maintained table that would drift.
const SCHEMA: &str = include_str!("../../agui-rs-proto/upstream-spec/schema.json");

/// Whether the wire value's `type` names an event the 1.0 protocol describes.
pub fn is_recognized_event(event: &Value) -> bool {
    event
        .get("type")
        .and_then(Value::as_str)
        .is_some_and(|event_type| event_properties().contains_key(event_type))
}

/// Applies the enforcement stage to one raw event.
///
/// Returns the event to hand on, or `None` when it was dropped. A malformed
/// value on a field the protocol *does* describe is left in place: typed
/// deserialization rejects it, which is where the run errors.
pub fn enforce_event(event: Value) -> Option<Value> {
    let Value::Object(mut map) = event else {
        return Some(event);
    };
    let Some(event_type) = map.get("type").and_then(Value::as_str).map(str::to_owned) else {
        return Some(Value::Object(map));
    };

    let Some(known) = event_properties().get(&event_type) else {
        warn(&format!(
            "Dropping unrecognised event '{event_type}': no middleware translated it and the protocol does not describe it."
        ));
        return None;
    };

    let unknown: Vec<String> = map
        .keys()
        .filter(|key| !known.contains(key.as_str()))
        .cloned()
        .collect();
    for key in unknown {
        map.remove(&key);
        warn(&format!(
            "Removed unrecognised material at '/{key}' on {event_type}. Nothing handled it; see the repo-root DEPRECATIONS.md if it is a retired shape."
        ));
    }

    Some(Value::Object(map))
}

/// `type` discriminator -> every property that event's definition describes.
fn event_properties() -> &'static HashMap<String, HashSet<String>> {
    static TABLE: OnceLock<HashMap<String, HashSet<String>>> = OnceLock::new();
    TABLE.get_or_init(build_event_properties)
}

fn build_event_properties() -> HashMap<String, HashSet<String>> {
    let root: Value = serde_json::from_str(SCHEMA).expect("bundled 1.0 schema is valid JSON");
    let defs = root
        .get("$defs")
        .and_then(Value::as_object)
        .expect("bundled 1.0 schema has $defs");

    let mut table = HashMap::new();
    let Some(members) = root
        .get("$defs")
        .and_then(|defs| defs.get("Event"))
        .and_then(|event| event.get("oneOf"))
        .and_then(Value::as_array)
    else {
        return table;
    };

    for member in members {
        let Some(name) = def_name(member) else {
            continue;
        };
        let Some(definition) = defs.get(name) else {
            continue;
        };
        let Some(event_type) = definition
            .get("properties")
            .and_then(|props| props.get("type"))
            .and_then(|ty| ty.get("const"))
            .and_then(Value::as_str)
        else {
            continue;
        };
        table.insert(event_type.to_owned(), described_keys(definition, defs));
    }
    table
}

/// An event's own property names, plus everything its `allOf` composes in.
fn described_keys(definition: &Value, defs: &Map<String, Value>) -> HashSet<String> {
    let mut keys: HashSet<String> = definition
        .get("properties")
        .and_then(Value::as_object)
        .map(|props| props.keys().cloned().collect())
        .unwrap_or_default();

    if let Some(members) = definition.get("allOf").and_then(Value::as_array) {
        for member in members {
            if let Some(composed) = def_name(member).and_then(|name| defs.get(name)) {
                keys.extend(described_keys(composed, defs));
            }
        }
    }
    keys
}

fn def_name(schema: &Value) -> Option<&str> {
    schema
        .get("$ref")
        .and_then(Value::as_str)
        .and_then(|reference| reference.rsplit('/').next())
}

fn suppressed() -> bool {
    std::env::var_os("SUPPRESS_TRANSFORMATION_WARNINGS").is_some()
}

fn warn(message: &str) {
    if suppressed() {
        return;
    }
    eprintln!("[ag-ui][enforce] {message} Set SUPPRESS_TRANSFORMATION_WARNINGS=true to silence.");
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn table_covers_every_event_the_schema_defines() {
        let table = event_properties();
        // One sanity anchor per family, including the `allOf`-only members that
        // a naive own-properties read would miss.
        for event_type in [
            "RUN_STARTED",
            "TEXT_MESSAGE_START",
            "MESSAGES_SNAPSHOT",
            "ACTIVITY_DELTA",
            "RAW",
            "SUBAGENT_ERROR",
        ] {
            let keys = table
                .get(event_type)
                .unwrap_or_else(|| panic!("{event_type} missing from the table"));
            // `type` is its own; `timestamp`/`rawEvent`/`metadata` come from
            // BaseEvent; `subagentRunId` from Attributable.
            assert!(keys.contains("type"), "{event_type} lost its own type");
            assert!(keys.contains("timestamp"), "{event_type} lost BaseEvent");
            assert!(keys.contains("rawEvent"), "{event_type} lost BaseEvent");
            assert!(keys.contains("metadata"), "{event_type} lost BaseEvent");
        }
        assert!(table["RAW"].contains("source"));
        assert!(table["SUBAGENT_ERROR"].contains("subagentRunId"));
        // The whole 1.0 discriminator enum, not a hand-picked subset.
        assert_eq!(table.len(), 31);
    }

    #[test]
    fn drops_an_unrecognised_event_type() {
        assert!(!is_recognized_event(&json!({"type": "THINKING_START"})));
        assert_eq!(enforce_event(json!({"type": "NOPE"})), None);
    }

    #[test]
    fn strips_and_keeps_known_properties() {
        let kept = enforce_event(json!({
            "type": "TEXT_MESSAGE_CONTENT",
            "messageId": "m1",
            "delta": "hi",
            "timestamp": 1,
            "rawEvent": {"a": 1},
            "metadata": {"k": "v"},
            "mystery": 42,
        }))
        .expect("known event survives");

        assert!(kept.get("mystery").is_none());
        assert_eq!(kept["messageId"], "m1");
        assert_eq!(kept["delta"], "hi");
        assert!(kept.get("rawEvent").is_some());
        assert!(kept.get("metadata").is_some());
    }

    #[test]
    fn leaves_a_malformed_known_field_in_place_for_serde_to_reject() {
        // `messageId` is described; a number there is a malformed VALUE, which
        // stays fatal rather than being stripped.
        let kept = enforce_event(json!({
            "type": "TEXT_MESSAGE_CONTENT",
            "messageId": 7,
            "delta": "hi",
        }))
        .expect("known event survives");
        assert_eq!(kept["messageId"], 7);
    }

    #[test]
    fn passes_through_values_that_name_no_event_type() {
        assert!(enforce_event(json!({})).is_some());
        assert!(enforce_event(json!("nope")).is_some());
    }
}
