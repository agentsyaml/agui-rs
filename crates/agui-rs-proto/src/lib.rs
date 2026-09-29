//! Protobuf binary encoding for AG-UI events.
//!
//! Mirrors the canonical TypeScript `@ag-ui/proto` package: encodes/decodes all
//! 31 event variants defined in `upstream-spec/events.proto`, including the
//! tool-call-result, activity and reasoning ones. `schema.rs` is verified against
//! the vendored upstream `.proto` by `tests/proto_drift.rs`.
//!
//! The binary media type is [`AGUI_MEDIA_TYPE_PROTOBUF`].

mod convert;
mod schema;
mod value;

pub use agui_rs_core::AGUI_MEDIA_TYPE_PROTOBUF;
pub use convert::{decode, encode};
pub use value::{json_to_proto, proto_to_json};
