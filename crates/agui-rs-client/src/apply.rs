use agui_rs_core::merge_metadata;
use agui_rs_core::types::{
    ActivityMessage, AssistantMessage, DeveloperMessage, ReasoningMessage, SystemMessage,
    ToolMessage, UserMessage,
};
use agui_rs_core::{
    ActivityDeltaEvent, ActivitySnapshotEvent, AgUiError, AttributableFields, Event, FunctionCall,
    Message, MessagesSnapshotEvent, ReasoningEncryptedValueSubtype, Result, RunStartedEvent, State,
    StateDeltaEvent, TextMessageRole, ToolCall, ToolCallKind, ToolCallStartEvent,
    UserMessageContent,
};
use async_stream::try_stream;
use futures::{stream::BoxStream, Stream, StreamExt};
use json_patch::{patch, Patch};
use serde_json::Value;
use std::collections::{HashMap, HashSet};

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

/// Folds the event's metadata into the target's accumulator (`merge_metadata`
/// in `agui-rs-core`, mirroring upstream `metadata.ts:53-63`). Event-level
/// metadata typed as a non-object `Value` is ignored rather than replacing,
/// since the schema pins event metadata to an object.
fn merge_event_metadata(
    target: &mut Option<serde_json::Map<String, Value>>,
    event_metadata: Option<&Value>,
) {
    let Some(incoming @ Value::Object(_)) = event_metadata else {
        return;
    };
    *target = match merge_metadata(
        target
            .as_ref()
            .map(|map| Value::Object(map.clone()))
            .as_ref(),
        Some(incoming),
    ) {
        Some(Value::Object(merged)) => Some(merged),
        _ => None,
    };
}

fn message_metadata_mut(message: &mut Message) -> &mut Option<serde_json::Map<String, Value>> {
    match message {
        Message::Developer(message) => &mut message.metadata,
        Message::System(message) => &mut message.metadata,
        Message::Assistant(message) => &mut message.metadata,
        Message::User(message) => &mut message.metadata,
        Message::Tool(message) => &mut message.metadata,
        Message::Reasoning(message) => &mut message.metadata,
        Message::Activity(message) => &mut message.metadata,
    }
}

pub fn apply_event(state: &mut ApplyState, event: &Event) -> Result<()> {
    match event {
        Event::TextMessageStart(event) => {
            // Message ids are unique across the conversation, so an activity
            // message under this id means the producer reused it. Streaming text
            // into it would overwrite its structured content with a string — warn
            // and drop the event, along with its metadata, which describes a text
            // message that never exists (`apply/default.ts:236-246`).
            if let Some(existing) = state
                .messages
                .iter_mut()
                .find(|message| message.id() == event.message_id)
            {
                if matches!(existing, Message::Activity(_)) {
                    tracing::warn!(
                        message_id = %event.message_id,
                        "TEXT_MESSAGE_START: Message '{}' is an activity message — message ids must be unique across activity and text messages; dropping the event",
                        event.message_id
                    );
                    return Ok(());
                }
                merge_event_metadata(
                    message_metadata_mut(existing),
                    event.base.metadata.as_ref(),
                );
                return Ok(());
            }

            let message = {
                state.messages.push(new_text_message(
                    &event.message_id,
                    event.role,
                    event.name.clone(),
                    event.attributable.subagent_run_id.clone(),
                ));
                state.messages.last_mut().expect("just pushed")
            };
            merge_event_metadata(message_metadata_mut(message), event.base.metadata.as_ref());
        }
        Event::TextMessageContent(event) => {
            let Some(message) = state
                .messages
                .iter_mut()
                .find(|message| message.id() == event.message_id)
            else {
                // A delta without its START is a producer defect, not a reason
                // to fail the run (`apply/default.ts:284-286`).
                tracing::warn!(
                    message_id = %event.message_id,
                    "TEXT_MESSAGE_CONTENT: No message found with ID '{}'; dropping the delta",
                    event.message_id
                );
                return Ok(());
            };
            if matches!(message, Message::Activity(_)) {
                // Appending here would replace the activity's structured
                // content with a string (`apply/default.ts:287-295`).
                tracing::warn!(
                    message_id = %event.message_id,
                    "TEXT_MESSAGE_CONTENT: Message '{}' is an activity message — message ids must be unique across activity and text messages; dropping the delta",
                    event.message_id
                );
                return Ok(());
            }
            append_message_content(message, &event.delta)?;
            merge_event_metadata(
                message_metadata_mut(message),
                event.base.metadata.as_ref(),
            );
        }
        Event::TextMessageEnd(event) => {
            if let Some(message) = state
                .messages
                .iter_mut()
                .find(|message| message.id() == event.message_id)
            {
                if matches!(message, Message::Activity(_)) {
                    // The matching START was dropped for the same reason, so
                    // there is no text message to finish
                    // (`apply/default.ts:338-343`).
                    tracing::warn!(
                        message_id = %event.message_id,
                        "TEXT_MESSAGE_END: Message '{}' is an activity message — dropping the event",
                        event.message_id
                    );
                    return Ok(());
                }
                // The end is where late-known values — token usage, finish
                // reason — typically arrive (`apply/default.ts:362-365`).
                merge_event_metadata(message_metadata_mut(message), event.base.metadata.as_ref());
            } else {
                tracing::warn!(
                    message_id = %event.message_id,
                    "TEXT_MESSAGE_END: No message found with ID '{}'",
                    event.message_id
                );
            }
        }
        Event::ToolCallStart(event) => apply_tool_call_start(&mut state.messages, event)?,
        Event::ToolCallArgs(event) => {
            let Ok(tool_call) = find_tool_call_mut(&mut state.messages, &event.tool_call_id) else {
                // An args delta for a call the client never saw started is a
                // producer defect, not a reason to fail the run
                // (`apply/default.ts:492-495`).
                tracing::warn!(
                    tool_call_id = %event.tool_call_id,
                    "TOOL_CALL_ARGS: No tool call found with ID '{}'; dropping the delta",
                    event.tool_call_id
                );
                return Ok(());
            };
            tool_call.function.arguments.push_str(&event.delta);
            merge_event_metadata(&mut tool_call.metadata, event.base.metadata.as_ref());
        }
        Event::ToolCallEnd(event) => {
            // Merge before onNewToolCall — an end event carries the values only
            // known once the call is closed (`apply/default.ts:585-589`).
            if let Ok(tool_call) = find_tool_call_mut(&mut state.messages, &event.tool_call_id) {
                merge_event_metadata(&mut tool_call.metadata, event.base.metadata.as_ref());
            } else {
                tracing::warn!(
                    tool_call_id = %event.tool_call_id,
                    "TOOL_CALL_END: No tool call found with ID '{}'",
                    event.tool_call_id
                );
            }
        }
        Event::ToolCallResult(event) => {
            let mut tool_message = Message::Tool(ToolMessage {
                id: event.message_id.clone(),
                content: event.content.clone(),
                tool_call_id: event.tool_call_id.clone(),
                error: None,
                encrypted_value: None,
                subagent_run_id: event.attributable.subagent_run_id.clone(),
                metadata: None,
            });
            merge_event_metadata(
                message_metadata_mut(&mut tool_message),
                event.base.metadata.as_ref(),
            );

            // Place the tool result immediately after the assistant message
            // that issued the matching tool call, not at the end. A result
            // event can arrive after a trailing assistant text message (e.g.
            // a chat -> tool -> chat loop streams the follow-up text before
            // the result is recorded). Appending would leave the history as
            // assistant(tool_call) -> text -> tool, which violates the
            // provider contract that an assistant tool_call is immediately
            // followed by its tool result. Skip past any tool results already
            // recorded for the same assistant so parallel results keep their
            // order. Fall back to append when no owner is found.
            let owner_index = state.messages.iter().position(|message| {
                if let Message::Assistant(assistant) = message {
                    assistant.tool_calls.as_ref().is_some_and(|tool_calls| {
                        tool_calls.iter().any(|tc| tc.id == event.tool_call_id)
                    })
                } else {
                    false
                }
            });

            match owner_index {
                Some(index) => {
                    let mut insert_at = index + 1;
                    while insert_at < state.messages.len()
                        && matches!(state.messages[insert_at], Message::Tool(_))
                    {
                        insert_at += 1;
                    }
                    state.messages.insert(insert_at, tool_message);
                }
                None => state.messages.push(tool_message),
            }
        }
        Event::MessagesSnapshot(event) => {
            apply_messages_snapshot(&mut state.messages, event)
        }
        Event::StateSnapshot(event) => {
            state.state = event.snapshot.clone();
        }
        Event::StateDelta(event) => apply_state_delta(event, &mut state.state)?,
        Event::ActivitySnapshot(event) => apply_activity_snapshot(&mut state.messages, event),
        Event::ActivityDelta(event) => apply_activity_delta(&mut state.messages, event)?,
        Event::ReasoningStart(_) | Event::ReasoningEnd(_) => {}
        Event::ReasoningMessageStart(event) => {
            // An activity message under this id means the producer reused it;
            // streaming reasoning into it would overwrite its structured
            // content — warn and drop the event and its metadata
            // (`apply/default.ts:1247-1262`).
            if state
                .messages
                .iter()
                .any(|message| message.id() == event.message_id)
            {
                if let Some(existing) = state
                    .messages
                    .iter_mut()
                    .find(|message| message.id() == event.message_id)
                {
                    if matches!(existing, Message::Activity(_)) {
                        tracing::warn!(
                            message_id = %event.message_id,
                            "REASONING_MESSAGE_START: Message '{}' is an activity message — message ids must be unique across activity and reasoning messages; dropping the event",
                            event.message_id
                        );
                        return Ok(());
                    }
                    merge_event_metadata(
                        message_metadata_mut(existing),
                        event.base.metadata.as_ref(),
                    );
                    return Ok(());
                }
                unreachable!("found above");
            }

            let message = {
                state
                    .messages
                    .push(Message::Reasoning(ReasoningMessage {
                        id: event.message_id.clone(),
                        content: String::new(),
                        encrypted_value: None,
                        subagent_run_id: event.attributable.subagent_run_id.clone(),
                        metadata: None,
                    }));
                state.messages.last_mut().expect("just pushed")
            };
            merge_event_metadata(message_metadata_mut(message), event.base.metadata.as_ref());
        }
        Event::ReasoningMessageContent(event) => {
            let Some(message) = state
                .messages
                .iter_mut()
                .find(|message| message.id() == event.message_id)
            else {
                // A delta without its START is a producer defect, not a reason
                // to fail the run (`apply/default.ts:1286`).
                tracing::warn!(
                    message_id = %event.message_id,
                    "REASONING_MESSAGE_CONTENT: No message found with ID '{}'; dropping the delta",
                    event.message_id
                );
                return Ok(());
            };
            if matches!(message, Message::Activity(_)) {
                // Appending here would replace the activity's structured
                // content with a string (`apply/default.ts:1289-1298`).
                tracing::warn!(
                    message_id = %event.message_id,
                    "REASONING_MESSAGE_CONTENT: Message '{}' is an activity message — message ids must be unique across activity and reasoning messages; dropping the delta",
                    event.message_id
                );
                return Ok(());
            }
            append_reasoning_content(message, &event.delta)?;
            merge_event_metadata(
                message_metadata_mut(message),
                event.base.metadata.as_ref(),
            );
        }
        Event::ReasoningMessageEnd(event) => {
            // The end is where late-known values — usage, finish reason — can
            // arrive (`apply/default.ts:1361-1363`).
            if let Some(message) = state
                .messages
                .iter_mut()
                .find(|message| message.id() == event.message_id)
            {
                merge_event_metadata(message_metadata_mut(message), event.base.metadata.as_ref());
            } else {
                tracing::warn!(
                    message_id = %event.message_id,
                    "REASONING_MESSAGE_END: No message found with ID '{}'",
                    event.message_id
                );
            }
        }
        Event::ReasoningEncryptedValue(event) => apply_reasoning_encrypted_value(
            &mut state.messages,
            event.subtype,
            &event.entity_id,
            &event.encrypted_value,
        )?,
        Event::TextMessageChunk(_)
        | Event::ToolCallChunk(_)
        | Event::ReasoningMessageChunk(_)
        | Event::Raw(_)
        | Event::Custom(_) => {}
        Event::RunStarted(event) => apply_run_started(&mut state.messages, event),
        Event::RunFinished(_)
        | Event::RunError(_)
        | Event::StepStarted(_)
        | Event::StepFinished(_)
        // ponytail: subagent events carry no message/state projection.
        | Event::SubagentStarted(_)
        | Event::SubagentFinished(_)
        | Event::SubagentError(_) => {}
    }

    Ok(())
}

fn new_text_message(
    message_id: &str,
    role: TextMessageRole,
    name: Option<String>,
    subagent_run_id: Option<String>,
) -> Message {
    match role {
        TextMessageRole::Developer => Message::Developer(DeveloperMessage {
            id: message_id.to_string(),
            content: String::new(),
            name,
            encrypted_value: None,
            subagent_run_id,
            metadata: None,
        }),
        TextMessageRole::System => Message::System(SystemMessage {
            id: message_id.to_string(),
            content: String::new(),
            name,
            encrypted_value: None,
            subagent_run_id,
            metadata: None,
        }),
        TextMessageRole::Assistant => Message::Assistant(AssistantMessage {
            id: message_id.to_string(),
            content: Some(String::new()),
            name,
            tool_calls: None,
            encrypted_value: None,
            subagent_run_id,
            metadata: None,
        }),
        TextMessageRole::User => Message::User(UserMessage {
            id: message_id.to_string(),
            content: UserMessageContent::Text(String::new()),
            name,
            encrypted_value: None,
            subagent_run_id,
            metadata: None,
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

fn apply_run_started(messages: &mut Vec<Message>, event: &RunStartedEvent) {
    // A HITL re-sync carries the conversation back in `input.messages`; add
    // the ones the client has not seen yet, keyed by id
    // (`apply/default.ts:1060-1080`, run-started-input.test.ts).
    let Some(input) = event.input.as_ref() else {
        return;
    };
    for message in &input.messages {
        if !messages
            .iter()
            .any(|existing| existing.id() == message.id())
        {
            messages.push(message.clone());
        }
    }
}

fn apply_tool_call_start(messages: &mut Vec<Message>, event: &ToolCallStartEvent) -> Result<()> {
    // Applying a start must be idempotent: the same start can reach the
    // reducer twice — a tool call already carried in `messages` from an
    // earlier run and replayed after a HITL `respond()` re-sync, or one
    // stream delivered over two transports. Dedupe across ALL messages
    // BEFORE resolving the parent, so a replay can't append a second entry
    // or a stray empty assistant message (its `parentMessageId` may no
    // longer be in state). Leave `arguments` untouched — a start carries
    // none, and the copy already in state holds the streamed args.
    let mut existing = None;
    'search: for message in messages.iter_mut() {
        if let Message::Assistant(assistant) = message {
            if let Some(tool_calls) = assistant.tool_calls.as_mut() {
                for tool_call in tool_calls.iter_mut() {
                    if tool_call.id == event.tool_call_id {
                        existing = Some(tool_call);
                        break 'search;
                    }
                }
            }
        }
    }

    if let Some(tool_call) = existing {
        if tool_call.function.name != event.tool_call_name {
            tracing::warn!(
                tool_call_id = %event.tool_call_id,
                "TOOL_CALL_START: tool call already exists — updating name to '{}'",
                event.tool_call_name
            );
            tool_call.function.name = event.tool_call_name.clone();
        }
        merge_event_metadata(&mut tool_call.metadata, event.base.metadata.as_ref());
        return Ok(());
    }

    let index = resolve_or_create_assistant_message(
        messages,
        event.parent_message_id.as_deref(),
        &event.tool_call_id,
        event.attributable.subagent_run_id.clone(),
    );
    let assistant = assistant_message_mut(&mut messages[index])?;
    let mut new_tool_call = ToolCall {
        id: event.tool_call_id.clone(),
        kind: ToolCallKind::Function,
        function: FunctionCall {
            name: event.tool_call_name.clone(),
            arguments: String::new(),
        },
        encrypted_value: None,
        metadata: None,
    };
    merge_event_metadata(&mut new_tool_call.metadata, event.base.metadata.as_ref());
    assistant
        .tool_calls
        .get_or_insert_with(Vec::new)
        .push(new_tool_call);

    Ok(())
}

/// Package-owned metadata namespace carrying the authority declaration.
/// Mirrors `activity-history.ts:3` upstream.
const ACTIVITY_HISTORY_METADATA: &str = "@ag-ui/client";

/// Adds a projector scope to an incoming `MESSAGES_SNAPSHOT`, preserving full
/// authority (`activity-history.ts:32-53`).
///
/// History projectors must call this on the snapshot *before* replacing its
/// messages so inferred authority describes the original set. A declared
/// `null` scope stays `null`; a scope inferred from the original activity
/// contents becomes `null`; an explicit array unions the requested types into
/// it (deduplicated, in first-seen order).
pub fn with_authoritative_activity_types(
    event: &MessagesSnapshotEvent,
    activity_types: &[String],
) -> MessagesSnapshotEvent {
    let mut event = event.clone();

    let prior = event
        .base
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get(ACTIVITY_HISTORY_METADATA))
        .cloned();
    let prior = prior.as_ref().and_then(|prior| prior.as_object());

    let scope = authoritative_activity_types(&event);
    let owns_all = scope == Some(None)
        || (scope.is_none()
            && event
                .messages
                .iter()
                .any(|m| matches!(m, Message::Activity(_))));

    let mut scope_value = match prior {
        // Keep the other keys the producer left in the namespace.
        Some(prior) => prior.clone(),
        None => serde_json::Map::new(),
    };
    if owns_all {
        scope_value.insert("authoritativeActivityTypes".into(), Value::Null);
    } else {
        let existing_types: Vec<String> = match scope {
            Some(Some(types)) => types,
            _ => Vec::new(),
        };
        let mut unioned: Vec<Value> = existing_types
            .iter()
            .map(|ty| Value::String(ty.clone()))
            .collect();
        for ty in activity_types {
            if !unioned.contains(&Value::String(ty.clone())) {
                unioned.push(Value::String(ty.clone()));
            }
        }
        scope_value.insert("authoritativeActivityTypes".into(), Value::Array(unioned));
    }

    let mut metadata = match event.base.metadata.take() {
        Some(Value::Object(metadata)) => metadata,
        // An invalid non-object metadata carries nothing we would clobber; the
        // upstream spread into an object replaces it wholesale.
        _ => serde_json::Map::new(),
    };
    metadata.insert(ACTIVITY_HISTORY_METADATA.into(), Value::Object(scope_value));
    event.base.metadata = Some(Value::Object(metadata));

    event
}

/// The activity types this snapshot is authoritative for.
///
/// `Some(None)` — the producer declared `null` — owns every type. `Some(Some(..))`
/// owns exactly the listed types, including the empty list (which grants
/// omission-based deletion of nothing). `None` is a snapshot with no
/// declaration, which falls back to inferring authority from whether the
/// snapshot itself carries activity. An invalid declaration owns no types.
///
/// Mirrors `authoritativeActivityTypes` (`activity-history.ts:15-27`).
fn authoritative_activity_types(event: &MessagesSnapshotEvent) -> Option<Option<Vec<String>>> {
    let scope = event
        .base
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get(ACTIVITY_HISTORY_METADATA))?;

    let Some(scope) = scope.as_object() else {
        return Some(Some(Vec::new()));
    };
    let types = scope.get("authoritativeActivityTypes")?;
    if types.is_null() {
        return Some(None);
    }
    let owned = types
        .as_array()
        .filter(|types| types.iter().all(Value::is_string))
        .map(|types| {
            types
                .iter()
                .map(|ty| ty.as_str().unwrap_or_default().to_string())
                .collect()
        });
    Some(Some(owned.unwrap_or_default()))
}

fn apply_messages_snapshot(messages: &mut Vec<Message>, event: &MessagesSnapshotEvent) {
    let snapshot = &event.messages;
    // `activity` messages are only sometimes client-only. They never travel
    // back to the backend, so a backend that does not track them cannot put
    // them in the snapshot, and the local copies must be preserved. But a
    // backend that does track them re-delivers the whole set it owns. A
    // MESSAGES_SNAPSHOT is a snapshot of every message, so once it carries
    // any activity the backend is declaring the complete activity set, and
    // one it leaves out has been removed — preserving it would make the
    // local copy undeletable. So when the snapshot carries activity, treat
    // it as the source of truth and apply the normal replace semantics.
    //
    // A producer may narrow that claim by declaring the activity types it
    // owns: then only those types may be deleted by omission and every other
    // type is preserved, whatever the snapshot's contents look like.
    //
    // `reasoning` is only sometimes client-only: most backends never include
    // reasoning in the snapshot, so dropping local reasoning would lose it.
    // But a backend that round-trips reasoning (e.g. LangGraph re-deriving
    // it from checkpointed content blocks) re-delivers it under its own
    // canonical id — message ids are generally NOT stable between streamed
    // events and the snapshot. When the snapshot carries reasoning, treat it
    // as the source of truth and apply normal replace semantics so the same
    // reasoning isn't rendered twice.
    let owned_activity_types = authoritative_activity_types(event);
    let snapshot_has_activity = snapshot.iter().any(|m| matches!(m, Message::Activity(_)));
    let snapshot_has_reasoning = snapshot.iter().any(|m| matches!(m, Message::Reasoning(_)));

    let snapshot_map: HashMap<String, Message> = snapshot
        .iter()
        .cloned()
        .map(|message| (message.id().to_string(), message))
        .collect();

    let is_preserved_client_only = |message: &Message| match message {
        Message::Activity(activity) => match &owned_activity_types {
            Some(Some(owned)) => !owned.iter().any(|ty| ty == &activity.activity_type),
            Some(None) => false,
            None => !snapshot_has_activity,
        },
        Message::Reasoning(_) => !snapshot_has_reasoning,
        _ => false,
    };

    // Existing message positions are preserved and a matching id always takes
    // the snapshot's copy — authority only governs messages the snapshot
    // omits, so an id outside a declared activity type still updates in place.
    let mut merged: Vec<Message> = Vec::new();
    for message in messages.iter() {
        if let Some(snapshot_message) = snapshot_map.get(message.id()) {
            merged.push(snapshot_message.clone());
        } else if is_preserved_client_only(message) {
            merged.push(message.clone());
        }
    }

    // New ids are appended in snapshot order. ponytail: upstream once owned the
    // whole transcript order here (`fix(client): apply MESSAGES_SNAPSHOT in
    // snapshot order`), then reverted to preserving existing positions
    // (`f6295c50`) — HEAD is the code above, and its README says so outright.
    let existing_ids: HashSet<String> = merged
        .iter()
        .map(|message| message.id().to_string())
        .collect();
    for snapshot_message in snapshot {
        if !existing_ids.contains(snapshot_message.id()) {
            merged.push(snapshot_message.clone());
        }
    }

    *messages = merged;
}

fn apply_activity_snapshot(messages: &mut Vec<Message>, event: &ActivitySnapshotEvent) {
    // Absent `replace` means replace: the snapshot is the authoritative
    // content for the activity it names.
    let replace = event.replace.unwrap_or(true);

    let mut activity_message = ActivityMessage {
        id: event.message_id.clone(),
        activity_type: event.activity_type.clone(),
        content: event.content.clone(),
        subagent_run_id: event.attributable.subagent_run_id.clone(),
        metadata: None,
    };

    if let Some(index) = messages
        .iter()
        .position(|message| message.id() == event.message_id)
    {
        let is_activity = matches!(messages[index], Message::Activity(_));
        if is_activity && replace {
            // The upstream replace spreads the existing message
            // (`apply/default.ts:882-884`): *"Spread carries the accumulated
            // metadata across the replace — a snapshot replaces content, not
            // the metadata built up so far."* Attribution is the exception
            // upstream re-mints and we re-take it above.
            activity_message.metadata = message_metadata_mut(&mut messages[index]).clone();
        }
        // `mergeTarget` resolves outside the `replace` branch
        // (`apply/default.ts:878-905`): a non-replace snapshot merges its
        // metadata into the existing message whose content stands. A
        // non-activity slot under this id only becomes the snapshot's activity
        // when `replace` — otherwise the merge target stays undefined and the
        // event is dropped entirely (`apply/default.ts:890-893`).
        if is_activity {
            if !replace {
                merge_event_metadata(
                    message_metadata_mut(&mut messages[index]),
                    event.base.metadata.as_ref(),
                );
                return;
            }
            merge_event_metadata(&mut activity_message.metadata, event.base.metadata.as_ref());
            messages[index] = Message::Activity(activity_message);
        } else if replace {
            merge_event_metadata(&mut activity_message.metadata, event.base.metadata.as_ref());
            messages[index] = Message::Activity(activity_message);
        }
        return;
    }

    merge_event_metadata(&mut activity_message.metadata, event.base.metadata.as_ref());
    messages.push(Message::Activity(activity_message));
}

fn apply_activity_delta(messages: &mut [Message], event: &ActivityDeltaEvent) -> Result<()> {
    let Some(index) = messages
        .iter()
        .position(|message| message.id() == event.message_id)
    else {
        return Ok(());
    };

    // Metadata does not depend on the patch succeeding — a stale path should
    // not cost the message its usage or trace keys — so merge it into the
    // message before attempting the patch and leave it there either way
    // (`apply/default.ts:962-966`).
    let Some(Message::Activity(existing)) = messages.get_mut(index) else {
        return Ok(());
    };
    merge_event_metadata(&mut existing.metadata, event.base.metadata.as_ref());

    // RFC 6902 against the activity's content, same `json_patch::patch` the
    // state reducer uses. A failed patch is announced and the prior content
    // kept: a stale path is a producer's defect, not a reason to fail the run
    // (`apply/default.ts:963-983`). The activity type still moves — it rides
    // the event, not the patch result.
    let mut content = Value::Object(existing.content.clone());
    match apply_state_delta(
        &StateDeltaEvent {
            delta: event.patch.clone(),
            base: event.base.clone(),
            attributable: AttributableFields::default(),
        },
        &mut content,
    ) {
        Ok(()) => {}
        Err(err) => {
            tracing::warn!(
                message_id = %event.message_id,
                error = %err,
                "Failed to apply activity patch"
            );
            return Ok(());
        }
    }

    let Value::Object(content) = content else {
        tracing::warn!(
            message_id = %event.message_id,
            "Failed to apply activity patch: the patched content is not an object"
        );
        return Ok(());
    };

    // The upstream rebuild spreads the message it read
    // (`apply/default.ts:979-983`: `{...existingActivityMessage, content,
    // activityType}`), so the metadata built up so far rides along. Upstream
    // merges `event.metadata` in first — that half is the unimplemented
    // `mergeMetadata` gap, not this one.
    messages[index] = Message::Activity(ActivityMessage {
        id: event.message_id.clone(),
        activity_type: event.activity_type.clone(),
        content,
        subagent_run_id: existing.subagent_run_id.clone(),
        metadata: existing.metadata.clone(),
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
                // Activity messages do not have encryptedValue; the event is
                // ignored, the run goes on (`apply/default.ts:1443-1447`).
                if !set_message_encrypted_value(message, encrypted_value) {
                    tracing::warn!(
                        entity_id = %entity_id,
                        "REASONING_ENCRYPTED_VALUE: activity messages do not support encrypted values"
                    );
                }
            }
        }
    }

    Ok(())
}

/// Sets the encrypted value, reporting whether the message kind supports one.
/// An activity message does not (`apply/default.ts:1443-1447`); the caller
/// announces that instead of failing the run.
fn set_message_encrypted_value(message: &mut Message, encrypted_value: &str) -> bool {
    let encrypted_value = Some(encrypted_value.to_string());

    match message {
        Message::Developer(message) => message.encrypted_value = encrypted_value,
        Message::System(message) => message.encrypted_value = encrypted_value,
        Message::Assistant(message) => message.encrypted_value = encrypted_value,
        Message::User(message) => message.encrypted_value = encrypted_value,
        Message::Tool(message) => message.encrypted_value = encrypted_value,
        Message::Reasoning(message) => message.encrypted_value = encrypted_value,
        Message::Activity(_) => return false,
    }

    true
}

fn resolve_or_create_assistant_message(
    messages: &mut Vec<Message>,
    parent_message_id: Option<&str>,
    tool_call_id: &str,
    subagent_run_id: Option<String>,
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
                subagent_run_id,
                metadata: None,
            }));
            return messages.len() - 1;
        }

        messages.push(Message::Assistant(AssistantMessage {
            id: parent_message_id.to_string(),
            content: None,
            name: None,
            tool_calls: Some(Vec::new()),
            encrypted_value: None,
            subagent_run_id,
            metadata: None,
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
        subagent_run_id,
        metadata: None,
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
                attributable: AttributableFields::default(),
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
            ReasoningMessageRole, ReasoningMessageStartEvent,
        };

        #[tokio::test]
        async fn creates_reasoning_message_on_start() {
            let items = collect(vec![Event::ReasoningMessageStart(
                ReasoningMessageStartEvent {
                    message_id: "r1".into(),
                    role: ReasoningMessageRole::Reasoning,
                    attributable: AttributableFields::default(),
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
                    attributable: AttributableFields::default(),
                    base: agui_rs_core::BaseEventFields::default(),
                }),
                Event::ReasoningMessageContent(ReasoningMessageContentEvent {
                    message_id: "r1".into(),
                    delta: "pla".into(),
                    base: agui_rs_core::BaseEventFields::default(),
                    attributable: AttributableFields::default(),
                }),
                Event::ReasoningMessageContent(ReasoningMessageContentEvent {
                    message_id: "r1".into(),
                    delta: "n".into(),
                    base: agui_rs_core::BaseEventFields::default(),
                    attributable: AttributableFields::default(),
                }),
                Event::ReasoningMessageEnd(ReasoningMessageEndEvent {
                    message_id: "r1".into(),
                    base: agui_rs_core::BaseEventFields::default(),
                    attributable: AttributableFields::default(),
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
                    attributable: AttributableFields::default(),
                    base: agui_rs_core::BaseEventFields::default(),
                }),
                Event::ReasoningEncryptedValue(ReasoningEncryptedValueEvent {
                    subtype: ReasoningEncryptedValueSubtype::Message,
                    entity_id: "r1".into(),
                    encrypted_value: "secret".into(),
                    attributable: AttributableFields::default(),
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
    }

    mod activity_apply {
        use super::*;

        #[tokio::test]
        async fn creates_activity_message_from_snapshot() {
            let items = collect(vec![Event::ActivitySnapshot(ActivitySnapshotEvent {
                message_id: "a1".into(),
                activity_type: "plan".into(),
                content: serde_json::Map::from_iter([(String::from("step"), json!("search"))]),
                attributable: AttributableFields::default(),
                replace: Some(true),
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
                    attributable: AttributableFields::default(),
                    replace: Some(true),
                    base: agui_rs_core::BaseEventFields::default(),
                }),
                Event::ActivityDelta(ActivityDeltaEvent {
                    message_id: "a1".into(),
                    activity_type: "execute".into(),
                    patch: vec![json!({"op": "add", "path": "/steps/0", "value": "search"})],
                    base: agui_rs_core::BaseEventFields::default(),
                    attributable: AttributableFields::default(),
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
                    metadata: None,
                    subagent_run_id: None,
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
                    attributable: AttributableFields::default(),
                    replace: Some(false),
                    base: agui_rs_core::BaseEventFields {
                        // The merge target exists even without `replace`
                        // (`apply/default.ts:878-905`), so the event's
                        // metadata lands on the message whose content stands.
                        metadata: Some(json!({"usage": {"tokens": 4}})),
                        ..Default::default()
                    },
                }),
            )
            .expect("snapshot should apply");

            match &state.messages[0] {
                Message::Activity(message) => {
                    assert_eq!(message.activity_type, "plan");
                    assert_eq!(message.content.get("step"), Some(&json!("keep")));
                    assert_eq!(
                        message.metadata.as_ref().map(|m| Value::Object(m.clone())),
                        Some(json!({"usage": {"tokens": 4}})),
                        "the non-replace snapshot's metadata lands on the existing message"
                    );
                }
                _ => panic!("expected activity message"),
            }
        }

        /// A snapshot replaces content, not the metadata built up so far. The
        /// accumulation here is a MESSAGES_SNAPSHOT-replayed message, which is
        /// how any metadata an activity can carry arrives.
        #[tokio::test]
        async fn activity_snapshot_replace_keeps_the_accumulated_metadata() {
            let metadata = json!({"@ag-ui/client": {"authoritativeActivityTypes": ["plan"]}})
                .as_object()
                .unwrap()
                .clone();
            let mut state = ApplyState {
                messages: vec![Message::Activity(ActivityMessage {
                    id: "a1".into(),
                    metadata: Some(metadata.clone()),
                    subagent_run_id: Some("sub-1".into()),
                    activity_type: "plan".into(),
                    content: serde_json::Map::from_iter([(String::from("step"), json!("old"))]),
                })],
                state: Value::Null,
            };

            // Absent `replace` means replace.
            apply_event(
                &mut state,
                &Event::ActivitySnapshot(ActivitySnapshotEvent {
                    message_id: "a1".into(),
                    activity_type: "execute".into(),
                    content: serde_json::Map::from_iter([(String::from("step"), json!("new"))]),
                    attributable: AttributableFields::default(),
                    replace: None,
                    base: agui_rs_core::BaseEventFields::default(),
                }),
            )
            .expect("snapshot should apply");

            match &state.messages[0] {
                Message::Activity(message) => {
                    assert_eq!(message.metadata.as_ref(), Some(&metadata));
                    assert_eq!(message.content.get("step"), Some(&json!("new")));
                    // A replace re-mints the activity, so it brings its own
                    // attribution (upstream: `apply/default.ts:886-891`).
                    assert_eq!(message.subagent_run_id, None);
                }
                other => panic!("expected activity message, got {other:?}"),
            }
        }

        /// Upstream's delta rebuild is a spread of the message it read
        /// (`apply/default.ts:979-983`); rebuilding one from scratch dropped
        /// everything but the patched content.
        #[tokio::test]
        async fn activity_delta_keeps_the_accumulated_metadata() {
            let metadata = json!({"usage": {"tokens": 12}})
                .as_object()
                .unwrap()
                .clone();
            let mut state = ApplyState {
                messages: vec![Message::Activity(ActivityMessage {
                    id: "a1".into(),
                    metadata: Some(metadata.clone()),
                    subagent_run_id: None,
                    activity_type: "plan".into(),
                    content: serde_json::Map::from_iter([(String::from("step"), json!("one"))]),
                })],
                state: Value::Null,
            };

            apply_event(
                &mut state,
                &Event::ActivityDelta(ActivityDeltaEvent {
                    message_id: "a1".into(),
                    activity_type: "execute".into(),
                    patch: vec![json!({"op": "add", "path": "/step2", "value": "two"})],
                    base: agui_rs_core::BaseEventFields::default(),
                    attributable: AttributableFields::default(),
                }),
            )
            .expect("delta should apply");

            match &state.messages[0] {
                Message::Activity(message) => {
                    assert_eq!(message.metadata.as_ref(), Some(&metadata));
                    assert_eq!(message.activity_type, "execute");
                    assert_eq!(message.content.get("step"), Some(&json!("one")));
                    assert_eq!(message.content.get("step2"), Some(&json!("two")));
                }
                other => panic!("expected activity message, got {other:?}"),
            }
        }

        #[tokio::test]
        async fn activity_delta_missing_message_is_noop() {
            let state = apply_all(vec![Event::ActivityDelta(ActivityDeltaEvent {
                message_id: "missing".into(),
                activity_type: "plan".into(),
                patch: vec![json!({"op": "add", "path": "/step", "value": "x"})],
                base: agui_rs_core::BaseEventFields::default(),
                attributable: AttributableFields::default(),
            })]);

            assert!(state.messages.is_empty());
        }
        #[tokio::test]
        async fn activity_delta_applies_an_rfc6902_patch() {
            let state = apply_all(vec![
                Event::ActivitySnapshot(ActivitySnapshotEvent {
                    message_id: "a1".into(),
                    activity_type: "plan".into(),
                    content: serde_json::Map::new(),
                    replace: Some(true),
                    base: agui_rs_core::BaseEventFields::default(),
                    attributable: AttributableFields::default(),
                }),
                Event::ActivityDelta(ActivityDeltaEvent {
                    message_id: "a1".into(),
                    activity_type: "plan".into(),
                    patch: vec![json!({"op": "add", "path": "/step", "value": "one"})],
                    base: agui_rs_core::BaseEventFields::default(),
                    attributable: AttributableFields::default(),
                }),
            ]);

            match &state.messages[0] {
                Message::Activity(message) => {
                    assert_eq!(message.content.get("step"), Some(&json!("one")));
                }
                other => panic!("expected activity message, got {other:?}"),
            }
        }

        #[tokio::test]
        async fn activity_delta_reports_a_stale_patch_without_failing_the_run() {
            let state = apply_all(vec![
                Event::ActivitySnapshot(ActivitySnapshotEvent {
                    message_id: "a1".into(),
                    activity_type: "plan".into(),
                    content: serde_json::Map::new(),
                    replace: Some(true),
                    base: agui_rs_core::BaseEventFields::default(),
                    attributable: AttributableFields::default(),
                }),
                Event::ActivityDelta(ActivityDeltaEvent {
                    message_id: "a1".into(),
                    activity_type: "plan".into(),
                    patch: vec![json!({"op": "replace", "path": "/nope", "value": "x"})],
                    base: agui_rs_core::BaseEventFields::default(),
                    attributable: AttributableFields::default(),
                }),
            ]);

            // The prior content stands; a producer's stale path is not a reason
            // to fail the run.
            match &state.messages[0] {
                Message::Activity(message) => {
                    assert!(message.content.is_empty(), "got {:?}", message.content);
                }
                other => panic!("expected activity message, got {other:?}"),
            }
        }

        /// A producer may declare the activity types it owns. Omitted activity
        /// of an undeclared type is then preserved, while a declared type the
        /// snapshot leaves out is deleted.
        #[test]
        fn messages_snapshot_honours_declared_activity_authority() {
            let activity = |id: &str, activity_type: &str| {
                Message::Activity(ActivityMessage {
                    id: id.into(),
                    activity_type: activity_type.into(),
                    content: serde_json::Map::new(),
                    subagent_run_id: None,
                    metadata: None,
                })
            };
            let owned = |types: Value| {
                Event::MessagesSnapshot(MessagesSnapshotEvent {
                    messages: vec![activity("a1", "plan")],
                    base: agui_rs_core::BaseEventFields {
                        metadata: Some(json!({
                            "@ag-ui/client": {"authoritativeActivityTypes": types}
                        })),
                        ..Default::default()
                    },
                })
            };
            fn ids(state: &ApplyState) -> Vec<String> {
                state.messages.iter().map(|m| m.id().to_string()).collect()
            }

            // `a1` matches by id, so it updates in place; `a2` is omitted and of
            // an UNDECLARED type, so it is preserved.
            let mut state = ApplyState {
                messages: vec![activity("a1", "plan"), activity("a2", "chat")],
                state: Value::Null,
            };
            apply_event(&mut state, &owned(json!(["plan"]))).unwrap();
            assert_eq!(
                ids(&state),
                vec!["a1", "a2"],
                "the undeclared type is preserved"
            );

            // A DECLARED type the snapshot omits is deleted.
            let mut state = ApplyState {
                messages: vec![activity("a1", "plan"), activity("a2", "chat")],
                state: Value::Null,
            };
            apply_event(
                &mut state,
                &Event::MessagesSnapshot(MessagesSnapshotEvent {
                    messages: vec![],
                    base: agui_rs_core::BaseEventFields {
                        metadata: Some(json!({
                            "@ag-ui/client": {"authoritativeActivityTypes": ["plan"]}
                        })),
                        ..Default::default()
                    },
                }),
            )
            .unwrap();
            assert_eq!(ids(&state), vec!["a2"], "the declared type is deletable");

            // An empty declaration owns nothing, so omission deletes nothing.
            let mut state = ApplyState {
                messages: vec![activity("a1", "plan"), activity("a2", "chat")],
                state: Value::Null,
            };
            apply_event(
                &mut state,
                &Event::MessagesSnapshot(MessagesSnapshotEvent {
                    messages: vec![],
                    base: agui_rs_core::BaseEventFields {
                        metadata: Some(
                            json!({"@ag-ui/client": {"authoritativeActivityTypes": []}}),
                        ),
                        ..Default::default()
                    },
                }),
            )
            .unwrap();
            assert_eq!(ids(&state), vec!["a1", "a2"]);

            // `null` means every type, so the transcript-only snapshot deletes
            // the activity it leaves out.
            let mut state = ApplyState {
                messages: vec![activity("a1", "plan"), activity("a2", "chat")],
                state: Value::Null,
            };
            apply_event(
                &mut state,
                &Event::MessagesSnapshot(MessagesSnapshotEvent {
                    messages: vec![],
                    base: agui_rs_core::BaseEventFields {
                        metadata: Some(
                            json!({"@ag-ui/client": {"authoritativeActivityTypes": null}}),
                        ),
                        ..Default::default()
                    },
                }),
            )
            .unwrap();
            assert!(ids(&state).is_empty());

            // No declaration at all falls back to inferring from the contents.
            let mut state = ApplyState {
                messages: vec![activity("a1", "plan")],
                state: Value::Null,
            };
            apply_event(
                &mut state,
                &Event::MessagesSnapshot(MessagesSnapshotEvent {
                    messages: vec![],
                    base: agui_rs_core::BaseEventFields::default(),
                }),
            )
            .unwrap();
            assert_eq!(
                ids(&state),
                vec!["a1"],
                "an undeclared, activity-free snapshot preserves"
            );
        }

        mod with_authoritative_activity_types_tests {
            use super::*;

            fn snapshot_event(messages: Vec<Message>) -> MessagesSnapshotEvent {
                MessagesSnapshotEvent {
                    messages,
                    base: agui_rs_core::BaseEventFields::default(),
                }
            }

            fn activity(id: &str, activity_type: &str) -> Message {
                Message::Activity(ActivityMessage {
                    id: id.into(),
                    activity_type: activity_type.into(),
                    content: serde_json::Map::new(),
                    subagent_run_id: None,
                    metadata: None,
                })
            }

            fn declaration(event: &MessagesSnapshotEvent) -> Value {
                event.base.metadata.as_ref().expect("metadata set")[ACTIVITY_HISTORY_METADATA]
                    .clone()
            }

            /// The upstream round-trip: what `with_..` writes,
            /// `authoritative_activity_types` reads back. The unmarked
            /// transcript carries no activity, so no inferred full authority
            /// fires and the projector's list is the declaration
            /// (activity-history.test.ts:46 + :97-108).
            #[test]
            fn writes_a_scope_the_reader_round_trips() {
                let event = snapshot_event(vec![]);
                let extended =
                    with_authoritative_activity_types(&event, &["projected".to_string()]);
                assert_eq!(
                    authoritative_activity_types(&extended),
                    Some(Some(vec!["projected".to_string()]))
                );
            }

            #[test]
            fn unmarked_transcript_gets_only_the_projector_scope() {
                // activity-history.test.ts:97-108
                let event = snapshot_event(vec![]);
                let extended =
                    with_authoritative_activity_types(&event, &["projected".to_string()]);
                assert_eq!(
                    declaration(&extended),
                    json!({"authoritativeActivityTypes": ["projected"]})
                );
            }

            /// An unmarked snapshot carrying activity owns everything; the
            /// projector preserves that full authority (activity-history.test.ts:91-95).
            #[test]
            fn inferred_full_authority_is_preserved_as_null() {
                let event = snapshot_event(vec![activity("foreign", "foreign")]);
                let extended =
                    with_authoritative_activity_types(&event, &["projected".to_string()]);
                assert_eq!(
                    declaration(&extended),
                    json!({"authoritativeActivityTypes": null})
                );
                assert_eq!(authoritative_activity_types(&extended), Some(None));
            }

            #[test]
            fn explicit_null_stays_null_regardless_of_contents() {
                // activity-history.test.ts:78-89
                let mut event = snapshot_event(vec![]);
                event.base.metadata = Some(json!({
                    "@ag-ui/client": {"authoritativeActivityTypes": null}
                }));
                let extended =
                    with_authoritative_activity_types(&event, &["projected".to_string()]);
                assert_eq!(authoritative_activity_types(&extended), Some(None));
            }

            #[test]
            fn unions_scopes_deduplicated_in_first_seen_order() {
                // activity-history.test.ts:51-76
                let mut event = snapshot_event(vec![activity("foreign", "foreign")]);
                event.base.metadata = Some(json!({
                    "@ag-ui/client": {
                        "other": "keep",
                        "authoritativeActivityTypes": ["first"]
                    },
                    "trace": {"id": "keep"}
                }));
                let requested = vec!["second".to_string(), "first".to_string()];
                let extended = with_authoritative_activity_types(&event, &requested);
                assert_eq!(
                    declaration(&extended),
                    json!({
                        "other": "keep",
                        "authoritativeActivityTypes": ["first", "second"]
                    })
                );
                // Idempotent: extending again changes nothing.
                let again = with_authoritative_activity_types(&extended, &requested);
                assert_eq!(again, extended);
                // The input is not mutated.
                assert_eq!(
                    event.base.metadata.as_ref().unwrap()["@ag-ui/client"]
                        ["authoritativeActivityTypes"],
                    json!(["first"])
                );
            }

            /// A present-but-invalid declaration owns nothing, so the projector's
            /// types land on top of an empty scope (activity-history.test.ts:41-49).
            #[test]
            fn invalid_declaration_grants_nothing_but_extends_fine() {
                let mut event = snapshot_event(vec![activity("foreign", "foreign")]);
                event.base.metadata = Some(json!({
                    "@ag-ui/client": {"authoritativeActivityTypes": "owned"}
                }));
                let extended =
                    with_authoritative_activity_types(&event, &["projected".to_string()]);
                assert_eq!(
                    authoritative_activity_types(&extended),
                    Some(Some(vec!["projected".to_string()]))
                );
            }

            /// Extending an explicit empty scope works even though the snapshot
            /// carries activity (activity-history.test.ts:110-119).
            #[test]
            fn extends_an_explicit_empty_scope_despite_activity() {
                let mut event = snapshot_event(vec![activity("foreign", "foreign")]);
                event.base.metadata = Some(json!({
                    "@ag-ui/client": {"authoritativeActivityTypes": []}
                }));
                let extended =
                    with_authoritative_activity_types(&event, &["projected".to_string()]);
                assert_eq!(
                    authoritative_activity_types(&extended),
                    Some(Some(vec!["projected".to_string()]))
                );
            }

            /// End-to-end: the projector marks the types it reconstructed on an
            /// unmarked, activity-carrying snapshot — wait, an unmarked one
            /// carrying activity owns everything already; the useful e2e is the
            /// explicit-scope one. Mark the types, then a later transcript-only
            /// snapshot deletes the declared type but spares the undeclared one.
            #[test]
            fn marked_snapshot_deletes_declared_types_and_spares_undeclared() {
                let mut event = snapshot_event(vec![]);
                event.base.metadata = Some(json!({
                    "@ag-ui/client": {"authoritativeActivityTypes": []}
                }));
                let projected =
                    with_authoritative_activity_types(&event, &["projected".to_string()]);

                let mut state = ApplyState {
                    messages: vec![activity("a-own", "projected"), activity("a-keep", "chat")],
                    state: Value::Null,
                };
                apply_event(
                    &mut state,
                    &Event::MessagesSnapshot(MessagesSnapshotEvent {
                        messages: vec![],
                        base: projected.base.clone(),
                    }),
                )
                .unwrap();
                let ids: Vec<&str> = state.messages.iter().map(|m| m.id()).collect();
                assert_eq!(ids, vec!["a-keep"]);
            }
        }

        /// Existing positions are preserved; ids the client has not seen are
        /// appended in snapshot order.
        #[test]
        fn messages_snapshot_updates_in_place_and_appends_new_ids() {
            let assistant = |id: &str, content: &str| {
                Message::Assistant(AssistantMessage {
                    id: id.into(),
                    metadata: None,
                    subagent_run_id: None,
                    content: Some(content.into()),
                    name: None,
                    tool_calls: None,
                    encrypted_value: None,
                })
            };

            let mut state = ApplyState {
                messages: vec![assistant("m1", "one"), assistant("m2", "two")],
                state: Value::Null,
            };
            apply_event(
                &mut state,
                &Event::MessagesSnapshot(MessagesSnapshotEvent {
                    messages: vec![assistant("m1", "one!"), assistant("m3", "three")],
                    base: agui_rs_core::BaseEventFields::default(),
                }),
            )
            .unwrap();

            // `m2` is a transcript message the snapshot omits, so it is gone;
            // `m3` is new and lands after the survivors, in snapshot order.
            let ids: Vec<String> = state.messages.iter().map(|m| m.id().to_string()).collect();
            assert_eq!(ids, vec!["m1", "m3"]);
            assert_eq!(state.messages[0], assistant("m1", "one!"));
        }
    }

    mod metadata_merge {
        use super::*;
        use agui_rs_core::{
            ReasoningMessageStartEvent, StateSnapshotEvent, ToolCallResultEvent, ToolResultRole,
        };

        fn metadata_event(metadata: Value) -> agui_rs_core::BaseEventFields {
            agui_rs_core::BaseEventFields {
                metadata: Some(metadata),
                ..Default::default()
            }
        }

        #[test]
        fn merges_key_by_key_with_last_write_winning() {
            let mut target = Some(json!({"a": 1, "b": 2}).as_object().unwrap().clone());
            merge_event_metadata(&mut target, Some(&json!({"b": 3, "c": 4, "a": null})));
            assert_eq!(
                target.map(Value::Object),
                Some(json!({"a": null, "b": 3, "c": 4}))
            );
        }

        #[test]
        fn replaces_outright_without_recursing() {
            // metadata.test.ts:185: an array under a key is swapped, not blended.
            let mut target = Some(
                json!({"tags": ["a", "b", "c"]})
                    .as_object()
                    .unwrap()
                    .clone(),
            );
            merge_event_metadata(&mut target, Some(&json!({"tags": ["z"]})));
            assert_eq!(target.map(Value::Object), Some(json!({"tags": ["z"]})));
        }

        #[test]
        fn absent_incoming_leaves_existing_and_vice_versa() {
            let existing = json!({"a": 1}).as_object().unwrap().clone();
            let mut target = Some(existing.clone());
            merge_event_metadata(&mut target, None);
            assert_eq!(target, Some(existing));

            let mut target = None;
            merge_event_metadata(&mut target, Some(&json!({"a": 1})));
            assert_eq!(target.map(Value::Object), Some(json!({"a": 1})));
        }

        /// Upstream ignores run-, step- and state-level event metadata
        /// (`apply/default.ts:104-106`); ours only folds where the upstream
        /// `applyEventMetadata` calls do.
        #[test]
        fn run_level_event_metadata_does_not_pollute_messages() {
            let state = apply_all(vec![
                Event::TextMessageStart(agui_rs_core::TextMessageStartEvent {
                    message_id: "m1".into(),
                    role: TextMessageRole::Assistant,
                    attributable: AttributableFields::default(),
                    name: None,
                    base: metadata_event(json!({"source": "run"})),
                }),
                Event::StateSnapshot(StateSnapshotEvent {
                    snapshot: json!({}),
                    attributable: AttributableFields::default(),
                    base: metadata_event(json!({"snap": true})),
                }),
                Event::StepStarted(agui_rs_core::StepStartedEvent {
                    step_name: "s".into(),
                    base: metadata_event(json!({"step": true})),
                    attributable: AttributableFields::default(),
                }),
            ]);

            let Message::Assistant(message) = &state.messages[0] else {
                panic!("expected assistant message");
            };
            assert_eq!(
                message.metadata.as_ref().map(|m| Value::Object(m.clone())),
                Some(json!({"source": "run"}))
            );
        }

        #[test]
        fn text_message_lifecycle_accumulates_event_metadata() {
            let state = apply_all(vec![
                Event::TextMessageStart(agui_rs_core::TextMessageStartEvent {
                    message_id: "m1".into(),
                    role: TextMessageRole::Assistant,
                    attributable: AttributableFields::default(),
                    name: None,
                    base: metadata_event(json!({"trace": "t-1"})),
                }),
                agui_rs_core::factory::text_message_content("m1", "hi"),
                Event::TextMessageEnd(agui_rs_core::TextMessageEndEvent {
                    message_id: "m1".into(),
                    // The end event is where usage typically arrives.
                    base: metadata_event(json!({"usage": {"tokens": 7}})),
                    attributable: AttributableFields::default(),
                }),
            ]);

            let Message::Assistant(message) = &state.messages[0] else {
                panic!("expected assistant message");
            };
            assert_eq!(
                message.metadata.as_ref().map(|m| Value::Object(m.clone())),
                Some(json!({"trace": "t-1", "usage": {"tokens": 7}}))
            );
        }

        #[test]
        fn events_without_metadata_leave_message_metadata_untouched() {
            let state = apply_all(vec![
                Event::TextMessageStart(agui_rs_core::TextMessageStartEvent {
                    message_id: "m1".into(),
                    role: TextMessageRole::Assistant,
                    attributable: AttributableFields::default(),
                    name: None,
                    base: agui_rs_core::BaseEventFields::default(),
                }),
                agui_rs_core::factory::text_message_content("m1", "hi"),
            ]);

            let Message::Assistant(message) = &state.messages[0] else {
                panic!("expected assistant message");
            };
            assert_eq!(message.metadata, None);
        }

        /// A message carried in from a previous run keeps its keys; the stream
        /// only adds on top.
        #[test]
        fn streamed_metadata_merges_into_existing_message_metadata() {
            let mut state = ApplyState {
                messages: vec![Message::Assistant(AssistantMessage {
                    id: "m1".into(),
                    metadata: Some(json!({"kept": true}).as_object().unwrap().clone()),
                    subagent_run_id: None,
                    content: Some(String::new()),
                    name: None,
                    tool_calls: None,
                    encrypted_value: None,
                })],
                state: Value::Null,
            };
            apply_event(
                &mut state,
                &Event::TextMessageContent(agui_rs_core::TextMessageContentEvent {
                    message_id: "m1".into(),
                    delta: "x".into(),
                    base: metadata_event(json!({"added": 1})),
                    attributable: AttributableFields::default(),
                }),
            )
            .unwrap();

            let Message::Assistant(message) = &state.messages[0] else {
                panic!("expected assistant message");
            };
            assert_eq!(
                message.metadata.as_ref().map(|m| Value::Object(m.clone())),
                Some(json!({"kept": true, "added": 1}))
            );
        }

        #[test]
        fn tool_call_events_fold_into_the_tool_call_not_the_parent() {
            let state = apply_all(vec![
                Event::ToolCallStart(ToolCallStartEvent {
                    tool_call_id: "tc1".into(),
                    tool_call_name: "search".into(),
                    parent_message_id: Some("m1".into()),
                    base: metadata_event(json!({"started": true})),
                    attributable: AttributableFields::default(),
                }),
                agui_rs_core::factory::tool_call_args("tc1", "{}"),
                agui_rs_core::factory::tool_call_end("tc1"),
            ]);

            let Message::Assistant(message) = &state.messages[0] else {
                panic!("expected assistant message");
            };
            assert_eq!(message.metadata, None, "the parent stays clean");
            let tool_call = &message.tool_calls.as_ref().expect("tool calls")[0];
            assert_eq!(
                tool_call
                    .metadata
                    .as_ref()
                    .map(|m| Value::Object(m.clone())),
                Some(json!({"started": true}))
            );
        }

        #[test]
        fn tool_result_folds_into_the_tool_message() {
            let state = apply_all(vec![Event::ToolCallResult(ToolCallResultEvent {
                message_id: "tm-1".into(),
                tool_call_id: "tc-1".into(),
                content: "ok".into(),
                attributable: AttributableFields::default(),
                role: Some(ToolResultRole::Tool),
                base: metadata_event(json!({"latency_ms": 42})),
            })]);

            let Message::Tool(message) = &state.messages[0] else {
                panic!("expected tool message");
            };
            assert_eq!(
                message.metadata.as_ref().map(|m| Value::Object(m.clone())),
                Some(json!({"latency_ms": 42}))
            );
        }

        #[test]
        fn reasoning_message_events_fold_into_the_reasoning_message() {
            let state = apply_all(vec![
                Event::ReasoningMessageStart(ReasoningMessageStartEvent {
                    message_id: "r1".into(),
                    role: agui_rs_core::ReasoningMessageRole::Reasoning,
                    attributable: AttributableFields::default(),
                    base: metadata_event(json!({"trace": "r"})),
                }),
                Event::ReasoningMessageContent(agui_rs_core::ReasoningMessageContentEvent {
                    message_id: "r1".into(),
                    delta: "x".into(),
                    base: metadata_event(json!({"tokens": 3})),
                    attributable: AttributableFields::default(),
                }),
            ]);

            let Message::Reasoning(message) = &state.messages[0] else {
                panic!("expected reasoning message");
            };
            assert_eq!(
                message.metadata.as_ref().map(|m| Value::Object(m.clone())),
                Some(json!({"trace": "r", "tokens": 3}))
            );
        }

        #[test]
        fn activity_snapshot_carries_event_metadata() {
            let state = apply_all(vec![Event::ActivitySnapshot(ActivitySnapshotEvent {
                message_id: "a1".into(),
                activity_type: "plan".into(),
                content: serde_json::Map::new(),
                replace: Some(true),
                base: metadata_event(json!({"usage": {"tokens": 9}})),
                attributable: AttributableFields::default(),
            })]);

            let Message::Activity(message) = &state.messages[0] else {
                panic!("expected activity message");
            };
            assert_eq!(
                message.metadata.as_ref().map(|m| Value::Object(m.clone())),
                Some(json!({"usage": {"tokens": 9}}))
            );
        }

        /// The activity-retention fix and the new fold compose: the replace
        /// keeps the accumulated keys and adds the snapshot's on top.
        #[test]
        fn activity_snapshot_replace_keeps_accumulated_and_adds_event_metadata() {
            let mut state = ApplyState {
                messages: vec![Message::Activity(ActivityMessage {
                    id: "a1".into(),
                    metadata: Some(
                        json!({"@ag-ui/client": {"other": 1}})
                            .as_object()
                            .unwrap()
                            .clone(),
                    ),
                    subagent_run_id: None,
                    activity_type: "plan".into(),
                    content: serde_json::Map::new(),
                })],
                state: Value::Null,
            };
            apply_event(
                &mut state,
                &Event::ActivitySnapshot(ActivitySnapshotEvent {
                    message_id: "a1".into(),
                    activity_type: "execute".into(),
                    content: serde_json::Map::new(),
                    replace: None,
                    base: metadata_event(json!({"usage": {"tokens": 2}})),
                    attributable: AttributableFields::default(),
                }),
            )
            .unwrap();

            let Message::Activity(message) = &state.messages[0] else {
                panic!("expected activity message");
            };
            assert_eq!(
                message.metadata.as_ref().map(|m| Value::Object(m.clone())),
                Some(json!({
                    "@ag-ui/client": {"other": 1},
                    "usage": {"tokens": 2}
                }))
            );
        }

        /// Upstream merges before patching so a stale path still costs nothing
        /// but the patch (`apply/default.ts:962-966`).
        #[test]
        fn activity_delta_folds_metadata_even_when_the_patch_is_stale() {
            let state = apply_all(vec![
                Event::ActivitySnapshot(ActivitySnapshotEvent {
                    message_id: "a1".into(),
                    activity_type: "plan".into(),
                    content: serde_json::Map::new(),
                    replace: Some(true),
                    base: agui_rs_core::BaseEventFields::default(),
                    attributable: AttributableFields::default(),
                }),
                Event::ActivityDelta(ActivityDeltaEvent {
                    message_id: "a1".into(),
                    activity_type: "plan".into(),
                    patch: vec![json!({"op": "replace", "path": "/nope", "value": "x"})],
                    base: metadata_event(json!({"seen": true})),
                    attributable: AttributableFields::default(),
                }),
            ]);

            let Message::Activity(message) = &state.messages[0] else {
                panic!("expected activity message");
            };
            assert_eq!(
                message.metadata.as_ref().map(|m| Value::Object(m.clone())),
                Some(json!({"seen": true})),
                "metadata lands despite the stale patch"
            );
        }

        #[test]
        fn messages_snapshot_metadata_is_declared_authority_not_message_metadata() {
            // The snapshot's own metadata never lands on its messages; only the
            // "@ag-ui/client" namespace is read, for authority (the reading-side
            // test above covers that). Here: a plain trace key changes nothing
            // on the messages it carries.
            let state = apply_all(vec![Event::MessagesSnapshot(MessagesSnapshotEvent {
                messages: vec![Message::Assistant(AssistantMessage {
                    id: "m1".into(),
                    metadata: None,
                    subagent_run_id: None,
                    content: Some("hi".into()),
                    name: None,
                    tool_calls: None,
                    encrypted_value: None,
                })],
                base: metadata_event(json!({"trace": "snapshot"})),
            })]);

            let Message::Assistant(message) = &state.messages[0] else {
                panic!("expected assistant message");
            };
            assert_eq!(message.metadata, None);
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
                attributable: AttributableFields::default(),
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
                    attributable: AttributableFields::default(),
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
                    attributable: AttributableFields::default(),
                }),
                Event::ReasoningEncryptedValue(ReasoningEncryptedValueEvent {
                    subtype: ReasoningEncryptedValueSubtype::ToolCall,
                    entity_id: "tc1".into(),
                    encrypted_value: "cipher".into(),
                    attributable: AttributableFields::default(),
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
                attributable: AttributableFields::default(),
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

        #[tokio::test]
        async fn places_tool_result_after_owning_assistant_even_with_trailing_text() {
            let state = apply_all(vec![
                Event::ToolCallStart(ToolCallStartEvent {
                    tool_call_id: "tool1".into(),
                    tool_call_name: "get_weather".into(),
                    parent_message_id: None,
                    base: agui_rs_core::BaseEventFields::default(),
                    attributable: AttributableFields::default(),
                }),
                agui_rs_core::factory::tool_call_end("tool1"),
                Event::TextMessageStart(agui_rs_core::TextMessageStartEvent {
                    message_id: "text1".into(),
                    role: TextMessageRole::Assistant,
                    attributable: AttributableFields::default(),
                    name: None,
                    base: agui_rs_core::BaseEventFields::default(),
                }),
                agui_rs_core::factory::text_message_content("text1", "Here is the weather."),
                agui_rs_core::factory::text_message_end("text1"),
                Event::ToolCallResult(ToolCallResultEvent {
                    message_id: "res1".into(),
                    tool_call_id: "tool1".into(),
                    content: "sunny".into(),
                    attributable: AttributableFields::default(),
                    role: Some(ToolResultRole::Tool),
                    base: agui_rs_core::BaseEventFields::default(),
                }),
            ]);

            let roles: Vec<&str> = state.messages.iter().map(role_name).collect();
            assert_eq!(roles, vec!["assistant", "tool", "assistant"]);

            match &state.messages[0] {
                Message::Assistant(message) => {
                    assert!(message
                        .tool_calls
                        .as_ref()
                        .is_some_and(|tcs| { tcs.iter().any(|tc| tc.id == "tool1") }));
                }
                _ => panic!("expected assistant with tool call at index 0"),
            }
            match &state.messages[1] {
                Message::Tool(message) => assert_eq!(message.tool_call_id, "tool1"),
                _ => panic!("expected tool message at index 1"),
            }
        }

        fn start_event(
            tool_call_id: &str,
            tool_call_name: &str,
            parent_message_id: Option<&str>,
        ) -> Event {
            Event::ToolCallStart(ToolCallStartEvent {
                tool_call_id: tool_call_id.into(),
                tool_call_name: tool_call_name.into(),
                parent_message_id: parent_message_id.map(Into::into),
                base: agui_rs_core::BaseEventFields::default(),
                attributable: AttributableFields::default(),
            })
        }

        // The assistant message an interrupted run leaves behind: the tool call
        // is already in the message list with its arguments fully streamed
        // before the next run starts (TS: `carriedOverAssistant`).
        fn carried_over_assistant() -> Message {
            Message::Assistant(AssistantMessage {
                id: "msg-1".into(),
                metadata: None,
                subagent_run_id: None,
                content: None,
                name: None,
                tool_calls: Some(vec![ToolCall {
                    id: "tc-1".into(),
                    metadata: None,
                    kind: ToolCallKind::Function,
                    function: FunctionCall {
                        name: "openPolicyException".into(),
                        arguments: "{\"txId\":\"t-9\"}".into(),
                    },
                    encrypted_value: None,
                }]),
                encrypted_value: None,
            })
        }

        #[tokio::test]
        async fn duplicate_tool_call_start_does_not_append_second_entry() {
            // The exact same event reaching the reducer twice — a run re-sync
            // replaying it, or one stream delivered over two transports.
            let state = apply_all(vec![
                start_event("tc-1", "search", Some("msg-1")),
                start_event("tc-1", "search", Some("msg-1")),
                agui_rs_core::factory::tool_call_args("tc-1", "{\"query\":\"x\"}"),
                agui_rs_core::factory::tool_call_end("tc-1"),
            ]);

            assert_eq!(state.messages.len(), 1);
            let Message::Assistant(message) = &state.messages[0] else {
                panic!("expected assistant message");
            };
            let tool_calls = message.tool_calls.as_ref().expect("tool calls");
            assert_eq!(tool_calls.len(), 1);
            assert_eq!(tool_calls[0].id, "tc-1");
            // The single surviving copy carries the arguments — without the
            // guard the deltas resolve to the first match and the second copy
            // stays empty.
            assert_eq!(tool_calls[0].function.arguments, "{\"query\":\"x\"}");
        }

        #[tokio::test]
        async fn replayed_start_for_carried_over_tool_call_preserves_arguments() {
            let mut state = ApplyState {
                messages: vec![carried_over_assistant()],
                state: Value::Null,
            };

            apply_event(
                &mut state,
                &start_event("tc-1", "openPolicyException", Some("msg-1")),
            )
            .expect("replayed start applies");
            apply_event(
                &mut state,
                &Event::ToolCallResult(ToolCallResultEvent {
                    message_id: "tm-1".into(),
                    tool_call_id: "tc-1".into(),
                    content: "approved".into(),
                    attributable: AttributableFields::default(),
                    role: Some(ToolResultRole::Tool),
                    base: agui_rs_core::BaseEventFields::default(),
                }),
            )
            .expect("result applies");

            assert_eq!(state.messages.len(), 2);
            let Message::Assistant(message) = &state.messages[0] else {
                panic!("expected assistant message");
            };
            assert_eq!(message.id, "msg-1");
            let tool_calls = message.tool_calls.as_ref().expect("tool calls");
            assert_eq!(tool_calls.len(), 1);
            // Arguments streamed on the previous run survive — a start event
            // carries none, so overwriting them would blank the call out.
            assert_eq!(tool_calls[0].function.arguments, "{\"txId\":\"t-9\"}");
        }

        #[tokio::test]
        async fn replayed_start_with_unknown_parent_creates_no_stray_assistant() {
            // The dedupe has to run before the parent is resolved: a replay
            // whose parentMessageId is no longer in state would otherwise
            // create a fresh assistant message to hang the duplicate off.
            let mut state = ApplyState {
                messages: vec![carried_over_assistant()],
                state: Value::Null,
            };

            apply_event(
                &mut state,
                &start_event("tc-1", "openPolicyException", Some("msg-regenerated")),
            )
            .expect("replayed start applies");
            apply_event(
                &mut state,
                &Event::ToolCallResult(ToolCallResultEvent {
                    message_id: "tm-1".into(),
                    tool_call_id: "tc-1".into(),
                    content: "approved".into(),
                    attributable: AttributableFields::default(),
                    role: Some(ToolResultRole::Tool),
                    base: agui_rs_core::BaseEventFields::default(),
                }),
            )
            .expect("result applies");

            let ids: Vec<&str> = state.messages.iter().map(|m| m.id()).collect();
            assert_eq!(ids, vec!["msg-1", "tm-1"]);
            let assistants: Vec<_> = state
                .messages
                .iter()
                .filter(|m| matches!(m, Message::Assistant(_)))
                .collect();
            assert_eq!(assistants.len(), 1);
            let Message::Assistant(assistant) = assistants[0] else {
                unreachable!();
            };
            assert_eq!(assistant.tool_calls.as_ref().expect("tool calls").len(), 1);
        }

        #[tokio::test]
        async fn replayed_start_with_different_name_updates_in_place() {
            // A start reusing an id under a different name updates the existing
            // entry in place and leaves the streamed arguments untouched.
            let state = apply_all(vec![
                start_event("tc-1", "search", None),
                agui_rs_core::factory::tool_call_args("tc-1", "{\"query\":\"x\"}"),
                start_event("tc-1", "lookup", None),
            ]);

            assert_eq!(state.messages.len(), 1);
            let Message::Assistant(message) = &state.messages[0] else {
                panic!("expected assistant message");
            };
            let tool_calls = message.tool_calls.as_ref().expect("tool calls");
            assert_eq!(tool_calls.len(), 1);
            assert_eq!(tool_calls[0].function.name, "lookup");
            assert_eq!(tool_calls[0].function.arguments, "{\"query\":\"x\"}");
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
                attributable: AttributableFields::default(),
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
                    attributable: AttributableFields::default(),
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
                        metadata: None,
                        subagent_run_id: None,
                        content: "plan".into(),
                        encrypted_value: None,
                    }),
                    Message::Assistant(AssistantMessage {
                        id: "m1".into(),
                        metadata: None,
                        subagent_run_id: None,
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
                        metadata: None,
                        subagent_run_id: None,
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
        async fn messages_snapshot_replaces_streamed_reasoning_when_snapshot_carries_it() {
            let mut state = ApplyState {
                messages: vec![
                    Message::User(UserMessage {
                        id: "m1".into(),
                        metadata: None,
                        subagent_run_id: None,
                        content: UserMessageContent::Text("What is the best car to buy?".into()),
                        name: None,
                        encrypted_value: None,
                    }),
                    Message::Reasoning(ReasoningMessage {
                        id: "uuid-a".into(),
                        metadata: None,
                        subagent_run_id: None,
                        content: "The user wants a car recommendation.".into(),
                        encrypted_value: None,
                    }),
                    Message::Assistant(AssistantMessage {
                        id: "lc-1".into(),
                        metadata: None,
                        subagent_run_id: None,
                        content: Some("Based on my analysis.".into()),
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
                    messages: vec![
                        Message::User(UserMessage {
                            id: "m1".into(),
                            metadata: None,
                            subagent_run_id: None,
                            content: UserMessageContent::Text(
                                "What is the best car to buy?".into(),
                            ),
                            name: None,
                            encrypted_value: None,
                        }),
                        Message::Reasoning(ReasoningMessage {
                            id: "rs-1".into(),
                            metadata: None,
                            subagent_run_id: None,
                            content: "The user wants a car recommendation.".into(),
                            encrypted_value: None,
                        }),
                        Message::Assistant(AssistantMessage {
                            id: "resp-1".into(),
                            metadata: None,
                            subagent_run_id: None,
                            content: Some("Based on my analysis.".into()),
                            name: None,
                            tool_calls: None,
                            encrypted_value: None,
                        }),
                    ],
                    base: agui_rs_core::BaseEventFields::default(),
                }),
            )
            .expect("snapshot should apply");

            let reasoning_count = state
                .messages
                .iter()
                .filter(|m| matches!(m, Message::Reasoning(_)))
                .count();
            assert_eq!(reasoning_count, 1);
            let ids: Vec<&str> = state.messages.iter().map(|m| m.id()).collect();
            assert_eq!(ids, vec!["m1", "rs-1", "resp-1"]);
        }

        #[tokio::test]
        async fn messages_snapshot_preserves_activity_when_snapshot_carries_reasoning() {
            let mut state = ApplyState {
                messages: vec![
                    Message::User(UserMessage {
                        id: "m1".into(),
                        metadata: None,
                        subagent_run_id: None,
                        content: UserMessageContent::Text("hello".into()),
                        name: None,
                        encrypted_value: None,
                    }),
                    Message::Activity(ActivityMessage {
                        id: "act-1".into(),
                        metadata: None,
                        subagent_run_id: None,
                        activity_type: "PLAN".into(),
                        content: serde_json::Map::from_iter([(
                            String::from("tasks"),
                            json!(["a"]),
                        )]),
                    }),
                    Message::Reasoning(ReasoningMessage {
                        id: "uuid-a".into(),
                        metadata: None,
                        subagent_run_id: None,
                        content: "thinking".into(),
                        encrypted_value: None,
                    }),
                    Message::Assistant(AssistantMessage {
                        id: "lc-1".into(),
                        metadata: None,
                        subagent_run_id: None,
                        content: Some("hi".into()),
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
                    messages: vec![
                        Message::User(UserMessage {
                            id: "m1".into(),
                            metadata: None,
                            subagent_run_id: None,
                            content: UserMessageContent::Text("hello".into()),
                            name: None,
                            encrypted_value: None,
                        }),
                        Message::Reasoning(ReasoningMessage {
                            id: "rs-1".into(),
                            metadata: None,
                            subagent_run_id: None,
                            content: "thinking".into(),
                            encrypted_value: None,
                        }),
                        Message::Assistant(AssistantMessage {
                            id: "resp-1".into(),
                            metadata: None,
                            subagent_run_id: None,
                            content: Some("hi".into()),
                            name: None,
                            tool_calls: None,
                            encrypted_value: None,
                        }),
                    ],
                    base: agui_rs_core::BaseEventFields::default(),
                }),
            )
            .expect("snapshot should apply");

            let activity_count = state
                .messages
                .iter()
                .filter(|m| matches!(m, Message::Activity(_)))
                .count();
            assert_eq!(activity_count, 1);
            let reasoning_count = state
                .messages
                .iter()
                .filter(|m| matches!(m, Message::Reasoning(_)))
                .count();
            assert_eq!(reasoning_count, 1);
            let ids: Vec<&str> = state.messages.iter().map(|m| m.id()).collect();
            assert_eq!(ids, vec!["m1", "act-1", "rs-1", "resp-1"]);
        }

        fn activity(id: &str, tasks: &[&str]) -> Message {
            Message::Activity(ActivityMessage {
                id: id.into(),
                metadata: None,
                subagent_run_id: None,
                activity_type: "PLAN".into(),
                content: serde_json::Map::from_iter([(String::from("tasks"), json!(tasks))]),
            })
        }

        fn snapshot(messages: Vec<Message>) -> Event {
            Event::MessagesSnapshot(MessagesSnapshotEvent {
                messages,
                base: agui_rs_core::BaseEventFields::default(),
            })
        }

        #[tokio::test]
        async fn messages_snapshot_replaces_activity_with_snapshot_version() {
            let mut state = ApplyState {
                messages: vec![
                    Message::User(UserMessage {
                        id: "m1".into(),
                        metadata: None,
                        subagent_run_id: None,
                        content: UserMessageContent::Text("hello".into()),
                        name: None,
                        encrypted_value: None,
                    }),
                    activity("act-1", &["stale"]),
                    Message::Assistant(AssistantMessage {
                        id: "a1".into(),
                        metadata: None,
                        subagent_run_id: None,
                        content: Some("hi".into()),
                        name: None,
                        tool_calls: None,
                        encrypted_value: None,
                    }),
                ],
                state: Value::Null,
            };

            apply_event(
                &mut state,
                &snapshot(vec![
                    Message::User(UserMessage {
                        id: "m1".into(),
                        metadata: None,
                        subagent_run_id: None,
                        content: UserMessageContent::Text("hello".into()),
                        name: None,
                        encrypted_value: None,
                    }),
                    activity("act-1", &["fresh"]),
                    Message::Assistant(AssistantMessage {
                        id: "a1".into(),
                        metadata: None,
                        subagent_run_id: None,
                        content: Some("hi".into()),
                        name: None,
                        tool_calls: None,
                        encrypted_value: None,
                    }),
                ]),
            )
            .expect("snapshot should apply");

            let activity_count = state
                .messages
                .iter()
                .filter(|m| matches!(m, Message::Activity(_)))
                .count();
            assert_eq!(activity_count, 1);
            let ids: Vec<&str> = state.messages.iter().map(|m| m.id()).collect();
            assert_eq!(ids, vec!["m1", "act-1", "a1"]);
            let Message::Activity(message) = &state.messages[1] else {
                panic!("expected activity message");
            };
            assert_eq!(message.content.get("tasks"), Some(&json!(["fresh"])));
        }

        #[tokio::test]
        async fn messages_snapshot_appends_activity_client_does_not_have() {
            let mut state = ApplyState {
                messages: vec![Message::User(UserMessage {
                    id: "m1".into(),
                    metadata: None,
                    subagent_run_id: None,
                    content: UserMessageContent::Text("hello".into()),
                    name: None,
                    encrypted_value: None,
                })],
                state: Value::Null,
            };

            apply_event(
                &mut state,
                &snapshot(vec![
                    Message::User(UserMessage {
                        id: "m1".into(),
                        metadata: None,
                        subagent_run_id: None,
                        content: UserMessageContent::Text("hello".into()),
                        name: None,
                        encrypted_value: None,
                    }),
                    activity("act-1", &["fresh"]),
                ]),
            )
            .expect("snapshot should apply");

            let ids: Vec<&str> = state.messages.iter().map(|m| m.id()).collect();
            assert_eq!(ids, vec!["m1", "act-1"]);
            let Message::Activity(message) = &state.messages[1] else {
                panic!("expected activity message");
            };
            assert_eq!(message.content.get("tasks"), Some(&json!(["fresh"])));
        }

        #[tokio::test]
        async fn messages_snapshot_drops_activity_it_leaves_out_when_it_carries_activity() {
            // A snapshot carrying any activity declares the complete activity
            // set: entries it repeats are replaced, ones it leaves out are
            // removed — preserving them would make the local copy undeletable.
            let mut state = ApplyState {
                messages: vec![
                    Message::User(UserMessage {
                        id: "m1".into(),
                        metadata: None,
                        subagent_run_id: None,
                        content: UserMessageContent::Text("hello".into()),
                        name: None,
                        encrypted_value: None,
                    }),
                    activity("act-gone", &["gone"]),
                    activity("act-1", &["stale"]),
                ],
                state: Value::Null,
            };

            apply_event(
                &mut state,
                &snapshot(vec![
                    Message::User(UserMessage {
                        id: "m1".into(),
                        metadata: None,
                        subagent_run_id: None,
                        content: UserMessageContent::Text("hello".into()),
                        name: None,
                        encrypted_value: None,
                    }),
                    activity("act-1", &["fresh"]),
                ]),
            )
            .expect("snapshot should apply");

            let ids: Vec<&str> = state.messages.iter().map(|m| m.id()).collect();
            assert_eq!(ids, vec!["m1", "act-1"]);
            let Message::Activity(message) = &state.messages[1] else {
                panic!("expected activity message");
            };
            assert_eq!(message.content.get("tasks"), Some(&json!(["fresh"])));
        }

        #[tokio::test]
        async fn messages_snapshot_keeps_activity_when_it_carries_none() {
            let mut state = ApplyState {
                messages: vec![
                    Message::User(UserMessage {
                        id: "m1".into(),
                        metadata: None,
                        subagent_run_id: None,
                        content: UserMessageContent::Text("hello".into()),
                        name: None,
                        encrypted_value: None,
                    }),
                    activity("act-1", &["local"]),
                    Message::Assistant(AssistantMessage {
                        id: "a1".into(),
                        metadata: None,
                        subagent_run_id: None,
                        content: Some("hi".into()),
                        name: None,
                        tool_calls: None,
                        encrypted_value: None,
                    }),
                ],
                state: Value::Null,
            };

            apply_event(
                &mut state,
                &snapshot(vec![
                    Message::User(UserMessage {
                        id: "m1".into(),
                        metadata: None,
                        subagent_run_id: None,
                        content: UserMessageContent::Text("hello".into()),
                        name: None,
                        encrypted_value: None,
                    }),
                    Message::Assistant(AssistantMessage {
                        id: "a1".into(),
                        metadata: None,
                        subagent_run_id: None,
                        content: Some("hi".into()),
                        name: None,
                        tool_calls: None,
                        encrypted_value: None,
                    }),
                ]),
            )
            .expect("snapshot should apply");

            let ids: Vec<&str> = state.messages.iter().map(|m| m.id()).collect();
            assert_eq!(ids, vec!["m1", "act-1", "a1"]);
            let Message::Activity(message) = &state.messages[1] else {
                panic!("expected activity message");
            };
            assert_eq!(message.content.get("tasks"), Some(&json!(["local"])));
        }

        #[tokio::test]
        async fn messages_snapshot_updates_id_stable_reasoning_with_snapshot_version() {
            let mut state = ApplyState {
                messages: vec![
                    Message::User(UserMessage {
                        id: "m1".into(),
                        metadata: None,
                        subagent_run_id: None,
                        content: UserMessageContent::Text("hello".into()),
                        name: None,
                        encrypted_value: None,
                    }),
                    Message::Reasoning(ReasoningMessage {
                        id: "r1".into(),
                        metadata: None,
                        subagent_run_id: None,
                        content: "thinking".into(),
                        encrypted_value: None,
                    }),
                    Message::Assistant(AssistantMessage {
                        id: "a1".into(),
                        metadata: None,
                        subagent_run_id: None,
                        content: Some("hi".into()),
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
                    messages: vec![
                        Message::User(UserMessage {
                            id: "m1".into(),
                            metadata: None,
                            subagent_run_id: None,
                            content: UserMessageContent::Text("hello".into()),
                            name: None,
                            encrypted_value: None,
                        }),
                        Message::Reasoning(ReasoningMessage {
                            id: "r1".into(),
                            metadata: None,
                            subagent_run_id: None,
                            content: "thinking".into(),
                            encrypted_value: Some("enc-1".into()),
                        }),
                        Message::Assistant(AssistantMessage {
                            id: "a1".into(),
                            metadata: None,
                            subagent_run_id: None,
                            content: Some("hi".into()),
                            name: None,
                            tool_calls: None,
                            encrypted_value: None,
                        }),
                    ],
                    base: agui_rs_core::BaseEventFields::default(),
                }),
            )
            .expect("snapshot should apply");

            let reasoning_count = state
                .messages
                .iter()
                .filter(|m| matches!(m, Message::Reasoning(_)))
                .count();
            assert_eq!(reasoning_count, 1);
            match &state.messages[1] {
                Message::Reasoning(message) => {
                    assert_eq!(message.encrypted_value.as_deref(), Some("enc-1"));
                }
                _ => panic!("expected reasoning message at index 1"),
            }
        }

        #[tokio::test]
        async fn messages_snapshot_converges_multi_turn_reasoning() {
            let mut state = ApplyState {
                messages: vec![
                    Message::User(UserMessage {
                        id: "u1".into(),
                        metadata: None,
                        subagent_run_id: None,
                        content: UserMessageContent::Text("q1".into()),
                        name: None,
                        encrypted_value: None,
                    }),
                    Message::Reasoning(ReasoningMessage {
                        id: "rs-1".into(),
                        metadata: None,
                        subagent_run_id: None,
                        content: "thinking about q1".into(),
                        encrypted_value: None,
                    }),
                    Message::Assistant(AssistantMessage {
                        id: "resp-1".into(),
                        metadata: None,
                        subagent_run_id: None,
                        content: Some("a1".into()),
                        name: None,
                        tool_calls: None,
                        encrypted_value: None,
                    }),
                    Message::User(UserMessage {
                        id: "u2".into(),
                        metadata: None,
                        subagent_run_id: None,
                        content: UserMessageContent::Text("q2".into()),
                        name: None,
                        encrypted_value: None,
                    }),
                    Message::Reasoning(ReasoningMessage {
                        id: "uuid-b".into(),
                        metadata: None,
                        subagent_run_id: None,
                        content: "thinking about q2".into(),
                        encrypted_value: None,
                    }),
                    Message::Assistant(AssistantMessage {
                        id: "lc-2".into(),
                        metadata: None,
                        subagent_run_id: None,
                        content: Some("a2".into()),
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
                    messages: vec![
                        Message::User(UserMessage {
                            id: "u1".into(),
                            metadata: None,
                            subagent_run_id: None,
                            content: UserMessageContent::Text("q1".into()),
                            name: None,
                            encrypted_value: None,
                        }),
                        Message::Reasoning(ReasoningMessage {
                            id: "rs-1".into(),
                            metadata: None,
                            subagent_run_id: None,
                            content: "thinking about q1".into(),
                            encrypted_value: None,
                        }),
                        Message::Assistant(AssistantMessage {
                            id: "resp-1".into(),
                            metadata: None,
                            subagent_run_id: None,
                            content: Some("a1".into()),
                            name: None,
                            tool_calls: None,
                            encrypted_value: None,
                        }),
                        Message::User(UserMessage {
                            id: "u2".into(),
                            metadata: None,
                            subagent_run_id: None,
                            content: UserMessageContent::Text("q2".into()),
                            name: None,
                            encrypted_value: None,
                        }),
                        Message::Reasoning(ReasoningMessage {
                            id: "rs-2".into(),
                            metadata: None,
                            subagent_run_id: None,
                            content: "thinking about q2".into(),
                            encrypted_value: None,
                        }),
                        Message::Assistant(AssistantMessage {
                            id: "resp-2".into(),
                            metadata: None,
                            subagent_run_id: None,
                            content: Some("a2".into()),
                            name: None,
                            tool_calls: None,
                            encrypted_value: None,
                        }),
                    ],
                    base: agui_rs_core::BaseEventFields::default(),
                }),
            )
            .expect("snapshot should apply");

            let reasoning_count = state
                .messages
                .iter()
                .filter(|m| matches!(m, Message::Reasoning(_)))
                .count();
            assert_eq!(reasoning_count, 2);
            let ids: Vec<&str> = state.messages.iter().map(|m| m.id()).collect();
            assert_eq!(ids, vec!["u1", "rs-1", "resp-1", "u2", "rs-2", "resp-2"]);
        }

        #[tokio::test]
        async fn apply_event_updates_both_messages_and_state() {
            let mut state = ApplyState::default();

            apply_event(
                &mut state,
                &Event::TextMessageStart(agui_rs_core::TextMessageStartEvent {
                    message_id: "m1".into(),
                    role: TextMessageRole::Assistant,
                    attributable: AttributableFields::default(),
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
                    attributable: AttributableFields::default(),
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

    /// The upstream handlers announce a defect with `console.warn` and return —
    /// the run continues (`apply/default.ts:284-286 / 291-295 / 336-343 /
    /// 483-495 / 554-557 / 936-940 / 1444-1447`). Our earlier `?` propagation
    /// turned each into a run-fatal error that stranded `RUN_FINISHED`.
    mod warn_not_fatal {
        use super::*;
        use agui_rs_core::{
            ReasoningEncryptedValueEvent, ReasoningMessageContentEvent, ReasoningMessageRole,
            ReasoningMessageStartEvent, RunFinishedEvent,
        };

        fn base(metadata: Value) -> agui_rs_core::BaseEventFields {
            agui_rs_core::BaseEventFields {
                metadata: Some(metadata),
                ..Default::default()
            }
        }

        fn reasoning_start(message_id: &str, metadata: Value) -> Event {
            Event::ReasoningMessageStart(ReasoningMessageStartEvent {
                message_id: message_id.into(),
                role: ReasoningMessageRole::Reasoning,
                attributable: AttributableFields::default(),
                base: base(metadata),
            })
        }

        #[tokio::test]
        async fn text_content_without_start_warns_and_the_run_continues() {
            // default.ts:284-286: a delta without its START is dropped with a
            // warning; downstream events — including RUN_FINISHED — still apply.
            let items = default_apply_events(
                stream::iter(vec![
                    Ok(agui_rs_core::factory::text_message_content("m1", "orphan")),
                    Ok(Event::RunFinished(RunFinishedEvent {
                        thread_id: "t".into(),
                        run_id: "r".into(),
                        result: None,
                        outcome: None,
                        usage: Vec::new(),
                        base: Default::default(),
                    })),
                ]),
                Vec::new(),
                Value::Null,
            )
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .collect::<Result<Vec<_>>>()
            .expect("the stream survives an orphan delta");

            assert_eq!(items.len(), 2, "both events yielded");
            assert!(items[0].messages.is_empty());
        }

        #[tokio::test]
        async fn content_into_an_activity_message_warns_and_the_run_continues() {
            // default.ts:291-295: appending a string to an activity message
            // would invalidate it; warn, leave it alone, keep streaming.
            let items = default_apply_events(
                stream::iter(vec![
                    Ok(Event::ActivitySnapshot(ActivitySnapshotEvent {
                        message_id: "a1".into(),
                        activity_type: "plan".into(),
                        content: serde_json::Map::from_iter([(
                            String::from("tasks"),
                            json!(["plan"]),
                        )]),
                        attributable: AttributableFields::default(),
                        replace: Some(true),
                        base: base(json!({"origin": "activity"})),
                    })),
                    Ok(agui_rs_core::factory::text_message_content("a1", "Hello")),
                    Ok(Event::RunFinished(RunFinishedEvent {
                        thread_id: "t".into(),
                        run_id: "r".into(),
                        result: None,
                        outcome: None,
                        usage: Vec::new(),
                        base: Default::default(),
                    })),
                ]),
                Vec::new(),
                Value::Null,
            )
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .collect::<Result<Vec<_>>>()
            .expect("the stream survives content aimed at an activity message");

            let last = items.last().expect("final event");
            let [message] = &last.messages[..] else {
                panic!("expected exactly one message, got {:?}", last.messages);
            };
            let Message::Activity(activity) = message else {
                panic!("expected activity message, got {message:?}");
            };
            // The structured content stands and the activity's own metadata
            // stays off the text event's.
            assert_eq!(activity.content.get("tasks"), Some(&json!(["plan"])));
            assert_eq!(
                activity.metadata.as_ref().map(|m| Value::Object(m.clone())),
                Some(json!({"origin": "activity"}))
            );
        }

        #[tokio::test]
        async fn tool_call_args_for_unknown_id_warn_and_the_run_continues() {
            // default.ts:492-495.
            let state = apply_all(vec![
                agui_rs_core::factory::tool_call_args("tc-ghost", "{\"q\":1}"),
                agui_rs_core::factory::tool_call_end("tc-ghost"),
            ]);
            assert!(state.messages.is_empty());
        }

        #[tokio::test]
        async fn text_start_reusing_an_activity_id_is_dropped_and_content_skipped() {
            // S8 — default.ts:236-246: the start is warned away, and the
            // following CONTENT then hits the activity guard at 291-295, so
            // the activity stands untouched and the stream lives.
            let items = default_apply_events(
                stream::iter(vec![
                    Ok(Event::ActivitySnapshot(ActivitySnapshotEvent {
                        message_id: "a1".into(),
                        activity_type: "plan".into(),
                        content: serde_json::Map::from_iter([(
                            String::from("tasks"),
                            json!(["plan"]),
                        )]),
                        attributable: AttributableFields::default(),
                        replace: Some(true),
                        base: base(json!({"origin": "activity"})),
                    })),
                    Ok(Event::TextMessageStart(
                        agui_rs_core::TextMessageStartEvent {
                            message_id: "a1".into(),
                            role: TextMessageRole::Assistant,
                            attributable: AttributableFields::default(),
                            name: None,
                            base: base(json!({"origin": "text"})),
                        },
                    )),
                    Ok(agui_rs_core::factory::text_message_content("a1", "leak")),
                    Ok(Event::RunFinished(RunFinishedEvent {
                        thread_id: "t".into(),
                        run_id: "r".into(),
                        result: None,
                        outcome: None,
                        usage: Vec::new(),
                        base: Default::default(),
                    })),
                ]),
                Vec::new(),
                Value::Null,
            )
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .collect::<Result<Vec<_>>>()
            .expect("the stream survives a reused activity id");

            assert_eq!(items.len(), 4, "every event yields");
            let last = items.last().expect("final event");
            let [message] = &last.messages[..] else {
                panic!("expected exactly one message, got {:?}", last.messages);
            };
            let Message::Activity(activity) = message else {
                panic!("expected the activity message, got {message:?}");
            };
            assert_eq!(activity.content.get("tasks"), Some(&json!(["plan"])));
            assert_eq!(
                activity.metadata.as_ref().map(|m| Value::Object(m.clone())),
                Some(json!({"origin": "activity"})),
                "neither text event's metadata lands on the activity"
            );
        }

        #[tokio::test]
        async fn content_after_a_messages_snapshot_flush_warns_and_the_run_continues() {
            // A MESSAGES_SNAPSHOT without the streamed message flushes it; the
            // next CONTENT for its id finds nothing and is dropped with a
            // warning (default.ts:284-286), not fatal.
            let items = default_apply_events(
                stream::iter(vec![
                    Ok(Event::TextMessageStart(
                        agui_rs_core::TextMessageStartEvent {
                            message_id: "m1".into(),
                            role: TextMessageRole::Assistant,
                            attributable: AttributableFields::default(),
                            name: None,
                            base: Default::default(),
                        },
                    )),
                    Ok(Event::MessagesSnapshot(MessagesSnapshotEvent {
                        messages: vec![],
                        base: Default::default(),
                    })),
                    Ok(agui_rs_core::factory::text_message_content("m1", "lost")),
                    Ok(Event::RunFinished(RunFinishedEvent {
                        thread_id: "t".into(),
                        run_id: "r".into(),
                        result: None,
                        outcome: None,
                        usage: Vec::new(),
                        base: Default::default(),
                    })),
                ]),
                Vec::new(),
                Value::Null,
            )
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .collect::<Result<Vec<_>>>()
            .expect("the stream survives content after a snapshot flush");

            let last = items.last().expect("final event");
            assert!(last.messages.is_empty());
        }

        #[tokio::test]
        async fn encrypted_value_on_an_activity_message_warns_and_the_run_continues() {
            // default.ts:1443-1447: activity messages do not carry
            // encryptedValue; the event is ignored, the run goes on.
            let state = apply_all(vec![
                Event::ActivitySnapshot(ActivitySnapshotEvent {
                    message_id: "a1".into(),
                    activity_type: "plan".into(),
                    content: serde_json::Map::new(),
                    attributable: AttributableFields::default(),
                    replace: Some(true),
                    base: Default::default(),
                }),
                Event::ReasoningEncryptedValue(ReasoningEncryptedValueEvent {
                    subtype: ReasoningEncryptedValueSubtype::Message,
                    entity_id: "a1".into(),
                    encrypted_value: "cipher".into(),
                    attributable: AttributableFields::default(),
                    base: Default::default(),
                }),
                Event::RunStarted(RunStartedEvent {
                    thread_id: "t".into(),
                    run_id: "r".into(),
                    protocol_version: None,
                    parent_run_id: None,
                    input: None,
                    base: Default::default(),
                }),
            ]);

            let [message] = &state.messages[..] else {
                panic!("expected one message");
            };
            // ActivityMessage carries no encrypted_value at all — the entity is
            // untouched either way.
            let Message::Activity(_) = message else {
                panic!("expected activity message, got {message:?}");
            };
        }

        #[tokio::test]
        async fn reasoning_content_for_activity_id_warns_and_the_run_continues() {
            // default.ts:1292-1298: same collision as the text handlers.
            let state = apply_all(vec![
                Event::ActivitySnapshot(ActivitySnapshotEvent {
                    message_id: "shared".into(),
                    activity_type: "plan".into(),
                    content: serde_json::Map::from_iter([(String::from("tasks"), json!(["plan"]))]),
                    attributable: AttributableFields::default(),
                    replace: Some(true),
                    base: Default::default(),
                }),
                Event::ReasoningMessageContent(ReasoningMessageContentEvent {
                    message_id: "shared".into(),
                    delta: "thinking".into(),
                    attributable: AttributableFields::default(),
                    base: Default::default(),
                }),
            ]);

            let [message] = &state.messages[..] else {
                panic!("expected one message");
            };
            let Message::Activity(activity) = message else {
                panic!("expected activity message, got {message:?}");
            };
            assert_eq!(activity.content.get("tasks"), Some(&json!(["plan"])));
        }

        #[tokio::test]
        async fn start_for_activity_id_warns_and_drops_the_event() {
            // default.ts:1253-1262 (reasoning) and 236-246 (text): an id an
            // activity message holds means the producer reused it — warn, leave
            // the activity alone, drop the event and its metadata.
            let state = apply_all(vec![
                Event::ActivitySnapshot(ActivitySnapshotEvent {
                    message_id: "shared".into(),
                    activity_type: "plan".into(),
                    content: serde_json::Map::new(),
                    attributable: AttributableFields::default(),
                    replace: Some(true),
                    base: base(json!({"origin": "activity"})),
                }),
                reasoning_start("shared", json!({"origin": "reasoning"})),
            ]);

            let [message] = &state.messages[..] else {
                panic!("expected one message");
            };
            let Message::Activity(activity) = message else {
                panic!("expected activity message, got {message:?}");
            };
            assert_eq!(
                activity.metadata.as_ref().map(|m| Value::Object(m.clone())),
                Some(json!({"origin": "activity"})),
                "the reasoning event's metadata does not land on the activity"
            );
        }
    }

    mod run_started_input {
        use super::*;
        use agui_rs_core::RunAgentInput;

        fn user_message(id: &str, content: &str) -> Message {
            Message::User(UserMessage {
                id: id.into(),
                metadata: None,
                subagent_run_id: None,
                content: UserMessageContent::Text(content.into()),
                name: None,
                encrypted_value: None,
            })
        }

        fn run_started(messages: Vec<Message>) -> Event {
            Event::RunStarted(RunStartedEvent {
                thread_id: "t".into(),
                run_id: "r".into(),
                protocol_version: None,
                parent_run_id: None,
                input: Some(RunAgentInput {
                    thread_id: "t".into(),
                    run_id: "r".into(),
                    protocol_version: None,
                    parent_run_id: None,
                    state: None,
                    messages,
                    tools: Vec::new(),
                    context: Vec::new(),
                    forwarded_props: None,
                    resume: None,
                }),
                base: Default::default(),
            })
        }

        /// run-started-input.test.ts:29-66: messages carried in
        /// `RUN_STARTED.input.messages` join the transcript.
        #[test]
        fn adds_messages_not_already_present() {
            let state = apply_all(vec![run_started(vec![
                user_message("msg-1", "Hello"),
                user_message("msg-2", "How are you?"),
            ])]);

            let ids: Vec<&str> = state.messages.iter().map(|m| m.id()).collect();
            assert_eq!(ids, vec!["msg-1", "msg-2"]);
        }

        /// run-started-input.test.ts:68-141: already-present ids are kept as
        /// they are; only new ids join.
        #[test]
        fn does_not_duplicate_existing_ids() {
            let mut state = ApplyState {
                messages: vec![user_message("msg-1", "Existing")],
                state: Value::Null,
            };
            apply_event(
                &mut state,
                &run_started(vec![
                    user_message("msg-1", "Duplicate (ignored)"),
                    user_message("msg-2", "New"),
                ]),
            )
            .unwrap();

            let ids: Vec<&str> = state.messages.iter().map(|m| m.id()).collect();
            assert_eq!(ids, vec!["msg-1", "msg-2"]);
            match &state.messages[0] {
                Message::User(user) => match &user.content {
                    UserMessageContent::Text(text) => assert_eq!(text, "Existing"),
                    other => panic!("expected text content, got {other:?}"),
                },
                other => panic!("expected user message, got {other:?}"),
            }
        }

        /// run-started-input.test.ts:144-171: no input, no projection.
        #[test]
        fn without_input_is_a_noop() {
            let state = apply_all(vec![Event::RunStarted(RunStartedEvent {
                thread_id: "t".into(),
                run_id: "r".into(),
                protocol_version: None,
                parent_run_id: None,
                input: None,
                base: Default::default(),
            })]);
            assert!(state.messages.is_empty());
        }
    }
}
