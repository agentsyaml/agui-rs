//! Protobuf binary encoding for AG-UI events.
//!
//! Mirrors the canonical TypeScript `@ag-ui/proto` package: encodes/decodes the
//! 16 base event variants plus the two chunk variants defined in `events.proto`.
//! Reasoning / activity / thinking events are **not** part of the protobuf
//! schema upstream and are rejected by [`encode`] with [`AgUiError::Unsupported`].
//!
//! The binary media type is [`AGUI_MEDIA_TYPE_PROTOBUF`].

mod convert;
mod schema;
mod value;

pub use agui_rs_core::AGUI_MEDIA_TYPE_PROTOBUF;
pub use convert::{decode, encode};
pub use value::{json_to_proto, proto_to_json};
