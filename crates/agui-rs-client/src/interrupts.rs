use std::collections::{HashMap, HashSet};

use agui_rs_core::{
    AgUiError, Event, Interrupt, Result, ResumeEntry, ResumeStatus, RunFinishedOutcome,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ResumeResponse {
    Resolved {
        #[serde(skip_serializing_if = "Option::is_none", default)]
        payload: Option<Value>,
    },
    Cancelled,
}

#[derive(Debug, Clone, PartialEq)]
pub enum RunOutcome {
    Pending,
    Finished(Option<RunFinishedOutcome>),
    Error {
        message: String,
        code: Option<String>,
    },
}

/// Returns the terminal outcome derived from a run event slice.
pub fn get_run_outcome(events: &[Event]) -> RunOutcome {
    let mut outcome = RunOutcome::Pending;

    for event in events {
        match event {
            Event::RunFinished(event) => {
                outcome = RunOutcome::Finished(event.outcome.clone());
            }
            Event::RunError(event) => {
                outcome = RunOutcome::Error {
                    message: event.message.clone(),
                    code: event.code.clone(),
                };
            }
            _ => {}
        }
    }

    outcome
}

/// Checks whether an interrupt-bearing event has expired.
pub fn is_interrupt_expired(interrupt: &Event, now_iso: &str) -> bool {
    match interrupt {
        Event::RunFinished(event) => match &event.outcome {
            Some(RunFinishedOutcome::Interrupt { interrupts }) => interrupts
                .iter()
                .filter_map(|entry| entry.expires_at.as_deref())
                .any(|expires_at| expires_at <= now_iso),
            _ => false,
        },
        _ => false,
    }
}

/// Checks whether a single [`Interrupt`] has expired relative to an ISO-8601
/// `now` timestamp.
///
/// Mirrors the TypeScript `isInterruptExpired(interrupt, now)` helper: an
/// interrupt with no `expiresAt` never expires; otherwise it is expired once
/// `expiresAt <= now`. Comparison is lexicographic, which is correct for
/// normalized (UTC, same-precision) ISO-8601 strings — the form the protocol
/// emits.
pub fn interrupt_is_expired(interrupt: &Interrupt, now_iso: &str) -> bool {
    match interrupt.expires_at.as_deref() {
        Some(expires_at) => expires_at <= now_iso,
        None => false,
    }
}

/// Validates that `resume` entries address every still-open interrupt and that
/// none of those interrupts has expired.
///
/// Mirrors the enforcement TypeScript `AbstractAgent.onInitialize` performs
/// before a run when `pendingInterrupts` is non-empty:
/// - every pending interrupt id must appear in `resume`, otherwise a
///   [`AgUiError::Validation`] listing the uncovered ids is returned;
/// - any pending interrupt whose `expiresAt <= now_iso` is rejected.
///
/// `now_iso` is the current time as an ISO-8601 string; pass the producer's
/// clock so the check stays dependency-free in `agui-rs-core`/`-client`.
pub fn ensure_resume_covers(
    pending: &[Interrupt],
    resume: &[ResumeEntry],
    now_iso: &str,
) -> Result<()> {
    if pending.is_empty() {
        return Ok(());
    }

    let resumed_ids: HashSet<&str> = resume
        .iter()
        .map(|entry| entry.interrupt_id.as_str())
        .collect();

    let mut uncovered: Vec<&str> = pending
        .iter()
        .map(|interrupt| interrupt.id.as_str())
        .filter(|id| !resumed_ids.contains(id))
        .collect();
    uncovered.sort_unstable();
    if !uncovered.is_empty() {
        return Err(AgUiError::validation(format!(
            "Thread has {} pending interrupt(s) not addressed by resume: {}",
            uncovered.len(),
            uncovered.join(", ")
        )));
    }

    for interrupt in pending {
        if interrupt_is_expired(interrupt, now_iso) {
            return Err(AgUiError::validation(format!(
                "Interrupt {} expired at {}",
                interrupt.id,
                interrupt.expires_at.as_deref().unwrap_or_default()
            )));
        }
    }

    Ok(())
}

/// Builds resume entries in interrupt order.
pub fn build_resume_array(
    interrupts: &[Interrupt],
    responses: &HashMap<String, ResumeResponse>,
) -> Result<Vec<ResumeEntry>> {
    let open_ids = interrupts
        .iter()
        .map(|interrupt| interrupt.id.clone())
        .collect::<HashSet<_>>();
    let response_ids = responses.keys().cloned().collect::<HashSet<_>>();

    let mut missing = open_ids
        .difference(&response_ids)
        .cloned()
        .collect::<Vec<_>>();
    missing.sort();
    if !missing.is_empty() {
        return Err(AgUiError::validation(format!(
            "build_resume_array: missing responses for open interrupts: {}",
            missing.join(", ")
        )));
    }

    let mut unknown = response_ids
        .difference(&open_ids)
        .cloned()
        .collect::<Vec<_>>();
    unknown.sort();
    if !unknown.is_empty() {
        return Err(AgUiError::validation(format!(
            "build_resume_array: responses reference unknown interrupt ids: {}",
            unknown.join(", ")
        )));
    }

    Ok(interrupts
        .iter()
        .map(|interrupt| match responses.get(&interrupt.id) {
            Some(ResumeResponse::Resolved { payload }) => ResumeEntry {
                interrupt_id: interrupt.id.clone(),
                status: ResumeStatus::Resolved,
                payload: payload.clone(),
            },
            Some(ResumeResponse::Cancelled) => ResumeEntry {
                interrupt_id: interrupt.id.clone(),
                status: ResumeStatus::Cancelled,
                payload: None,
            },
            None => unreachable!("validated missing responses before mapping"),
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use agui_rs_core::{
        factory, BaseEventFields, Event, Interrupt, RunErrorEvent, RunFinishedEvent,
        RunFinishedOutcome,
    };
    use serde_json::json;

    use super::{
        build_resume_array, get_run_outcome, is_interrupt_expired, ResumeResponse, RunOutcome,
    };

    fn interrupt(id: &str, expires_at: Option<&str>) -> Interrupt {
        Interrupt {
            id: id.into(),
            reason: "tool_call".into(),
            message: None,
            tool_call_id: None,
            response_schema: None,
            expires_at: expires_at.map(str::to_string),
            metadata: None,
        }
    }

    #[test]
    fn resume_response_serializes_with_tag() {
        let value = serde_json::to_value(ResumeResponse::Resolved {
            payload: Some(json!({"approved": true})),
        })
        .unwrap();
        assert_eq!(value["status"], "resolved");
        assert_eq!(value["payload"]["approved"], true);
    }

    #[test]
    fn get_run_outcome_is_pending_without_terminal_event() {
        assert_eq!(
            get_run_outcome(&[factory::run_started("t1", "r1")]),
            RunOutcome::Pending
        );
    }

    #[test]
    fn get_run_outcome_returns_finished_success() {
        assert_eq!(
            get_run_outcome(&[factory::run_finished("t1", "r1")]),
            RunOutcome::Finished(Some(RunFinishedOutcome::Success))
        );
    }

    #[test]
    fn get_run_outcome_returns_finished_interrupt() {
        let interrupts = vec![interrupt("i1", None)];
        let event = Event::RunFinished(RunFinishedEvent {
            thread_id: "t1".into(),
            run_id: "r1".into(),
            result: None,
            outcome: Some(RunFinishedOutcome::Interrupt {
                interrupts: interrupts.clone(),
            }),
            base: BaseEventFields::default(),
        });

        assert_eq!(
            get_run_outcome(&[event]),
            RunOutcome::Finished(Some(RunFinishedOutcome::Interrupt { interrupts }))
        );
    }

    #[test]
    fn get_run_outcome_returns_error() {
        assert_eq!(
            get_run_outcome(&[Event::RunError(RunErrorEvent {
                message: "boom".into(),
                code: Some("E_BOOM".into()),
                base: BaseEventFields::default(),
            })]),
            RunOutcome::Error {
                message: "boom".into(),
                code: Some("E_BOOM".into()),
            }
        );
    }

    #[test]
    fn later_terminal_event_wins() {
        let events = vec![
            Event::RunError(RunErrorEvent {
                message: "boom".into(),
                code: None,
                base: BaseEventFields::default(),
            }),
            factory::run_finished("t1", "r1"),
        ];

        assert_eq!(
            get_run_outcome(&events),
            RunOutcome::Finished(Some(RunFinishedOutcome::Success))
        );
    }

    #[test]
    fn interrupt_expiration_is_false_for_non_interrupt_event() {
        assert!(!is_interrupt_expired(
            &factory::run_started("t1", "r1"),
            "2026-01-01T00:00:00Z"
        ));
    }

    #[test]
    fn interrupt_expiration_uses_lexicographic_iso_compare() {
        let event = Event::RunFinished(RunFinishedEvent {
            thread_id: "t1".into(),
            run_id: "r1".into(),
            result: None,
            outcome: Some(RunFinishedOutcome::Interrupt {
                interrupts: vec![interrupt("i1", Some("2026-04-22T12:00:00Z"))],
            }),
            base: BaseEventFields::default(),
        });

        assert!(!is_interrupt_expired(&event, "2026-04-22T11:59:59Z"));
        assert!(is_interrupt_expired(&event, "2026-04-22T12:00:00Z"));
        assert!(is_interrupt_expired(&event, "2026-04-22T12:00:01Z"));
    }

    #[test]
    fn build_resume_array_preserves_interrupt_order() {
        let interrupts = vec![interrupt("i1", None), interrupt("i2", None)];
        let responses = HashMap::from([
            (
                "i1".to_string(),
                ResumeResponse::Resolved {
                    payload: Some(json!({"approved": true})),
                },
            ),
            ("i2".to_string(), ResumeResponse::Cancelled),
        ]);

        let result = build_resume_array(&interrupts, &responses).unwrap();
        assert_eq!(result[0].interrupt_id, "i1");
        assert_eq!(result[0].status, agui_rs_core::ResumeStatus::Resolved);
        assert_eq!(result[0].payload, Some(json!({"approved": true})));
        assert_eq!(result[1].interrupt_id, "i2");
        assert_eq!(result[1].status, agui_rs_core::ResumeStatus::Cancelled);
        assert_eq!(result[1].payload, None);
    }

    #[test]
    fn build_resume_array_errors_on_missing_response() {
        let interrupts = vec![interrupt("i1", None), interrupt("i2", None)];
        let responses =
            HashMap::from([("i1".to_string(), ResumeResponse::Resolved { payload: None })]);

        let error = build_resume_array(&interrupts, &responses).unwrap_err();
        assert!(error.to_string().contains("i2"));
    }

    #[test]
    fn build_resume_array_errors_on_unknown_response() {
        let interrupts = vec![interrupt("i1", None)];
        let responses = HashMap::from([
            ("i1".to_string(), ResumeResponse::Resolved { payload: None }),
            ("i2".to_string(), ResumeResponse::Cancelled),
        ]);

        let error = build_resume_array(&interrupts, &responses).unwrap_err();
        assert!(error.to_string().contains("i2"));
    }
}
