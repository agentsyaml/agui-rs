//! Axum-oriented server helpers for implementing AG-UI agents.
//!
//! This crate provides:
//! - [`RunHandler`] for converting [`agui_rs_core::RunAgentInput`] into a stream of AG-UI events.
//! - [`EventEmitter`] channel helpers for producing typed event streams.
//! - [`sse_body`] for turning AG-UI event streams into SSE bodies.
//! - [`agui_route`] / [`agui_router`] for axum integration.

pub mod axum;
pub mod emit;
pub mod error;
pub mod handler;
pub mod sse;

pub use crate::axum::{agui_route, agui_router};
pub use crate::emit::{channel, EventEmitter, EventSink};
pub use crate::error::{AgUiError, Result};
pub use crate::handler::{RunContext, RunHandler};
pub use crate::sse::sse_body;
