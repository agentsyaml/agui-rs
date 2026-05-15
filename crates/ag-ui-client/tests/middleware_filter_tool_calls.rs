#![allow(dead_code)]
#![allow(unused_imports)]

#[path = "../src/middleware.rs"]
mod middleware;

use ag_ui_core::{factory, BaseEventFields, Event, RunFinishedEvent, ToolCallResultEvent, ToolResultRole};
use futures::{stream, StreamExt};
use middleware::filter_tool_calls::{FilterToolCallsConfig, FilterToolCallsMiddleware};
use middleware::Middleware as _;
use std::sync::Arc;

fn tool_events(tool_call_id: &str, tool_name: &str) -> Vec<Event> {
    vec![
        factory::tool_call_start(tool_call_id, tool_name),
        factory::tool_call_args(tool_call_id, "{}"),
        factory::tool_call_end(tool_call_id),
        Event::ToolCallResult(ToolCallResultEvent {
            message_id: format!("tool-message-{tool_call_id}"),
            tool_call_id: tool_call_id.into(),
            content: format!("result-{tool_name}"),
            role: Some(ToolResultRole::Tool),
            base: BaseEventFields::default(),
        }),
    ]
}

async fn run_filter(middleware: FilterToolCallsMiddleware) -> Vec<Event> {
    let mut events = vec![factory::run_started("test-thread", "test-run")];
    events.extend(tool_events("tool-call-1", "calculator"));
    events.extend(tool_events("tool-call-2", "weather"));
    events.extend(tool_events("tool-call-3", "search"));
    events.push(Event::RunFinished(RunFinishedEvent {
        thread_id: "test-thread".into(),
        run_id: "test-run".into(),
        result: None,
        outcome: None,
        base: BaseEventFields::default(),
    }));

    let terminal: middleware::TerminalFn = Arc::new(move |_input| {
        let events = events.clone();
        Box::pin(async move { Ok(Box::pin(stream::iter(events.into_iter().map(Ok))) as middleware::EventStream) })
    });

    let stream = middleware
        .run(
            middleware::MiddlewareInput::from(ag_ui_core::RunAgentInput::new("test-thread", "test-run")),
            terminal,
        )
        .await
        .expect("middleware should succeed");

    stream
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .collect::<ag_ui_core::Result<Vec<_>>>()
        .expect("stream should succeed")
}

#[tokio::test]
async fn filters_out_disallowed_tool_calls() {
    let middleware = FilterToolCallsMiddleware::new(FilterToolCallsConfig::disallowed(vec![
        "calculator".into(),
        "search".into(),
    ]));

    let events = run_filter(middleware).await;

    assert_eq!(events.len(), 6);
    assert!(matches!(events.first(), Some(Event::RunStarted(_))));
    assert!(matches!(events.last(), Some(Event::RunFinished(_))));

    let starts = events
        .iter()
        .filter_map(|event| match event {
            Event::ToolCallStart(start) => Some(start.tool_call_name.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(starts, vec!["weather"]);
}

#[tokio::test]
async fn allowlist_keeps_only_allowed_tool_calls() {
    let middleware = FilterToolCallsMiddleware::new(FilterToolCallsConfig::allowed(vec![
        "calculator".into(),
    ]));

    let events = run_filter(middleware).await;

    assert_eq!(events.len(), 6);
    let starts = events
        .iter()
        .filter_map(|event| match event {
            Event::ToolCallStart(start) => Some(start.tool_call_name.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(starts, vec!["calculator"]);
}
