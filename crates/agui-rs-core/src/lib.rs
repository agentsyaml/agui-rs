//! Core types, events, and errors for the AG-UI protocol.
//!
//! This crate is transport-agnostic and provides:
//! - Strongly-typed [`Event`] enum mirroring the AG-UI event surface (33 active variants).
//! - Domain types ([`Message`], [`Tool`], [`RunAgentInput`], etc.).
//! - A unified [`AgUiError`] / [`Result`] alias.

pub mod capabilities;
pub mod error;
pub mod event_factories;
pub mod events;
pub mod types;

pub use capabilities::Capabilities;
pub use error::{AgUiError, Result};
pub use events::{
    factory, ActivityDeltaEvent, ActivitySnapshotEvent, BaseEventFields, CustomEvent, Event,
    EventType, MessagesSnapshotEvent, RawEvent, ReasoningEncryptedValueEvent,
    ReasoningEncryptedValueSubtype, ReasoningEndEvent, ReasoningMessageChunkEvent,
    ReasoningMessageContentEvent, ReasoningMessageEndEvent, ReasoningMessageRole,
    ReasoningMessageStartEvent, ReasoningStartEvent, RunErrorEvent, RunFinishedEvent,
    RunFinishedOutcome, RunStartedEvent, StateDeltaEvent, StateSnapshotEvent, StepFinishedEvent,
    StepStartedEvent, TextMessageChunkEvent, TextMessageContentEvent, TextMessageEndEvent,
    TextMessageStartEvent, ThinkingEndEvent, ThinkingStartEvent, ThinkingTextMessageContentEvent,
    ThinkingTextMessageEndEvent, ThinkingTextMessageStartEvent, ToolCallArgsEvent,
    ToolCallChunkEvent, ToolCallEndEvent, ToolCallResultEvent, ToolCallStartEvent, ToolResultRole,
};
pub use types::{
    AgentCapabilities, BinaryInputContent, Context, FunctionCall, InputContent, InputContentSource,
    Interrupt, Message, ResumeEntry, ResumeStatus, Role, RunAgentInput, State, TextMessageRole,
    Tool, ToolCall, ToolCallKind, UserMessageContent,
};
