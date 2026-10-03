//! Metadata folding. Mirrors upstream `core/src/metadata.ts`.

use serde_json::{Map, Value};

/// The key reserved for AG-UI's own use inside a metadata object. Every other
/// key is user space. Reservation is by convention: nothing rejects a write to
/// it, because metadata is open by key.
pub const AGUI_METADATA_KEY: &str = "ag-ui";

/// Folds `incoming` into `existing`, key by key, last write winning.
///
/// A message is assembled from a sequence of events, and the interesting values
/// — token usage and finish reason among them — are only known at the end, so
/// metadata accumulates as the sequence arrives rather than being fixed at the
/// start. A key's value is replaced outright and never recursed into, so an
/// array or object under any key is replaced wholesale rather than blended.
///
/// Event-level metadata is typed `Option<Value>` here while the schema types it
/// as an object, so a non-object can arrive. Folding key-wise, upstream spreads
/// it (`metadata.ts:63`, `{ ...existing, ...incoming }`): a non-object
/// contributes no keys of its own, so
///
/// * a non-object `incoming` over an object `existing` changes nothing — it
///   cannot replace anything key-wise (JS does not invent keys from a scalar),
///   and
/// * a non-object `existing` folds away to nothing and the result is
///   `incoming`'s keys, or an empty object if both are non-objects.
///
/// Both spellings differ from "the non-object replaces the accumulator": only
/// the key-wise outcome above matches the source.
///
/// An absent `incoming` returns `existing` untouched; an empty object changes
/// nothing.
pub fn merge_metadata(existing: Option<&Value>, incoming: Option<&Value>) -> Option<Value> {
    let Some(incoming) = incoming else {
        return existing.cloned();
    };

    match (existing, incoming) {
        (Some(Value::Object(previous)), Value::Object(next)) => {
            let mut merged = Map::new();
            merged.extend(previous.clone());
            for (key, value) in next {
                merged.insert(key.clone(), value.clone());
            }
            Some(Value::Object(merged))
        }
        // A non-object spreads to no keys (`...7` in JS is empty), so which
        // side is the object decides what survives.
        (Some(existing @ Value::Object(_)), _) => Some(existing.clone()),
        (_, incoming) => Some(Value::Object(
            incoming.as_object().cloned().unwrap_or_default(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn obj(value: Value) -> Option<Value> {
        Some(value)
    }

    #[test]
    fn absent_incoming_returns_existing_untouched() {
        let existing = obj(json!({"a": 1}));
        assert_eq!(merge_metadata(existing.as_ref(), None), existing);
    }

    #[test]
    fn absent_both_stays_absent() {
        assert_eq!(merge_metadata(None, None), None);
    }

    #[test]
    fn empty_incoming_changes_nothing() {
        let existing = obj(json!({"a": 1}));
        assert_eq!(
            merge_metadata(existing.as_ref(), Some(&json!({}))),
            existing
        );
    }

    #[test]
    fn folds_key_by_key_with_last_write_winning() {
        let existing = obj(json!({"a": 1, "b": 2}));
        let merged = merge_metadata(existing.as_ref(), Some(&json!({"b": 3, "c": 4})));
        assert_eq!(merged, Some(json!({"a": 1, "b": 3, "c": 4})));
    }

    #[test]
    fn absent_existing_adopts_incoming() {
        assert_eq!(
            merge_metadata(None, Some(&json!({"a": 1}))),
            Some(json!({"a": 1}))
        );
    }

    #[test]
    fn a_value_is_replaced_wholesale_never_merged() {
        let existing = obj(json!({"k": {"deep": 1, "keep": true}}));
        let merged = merge_metadata(existing.as_ref(), Some(&json!({"k": [1, 2]})));
        assert_eq!(merged, Some(json!({"k": [1, 2]})));
    }

    #[test]
    fn the_reserved_key_is_folded_like_any_other() {
        let existing = obj(json!({AGUI_METADATA_KEY: {"v": 1}}));
        let merged = merge_metadata(existing.as_ref(), Some(&json!({AGUI_METADATA_KEY: 2})));
        assert_eq!(merged, Some(json!({AGUI_METADATA_KEY: 2})));
    }

    #[test]
    fn a_non_object_incoming_spreads_to_no_keys() {
        // Upstream spreads (`{ ...existing, ...incoming }`), and a scalar
        // contributes no keys, so the object survives untouched.
        let existing = obj(json!({"a": 1}));
        assert_eq!(
            merge_metadata(existing.as_ref(), Some(&json!(7))),
            Some(json!({"a": 1}))
        );
    }

    #[test]
    fn a_non_object_existing_folds_to_the_incoming_keys() {
        assert_eq!(
            merge_metadata(Some(&json!(7)), Some(&json!({"a": 1}))),
            Some(json!({"a": 1}))
        );
    }

    #[test]
    fn two_non_objects_fold_to_an_empty_object() {
        assert_eq!(
            merge_metadata(Some(&json!(7)), Some(&json!(9))),
            Some(json!({}))
        );
    }
}
