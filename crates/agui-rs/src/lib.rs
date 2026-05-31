//! `agui-rs` — facade crate for the AG-UI Rust SDK.
//!
//! Pick what you need via Cargo features:
//! - `core` (default): protocol types & events
//! - `encoder`: SSE / protobuf encoding
//! - `client`: HTTP client with `HttpAgent`, `AgentRunner`
//! - `server`: axum server integration
//! - `full`: client + server + encoder
//!
//! ```toml
//! # Just the client
//! agui-rs = { version = "0.1", features = ["client"] }
//!
//! # Just the server
//! agui-rs = { version = "0.1", features = ["server"] }
//!
//! # Everything
//! agui-rs = { version = "0.1", features = ["full"] }
//! ```
//!
//! ```ignore
//! // Requires the `client` feature: agui-rs = { version = "0.1", features = ["client"] }
//! use agui_rs::client::{HttpAgent, AgentRunner};
//! use agui_rs::prelude::*;
//! ```

#![doc(html_root_url = "https://docs.rs/agui-rs/0.1.1")]
#![cfg_attr(docsrs, feature(doc_cfg))]

#[cfg(feature = "core")]
#[cfg_attr(docsrs, doc(cfg(feature = "core")))]
pub use agui_rs_core as core;

#[cfg(feature = "encoder")]
#[cfg_attr(docsrs, doc(cfg(feature = "encoder")))]
pub use agui_rs_encoder as encoder;

#[cfg(feature = "proto")]
#[cfg_attr(docsrs, doc(cfg(feature = "proto")))]
pub use agui_rs_proto as proto;

#[cfg(feature = "client")]
#[cfg_attr(docsrs, doc(cfg(feature = "client")))]
pub use agui_rs_client as client;

#[cfg(feature = "server")]
#[cfg_attr(docsrs, doc(cfg(feature = "server")))]
pub use agui_rs_server as server;

/// Commonly used types. Requires the `core` feature (enabled by default).
#[cfg(feature = "core")]
#[cfg_attr(docsrs, doc(cfg(feature = "core")))]
pub mod prelude {
    pub use agui_rs_core::{
        AgUiError, Event, Message, Result, RunAgentInput, State, Tool, ToolCall,
    };
}
