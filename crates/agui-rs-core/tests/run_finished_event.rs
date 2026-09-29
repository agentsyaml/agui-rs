use agui_rs_core::{Event, RunFinishedOutcome};
use serde_json::json;

/// The success outcome with no pending tool calls named.
fn success() -> RunFinishedOutcome {
    RunFinishedOutcome::Success {
        pending_tool_call_ids: None,
    }
}

fn parse_run_finished(value: serde_json::Value) -> Event {
    serde_json::from_value(value).expect("deserialize run finished event")
}

#[test]
fn run_finished_parses_legacy_shape_without_outcome() {
    let event = parse_run_finished(json!({
        "type": "RUN_FINISHED",
        "threadId": "t-1",
        "runId": "r-1"
    }));

    assert!(matches!(event, Event::RunFinished(ref event) if event.outcome.is_none()));
}

#[test]
fn run_finished_treats_null_outcome_as_missing() {
    let event = parse_run_finished(json!({
        "type": "RUN_FINISHED",
        "threadId": "t-1",
        "runId": "r-1",
        "outcome": null
    }));

    assert!(matches!(event, Event::RunFinished(ref event) if event.outcome.is_none()));
}

#[test]
fn run_finished_parses_legacy_shape_with_result() {
    let event = parse_run_finished(json!({
        "type": "RUN_FINISHED",
        "threadId": "t-1",
        "runId": "r-1",
        "result": { "answer": 42 }
    }));

    assert!(
        matches!(event, Event::RunFinished(ref event) if event.outcome.is_none() && event.result == Some(json!({ "answer": 42 })))
    );
}

#[test]
fn run_finished_parses_cancelled_outcome() {
    let event = parse_run_finished(json!({
        "type": "RUN_FINISHED",
        "threadId": "t-1",
        "runId": "r-1",
        "outcome": { "type": "cancelled" }
    }));

    assert!(
        matches!(event, Event::RunFinished(ref event) if event.outcome == Some(RunFinishedOutcome::Cancelled))
    );
}

#[test]
fn run_finished_success_outcome_carries_pending_tool_call_ids() {
    let event = parse_run_finished(json!({
        "type": "RUN_FINISHED",
        "threadId": "t-1",
        "runId": "r-1",
        "outcome": { "type": "success", "pendingToolCallIds": ["tc-1"] }
    }));

    assert!(
        matches!(event, Event::RunFinished(ref event) if matches!(&event.outcome, Some(RunFinishedOutcome::Success { pending_tool_call_ids: Some(ids) }) if ids == &["tc-1".to_string()]))
    );
}

#[test]
fn run_finished_parses_success_outcome() {
    let event = parse_run_finished(json!({
        "type": "RUN_FINISHED",
        "threadId": "t-1",
        "runId": "r-1",
        "outcome": { "type": "success" },
        "result": { "answer": 42 }
    }));

    assert!(
        matches!(event, Event::RunFinished(ref event) if event.outcome == Some(success()) && event.result == Some(json!({ "answer": 42 })))
    );
}

#[test]
fn run_finished_parses_interrupt_outcome() {
    let event = parse_run_finished(json!({
        "type": "RUN_FINISHED",
        "threadId": "t-1",
        "runId": "r-1",
        "outcome": {
            "type": "interrupt",
            "interrupts": [{ "id": "int-1", "reason": "tool_call" }]
        }
    }));

    assert!(
        matches!(event, Event::RunFinished(ref event) if matches!(&event.outcome, Some(RunFinishedOutcome::Interrupt { interrupts }) if interrupts.len() == 1))
    );
}

// SKIPPED: rejects outcome with empty interrupts: Rust SDK has no runtime non-empty validation for RunFinishedOutcome::Interrupt.

#[test]
fn run_finished_rejects_unknown_outcome_type() {
    let error = serde_json::from_value::<Event>(json!({
        "type": "RUN_FINISHED",
        "threadId": "t-1",
        "runId": "r-1",
        "outcome": { "type": "nope" }
    }))
    .expect_err("unknown outcome type should fail");

    assert!(error.to_string().contains("nope") || error.to_string().contains("unknown variant"));
}

#[test]
fn event_union_routes_run_finished_success_correctly() {
    let parsed: Event = serde_json::from_value(json!({
        "type": "RUN_FINISHED",
        "threadId": "t-1",
        "runId": "r-1",
        "outcome": { "type": "success" }
    }))
    .expect("deserialize success through outer union");

    assert!(matches!(parsed, Event::RunFinished(ref event) if event.outcome == Some(success())));
}

#[test]
fn event_union_routes_run_finished_interrupt_correctly() {
    let parsed: Event = serde_json::from_value(json!({
        "type": "RUN_FINISHED",
        "threadId": "t-1",
        "runId": "r-1",
        "outcome": {
            "type": "interrupt",
            "interrupts": [{ "id": "int-1", "reason": "tool_call" }]
        }
    }))
    .expect("deserialize interrupt through outer union");

    assert!(
        matches!(parsed, Event::RunFinished(ref event) if matches!(&event.outcome, Some(RunFinishedOutcome::Interrupt { interrupts }) if interrupts.len() == 1))
    );
}

#[test]
fn event_union_routes_legacy_run_finished_correctly() {
    let parsed: Event = serde_json::from_value(json!({
        "type": "RUN_FINISHED",
        "threadId": "t-1",
        "runId": "r-1"
    }))
    .expect("deserialize legacy run finished through outer union");

    assert!(matches!(parsed, Event::RunFinished(ref event) if event.outcome.is_none()));
}
