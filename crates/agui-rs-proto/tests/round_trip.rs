//! Round-trip and rejection tests for the protobuf event mapping.

use agui_rs_core::{
    factory, AgUiError, BaseEventFields, CustomEvent, Event, Interrupt, RawEvent,
    ReasoningStartEvent, RunFinishedEvent, RunFinishedOutcome, SubagentErrorEvent,
    SubagentFinishedEvent, SubagentFinishedOutcome, SubagentStartedEvent,
};
use serde_json::json;

fn round_trip(event: &Event) -> Event {
    let bytes = agui_rs_proto::encode(event).expect("encode");
    agui_rs_proto::decode(&bytes).expect("decode")
}

#[test]
fn run_started_round_trips() {
    let event = factory::run_started("t1", "r1");
    assert_eq!(round_trip(&event), event);
}

#[test]
fn text_message_chain_round_trips() {
    assert_eq!(
        round_trip(&factory::text_message_start("m1")),
        factory::text_message_start("m1")
    );
    assert_eq!(
        round_trip(&factory::text_message_content("m1", "hi")),
        factory::text_message_content("m1", "hi")
    );
    assert_eq!(
        round_trip(&factory::text_message_end("m1")),
        factory::text_message_end("m1")
    );
}

#[test]
fn tool_call_chain_round_trips() {
    assert_eq!(
        round_trip(&factory::tool_call_start("tc1", "search")),
        factory::tool_call_start("tc1", "search")
    );
    assert_eq!(
        round_trip(&factory::tool_call_args("tc1", "{}")),
        factory::tool_call_args("tc1", "{}")
    );
}

#[test]
fn run_finished_success_outcome_round_trips() {
    let event = factory::run_finished("t1", "r1");
    assert_eq!(round_trip(&event), event);
}

#[test]
fn run_finished_interrupt_outcome_round_trips() {
    let event = Event::RunFinished(RunFinishedEvent {
        thread_id: "t1".into(),
        run_id: "r1".into(),
        result: None,
        outcome: Some(RunFinishedOutcome::Interrupt {
            interrupts: vec![Interrupt {
                id: "i1".into(),
                reason: "approval".into(),
                message: Some("ok?".into()),
                tool_call_id: None,
                response_schema: None,
                expires_at: None,
                metadata: None,
            }],
        }),
        usage: Vec::new(),
        base: BaseEventFields::default(),
    });
    assert_eq!(round_trip(&event), event);
}

#[test]
fn run_finished_no_outcome_round_trips_to_none() {
    let event = Event::RunFinished(RunFinishedEvent {
        thread_id: "t1".into(),
        run_id: "r1".into(),
        result: None,
        outcome: None,
        usage: Vec::new(),
        base: BaseEventFields::default(),
    });
    assert_eq!(round_trip(&event), event);
}

#[test]
fn custom_event_round_trips() {
    let event = Event::Custom(CustomEvent {
        name: "ping".into(),
        value: json!({"x": true, "s": "hi"}),
        base: BaseEventFields::default(),
    });
    assert_eq!(round_trip(&event), event);
}

#[test]
fn raw_event_round_trips() {
    let event = Event::Raw(RawEvent {
        event: json!({"provider": "openai"}),
        source: Some("upstream".into()),
        base: BaseEventFields::default(),
    });
    assert_eq!(round_trip(&event), event);
}

#[test]
fn reasoning_events_are_not_part_of_proto_schema() {
    let event = Event::ReasoningStart(ReasoningStartEvent {
        message_id: "r1".into(),
        base: BaseEventFields::default(),
    });
    let err = agui_rs_proto::encode(&event).expect_err("reasoning is not in proto schema");
    assert!(matches!(err, AgUiError::Unsupported(_)));
}

#[test]
fn subagent_started_full_fields_round_trip() {
    let event = Event::SubagentStarted(SubagentStartedEvent {
        subagent_run_id: "sub-1".into(),
        name: "researcher".into(),
        description: Some("deep dive".into()),
        parent_subagent_run_id: Some("sub-0".into()),
        parent_tool_call_id: Some("tc-1".into()),
        parent_message_id: Some("m-1".into()),
        base: BaseEventFields::default(),
    });
    assert_eq!(round_trip(&event), event);
}

#[test]
fn subagent_finished_suspended_ids_round_trip() {
    let event = Event::SubagentFinished(SubagentFinishedEvent {
        subagent_run_id: "sub-1".into(),
        result: Some(json!({"summary": "paused"})),
        outcome: Some(SubagentFinishedOutcome::Suspended {
            interrupt_ids: vec!["a".into()],
        }),
        base: BaseEventFields::default(),
    });
    assert_eq!(round_trip(&event), event);
}

#[test]
fn subagent_finished_success_with_result_round_trips() {
    let event = Event::SubagentFinished(SubagentFinishedEvent {
        subagent_run_id: "sub-1".into(),
        result: Some(json!({"ok": true})),
        outcome: Some(SubagentFinishedOutcome::Success),
        base: BaseEventFields::default(),
    });
    assert_eq!(round_trip(&event), event);
}

#[test]
fn subagent_error_round_trips() {
    let event = Event::SubagentError(SubagentErrorEvent {
        subagent_run_id: "sub-1".into(),
        message: "boom".into(),
        code: Some("E_SUB".into()),
        base: BaseEventFields::default(),
    });
    assert_eq!(round_trip(&event), event);
}
