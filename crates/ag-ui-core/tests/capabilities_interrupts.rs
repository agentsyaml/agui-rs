use ag_ui_core::capabilities::InterruptsCapabilities;
use ag_ui_core::Capabilities;
use serde_json::json;

#[test]
fn interrupts_capabilities_accepts_supported_interrupt_flags() {
    let capabilities: InterruptsCapabilities = serde_json::from_value(json!({
        "supported": true,
        "approvals": true,
        "interventions": true,
        "feedback": true,
        "resume": true,
        "approveWithEdits": true
    }))
    .expect("deserialize interrupts capabilities");

    assert_eq!(capabilities.supported, Some(true));
    assert_eq!(capabilities.approvals, Some(true));
    assert_eq!(capabilities.interventions, Some(true));
    assert_eq!(capabilities.feedback, Some(true));
    assert_eq!(capabilities.resume, Some(true));
    assert_eq!(capabilities.approve_with_edits, Some(true));
}

#[test]
fn capabilities_nested_interrupts_preserve_new_flags() {
    let capabilities: Capabilities = serde_json::from_value(json!({
        "interrupts": {
            "supported": true,
            "approvals": true,
            "interventions": true,
            "approveWithEdits": true
        }
    }))
    .expect("deserialize capabilities");

    let interrupts = capabilities
        .interrupts
        .expect("interrupts should be present");
    assert_eq!(interrupts.supported, Some(true));
    assert_eq!(interrupts.approvals, Some(true));
    assert_eq!(interrupts.interventions, Some(true));
    assert_eq!(interrupts.approve_with_edits, Some(true));
}

#[test]
fn interrupts_capabilities_leave_new_flags_absent_when_omitted() {
    let capabilities: InterruptsCapabilities = serde_json::from_value(json!({
        "supported": true
    }))
    .expect("deserialize interrupts capabilities");

    assert_eq!(capabilities.supported, Some(true));
    assert_eq!(capabilities.interventions, None);
    assert_eq!(capabilities.approve_with_edits, None);
}
