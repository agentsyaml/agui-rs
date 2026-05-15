#![allow(dead_code)]
#![allow(unused_imports)]

use ag_ui_client::interrupts::build_resume_array;
use ag_ui_client::{get_run_outcome, is_interrupt_expired, ResumeResponse};
use ag_ui_core::{
    BaseEventFields, Event, Interrupt, ResumeStatus, RunFinishedEvent, RunFinishedOutcome,
};
use serde_json::json;
use std::collections::HashMap;

fn interrupt(id: &str, expires_at: Option<&str>) -> Interrupt {
    Interrupt {
        id: id.into(),
        reason: "tool_call".into(),
        message: None,
        tool_call_id: None,
        response_schema: None,
        expires_at: expires_at.map(str::to_owned),
        metadata: None,
    }
}

#[test]
fn get_run_outcome_covers_legacy_success_and_interrupt_cases() {
    let legacy = Event::RunFinished(RunFinishedEvent {
        thread_id: "t-1".into(),
        run_id: "r-1".into(),
        result: None,
        outcome: None,
        base: BaseEventFields::default(),
    });
    let success = Event::RunFinished(RunFinishedEvent {
        thread_id: "t-1".into(),
        run_id: "r-1".into(),
        result: None,
        outcome: Some(RunFinishedOutcome::Success),
        base: BaseEventFields::default(),
    });
    let interrupts = vec![interrupt("int-1", None)];
    let interrupt_event = Event::RunFinished(RunFinishedEvent {
        thread_id: "t-1".into(),
        run_id: "r-1".into(),
        result: None,
        outcome: Some(RunFinishedOutcome::Interrupt {
            interrupts: interrupts.clone(),
        }),
        base: BaseEventFields::default(),
    });

    assert_eq!(
        get_run_outcome(&[legacy]),
        ag_ui_client::interrupts::RunOutcome::Finished(None)
    );
    assert_eq!(
        get_run_outcome(&[success]),
        ag_ui_client::interrupts::RunOutcome::Finished(Some(RunFinishedOutcome::Success))
    );
    assert_eq!(
        get_run_outcome(&[interrupt_event]),
        ag_ui_client::interrupts::RunOutcome::Finished(Some(RunFinishedOutcome::Interrupt {
            interrupts
        }))
    );
}

#[test]
fn interrupt_expiration_honors_missing_past_future_and_injected_now() {
    let unset = Event::RunFinished(RunFinishedEvent {
        thread_id: "t-1".into(),
        run_id: "r-1".into(),
        result: None,
        outcome: Some(RunFinishedOutcome::Interrupt {
            interrupts: vec![interrupt("int-1", None)],
        }),
        base: BaseEventFields::default(),
    });
    let past = Event::RunFinished(RunFinishedEvent {
        thread_id: "t-1".into(),
        run_id: "r-1".into(),
        result: None,
        outcome: Some(RunFinishedOutcome::Interrupt {
            interrupts: vec![interrupt("int-1", Some("2000-01-01T00:00:00Z"))],
        }),
        base: BaseEventFields::default(),
    });
    let future = Event::RunFinished(RunFinishedEvent {
        thread_id: "t-1".into(),
        run_id: "r-1".into(),
        result: None,
        outcome: Some(RunFinishedOutcome::Interrupt {
            interrupts: vec![interrupt("int-1", Some("2099-01-01T00:00:00Z"))],
        }),
        base: BaseEventFields::default(),
    });
    let deterministic = Event::RunFinished(RunFinishedEvent {
        thread_id: "t-1".into(),
        run_id: "r-1".into(),
        result: None,
        outcome: Some(RunFinishedOutcome::Interrupt {
            interrupts: vec![interrupt("int-1", Some("2026-04-22T12:00:00Z"))],
        }),
        base: BaseEventFields::default(),
    });

    assert!(!is_interrupt_expired(&unset, "2026-01-01T00:00:00Z"));
    assert!(is_interrupt_expired(&past, "2026-01-01T00:00:00Z"));
    assert!(!is_interrupt_expired(&future, "2026-01-01T00:00:00Z"));
    assert!(!is_interrupt_expired(
        &deterministic,
        "2026-04-22T11:59:00Z"
    ));
    assert!(is_interrupt_expired(&deterministic, "2026-04-22T12:00:01Z"));
}

#[test]
fn build_resume_array_validates_and_preserves_order() {
    let interrupts = vec![interrupt("int-1", None), interrupt("int-2", None)];
    let responses = HashMap::from([
        (
            "int-1".to_string(),
            ResumeResponse::Resolved {
                payload: Some(json!({"approved": true})),
            },
        ),
        ("int-2".to_string(), ResumeResponse::Cancelled),
    ]);

    let resume = build_resume_array(&interrupts, &responses).expect("resume array should build");
    assert_eq!(resume.len(), 2);
    assert_eq!(resume[0].interrupt_id, "int-1");
    assert_eq!(resume[0].status, ResumeStatus::Resolved);
    assert_eq!(resume[0].payload, Some(json!({"approved": true})));
    assert_eq!(resume[1].interrupt_id, "int-2");
    assert_eq!(resume[1].status, ResumeStatus::Cancelled);
    assert_eq!(resume[1].payload, None);

    let missing = HashMap::from([(
        "int-1".to_string(),
        ResumeResponse::Resolved { payload: None },
    )]);
    let unknown = HashMap::from([
        (
            "int-1".to_string(),
            ResumeResponse::Resolved { payload: None },
        ),
        ("int-2".to_string(), ResumeResponse::Cancelled),
        ("int-3".to_string(), ResumeResponse::Cancelled),
    ]);

    assert!(build_resume_array(&interrupts, &missing)
        .expect_err("missing response should error")
        .to_string()
        .contains("int-2"));
    assert!(build_resume_array(&interrupts, &unknown)
        .expect_err("unknown response should error")
        .to_string()
        .contains("int-3"));
}
