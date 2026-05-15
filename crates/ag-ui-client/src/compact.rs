use std::collections::HashMap;

use ag_ui_core::{
    BaseEventFields, Event, ReasoningMessageContentEvent, TextMessageContentEvent,
    ToolCallArgsEvent,
};

struct PendingTextMessage {
    start: Option<Event>,
    contents: Vec<TextMessageContentEvent>,
    end: Option<Event>,
    other_events: Vec<Event>,
}

impl PendingTextMessage {
    fn new() -> Self {
        Self {
            start: None,
            contents: Vec::new(),
            end: None,
            other_events: Vec::new(),
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
}

impl PendingToolCall {
    fn new() -> Self {
        Self {
            start: None,
            args: Vec::new(),
            end: None,
            other_events: Vec::new(),
        }
    }

    fn is_open(&self) -> bool {
        self.start.is_some() && self.end.is_none()
    }
}

struct PendingReasoning {
    reasoning_start: Option<Event>,
    message_start: Option<Event>,
    contents: Vec<ReasoningMessageContentEvent>,
    message_end: Option<Event>,
    reasoning_end: Option<Event>,
    other_events: Vec<Event>,
}

impl PendingReasoning {
    fn new() -> Self {
        Self {
            reasoning_start: None,
            message_start: None,
            contents: Vec::new(),
            message_end: None,
            reasoning_end: None,
            other_events: Vec::new(),
        }
    }

    fn is_open(&self) -> bool {
        (self.reasoning_start.is_some() || self.message_start.is_some())
            && self.reasoning_end.is_none()
            && self.message_end.is_none()
    }
}

/// Compacts streaming AG-UI event sequences.
pub fn compact_events(events: Vec<Event>) -> Vec<Event> {
    let mut compacted = Vec::new();
    let mut pending_text_messages: HashMap<String, PendingTextMessage> = HashMap::new();
    let mut open_text_order = Vec::new();
    let mut pending_tool_calls: HashMap<String, PendingToolCall> = HashMap::new();
    let mut open_tool_order = Vec::new();
    let mut pending_reasoning: HashMap<String, PendingReasoning> = HashMap::new();
    let mut open_reasoning_order = Vec::new();

    for event in events {
        match event {
            Event::TextMessageStart(start) => {
                let message_id = start.message_id.clone();
                let pending = pending_text_messages
                    .entry(message_id.clone())
                    .or_insert_with(PendingTextMessage::new);
                pending.start = Some(Event::TextMessageStart(start));
                push_open_id(&mut open_text_order, &message_id);
            }
            Event::TextMessageContent(content) => {
                pending_text_messages
                    .entry(content.message_id.clone())
                    .or_insert_with(PendingTextMessage::new)
                    .contents
                    .push(content);
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
                pending.start = Some(Event::ToolCallStart(start));
                push_open_id(&mut open_tool_order, &tool_call_id);
            }
            Event::ToolCallArgs(args) => {
                pending_tool_calls
                    .entry(args.tool_call_id.clone())
                    .or_insert_with(PendingToolCall::new)
                    .args
                    .push(args);
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
            Event::ReasoningStart(start) => {
                let message_id = start.message_id.clone();
                let pending = pending_reasoning
                    .entry(message_id.clone())
                    .or_insert_with(PendingReasoning::new);
                pending.reasoning_start = Some(Event::ReasoningStart(start));
                push_open_id(&mut open_reasoning_order, &message_id);
            }
            Event::ReasoningMessageStart(start) => {
                let message_id = start.message_id.clone();
                let pending = pending_reasoning
                    .entry(message_id.clone())
                    .or_insert_with(PendingReasoning::new);
                pending.message_start = Some(Event::ReasoningMessageStart(start));
                push_open_id(&mut open_reasoning_order, &message_id);
            }
            Event::ReasoningMessageContent(content) => {
                pending_reasoning
                    .entry(content.message_id.clone())
                    .or_insert_with(PendingReasoning::new)
                    .contents
                    .push(content);
            }
            Event::ReasoningMessageEnd(end) => {
                let message_id = end.message_id.clone();
                let pending = pending_reasoning
                    .entry(message_id)
                    .or_insert_with(PendingReasoning::new);
                pending.message_end = Some(Event::ReasoningMessageEnd(end));
            }
            Event::ReasoningEnd(end) => {
                let message_id = end.message_id.clone();
                let pending = pending_reasoning
                    .entry(message_id.clone())
                    .or_insert_with(PendingReasoning::new);
                pending.reasoning_end = Some(Event::ReasoningEnd(end));
                flush_reasoning(&message_id, &mut pending_reasoning, &mut compacted);
                remove_open_id(&mut open_reasoning_order, &message_id);
            }
            other => {
                if buffer_other_event(
                    &other,
                    &open_text_order,
                    &mut pending_text_messages,
                    &open_tool_order,
                    &mut pending_tool_calls,
                    &open_reasoning_order,
                    &mut pending_reasoning,
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
    for message_id in pending_reasoning.keys().cloned().collect::<Vec<_>>() {
        flush_reasoning(&message_id, &mut pending_reasoning, &mut compacted);
    }

    compacted
}

fn buffer_other_event(
    event: &Event,
    open_text_order: &[String],
    pending_text_messages: &mut HashMap<String, PendingTextMessage>,
    open_tool_order: &[String],
    pending_tool_calls: &mut HashMap<String, PendingToolCall>,
    open_reasoning_order: &[String],
    pending_reasoning: &mut HashMap<String, PendingReasoning>,
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

    for message_id in open_reasoning_order {
        if let Some(pending) = pending_reasoning.get_mut(message_id) {
            if pending.is_open() {
                pending.other_events.push(event.clone());
                return true;
            }
        }
    }

    false
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
            base: BaseEventFields::default(),
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
            base: BaseEventFields::default(),
        }));
    }

    if let Some(end) = pending.end {
        compacted.push(end);
    }

    compacted.extend(pending.other_events);
}

fn flush_reasoning(
    message_id: &str,
    pending_reasoning: &mut HashMap<String, PendingReasoning>,
    compacted: &mut Vec<Event>,
) {
    let Some(pending) = pending_reasoning.remove(message_id) else {
        return;
    };

    if let Some(start) = pending.reasoning_start {
        compacted.push(start);
    }

    if let Some(start) = pending.message_start {
        compacted.push(start);
    }

    if !pending.contents.is_empty() {
        compacted.push(Event::ReasoningMessageContent(
            ReasoningMessageContentEvent {
                message_id: message_id.to_string(),
                delta: pending
                    .contents
                    .into_iter()
                    .map(|part| part.delta)
                    .collect(),
                base: BaseEventFields::default(),
            },
        ));
    }

    if let Some(end) = pending.message_end {
        compacted.push(end);
    }

    if let Some(end) = pending.reasoning_end {
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
    use ag_ui_core::{
        factory, BaseEventFields, CustomEvent, Event, ReasoningEndEvent,
        ReasoningMessageContentEvent, ReasoningMessageEndEvent, ReasoningMessageRole,
        ReasoningMessageStartEvent, ReasoningStartEvent, TextMessageRole,
    };
    use serde_json::json;

    use super::compact_events;

    fn text_start(message_id: &str) -> Event {
        Event::TextMessageStart(ag_ui_core::TextMessageStartEvent {
            message_id: message_id.into(),
            role: TextMessageRole::Assistant,
            name: None,
            base: BaseEventFields::default(),
        })
    }

    fn reasoning_start(message_id: &str) -> Event {
        Event::ReasoningStart(ReasoningStartEvent {
            message_id: message_id.into(),
            base: BaseEventFields::default(),
        })
    }

    fn reasoning_message_start(message_id: &str) -> Event {
        Event::ReasoningMessageStart(ReasoningMessageStartEvent {
            message_id: message_id.into(),
            role: ReasoningMessageRole::Reasoning,
            base: BaseEventFields::default(),
        })
    }

    fn reasoning_content(message_id: &str, delta: &str) -> Event {
        Event::ReasoningMessageContent(ReasoningMessageContentEvent {
            message_id: message_id.into(),
            delta: delta.into(),
            base: BaseEventFields::default(),
        })
    }

    fn reasoning_message_end(message_id: &str) -> Event {
        Event::ReasoningMessageEnd(ReasoningMessageEndEvent {
            message_id: message_id.into(),
            base: BaseEventFields::default(),
        })
    }

    fn reasoning_end(message_id: &str) -> Event {
        Event::ReasoningEnd(ReasoningEndEvent {
            message_id: message_id.into(),
            base: BaseEventFields::default(),
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

    #[test]
    fn compacts_reasoning_content() {
        let result = compact_events(vec![
            reasoning_start("r1"),
            reasoning_message_start("r1"),
            reasoning_content("r1", "step 1"),
            reasoning_content("r1", " + step 2"),
            reasoning_message_end("r1"),
            reasoning_end("r1"),
        ]);

        assert_eq!(
            result,
            vec![
                reasoning_start("r1"),
                reasoning_message_start("r1"),
                reasoning_content("r1", "step 1 + step 2"),
                reasoning_message_end("r1"),
                reasoning_end("r1"),
            ]
        );
    }

    #[test]
    fn moves_interleaved_events_after_reasoning() {
        let step = factory::step_started("plan");
        let result = compact_events(vec![
            reasoning_start("r1"),
            reasoning_message_start("r1"),
            step.clone(),
            reasoning_content("r1", "a"),
            reasoning_message_end("r1"),
            reasoning_end("r1"),
        ]);

        assert_eq!(
            result,
            vec![
                reasoning_start("r1"),
                reasoning_message_start("r1"),
                reasoning_content("r1", "a"),
                reasoning_message_end("r1"),
                reasoning_end("r1"),
                step,
            ]
        );
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
}
