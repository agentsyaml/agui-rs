use agui_rs_core::{Interrupt, ResumeEntry, ResumeStatus, RunAgentInput};
use serde_json::json;

#[test]
fn interrupt_accepts_only_required_fields() {
    let interrupt: Interrupt = serde_json::from_value(json!({
        "id": "int-1",
        "reason": "tool_call"
    }))
    .expect("deserialize interrupt");

    assert_eq!(interrupt.id, "int-1");
    assert_eq!(interrupt.reason, "tool_call");
    assert_eq!(interrupt.message, None);
    assert_eq!(
        serde_json::to_value(interrupt).unwrap(),
        json!({
            "id": "int-1",
            "reason": "tool_call"
        })
    );
}

#[test]
fn interrupt_accepts_all_standard_optional_fields() {
    let value = json!({
        "id": "int-1",
        "reason": "input_required",
        "message": "Approve?",
        "toolCallId": "tc-1",
        "responseSchema": { "type": "object" },
        "expiresAt": "2026-04-22T00:00:00Z",
        "metadata": { "foo": "bar" }
    });

    let interrupt: Interrupt =
        serde_json::from_value(value.clone()).expect("deserialize interrupt");
    assert_eq!(serde_json::to_value(interrupt).unwrap(), value);
}

#[test]
fn interrupt_rejects_when_id_is_missing() {
    let error = serde_json::from_value::<Interrupt>(json!({ "reason": "tool_call" }))
        .expect_err("missing id should fail");
    assert!(error.to_string().contains("id"));
}

#[test]
fn interrupt_rejects_when_reason_is_missing() {
    let error = serde_json::from_value::<Interrupt>(json!({ "id": "int-1" }))
        .expect_err("missing reason should fail");
    assert!(error.to_string().contains("reason"));
}

#[test]
fn resume_entry_accepts_resolved_with_payload() {
    let entry: ResumeEntry = serde_json::from_value(json!({
        "interruptId": "int-1",
        "status": "resolved",
        "payload": { "approved": true }
    }))
    .expect("deserialize resume entry");

    assert_eq!(entry.status, ResumeStatus::Resolved);
    assert_eq!(entry.payload, Some(json!({ "approved": true })));
}

#[test]
fn resume_entry_accepts_cancelled_without_payload() {
    let entry: ResumeEntry = serde_json::from_value(json!({
        "interruptId": "int-1",
        "status": "cancelled"
    }))
    .expect("deserialize cancelled resume entry");

    assert_eq!(entry.status, ResumeStatus::Cancelled);
    assert_eq!(entry.payload, None);
}

#[test]
fn resume_entry_rejects_unknown_status_value() {
    let error = serde_json::from_value::<ResumeEntry>(json!({
        "interruptId": "int-1",
        "status": "denied"
    }))
    .expect_err("unknown status should fail");

    assert!(error.to_string().contains("resolved") || error.to_string().contains("cancelled"));
}

#[test]
fn run_agent_input_accepts_missing_resume_for_backwards_compatibility() {
    let input: RunAgentInput = serde_json::from_value(json!({
        "threadId": "t-1",
        "runId": "r-1",
        "state": {},
        "messages": [],
        "tools": [],
        "context": [],
        "forwardedProps": {}
    }))
    .expect("deserialize run input without resume");

    assert_eq!(input.resume, None);
    input.validate().expect("input should validate");
}

#[test]
fn run_agent_input_accepts_resume_array() {
    let input: RunAgentInput = serde_json::from_value(json!({
        "threadId": "t-1",
        "runId": "r-1",
        "state": {},
        "messages": [],
        "tools": [],
        "context": [],
        "forwardedProps": {},
        "resume": [
            { "interruptId": "int-1", "status": "resolved", "payload": { "approved": true } },
            { "interruptId": "int-2", "status": "cancelled" }
        ]
    }))
    .expect("deserialize run input with resume");

    let resume = input.resume.expect("resume should be present");
    assert_eq!(resume.len(), 2);
    assert_eq!(resume[0].status, ResumeStatus::Resolved);
    assert_eq!(resume[1].status, ResumeStatus::Cancelled);
}

#[test]
fn run_agent_input_rejects_resume_entry_with_invalid_status() {
    let error = serde_json::from_value::<RunAgentInput>(json!({
        "threadId": "t-1",
        "runId": "r-1",
        "state": {},
        "messages": [],
        "tools": [],
        "context": [],
        "forwardedProps": {},
        "resume": [{ "interruptId": "int-1", "status": "ignored" }]
    }))
    .expect_err("invalid resume status should fail");

    assert!(error.to_string().contains("resolved") || error.to_string().contains("cancelled"));
}
