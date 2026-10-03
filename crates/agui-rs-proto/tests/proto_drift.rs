//! Mechanical drift guard: `src/schema.rs` must match the vendored upstream
//! `.proto` files, field for field.
//!
//! Neither side of the comparison is a hardcoded expectation list:
//!
//! * the **actual** side is a real parse of the vendored `.proto` text, and
//! * the **expected** side is a real parse of `src/schema.rs`'s own
//!   `#[prost(...)]` attributes and field declarations.
//!
//! So renumbering a field, renaming one, changing its wire type, changing
//! `repeated`, adding or dropping a field, moving a `oneof` tag, or shifting an
//! enum value all fail here. Re-vendor a new upstream commit and this test tells
//! you exactly what moved.
//!
//! It reads `schema.rs` as text rather than reflecting over the compiled types
//! because prost 0.13 dropped runtime reflection (`Message::descriptor` is
//! gone; `prost-reflect` is a separate crate and this workspace takes no new
//! dependencies). The link from `#[prost(tag = "N")]` to the bytes on the wire
//! is prost's own contract. `round_trip.rs` anchors it with literal expected
//! bytes, so the two together cover the source and the wire.
//!
//! This test exists because of a real incident: an earlier round aligned this
//! crate against a *stale cached* web copy of upstream rather than a pinned
//! commit, and shipped 0.0.5x wire numbers under a 1.0.0 label.

use std::collections::BTreeMap;
use std::path::PathBuf;

// `include_str!` bakes the vendored bytes in at compile time, so this runs in CI
// with no path assumptions. `src/schema.rs` is read at runtime only because it is
// not a dependency of anything, and its path is derived from CARGO_MANIFEST_DIR.
const EVENTS_PROTO: &str = include_str!("../upstream-spec/events.proto");
const TYPES_PROTO: &str = include_str!("../upstream-spec/types.proto");
const PATCH_PROTO: &str = include_str!("../upstream-spec/patch.proto");
const FREEZE: &str = include_str!("../upstream-spec/proto-freeze.txt");

// ---------------------------------------------------------------------------
// Shape shared by both parsers
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Field {
    number: u32,
    /// Upstream: the declared type. Ours: the normalised equivalent.
    type_name: String,
    repeated: bool,
    /// proto3 explicit presence: both sides map it to `Option<T>`, so a side
    /// flipping a field to or from `optional` changes the skip-if-none wire
    /// behaviour and must fail here even though no type changed.
    optional: bool,
}

type Fields = BTreeMap<String, Field>;

// ---------------------------------------------------------------------------
// Side A: the vendored `.proto`
// ---------------------------------------------------------------------------

fn strip_line_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for line in text.lines() {
        let line = match line.find("//") {
            Some(at) => &line[..at],
            None => line,
        };
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// Text between the braces opened at `open` (which indexes the `{`).
fn block(text: &str, open: usize) -> &str {
    let bytes = text.as_bytes();
    let mut depth = 0usize;
    for (i, b) in bytes.iter().enumerate().skip(open) {
        match b {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return &text[open + 1..i];
                }
            }
            _ => {}
        }
    }
    panic!("unbalanced braces");
}

/// The keyword and the identifier immediately preceding the `{` at `at`, e.g.
/// `("message", "ActivityDeltaEvent")` or `("pub", "struct")`.
fn decl_before(text: &str, at: usize) -> (String, String) {
    // trailing whitespace must go first: the name is a suffix of the trimmed
    // prefix, not of the raw one, or the keyword slice cuts into the name.
    let prefix = text[..at].trim_end();
    let name = prefix
        .rsplit(|c: char| !(c.is_alphanumeric() || c == '_'))
        .find(|s| !s.is_empty())
        .unwrap_or("");
    let before = &prefix[..prefix.len() - name.len()];
    let keyword = before
        .rsplit(|c: char| !(c.is_alphanumeric() || c == '_'))
        .find(|s| !s.is_empty())
        .unwrap_or("");
    (keyword.to_string(), name.to_string())
}

/// Byte offset where the declaration ending at `{` (index `at`) begins, so a
/// `message X {` / `oneof y {` header can be cut out along with its block.
fn decl_start(text: &str, at: usize) -> usize {
    let b = text.as_bytes();
    let wordy = |i: usize| b[i - 1].is_ascii_alphanumeric() || b[i - 1] == b'_';
    let mut i = at;
    while i > 0 && b[i - 1].is_ascii_whitespace() {
        i -= 1;
    }
    while i > 0 && wordy(i) {
        i -= 1;
    }
    while i > 0 && b[i - 1].is_ascii_whitespace() {
        i -= 1;
    }
    while i > 0 && wordy(i) {
        i -= 1;
    }
    i
}

#[derive(Default)]
struct ProtoFile {
    messages: BTreeMap<String, Fields>,
    /// Top-level and nested enums, keyed by name. A nested enum lives in the
    /// same map so `enum_values_match_upstream` sees it either way; proto
    /// enum names are package-wide unique in these files.
    enums: BTreeMap<String, Fields>,
}

fn parse_proto(raw: &str) -> ProtoFile {
    let text = strip_line_comments(raw);
    let mut out = ProtoFile::default();
    let mut scan = 0usize;
    while let Some(rel) = text[scan..].find('{') {
        let at = scan + rel;
        let (keyword, name) = decl_before(&text, at);
        let body = block(&text, at);
        scan = at + body.len() + 2;
        match keyword.as_str() {
            "enum" => {
                // Enum values are `NAME = 0;`, the same shape as a field with
                // no type, so they reuse `Field`.
                out.enums.insert(name, parse_bare_values(body));
            }
            "message" => {
                let (own, nested, nested_enums) = parse_proto_body(body);
                out.messages.insert(name.clone(), own);
                for (nested_name, fields) in nested {
                    out.messages.entry(nested_name).or_insert(fields);
                }
                for (enum_name, fields) in nested_enums {
                    out.enums.entry(enum_name).or_insert(fields);
                }
            }
            _ => {}
        }
    }
    out
}

/// Fields of one message body, plus any nested `message` or `enum` declared
/// inside it. `oneof` arms belong to the enclosing message, so they are merged
/// in; a nested `message` gets its own fields entry and a nested `enum` its own
/// enums entry. A nested enum that fell through to the field parser instead
/// would be dropped wholesale (`NAME = N;` has no type), so a renamed or
/// renumbered nested enum could drift unseen.
fn parse_proto_body(body: &str) -> (Fields, BTreeMap<String, Fields>, BTreeMap<String, Fields>) {
    let mut own = Fields::new();
    let mut nested = BTreeMap::new();
    let mut nested_enums = BTreeMap::new();
    // Nested `message`/`enum` blocks carry their own entries, so they are
    // removed before this body's own `;`-terminated statements are read --
    // otherwise a nested field declared before the outer ones lands in the
    // wrong message.
    let mut stripped = body.to_string();
    let mut removals: Vec<(usize, usize)> = Vec::new();
    let mut scan = 0usize;
    while let Some(rel) = stripped[scan..].find('{') {
        let at = scan + rel;
        let (keyword, name) = decl_before(&stripped, at);
        let inner = block(&stripped, at);
        let end = at + inner.len() + 2;
        scan = end;
        let start = decl_start(&stripped, at);
        match keyword.as_str() {
            // A nested message keeps its own fields; drop the whole block.
            "message" => {
                let (inner_fields, deeper, deeper_enums) = parse_proto_body(inner);
                nested.insert(name, inner_fields);
                for (n, f) in deeper {
                    nested.entry(n).or_insert(f);
                }
                for (n, f) in deeper_enums {
                    nested_enums.entry(n).or_insert(f);
                }
                removals.push((start, end));
            }
            // A nested enum is collected like a top-level one, then dropped
            // from this body so its values do not read as fields.
            "enum" => {
                nested_enums.insert(name, parse_bare_values(inner));
                removals.push((start, end));
            }
            // A oneof's arms belong to this message, so only the header goes.
            "oneof" => removals.push((start, at + 1)),
            _ => {}
        }
    }
    for (at, end) in removals.into_iter().rev() {
        stripped = format!("{}{}", &stripped[..at], &stripped[end..]);
    }
    for f in parse_field_statements(&stripped) {
        own.insert(f.0, f.1);
    }
    (own, nested, nested_enums)
}

fn parse_bare_values(body: &str) -> Fields {
    let mut out = Fields::new();
    for stmt in body.split(';') {
        let stmt = stmt.trim();
        let Some((name, value)) = stmt.rsplit_once('=') else {
            continue;
        };
        let Ok(number) = value.trim().parse::<u32>() else {
            continue;
        };
        let name = name.trim();
        if is_ident(name) {
            out.insert(
                name.to_string(),
                Field {
                    number,
                    type_name: String::new(),
                    repeated: false,
                    optional: false,
                },
            );
        }
    }
    out
}

fn is_ident(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_alphanumeric() || c == '_')
}

/// Same, but allowing the dots in fully-qualified well-known types.
fn is_type_name(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '.')
}

/// `[repeated|optional] Type name = N;` declarations. Splitting on braces as
/// well as semicolons means a `message X` or `oneof y` header (which carries no
/// `=`, and so is skipped) cannot bleed into the declaration that follows it.
fn parse_field_statements(body: &str) -> Vec<(String, Field)> {
    let mut out = Vec::new();
    for stmt in body.split([';', '{', '}']) {
        if let Some(field) = parse_field(stmt.trim()) {
            out.push(field);
        }
    }
    out
}

fn parse_field(stmt: &str) -> Option<(String, Field)> {
    let (decl, number) = stmt.rsplit_once('=')?;
    let number = number.trim().parse::<u32>().ok()?;
    let decl = decl.trim();
    let (repeated, decl) = match decl.strip_prefix("repeated") {
        Some(rest) => (true, rest.trim()),
        None => (false, decl),
    };
    // `optional` must be tracked, not just peeled: proto3 explicit presence
    // decides skip-if-none on the wire, so its loss is a real drift.
    let (optional, decl) = match decl.strip_prefix("optional") {
        Some(rest) => (true, rest.trim()),
        None => (false, decl),
    };
    let (ty, name) = decl.split_once(char::is_whitespace)?;
    let name = name.trim();
    if !is_ident(name) || !is_type_name(ty) {
        return None;
    }
    Some((
        name.to_string(),
        Field {
            number,
            type_name: ty.to_string(),
            repeated,
            optional,
        },
    ))
}

// ---------------------------------------------------------------------------
// Side B: src/schema.rs
// ---------------------------------------------------------------------------

fn schema_rs() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/schema.rs");
    strip_line_comments(&std::fs::read_to_string(&path).expect("read src/schema.rs"))
}

#[derive(Default)]
struct SchemaFile {
    /// Regular fields, keyed by message. A oneof holder is not a field.
    messages: BTreeMap<String, Fields>,
    enums: BTreeMap<String, Fields>,
    /// oneof module name -> arm name -> tag/type
    oneofs: BTreeMap<String, Fields>,
    /// message name -> the oneof module it holds
    holder: BTreeMap<String, String>,
}

fn parse_schema_rs() -> SchemaFile {
    let text = schema_rs();
    let mut out = SchemaFile::default();
    let mut scan = 0usize;
    while let Some(rel) = text[scan..].find('{') {
        let at = scan + rel;
        let (keyword, name) = decl_before(&text, at);
        let body = block(&text, at);
        scan = at + body.len() + 2;
        // The derive belonging to *this* item is whatever sits between the
        // previous item's closing brace and this one.
        let window_start = text[..at].rfind('}').map(|i| i + 1).unwrap_or(0);
        let derive = &text[window_start..at];
        match keyword.as_str() {
            "struct" if name_is(derive, "Message") => {
                let (fields, oneof) = parse_message_body(body);
                if let Some(module) = oneof {
                    out.holder.insert(name.clone(), module);
                }
                out.messages.insert(name, fields);
            }
            "enum" if name_is(derive, "prost::Enumeration") => {
                out.enums.insert(name, parse_enum_body(body));
            }
            "enum" if name_is(derive, "prost::Oneof") => {
                out.oneofs.insert(name, parse_oneof_body(body));
            }
            // `pub mod` only ever wraps a oneof in this file.
            "mod" => {
                out.oneofs.insert(name, parse_oneof_body(body));
            }
            _ => {}
        }
    }
    out
}

fn name_is(haystack: &str, needle: &str) -> bool {
    haystack
        .split(|c: char| !(c.is_alphanumeric() || c == ':' || c == '_'))
        .any(|t| t == needle)
}

/// Yields `(attribute, field_name, rust_type)` triples from a prost message
/// body. An `#[prost(...)]` attribute may wrap across lines, so a partial one
/// is accumulated until its closing `)` / `)]`.
fn prost_fields(body: &str) -> Vec<(String, String, String)> {
    let mut out = Vec::new();
    let mut ready: Option<String> = None;
    let mut acc: Option<String> = None;
    for line in body.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("#[prost(") {
            let rest = rest.trim();
            if rest.ends_with(')') || rest.ends_with(']') {
                ready = Some(rest.trim_end_matches(']').trim_end_matches(')').to_string());
            } else {
                acc = Some(rest.to_string());
            }
            continue;
        }
        if let Some(current) = acc.as_mut() {
            let piece = line.trim_end_matches(']').trim_end_matches(')').trim();
            if !piece.is_empty() {
                current.push(' ');
                current.push_str(piece);
            }
            if line.ends_with(')') || line.ends_with(']') {
                ready = acc.take().map(|c| c.trim().to_string());
            }
            continue;
        }
        let Some(rest) = line.strip_prefix("pub ") else {
            continue;
        };
        let Some((name, ty)) = rest.split_once(':') else {
            continue;
        };
        let Some(attr) = ready.take() else {
            continue;
        };
        out.push((
            attr,
            name.trim().trim_start_matches("r#").to_string(),
            ty.trim().trim_end_matches(',').trim().to_string(),
        ));
    }
    out
}

fn parse_message_body(body: &str) -> (Fields, Option<String>) {
    let mut out = Fields::new();
    let mut oneof = None;
    for (attr, name, ty) in prost_fields(body) {
        if attr.contains("oneof") {
            // `Option<event::Event>` names the module holding the arms.
            let inner = ty
                .trim_start_matches("Option<")
                .trim_end_matches('>')
                .trim();
            oneof = inner.split("::").next().map(str::to_string);
            continue;
        }
        out.insert(name, field_from_attr(&attr, &ty));
    }
    (out, oneof)
}

/// A `prost::Oneof` enum holds bare `Name(Type),` lines, so the field parser
/// used for messages does not apply.
fn parse_oneof_body(body: &str) -> Fields {
    let mut out = Fields::new();
    let mut ready: Option<String> = None;
    let mut acc: Option<String> = None;
    for line in body.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("#[prost(") {
            let rest = rest.trim();
            if rest.ends_with(')') || rest.ends_with(']') {
                ready = Some(rest.trim_end_matches(']').trim_end_matches(')').to_string());
            } else {
                acc = Some(rest.to_string());
            }
            continue;
        }
        if let Some(current) = acc.as_mut() {
            let piece = line.trim_end_matches(']').trim_end_matches(')').trim();
            if !piece.is_empty() {
                current.push(' ');
                current.push_str(piece);
            }
            if line.ends_with(')') || line.ends_with(']') {
                ready = acc.take().map(|c| c.trim().to_string());
            }
            continue;
        }
        let Some((name, ty)) = line.split_once('(') else {
            continue;
        };
        let Some(attr) = ready.take() else {
            continue;
        };
        let name = name.trim();
        if !is_ident(name) {
            continue;
        }
        let ty = ty.trim().trim_end_matches([')', ',']);
        out.insert(name.to_string(), field_from_attr(&attr, ty));
    }
    out
}

/// Turns `string, optional, tag = "3"` + `Option<String>` into a `Field`.
fn field_from_attr(attr: &str, rust_ty: &str) -> Field {
    let number = attr_tag(attr).unwrap_or_else(|| panic!("no tag in attribute: {attr}"));
    let repeated = attr.contains("repeated");
    let optional = attr.contains("optional");
    let type_name = if attr.contains("enumeration") {
        attr_quote_after(attr, "enumeration =").expect("enumeration target")
    } else if attr.contains("message") {
        message_type(rust_ty)
    } else {
        attr.split(',')
            .map(str::trim)
            .find(|t| !t.is_empty() && !t.contains('=') && *t != "optional")
            .unwrap_or("")
            .to_string()
    };
    Field {
        number,
        type_name,
        repeated,
        // A prost message field is always `Option<T>` — proto3 message presence
        // is inherent — so the `optional` keyword there carries no information.
        // Only scalar presence (`optional string`, `optional int64`) is real.
        optional: optional && !attr.contains("message"),
    }
}

fn attr_tag(attr: &str) -> Option<u32> {
    let at = attr.find("tag")?;
    let after = &attr[at..];
    let q = after.find('"')?;
    let end = after[q + 1..].find('"')?;
    after[q + 1..q + 1 + end].parse().ok()
}

fn attr_quote_after(attr: &str, key: &str) -> Option<String> {
    let at = attr.find(key)? + key.len();
    let after = &attr[at..];
    let q = after.find('"')?;
    let end = after[q + 1..].find('"')?;
    Some(after[q + 1..q + 1 + end].to_string())
}

/// The message a `#[prost(message, ...)]` field refers to, taken from the
/// field's declared Rust type. Well-known types are named as upstream names
/// them; the two hand-renamed types are mapped back.
fn message_type(rust_ty: &str) -> String {
    let inner = rust_ty
        .trim_start_matches("Option<")
        .trim_start_matches("Vec<")
        .trim_end_matches('>')
        .trim();
    match inner {
        "ProtoValue" => "google.protobuf.Value".to_string(),
        "ProtoStruct" => "google.protobuf.Struct".to_string(),
        "ToolCallFunction" => "Function".to_string(),
        "ProtoMessage" => "Message".to_string(),
        other => other.to_string(),
    }
}

fn parse_enum_body(body: &str) -> Fields {
    let mut out = Fields::new();
    for line in body.lines() {
        let line = line.trim().trim_end_matches(',');
        let Some((name, value)) = line.rsplit_once('=') else {
            continue;
        };
        let Ok(number) = value.trim().parse::<u32>() else {
            continue;
        };
        let name = name.trim();
        if is_ident(name) {
            out.insert(
                name.to_string(),
                Field {
                    number,
                    type_name: String::new(),
                    repeated: false,
                    optional: false,
                },
            );
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Aliases
// ---------------------------------------------------------------------------

/// Upstream names our schema deliberately spells differently, and vice versa.
/// Both are wire-identical; the notes on each type in `schema.rs` say why.
fn alias(upstream: &str) -> &str {
    match upstream {
        "Function" => "ToolCallFunction",
        // one shared type for four identical upstream messages
        "ImageInputPart" | "AudioInputPart" | "VideoInputPart" | "DocumentInputPart" => {
            "MediaInputPart"
        }
        "Message" => "ProtoMessage",
        other => other,
    }
}

/// Set of upstream messages our single `MediaInputPart` stands in for.
fn is_media_part_alias(name: &str) -> bool {
    matches!(
        name,
        "ImageInputPart" | "AudioInputPart" | "VideoInputPart" | "DocumentInputPart"
    )
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

fn merged_proto() -> ProtoFile {
    let mut out = ProtoFile::default();
    for text in [EVENTS_PROTO, TYPES_PROTO, PATCH_PROTO] {
        let parsed = parse_proto(text);
        out.messages.extend(parsed.messages);
        out.enums.extend(parsed.enums);
    }
    out
}

/// `text_message_start` and `TextMessageStart` are the same oneof arm; prost
/// keeps whatever case the variant is written in, the .proto fixes snake_case.
fn fold(name: &str) -> String {
    name.to_lowercase().replace('_', "")
}

/// Field equality, with the type name put through the same alias table as the
/// message names: our oneof arms point at `MediaInputPart` where upstream names
/// four separate media messages.
///
/// The `optional` keyword is compared only for scalar fields. A prost message
/// field is always `Option<T>` — proto3 message presence is inherent — so the
/// keyword there carries no information on our side, while the .proto spells it
/// on some message fields (`optional RunAgentInput input`) and not others.
/// Holding both to the same spelling would flag every message field either way;
/// scalar presence (`optional string`, `optional int64`) is the real signal,
/// and that is what `same_field` guards.
fn same_field(got: &Field, want: &Field) -> bool {
    got.number == want.number
        && got.repeated == want.repeated
        && (got.optional == want.optional || !is_scalar(&got.type_name))
        && alias(&got.type_name) == alias(&want.type_name)
}

/// Types whose presence the `optional` keyword actually decides: primitives and
/// enums. Anything else — declared messages and the google well-knowns, both
/// spelled with a dot or a known alias — has inherent presence.
fn is_scalar(type_name: &str) -> bool {
    let known_scalars = [
        "string", "bytes", "bool", "float", "double", "int32", "int64", "uint32", "uint64",
        "sint32", "sint64", "fixed32", "fixed64", "sfixed32", "sfixed64",
    ];
    known_scalars.contains(&type_name)
}

fn fold_fields(fields: &Fields) -> BTreeMap<String, Field> {
    fields.iter().map(|(k, v)| (fold(k), v.clone())).collect()
}

/// The fields of one upstream message as *our* schema spells them: the message's
/// own fields, or — when upstream's fields are all oneof arms — the arms of the
/// module we hold them in.
fn our_view(schema: &SchemaFile, upstream_name: &str) -> Option<Fields> {
    let target = alias(upstream_name);
    if let Some(module) = schema.holder.get(target) {
        return Some(fold_fields(schema.oneofs.get(module)?));
    }
    schema.messages.get(target).cloned()
}

/// Guards against the parsers silently degrading: a parser that stopped
/// recognising `message` would leave the tests below comparing near-empty maps
/// and passing. The freeze test independently pins the parse to 298 entries.
#[test]
fn the_parsers_see_the_whole_schema() {
    let upstream = merged_proto();
    assert_eq!(upstream.messages.len(), 53, "upstream message count");
    // Top-level enums plus every nested one (`ToolCall.Function`-style nesting
    // included): the count must see what `parse_proto_body` lifts out.
    assert_eq!(upstream.enums.len(), 2, "upstream enum count");
    let ours = parse_schema_rs();
    assert_eq!(
        ours.messages.len() + ours.holder.len(),
        53,
        "our message count: {}",
        ours.messages.len() + ours.holder.len()
    );
    assert_eq!(ours.enums.len(), 2, "our enum count");
    assert_eq!(ours.oneofs.len(), 3, "our oneof module count");
    assert_eq!(ours.holder.len(), 3, "our oneof holder count");
}

#[test]
fn every_proto_message_is_mirrored_by_our_schema() {
    let upstream = merged_proto();
    let ours = parse_schema_rs();
    for (name, expected) in &upstream.messages {
        let target = alias(name);
        let actual = our_view(&ours, name)
            .unwrap_or_else(|| panic!("upstream message {name} has no counterpart ({target})"));
        if is_media_part_alias(name) {
            continue; // covered by its own test
        }
        let folded: BTreeMap<String, Field> = if ours.holder.contains_key(target) {
            expected.iter().map(|(k, v)| (fold(k), v.clone())).collect()
        } else {
            expected.clone()
        };
        let expected = &folded;
        for (field, want) in expected {
            let got = actual
                .get(field)
                .unwrap_or_else(|| panic!("{target}.{field} missing from our schema"));
            assert!(
                same_field(got, want),
                "{target}.{field}: ours {got:?}, upstream {want:?}"
            );
        }
    }
}

#[test]
fn no_extra_fields_where_upstream_has_fewer() {
    let upstream = merged_proto();
    let ours = parse_schema_rs();
    for (name, expected) in &upstream.messages {
        if is_media_part_alias(name) {
            continue;
        }
        let target = alias(name);
        let actual = our_view(&ours, name).expect("covered above");
        let expected: BTreeMap<String, Field> = if ours.holder.contains_key(target) {
            expected.iter().map(|(k, v)| (fold(k), v.clone())).collect()
        } else {
            expected.clone()
        };
        let extra: Vec<&String> = actual
            .keys()
            .filter(|k| !expected.contains_key(*k))
            .collect();
        assert!(
            extra.is_empty(),
            "{target} has fields upstream lacks: {extra:?}"
        );
    }
}

#[test]
fn the_shared_media_part_matches_every_upstream_media_message() {
    let upstream = merged_proto();
    let ours = parse_schema_rs();
    for name in [
        "ImageInputPart",
        "AudioInputPart",
        "VideoInputPart",
        "DocumentInputPart",
    ] {
        // Presence on a message-typed field is inherent, so only scalar
        // `optional` is comparable — same rule as `same_field`.
        let normalise = |fields: Option<&Fields>| -> Option<Fields> {
            fields.map(|fs| {
                fs.iter()
                    .map(|(k, v)| {
                        let mut v = v.clone();
                        if !is_scalar(&v.type_name) {
                            v.optional = false;
                        }
                        (k.clone(), v)
                    })
                    .collect()
            })
        };
        assert_eq!(
            normalise(ours.messages.get("MediaInputPart")),
            normalise(upstream.messages.get(name)),
            "{name}"
        );
    }
}

#[test]
fn our_schema_invents_no_message_upstream_does_not_have() {
    let upstream = merged_proto();
    let ours = parse_schema_rs();
    let known: Vec<&str> = upstream.messages.keys().map(|k| alias(k)).collect();
    let mut declared: Vec<&String> = ours.messages.keys().collect();
    declared.extend(ours.holder.keys());
    for name in declared {
        assert!(
            known.contains(&name.as_str()),
            "our message {name} is not in the vendored .proto"
        );
    }
}

#[test]
fn enum_values_match_upstream() {
    let upstream = merged_proto();
    let ours = parse_schema_rs();
    for (name, expected) in &upstream.enums {
        let actual = ours
            .enums
            .get(name)
            .unwrap_or_else(|| panic!("enum {name} missing from our schema"));
        // prost derives `TEXT_MESSAGE_START` as `TextMessageStart`; the two
        // spell the same value, so compare them folded.
        let fold = |k: &str| k.to_uppercase().replace('_', "");
        let ours_values: BTreeMap<String, u32> =
            actual.iter().map(|(k, v)| (fold(k), v.number)).collect();
        let theirs: BTreeMap<String, u32> =
            expected.iter().map(|(k, v)| (fold(k), v.number)).collect();
        assert_eq!(ours_values, theirs, "enum {name} values");
    }
}

#[test]
fn the_event_oneof_covers_all_thirty_one_tags() {
    let ours = parse_schema_rs();
    assert_eq!(ours.holder.get("Event").map(String::as_str), Some("event"));
    let event = ours.oneofs.get("event").expect("event oneof");
    let mut tags: Vec<u32> = event.values().map(|f| f.number).collect();
    tags.sort_unstable();
    assert_eq!(tags, (1..=31).collect::<Vec<u32>>(), "Event oneof tags");
}

#[test]
fn proto_freeze_agrees_with_the_proto_files() {
    // The freeze is the authority for numbers. If the .proto and the freeze ever
    // disagree, the vendored copy is inconsistent and nothing built on it can be
    // trusted.
    let upstream = merged_proto();
    let mut checked = 0usize;
    for line in FREEZE.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((path, number)) = line.rsplit_once(" = ") else {
            continue;
        };
        let parts: Vec<&str> = path.split('.').collect();
        let (owner, field) = (parts[0], parts[parts.len() - 1]);
        // `ToolCall.Function.name` names a nested message, which the parser
        // hoists to the top level under its own name.
        let fields = parts
            .get(1)
            .and_then(|n| upstream.messages.get(*n))
            .or_else(|| upstream.messages.get(owner))
            .or_else(|| upstream.enums.get(owner))
            .unwrap_or_else(|| panic!("freeze names unknown message or enum {owner}"));
        let entry = fields
            .get(field)
            .unwrap_or_else(|| panic!("freeze names unknown field {path}"));
        assert_eq!(
            entry.number.to_string(),
            number.trim(),
            "{path}: freeze says {number}, .proto says {}",
            entry.number
        );
        checked += 1;
    }
    assert!(
        checked > 250,
        "freeze only yielded {checked} entries; the parser regressed"
    );
}
