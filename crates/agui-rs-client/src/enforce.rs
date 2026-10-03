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
//! # Recursive stripping
//!
//! Upstream deep-strips against a generated zod schema
//! (`enforce/strip.ts:93-250`). This crate has no schema runtime, so the
//! shapes are read once out of the byte-for-byte copy of the 1.0 JSON Schema
//! that `agui-rs-proto` already vendors (`upstream-spec/schema.json`) and
//! walked recursively, mirroring `stripAgainst`:
//!
//! - a described object strips every key its `properties` (own, plus those
//!   composed in via `allOf`) does not name (`strip.ts:130-137`), unless the
//!   schema marks itself open (`additionalProperties: true`, e.g.
//!   `Metadata`; `strip.ts:116-134`);
//! - an array descends into each element and reports the index
//!   (`strip.ts:163-179`);
//! - a discriminated union recurses into the member whose const tag matches
//!   the value's tag; an object carrying no member's tag is removed whole
//!   from its container (`strip.ts:221-222`), and a drop collapses the
//!   removal report to the container entry alone (`strip.ts:170-175`);
//! - a non-object (string/array/number/bool/null) in a discriminated-union
//!   slot is left in place — a malformed VALUE stays fatal
//!   (`strip.ts:211`, tested at `strip.test.ts:23-61`);
//! - a structural union recurses into the single matching-kind member
//!   (`strip.ts:232-244`; the two-object-options guard is `strip.ts:234`);
//! - leaves and opaque positions (`State`, `forwardedProps`, `rawEvent`,
//!   a patch operation's `value`, …) pass through whole
//!   (`strip.ts:247-249`).
//!
//! Paths are pointer-ish, joined as parent/index-or-key exactly as upstream
//! builds them (`strip.ts:144,169`): `""` → `/messages/0/content/7/junk`.
//!
//! RFC 6902 operations are open by spec (generated zod marks them
//! `.meta({ specOpen: true })` because RFC 6902 §4 requires members an
//! operation does not define to be ignored; tested at
//! `enforce.test.ts:255-275`). The vendored JSON Schema carries no marker, so
//! their six `$defs` names are kept as a hand-checked set below.
//!
//! # Not covered
//!
//! Upstream guards three constructs no 1.0 schema uses, so the walker has no
//! branch for them: a structural union of two-or-more object options
//! (`strip.ts:232-234`), zod `record` (`strip.ts:181-199`), and the wrapper
//! unwrapping of pipes/lazy/catch (`strip.ts:32-57`). Non-`$ref` `allOf`
//! members are likewise ignored (every 1.0 composition goes through `$ref`).
//! If a schema ever grows one of these shapes, unknown keys under it stay
//! silently ignored by serde, as before this stage existed.

use serde_json::{Map, Value};
use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

/// The frozen 1.0 schema. Same bytes `agui-rs-proto` vendors, read at compile
/// time rather than duplicated as a hand-maintained table that would drift.
const SCHEMA: &str = include_str!("../../agui-rs-proto/upstream-spec/schema.json");

/// The six RFC 6902 operations. The generated zod marks them `.meta({ specOpen:
/// true })` (`core/src/generated/schemas.ts:358-412`); the vendored JSON Schema
/// has no marker, so their names stand in for it. Verified against
/// `JsonPatchOperation`'s `oneOf` in the bundled schema.
const SPEC_OPEN_DEFS: [&str; 6] = [
    "AddOperation",
    "RemoveOperation",
    "ReplaceOperation",
    "MoveOperation",
    "CopyOperation",
    "TestOperation",
];

/// Whether the wire value's `type` names an event the 1.0 protocol describes.
pub fn is_recognized_event(event: &Value) -> bool {
    event
        .get("type")
        .and_then(Value::as_str)
        .is_some_and(|event_type| event_shapes().contains_key(event_type))
}

/// Applies the enforcement stage to one raw event.
///
/// Returns the event to hand on, or `None` when it was dropped. A malformed
/// value on a field the protocol *does* describe is left in place: typed
/// deserialization rejects it, which is where the run errors.
pub fn enforce_event(event: Value) -> Option<Value> {
    let Value::Object(map) = event else {
        return Some(event);
    };
    let Some(event_type) = map.get("type").and_then(Value::as_str).map(str::to_owned) else {
        return Some(Value::Object(map));
    };

    let Some(shape) = event_shapes().get(&event_type) else {
        warn(&format!(
            "Dropping unrecognised event '{event_type}': no middleware translated it and the protocol does not describe it."
        ));
        return None;
    };

    let mut stripped = Vec::new();
    let value = strip_against(Value::Object(map), shape, "", &mut stripped);
    for path in &stripped {
        warn(&format!(
            "Removed unrecognised material at '{path}' on {event_type}. Nothing handled it; see the repo-root DEPRECATIONS.md if it is a retired shape."
        ));
    }
    match value {
        Stripped::Stripped(value) => Some(value),
        Stripped::Drop => {
            // Unreachable through the 1.0 schemas: every required union sits
            // inside an array, whose element drop absorbs it
            // (`strip.ts:256-265`). Unreachability is not a licence to hand
            // back unstripped material quietly, so it stays loud.
            warn(&format!(
                "Internal error: the stripper found the whole event '{event_type}' unrecognisable, which the schema is not supposed to allow — schema/stripper mismatch."
            ));
            Some(Value::Object(Map::new()))
        }
    }
}

/// Mirrors the `DROP` symbol (`strip.ts:30`): either the value, or a marker
/// that the whole value is unrecognisable here while unwinding.
enum Stripped {
    Stripped(Value),
    Drop,
}

fn strip_against(value: Value, shape: &Shape, path: &str, stripped: &mut Vec<String>) -> Stripped {
    match shape {
        Shape::Ref(name) => {
            let Some(definition) = definitions().get(name.as_str()) else {
                return Stripped::Stripped(value);
            };
            strip_against(value, definition.shape(), path, stripped)
        }
        Shape::OneOf(members) => strip_union(value, members, path, stripped),
        Shape::Object {
            properties,
            all_of,
            required,
            open,
        } => strip_object(value, properties, all_of, required, *open, path, stripped),
        Shape::Array(items) => match value {
            Value::Array(entries) => {
                let mut out = Vec::with_capacity(entries.len());
                for (index, entry) in entries.into_iter().enumerate() {
                    let child_path = format!("{path}/{index}");
                    let mark = stripped.len();
                    match strip_against(entry, items, &child_path, stripped) {
                        Stripped::Stripped(child) => out.push(child),
                        Stripped::Drop => {
                            // The element goes whole, so the paths its descent
                            // named go with it (`strip.ts:170-175`).
                            stripped.truncate(mark);
                            stripped.push(child_path);
                        }
                    }
                }
                Stripped::Stripped(Value::Array(out))
            }
            _ => Stripped::Stripped(value),
        },
        // Leaves and opaque schemas pass through whole (`strip.ts:247-249`).
        Shape::Leaf | Shape::Const(_) => Stripped::Stripped(value),
    }
}

fn strip_object(
    value: Value,
    properties: &HashMap<String, Shape>,
    all_of: &[String],
    required: &HashSet<String>,
    open: bool,
    path: &str,
    stripped: &mut Vec<String>,
) -> Stripped {
    let Value::Object(record) = value else {
        // Wrong shape: the validator's to reject, fatally (`strip.ts:103`).
        return Stripped::Stripped(value);
    };
    let mut result = Map::new();

    for (key, child_value) in record {
        let field_shape = properties.get(&key).or_else(|| {
            all_of.iter().find_map(|composed| {
                definitions()
                    .get(composed.as_str())
                    .and_then(Definition::properties)
                    .and_then(|props| props.get(&key))
            })
        });
        let Some(field_shape) = field_shape else {
            if open {
                // The spec leaves this object open (`strip.ts:116-134`).
                result.insert(key, child_value);
            } else {
                // Unknown property on a closed described object: strip and
                // report (`strip.ts:130-137`).
                stripped.push(format!("{path}/{key}"));
            }
            continue;
        };

        let child_path = format!("{path}/{key}");
        let mark = stripped.len();
        match strip_against(child_value, field_shape, &child_path, stripped) {
            Stripped::Stripped(child) => {
                result.insert(key, child);
            }
            Stripped::Drop => {
                // An unrecognisable value in an OPTIONAL position is
                // removable; in a REQUIRED one the drop cascades
                // (`strip.ts:143-157`).
                stripped.truncate(mark);
                if required.contains(key.as_str()) {
                    return Stripped::Drop;
                }
                stripped.push(child_path);
            }
        }
    }

    Stripped::Stripped(Value::Object(result))
}

fn strip_union(
    value: Value,
    members: &[Shape],
    path: &str,
    stripped: &mut Vec<String>,
) -> Stripped {
    if let Value::Object(record) = &value {
        // Discriminated union: recurse into the member whose const tag(s) the
        // value's tags match (`strip.ts:213-220`).
        let tagged: Vec<&Shape> = members
            .iter()
            .filter(|member| !const_tags_of(member).is_empty())
            .collect();
        if !tagged.is_empty() {
            for member in tagged {
                let matches = const_tags_of(member)
                    .iter()
                    .all(|(key, tag)| record.get(key) == Some(tag));
                if matches {
                    return strip_against(value, member, path, stripped);
                }
            }
            // An unrecognised union member: removable, never fatal
            // (`strip.ts:221-222`).
            return Stripped::Drop;
        }
        // Structural union. With two or more object options there is no tag
        // to read; strip against neither (`strip.ts:232-234`).
        let object_members: Vec<&Shape> = members
            .iter()
            .filter(|member| member.object_shape().is_some())
            .collect();
        if object_members.len() == 1 {
            return strip_against(value, object_members[0], path, stripped);
        }
        return Stripped::Stripped(value);
    }
    if let Value::Array(_) = value {
        // A structural union whose kind-match is an array (e.g. `content`):
        // recurse into the array option (`strip.ts:237-239`).
        if let Some(member) = members
            .iter()
            .find(|member| matches!(member, Shape::Array(_)))
        {
            return strip_against(value, member, path, stripped);
        }
    }
    // Anything else — a string/array/number in a DISCRIMINATED slot included —
    // is a malformed VALUE on a described field: left for the validator
    // (`strip.ts:211`).
    Stripped::Stripped(value)
}

/// The `(key, const)` pairs a union member fixes directly. Tags live in the
/// member's own `properties`; a `$ref` is resolved at strip time, when the
/// definitions table is complete.
fn const_tags_of(shape: &Shape) -> Vec<(String, Value)> {
    let Some(Shape::Object { properties, .. }) = shape.object_shape() else {
        return Vec::new();
    };
    properties
        .iter()
        .filter_map(|(key, prop)| match prop {
            Shape::Const(tag) => Some((key.clone(), tag.clone())),
            _ => None,
        })
        .collect()
}

// --- schema reading ---------------------------------------------------------

#[derive(Clone)]
enum Shape {
    /// A `$ref` into `$defs`, resolved lazily at strip time.
    Ref(String),
    /// A `oneOf` over member shapes.
    OneOf(Vec<Shape>),
    Object {
        properties: HashMap<String, Shape>,
        /// `allOf` members pulled in via `$ref`, by `$defs` name.
        all_of: Vec<String>,
        /// Own `required`, plus everything the composed schemas require.
        required: HashSet<String>,
        /// The spec leaves this object open (`additionalProperties: true`,
        /// or one of the RFC 6902 operations).
        open: bool,
    },
    Array(Box<Shape>),
    /// A `const` constraint. Its value is a tag for union matching; the
    /// value underneath passes through (a mismatch is the validator's to
    /// reject, fatally).
    Const(Value),
    /// Anything else: leaves and opaque positions pass through whole.
    Leaf,
}

impl Shape {
    fn object_shape(&self) -> Option<&Shape> {
        match self {
            Shape::Object { .. } => Some(self),
            Shape::Ref(name) => definitions()
                .get(name.as_str())
                .map(Definition::shape)
                .and_then(Shape::object_shape),
            _ => None,
        }
    }
}

struct Definition {
    shape: Shape,
    /// Property keys the definition's own `properties` describe.
    properties: Option<HashMap<String, Shape>>,
}

impl Definition {
    fn shape(&self) -> &Shape {
        &self.shape
    }

    fn properties(&self) -> Option<&HashMap<String, Shape>> {
        self.properties.as_ref()
    }
}

fn definitions() -> &'static HashMap<String, Definition> {
    static TABLE: OnceLock<HashMap<String, Definition>> = OnceLock::new();
    TABLE.get_or_init(build_definitions)
}

/// `type` discriminator -> the event definition it selects, discovered from
/// the `Event` union exactly as the original table was.
fn event_shapes() -> &'static HashMap<String, Shape> {
    static TABLE: OnceLock<HashMap<String, Shape>> = OnceLock::new();
    TABLE.get_or_init(build_event_shapes)
}

fn build_event_shapes() -> HashMap<String, Shape> {
    let root: Value = serde_json::from_str(SCHEMA).expect("bundled 1.0 schema is valid JSON");
    let Some(defs) = root.get("$defs").and_then(Value::as_object) else {
        return HashMap::new();
    };
    let Some(members) = defs
        .get("Event")
        .and_then(|event| event.get("oneOf"))
        .and_then(Value::as_array)
    else {
        return HashMap::new();
    };

    members
        .iter()
        .filter_map(|member| {
            let name = def_name(member)?;
            let definition = defs.get(name)?;
            let event_type = definition
                .get("properties")?
                .get("type")?
                .get("const")?
                .as_str()?
                .to_owned();
            Some((event_type, Shape::Ref(name.to_owned())))
        })
        .collect()
}

fn build_definitions() -> HashMap<String, Definition> {
    let root: Value = serde_json::from_str(SCHEMA).expect("bundled 1.0 schema is valid JSON");
    let raw_defs = root
        .get("$defs")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();

    raw_defs
        .iter()
        .filter_map(|(name, def)| {
            let shape = shape_of(def, &raw_defs)?;
            let properties = shape.object_shape().and_then(Shape::as_properties).cloned();
            Some((name.clone(), Definition { shape, properties }))
        })
        .collect()
}

impl Shape {
    fn as_properties(&self) -> Option<&HashMap<String, Shape>> {
        match self {
            Shape::Object { properties, .. } => Some(properties),
            _ => None,
        }
    }
}

/// Reads one JSON-Schema node into a [`Shape`]. Handles exactly the constructs
/// the frozen 1.0 schema uses: `$ref`, `oneOf`, `allOf` composition through
/// `$ref`, `type: object` with `properties`, `type: array` with `items`,
/// `const`, and everything else as an opaque leaf.
fn shape_of(def: &Value, raw_defs: &Map<String, Value>) -> Option<Shape> {
    if let Some(reference) = def.get("$ref").and_then(Value::as_str) {
        return Some(Shape::Ref(reference.rsplit('/').next()?.to_owned()));
    }
    if let Some(members) = def.get("oneOf").and_then(Value::as_array) {
        let shapes: Option<Vec<Shape>> = members.iter().map(|m| shape_of(m, raw_defs)).collect();
        if let Some(members) = shapes {
            return Some(Shape::OneOf(members));
        }
        // A non-$ref member defeat: fall through to the generic read below.
    }
    if let Some(const_) = def.get("const") {
        return Some(Shape::Const(const_.clone()));
    }

    match def.get("type").and_then(Value::as_str) {
        Some("object") => {
            let mut properties = HashMap::new();
            if let Some(props) = def.get("properties").and_then(Value::as_object) {
                for (key, prop) in props {
                    properties.insert(key.clone(), shape_of(prop, raw_defs).unwrap_or(Shape::Leaf));
                }
            }
            let mut all_of = Vec::new();
            if let Some(composed) = def.get("allOf").and_then(Value::as_array) {
                for member in composed {
                    if let Some(name) = member
                        .get("$ref")
                        .and_then(Value::as_str)
                        .map(|r| r.rsplit('/').next().unwrap_or_default().to_owned())
                    {
                        all_of.push(name);
                    }
                }
            }
            let mut required = def
                .get("required")
                .and_then(Value::as_array)
                .map(|required| {
                    required
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect::<HashSet<_>>()
                })
                .unwrap_or_default();
            for name in &all_of {
                if let Some(composed) = raw_defs.get(name) {
                    required.extend(required_keys_of(composed, raw_defs));
                }
            }
            let name_is_open = raw_defs
                .iter()
                .any(|(key, raw)| raw == def && SPEC_OPEN_DEFS.contains(&key.as_str()));
            Some(Shape::Object {
                properties,
                all_of,
                required,
                open: def.get("additionalProperties") == Some(&Value::Bool(true)) || name_is_open,
            })
        }
        Some("array") => {
            let items = def
                .get("items")
                .and_then(|items| shape_of(items, raw_defs))
                .unwrap_or(Shape::Leaf);
            Some(Shape::Array(Box::new(items)))
        }
        _ => Some(Shape::Leaf),
    }
}

/// A definition's required keys: its own, plus everything its `allOf`
/// composes in. The frozen 1.0 schema is acyclic (verified once by hand), so
/// this terminates; the depth guard turns a future cycle into silence rather
/// than a stack overflow.
fn required_keys_of(def: &Value, raw_defs: &Map<String, Value>) -> HashSet<String> {
    required_keys_of_inner(def, raw_defs, 0)
}

const REQUIRED_DEPTH_LIMIT: usize = 32;

fn required_keys_of_inner(
    def: &Value,
    raw_defs: &Map<String, Value>,
    depth: usize,
) -> HashSet<String> {
    if depth > REQUIRED_DEPTH_LIMIT {
        return HashSet::new();
    }
    let mut keys: HashSet<String> = def
        .get("required")
        .and_then(Value::as_array)
        .map(|required| {
            required
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();

    if let Some(members) = def.get("allOf").and_then(Value::as_array) {
        for member in members {
            if let Some(composed) = def_name(member).and_then(|name| raw_defs.get(name)) {
                keys.extend(required_keys_of_inner(composed, raw_defs, depth + 1));
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

#[cfg(test)]
thread_local! {
    /// Test-only capture of what `warn` would print, so the path format is
    /// asserted rather than trusted.
    static WARN_SINK: std::cell::RefCell<Option<Vec<String>>> =
        const { std::cell::RefCell::new(None) };
}

fn warn(message: &str) {
    #[cfg(test)]
    {
        let captured = WARN_SINK.with(|sink| sink.borrow().is_some());
        if captured {
            let path = message.split('\'').nth(1).unwrap_or(message).to_owned();
            WARN_SINK.with(|sink| {
                if let Some(sink) = sink.borrow_mut().as_mut() {
                    sink.push(path);
                }
            });
            return;
        }
    }
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
        let table = event_shapes();
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
            assert!(
                table.contains_key(event_type),
                "{event_type} missing from the table"
            );
        }
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

    #[test]
    fn strips_an_unknown_property_nested_in_a_message() {
        let mut warns = 0;
        let kept = capture_warns(&mut warns, || {
            enforce_event(json!({
                "type": "MESSAGES_SNAPSHOT",
                "messages": [
                    {"id": "m1", "role": "user", "content": "hi", "junkTop": 1},
                ],
            }))
            .expect("known event survives")
        });
        assert_eq!(warns, 1, "one warn, naming the nested path");
        assert!(kept["messages"][0].get("junkTop").is_none());
        assert_eq!(kept["messages"][0]["content"], "hi");
    }

    #[test]
    fn warns_with_a_pointer_path_into_nested_material() {
        let mut warns: Vec<String> = Vec::new();
        capture_warns_texts(&mut warns, || {
            enforce_event(json!({
                "type": "MESSAGES_SNAPSHOT",
                "messages": [
                    {"id": "m1", "role": "assistant", "content": "hi", "toolCalls": [
                        {"id": "c1", "type": "function", "function": {"name": "f", "arguments": "{}", "bogus": true}},
                    ]},
                ],
            }))
        });
        assert_eq!(
            warns,
            vec!["/messages/0/toolCalls/0/function/bogus".to_string()],
            "path mirrors the parent/index/key join of strip.ts:144,169"
        );
    }

    #[test]
    fn a_message_role_nothing_knows_is_removed_whole_from_its_array() {
        let mut warns = 0;
        let kept = capture_warns(&mut warns, || {
            enforce_event(json!({
                "type": "MESSAGES_SNAPSHOT",
                "messages": [
                    {"id": "m0", "role": "user", "content": "keep"},
                    {"id": "m1", "role": "future-role", "content": "gone", "junk": 2},
                ],
            }))
            .expect("known event survives")
        });
        assert_eq!(warns, 1, "only the container entry, not its inner junk");
        assert_eq!(kept["messages"].as_array().unwrap().len(), 1);
        assert_eq!(kept["messages"][0]["id"], "m0");
    }

    #[test]
    fn an_unrecognisable_required_member_collapses_to_one_report() {
        // Upstream's own case (`strip.test.ts:64-87`): a media part whose
        // source kind nothing knows is removed entire, because `source` is
        // required — the inner junk path is suppressed with it.
        let mut warns: Vec<String> = Vec::new();
        capture_warns_texts(&mut warns, || {
            enforce_event(json!({
                "type": "MESSAGES_SNAPSHOT",
                "messages": [
                    {"id": "m1", "role": "user", "junkTop": 1,
                     "content": [{"type": "image", "junk": 2, "source": {"type": "future-kind"}}]},
                ],
            }))
        });
        let mut sorted = warns.clone();
        sorted.sort();
        assert_eq!(
            sorted,
            vec![
                "/messages/0/content/0".to_string(),
                "/messages/0/junkTop".to_string()
            ]
        );
    }

    #[test]
    fn normal_nested_content_is_untouched() {
        let kept = enforce_event(json!({
            "type": "MESSAGES_SNAPSHOT",
            "messages": [
                {"id": "m1", "role": "user", "metadata": {"anything": {"deep": [1, 2]}},
                 "content": [
                     {"type": "text", "text": "hello"},
                     {"type": "image", "source": {"type": "url", "value": "https://x", "mimeType": "image/png"}},
                 ]},
                {"id": "m2", "role": "assistant", "content": "sure",
                 "toolCalls": [{"id": "c1", "type": "function",
                                "function": {"name": "f", "arguments": "{\"a\":1}"}}]},
            ],
            "timestamp": 5,
        }))
        .expect("known event survives");
        assert_eq!(kept["messages"][0]["content"][0]["text"], "hello");
        assert_eq!(
            kept["messages"][0]["content"][1]["source"]["value"],
            "https://x"
        );
        assert_eq!(kept["messages"][1]["toolCalls"][0]["function"]["name"], "f");
        assert_eq!(
            kept["messages"][0]["metadata"]["anything"]["deep"],
            json!([1, 2])
        );
    }

    #[test]
    fn patch_operations_keep_members_the_rfc_tells_them_to_ignore() {
        // RFC 6902 section 4, mirrored by upstream's specOpen marker
        // (`enforce.test.ts:255-275`): a conformant patch arrives whole.
        let mut warns = 0;
        let kept = capture_warns(&mut warns, || {
            enforce_event(json!({
                "type": "STATE_DELTA",
                "delta": [{"op": "remove", "path": "/a", "value": 1, "ext": "keep me"}],
            }))
            .expect("known event survives")
        });
        assert_eq!(warns, 0);
        assert_eq!(
            kept["delta"][0],
            json!({"op": "remove", "path": "/a", "value": 1, "ext": "keep me"})
        );
    }

    #[test]
    fn a_patch_operation_with_no_known_op_is_removed_from_the_patch() {
        let mut warns = 0;
        let kept = capture_warns(&mut warns, || {
            enforce_event(json!({
                "type": "ACTIVITY_DELTA",
                "messageId": "m1",
                "activityType": "progress",
                "patch": [
                    {"op": "add", "path": "/a", "value": 1},
                    {"op": "future-op", "path": "/b"},
                ],
            }))
            .expect("known event survives")
        });
        assert_eq!(warns, 1);
        assert_eq!(kept["patch"].as_array().unwrap().len(), 1);
        assert_eq!(kept["patch"][0]["op"], "add");
    }

    #[test]
    fn opaque_positions_pass_through_whole() {
        let kept = enforce_event(json!({
            "type": "RAW",
            "event": {"deep": {"unknown": [1, {"x": null}]}},
            "source": "provider",
            "timestamp": 1,
        }))
        .expect("known event survives");
        assert_eq!(
            kept["event"],
            json!({"deep": {"unknown": [1, {"x": null}]}})
        );

        let kept = enforce_event(json!({
            "type": "RUN_STARTED",
            "threadId": "t", "runId": "r",
            "input": {"threadId": "t", "runId": "r", "messages": [], "forwardedProps": {"any": "thing"}},
        }))
        .expect("known event survives");
        assert_eq!(kept["input"]["forwardedProps"], json!({"any": "thing"}));
    }

    #[test]
    fn an_unknown_tool_or_context_entry_is_reported_and_stripped() {
        let mut warns: Vec<String> = Vec::new();
        capture_warns_texts(&mut warns, || {
            enforce_event(json!({
                "type": "RUN_STARTED",
                "threadId": "t", "runId": "r",
                "input": {"threadId": "t", "runId": "r", "messages": [],
                          "tools": [{"name": "f", "description": "d", "wat": 1}],
                          "context": [{"description": "d", "value": "v", "extra": true}]},
            }))
        });
        let mut sorted = warns.clone();
        sorted.sort();
        assert_eq!(
            sorted,
            vec![
                "/input/context/0/extra".to_string(),
                "/input/tools/0/wat".to_string()
            ]
        );
    }

    #[test]
    fn non_object_in_a_discriminated_slot_is_left_for_serde() {
        // `strip.test.ts:35-60`: an array (or anything non-object) in a
        // discriminated-union slot is a malformed VALUE, not removable.
        let kept = enforce_event(json!({
            "type": "MESSAGES_SNAPSHOT",
            "messages": "not-an-array-of-messages",
        }))
        .expect("known event survives");
        assert_eq!(kept["messages"], "not-an-array-of-messages");

        let kept = enforce_event(json!({
            "type": "MESSAGES_SNAPSHOT",
            "messages": [{"unexpected": "shape"}],
        }))
        .expect("known event survives");
        // An object carrying no member's role is removed whole
        // (`strip.ts:221-222`), reported at the element.
        assert_eq!(kept["messages"].as_array().unwrap().len(), 0);
    }

    // -- helpers -------------------------------------------------------------

    /// Runs `f`, counting the warnings it emits.
    fn capture_warns<T>(count: &mut usize, f: impl FnOnce() -> T) -> T {
        let mut texts = Vec::new();
        let out = capture_warns_texts(&mut texts, f);
        *count = texts.len();
        out
    }

    /// Runs `f` with `warn` routed into a test sink, collecting the paths it
    /// reported. The real suppression env var still works on top.
    fn capture_warns_texts<T>(texts: &mut Vec<String>, f: impl FnOnce() -> T) -> T {
        std::env::remove_var("SUPPRESS_TRANSFORMATION_WARNINGS");
        WARN_SINK.with(|sink| *sink.borrow_mut() = Some(Vec::new()));
        let out = f();
        WARN_SINK.with(|sink| {
            if let Some(collected) = sink.borrow_mut().take() {
                *texts = collected;
            }
        });
        out
    }
}
