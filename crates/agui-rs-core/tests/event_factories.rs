use agui_rs_core::event_factories as factory;
use agui_rs_core::types::{Interrupt, Message, RunAgentInput};
use agui_rs_core::{
    Event, ReasoningEncryptedValueSubtype, ReasoningMessageRole, RunFinishedOutcome,
    TextMessageRole, ToolResultRole,
};
use serde_json::json;

#[test]
fn text_message_factories_cover_roles_names_and_optional_fields() {
    let start_default =
        factory::create_text_message_start_event("msg-1", None, None, Some(11), None);
    let start_user = factory::create_text_message_start_event(
        "msg-2",
        Some(TextMessageRole::User),
        Some("research-agent".into()),
        Some(12),
        None,
    );
    let content_empty = factory::create_text_message_content_event("msg-3", "", Some(13), None);
    let content_text = factory::create_text_message_content_event("msg-4", "hi", Some(14), None);
    let end = factory::create_text_message_end_event("msg-5", Some(15), None);
    let chunk_named = factory::create_text_message_chunk_event(
        Some("msg-1".into()),
        None,
        Some("Hello".into()),
        Some("research-agent".into()),
        Some(16),
        None,
    );
    let chunk_optional = factory::create_text_message_chunk_event(
        None,
        None,
        Some("partial".into()),
        None,
        Some(17),
        None,
    );

    assert!(
        matches!(start_default, Event::TextMessageStart(ref event) if event.role == TextMessageRole::Assistant && event.base.timestamp == Some(11))
    );
    assert!(
        matches!(start_user, Event::TextMessageStart(ref event) if event.role == TextMessageRole::User && event.name.as_deref() == Some("research-agent"))
    );
    assert!(
        matches!(content_empty, Event::TextMessageContent(ref event) if event.delta.is_empty())
    );
    assert!(matches!(content_text, Event::TextMessageContent(ref event) if event.delta == "hi"));
    assert!(matches!(end, Event::TextMessageEnd(ref event) if event.message_id == "msg-5"));
    assert!(
        matches!(chunk_named, Event::TextMessageChunk(ref event) if event.name.as_deref() == Some("research-agent"))
    );
    assert!(
        matches!(chunk_optional, Event::TextMessageChunk(ref event) if event.delta.as_deref() == Some("partial") && event.message_id.is_none())
    );
}

#[test]
fn thinking_message_and_reasoning_factories_build_expected_variants() {
    let thinking_start =
        factory::create_thinking_start_event(Some("working".into()), Some(21), None);
    let thinking_text_start = factory::create_thinking_text_message_start_event(Some(22), None);
    let thinking_text_content =
        factory::create_thinking_text_message_content_event("thinking…", Some(23), None);
    let thinking_text_end = factory::create_thinking_text_message_end_event(Some(24), None);
    let thinking_end = factory::create_thinking_end_event(Some(25), None);
    let reasoning_start = factory::create_reasoning_start_event("r1", Some(26), None);
    let reasoning_message_start = factory::create_reasoning_message_start_event(
        "r1",
        Some(ReasoningMessageRole::Reasoning),
        Some(27),
        None,
    );
    let reasoning_message_content =
        factory::create_reasoning_message_content_event("r1", "step", Some(28), None);
    let reasoning_message_end = factory::create_reasoning_message_end_event("r1", Some(29), None);
    let reasoning_chunk = factory::create_reasoning_message_chunk_event(
        Some("r1".into()),
        Some("chunk".into()),
        Some(30),
        None,
    );
    let reasoning_end = factory::create_reasoning_end_event("r1", Some(31), None);
    let encrypted = factory::create_reasoning_encrypted_value_event(
        ReasoningEncryptedValueSubtype::ToolCall,
        "tc1",
        "cipher",
        Some(32),
        None,
    );

    assert!(
        matches!(thinking_start, Event::ThinkingStart(ref event) if event.title.as_deref() == Some("working"))
    );
    assert!(matches!(
        thinking_text_start,
        Event::ThinkingTextMessageStart(_)
    ));
    assert!(
        matches!(thinking_text_content, Event::ThinkingTextMessageContent(ref event) if event.delta == "thinking…")
    );
    assert!(matches!(
        thinking_text_end,
        Event::ThinkingTextMessageEnd(_)
    ));
    assert!(matches!(thinking_end, Event::ThinkingEnd(_)));
    assert!(
        matches!(reasoning_start, Event::ReasoningStart(ref event) if event.message_id == "r1")
    );
    assert!(
        matches!(reasoning_message_start, Event::ReasoningMessageStart(ref event) if event.role == ReasoningMessageRole::Reasoning)
    );
    assert!(
        matches!(reasoning_message_content, Event::ReasoningMessageContent(ref event) if event.delta == "step")
    );
    assert!(
        matches!(reasoning_message_end, Event::ReasoningMessageEnd(ref event) if event.message_id == "r1")
    );
    assert!(
        matches!(reasoning_chunk, Event::ReasoningMessageChunk(ref event) if event.delta.as_deref() == Some("chunk"))
    );
    assert!(matches!(reasoning_end, Event::ReasoningEnd(ref event) if event.message_id == "r1"));
    assert!(
        matches!(encrypted, Event::ReasoningEncryptedValue(ref event) if event.subtype == ReasoningEncryptedValueSubtype::ToolCall)
    );
}

#[test]
fn tool_call_factories_cover_start_args_chunk_end_and_result() {
    let start = factory::create_tool_call_start_event(
        "tc-1",
        "search",
        Some("msg-parent".into()),
        Some(41),
        None,
    );
    let args = factory::create_tool_call_args_event("tc-1", r#"{"q":"hi"}"#, Some(42), None);
    let chunk = factory::create_tool_call_chunk_event(
        Some("tc-1".into()),
        Some("search".into()),
        None,
        Some("partial".into()),
        Some(43),
        None,
    );
    let end = factory::create_tool_call_end_event("tc-1", Some(44), None);
    let result = factory::create_tool_call_result_event(
        "msg-6",
        "tc-1",
        r#"{"ok":true}"#,
        Some(ToolResultRole::Tool),
        Some(45),
        None,
    );

    assert!(
        matches!(start, Event::ToolCallStart(ref event) if event.parent_message_id.as_deref() == Some("msg-parent"))
    );
    assert!(matches!(args, Event::ToolCallArgs(ref event) if event.delta.contains('q')));
    assert!(
        matches!(chunk, Event::ToolCallChunk(ref event) if event.delta.as_deref() == Some("partial"))
    );
    assert!(matches!(end, Event::ToolCallEnd(ref event) if event.tool_call_id == "tc-1"));
    assert!(
        matches!(result, Event::ToolCallResult(ref event) if event.role == Some(ToolResultRole::Tool) && event.content == r#"{"ok":true}"#)
    );
}

#[test]
fn snapshot_raw_custom_run_and_step_factories_cover_public_api() {
    let messages = vec![
        serde_json::from_value::<Message>(
            json!({ "role": "user", "id": "u1", "content": "hello" }),
        )
        .unwrap(),
        serde_json::from_value::<Message>(
            json!({ "role": "assistant", "id": "a1", "content": "hi there" }),
        )
        .unwrap(),
    ];
    let run_input = RunAgentInput::new("t1", "r1");
    let state_snapshot = factory::create_state_snapshot_event(json!({ "step": 1 }), Some(51), None);
    let state_delta = factory::create_state_delta_event(
        vec![json!({ "op": "add", "path": "/foo", "value": "bar" })],
        Some(52),
        None,
    );
    let messages_snapshot = factory::create_messages_snapshot_event(messages, Some(123), None);
    let activity_snapshot = factory::create_activity_snapshot_event(
        "activity-1",
        "PLAN",
        serde_json::Map::from_iter([("steps".to_string(), json!([]))]),
        None,
        Some(53),
        None,
    );
    let activity_delta = factory::create_activity_delta_event(
        "activity-1",
        "PLAN",
        vec![json!({ "op": "replace", "path": "/steps/0", "value": "done" })],
        Some(54),
        None,
    );
    let raw = factory::create_raw_event(
        json!({ "any": true }),
        Some("webhook".into()),
        Some(55),
        None,
    );
    let custom = factory::create_custom_event("metric", json!(42), Some(56), None);
    let started =
        factory::create_run_started_event("t1", "r1", None, Some(run_input), Some(57), None);
    let finished = factory::create_run_finished_event(
        "t1",
        "r1",
        Some(json!({ "ok": true })),
        None,
        Some(58),
        None,
    );
    let error = factory::create_run_error_event("boom", Some("E_FAIL".into()), Some(59), None);
    let step_started = factory::create_step_started_event("fetch", Some(60), None);
    let step_finished = factory::create_step_finished_event("fetch", Some(61), None);

    assert!(
        matches!(state_snapshot, Event::StateSnapshot(ref event) if event.snapshot == json!({ "step": 1 }))
    );
    assert!(matches!(state_delta, Event::StateDelta(ref event) if event.delta[0]["op"] == "add"));
    assert!(
        matches!(messages_snapshot, Event::MessagesSnapshot(ref event) if event.messages.len() == 2 && event.base.timestamp == Some(123))
    );
    assert!(matches!(activity_snapshot, Event::ActivitySnapshot(ref event) if event.replace));
    assert!(
        matches!(activity_delta, Event::ActivityDelta(ref event) if event.patch[0]["path"] == "/steps/0")
    );
    assert!(matches!(raw, Event::Raw(ref event) if event.source.as_deref() == Some("webhook")));
    assert!(matches!(custom, Event::Custom(ref event) if event.value == json!(42)));
    assert!(
        matches!(started, Event::RunStarted(ref event) if event.input.as_ref().map(|input| input.run_id.as_str()) == Some("r1"))
    );
    assert!(
        matches!(finished, Event::RunFinished(ref event) if event.result == Some(json!({ "ok": true })) && event.outcome.is_none())
    );
    assert!(matches!(error, Event::RunError(ref event) if event.code.as_deref() == Some("E_FAIL")));
    assert!(matches!(step_started, Event::StepStarted(ref event) if event.step_name == "fetch"));
    assert!(matches!(step_finished, Event::StepFinished(ref event) if event.step_name == "fetch"));
}

#[test]
fn run_finished_success_factory_sets_success_outcome() {
    let event = factory::create_run_finished_success_event(
        "t-1",
        "r-1",
        Some(json!({ "ok": true })),
        Some(71),
        None,
    );

    assert!(
        matches!(event, Event::RunFinished(ref event) if event.outcome == Some(RunFinishedOutcome::Success) && event.result == Some(json!({ "ok": true })))
    );
}

#[test]
fn run_finished_interrupt_factory_sets_interrupt_outcome() {
    let event = factory::create_run_finished_interrupt_event(
        "t-1",
        "r-1",
        None,
        vec![Interrupt {
            id: "int-1".into(),
            reason: "tool_call".into(),
            message: None,
            tool_call_id: None,
            response_schema: None,
            expires_at: None,
            metadata: None,
        }],
        Some(72),
        None,
    );

    assert!(
        matches!(event, Event::RunFinished(ref event) if matches!(&event.outcome, Some(RunFinishedOutcome::Interrupt { interrupts }) if interrupts.len() == 1))
    );
}

#[test]
fn run_finished_factory_accepts_explicit_success_and_interrupt_outcomes() {
    let success = factory::create_run_finished_event(
        "t-1",
        "r-1",
        None,
        Some(RunFinishedOutcome::Success),
        Some(73),
        None,
    );
    let interrupt = factory::create_run_finished_event(
        "t-1",
        "r-1",
        None,
        Some(RunFinishedOutcome::Interrupt {
            interrupts: vec![Interrupt {
                id: "int-1".into(),
                reason: "tool_call".into(),
                message: None,
                tool_call_id: None,
                response_schema: None,
                expires_at: None,
                metadata: None,
            }],
        }),
        Some(74),
        None,
    );

    assert!(
        matches!(success, Event::RunFinished(ref event) if event.outcome == Some(RunFinishedOutcome::Success))
    );
    assert!(
        matches!(interrupt, Event::RunFinished(ref event) if matches!(&event.outcome, Some(RunFinishedOutcome::Interrupt { interrupts }) if interrupts.len() == 1))
    );
}

// SKIPPED: rejects empty interrupts array: Rust factory API does not validate non-empty interrupt lists.
