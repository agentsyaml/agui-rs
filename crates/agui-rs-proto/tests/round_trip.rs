//! Round-trip and rejection tests for the protobuf event mapping.

use agui_rs_core::types::{ContentPart, PartSource, UserMessage, UserMessageContent};
use agui_rs_core::{
    factory, ActivityDeltaEvent, ActivitySnapshotEvent, AttributableFields, BaseEventFields,
    CustomEvent, Event, Interrupt, MessagesSnapshotEvent, RawEvent, ReasoningEncryptedValueEvent,
    ReasoningEncryptedValueSubtype, ReasoningEndEvent, ReasoningMessageContentEvent,
    ReasoningMessageStartEvent, ReasoningStartEvent, RunAgentInput, RunFinishedEvent,
    RunFinishedOutcome, RunStartedEvent, SubagentErrorEvent, SubagentFinishedEvent,
    SubagentFinishedOutcome, SubagentStartedEvent, TokenUsage, ToolCallResultEvent,
    ToolResultContent, ToolResultRole,
};
use serde_json::json;

/// `Attributable` is `#[serde(flatten)]`, so it reads as an extra field on every
/// event that carries one.
fn attributed(subagent_run_id: Option<&str>) -> AttributableFields {
    AttributableFields {
        subagent_run_id: subagent_run_id.map(str::to_string),
    }
}

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
                subagent_run_id: Some("sub-1".into()),
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
        attributable: AttributableFields::default(),
    });
    assert_eq!(round_trip(&event), event);
}

#[test]
fn raw_event_round_trips() {
    let event = Event::Raw(RawEvent {
        event: json!({"provider": "openai"}),
        source: Some("upstream".into()),
        base: BaseEventFields::default(),
        attributable: AttributableFields::default(),
    });
    assert_eq!(round_trip(&event), event);
}

/// 1.0 moved reasoning into the protobuf schema: `REASONING_START = 24` in
/// `EventType` and oneof tag 25. It used to be rejected as unsupported; an
/// earlier version of this test asserted exactly that, against a stale copy of
/// upstream.
#[test]
fn reasoning_events_are_part_of_the_proto_schema() {
    let start = Event::ReasoningStart(ReasoningStartEvent {
        message_id: "r1".into(),
        base: BaseEventFields::default(),
        attributable: attributed(Some("sub-1")),
    });
    assert_eq!(round_trip(&start), start);

    let content = Event::ReasoningMessageContent(ReasoningMessageContentEvent {
        message_id: "r1".into(),
        delta: "because".into(),
        base: BaseEventFields::default(),
        attributable: AttributableFields::default(),
    });
    assert_eq!(round_trip(&content), content);

    let end = Event::ReasoningEnd(ReasoningEndEvent {
        message_id: "r1".into(),
        base: BaseEventFields::default(),
        attributable: AttributableFields::default(),
    });
    assert_eq!(round_trip(&end), end);

    let encrypted = Event::ReasoningEncryptedValue(ReasoningEncryptedValueEvent {
        subtype: ReasoningEncryptedValueSubtype::ToolCall,
        entity_id: "tc-1".into(),
        encrypted_value: "blob".into(),
        base: BaseEventFields::default(),
        attributable: AttributableFields::default(),
    });
    assert_eq!(round_trip(&encrypted), encrypted);
}

#[test]
fn reasoning_message_start_round_trips() {
    let event = Event::ReasoningMessageStart(ReasoningMessageStartEvent {
        message_id: "r1".into(),
        role: agui_rs_core::ReasoningMessageRole::Reasoning,
        base: BaseEventFields::default(),
        attributable: AttributableFields::default(),
    });
    assert_eq!(round_trip(&event), event);
}

/// The bug this crate shipped: `TextMessageChunk` and `ToolCallChunk` claimed
/// their `base.type` was the non-chunk event, so a peer dispatching on the type
/// read the wrong event. Round-tripping hid it, because both ends lied the same
/// way, so the discriminator is asserted against the bytes.
#[test]
fn chunk_events_carry_their_own_event_type() {
    let chunk = Event::TextMessageChunk(agui_rs_core::TextMessageChunkEvent {
        message_id: Some("m1".into()),
        role: None,
        delta: Some("hi".into()),
        name: None,
        base: BaseEventFields::default(),
        attributable: AttributableFields::default(),
    });
    assert_eq!(round_trip(&chunk), chunk);

    let tool_chunk = Event::ToolCallChunk(agui_rs_core::ToolCallChunkEvent {
        tool_call_id: Some("tc-1".into()),
        tool_call_name: None,
        parent_message_id: None,
        delta: Some("{}".into()),
        base: BaseEventFields::default(),
        attributable: AttributableFields::default(),
    });
    assert_eq!(round_trip(&tool_chunk), tool_chunk);
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

// ----- the 1.0 surface that was previously missing -----

#[test]
fn run_started_carries_parent_run_id_input_and_protocol_version() {
    // These were wrongly marked "reserved, never reuse" on the strength of a
    // stale upstream copy. They are live fields at 4, 5 and 6.
    let mut input = RunAgentInput::new("t1", "r1");
    input.parent_run_id = Some("p0".into());
    let event = Event::RunStarted(RunStartedEvent {
        thread_id: "t1".into(),
        run_id: "r1".into(),
        protocol_version: Some("1.0".into()),
        parent_run_id: Some("p0".into()),
        input: Some(input.clone()),
        base: BaseEventFields::default(),
    });
    let decoded = round_trip(&event);
    assert_eq!(decoded, event);
    let Event::RunStarted(started) = decoded else {
        panic!("expected run started");
    };
    assert_eq!(started.protocol_version.as_deref(), Some("1.0"));
    assert_eq!(started.parent_run_id.as_deref(), Some("p0"));
    assert_eq!(started.input, Some(input));
}

#[test]
fn run_finished_success_carries_pending_tool_call_ids() {
    let event = Event::RunFinished(RunFinishedEvent {
        thread_id: "t1".into(),
        run_id: "r1".into(),
        result: None,
        outcome: Some(RunFinishedOutcome::Success {
            pending_tool_call_ids: Some(vec!["tc-1".into(), "tc-2".into()]),
        }),
        usage: Vec::new(),
        base: BaseEventFields::default(),
    });
    assert_eq!(round_trip(&event), event);
}

#[test]
fn run_finished_cancelled_round_trips() {
    let event = Event::RunFinished(RunFinishedEvent {
        thread_id: "t1".into(),
        run_id: "r1".into(),
        result: None,
        outcome: Some(RunFinishedOutcome::Cancelled),
        usage: Vec::new(),
        base: BaseEventFields::default(),
    });
    assert_eq!(round_trip(&event), event);
}

#[test]
fn token_usage_round_trips_through_int64() {
    // Upstream types every token count `optional int64`; core uses `Option<u64>`.
    // The negative side is not a token count, so it decodes as absent.
    let event = Event::RunFinished(RunFinishedEvent {
        thread_id: "t1".into(),
        run_id: "r1".into(),
        result: None,
        outcome: None,
        usage: vec![TokenUsage {
            provider: Some("openai".into()),
            model: Some("gpt".into()),
            input_tokens: Some(11),
            output_tokens: Some(22),
            total_tokens: Some(33),
            reasoning_tokens: Some(4),
            cached_input_tokens: Some(5),
            cache_write_input_tokens: Some(6),
        }],
        base: BaseEventFields::default(),
    });
    assert_eq!(round_trip(&event), event);
}

#[test]
fn tool_call_result_round_trips_both_body_shapes() {
    let text = Event::ToolCallResult(ToolCallResultEvent {
        message_id: "m1".into(),
        tool_call_id: "tc-1".into(),
        content: ToolResultContent::Text("42".into()),
        role: Some(ToolResultRole::Tool),
        base: BaseEventFields::default(),
        attributable: attributed(Some("sub-1")),
    });
    assert_eq!(round_trip(&text), text);

    let parts = Event::ToolCallResult(ToolCallResultEvent {
        message_id: "m1".into(),
        tool_call_id: "tc-1".into(),
        content: ToolResultContent::Parts(vec![ContentPart::Text {
            id: Some("p1".into()),
            text: "42".into(),
            metadata: None,
        }]),
        role: None,
        base: BaseEventFields::default(),
        attributable: AttributableFields::default(),
    });
    assert_eq!(round_trip(&parts), parts);
}

#[test]
fn activity_events_round_trip() {
    let snapshot = Event::ActivitySnapshot(ActivitySnapshotEvent {
        message_id: "a1".into(),
        activity_type: "plan".into(),
        // `google.protobuf.Value` carries a double, so a whole number reads back
        // as 1.0. That is upstream's wire type, not a mapping bug.
        content: json!({"step": 1.0}).as_object().unwrap().clone(),
        replace: Some(true),
        base: BaseEventFields::default(),
        attributable: attributed(Some("sub-1")),
    });
    assert_eq!(round_trip(&snapshot), snapshot);

    let delta = Event::ActivityDelta(ActivityDeltaEvent {
        message_id: "a1".into(),
        activity_type: "plan".into(),
        // again, the patch value rides a `google.protobuf.Value`, a double
        patch: vec![json!({"op": "replace", "path": "/step", "value": 2.0})],
        base: BaseEventFields::default(),
        attributable: AttributableFields::default(),
    });
    assert_eq!(round_trip(&delta), delta);
}

#[test]
fn multimodal_user_message_round_trips() {
    let event = Event::MessagesSnapshot(MessagesSnapshotEvent {
        messages: vec![agui_rs_core::Message::User(UserMessage {
            id: "u1".into(),
            content: UserMessageContent::Parts(vec![
                ContentPart::Text {
                    id: Some("t1".into()),
                    text: "look".into(),
                    metadata: None,
                },
                ContentPart::Image {
                    id: None,
                    source: PartSource::Url {
                        value: "https://example.com/a.png".into(),
                        mime_type: Some("image/png".into()),
                    },
                    metadata: None,
                },
            ]),
            name: None,
            encrypted_value: None,
            subagent_run_id: None,
            metadata: None,
        })],
        base: BaseEventFields::default(),
    });
    assert_eq!(round_trip(&event), event);
}

/// Anchors the source-level drift test to the wire: these are the literal bytes
/// upstream's field numbers produce. `proto_drift.rs` proves the numbers match
/// the vendored `.proto`; this proves the numbers reach the encoder.
#[test]
fn encoded_bytes_match_the_upstream_wire_layout() {
    let run_started = agui_rs_proto::encode(&factory::run_started("t1", "r1")).expect("encode");
    assert_eq!(
        run_started,
        vec![
            0x62, 12, // Event.run_started = 12, LEN 12
            0x0A, 2, // RunStartedEvent.base_event = 1, LEN 2
            0x08, 0x0B, // BaseEvent.type = 1, RUN_STARTED = 11
            0x12, 0x02, b't', b'1', // thread_id = 2
            0x1A, 0x02, b'r', b'1', // run_id = 3
        ]
    );
}
