//! Ported from TypeScript `core/src/__tests__/capabilities-interrupts.test.ts`.
//!
//! Interrupt-related flags live on `HumanInTheLoopCapabilities` in the canonical
//! `AgentCapabilities` schema (not a standalone `InterruptsCapabilities`).

use agui_rs_core::capabilities::HumanInTheLoopCapabilities;
use agui_rs_core::AgentCapabilities;
use serde_json::json;

#[test]
fn human_in_the_loop_accepts_interrupts_true() {
    let parsed: HumanInTheLoopCapabilities =
        serde_json::from_value(json!({ "interrupts": true })).expect("deserialize");
    assert_eq!(parsed.interrupts, Some(true));
}

#[test]
fn human_in_the_loop_accepts_approve_with_edits_true() {
    let parsed: HumanInTheLoopCapabilities =
        serde_json::from_value(json!({ "approveWithEdits": true })).expect("deserialize");
    assert_eq!(parsed.approve_with_edits, Some(true));
}

#[test]
fn human_in_the_loop_accepts_both_flags_with_existing_ones() {
    let parsed: HumanInTheLoopCapabilities = serde_json::from_value(json!({
        "supported": true,
        "approvals": true,
        "interrupts": true,
        "approveWithEdits": true
    }))
    .expect("deserialize");

    assert_eq!(parsed.supported, Some(true));
    assert_eq!(parsed.approvals, Some(true));
    assert_eq!(parsed.interrupts, Some(true));
    assert_eq!(parsed.approve_with_edits, Some(true));
}

#[test]
fn human_in_the_loop_leaves_new_flags_undefined_when_omitted() {
    let parsed: HumanInTheLoopCapabilities =
        serde_json::from_value(json!({ "supported": true })).expect("deserialize");
    assert_eq!(parsed.interrupts, None);
    assert_eq!(parsed.approve_with_edits, None);
}

#[test]
fn agent_capabilities_nest_human_in_the_loop_interrupt_flags() {
    let capabilities: AgentCapabilities = serde_json::from_value(json!({
        "humanInTheLoop": {
            "supported": true,
            "interrupts": true,
            "approveWithEdits": true
        }
    }))
    .expect("deserialize capabilities");

    let hitl = capabilities
        .human_in_the_loop
        .expect("humanInTheLoop should be present");
    assert_eq!(hitl.interrupts, Some(true));
    assert_eq!(hitl.approve_with_edits, Some(true));
}
