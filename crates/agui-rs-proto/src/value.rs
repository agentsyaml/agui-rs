//! Conversions between `serde_json::Value` and `prost_types::Value`
//! (`google.protobuf.Value`).

use prost_types::{value::Kind, ListValue, Struct, Value as ProtoValue};
use serde_json::{Map, Number, Value as JsonValue};

/// Converts a `serde_json::Value` into a `google.protobuf.Value`.
pub fn json_to_proto(value: &JsonValue) -> ProtoValue {
    let kind = match value {
        JsonValue::Null => Kind::NullValue(0),
        JsonValue::Bool(b) => Kind::BoolValue(*b),
        JsonValue::Number(n) => Kind::NumberValue(n.as_f64().unwrap_or(0.0)),
        JsonValue::String(s) => Kind::StringValue(s.clone()),
        JsonValue::Array(items) => Kind::ListValue(ListValue {
            values: items.iter().map(json_to_proto).collect(),
        }),
        JsonValue::Object(map) => Kind::StructValue(Struct {
            fields: map
                .iter()
                .map(|(k, v)| (k.clone(), json_to_proto(v)))
                .collect(),
        }),
    };
    ProtoValue { kind: Some(kind) }
}

/// Converts a `google.protobuf.Value` back into a `serde_json::Value`.
pub fn proto_to_json(value: &ProtoValue) -> JsonValue {
    match &value.kind {
        None | Some(Kind::NullValue(_)) => JsonValue::Null,
        Some(Kind::BoolValue(b)) => JsonValue::Bool(*b),
        Some(Kind::NumberValue(n)) => Number::from_f64(*n)
            .map(JsonValue::Number)
            .unwrap_or(JsonValue::Null),
        Some(Kind::StringValue(s)) => JsonValue::String(s.clone()),
        Some(Kind::ListValue(list)) => {
            JsonValue::Array(list.values.iter().map(proto_to_json).collect())
        }
        Some(Kind::StructValue(strukt)) => {
            let mut map = Map::new();
            for (k, v) in &strukt.fields {
                map.insert(k.clone(), proto_to_json(v));
            }
            JsonValue::Object(map)
        }
    }
}

/// Converts a JSON value into a `google.protobuf.Struct`.
///
/// Seven upstream fields are typed `Struct` rather than `Value` — every
/// `metadata`, plus `Message.activity_content` and `ActivitySnapshotEvent.
/// content`. The two are *not* wire-interchangeable: a `Value` carrying an
/// object puts its contents in field 7, which a peer reading a `Struct` (field
/// 1, a map entry) drops on the floor. Anything that is not an object encodes as
/// an empty struct.
pub fn json_to_struct(value: &JsonValue) -> Struct {
    match json_to_proto(value).kind {
        Some(Kind::StructValue(strukt)) => strukt,
        _ => Struct::default(),
    }
}

/// Converts a `google.protobuf.Struct` back into a JSON object.
pub fn struct_to_json(value: &Struct) -> JsonValue {
    proto_to_json(&ProtoValue {
        kind: Some(Kind::StructValue(value.clone())),
    })
}
