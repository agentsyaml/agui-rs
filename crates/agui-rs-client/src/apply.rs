use agui_rs_core::types::{
    ActivityMessage, AssistantMessage, DeveloperMessage, ReasoningMessage, SystemMessage,
    ToolMessage, UserMessage,
};
use agui_rs_core::{
    ActivityDeltaEvent, ActivitySnapshotEvent, AgUiError, Event, FunctionCall, Message,
    ReasoningEncryptedValueSubtype, Result, State, StateDeltaEvent, TextMessageRole, ToolCall,
    ToolCallKind, ToolCallStartEvent, UserMessageContent,
};
use async_stream::try_stream;
use futures::{stream::BoxStream, Stream, StreamExt};
use json_patch::{patch, Patch};
use serde_json::Value;
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub struct AppliedEvent {
    pub event: Event,
    pub messages: Vec<Message>,
    pub state: State,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ApplyState {
    pub messages: Vec<Message>,
    pub state: State,
}

pub fn default_apply_events<S>(
    stream: S,
    initial_messages: Vec<Message>,
    initial_state: State,
) -> BoxStream<'static, Result<AppliedEvent>>
where
    S: Stream<Item = Result<Event>> + Send + 'static,
{
    Box::pin(try_stream! {
        let mut stream = stream.boxed();
        let mut state = ApplyState {
            messages: initial_messages,
            state: initial_state,
        };

        while let Some(item) = stream.next().await {
            let event = item?;
            apply_event(&mut state, &event)?;
            yield AppliedEvent {
                event,
                messages: state.messages.clone(),
                state: state.state.clone(),
            };
        }
    })
}

pub fn apply_event(state: &mut ApplyState, event: &Event) -> Result<()> {
    match event {
        Event::TextMessageStart(event) => {
            if !state
                .messages
                .iter()
                .any(|message| message.id() == event.message_id)
            {
                state.messages.push(new_text_message(
                    &event.message_id,
                    event.role,
                    event.name.clone(),
                ));
            }
        }
        Event::TextMessageContent(event) => {
            let message = state
                .messages
                .iter_mut()
                .find(|message| message.id() == event.message_id)
                .ok_or_else(|| {
                    AgUiError::validation(format!("message '{}' not found", event.message_id))
                })?;
            append_message_content(message, &event.delta)?;
        }
        Event::TextMessageEnd(_) => {}
        Event::ToolCallStart(event) => apply_tool_call_start(&mut state.messages, event)?,
        Event::ToolCallArgs(event) => {
            let tool_call = find_tool_call_mut(&mut state.messages, &event.tool_call_id)?;
            tool_call.function.arguments.push_str(&event.delta);
        }
        Event::ToolCallEnd(_) => {}
        Event::ToolCallResult(event) => {
            state.messages.push(Message::Tool(ToolMessage {
                id: event.message_id.clone(),
                content: event.content.clone(),
                tool_call_id: event.tool_call_id.clone(),
                error: None,
                encrypted_value: None,
            }));
        }
        Event::MessagesSnapshot(event) => {
            apply_messages_snapshot(&mut state.messages, &event.messages)
        }
        Event::StateSnapshot(event) => {
            state.state = event.snapshot.clone();
        }
        Event::StateDelta(event) => apply_state_delta(event, &mut state.state)?,
        Event::ActivitySnapshot(event) => apply_activity_snapshot(&mut state.messages, event),
        Event::ActivityDelta(event) => apply_activity_delta(&mut state.messages, event)?,
        Event::ReasoningStart(_) | Event::ReasoningEnd(_) => {}
        Event::ReasoningMessageStart(event) => {
            if !state
                .messages
                .iter()
                .any(|message| message.id() == event.message_id)
            {
                state.messages.push(Message::Reasoning(ReasoningMessage {
                    id: event.message_id.clone(),
                    content: String::new(),
                    encrypted_value: None,
                }));
            }
        }
        Event::ReasoningMessageContent(event) => {
            let message = state
                .messages
                .iter_mut()
                .find(|message| message.id() == event.message_id)
                .ok_or_else(|| {
                    AgUiError::validation(format!(
                        "reasoning message '{}' not found",
                        event.message_id
                    ))
                })?;
            append_reasoning_content(message, &event.delta)?;
        }
        Event::ReasoningMessageEnd(_) => {}
        Event::ReasoningEncryptedValue(event) => apply_reasoning_encrypted_value(
            &mut state.messages,
            event.subtype,
            &event.entity_id,
            &event.encrypted_value,
        )?,
        Event::ThinkingStart(_)
        | Event::ThinkingEnd(_)
        | Event::ThinkingTextMessageStart(_)
        | Event::ThinkingTextMessageContent(_)
        | Event::ThinkingTextMessageEnd(_)
        | Event::TextMessageChunk(_)
        | Event::ToolCallChunk(_)
        | Event::ReasoningMessageChunk(_)
        | Event::Raw(_)
        | Event::Custom(_)
        | Event::RunStarted(_)
        | Event::RunFinished(_)
        | Event::RunError(_)
        | Event::StepStarted(_)
        | Event::StepFinished(_) => {}
    }

    Ok(())
}

fn new_text_message(message_id: &str, role: TextMessageRole, name: Option<String>) -> Message {
    match role {
        TextMessageRole::Developer => Message::Developer(DeveloperMessage {
            id: message_id.to_string(),
            content: String::new(),
            name,
            encrypted_value: None,
        }),
        TextMessageRole::System => Message::System(SystemMessage {
            id: message_id.to_string(),
            content: String::new(),
            name,
            encrypted_value: None,
        }),
        TextMessageRole::Assistant => Message::Assistant(AssistantMessage {
            id: message_id.to_string(),
            content: Some(String::new()),
            name,
            tool_calls: None,
            encrypted_value: None,
        }),
        TextMessageRole::User => Message::User(UserMessage {
            id: message_id.to_string(),
            content: UserMessageContent::Text(String::new()),
            name,
            encrypted_value: None,
        }),
    }
}

fn append_message_content(message: &mut Message, delta: &str) -> Result<()> {
    match message {
        Message::Developer(message) => message.content.push_str(delta),
        Message::System(message) => message.content.push_str(delta),
        Message::Assistant(message) => message
            .content
            .get_or_insert_with(String::new)
            .push_str(delta),
        Message::User(message) => match &mut message.content {
            UserMessageContent::Text(content) => content.push_str(delta),
            UserMessageContent::Parts(_) => {
                return Err(AgUiError::validation(
                    "cannot append text delta to multipart user message",
                ));
            }
        },
        other => {
            return Err(AgUiError::validation(format!(
                "cannot append text content to '{}' message",
                role_name(other)
            )));
        }
    }

    Ok(())
}

fn append_reasoning_content(message: &mut Message, delta: &str) -> Result<()> {
    match message {
        Message::Reasoning(message) => {
            message.content.push_str(delta);
            Ok(())
        }
        other => Err(AgUiError::validation(format!(
            "cannot append reasoning content to '{}' message",
            role_name(other)
        ))),
    }
}

fn apply_tool_call_start(messages: &mut Vec<Message>, event: &ToolCallStartEvent) -> Result<()> {
    let index = resolve_or_create_assistant_message(
        messages,
        event.parent_message_id.as_deref(),
        &event.tool_call_id,
    );
    let assistant = assistant_message_mut(&mut messages[index])?;
    let tool_calls = assistant.tool_calls.get_or_insert_with(Vec::new);

    if let Some(tool_call) = tool_calls
        .iter_mut()
        .find(|tool_call| tool_call.id == event.tool_call_id)
    {
        tool_call.function.name = event.tool_call_name.clone();
        return Ok(());
    }

    tool_calls.push(ToolCall {
        id: event.tool_call_id.clone(),
        kind: ToolCallKind::Function,
        function: FunctionCall {
            name: event.tool_call_name.clone(),
            arguments: String::new(),
        },
        encrypted_value: None,
    });

    Ok(())
}

fn apply_messages_snapshot(messages: &mut Vec<Message>, snapshot: &[Message]) {
    let mut snapshot_map: HashMap<String, Message> = snapshot
        .iter()
        .cloned()
        .map(|message| (message.id().to_string(), message))
        .collect();
    let mut merged = Vec::new();

    for message in messages.iter() {
        match message {
            Message::Activity(_) | Message::Reasoning(_) => merged.push(message.clone()),
            _ => {
                if let Some(snapshot_message) = snapshot_map.remove(message.id()) {
                    merged.push(snapshot_message);
                }
            }
        }
    }

    for snapshot_message in snapshot {
        if let Some(snapshot_message) = snapshot_map.remove(snapshot_message.id()) {
            merged.push(snapshot_message);
        }
    }

    *messages = merged;
}

fn apply_activity_snapshot(messages: &mut Vec<Message>, event: &ActivitySnapshotEvent) {
    let activity_message = Message::Activity(ActivityMessage {
        id: event.message_id.clone(),
        activity_type: event.activity_type.clone(),
        content: event.content.clone(),
    });

    if let Some(index) = messages
        .iter()
        .position(|message| message.id() == event.message_id)
    {
        match &messages[index] {
            Message::Activity(_) if event.replace => messages[index] = activity_message,
            Message::Activity(_) => {}
            _ if event.replace => messages[index] = activity_message,
            _ => {}
        }
        return;
    }

    messages.push(activity_message);
}

fn apply_activity_delta(messages: &mut [Message], event: &ActivityDeltaEvent) -> Result<()> {
    let Some(index) = messages
        .iter()
        .position(|message| message.id() == event.message_id)
    else {
        return Ok(());
    };

    let Message::Activity(existing) = &messages[index] else {
        return Ok(());
    };

    let mut content = Value::Object(existing.content.clone());
    apply_state_delta(
        &StateDeltaEvent {
            delta: event.patch.clone(),
            base: event.base.clone(),
        },
        &mut content,
    )?;

    let Value::Object(content) = content else {
        return Err(AgUiError::validation(format!(
            "activity '{}' content patch must produce an object",
            event.message_id
        )));
    };

    messages[index] = Message::Activity(ActivityMessage {
        id: event.message_id.clone(),
        activity_type: event.activity_type.clone(),
        content,
    });

    Ok(())
}

fn apply_reasoning_encrypted_value(
    messages: &mut [Message],
    subtype: ReasoningEncryptedValueSubtype,
    entity_id: &str,
    encrypted_value: &str,
) -> Result<()> {
    match subtype {
        ReasoningEncryptedValueSubtype::ToolCall => {
            if let Ok(tool_call) = find_tool_call_mut(messages, entity_id) {
                tool_call.encrypted_value = Some(encrypted_value.to_string());
            }
        }
        ReasoningEncryptedValueSubtype::Message => {
            if let Some(message) = messages
                .iter_mut()
                .find(|message| message.id() == entity_id)
            {
                set_message_encrypted_value(message, encrypted_value)?;
            }
        }
    }

    Ok(())
}

fn set_message_encrypted_value(message: &mut Message, encrypted_value: &str) -> Result<()> {
    let encrypted_value = Some(encrypted_value.to_string());

    match message {
        Message::Developer(message) => message.encrypted_value = encrypted_value,
        Message::System(message) => message.encrypted_value = encrypted_value,
        Message::Assistant(message) => message.encrypted_value = encrypted_value,
        Message::User(message) => message.encrypted_value = encrypted_value,
        Message::Tool(message) => message.encrypted_value = encrypted_value,
        Message::Reasoning(message) => message.encrypted_value = encrypted_value,
        Message::Activity(_) => {
            return Err(AgUiError::validation(
                "activity messages do not support encrypted values",
            ));
        }
    }

    Ok(())
}

fn resolve_or_create_assistant_message(
    messages: &mut Vec<Message>,
    parent_message_id: Option<&str>,
    tool_call_id: &str,
) -> usize {
    if let Some(parent_message_id) = parent_message_id {
        if let Some(index) = messages
            .iter()
            .position(|message| message.id() == parent_message_id)
        {
            if matches!(messages[index], Message::Assistant(_)) {
                return index;
            }

            messages.push(Message::Assistant(AssistantMessage {
                id: tool_call_id.to_string(),
                content: None,
                name: None,
                tool_calls: Some(Vec::new()),
                encrypted_value: None,
            }));
            return messages.len() - 1;
        }

        messages.push(Message::Assistant(AssistantMessage {
            id: parent_message_id.to_string(),
            content: None,
            name: None,
            tool_calls: Some(Vec::new()),
            encrypted_value: None,
        }));
        return messages.len() - 1;
    }

    if let Some(index) = messages
        .iter()
        .rposition(|message| matches!(message, Message::Assistant(_)))
    {
        return index;
    }

    messages.push(Message::Assistant(AssistantMessage {
        id: tool_call_id.to_string(),
        content: None,
        name: None,
        tool_calls: Some(Vec::new()),
        encrypted_value: None,
    }));
    messages.len() - 1
}

fn assistant_message_mut(message: &mut Message) -> Result<&mut AssistantMessage> {
    match message {
        Message::Assistant(message) => Ok(message),
        _ => Err(AgUiError::validation("expected assistant message")),
    }
}

fn find_tool_call_mut<'a>(
    messages: &'a mut [Message],
    tool_call_id: &str,
) -> Result<&'a mut ToolCall> {
    for message in messages {
        if let Message::Assistant(assistant) = message {
            if let Some(tool_call) = assistant.tool_calls.as_mut().and_then(|tool_calls| {
                tool_calls
                    .iter_mut()
                    .find(|tool_call| tool_call.id == tool_call_id)
            }) {
                return Ok(tool_call);
            }
        }
    }

    Err(AgUiError::validation(format!(
        "tool call '{}' not found",
        tool_call_id
    )))
}

fn apply_state_delta(event: &StateDeltaEvent, state: &mut Value) -> Result<()> {
    let patch_value = Value::Array(event.delta.clone());
    let patch_ops: Patch = serde_json::from_value(patch_value)?;
    patch(state, &patch_ops.0)
        .map_err(|error| AgUiError::validation(format!("failed to apply state delta: {error}")))
}

fn role_name(message: &Message) -> &'static str {
    match message {
        Message::Developer(_) => "developer",
        Message::System(_) => "system",
        Message::Assistant(_) => "assistant",
        Message::User(_) => "user",
        Message::Tool(_) => "tool",
        Message::Activity(_) => "activity",
        Message::Reasoning(_) => "reasoning",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::stream;
    use serde_json::json;

    async fn collect(events: Vec<Event>) -> Vec<AppliedEvent> {
        default_apply_events(
            stream::iter(events.into_iter().map(Ok)),
            Vec::new(),
            Value::Null,
        )
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .collect::<Result<Vec<_>>>()
        .expect("apply stream should succeed")
    }

    fn apply_all(events: Vec<Event>) -> ApplyState {
        let mut state = ApplyState::default();
        for event in events {
            apply_event(&mut state, &event).expect("event should apply");
        }
        state
    }

    #[tokio::test]
    async fn builds_assistant_message_from_text_events() {
        let items = collect(vec![
            Event::TextMessageStart(agui_rs_core::TextMessageStartEvent {
                message_id: "m1".into(),
                role: TextMessageRole::Assistant,
                name: None,
                base: agui_rs_core::BaseEventFields::default(),
            }),
            agui_rs_core::factory::text_message_content("m1", "hello"),
            agui_rs_core::factory::text_message_end("m1"),
        ])
        .await;

        let final_messages = &items.last().expect("final event").messages;
        match &final_messages[0] {
            Message::Assistant(message) => assert_eq!(message.content.as_deref(), Some("hello")),
            _ => panic!("expected assistant message"),
        }
    }

    mod reasoning_apply {
        use super::*;
        use agui_rs_core::{
            ReasoningEncryptedValueEvent, ReasoningMessageContentEvent, ReasoningMessageEndEvent,
            ReasoningMessageRole, ReasoningMessageStartEvent, ThinkingEndEvent, ThinkingStartEvent,
        };

        #[tokio::test]
        async fn creates_reasoning_message_on_start() {
            let items = collect(vec![Event::ReasoningMessageStart(
                ReasoningMessageStartEvent {
                    message_id: "r1".into(),
                    role: ReasoningMessageRole::Reasoning,
                    base: agui_rs_core::BaseEventFields::default(),
                },
            )])
            .await;

            assert!(matches!(items[0].messages[0], Message::Reasoning(_)));
        }

        #[tokio::test]
        async fn appends_reasoning_content_across_events() {
            let state = apply_all(vec![
                Event::ReasoningMessageStart(ReasoningMessageStartEvent {
                    message_id: "r1".into(),
                    role: ReasoningMessageRole::Reasoning,
                    base: agui_rs_core::BaseEventFields::default(),
                }),
                Event::ReasoningMessageContent(ReasoningMessageContentEvent {
                    message_id: "r1".into(),
                    delta: "pla".into(),
                    base: agui_rs_core::BaseEventFields::default(),
                }),
                Event::ReasoningMessageContent(ReasoningMessageContentEvent {
                    message_id: "r1".into(),
                    delta: "n".into(),
                    base: agui_rs_core::BaseEventFields::default(),
                }),
                Event::ReasoningMessageEnd(ReasoningMessageEndEvent {
                    message_id: "r1".into(),
                    base: agui_rs_core::BaseEventFields::default(),
                }),
            ]);

            match &state.messages[0] {
                Message::Reasoning(message) => assert_eq!(message.content, "plan"),
                _ => panic!("expected reasoning message"),
            }
        }

        #[tokio::test]
        async fn applies_reasoning_message_encrypted_value() {
            let state = apply_all(vec![
                Event::ReasoningMessageStart(ReasoningMessageStartEvent {
                    message_id: "r1".into(),
                    role: ReasoningMessageRole::Reasoning,
                    base: agui_rs_core::BaseEventFields::default(),
                }),
                Event::ReasoningEncryptedValue(ReasoningEncryptedValueEvent {
                    subtype: ReasoningEncryptedValueSubtype::Message,
                    entity_id: "r1".into(),
                    encrypted_value: "secret".into(),
                    base: agui_rs_core::BaseEventFields::default(),
                }),
            ]);

            match &state.messages[0] {
                Message::Reasoning(message) => {
                    assert_eq!(message.encrypted_value.as_deref(), Some("secret"))
                }
                _ => panic!("expected reasoning message"),
            }
        }

        #[tokio::test]
        async fn thinking_events_are_accepted_as_noops() {
            let state = apply_all(vec![
                Event::ThinkingStart(ThinkingStartEvent {
                    title: Some("legacy".into()),
                    base: agui_rs_core::BaseEventFields::default(),
                }),
                Event::ThinkingEnd(ThinkingEndEvent {
                    base: agui_rs_core::BaseEventFields::default(),
                }),
            ]);

            assert!(state.messages.is_empty());
            assert_eq!(state.state, Value::Null);
        }
    }

    mod activity_apply {
        use super::*;

        #[tokio::test]
        async fn creates_activity_message_from_snapshot() {
            let items = collect(vec![Event::ActivitySnapshot(ActivitySnapshotEvent {
                message_id: "a1".into(),
                activity_type: "plan".into(),
                content: serde_json::Map::from_iter([(String::from("step"), json!("search"))]),
                replace: true,
                base: agui_rs_core::BaseEventFields::default(),
            })])
            .await;

            match &items[0].messages[0] {
                Message::Activity(message) => assert_eq!(message.activity_type, "plan"),
                _ => panic!("expected activity message"),
            }
        }

        #[tokio::test]
        async fn updates_activity_message_from_delta() {
            let state = apply_all(vec![
                Event::ActivitySnapshot(ActivitySnapshotEvent {
                    message_id: "a1".into(),
                    activity_type: "plan".into(),
                    content: serde_json::Map::from_iter([(String::from("steps"), json!([]))]),
                    replace: true,
                    base: agui_rs_core::BaseEventFields::default(),
                }),
                Event::ActivityDelta(ActivityDeltaEvent {
                    message_id: "a1".into(),
                    activity_type: "execute".into(),
                    patch: vec![json!({"op": "add", "path": "/steps/0", "value": "search"})],
                    base: agui_rs_core::BaseEventFields::default(),
                }),
            ]);

            match &state.messages[0] {
                Message::Activity(message) => {
                    assert_eq!(message.activity_type, "execute");
                    assert_eq!(message.content.get("steps"), Some(&json!(["search"])));
                }
                _ => panic!("expected activity message"),
            }
        }

        #[tokio::test]
        async fn activity_snapshot_replace_false_preserves_existing_message() {
            let mut state = ApplyState {
                messages: vec![Message::Activity(ActivityMessage {
                    id: "a1".into(),
                    activity_type: "plan".into(),
                    content: serde_json::Map::from_iter([(String::from("step"), json!("keep"))]),
                })],
                state: Value::Null,
            };

            apply_event(
                &mut state,
                &Event::ActivitySnapshot(ActivitySnapshotEvent {
                    message_id: "a1".into(),
                    activity_type: "execute".into(),
                    content: serde_json::Map::from_iter([(String::from("step"), json!("drop"))]),
                    replace: false,
                    base: agui_rs_core::BaseEventFields::default(),
                }),
            )
            .expect("snapshot should apply");

            match &state.messages[0] {
                Message::Activity(message) => {
                    assert_eq!(message.activity_type, "plan");
                    assert_eq!(message.content.get("step"), Some(&json!("keep")));
                }
                _ => panic!("expected activity message"),
            }
        }

        #[tokio::test]
        async fn activity_delta_missing_message_is_noop() {
            let state = apply_all(vec![Event::ActivityDelta(ActivityDeltaEvent {
                message_id: "missing".into(),
                activity_type: "plan".into(),
                patch: vec![json!({"op": "add", "path": "/step", "value": "x"})],
                base: agui_rs_core::BaseEventFields::default(),
            })]);

            assert!(state.messages.is_empty());
        }
    }

    mod tool_call_apply {
        use super::*;
        use agui_rs_core::{ReasoningEncryptedValueEvent, ToolCallResultEvent, ToolResultRole};

        #[tokio::test]
        async fn creates_assistant_message_for_parentless_tool_call() {
            let state = apply_all(vec![Event::ToolCallStart(ToolCallStartEvent {
                tool_call_id: "tc1".into(),
                tool_call_name: "search".into(),
                parent_message_id: None,
                base: agui_rs_core::BaseEventFields::default(),
            })]);

            match &state.messages[0] {
                Message::Assistant(message) => {
                    assert_eq!(message.id, "tc1");
                    assert_eq!(message.tool_calls.as_ref().map(Vec::len), Some(1));
                }
                _ => panic!("expected assistant message"),
            }
        }

        #[tokio::test]
        async fn merges_tool_call_args_across_lifecycle() {
            let state = apply_all(vec![
                Event::ToolCallStart(ToolCallStartEvent {
                    tool_call_id: "tc1".into(),
                    tool_call_name: "search".into(),
                    parent_message_id: Some("m1".into()),
                    base: agui_rs_core::BaseEventFields::default(),
                }),
                agui_rs_core::factory::tool_call_args("tc1", "{\"q\":\"ru"),
                agui_rs_core::factory::tool_call_args("tc1", "st\"}"),
                agui_rs_core::factory::tool_call_end("tc1"),
            ]);

            match &state.messages[0] {
                Message::Assistant(message) => {
                    let tool_call = &message.tool_calls.as_ref().expect("tool calls")[0];
                    assert_eq!(tool_call.function.arguments, "{\"q\":\"rust\"}");
                }
                _ => panic!("expected assistant message"),
            }
        }

        #[tokio::test]
        async fn applies_reasoning_encrypted_value_to_tool_call() {
            let state = apply_all(vec![
                Event::ToolCallStart(ToolCallStartEvent {
                    tool_call_id: "tc1".into(),
                    tool_call_name: "search".into(),
                    parent_message_id: None,
                    base: agui_rs_core::BaseEventFields::default(),
                }),
                Event::ReasoningEncryptedValue(ReasoningEncryptedValueEvent {
                    subtype: ReasoningEncryptedValueSubtype::ToolCall,
                    entity_id: "tc1".into(),
                    encrypted_value: "cipher".into(),
                    base: agui_rs_core::BaseEventFields::default(),
                }),
            ]);

            match &state.messages[0] {
                Message::Assistant(message) => {
                    let tool_call = &message.tool_calls.as_ref().expect("tool calls")[0];
                    assert_eq!(tool_call.encrypted_value.as_deref(), Some("cipher"));
                }
                _ => panic!("expected assistant message"),
            }
        }

        #[tokio::test]
        async fn appends_tool_message_from_tool_call_result() {
            let items = collect(vec![Event::ToolCallResult(ToolCallResultEvent {
                message_id: "tool-msg-1".into(),
                tool_call_id: "tc1".into(),
                content: "done".into(),
                role: Some(ToolResultRole::Tool),
                base: agui_rs_core::BaseEventFields::default(),
            })])
            .await;

            match &items[0].messages[0] {
                Message::Tool(message) => {
                    assert_eq!(message.id, "tool-msg-1");
                    assert_eq!(message.tool_call_id, "tc1");
                }
                _ => panic!("expected tool message"),
            }
        }
    }

    mod state_patches {
        use super::*;
        use agui_rs_core::{MessagesSnapshotEvent, StateSnapshotEvent};

        #[tokio::test]
        async fn applies_state_snapshot() {
            let items = collect(vec![Event::StateSnapshot(StateSnapshotEvent {
                snapshot: json!({"count": 1}),
                base: agui_rs_core::BaseEventFields::default(),
            })])
            .await;

            assert_eq!(items[0].state, json!({"count": 1}));
        }

        #[tokio::test]
        async fn applies_state_delta_via_json_patch() {
            let state = apply_all(vec![
                Event::StateSnapshot(StateSnapshotEvent {
                    snapshot: json!({"count": 1}),
                    base: agui_rs_core::BaseEventFields::default(),
                }),
                agui_rs_core::factory::state_delta(vec![
                    json!({"op": "replace", "path": "/count", "value": 2}),
                ]),
            ]);

            assert_eq!(state.state, json!({"count": 2}));
        }

        #[tokio::test]
        async fn messages_snapshot_preserves_client_only_messages() {
            let mut state = ApplyState {
                messages: vec![
                    Message::Reasoning(ReasoningMessage {
                        id: "r1".into(),
                        content: "plan".into(),
                        encrypted_value: None,
                    }),
                    Message::Assistant(AssistantMessage {
                        id: "m1".into(),
                        content: Some("old".into()),
                        name: None,
                        tool_calls: None,
                        encrypted_value: None,
                    }),
                ],
                state: Value::Null,
            };

            apply_event(
                &mut state,
                &Event::MessagesSnapshot(MessagesSnapshotEvent {
                    messages: vec![Message::Assistant(AssistantMessage {
                        id: "m1".into(),
                        content: Some("new".into()),
                        name: None,
                        tool_calls: None,
                        encrypted_value: None,
                    })],
                    base: agui_rs_core::BaseEventFields::default(),
                }),
            )
            .expect("snapshot should apply");

            assert!(matches!(state.messages[0], Message::Reasoning(_)));
            match &state.messages[1] {
                Message::Assistant(message) => assert_eq!(message.content.as_deref(), Some("new")),
                _ => panic!("expected assistant message"),
            }
        }

        #[tokio::test]
        async fn apply_event_updates_both_messages_and_state() {
            let mut state = ApplyState::default();

            apply_event(
                &mut state,
                &Event::TextMessageStart(agui_rs_core::TextMessageStartEvent {
                    message_id: "m1".into(),
                    role: TextMessageRole::Assistant,
                    name: None,
                    base: agui_rs_core::BaseEventFields::default(),
                }),
            )
            .expect("start applies");
            apply_event(
                &mut state,
                &agui_rs_core::factory::text_message_content("m1", "hello"),
            )
            .expect("content applies");
            apply_event(
                &mut state,
                &Event::StateSnapshot(StateSnapshotEvent {
                    snapshot: json!({"status": "ok"}),
                    base: agui_rs_core::BaseEventFields::default(),
                }),
            )
            .expect("snapshot applies");

            assert_eq!(state.state, json!({"status": "ok"}));
            match &state.messages[0] {
                Message::Assistant(message) => {
                    assert_eq!(message.content.as_deref(), Some("hello"))
                }
                _ => panic!("expected assistant message"),
            }
        }
    }
}
