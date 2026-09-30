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
                .any(|entry| interrupt_is_expired(entry, now_iso)),
            _ => false,
        },
        _ => false,
    }
}

/// Checks whether a single [`Interrupt`] has expired relative to an ISO-8601
/// `now` timestamp.
///
/// Mirrors `isInterruptExpired(interrupt, now)` upstream
/// (`interrupts/index.ts:13-16`): no `expiresAt` never expires, otherwise the
/// value is PARSED as a date and `parsed <= now` decides. Parsing, not string
/// order, is the whole behaviour — `2026-09-18T00:00:00+02:00` is earlier than
/// `2026-09-17T23:00:00Z` while sorting after it, and the schema deliberately
/// leaves the field unconstrained: *"a consumer comparing this value will parse
/// it as a date, so a value that is not one leaves the interrupt looking
/// permanently unexpired"* (`$defs.Interrupt.properties.expiresAt`).
pub fn interrupt_is_expired(interrupt: &Interrupt, now_iso: &str) -> bool {
    let Some(expires_at) = interrupt.expires_at.as_deref() else {
        return false;
    };
    match (
        parse_iso8601_millis(expires_at),
        parse_iso8601_millis(now_iso),
    ) {
        (Some(expires), Some(now)) => expires <= now,
        // No comparison, so no expiry: `new Date("soon")` upstream is an
        // Invalid Date and every comparison against it is false. An
        // unparseable `now_iso` is the caller's own clock failing the same
        // way.
        _ => false,
    }
}

/// Epoch milliseconds for an ISO-8601 date, or `None` when the value is not
/// one — the `Invalid Date` case, which must never expire.
///
/// Deliberately narrow: `YYYY-MM-DD`, optional `T HH:MM[:SS[.fff]]`, optional
/// `Z` or `±HH[:]MM`. The schema pins no format (`expiresAt` is a bare
/// `string`), so this covers the convention it documents and nothing more;
/// anything else reads as unexpired, which is the schema's own instruction.
fn parse_iso8601_millis(value: &str) -> Option<i64> {
    let value = value.trim();
    let (date, clock) = match value.find(['T', 't']) {
        Some(index) => (&value[..index], Some(&value[index + 1..])),
        None => (value, None),
    };

    let mut date_parts = date.split('-');
    let year = digits(date_parts.next()?)?;
    let month = digits(date_parts.next()?)?;
    let day = digits(date_parts.next()?)?;
    if date_parts.next().is_some() || !(1..=12).contains(&month) || day < 1 {
        return None;
    }
    const MONTH_DAYS: [i64; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
    let mut month_days = MONTH_DAYS[(month - 1) as usize];
    if month == 2 && leap {
        month_days = 29;
    }
    if day > month_days {
        return None;
    }

    let (hour, minute, second, millis, offset) = match clock {
        None => (0, 0, 0, 0, 0),
        Some(clock) => {
            let (clock, offset) = split_offset(clock)?;
            let mut clock_parts = clock.split(':');
            let hour = digits(clock_parts.next()?)?;
            let minute = digits(clock_parts.next()?)?;
            let (second, millis) = match clock_parts.next() {
                None => (0, 0),
                Some(second) => match second.split_once('.') {
                    Some((second, fraction)) => (digits(second)?, fraction_millis(fraction)?),
                    None => (digits(second)?, 0),
                },
            };
            if clock_parts.next().is_some() || hour > 23 || minute > 59 || second > 59 {
                return None;
            }
            (hour, minute, second, millis, offset)
        }
    };

    Some(
        days_from_civil(year, month, day) * 86_400_000
            + hour * 3_600_000
            + minute * 60_000
            + second * 1_000
            + millis
            - offset,
    )
}

/// Splits a clock from its zone designator, returning the offset in millis.
fn split_offset(clock: &str) -> Option<(&str, i64)> {
    if let Some(clock) = clock.strip_suffix('Z').or_else(|| clock.strip_suffix('z')) {
        return Some((clock, 0));
    }
    // Only a zone offset can carry a sign; hours and minutes are bare digits.
    let index = clock.rfind(['+', '-'])?;
    let (clock, zone) = clock.split_at(index);
    let mut parts = zone[1..].split(':');
    let hours = digits(parts.next()?)?;
    let minutes = match parts.next() {
        Some(minutes) => digits(minutes)?,
        None => 0,
    };
    if parts.next().is_some() || hours > 23 || minutes > 59 {
        return None;
    }
    let sign = if zone.starts_with('-') { -1 } else { 1 };
    Some((clock, sign * (hours * 60 + minutes) * 60_000))
}

/// Fractional seconds as whole millis, truncated at the third digit — the
/// resolution a date has.
fn fraction_millis(fraction: &str) -> Option<i64> {
    if fraction.is_empty() || !fraction.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let mut millis = 0;
    for index in 0..3 {
        millis *= 10;
        millis += fraction
            .as_bytes()
            .get(index)
            .map_or(0, |byte| i64::from(byte - b'0'));
    }
    Some(millis)
}

fn digits(value: &str) -> Option<i64> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    value.parse().ok()
}

/// Days between 1970-01-01 and a proleptic Gregorian date.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = year - i64::from(month <= 2);
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let day_of_year = (153 * ((month + 9) % 12) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// Validates that `resume` entries address every still-open interrupt and that
/// no expired interrupt is left answered rather than cancelled.
///
/// Mirrors the enforcement TypeScript `AbstractAgent.onInitialize` performs
/// before a `runAgent` run when `pendingInterrupts` is non-empty
/// (`agent/agent.ts:576-601`):
/// - every pending interrupt id must appear in `resume`, otherwise a
///   [`AgUiError::Validation`] listing the uncovered ids is returned;
/// - an expired interrupt is foreclosed, but a `cancelled` entry is the
///   conforming way past it: *"Expiry forecloses ANSWERING, not resolving the
///   thread … Throwing on mere presence — which this once did — made an
///   expired interrupt block its thread forever, since coverage is mandatory
///   and no entry could ever satisfy this check."*
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
        if !interrupt_is_expired(interrupt, now_iso) {
            continue;
        }
        let cancelled = resume.iter().any(|entry| {
            entry.interrupt_id == interrupt.id && matches!(entry.status, ResumeStatus::Cancelled)
        });
        if !cancelled {
            return Err(AgUiError::validation(format!(
                "Interrupt {} expired at {} and can no longer be answered. Cancel it to continue the thread.",
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
                metadata: None,
            },
            Some(ResumeResponse::Cancelled) => ResumeEntry {
                interrupt_id: interrupt.id.clone(),
                status: ResumeStatus::Cancelled,
                payload: None,
                metadata: None,
            },
            None => unreachable!("validated missing responses before mapping"),
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use agui_rs_core::{
        factory, BaseEventFields, Event, Interrupt, ResumeEntry, ResumeStatus, RunErrorEvent,
        RunFinishedEvent, RunFinishedOutcome,
    };
    use serde_json::json;

    use super::{
        build_resume_array, ensure_resume_covers, get_run_outcome, is_interrupt_expired,
        ResumeResponse, RunOutcome,
    };

    fn interrupt(id: &str, expires_at: Option<&str>) -> Interrupt {
        Interrupt {
            id: id.into(),
            subagent_run_id: None,
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
            RunOutcome::Finished(Some(RunFinishedOutcome::Success {
                pending_tool_call_ids: None,
            }))
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
            usage: Vec::new(),
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
                usage: Vec::new(),
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
                usage: Vec::new(),
                base: BaseEventFields::default(),
            }),
            factory::run_finished("t1", "r1"),
        ];

        assert_eq!(
            get_run_outcome(&events),
            RunOutcome::Finished(Some(RunFinishedOutcome::Success {
                pending_tool_call_ids: None,
            }))
        );
    }

    #[test]
    fn interrupt_expiration_is_false_for_non_interrupt_event() {
        assert!(!is_interrupt_expired(
            &factory::run_started("t1", "r1"),
            "2026-01-01T00:00:00Z"
        ));
    }

    fn finished_with_interrupts(expires_at: Option<&str>) -> Event {
        Event::RunFinished(RunFinishedEvent {
            thread_id: "t1".into(),
            run_id: "r1".into(),
            result: None,
            outcome: Some(RunFinishedOutcome::Interrupt {
                interrupts: vec![interrupt("i1", expires_at)],
            }),
            usage: Vec::new(),
            base: BaseEventFields::default(),
        })
    }

    #[test]
    fn interrupt_expiration_compares_parsed_instants() {
        let event = finished_with_interrupts(Some("2026-04-22T12:00:00Z"));

        assert!(!is_interrupt_expired(&event, "2026-04-22T11:59:59Z"));
        assert!(is_interrupt_expired(&event, "2026-04-22T12:00:00Z"));
        assert!(is_interrupt_expired(&event, "2026-04-22T12:00:01Z"));
    }

    /// `+02:00` puts the expiry two hours EARLIER than its own date reads, so
    /// a lexicographic compare — the shape this used to have — reports it live
    /// and lets an interrupt nobody can answer block its thread as answerable.
    #[test]
    fn a_positive_offset_expiry_expires_against_an_earlier_utc_now() {
        let event = finished_with_interrupts(Some("2026-09-18T00:00:00+02:00"));

        assert!(!is_interrupt_expired(&event, "2026-09-17T21:59:59Z"));
        assert!(is_interrupt_expired(&event, "2026-09-17T22:00:00Z"));
        assert!(is_interrupt_expired(&event, "2026-09-17T23:00:00Z"));
    }

    #[test]
    fn a_negative_offset_expiry_expires_against_a_later_utc_now() {
        let event = finished_with_interrupts(Some("2026-09-17T20:00:00-05:00"));

        assert!(!is_interrupt_expired(&event, "2026-09-18T00:59:59Z"));
        assert!(is_interrupt_expired(&event, "2026-09-18T01:00:00Z"));
    }

    /// Millisecond resolution is what `<=` at the boundary turns on: an expiry
    /// a single millisecond in the future has not expired yet.
    #[test]
    fn millisecond_precision_decides_the_boundary() {
        let event = finished_with_interrupts(Some("2026-09-17T12:00:00.250Z"));

        assert!(!is_interrupt_expired(&event, "2026-09-17T12:00:00.249Z"));
        assert!(is_interrupt_expired(&event, "2026-09-17T12:00:00.250Z"));
        assert!(is_interrupt_expired(&event, "2026-09-17T12:00:00.2505Z"));
    }

    /// The schema: a value that is not a date "leaves the interrupt looking
    /// permanently unexpired". A lexicographic compare said the opposite for
    /// every non-date that sorts after `"2026-…"`, and expired interrupts the
    /// caller then had to cancel by hand.
    #[test]
    fn a_value_that_is_not_a_date_never_expires() {
        for expires_at in ["soon", "", "not-a-date", "2026-13-45T99:99:99Z"] {
            let event = finished_with_interrupts(Some(expires_at));
            assert!(
                !is_interrupt_expired(&event, "2999-01-01T00:00:00Z"),
                "{expires_at} should leave the interrupt unexpired"
            );
        }
    }

    #[test]
    fn a_leap_day_expiry_is_read_as_a_real_date() {
        let event = finished_with_interrupts(Some("2028-02-29T00:00:00Z"));

        assert!(!is_interrupt_expired(&event, "2028-02-28T23:59:59Z"));
        assert!(is_interrupt_expired(&event, "2028-03-01T00:00:00Z"));
        // 2027 is not a leap year, so this is not a date at all.
        let event = finished_with_interrupts(Some("2027-02-29T00:00:00Z"));
        assert!(!is_interrupt_expired(&event, "2999-01-01T00:00:00Z"));
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

    fn entry(id: &str, status: ResumeStatus) -> ResumeEntry {
        ResumeEntry {
            interrupt_id: id.into(),
            status,
            payload: None,
            metadata: None,
        }
    }

    const NOW: &str = "2026-09-17T12:00:00Z";

    #[test]
    fn uncovered_interrupts_are_rejected() {
        let error = ensure_resume_covers(
            &[interrupt("i1", None), interrupt("i2", None)],
            &[entry("i1", ResumeStatus::Resolved)],
            NOW,
        )
        .unwrap_err();

        assert_eq!(
            error.to_string(),
            "event validation failed: Thread has 1 pending interrupt(s) not addressed by resume: i2"
        );
    }

    #[test]
    fn an_expired_interrupt_needs_a_cancelled_entry_to_let_the_thread_through() {
        let pending = [interrupt("i1", Some("2026-09-16T00:00:00Z"))];

        // Answered, not cancelled: expiry forecloses ANSWERING.
        let error = ensure_resume_covers(&pending, &[entry("i1", ResumeStatus::Resolved)], NOW)
            .unwrap_err();
        assert_eq!(
            error.to_string(),
            "event validation failed: Interrupt i1 expired at 2026-09-16T00:00:00Z and can no longer be answered. Cancel it to continue the thread."
        );

        // Cancelled is the conforming way past an interrupt nobody answered in
        // time, and it must NOT block its thread forever.
        ensure_resume_covers(&pending, &[entry("i1", ResumeStatus::Cancelled)], NOW).unwrap();
    }

    #[test]
    fn a_live_interrupt_may_be_answered_normally() {
        let pending = [interrupt("i1", Some("2026-09-18T00:00:00Z"))];
        ensure_resume_covers(&pending, &[entry("i1", ResumeStatus::Resolved)], NOW).unwrap();
    }

    #[test]
    fn an_offset_expiry_forecloses_answering_like_any_expired_one() {
        // `NOW` is 2026-09-17T12:00:00Z; this expiry is 2026-09-16T22:00:00Z.
        let pending = [interrupt("i1", Some("2026-09-17T00:00:00+02:00"))];

        let error = ensure_resume_covers(&pending, &[entry("i1", ResumeStatus::Resolved)], NOW)
            .unwrap_err();
        assert!(error.to_string().contains("no longer be answered"));
        ensure_resume_covers(&pending, &[entry("i1", ResumeStatus::Cancelled)], NOW).unwrap();
    }

    #[test]
    fn an_interrupt_without_an_expiry_never_expires() {
        let pending = [interrupt("i1", None)];
        ensure_resume_covers(&pending, &[entry("i1", ResumeStatus::Resolved)], NOW).unwrap();
    }
}
