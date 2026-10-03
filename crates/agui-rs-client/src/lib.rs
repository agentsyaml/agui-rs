//! Async AG-UI client primitives.
//!
//! The crate exposes:
//! - [`Agent`] implementations returning `futures::Stream<Item = agui_rs_core::Result<Event>>`
//! - [`AgentRunner`] which expands chunk events, verifies ordering, applies message/state updates,
//!   and notifies [`AgentSubscriber`] hooks
//! - [`HttpAgent`] for HTTP + SSE execution against AG-UI compatible endpoints

mod agent;
mod apply;
mod chunks;
pub mod compact;
mod compat;
pub mod debug_logger;
pub mod enforce;
mod error;
mod http;
pub mod interrupts;
pub mod legacy;
mod middleware;
mod subscriber;
mod transform;
mod verify;
mod version;

pub use agent::{
    AbortHandle, Agent, AgentConfig, AgentRunner, RunAgentParameters, RunAgentResult, Subscription,
};
pub use apply::{default_apply_events, with_authoritative_activity_types, AppliedEvent};
pub use chunks::expand_chunks;
pub use compact::compact_events;
pub use debug_logger::DebugLogger;
pub use error::{AgUiError, Result};
pub use http::{HttpAgent, HttpAgentConfig, HttpRequestExecutor};
pub use interrupts::{
    ensure_resume_covers, get_run_outcome, interrupt_is_expired, is_interrupt_expired,
    ResumeResponse,
};
pub use legacy::convert_legacy_events;
pub use middleware::{
    filter_tool_calls::{FilterToolCallsConfig, FilterToolCallsMiddleware},
    Middleware, MiddlewareChain,
};
pub use subscriber::{
    ActivityDeltaContext, ActivitySnapshotContext, AgentSubscriber, EventContext, InterruptContext,
    NewMessageContext, NewToolCallContext, ReasoningContentContext, ReasoningEndContext,
    RunContext, RunFailedContext, RunFinishedContext, TextContentContext, TextEndContext,
    ToolCallArgsContext, ToolCallEndContext, ToolCallResultContext,
};
pub use transform::{
    detect_stream_format, parse_proto_stream, parse_sse_stream, StreamFormat,
    AGUI_MEDIA_TYPE_PROTOBUF, AGUI_MEDIA_TYPE_SSE,
};
pub use verify::verify_events;
pub use version::{
    compare_declared_protocol, compare_versions, warn_on_producer_declaration, DeclaredVerdict,
    PROTOCOL_VERSION,
};

pub use agui_rs_core::{Event, Message, RunAgentInput, State};
