#![allow(dead_code)]
#![allow(unused_imports)]

#[path = "../src/middleware.rs"]
mod middleware;

use agui_rs_core::{
    factory, BaseEventFields, Event, ReasoningMessageRole, RunAgentInput, ThinkingEndEvent,
    ThinkingStartEvent, ThinkingTextMessageContentEvent, ThinkingTextMessageEndEvent,
    ThinkingTextMessageStartEvent,
};
use futures::{stream, StreamExt};
use middleware::Middleware as _;
use std::sync::Arc;

async fn run(events: Vec<Event>) -> Vec<Event> {
    let middleware = middleware::backward_compat::BackwardCompat0_0_45;
    let terminal: middleware::TerminalFn = Arc::new(move |_input| {
        let events = events.clone();
        Box::pin(async move {
            Ok(Box::pin(stream::iter(events.into_iter().map(Ok))) as middleware::EventStream)
        })
    });

    middleware
        .run(
            middleware::MiddlewareInput::from(RunAgentInput::new("thread-1", "run-1")),
            terminal,
        )
        .await
        .expect("middleware should succeed")
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .collect::<agui_rs_core::Result<Vec<_>>>()
        .expect("stream should succeed")
}

#[tokio::test]
async fn transforms_thinking_start_and_end_into_reasoning_events() {
    let events = run(vec![
        Event::ThinkingStart(ThinkingStartEvent {
            title: Some("Processing...".into()),
            base: BaseEventFields::default(),
        }),
        Event::ThinkingEnd(ThinkingEndEvent {
            base: BaseEventFields::default(),
        }),
    ])
    .await;

    match (&events[0], &events[1]) {
        (Event::ReasoningStart(start), Event::ReasoningEnd(end)) => {
            assert!(!start.message_id.is_empty());
            assert_eq!(start.message_id, end.message_id);
        }
        other => panic!("expected reasoning start/end, got {other:?}"),
    }
}

#[tokio::test]
async fn transforms_thinking_message_lifecycle_into_reasoning_message_events() {
    let events = run(vec![
        Event::ThinkingTextMessageStart(ThinkingTextMessageStartEvent {
            base: BaseEventFields::default(),
        }),
        Event::ThinkingTextMessageContent(ThinkingTextMessageContentEvent {
            delta: "thinking...".into(),
            base: BaseEventFields::default(),
        }),
        Event::ThinkingTextMessageEnd(ThinkingTextMessageEndEvent {
            base: BaseEventFields::default(),
        }),
    ])
    .await;

    match (&events[0], &events[1], &events[2]) {
        (
            Event::ReasoningMessageStart(start),
            Event::ReasoningMessageContent(content),
            Event::ReasoningMessageEnd(end),
        ) => {
            assert_eq!(start.role, ReasoningMessageRole::Reasoning);
            assert_eq!(start.message_id, content.message_id);
            assert_eq!(content.message_id, end.message_id);
            assert_eq!(content.delta, "thinking...");
        }
        other => panic!("expected reasoning message lifecycle, got {other:?}"),
    }
}

#[tokio::test]
async fn passes_through_non_thinking_events_unchanged() {
    let original = vec![
        factory::text_message_start("msg-1"),
        factory::text_message_content("msg-1", "Hello"),
        factory::text_message_end("msg-1"),
    ];
    let events = run(original.clone()).await;
    assert_eq!(events, original);
}

#[tokio::test]
async fn transforms_complete_thinking_flow_and_keeps_ids_stable() {
    let events = run(vec![
        Event::ThinkingStart(ThinkingStartEvent {
            title: Some("Analyzing".into()),
            base: BaseEventFields::default(),
        }),
        Event::ThinkingTextMessageStart(ThinkingTextMessageStartEvent {
            base: BaseEventFields::default(),
        }),
        Event::ThinkingTextMessageContent(ThinkingTextMessageContentEvent {
            delta: "Step 1...".into(),
            base: BaseEventFields::default(),
        }),
        Event::ThinkingTextMessageContent(ThinkingTextMessageContentEvent {
            delta: "Step 2...".into(),
            base: BaseEventFields::default(),
        }),
        Event::ThinkingTextMessageEnd(ThinkingTextMessageEndEvent {
            base: BaseEventFields::default(),
        }),
        Event::ThinkingEnd(ThinkingEndEvent {
            base: BaseEventFields::default(),
        }),
    ])
    .await;

    assert_eq!(
        events.iter().map(Event::event_type).collect::<Vec<_>>(),
        vec![
            agui_rs_core::EventType::ReasoningStart,
            agui_rs_core::EventType::ReasoningMessageStart,
            agui_rs_core::EventType::ReasoningMessageContent,
            agui_rs_core::EventType::ReasoningMessageContent,
            agui_rs_core::EventType::ReasoningMessageEnd,
            agui_rs_core::EventType::ReasoningEnd,
        ]
    );

    let reasoning_id = match &events[0] {
        Event::ReasoningStart(start) => start.message_id.clone(),
        other => panic!("expected reasoning start, got {other:?}"),
    };
    let reasoning_message_id = match &events[1] {
        Event::ReasoningMessageStart(start) => start.message_id.clone(),
        other => panic!("expected reasoning message start, got {other:?}"),
    };
    assert_ne!(reasoning_id, reasoning_message_id);
    assert!(events
        .iter()
        .any(|event| matches!(event, Event::ReasoningEnd(end) if end.message_id == reasoning_id)));
    assert!(events.iter().any(|event| matches!(event, Event::ReasoningMessageEnd(end) if end.message_id == reasoning_message_id)));
}

#[tokio::test]
async fn handles_mixed_thinking_and_regular_events() {
    let events = run(vec![
        factory::run_started("t1", "r1"),
        Event::ThinkingStart(ThinkingStartEvent {
            title: None,
            base: BaseEventFields::default(),
        }),
        Event::ThinkingTextMessageStart(ThinkingTextMessageStartEvent {
            base: BaseEventFields::default(),
        }),
        Event::ThinkingTextMessageContent(ThinkingTextMessageContentEvent {
            delta: "thinking".into(),
            base: BaseEventFields::default(),
        }),
        Event::ThinkingTextMessageEnd(ThinkingTextMessageEndEvent {
            base: BaseEventFields::default(),
        }),
        Event::ThinkingEnd(ThinkingEndEvent {
            base: BaseEventFields::default(),
        }),
        factory::text_message_start("msg-1"),
        factory::text_message_content("msg-1", "Response"),
        factory::text_message_end("msg-1"),
        factory::run_finished("t1", "r1"),
    ])
    .await;

    assert_eq!(events.len(), 10);
    assert!(matches!(events[0], Event::RunStarted(_)));
    assert!(matches!(events[1], Event::ReasoningStart(_)));
    assert!(matches!(events[6], Event::TextMessageStart(_)));
    assert!(matches!(events[9], Event::RunFinished(_)));
}

// SKIPPED: warnAboutTransformation should not throw when process is undefined: Rust implementation uses std::env directly and does not model browser-global `process`.
// SKIPPED: automatically transforms THINKING events when maxVersion <= 0.0.45: Rust tests target the middleware submodule directly; there is no auto-insertion layer to exercise here.
