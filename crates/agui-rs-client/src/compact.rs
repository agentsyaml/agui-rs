use std::collections::HashMap;

use agui_rs_core::{
    merge_metadata, AttributableFields, BaseEventFields, Event, StateSnapshotEvent,
    TextMessageContentEvent, ToolCallArgsEvent,
};
use serde_json::Value;

/// Upstream `carryStartMetadata` (`compact.ts:40-51`). A start event can be
/// replayed before its end — the HITL re-sync path does this, which is why the
/// reducer's start handling is idempotent. Uncompacted, both starts merge into
/// the message; keep that true after compaction instead of letting the later
/// start silently replace the earlier one's keys.
fn carry_start_metadata(previous: Option<Event>, next: Event) -> Event {
    let Some(previous) = previous else {
        return next;
    };
    let merged = merge_metadata(
        previous.base().metadata.as_ref(),
        next.base().metadata.as_ref(),
    );
    let mut next = next;
    next.base_mut().metadata = merged;
    next
}

/// Upstream `replaceStartFields` (`compact.ts:59-65`). A start replayed *after*
/// deltas have been buffered keeps its own non-metadata fields — the reducer
/// deliberately renames an existing tool call on such a replay — but its
/// metadata does not ride the start, because compaction emits the start ahead
/// of the collapsed delta event. The caller stages it separately so arrival
/// order is preserved.
fn replace_start_fields(previous: Option<Event>, next: Event) -> Event {
    let carried = previous.and_then(|previous| previous.base().metadata.clone());
    let mut next = next;
    next.base_mut().metadata = carried;
    next
}

/// Upstream `collapseMetadata` (`compact.ts:68-73`).
fn collapse_metadata<'a>(events: impl Iterator<Item = &'a Event>) -> Option<Value> {
    let mut merged = None;
    for event in events {
        merged = merge_metadata(merged.as_ref(), event.base().metadata.as_ref());
    }
    merged
}

struct PendingTextMessage {
    start: Option<Event>,
    contents: Vec<TextMessageContentEvent>,
    end: Option<Event>,
    other_events: Vec<Event>,
    /// Metadata staged from events that arrived after the start was hoisted.
    post_start_metadata: Option<Value>,
}

impl PendingTextMessage {
    fn new() -> Self {
        Self {
            start: None,
            contents: Vec::new(),
            end: None,
            other_events: Vec::new(),
            post_start_metadata: None,
        }
    }

    fn is_open(&self) -> bool {
        self.start.is_some() && self.end.is_none()
    }
}

struct PendingToolCall {
    start: Option<Event>,
    args: Vec<ToolCallArgsEvent>,
    end: Option<Event>,
    other_events: Vec<Event>,
    /// See [`PendingTextMessage::post_start_metadata`].
    post_start_metadata: Option<Value>,
}

impl PendingToolCall {
    fn new() -> Self {
        Self {
            start: None,
            args: Vec::new(),
            end: None,
            other_events: Vec::new(),
            post_start_metadata: None,
        }
    }

    fn is_open(&self) -> bool {
        self.start.is_some() && self.end.is_none()
    }
}

/// Compacts streaming AG-UI event sequences.
///
/// Mirrors the canonical TypeScript `compactEvents`: only **text messages**,
/// **tool calls**, and **state** are compacted. Reasoning, activity, and all
/// other events pass through unchanged (buffered after an open text/tool
/// sequence when they appear mid-stream).
pub fn compact_events(events: Vec<Event>) -> Vec<Event> {
    let mut compacted = Vec::new();
    let mut pending_text_messages: HashMap<String, PendingTextMessage> = HashMap::new();
    let mut open_text_order = Vec::new();
    let mut pending_tool_calls: HashMap<String, PendingToolCall> = HashMap::new();
    let mut open_tool_order = Vec::new();
    // State compaction: collected within a run, flushed at RUN_STARTED
    // (pre-/inter-run), RUN_FINISHED / RUN_ERROR (in-run), and at end (trailing).
    let mut state_events: Vec<Event> = Vec::new();

    for event in events {
        match event {
            Event::RunStarted(_) => {
                // Flush any pre-run state events before starting a new run.
                flush_state(&mut state_events, &mut compacted);
                compacted.push(event);
            }
            Event::RunFinished(_) | Event::RunError(_) => {
                // Flush compacted state before the run boundary event.
                flush_state(&mut state_events, &mut compacted);
                compacted.push(event);
            }
            Event::StateSnapshot(_) | Event::StateDelta(_) => {
                state_events.push(event);
            }
            Event::TextMessageStart(start) => {
                let message_id = start.message_id.clone();
                let pending = pending_text_messages
                    .entry(message_id.clone())
                    .or_insert_with(PendingTextMessage::new);
                let start_event = Event::TextMessageStart(start);
                if pending.contents.is_empty() {
                    pending.start = Some(carry_start_metadata(pending.start.take(), start_event));
                } else {
                    let staged = merge_metadata(
                        pending.post_start_metadata.as_ref(),
                        start_event.base().metadata.as_ref(),
                    );
                    pending.post_start_metadata = staged;
                    pending.start = Some(replace_start_fields(pending.start.take(), start_event));
                }
                push_open_id(&mut open_text_order, &message_id);
            }
            Event::TextMessageContent(content) => {
                let staged = merge_metadata(
                    pending_text_messages
                        .entry(content.message_id.clone())
                        .or_insert_with(PendingTextMessage::new)
                        .post_start_metadata
                        .as_ref(),
                    content.base.metadata.as_ref(),
                );
                let pending = pending_text_messages
                    .entry(content.message_id.clone())
                    .or_insert_with(PendingTextMessage::new);
                pending.post_start_metadata = staged;
                pending.contents.push(content);
            }
            Event::TextMessageEnd(end) => {
                let message_id = end.message_id.clone();
                let pending = pending_text_messages
                    .entry(message_id.clone())
                    .or_insert_with(PendingTextMessage::new);
                pending.end = Some(Event::TextMessageEnd(end));
                flush_text_message(&message_id, &mut pending_text_messages, &mut compacted);
                remove_open_id(&mut open_text_order, &message_id);
            }
            Event::ToolCallStart(start) => {
                let tool_call_id = start.tool_call_id.clone();
                let pending = pending_tool_calls
                    .entry(tool_call_id.clone())
                    .or_insert_with(PendingToolCall::new);
                let start_event = Event::ToolCallStart(start);
                if pending.args.is_empty() {
                    pending.start = Some(carry_start_metadata(pending.start.take(), start_event));
                } else {
                    let staged = merge_metadata(
                        pending.post_start_metadata.as_ref(),
                        start_event.base().metadata.as_ref(),
                    );
                    pending.post_start_metadata = staged;
                    pending.start = Some(replace_start_fields(pending.start.take(), start_event));
                }
                push_open_id(&mut open_tool_order, &tool_call_id);
            }
            Event::ToolCallArgs(args) => {
                let staged = merge_metadata(
                    pending_tool_calls
                        .entry(args.tool_call_id.clone())
                        .or_insert_with(PendingToolCall::new)
                        .post_start_metadata
                        .as_ref(),
                    args.base.metadata.as_ref(),
                );
                let pending = pending_tool_calls
                    .entry(args.tool_call_id.clone())
                    .or_insert_with(PendingToolCall::new);
                pending.post_start_metadata = staged;
                pending.args.push(args);
            }
            Event::ToolCallEnd(end) => {
                let tool_call_id = end.tool_call_id.clone();
                let pending = pending_tool_calls
                    .entry(tool_call_id.clone())
                    .or_insert_with(PendingToolCall::new);
                pending.end = Some(Event::ToolCallEnd(end));
                flush_tool_call(&tool_call_id, &mut pending_tool_calls, &mut compacted);
                remove_open_id(&mut open_tool_order, &tool_call_id);
            }
            other => {
                if buffer_other_event(
                    &other,
                    &open_text_order,
                    &mut pending_text_messages,
                    &open_tool_order,
                    &mut pending_tool_calls,
                ) {
                    continue;
                }
                compacted.push(other);
            }
        }
    }

    for message_id in pending_text_messages.keys().cloned().collect::<Vec<_>>() {
        flush_text_message(&message_id, &mut pending_text_messages, &mut compacted);
    }
    for tool_call_id in pending_tool_calls.keys().cloned().collect::<Vec<_>>() {
        flush_tool_call(&tool_call_id, &mut pending_tool_calls, &mut compacted);
    }

    // Flush any remaining state events (incomplete run or events outside runs).
    flush_state(&mut state_events, &mut compacted);

    compacted
}

fn buffer_other_event(
    event: &Event,
    open_text_order: &[String],
    pending_text_messages: &mut HashMap<String, PendingTextMessage>,
    open_tool_order: &[String],
    pending_tool_calls: &mut HashMap<String, PendingToolCall>,
) -> bool {
    for message_id in open_text_order {
        if let Some(pending) = pending_text_messages.get_mut(message_id) {
            if pending.is_open() {
                pending.other_events.push(event.clone());
                return true;
            }
        }
    }

    for tool_call_id in open_tool_order {
        if let Some(pending) = pending_tool_calls.get_mut(tool_call_id) {
            if pending.is_open() {
                pending.other_events.push(event.clone());
                return true;
            }
        }
    }

    false
}

/// Reduces a collected `STATE_SNAPSHOT` / `STATE_DELTA` window into a single
/// `STATE_SNAPSHOT` representing the final state, then clears the accumulator.
///
/// Mirrors the canonical TS `flushState` (`compact.ts:381-454`). A window with
/// **no** SNAPSHOT in it cannot be collapsed into one: deltas alone are
/// relative to a state this window never saw, so seeding `{}` would manufacture
/// an authoritative claim that everything else was absent and wipe consumer
/// state on replay — those windows pass through unchanged instead
/// (`compact.ts:407-419`). When a snapshot *is* present, folding starts at the
/// **last** snapshot (`compact.ts:408-424`): everything before it is
/// unobservable, because a snapshot restates the whole document.
fn flush_state(state_events: &mut Vec<Event>, compacted: &mut Vec<Event>) {
    if state_events.is_empty() {
        return;
    }

    // Upstream `compact.ts:408-418`: reverse scan for the LAST snapshot.
    let last_snapshot = state_events
        .iter()
        .rposition(|event| matches!(event, Event::StateSnapshot(_)));

    // Upstream `compact.ts:415-419`: no snapshot → pass the window through
    // unchanged rather than manufacturing a snapshot.
    let Some(last_snapshot) = last_snapshot else {
        compacted.append(state_events);
        return;
    };

    // Upstream `compact.ts:445` collapses the metadata of every state event
    // in the window onto the snapshot itself — note this is the whole window,
    // not just the folded tail.
    let collapsed_metadata = collapse_metadata(state_events.iter());

    // Upstream `compact.ts:422-424`: seed `{}` and fold only from the last
    // snapshot onward; the snapshot replaces the document wholesale, so
    // pre-snapshot events are dropped (`slice(lastSnapshot)`).
    let mut state = Value::Object(serde_json::Map::new());
    for event in state_events.drain(last_snapshot..) {
        match event {
            Event::StateSnapshot(snapshot) => {
                state = snapshot.snapshot;
            }
            Event::StateDelta(delta) => {
                if let Ok(patch) =
                    serde_json::from_value::<json_patch::Patch>(Value::Array(delta.delta))
                {
                    // Best-effort: a failed patch leaves the prior state intact,
                    // matching the lenient reducer used elsewhere in the client.
                    let _ = json_patch::patch(&mut state, &patch);
                }
            }
            _ => {}
        }
    }
    // Drop the discarded pre-snapshot prefix left behind by the ranged drain.
    state_events.clear();

    compacted.push(Event::StateSnapshot(StateSnapshotEvent {
        snapshot: state,
        base: BaseEventFields {
            metadata: collapsed_metadata,
            ..Default::default()
        },
        attributable: AttributableFields::default(),
    }));
}

fn flush_text_message(
    message_id: &str,
    pending_text_messages: &mut HashMap<String, PendingTextMessage>,
    compacted: &mut Vec<Event>,
) {
    let Some(pending) = pending_text_messages.remove(message_id) else {
        return;
    };

    if let Some(start) = pending.start {
        compacted.push(start);
    }

    if !pending.contents.is_empty() {
        compacted.push(Event::TextMessageContent(TextMessageContentEvent {
            message_id: message_id.to_string(),
            delta: pending
                .contents
                .into_iter()
                .map(|part| part.delta)
                .collect(),
            base: BaseEventFields {
                metadata: pending.post_start_metadata,
                ..Default::default()
            },
            attributable: AttributableFields::default(),
        }));
    }

    if let Some(end) = pending.end {
        compacted.push(end);
    }

    compacted.extend(pending.other_events);
}

fn flush_tool_call(
    tool_call_id: &str,
    pending_tool_calls: &mut HashMap<String, PendingToolCall>,
    compacted: &mut Vec<Event>,
) {
    let Some(pending) = pending_tool_calls.remove(tool_call_id) else {
        return;
    };

    if let Some(start) = pending.start {
        compacted.push(start);
    }

    if !pending.args.is_empty() {
        compacted.push(Event::ToolCallArgs(ToolCallArgsEvent {
            tool_call_id: tool_call_id.to_string(),
            delta: pending.args.into_iter().map(|part| part.delta).collect(),
            base: BaseEventFields {
                metadata: pending.post_start_metadata,
                ..Default::default()
            },
            attributable: AttributableFields::default(),
        }));
    }

    if let Some(end) = pending.end {
        compacted.push(end);
    }

    compacted.extend(pending.other_events);
}

fn push_open_id(order: &mut Vec<String>, id: &str) {
    if !order.iter().any(|current| current == id) {
        order.push(id.to_string());
    }
}

fn remove_open_id(order: &mut Vec<String>, id: &str) {
    if let Some(index) = order.iter().position(|current| current == id) {
        order.remove(index);
    }
}

#[cfg(test)]
mod tests {
    use agui_rs_core::{
        factory, AttributableFields, BaseEventFields, CustomEvent, Event, ReasoningEndEvent,
        ReasoningMessageContentEvent, ReasoningMessageEndEvent, ReasoningMessageRole,
        ReasoningMessageStartEvent, ReasoningStartEvent, TextMessageRole,
    };
    use serde_json::json;

    use super::compact_events;

    fn text_start(message_id: &str) -> Event {
        Event::TextMessageStart(agui_rs_core::TextMessageStartEvent {
            message_id: message_id.into(),
            role: TextMessageRole::Assistant,
            attributable: AttributableFields::default(),
            name: None,
            base: BaseEventFields::default(),
        })
    }

    fn reasoning_start(message_id: &str) -> Event {
        Event::ReasoningStart(ReasoningStartEvent {
            message_id: message_id.into(),
            base: BaseEventFields::default(),
            attributable: AttributableFields::default(),
        })
    }

    fn reasoning_message_start(message_id: &str) -> Event {
        Event::ReasoningMessageStart(ReasoningMessageStartEvent {
            message_id: message_id.into(),
            role: ReasoningMessageRole::Reasoning,
            attributable: AttributableFields::default(),
            base: BaseEventFields::default(),
        })
    }

    fn reasoning_content(message_id: &str, delta: &str) -> Event {
        Event::ReasoningMessageContent(ReasoningMessageContentEvent {
            message_id: message_id.into(),
            delta: delta.into(),
            base: BaseEventFields::default(),
            attributable: AttributableFields::default(),
        })
    }

    fn reasoning_message_end(message_id: &str) -> Event {
        Event::ReasoningMessageEnd(ReasoningMessageEndEvent {
            message_id: message_id.into(),
            base: BaseEventFields::default(),
            attributable: AttributableFields::default(),
        })
    }

    fn reasoning_end(message_id: &str) -> Event {
        Event::ReasoningEnd(ReasoningEndEvent {
            message_id: message_id.into(),
            base: BaseEventFields::default(),
            attributable: AttributableFields::default(),
        })
    }

    #[test]
    fn compacts_empty_stream() {
        assert!(compact_events(Vec::new()).is_empty());
    }

    #[test]
    fn compacts_text_message_contents() {
        let result = compact_events(vec![
            text_start("m1"),
            factory::text_message_content("m1", "hel"),
            factory::text_message_content("m1", "lo"),
            factory::text_message_end("m1"),
        ]);

        assert_eq!(result.len(), 3);
        assert_eq!(result[0], text_start("m1"));
        assert_eq!(result[1], factory::text_message_content("m1", "hello"));
        assert_eq!(result[2], factory::text_message_end("m1"));
    }

    #[test]
    fn moves_interleaved_events_after_text_message() {
        let interleaved = Event::Custom(CustomEvent {
            name: "mark".into(),
            value: json!({"x": 1}),
            base: BaseEventFields::default(),
            attributable: AttributableFields::default(),
        });
        let result = compact_events(vec![
            text_start("m1"),
            interleaved.clone(),
            factory::text_message_content("m1", "a"),
            factory::text_message_content("m1", "b"),
            factory::text_message_end("m1"),
        ]);

        assert_eq!(
            result,
            vec![
                text_start("m1"),
                factory::text_message_content("m1", "ab"),
                factory::text_message_end("m1"),
                interleaved,
            ]
        );
    }

    #[test]
    fn compacts_tool_call_args() {
        let result = compact_events(vec![
            factory::tool_call_start("tc1", "search"),
            factory::tool_call_args("tc1", "{\"q\":"),
            factory::tool_call_args("tc1", "\"rust\"}"),
            factory::tool_call_end("tc1"),
        ]);

        assert_eq!(
            result,
            vec![
                factory::tool_call_start("tc1", "search"),
                factory::tool_call_args("tc1", "{\"q\":\"rust\"}"),
                factory::tool_call_end("tc1"),
            ]
        );
    }

    // TS `compactEvents` does NOT compact reasoning events — they are not in the
    // streaming set (only text / tool-call / state are). Reasoning content
    // passes through unchanged, even across multiple content deltas.
    #[test]
    fn reasoning_events_pass_through_uncompacted() {
        let events = vec![
            reasoning_start("r1"),
            reasoning_message_start("r1"),
            reasoning_content("r1", "step 1"),
            reasoning_content("r1", " + step 2"),
            reasoning_message_end("r1"),
            reasoning_end("r1"),
        ];

        // Identity: nothing is merged or reordered.
        assert_eq!(compact_events(events.clone()), events);
    }

    // Reasoning events do not open a streaming sequence, so an interleaved event
    // is NOT buffered/reordered around them — it stays in place. (Contrast with
    // text/tool sequences, which do reorder interleaved events.)
    #[test]
    fn interleaved_events_around_reasoning_keep_their_position() {
        let step = factory::step_started("plan");
        let events = vec![
            reasoning_start("r1"),
            reasoning_message_start("r1"),
            step.clone(),
            reasoning_content("r1", "a"),
            reasoning_message_end("r1"),
            reasoning_end("r1"),
        ];

        assert_eq!(compact_events(events.clone()), events);
    }

    #[test]
    fn flushes_incomplete_sequences_at_end() {
        let result = compact_events(vec![
            text_start("m1"),
            factory::text_message_content("m1", "hi"),
            factory::tool_call_start("tc1", "search"),
            factory::tool_call_args("tc1", "{}"),
        ]);

        assert_eq!(
            result,
            vec![
                text_start("m1"),
                factory::text_message_content("m1", "hi"),
                factory::tool_call_start("tc1", "search"),
                factory::tool_call_args("tc1", "{}"),
            ]
        );
    }

    #[test]
    fn keeps_unrelated_events_in_place() {
        let run_started = factory::run_started("t1", "r1");
        let run_finished = factory::run_finished("t1", "r1");
        let result = compact_events(vec![run_started.clone(), run_finished.clone()]);

        assert_eq!(result, vec![run_started, run_finished]);
    }

    #[test]
    fn is_idempotent() {
        let events = vec![
            text_start("m1"),
            factory::text_message_content("m1", "hello"),
            factory::text_message_end("m1"),
            factory::tool_call_start("tc1", "search"),
            factory::tool_call_args("tc1", "{}"),
            factory::tool_call_end("tc1"),
            reasoning_start("r1"),
            reasoning_message_start("r1"),
            reasoning_content("r1", "think"),
            reasoning_message_end("r1"),
            reasoning_end("r1"),
        ];

        assert_eq!(
            compact_events(events.clone()),
            compact_events(compact_events(events))
        );
    }

    // Ported from TypeScript `compact/__tests__/compact.test.ts` → "State Compaction".
    mod state_compaction {
        use super::*;

        fn snapshot(event: &Event) -> &serde_json::Value {
            match event {
                Event::StateSnapshot(e) => &e.snapshot,
                other => panic!("expected STATE_SNAPSHOT, got {other:?}"),
            }
        }

        #[test]
        fn compacts_multiple_state_snapshots_into_one_per_run() {
            let result = compact_events(vec![
                factory::run_started("t1", "r1"),
                factory::state_snapshot(json!({"count": 1})),
                factory::state_snapshot(json!({"count": 2})),
                factory::state_snapshot(json!({"count": 3})),
                factory::run_finished("t1", "r1"),
            ]);

            assert_eq!(result.len(), 3);
            assert!(matches!(result[0], Event::RunStarted(_)));
            assert_eq!(snapshot(&result[1]), &json!({"count": 3}));
            assert!(matches!(result[2], Event::RunFinished(_)));
        }

        #[test]
        fn compacts_snapshot_plus_deltas_into_a_single_snapshot() {
            let result = compact_events(vec![
                factory::run_started("t1", "r1"),
                factory::state_snapshot(json!({"count": 0, "name": "test"})),
                factory::state_delta(vec![json!({"op": "replace", "path": "/count", "value": 1})]),
                factory::state_delta(vec![json!({"op": "replace", "path": "/count", "value": 2})]),
                factory::run_finished("t1", "r1"),
            ]);

            assert_eq!(result.len(), 3);
            assert_eq!(snapshot(&result[1]), &json!({"count": 2, "name": "test"}));
        }

        // Upstream `compact.state-window.test.ts` → "keeps the deltas as deltas
        // rather than manufacturing a snapshot": a window with no SNAPSHOT is
        // relative to state this window never saw, so it passes through
        // unchanged instead of being collapsed into an authoritative snapshot.
        #[test]
        fn passes_a_delta_only_window_through_without_manufacturing_a_snapshot() {
            let run_started = factory::run_started("t1", "r1");
            let delta_1 =
                factory::state_delta(vec![json!({"op": "add", "path": "/foo", "value": "bar"})]);
            let delta_2 =
                factory::state_delta(vec![json!({"op": "add", "path": "/baz", "value": 42})]);
            let run_finished = factory::run_finished("t1", "r1");
            let events = vec![
                run_started.clone(),
                delta_1.clone(),
                delta_2.clone(),
                run_finished.clone(),
            ];

            // Identity: no snapshot is manufactured from the deltas.
            assert_eq!(compact_events(events.clone()), events);
        }

        // Upstream `compact.state-window.test.ts` → "folds from the LAST
        // snapshot in the window, not from the start of it": a delta ahead of a
        // snapshot is unobservable (the snapshot restates the whole document),
        // so it is dropped and only the tail is folded.
        #[test]
        fn folds_from_the_last_snapshot_and_drops_the_prefix() {
            let result = compact_events(vec![
                factory::run_started("t1", "r1"),
                factory::state_delta(vec![json!({"op": "replace", "path": "/a", "value": 1})]),
                factory::state_snapshot(json!({"b": 2})),
                factory::state_delta(vec![json!({"op": "add", "path": "/c", "value": 3})]),
                factory::run_finished("t1", "r1"),
            ]);

            assert_eq!(result.len(), 3);
            assert_eq!(snapshot(&result[1]), &json!({"b": 2, "c": 3}));
        }

        #[test]
        fn handles_snapshot_followed_by_delta_that_overwrites_it() {
            let result = compact_events(vec![
                factory::run_started("t1", "r1"),
                factory::state_snapshot(json!({"a": 1, "b": 2})),
                factory::state_delta(vec![json!({"op": "remove", "path": "/b"})]),
                factory::state_delta(vec![json!({"op": "add", "path": "/c", "value": 3})]),
                factory::run_finished("t1", "r1"),
            ]);

            assert_eq!(result.len(), 3);
            assert_eq!(snapshot(&result[1]), &json!({"a": 1, "c": 3}));
        }

        #[test]
        fn handles_multiple_runs_independently() {
            let result = compact_events(vec![
                factory::run_started("t1", "r1"),
                factory::state_snapshot(json!({"step": 1})),
                factory::state_delta(vec![json!({"op": "replace", "path": "/step", "value": 2})]),
                factory::run_finished("t1", "r1"),
                factory::run_started("t1", "r2"),
                factory::state_snapshot(json!({"step": 10})),
                factory::state_delta(vec![json!({"op": "replace", "path": "/step", "value": 20})]),
                factory::run_finished("t1", "r2"),
            ]);

            assert_eq!(result.len(), 6);
            assert_eq!(snapshot(&result[1]), &json!({"step": 2}));
            assert_eq!(snapshot(&result[4]), &json!({"step": 20}));
        }

        #[test]
        fn does_not_emit_state_snapshot_when_no_state_events_in_run() {
            let result = compact_events(vec![
                factory::run_started("t1", "r1"),
                text_start("msg1"),
                factory::text_message_content("msg1", "Hello"),
                factory::text_message_end("msg1"),
                factory::run_finished("t1", "r1"),
            ]);

            assert_eq!(result.len(), 5);
            assert_eq!(
                result
                    .iter()
                    .filter(|e| matches!(e, Event::StateSnapshot(_)))
                    .count(),
                0
            );
        }

        #[test]
        fn handles_state_events_outside_of_runs() {
            let result = compact_events(vec![
                factory::state_snapshot(json!({"x": 1})),
                factory::state_delta(vec![json!({"op": "replace", "path": "/x", "value": 2})]),
            ]);

            assert_eq!(result.len(), 1);
            assert_eq!(snapshot(&result[0]), &json!({"x": 2}));
        }

        #[test]
        fn handles_snapshot_after_deltas_within_a_run() {
            let result = compact_events(vec![
                factory::run_started("t1", "r1"),
                factory::state_delta(vec![json!({"op": "add", "path": "/old", "value": true})]),
                factory::state_snapshot(json!({"fresh": true})),
                factory::state_delta(vec![json!({"op": "add", "path": "/extra", "value": 1})]),
                factory::run_finished("t1", "r1"),
            ]);

            assert_eq!(result.len(), 3);
            assert_eq!(snapshot(&result[1]), &json!({"fresh": true, "extra": 1}));
        }

        #[test]
        fn preserves_non_state_events_alongside_state_compaction() {
            let result = compact_events(vec![
                factory::run_started("t1", "r1"),
                factory::state_snapshot(json!({"count": 0})),
                text_start("msg1"),
                factory::text_message_content("msg1", "Hi"),
                factory::text_message_end("msg1"),
                factory::state_delta(vec![json!({"op": "replace", "path": "/count", "value": 1})]),
                factory::run_finished("t1", "r1"),
            ]);

            assert_eq!(result.len(), 6);
            assert!(matches!(result[0], Event::RunStarted(_)));
            assert!(matches!(result[1], Event::TextMessageStart(_)));
            assert!(matches!(result[2], Event::TextMessageContent(_)));
            assert!(matches!(result[3], Event::TextMessageEnd(_)));
            assert_eq!(snapshot(&result[4]), &json!({"count": 1}));
            assert!(matches!(result[5], Event::RunFinished(_)));
        }

        #[test]
        fn flushes_state_events_before_run_started_when_they_precede_any_run() {
            let result = compact_events(vec![
                factory::state_snapshot(json!({"preRun": true})),
                factory::run_started("t1", "r1"),
                factory::state_snapshot(json!({"inRun": true})),
                factory::run_finished("t1", "r1"),
            ]);

            assert_eq!(result.len(), 4);
            assert_eq!(snapshot(&result[0]), &json!({"preRun": true}));
            assert!(matches!(result[1], Event::RunStarted(_)));
            assert_eq!(snapshot(&result[2]), &json!({"inRun": true}));
            assert!(matches!(result[3], Event::RunFinished(_)));
        }

        #[test]
        fn flushes_state_events_between_runs() {
            let result = compact_events(vec![
                factory::run_started("t1", "r1"),
                factory::state_snapshot(json!({"run": 1})),
                factory::run_finished("t1", "r1"),
                factory::state_snapshot(json!({"between": true})),
                factory::state_delta(vec![json!({"op": "add", "path": "/extra", "value": 1})]),
                factory::run_started("t1", "r2"),
                factory::state_snapshot(json!({"run": 2})),
                factory::run_finished("t1", "r2"),
            ]);

            assert_eq!(result.len(), 7);
            assert_eq!(snapshot(&result[1]), &json!({"run": 1}));
            assert_eq!(snapshot(&result[3]), &json!({"between": true, "extra": 1}));
            assert_eq!(snapshot(&result[5]), &json!({"run": 2}));
        }

        #[test]
        fn flushes_state_on_run_error() {
            let result = compact_events(vec![
                factory::run_started("t1", "r1"),
                factory::state_snapshot(json!({"count": 0})),
                factory::state_delta(vec![json!({"op": "replace", "path": "/count", "value": 5})]),
                factory::run_error("something failed"),
            ]);

            assert_eq!(result.len(), 3);
            assert!(matches!(result[0], Event::RunStarted(_)));
            assert_eq!(snapshot(&result[1]), &json!({"count": 5}));
            assert!(matches!(result[2], Event::RunError(_)));
        }

        #[test]
        fn handles_complex_nested_state_with_json_patch() {
            let result = compact_events(vec![
                factory::run_started("t1", "r1"),
                factory::state_snapshot(json!({
                    "users": [{"name": "Alice", "age": 30}],
                    "settings": {"theme": "dark"}
                })),
                factory::state_delta(vec![
                    json!({"op": "add", "path": "/users/-", "value": {"name": "Bob", "age": 25}}),
                ]),
                factory::state_delta(vec![
                    json!({"op": "replace", "path": "/settings/theme", "value": "light"}),
                ]),
                factory::run_finished("t1", "r1"),
            ]);

            assert_eq!(result.len(), 3);
            assert_eq!(
                snapshot(&result[1]),
                &json!({
                    "users": [{"name": "Alice", "age": 30}, {"name": "Bob", "age": 25}],
                    "settings": {"theme": "light"}
                })
            );
        }
    }

    fn text_start_with_metadata(message_id: &str, metadata: serde_json::Value) -> Event {
        Event::TextMessageStart(agui_rs_core::TextMessageStartEvent {
            message_id: message_id.into(),
            role: TextMessageRole::Assistant,
            attributable: AttributableFields::default(),
            name: None,
            base: BaseEventFields {
                metadata: Some(metadata),
                ..Default::default()
            },
        })
    }

    fn text_content_with_metadata(
        message_id: &str,
        delta: &str,
        metadata: serde_json::Value,
    ) -> Event {
        Event::TextMessageContent(agui_rs_core::TextMessageContentEvent {
            message_id: message_id.into(),
            delta: delta.into(),
            base: BaseEventFields {
                metadata: Some(metadata),
                ..Default::default()
            },
            attributable: AttributableFields::default(),
        })
    }

    /// Upstream `carryStartMetadata`: a start replayed before its end merges
    /// metadata instead of letting the later start replace the earlier keys.
    #[test]
    fn carries_metadata_across_a_replayed_start() {
        let result = compact_events(vec![
            text_start_with_metadata("m1", json!({"a": 1})),
            text_start_with_metadata("m1", json!({"b": 2})),
            factory::text_message_end("m1"),
        ]);

        assert_eq!(result[0].base().metadata, Some(json!({"a": 1, "b": 2})));
    }

    /// Upstream `replaceStartFields`: a start replayed once deltas are buffered
    /// keeps its own fields, and its metadata rides the collapsed delta rather
    /// than the hoisted start, so arrival order survives compaction.
    #[test]
    fn a_start_replayed_after_content_stages_its_metadata() {
        let result = compact_events(vec![
            text_start_with_metadata("m1", json!({"from": "first"})),
            text_content_with_metadata("m1", "hel", json!({"from": "content"})),
            text_start_with_metadata("m1", json!({"from": "replay"})),
            factory::text_message_end("m1"),
        ]);

        // The replayed start is not hoisted: its own metadata stays on it.
        assert_eq!(result[0].base().metadata, Some(json!({"from": "first"})));
        // The collapsed content carries the staged metadata.
        assert_eq!(result[1].base().metadata, Some(json!({"from": "replay"})));
    }

    /// Upstream `collapseMetadata` (`compact.ts:445`): the metadata of every
    /// state event folded onto the snapshot that replaces them.
    #[test]
    fn collapses_state_metadata_onto_the_snapshot() {
        let with_meta = |value: serde_json::Value, metadata: serde_json::Value| {
            Event::StateSnapshot(agui_rs_core::StateSnapshotEvent {
                snapshot: value,
                base: BaseEventFields {
                    metadata: Some(metadata),
                    ..Default::default()
                },
                attributable: AttributableFields::default(),
            })
        };

        let result = compact_events(vec![
            with_meta(json!({"n": 1}), json!({"a": 1})),
            with_meta(json!({"n": 2}), json!({"b": 2})),
            factory::run_started("thread", "run"),
            factory::run_finished("thread", "run"),
        ]);

        assert_eq!(result[0].base().metadata, Some(json!({"a": 1, "b": 2})));
    }
}
