//! Round-trip and rejection tests for the protobuf event mapping.

use agui_rs_core::{
    factory, AgUiError, BaseEventFields, CustomEvent, Event, Interrupt, RawEvent,
    ReasoningStartEvent, RunFinishedEvent, RunFinishedOutcome,
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
