use agui_rs_core::{
    AgUiError, Event, Message, ReasoningEncryptedValueSubtype, SubagentStartedEvent,
};
use async_stream::stream;
use futures::{Stream, StreamExt};
use std::collections::{HashMap, HashSet};

type VerifyResult<T> = std::result::Result<T, AgUiError>;

/// One entity's recorded owner (upstream `verify/verify.ts:53`). An entry with
/// `subagent_run_id: None` means the PARENT owns the entity — which is as much
/// an owner as a subagent is, so a tagged continuation on it still disagrees.
#[derive(Debug, Clone, Default, PartialEq)]
struct Owner {
    subagent_run_id: Option<String>,
}

/// Owners, retained for the whole run, in one bucket PER ENTITY KIND
/// (upstream `verify/verify.ts:42-59`). An id is only unique within a kind,
/// and entries outlive the entity's close so later continuations —
/// `REASONING_ENCRYPTED_VALUE` after `*_END`, an opener re-opening a closed
/// id — still have an owner to check against.
#[derive(Debug, Default)]
struct Owners {
    message: HashMap<String, Owner>,
    tool_call: HashMap<String, Owner>,
    activity: HashMap<String, Owner>,
    reasoning: HashMap<String, Owner>,
}

#[derive(Debug, Default)]
struct VerifierState {
    first_event_received: bool,
    run_started: bool,
    run_finished: bool,
    /// Open text message ids (upstream `activeMessages` Set,
    /// `verify/verify.ts:15`). A SET, not a slot: distinct messages stream
    /// concurrently and may END out of order (upstream
    /// `verify/__tests__/verify.concurrent.test.ts:23-96`).
    active_text_messages: HashSet<String>,
    /// Open tool call ids keyed by id, valued by the `parent_message_id` the
    /// START named (upstream `activeToolCalls` Set plus the per-call parent the
    /// CHUNK continuation must match). Concurrency applies here too.
    active_tool_calls: HashMap<String, Option<String>>,
    // Reasoning has TWO bracketed entities, not one. A SPAN is opened by
    // REASONING_START and closed by REASONING_END; a reasoning MESSAGE is
    // opened by REASONING_MESSAGE_START and closed by REASONING_MESSAGE_END
    // (upstream `verify/verify.ts:22-38`). The specification says the span's
    // identifier "namespaces nothing" and the messages inside carry their own
    // ids, so the two are tracked separately — which also means one id may
    // legitimately name both. Reasoning was the one streaming entity whose
    // open/close discipline went unverified: a content event with no opener, a
    // message never closed, and a span closed without being opened all passed.
    active_reasoning_spans: HashSet<String>,
    active_reasoning_messages: HashSet<String>,
    /// Steps keyed by OWNER then name (upstream `verify/verify.ts:64-89`): a
    /// subagent routinely runs the same graph shape as its parent, so both may
    /// legitimately have a step of the same name open at once.
    active_step_names: HashMap<Option<String>, HashSet<String>>,
    /// Ids of subagents currently open (upstream `activeSubagents`,
    /// `verify/verify.ts:90`). Presence = `SUBAGENT_STARTED` without a terminal yet.
    active_subagents: HashSet<String>,
    /// Ids closed by `SUBAGENT_FINISHED`/`SUBAGENT_ERROR` in this run
    /// (upstream `closedSubagents`, `verify/verify.ts:92-102`): a
    /// subagentRunId is a unique handle for ONE invocation, so the
    /// no-duplicate-start rule holds for the whole run, not just while active.
    closed_subagents: HashSet<String>,
    owners: Owners,
    run_errored: bool,
}

pub fn verify_events<S>(stream: S) -> impl Stream<Item = VerifyResult<Event>>
where
    S: Stream<Item = VerifyResult<Event>> + Send + 'static,
{
    stream! {
        let mut stream = stream.boxed();
        let mut state = VerifierState::default();

        while let Some(item) = stream.next().await {
            let event = match item {
                Ok(event) => event,
                Err(err) => {
                    yield Err(err);
                    return;
                }
            };

            if let Err(err) = state.validate_event(&event) {
                yield Err(err);
                return;
            }

            yield Ok(event);
        }
    }
}

impl VerifierState {
    /// Resets per-run state so a new `RUN_STARTED` can begin a fresh run after a
    /// previous `RUN_FINISHED` **or `RUN_ERROR`**. Mirrors `resetRunState()` in
    /// the TypeScript `verifyEvents` (`verify/verify.ts:106-121`), which clears
    /// `runError` too — a new run is not still dead.
    fn reset_run_state(&mut self) {
        self.run_finished = false;
        self.run_errored = false;
        self.run_started = true;
        self.clear_active_state();
    }

    /// Subagent attribution consistency: a continuation/close event must not
    /// disagree with the subagent that owns its entity (upstream
    /// `subagentTagError`, `verify/verify.ts:192-213`). An absent tag is always
    /// allowed — the field is optional, and attribution-only producers never
    /// send `SUBAGENT_*`, so the tag is deliberately not required to reference
    /// an "active" subagent. `owner` being present at all is what matters; its
    /// id being `None` is the parent agent, not "unknown".
    fn subagent_tag_error(
        event_type: &str,
        ev_subagent_run_id: Option<&str>,
        owner: Option<&Owner>,
        entity_kind: &str,
        entity_id: &str,
    ) -> VerifyResult<()> {
        let Some(ev_subagent_run_id) = ev_subagent_run_id else {
            return Ok(());
        };
        if let Some(owner) = owner {
            if owner.subagent_run_id.as_deref() != Some(ev_subagent_run_id) {
                return Err(AgUiError::validation(format!(
                    "Cannot send '{event_type}': subagentRunId '{ev_subagent_run_id}' does not match the {entity_kind} '{entity_id}' opener's subagent '{}'.",
                    owner
                        .subagent_run_id
                        .as_deref()
                        .unwrap_or("(the parent agent)")
                )));
            }
        }
        Ok(())
    }

    fn validate_event(&mut self, event: &Event) -> VerifyResult<()> {
        // RUN_ERROR is terminal until a new RUN_STARTED opens the next run: a
        // stream can carry more than one run (a replay of a stored thread being
        // the common case), and a run that errored is over rather than active
        // (upstream `verify/verify.ts:222-226`).
        if self.run_errored && !matches!(event, Event::RunStarted(_)) {
            return Err(AgUiError::validation(format!(
                "Cannot send event type '{}': The run has already errored with 'RUN_ERROR'. No further events can be sent.",
                event_name(event)
            )));
        }

        // After RUN_FINISHED only RUN_ERROR or a new RUN_STARTED may follow.
        if self.run_finished
            && !matches!(event, Event::RunError(_))
            && !matches!(event, Event::RunStarted(_))
        {
            return Err(AgUiError::validation(format!(
                "Cannot send event type '{}': The run has already finished with 'RUN_FINISHED'. Start a new run with 'RUN_STARTED'.",
                event_name(event)
            )));
        }

        // First-event requirement: must be RUN_STARTED or RUN_ERROR.
        if !self.first_event_received {
            self.first_event_received = true;
            if !matches!(event, Event::RunStarted(_) | Event::RunError(_)) {
                return Err(AgUiError::validation("First event must be 'RUN_STARTED'"));
            }
        } else if matches!(event, Event::RunStarted(_)) {
            // A RUN_STARTED mid-stream is only valid as the start of a new run
            // after the previous one finished — OR after it errored, which is
            // over rather than active (upstream
            // `verify/verify.ts:255-261`: `runStarted && !runFinished &&
            // !runError`).
            if self.run_started && !self.run_finished && !self.run_errored {
                return Err(AgUiError::validation(
                    "Cannot send 'RUN_STARTED' while a run is still active. The previous run must be finished with 'RUN_FINISHED' before starting a new run.",
                ));
            }
            if self.run_finished || self.run_errored {
                self.reset_run_state();
            }
        }

        match event {
            Event::RunStarted(raw) => {
                self.run_started = true;
                // The input echo carries replayed history the reducer applies,
                // so it seeds ownership non-authoritatively (upstream
                // `verify/verify.ts:922-936`).
                let messages = raw
                    .input
                    .as_ref()
                    .map(|input| input.messages.as_slice())
                    .unwrap_or(&[]);
                self.seed_owners_from_messages(messages, false)
            }
            Event::MessagesSnapshot(event) => {
                // Authoritative: the snapshot restates the conversation and the
                // reducer replaces each message, so its owners replace recorded
                // ones (upstream `verify/verify.ts:906-920`).
                self.seed_owners_from_messages(&event.messages, true)
            }
            Event::RunFinished(_) => self.finish_run(),
            Event::RunError(_) => {
                self.run_errored = true;
                self.clear_active_state();
                Ok(())
            }
            Event::TextMessageStart(event) => self.start_text_message(
                &event.message_id,
                event.attributable.subagent_run_id.as_deref(),
            ),
            Event::TextMessageContent(event) => self.text_message_content(
                &event.message_id,
                event.attributable.subagent_run_id.as_deref(),
            ),
            Event::TextMessageChunk(event) => self.text_message_chunk(event.message_id.as_deref()),
            Event::TextMessageEnd(event) => self.end_text_message(
                &event.message_id,
                event.attributable.subagent_run_id.as_deref(),
            ),
            Event::ToolCallStart(event) => self.start_tool_call(
                &event.tool_call_id,
                event.parent_message_id.as_deref(),
                event.attributable.subagent_run_id.as_deref(),
            ),
            Event::ToolCallArgs(event) => self.tool_call_args(
                &event.tool_call_id,
                event.attributable.subagent_run_id.as_deref(),
            ),
            Event::ToolCallChunk(event) => self.tool_call_chunk(
                event.tool_call_id.as_deref(),
                event.parent_message_id.as_deref(),
            ),
            Event::ToolCallEnd(event) => self.end_tool_call(
                &event.tool_call_id,
                event.attributable.subagent_run_id.as_deref(),
            ),
            Event::ReasoningStart(event) => self.start_reasoning(
                &event.message_id,
                event.attributable.subagent_run_id.as_deref(),
            ),
            Event::ReasoningMessageStart(event) => self.start_reasoning_message(
                &event.message_id,
                event.attributable.subagent_run_id.as_deref(),
            ),
            Event::ReasoningMessageContent(event) => self.continue_reasoning_message(
                &event.message_id,
                "REASONING_MESSAGE_CONTENT",
                event.attributable.subagent_run_id.as_deref(),
            ),
            Event::ReasoningMessageChunk(event) => {
                self.reasoning_chunk(event.message_id.as_deref())
            }
            Event::ReasoningMessageEnd(event) => self.end_reasoning_message(
                &event.message_id,
                event.attributable.subagent_run_id.as_deref(),
            ),
            Event::ReasoningEnd(event) => self.end_reasoning(
                &event.message_id,
                event.attributable.subagent_run_id.as_deref(),
            ),
            Event::StepStarted(event) => self.start_step(
                &event.step_name,
                event.attributable.subagent_run_id.as_deref(),
            ),
            Event::StepFinished(event) => self.finish_step(
                &event.step_name,
                event.attributable.subagent_run_id.as_deref(),
            ),
            // Subagent flow (upstream `verify/verify.ts:822-908`).
            Event::SubagentStarted(event) => self.subagent_started(event),
            Event::SubagentFinished(event) => {
                self.subagent_terminal(&event.subagent_run_id, "SUBAGENT_FINISHED")
            }
            Event::SubagentError(event) => {
                self.subagent_terminal(&event.subagent_run_id, "SUBAGENT_ERROR")
            }
            Event::ReasoningEncryptedValue(event) => self.reasoning_encrypted_value(
                event.subtype,
                &event.entity_id,
                event.attributable.subagent_run_id.as_deref(),
            ),
            Event::ToolCallResult(event) => {
                // A creation event: it mints the tool message the reducer
                // inserts, so the minted id must be on record. Recorded
                // unconditionally — the newest mint wins
                // (upstream `verify/verify.ts:674-687`).
                let owner = Owner {
                    subagent_run_id: event.attributable.subagent_run_id.clone(),
                };
                self.owners.message.insert(event.message_id.clone(), owner);
                Ok(())
            }
            Event::ActivitySnapshot(event) => {
                // Only a REPLACING snapshot re-mints the activity and so
                // re-owns it; `replace:false` leaves the tracked owner alone
                // (upstream `verify/verify.ts:656-672`). Absent `replace` is
                // true (the schema default).
                let known = self.owners.activity.contains_key(&event.message_id);
                if !known || event.replace != Some(false) {
                    let owner = Owner {
                        subagent_run_id: event.attributable.subagent_run_id.clone(),
                    };
                    self.owners.activity.insert(event.message_id.clone(), owner);
                }
                Ok(())
            }
            Event::ActivityDelta(event) => Self::subagent_tag_error(
                "ACTIVITY_DELTA",
                event.attributable.subagent_run_id.as_deref(),
                self.owners.activity.get(&event.message_id),
                "activity",
                &event.message_id,
            ),
            _ => Ok(()),
        }
    }

    fn finish_run(&mut self) -> VerifyResult<()> {
        // Check that all steps are finished before run ends
        // (upstream `verify/verify.ts:943-957`).
        let mut any_steps_active = false;
        for (owner, names) in &self.active_step_names {
            for name in names {
                any_steps_active = true;
                if owner.is_some() {
                    let _ = name; // owner reported inline below
                }
            }
        }
        if any_steps_active {
            let mut parts: Vec<String> = Vec::new();
            for (owner, names) in &self.active_step_names {
                for name in names {
                    parts.push(match owner {
                        Some(owner) => format!("{name} (subagent '{owner}')"),
                        None => name.clone(),
                    });
                }
            }
            parts.sort();
            return Err(AgUiError::validation(format!(
                "Cannot send 'RUN_FINISHED' while steps are still active: {}",
                parts.join(", ")
            )));
        }

        if !self.active_text_messages.is_empty() {
            let mut open = self
                .active_text_messages
                .iter()
                .cloned()
                .collect::<Vec<_>>();
            open.sort();
            return Err(AgUiError::validation(format!(
                "Cannot send 'RUN_FINISHED' while text messages are still active: {}",
                open.join(", ")
            )));
        }

        if !self.active_reasoning_messages.is_empty() {
            let mut open = self
                .active_reasoning_messages
                .iter()
                .cloned()
                .collect::<Vec<_>>();
            open.sort();
            return Err(AgUiError::validation(format!(
                "Cannot send 'RUN_FINISHED' while reasoning messages are still active: {}",
                open.join(", ")
            )));
        }

        if !self.active_reasoning_spans.is_empty() {
            let mut open = self
                .active_reasoning_spans
                .iter()
                .cloned()
                .collect::<Vec<_>>();
            open.sort();
            return Err(AgUiError::validation(format!(
                "Cannot send 'RUN_FINISHED' while reasoning spans are still active: {}",
                open.join(", ")
            )));
        }

        if !self.active_tool_calls.is_empty() {
            let mut open = self.active_tool_calls.keys().cloned().collect::<Vec<_>>();
            open.sort();
            return Err(AgUiError::validation(format!(
                "Cannot send 'RUN_FINISHED' while tool calls are still active: {}",
                open.join(", ")
            )));
        }

        // Check that all subagents are finished before run ends
        // (upstream `verify/verify.ts:1002-1011`).
        if !self.active_subagents.is_empty() {
            let mut open = self.active_subagents.iter().cloned().collect::<Vec<_>>();
            open.sort();
            return Err(AgUiError::validation(format!(
                "Cannot send 'RUN_FINISHED' while subagents are still active: {}",
                open.join(", ")
            )));
        }

        self.run_finished = true;
        Ok(())
    }

    fn start_text_message(&mut self, message_id: &str, tag: Option<&str>) -> VerifyResult<()> {
        // Membership check on the OPEN set only; the owner check below is
        // separate and reads the retained owner map (upstream
        // `verify/verify.ts:378-408`). The old slot model reported the OTHER
        // active id here; upstream reports the id that was SENT.
        if self.active_text_messages.contains(message_id) {
            return Err(AgUiError::validation(format!(
                "Cannot send 'TEXT_MESSAGE_START' event: A text message with ID '{message_id}' is already in progress. Complete it with 'TEXT_MESSAGE_END' first."
            )));
        }

        // First writer wins (upstream `verify/verify.ts:389-408`): the owner is
        // retained for the whole run, so a different producer reopening a
        // closed id under another subagent is a contradiction.
        let existing_owner = self.owners.message.get(message_id).cloned();
        if existing_owner.is_some() {
            Self::subagent_tag_error(
                "TEXT_MESSAGE_START",
                tag,
                existing_owner.as_ref(),
                "message",
                message_id,
            )?;
        } else {
            self.owners.message.insert(
                message_id.to_owned(),
                Owner {
                    subagent_run_id: tag.map(ToOwned::to_owned),
                },
            );
        }

        self.active_text_messages.insert(message_id.to_owned());
        Ok(())
    }

    fn text_message_content(&self, message_id: &str, tag: Option<&str>) -> VerifyResult<()> {
        if !self.active_text_messages.contains(message_id) {
            return Err(AgUiError::validation(format!(
                "Cannot send 'TEXT_MESSAGE_CONTENT' event: No active text message found with ID '{}'. Start a text message with 'TEXT_MESSAGE_START' first.",
                message_id
            )));
        }
        Self::subagent_tag_error(
            "TEXT_MESSAGE_CONTENT",
            tag,
            self.owners.message.get(message_id),
            "message",
            message_id,
        )
    }

    fn text_message_chunk(&self, message_id: Option<&str>) -> VerifyResult<()> {
        match message_id {
            Some(message_id) if self.active_text_messages.contains(message_id) => Ok(()),
            Some(_) => Err(AgUiError::validation(format!(
                "Cannot send 'TEXT_MESSAGE_CHUNK' event: No active text message found with ID '{}'. Start a text message with 'TEXT_MESSAGE_START' first.",
                message_id.unwrap_or("")
            ))),
            None => Err(AgUiError::validation(
                "Cannot send 'TEXT_MESSAGE_CHUNK' event: No active text message found. Start a text message with 'TEXT_MESSAGE_START' first.",
            )),
        }
    }

    fn end_text_message(&mut self, message_id: &str, tag: Option<&str>) -> VerifyResult<()> {
        if !self.active_text_messages.contains(message_id) {
            return Err(AgUiError::validation(format!(
                "Cannot send 'TEXT_MESSAGE_END' event: No active text message found with ID '{}'. A 'TEXT_MESSAGE_START' event must be sent first.",
                message_id
            )));
        }
        Self::subagent_tag_error(
            "TEXT_MESSAGE_END",
            tag,
            self.owners.message.get(message_id),
            "message",
            message_id,
        )?;
        self.active_text_messages.remove(message_id);
        Ok(())
    }

    fn start_tool_call(
        &mut self,
        tool_call_id: &str,
        parent_message_id: Option<&str>,
        tag: Option<&str>,
    ) -> VerifyResult<()> {
        if self.active_tool_calls.contains_key(tool_call_id) {
            return Err(AgUiError::validation(format!(
                "Cannot send 'TOOL_CALL_START' event: A tool call with ID '{tool_call_id}' is already in progress. Complete it with 'TOOL_CALL_END' first."
            )));
        }

        // Check parent BEFORE any state change: a rejected start must leave no
        // dirty entry behind.
        let parent_owner = parent_message_id.and_then(|m| self.owners.message.get(m).cloned());
        if let Some(parent_message_id) = parent_message_id {
            // The parent must name a message the verifier knows: an open one,
            // or a closed one whose owner the run still retains. Upstream only
            // consults the retained owner map here
            // (`verify/verify.ts:476-492`) — its own test nests a tool call in
            // an already-ended message
            // (`subagent-verify.test.ts:1614-1636`) — so requiring liveness
            // rejected conforming streams. An unknown id is still rejected to
            // keep the port's stricter base-suite behavior.
            let known = self.active_text_messages.contains(parent_message_id)
                || self.owners.message.contains_key(parent_message_id);
            if !known {
                return Err(match self.active_text_messages.iter().next() {
                    Some(active) => AgUiError::validation(format!(
                        "Cannot send 'TOOL_CALL_START' event: Parent message ID '{}' must match the active text message ID '{}'.",
                        parent_message_id, active
                    )),
                    None => AgUiError::validation(format!(
                        "Cannot send 'TOOL_CALL_START' event: Parent message ID '{}' does not reference an active text message. Start a text message with 'TEXT_MESSAGE_START' first.",
                        parent_message_id
                    )),
                });
            }
        }

        // A tool call lives INSIDE the assistant message `parentMessageId`
        // names and ToolCall carries no attribution of its own — so a call
        // whose explicit tag disagrees with that message's owner is rejected
        // rather than silently reattributed; an untagged call inherits the
        // parent message's owner (upstream `verify/verify.ts:468-492`).
        if let Some(parent_owner) = &parent_owner {
            if let Some(tag) = tag {
                if parent_owner.subagent_run_id.as_deref() != Some(tag) {
                    return Err(AgUiError::validation(format!(
                        "Cannot send 'TOOL_CALL_START': subagentRunId '{tag}' does not match its parent message '{}' owner '{}'. A tool call belongs to the message that carries it.",
                        parent_message_id.unwrap_or(""),
                        parent_owner
                            .subagent_run_id
                            .as_deref()
                            .unwrap_or("(the parent agent)")
                    )));
                }
            }
        }

        // First writer wins (upstream `verify/verify.ts:494-529`). An untagged
        // reopen's EFFECTIVE owner is the one it inherits from its parent
        // message, so the retained owner is also compared against that.
        let inherited_owner = Owner {
            subagent_run_id: match tag {
                Some(tag) => Some(tag.to_owned()),
                None => parent_owner.map(|o| o.subagent_run_id).unwrap_or_default(),
            },
        };
        let existing_owner = self.owners.tool_call.get(tool_call_id).cloned();
        if let Some(existing_owner) = &existing_owner {
            Self::subagent_tag_error(
                "TOOL_CALL_START",
                tag,
                Some(existing_owner),
                "tool call",
                tool_call_id,
            )?;
            if tag.is_none() && existing_owner.subagent_run_id != inherited_owner.subagent_run_id {
                return Err(AgUiError::validation(format!(
                    "Cannot send 'TOOL_CALL_START': tool call '{tool_call_id}' is owned by '{}' but its parent message '{}' is owned by '{}'. A tool call belongs to the message that carries it.",
                    existing_owner
                        .subagent_run_id
                        .as_deref()
                        .unwrap_or("(the parent agent)"),
                    parent_message_id.unwrap_or(""),
                    inherited_owner
                        .subagent_run_id
                        .as_deref()
                        .unwrap_or("(the parent agent)"),
                )));
            }
        }

        // All checks passed — now mutate.
        if existing_owner.is_none() {
            self.owners
                .tool_call
                .insert(tool_call_id.to_owned(), inherited_owner);
        }
        self.active_tool_calls.insert(
            tool_call_id.to_owned(),
            parent_message_id.map(ToOwned::to_owned),
        );
        Ok(())
    }

    fn tool_call_args(&self, tool_call_id: &str, tag: Option<&str>) -> VerifyResult<()> {
        if !self.active_tool_calls.contains_key(tool_call_id) {
            return Err(AgUiError::validation(format!(
                "Cannot send 'TOOL_CALL_ARGS' event: No active tool call found with ID '{}'. Start a tool call with 'TOOL_CALL_START' first.",
                tool_call_id
            )));
        }
        Self::subagent_tag_error(
            "TOOL_CALL_ARGS",
            tag,
            self.owners.tool_call.get(tool_call_id),
            "tool call",
            tool_call_id,
        )
    }

    fn tool_call_chunk(
        &self,
        tool_call_id: Option<&str>,
        parent_message_id: Option<&str>,
    ) -> VerifyResult<()> {
        let Some(tool_call_id) = tool_call_id else {
            return Err(AgUiError::validation(
                "Cannot send 'TOOL_CALL_CHUNK' event: No active tool call found. Start a tool call with 'TOOL_CALL_START' first.",
            ));
        };
        let Some(active_parent) = self.active_tool_calls.get(tool_call_id) else {
            return Err(AgUiError::validation(format!(
                "Cannot send 'TOOL_CALL_CHUNK' event: No active tool call found with ID '{}'. Start a tool call with 'TOOL_CALL_START' first.",
                tool_call_id
            )));
        };

        if let Some(parent_message_id) = parent_message_id {
            match active_parent {
                Some(active) if active == parent_message_id => {}
                Some(active) => {
                    return Err(AgUiError::validation(format!(
                        "Cannot send 'TOOL_CALL_CHUNK' event: Parent message ID '{}' must match the active tool call parent message ID '{}'.",
                        parent_message_id, active
                    )));
                }
                None => {
                    return Err(AgUiError::validation(format!(
                        "Cannot send 'TOOL_CALL_CHUNK' event: Parent message ID '{}' is not valid for the active tool call.",
                        parent_message_id
                    )));
                }
            }
        }

        Ok(())
    }

    fn end_tool_call(&mut self, tool_call_id: &str, tag: Option<&str>) -> VerifyResult<()> {
        if !self.active_tool_calls.contains_key(tool_call_id) {
            return Err(AgUiError::validation(format!(
                "Cannot send 'TOOL_CALL_END' event: No active tool call found with ID '{}'. A 'TOOL_CALL_START' event must be sent first.",
                tool_call_id
            )));
        }
        Self::subagent_tag_error(
            "TOOL_CALL_END",
            tag,
            self.owners.tool_call.get(tool_call_id),
            "tool call",
            tool_call_id,
        )?;
        self.active_tool_calls.remove(tool_call_id);
        Ok(())
    }

    /// Opens a reasoning SPAN. Checked against the opener's OWN set: a span
    /// and the message inside it may share an id, so one set for both would
    /// reject the canonical shape (upstream `verify/verify.ts:689-732`).
    fn start_reasoning(&mut self, message_id: &str, tag: Option<&str>) -> VerifyResult<()> {
        // Open-check AND owner check before any mutation: a rejected start
        // must leave no dirty entry behind.
        if self.active_reasoning_spans.contains(message_id) {
            return Err(AgUiError::validation(format!(
                "Cannot send 'REASONING_START' event: A reasoning span with ID '{message_id}' is already in progress. Complete it with 'REASONING_END' first."
            )));
        }
        self.record_reasoning_owner("REASONING_START", message_id, tag)?;
        self.active_reasoning_spans.insert(message_id.to_owned());
        Ok(())
    }

    /// Opens a reasoning MESSAGE. It requires no enclosing span — the span's
    /// identifier "namespaces nothing" — so only a duplicate open is rejected.
    fn start_reasoning_message(&mut self, message_id: &str, tag: Option<&str>) -> VerifyResult<()> {
        if self.active_reasoning_messages.contains(message_id) {
            return Err(AgUiError::validation(format!(
                "Cannot send 'REASONING_MESSAGE_START' event: A reasoning message with ID '{message_id}' is already in progress. Complete it with 'REASONING_MESSAGE_END' first."
            )));
        }
        self.record_reasoning_owner("REASONING_MESSAGE_START", message_id, tag)?;
        self.active_reasoning_messages.insert(message_id.to_owned());
        Ok(())
    }

    /// First writer records the owner; the owner outlives the close, so a
    /// later `REASONING_ENCRYPTED_VALUE` or an untagged reopen still has one
    /// to check against (upstream `verify/verify.ts:710-730`). A second opener
    /// that DISAGREES is rejected, not silently ignored
    /// (upstream `verify/verify.ts:717-723`).
    fn record_reasoning_owner(
        &mut self,
        event_type: &str,
        message_id: &str,
        tag: Option<&str>,
    ) -> VerifyResult<()> {
        let existing_owner = self.owners.reasoning.get(message_id).cloned();
        if let Some(existing_owner) = &existing_owner {
            Self::subagent_tag_error(
                event_type,
                tag,
                Some(existing_owner),
                "reasoning message",
                message_id,
            )?;
        } else {
            self.owners.reasoning.insert(
                message_id.to_owned(),
                Owner {
                    subagent_run_id: tag.map(ToOwned::to_owned),
                },
            );
        }
        Ok(())
    }

    /// A continuation must name something that is open. Content does not close;
    /// only the matching `*_END` drops the OPEN flag.
    fn continue_reasoning_message(
        &self,
        message_id: &str,
        event_name: &str,
        tag: Option<&str>,
    ) -> VerifyResult<()> {
        if self.active_reasoning_messages.contains(message_id) {
            return Self::subagent_tag_error(
                event_name,
                tag,
                self.owners.reasoning.get(message_id),
                "reasoning message",
                message_id,
            );
        }
        Err(AgUiError::validation(format!(
            "Cannot send '{event_name}' event: No active reasoning message found with ID '{message_id}'. Start a reasoning message with 'REASONING_MESSAGE_START' first."
        )))
    }

    fn end_reasoning_message(&mut self, message_id: &str, tag: Option<&str>) -> VerifyResult<()> {
        if !self.active_reasoning_messages.contains(message_id) {
            return Err(AgUiError::validation(format!(
                "Cannot send 'REASONING_MESSAGE_END' event: No active reasoning message found with ID '{message_id}'. A 'REASONING_MESSAGE_START' event must be sent first."
            )));
        }
        Self::subagent_tag_error(
            "REASONING_MESSAGE_END",
            tag,
            self.owners.reasoning.get(message_id),
            "reasoning message",
            message_id,
        )?;
        self.active_reasoning_messages.remove(message_id);
        Ok(())
    }

    /// Closes a reasoning SPAN, independently of any message it bracketed: a
    /// span may end while its message is still open, and the message's own
    /// `REASONING_MESSAGE_END` remains owed (upstream `verify/verify.ts:733-771`).
    fn end_reasoning(&mut self, message_id: &str, tag: Option<&str>) -> VerifyResult<()> {
        if !self.active_reasoning_spans.contains(message_id) {
            return Err(AgUiError::validation(format!(
                "Cannot send 'REASONING_END' event: No active reasoning span found with ID '{message_id}'. A 'REASONING_START' event must be sent first."
            )));
        }
        Self::subagent_tag_error(
            "REASONING_END",
            tag,
            self.owners.reasoning.get(message_id),
            "reasoning message",
            message_id,
        )?;
        self.active_reasoning_spans.remove(message_id);
        Ok(())
    }

    /// A chunk is a continuation like content, so it must name an open message.
    ///
    /// ponytail: upstream has no `REASONING_MESSAGE_CHUNK` case at all
    /// (`verify/verify.ts` switches on CONTENT/END, never CHUNK) — chunks are
    /// expanded upstream of verification, so it is unobservable there. We keep
    /// the check, rebased on the message set.
    fn reasoning_chunk(&self, message_id: Option<&str>) -> VerifyResult<()> {
        match message_id {
            Some(message_id) => {
                self.continue_reasoning_message(message_id, "REASONING_MESSAGE_CHUNK", None)
            }
            None => Err(AgUiError::validation(
                "Cannot send 'REASONING_MESSAGE_CHUNK' event: No active reasoning message found. Start a reasoning message with 'REASONING_MESSAGE_START' first.",
            )),
        }
    }

    /// Owner rule for `REASONING_ENCRYPTED_VALUE` (upstream
    /// `verify/verify.ts:773-807`): `subtype` says which entity kind the
    /// `entityId` names, and the owner is looked up in that kind's bucket —
    /// `owners.toolCall` for "tool-call", BOTH the message and reasoning
    /// buckets for "message" (ids are unique per kind, so consulting both
    /// cannot pick a wrong owner), reasoning as the fallback.
    fn reasoning_encrypted_value(
        &self,
        subtype: ReasoningEncryptedValueSubtype,
        entity_id: &str,
        tag: Option<&str>,
    ) -> VerifyResult<()> {
        let (kind_label, owner) = match subtype {
            ReasoningEncryptedValueSubtype::ToolCall => {
                ("tool call", self.owners.tool_call.get(entity_id))
            }
            ReasoningEncryptedValueSubtype::Message => (
                "message",
                self.owners
                    .message
                    .get(entity_id)
                    .or_else(|| self.owners.reasoning.get(entity_id)),
            ),
        };
        Self::subagent_tag_error(
            "REASONING_ENCRYPTED_VALUE",
            tag,
            owner,
            kind_label,
            entity_id,
        )
    }

    fn start_step(&mut self, step_name: &str, owner: Option<&str>) -> VerifyResult<()> {
        let steps = self
            .active_step_names
            .entry(owner.map(ToOwned::to_owned))
            .or_default();
        if !steps.insert(step_name.to_owned()) {
            // Upstream appends ` in subagent '<id>'` to the duplicate-step
            // message when the opener was tagged
            // (`verify/verify.ts:576-591`).
            return Err(AgUiError::validation(format!(
                "Step \"{}\" is already active for 'STEP_STARTED'{}",
                step_name,
                owner.map_or(String::new(), |owner| format!(" in subagent '{owner}'")),
            )));
        }

        Ok(())
    }

    /// Step ownership: a step must be finished by whoever started it
    /// (upstream `verify/verify.ts:593-636`). When another owner DOES hold a
    /// step of this name, that is reported separately from "was not started" —
    /// it is the much likelier mistake.
    fn finish_step(&mut self, step_name: &str, owner: Option<&str>) -> VerifyResult<()> {
        let owned_here = self
            .active_step_names
            .get_mut(&owner.map(ToOwned::to_owned))
            .map(|steps| steps.remove(step_name))
            .unwrap_or(false);
        if owned_here {
            return Ok(());
        }

        let this_owner = owner.map(ToOwned::to_owned);
        let mut other_owner: Option<Option<String>> = None;
        'outer: for (candidate_owner, names) in &self.active_step_names {
            if *candidate_owner != this_owner && names.contains(step_name) {
                other_owner = Some(candidate_owner.clone());
                break 'outer;
            }
        }
        if let Some(other_owner) = other_owner {
            let attributed_to = match &this_owner {
                Some(owner) => format!("subagent '{owner}'"),
                None => "the parent agent".to_string(),
            };
            let open_under = match other_owner.as_deref() {
                Some(owner) => format!("subagent '{owner}'"),
                None => "the parent agent".to_string(),
            };
            return Err(AgUiError::validation(format!(
                "Cannot send 'STEP_FINISHED' for step \"{step_name}\" attributed to {attributed_to}: that step is open under {open_under}. A step must be finished by whoever started it."
            )));
        }

        Err(AgUiError::validation(format!(
            "Cannot send 'STEP_FINISHED' for step \"{step_name}\" that was not started"
        )))
    }

    /// Subagent lifecycle (upstream `verify/verify.ts:822-908`). `subagentRunId`
    /// is the subagent's own identity here, not the optional attribution tag
    /// other events carry.
    fn subagent_started(&mut self, event: &SubagentStartedEvent) -> VerifyResult<()> {
        let subagent_run_id = event.subagent_run_id.as_str();
        if self.active_subagents.contains(subagent_run_id) {
            return Err(AgUiError::validation(format!(
                "Cannot send 'SUBAGENT_STARTED': subagent '{subagent_run_id}' is already active. Finish it with 'SUBAGENT_FINISHED' first."
            )));
        }

        // Reopening a closed id would give one invocation two starts and two
        // terminals; ids are per-invocation, so reuse within a run is a
        // producer bug (upstream `verify/verify.ts:853-863`).
        if self.closed_subagents.contains(subagent_run_id) {
            return Err(AgUiError::validation(format!(
                "Cannot send 'SUBAGENT_STARTED': subagent '{subagent_run_id}' has already finished in this run. Subagent IDs are per-invocation and cannot be reused."
            )));
        }

        // The parent must have been STARTED — not still be active
        // (upstream `verify/verify.ts:864-875`).
        if let Some(parent) = event.parent_subagent_run_id.as_deref() {
            if !self.active_subagents.contains(parent) && !self.closed_subagents.contains(parent) {
                return Err(AgUiError::validation(format!(
                    "Cannot send 'SUBAGENT_STARTED': parentSubagentRunId '{parent}' has not been started in this run."
                )));
            }
        }

        self.active_subagents.insert(subagent_run_id.to_owned());
        Ok(())
    }

    fn subagent_terminal(&mut self, subagent_run_id: &str, event_type: &str) -> VerifyResult<()> {
        if !self.active_subagents.remove(subagent_run_id) {
            return Err(AgUiError::validation(format!(
                "Cannot send '{event_type}': no active subagent found with ID '{subagent_run_id}'. A 'SUBAGENT_STARTED' event must be sent first."
            )));
        }
        self.closed_subagents.insert(subagent_run_id.to_owned());
        Ok(())
    }

    fn clear_active_state(&mut self) {
        self.active_text_messages.clear();
        self.active_tool_calls.clear();
        self.active_reasoning_spans.clear();
        self.active_reasoning_messages.clear();
        self.active_step_names.clear();
        // Per-run maps: subagent ids are per-invocation handles and owners are
        // run-scoped, so both reset with everything else
        // (upstream `resetRunState`, `verify/verify.ts:106-121`).
        self.active_subagents.clear();
        self.closed_subagents.clear();
        self.owners = Owners::default();
    }

    /// Ownership seeded from replayed history: `MESSAGES_SNAPSHOT` and the
    /// `RUN_STARTED` input echo both put messages on the wire that later events
    /// can reference, so their owners (absent = the parent agent) go on record
    /// like an opener's would — and each message's tool calls inherit the
    /// message's owner, since a ToolCall carries no owner field of its own
    /// (upstream `seedOwnersFromMessages`, `verify/verify.ts:143-184`).
    ///
    /// `authoritative` distinguishes the two sources: a snapshot restates the
    /// whole conversation and the reducer REPLACES the message, so its owner
    /// replaces the recorded one (`verify/verify.ts:906-920`); the `RUN_STARTED`
    /// input echo is plain history and seeds only ids nothing else has claimed
    /// (`verify/verify.ts:922-936`).
    fn seed_owners_from_messages(
        &mut self,
        messages: &[Message],
        authoritative: bool,
    ) -> VerifyResult<()> {
        for message in messages {
            // Owners are per entity KIND, so the message must seed the bucket
            // its role streams through (upstream `verify/verify.ts:164-170`).
            let subagent_run_id = match message {
                Message::Reasoning(m) => {
                    let owner = Owner {
                        subagent_run_id: m.subagent_run_id.clone(),
                    };
                    if authoritative || !self.owners.reasoning.contains_key(&m.id) {
                        self.owners.reasoning.insert(m.id.clone(), owner);
                    }
                    m.subagent_run_id.clone()
                }
                Message::Activity(m) => {
                    let owner = Owner {
                        subagent_run_id: m.subagent_run_id.clone(),
                    };
                    if authoritative || !self.owners.activity.contains_key(&m.id) {
                        self.owners.activity.insert(m.id.clone(), owner);
                    }
                    m.subagent_run_id.clone()
                }
                _ => {
                    let subagent_run_id = match message {
                        Message::Developer(m) => m.subagent_run_id.clone(),
                        Message::System(m) => m.subagent_run_id.clone(),
                        Message::Assistant(m) => m.subagent_run_id.clone(),
                        Message::User(m) => m.subagent_run_id.clone(),
                        Message::Tool(m) => m.subagent_run_id.clone(),
                        Message::Reasoning(_) | Message::Activity(_) => unreachable!(),
                    };
                    let id = message.id().to_owned();
                    if authoritative || !self.owners.message.contains_key(&id) {
                        self.owners.message.insert(
                            id,
                            Owner {
                                subagent_run_id: subagent_run_id.clone(),
                            },
                        );
                    }
                    subagent_run_id
                }
            };

            // toolCalls inherit their message's owner (upstream
            // `verify/verify.ts:172-176`).
            if let Message::Assistant(assistant) = message {
                if let Some(tool_calls) = &assistant.tool_calls {
                    for tool_call in tool_calls {
                        if authoritative || !self.owners.tool_call.contains_key(&tool_call.id) {
                            self.owners.tool_call.insert(
                                tool_call.id.clone(),
                                Owner {
                                    subagent_run_id: subagent_run_id.clone(),
                                },
                            );
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

fn event_name(event: &Event) -> &'static str {
    match event {
        Event::TextMessageStart(_) => "TEXT_MESSAGE_START",
        Event::TextMessageContent(_) => "TEXT_MESSAGE_CONTENT",
        Event::TextMessageEnd(_) => "TEXT_MESSAGE_END",
        Event::TextMessageChunk(_) => "TEXT_MESSAGE_CHUNK",
        Event::ToolCallStart(_) => "TOOL_CALL_START",
        Event::ToolCallArgs(_) => "TOOL_CALL_ARGS",
        Event::ToolCallEnd(_) => "TOOL_CALL_END",
        Event::ToolCallChunk(_) => "TOOL_CALL_CHUNK",
        Event::ToolCallResult(_) => "TOOL_CALL_RESULT",
        Event::StateSnapshot(_) => "STATE_SNAPSHOT",
        Event::StateDelta(_) => "STATE_DELTA",
        Event::MessagesSnapshot(_) => "MESSAGES_SNAPSHOT",
        Event::ActivitySnapshot(_) => "ACTIVITY_SNAPSHOT",
        Event::ActivityDelta(_) => "ACTIVITY_DELTA",
        Event::Raw(_) => "RAW",
        Event::Custom(_) => "CUSTOM",
        Event::RunStarted(_) => "RUN_STARTED",
        Event::RunFinished(_) => "RUN_FINISHED",
        Event::RunError(_) => "RUN_ERROR",
        Event::StepStarted(_) => "STEP_STARTED",
        Event::StepFinished(_) => "STEP_FINISHED",
        Event::ReasoningStart(_) => "REASONING_START",
        Event::ReasoningMessageStart(_) => "REASONING_MESSAGE_START",
        Event::ReasoningMessageContent(_) => "REASONING_MESSAGE_CONTENT",
        Event::ReasoningMessageEnd(_) => "REASONING_MESSAGE_END",
        Event::ReasoningMessageChunk(_) => "REASONING_MESSAGE_CHUNK",
        Event::ReasoningEnd(_) => "REASONING_END",
        Event::ReasoningEncryptedValue(_) => "REASONING_ENCRYPTED_VALUE",
        Event::SubagentStarted(_) => "SUBAGENT_STARTED",
        Event::SubagentFinished(_) => "SUBAGENT_FINISHED",
        Event::SubagentError(_) => "SUBAGENT_ERROR",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agui_rs_core::AttributableFields;
    use agui_rs_core::{
        factory, BaseEventFields, Event, ReasoningEncryptedValueEvent, ReasoningEndEvent,
        ReasoningMessageChunkEvent, ReasoningMessageContentEvent, ReasoningMessageEndEvent,
        ReasoningMessageRole, ReasoningMessageStartEvent, ReasoningStartEvent, StateSnapshotEvent,
        StepFinishedEvent, StepStartedEvent, SubagentErrorEvent, SubagentFinishedEvent,
        SubagentStartedEvent, TextMessageContentEvent, TextMessageEndEvent, TextMessageRole,
        TextMessageStartEvent, ToolCallArgsEvent, ToolCallChunkEvent, ToolCallStartEvent,
    };
    use futures::stream;

    async fn collect(events: Vec<Event>) -> Vec<VerifyResult<Event>> {
        verify_events(stream::iter(events.into_iter().map(Ok)))
            .collect::<Vec<_>>()
            .await
    }

    fn assert_validation(result: &VerifyResult<Event>, expected: &str) {
        match result {
            Err(AgUiError::Validation(message)) => assert!(
                message.contains(expected),
                "expected '{expected}' in '{message}'"
            ),
            other => panic!("expected validation error, got {other:?}"),
        }
    }

    fn tool_call_start_with_parent(tool_call_id: &str, parent_message_id: &str) -> Event {
        Event::ToolCallStart(ToolCallStartEvent {
            tool_call_id: tool_call_id.into(),
            tool_call_name: "search".into(),
            parent_message_id: Some(parent_message_id.into()),
            base: BaseEventFields::default(),
            attributable: AttributableFields::default(),
        })
    }

    fn tool_call_chunk(tool_call_id: Option<&str>, parent_message_id: Option<&str>) -> Event {
        Event::ToolCallChunk(ToolCallChunkEvent {
            tool_call_id: tool_call_id.map(str::to_owned),
            tool_call_name: None,
            parent_message_id: parent_message_id.map(str::to_owned),
            delta: Some("{}".into()),
            base: BaseEventFields::default(),
            attributable: AttributableFields::default(),
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

    fn reasoning_message_content(message_id: &str, delta: &str) -> Event {
        Event::ReasoningMessageContent(ReasoningMessageContentEvent {
            message_id: message_id.into(),
            delta: delta.into(),
            base: BaseEventFields::default(),
            attributable: AttributableFields::default(),
        })
    }

    fn reasoning_message_chunk(message_id: Option<&str>, delta: &str) -> Event {
        Event::ReasoningMessageChunk(ReasoningMessageChunkEvent {
            message_id: message_id.map(str::to_owned),
            delta: Some(delta.into()),
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

    mod run_lifecycle {
        use super::*;

        #[tokio::test]
        async fn rejects_non_run_started_first_event() {
            let items = collect(vec![factory::text_message_start("m1")]).await;
            assert_validation(&items[0], "First event must be 'RUN_STARTED'");
        }

        #[tokio::test]
        async fn rejects_second_run_started_during_active_run() {
            let items = collect(vec![
                factory::run_started("thread", "run"),
                factory::run_started("thread", "run-2"),
            ])
            .await;
            assert!(items[0].is_ok());
            assert_validation(
                &items[1],
                "Cannot send 'RUN_STARTED' while a run is still active",
            );
        }

        #[tokio::test]
        async fn rejects_events_after_run_finished() {
            let items = collect(vec![
                factory::run_started("thread", "run"),
                factory::run_finished("thread", "run"),
                factory::text_message_start("m1"),
            ])
            .await;
            assert_validation(
                &items[2],
                "The run has already finished with 'RUN_FINISHED'",
            );
        }

        #[tokio::test]
        async fn rejects_events_after_run_error() {
            let items = collect(vec![
                factory::run_started("thread", "run"),
                factory::run_error("boom"),
                factory::text_message_start("m1"),
            ])
            .await;
            assert_validation(&items[2], "The run has already errored with 'RUN_ERROR'");
        }

        #[tokio::test]
        async fn accepts_valid_run_lifecycle() {
            let items = collect(vec![
                factory::run_started("thread", "run"),
                factory::text_message_start("m1"),
                factory::text_message_end("m1"),
                factory::run_finished("thread", "run"),
            ])
            .await;
            assert!(items.iter().all(VerifyResult::is_ok));
        }
    }

    mod text_messages {
        use super::*;

        #[tokio::test]
        async fn rejects_content_before_start() {
            let items = collect(vec![
                factory::run_started("thread", "run"),
                factory::text_message_content("m1", "hello"),
            ])
            .await;
            assert_validation(&items[1], "Cannot send 'TEXT_MESSAGE_CONTENT' event: No active text message found with ID 'm1'");
        }

        #[tokio::test]
        async fn rejects_end_before_start() {
            let items = collect(vec![
                factory::run_started("thread", "run"),
                factory::text_message_end("m1"),
            ])
            .await;
            assert_validation(
                &items[1],
                "Cannot send 'TEXT_MESSAGE_END' event: No active text message found with ID 'm1'",
            );
        }

        #[tokio::test]
        async fn allows_distinct_concurrent_text_messages() {
            // Upstream `verify/__tests__/verify.concurrent.test.ts:23-96`:
            // two ids open at once and END out of order all pass.
            let items = collect(vec![
                factory::run_started("thread", "run"),
                factory::text_message_start("m1"),
                factory::text_message_start("m2"),
                factory::text_message_content("m1", "a"),
                factory::text_message_content("m2", "b"),
                factory::text_message_end("m2"),
                factory::text_message_end("m1"),
                factory::run_finished("thread", "run"),
            ])
            .await;
            assert!(items.iter().all(VerifyResult::is_ok));
        }

        #[tokio::test]
        async fn rejects_duplicate_text_message_start_while_active() {
            let items = collect(vec![
                factory::run_started("thread", "run"),
                factory::text_message_start("m1"),
                factory::text_message_start("m1"),
            ])
            .await;
            assert_validation(
                &items[2],
                "A text message with ID 'm1' is already in progress",
            );
        }

        #[tokio::test]
        async fn rejects_mismatched_content_id() {
            let items = collect(vec![
                factory::run_started("thread", "run"),
                factory::text_message_start("m1"),
                factory::text_message_content("m2", "hello"),
            ])
            .await;
            assert_validation(&items[2], "No active text message found with ID 'm2'");
        }

        #[tokio::test]
        async fn allows_valid_text_message_sequence() {
            let items = collect(vec![
                factory::run_started("thread", "run"),
                factory::text_message_start("m1"),
                factory::text_message_content("m1", "hello"),
                factory::text_message_content("m1", " world"),
                factory::text_message_end("m1"),
                factory::run_finished("thread", "run"),
            ])
            .await;
            assert!(items.iter().all(VerifyResult::is_ok));
        }
    }

    mod tool_calls {
        use super::*;

        #[tokio::test]
        async fn rejects_args_before_start() {
            let items = collect(vec![
                factory::run_started("thread", "run"),
                factory::tool_call_args("tc1", "{}"),
            ])
            .await;
            assert_validation(
                &items[1],
                "Cannot send 'TOOL_CALL_ARGS' event: No active tool call found with ID 'tc1'",
            );
        }

        #[tokio::test]
        async fn rejects_end_before_start() {
            let items = collect(vec![
                factory::run_started("thread", "run"),
                factory::tool_call_end("tc1"),
            ])
            .await;
            assert_validation(
                &items[1],
                "Cannot send 'TOOL_CALL_END' event: No active tool call found with ID 'tc1'",
            );
        }

        #[tokio::test]
        async fn rejects_tool_call_start_with_non_active_parent_message() {
            let items = collect(vec![
                factory::run_started("thread", "run"),
                tool_call_start_with_parent("tc1", "m1"),
            ])
            .await;
            assert_validation(
                &items[1],
                "Parent message ID 'm1' does not reference an active text message",
            );
        }

        #[tokio::test]
        async fn allows_distinct_concurrent_tool_calls() {
            // Upstream `verify/__tests__/verify.concurrent.test.ts:99-170`.
            let items = collect(vec![
                factory::run_started("thread", "run"),
                factory::tool_call_start("tc1", "search"),
                factory::tool_call_start("tc2", "calculate"),
                factory::tool_call_args("tc1", "{}"),
                factory::tool_call_args("tc2", "{}"),
                factory::tool_call_end("tc2"),
                factory::tool_call_end("tc1"),
                factory::run_finished("thread", "run"),
            ])
            .await;
            assert!(items.iter().all(VerifyResult::is_ok));
        }

        #[tokio::test]
        async fn rejects_duplicate_tool_call_start_while_active() {
            let items = collect(vec![
                factory::run_started("thread", "run"),
                factory::tool_call_start("tc1", "search"),
                factory::tool_call_start("tc1", "search"),
            ])
            .await;
            assert_validation(
                &items[2],
                "A tool call with ID 'tc1' is already in progress",
            );
        }

        #[tokio::test]
        async fn allows_tool_call_sequence_with_parent_message() {
            let items = collect(vec![
                factory::run_started("thread", "run"),
                factory::text_message_start("m1"),
                tool_call_start_with_parent("tc1", "m1"),
                factory::tool_call_args("tc1", "{"),
                tool_call_chunk(Some("tc1"), Some("m1")),
                factory::tool_call_end("tc1"),
                factory::text_message_end("m1"),
                factory::run_finished("thread", "run"),
            ])
            .await;
            assert!(items.iter().all(VerifyResult::is_ok));
        }
    }

    mod reasoning {
        use super::*;

        #[tokio::test]
        async fn allows_reasoning_message_start_without_an_enclosing_span() {
            // The span's identifier "namespaces nothing" and the messages inside
            // carry their own ids, so a message opener requires no span.
            let items = collect(vec![
                factory::run_started("thread", "run"),
                reasoning_message_start("r1"),
                reasoning_message_content("r1", "thinking"),
                reasoning_message_end("r1"),
                factory::run_finished("thread", "run"),
            ])
            .await;
            assert!(items.iter().all(VerifyResult::is_ok));
        }

        #[tokio::test]
        async fn rejects_a_second_reasoning_message_start_with_the_same_id() {
            let items = collect(vec![
                factory::run_started("thread", "run"),
                reasoning_message_start("r1"),
                reasoning_message_start("r1"),
            ])
            .await;
            assert_validation(
                &items[2],
                "A reasoning message with ID 'r1' is already in progress. Complete it with 'REASONING_MESSAGE_END' first.",
            );
        }

        #[tokio::test]
        async fn rejects_a_second_reasoning_start_with_the_same_id() {
            let items = collect(vec![
                factory::run_started("thread", "run"),
                reasoning_start("r1"),
                reasoning_start("r1"),
            ])
            .await;
            assert_validation(
                &items[2],
                "A reasoning span with ID 'r1' is already in progress. Complete it with 'REASONING_END' first.",
            );
        }

        #[tokio::test]
        async fn rejects_reasoning_content_before_reasoning_message_start() {
            let items = collect(vec![
                factory::run_started("thread", "run"),
                reasoning_start("r1"),
                reasoning_message_content("r1", "thinking"),
            ])
            .await;
            assert_validation(&items[2], "No active reasoning message found with ID 'r1'");
        }

        #[tokio::test]
        async fn rejects_reasoning_message_end_with_mismatched_id() {
            let items = collect(vec![
                factory::run_started("thread", "run"),
                reasoning_start("r1"),
                reasoning_message_start("r1"),
                reasoning_message_end("r2"),
            ])
            .await;
            assert_validation(&items[3], "No active reasoning message found with ID 'r2'");
        }

        #[tokio::test]
        async fn closes_only_its_own_entity() {
            // REASONING_END drops the SPAN's open flag and nothing else; the
            // message's own REASONING_MESSAGE_END is still owed.
            let items = collect(vec![
                factory::run_started("thread", "run"),
                reasoning_start("span"),
                reasoning_message_start("msg"),
                reasoning_end("span"),
                reasoning_message_end("msg"),
                factory::run_finished("thread", "run"),
            ])
            .await;
            assert!(items.iter().all(VerifyResult::is_ok));
        }

        #[tokio::test]
        async fn rejects_reasoning_end_for_a_span_nothing_opened() {
            let items = collect(vec![
                factory::run_started("thread", "run"),
                reasoning_end("r1"),
            ])
            .await;
            assert_validation(
                &items[1],
                "Cannot send 'REASONING_END' event: No active reasoning span found with ID 'r1'.",
            );
        }

        #[tokio::test]
        async fn lets_one_id_name_both_a_span_and_its_message() {
            let items = collect(vec![
                factory::run_started("thread", "run"),
                reasoning_start("r1"),
                reasoning_message_start("r1"),
                reasoning_message_content("r1", "a"),
                reasoning_message_end("r1"),
                reasoning_end("r1"),
                factory::run_finished("thread", "run"),
            ])
            .await;
            assert!(items.iter().all(VerifyResult::is_ok));
        }

        #[tokio::test]
        async fn allows_complete_reasoning_sequence() {
            let items = collect(vec![
                factory::run_started("thread", "run"),
                reasoning_start("r1"),
                reasoning_message_start("r1"),
                reasoning_message_content("r1", "a"),
                reasoning_message_chunk(Some("r1"), "b"),
                reasoning_message_end("r1"),
                reasoning_end("r1"),
                factory::run_finished("thread", "run"),
            ])
            .await;
            assert!(items.iter().all(VerifyResult::is_ok));
        }
    }

    mod steps {
        use super::*;

        #[tokio::test]
        async fn rejects_duplicate_step_start() {
            let items = collect(vec![
                factory::run_started("thread", "run"),
                factory::step_started("plan"),
                factory::step_started("plan"),
            ])
            .await;
            assert_validation(
                &items[2],
                "Step \"plan\" is already active for 'STEP_STARTED'",
            );
        }

        #[tokio::test]
        async fn rejects_step_finish_without_start() {
            let items = collect(vec![
                factory::run_started("thread", "run"),
                factory::step_finished("plan"),
            ])
            .await;
            assert_validation(
                &items[1],
                "Cannot send 'STEP_FINISHED' for step \"plan\" that was not started",
            );
        }

        #[tokio::test]
        async fn rejects_run_finished_with_active_step() {
            let items = collect(vec![
                factory::run_started("thread", "run"),
                factory::step_started("plan"),
                factory::run_finished("thread", "run"),
            ])
            .await;
            assert_validation(
                &items[2],
                "Cannot send 'RUN_FINISHED' while steps are still active: plan",
            );
        }

        #[tokio::test]
        async fn allows_multiple_distinct_steps() {
            let items = collect(vec![
                factory::run_started("thread", "run"),
                factory::step_started("plan"),
                factory::step_started("execute"),
                factory::step_finished("execute"),
                factory::step_finished("plan"),
                factory::run_finished("thread", "run"),
            ])
            .await;
            assert!(items.iter().all(VerifyResult::is_ok));
        }

        #[tokio::test]
        async fn allows_reusing_step_name_after_finish() {
            let items = collect(vec![
                factory::run_started("thread", "run"),
                factory::step_started("plan"),
                factory::step_finished("plan"),
                factory::step_started("plan"),
                factory::step_finished("plan"),
                factory::run_finished("thread", "run"),
            ])
            .await;
            assert!(items.iter().all(VerifyResult::is_ok));
        }
    }

    mod sequencing {
        use super::*;

        #[tokio::test]
        async fn rejects_run_finished_with_active_text_message() {
            let items = collect(vec![
                factory::run_started("thread", "run"),
                factory::text_message_start("m1"),
                factory::run_finished("thread", "run"),
            ])
            .await;
            assert_validation(
                &items[2],
                "Cannot send 'RUN_FINISHED' while text messages are still active: m1",
            );
        }

        #[tokio::test]
        async fn rejects_run_finished_with_active_tool_call() {
            let items = collect(vec![
                factory::run_started("thread", "run"),
                factory::tool_call_start("tc1", "search"),
                factory::run_finished("thread", "run"),
            ])
            .await;
            assert_validation(
                &items[2],
                "Cannot send 'RUN_FINISHED' while tool calls are still active: tc1",
            );
        }

        #[tokio::test]
        async fn rejects_run_finished_with_active_reasoning_span() {
            let items = collect(vec![
                factory::run_started("thread", "run"),
                reasoning_start("r1"),
                factory::run_finished("thread", "run"),
            ])
            .await;
            assert_validation(
                &items[2],
                "Cannot send 'RUN_FINISHED' while reasoning spans are still active: r1",
            );
        }

        #[tokio::test]
        async fn rejects_run_finished_with_active_reasoning_message() {
            let items = collect(vec![
                factory::run_started("thread", "run"),
                reasoning_start("r1"),
                reasoning_message_start("r1"),
                reasoning_end("r1"),
                factory::run_finished("thread", "run"),
            ])
            .await;
            // The span closed; the message inside it did not, and the two are
            // tracked separately.
            assert_validation(
                &items[4],
                "Cannot send 'RUN_FINISHED' while reasoning messages are still active: r1",
            );
        }

        #[tokio::test]
        async fn rejects_mismatched_tool_call_chunk_id() {
            let items = collect(vec![
                factory::run_started("thread", "run"),
                factory::tool_call_start("tc1", "search"),
                tool_call_chunk(Some("tc2"), None),
            ])
            .await;
            assert_validation(
                &items[2],
                "Cannot send 'TOOL_CALL_CHUNK' event: No active tool call found with ID 'tc2'",
            );
        }

        #[tokio::test]
        async fn allows_mixed_full_sequence() {
            let items = collect(vec![
                factory::run_started("thread", "run"),
                factory::step_started("plan"),
                factory::text_message_start("m1"),
                factory::text_message_content("m1", "Preparing"),
                tool_call_start_with_parent("tc1", "m1"),
                factory::tool_call_args("tc1", "{}"),
                factory::tool_call_end("tc1"),
                factory::text_message_end("m1"),
                reasoning_start("r1"),
                reasoning_message_start("r1"),
                reasoning_message_content("r1", "thought"),
                reasoning_message_end("r1"),
                reasoning_end("r1"),
                factory::step_finished("plan"),
                factory::run_finished("thread", "run"),
            ])
            .await;
            assert_eq!(items.len(), 15);
            assert!(items.iter().all(VerifyResult::is_ok));
        }
    }

    /// Subagent lifecycle and attribution discipline, ported from the
    /// upstream `subagent-verify.test.ts` suite.
    mod subagent {
        use super::*;

        fn subagent_started(subagent_run_id: &str) -> Event {
            Event::SubagentStarted(SubagentStartedEvent {
                subagent_run_id: subagent_run_id.into(),
                name: "worker".into(),
                description: None,
                parent_subagent_run_id: None,
                parent_tool_call_id: None,
                parent_message_id: None,
                base: BaseEventFields::default(),
            })
        }

        fn subagent_started_named(subagent_run_id: &str, name: &str) -> Event {
            Event::SubagentStarted(SubagentStartedEvent {
                subagent_run_id: subagent_run_id.into(),
                name: name.into(),
                description: None,
                parent_subagent_run_id: None,
                parent_tool_call_id: None,
                parent_message_id: None,
                base: BaseEventFields::default(),
            })
        }

        fn subagent_finished(subagent_run_id: &str) -> Event {
            Event::SubagentFinished(SubagentFinishedEvent {
                subagent_run_id: subagent_run_id.into(),
                result: None,
                outcome: None,
                base: BaseEventFields::default(),
            })
        }

        fn text_start_tagged(message_id: &str, tag: &str) -> Event {
            Event::TextMessageStart(TextMessageStartEvent {
                message_id: message_id.into(),
                role: TextMessageRole::Assistant,
                name: None,
                base: BaseEventFields::default(),
                attributable: AttributableFields {
                    subagent_run_id: Some(tag.into()),
                },
            })
        }

        fn tool_call_start_tagged(tool_call_id: &str, tag: &str) -> Event {
            Event::ToolCallStart(ToolCallStartEvent {
                tool_call_id: tool_call_id.into(),
                tool_call_name: "search".into(),
                parent_message_id: None,
                base: BaseEventFields::default(),
                attributable: AttributableFields {
                    subagent_run_id: Some(tag.into()),
                },
            })
        }

        fn encrypted_value(
            subtype: ReasoningEncryptedValueSubtype,
            entity_id: &str,
            tag: &str,
        ) -> Event {
            Event::ReasoningEncryptedValue(ReasoningEncryptedValueEvent {
                subtype,
                entity_id: entity_id.into(),
                encrypted_value: "opaque".into(),
                base: BaseEventFields::default(),
                attributable: AttributableFields {
                    subagent_run_id: Some(tag.into()),
                },
            })
        }

        #[tokio::test]
        async fn allows_a_well_formed_lifecycle() {
            // "should allow a well-formed subagent lifecycle within a run"
            let items = collect(vec![
                factory::run_started("thread", "run"),
                subagent_started("s1"),
                subagent_finished("s1"),
                factory::run_finished("thread", "run"),
            ])
            .await;
            assert!(items.iter().all(VerifyResult::is_ok));
        }

        #[tokio::test]
        async fn rejects_a_duplicate_start_for_the_same_id() {
            // "should reject a duplicate SUBAGENT_STARTED for the same id"
            let items = collect(vec![
                factory::run_started("thread", "run"),
                subagent_started("s1"),
                subagent_started("s1"),
            ])
            .await;
            assert_validation(
                &items[2],
                "Cannot send 'SUBAGENT_STARTED': subagent 's1' is already active. Finish it with 'SUBAGENT_FINISHED' first.",
            );
        }

        #[tokio::test]
        async fn rejects_a_finish_without_a_start() {
            // "should reject SUBAGENT_FINISHED for an id that never started"
            let items = collect(vec![
                factory::run_started("thread", "run"),
                subagent_finished("s1"),
            ])
            .await;
            assert_validation(
                &items[1],
                "Cannot send 'SUBAGENT_FINISHED': no active subagent found with ID 's1'. A 'SUBAGENT_STARTED' event must be sent first.",
            );
        }

        #[tokio::test]
        async fn rejects_an_error_without_a_start() {
            // pins the SUBAGENT_ERROR half of the shared terminal check
            let items = collect(vec![
                factory::run_started("thread", "run"),
                Event::SubagentError(SubagentErrorEvent {
                    subagent_run_id: "s1".into(),
                    message: "boom".into(),
                    code: None,
                    base: BaseEventFields::default(),
                }),
            ])
            .await;
            assert_validation(
                &items[1],
                "Cannot send 'SUBAGENT_ERROR': no active subagent found with ID 's1'.",
            );
        }

        #[tokio::test]
        async fn rejects_restarting_a_subagent_that_already_finished_in_this_run() {
            // "should reject restarting a subagent that already finished in this run"
            let items = collect(vec![
                factory::run_started("thread", "run"),
                subagent_started("s1"),
                subagent_finished("s1"),
                subagent_started("s1"),
            ])
            .await;
            assert_validation(
                &items[3],
                "subagent 's1' has already finished in this run. Subagent IDs are per-invocation and cannot be reused.",
            );
        }

        #[tokio::test]
        async fn rejects_a_second_terminal_for_the_same_subagent() {
            // "should reject a second terminal for the same subagent"
            let items = collect(vec![
                factory::run_started("thread", "run"),
                subagent_started("s1"),
                subagent_finished("s1"),
                Event::SubagentError(SubagentErrorEvent {
                    subagent_run_id: "s1".into(),
                    message: "boom".into(),
                    code: None,
                    base: BaseEventFields::default(),
                }),
            ])
            .await;
            assert_validation(&items[3], "no active subagent found with ID 's1'");
        }

        #[tokio::test]
        async fn rejects_a_parent_that_was_never_started() {
            // "should still reject a parent never started in this run"
            let items = collect(vec![
                factory::run_started("thread", "run"),
                Event::SubagentStarted(SubagentStartedEvent {
                    subagent_run_id: "c".into(),
                    name: "child".into(),
                    description: None,
                    parent_subagent_run_id: Some("ghost".into()),
                    parent_tool_call_id: None,
                    parent_message_id: None,
                    base: BaseEventFields::default(),
                }),
            ])
            .await;
            assert_validation(
                &items[1],
                "parentSubagentRunId 'ghost' has not been started in this run",
            );
        }

        #[tokio::test]
        async fn allows_a_child_whose_parent_has_already_finished() {
            // "should allow a child whose parent has already finished"
            let items = collect(vec![
                factory::run_started("thread", "run"),
                subagent_started("p"),
                subagent_finished("p"),
                Event::SubagentStarted(SubagentStartedEvent {
                    subagent_run_id: "c".into(),
                    name: "child".into(),
                    description: None,
                    parent_subagent_run_id: Some("p".into()),
                    parent_tool_call_id: None,
                    parent_message_id: None,
                    base: BaseEventFields::default(),
                }),
                subagent_finished("c"),
                factory::run_finished("thread", "run"),
            ])
            .await;
            assert!(items.iter().all(VerifyResult::is_ok));
        }

        #[tokio::test]
        async fn lets_a_new_run_reuse_a_subagent_id_closed_by_the_previous_run() {
            // "should let a new run reuse a subagent id closed by the previous run"
            let items = collect(vec![
                factory::run_started("thread", "r1"),
                subagent_started("s1"),
                subagent_finished("s1"),
                factory::run_finished("thread", "r1"),
                factory::run_started("thread", "r2"),
                subagent_started("s1"),
                subagent_finished("s1"),
                factory::run_finished("thread", "r2"),
            ])
            .await;
            assert!(items.iter().all(VerifyResult::is_ok));
        }

        #[tokio::test]
        async fn rejects_run_finished_while_a_subagent_is_still_open() {
            // "should reject RUN_FINISHED while a subagent is still open"
            let items = collect(vec![
                factory::run_started("thread", "run"),
                subagent_started("s1"),
                factory::run_finished("thread", "run"),
            ])
            .await;
            assert_validation(
                &items[2],
                "Cannot send 'RUN_FINISHED' while subagents are still active: s1",
            );
        }

        #[tokio::test]
        async fn allows_an_attribution_only_stream_with_no_lifecycle_events() {
            // "should allow an attribution-only stream with no lifecycle events":
            // tagging alone never requires SUBAGENT_* — the closed-set rule must
            // not break Phase-1 producers.
            let items = collect(vec![
                factory::run_started("thread", "run"),
                text_start_tagged("m1", "never-declared"),
                factory::text_message_end("m1"),
                factory::run_finished("thread", "run"),
            ])
            .await;
            assert!(items.iter().all(VerifyResult::is_ok));
        }

        #[tokio::test]
        async fn rejects_a_close_event_whose_subagent_differs_from_its_opener() {
            // "should reject a close event whose subagentRunId differs from its opener"
            let items = collect(vec![
                factory::run_started("thread", "run"),
                text_start_tagged("m1", "s1"),
                Event::TextMessageEnd(TextMessageEndEvent {
                    message_id: "m1".into(),
                    base: BaseEventFields::default(),
                    attributable: AttributableFields {
                        subagent_run_id: Some("s2".into()),
                    },
                }),
            ])
            .await;
            assert_validation(
                &items[2],
                "subagentRunId 's2' does not match the message 'm1' opener's subagent 's1'",
            );
        }

        #[tokio::test]
        async fn rejects_a_tagged_continuation_of_an_untagged_opener() {
            // "should reject a tagged continuation of an UNTAGGED (parent-owned) opener"
            let items = collect(vec![
                factory::run_started("thread", "run"),
                factory::text_message_start("m1"),
                Event::TextMessageContent(TextMessageContentEvent {
                    message_id: "m1".into(),
                    delta: "x".into(),
                    base: BaseEventFields::default(),
                    attributable: AttributableFields {
                        subagent_run_id: Some("s1".into()),
                    },
                }),
            ])
            .await;
            assert_validation(
                &items[2],
                "does not match the message 'm1' opener's subagent '(the parent agent)'",
            );
        }

        #[tokio::test]
        async fn rejects_tool_call_args_whose_subagent_differs_from_its_opener() {
            // "should reject TOOL_CALL_ARGS whose subagentRunId differs from its opener"
            let items = collect(vec![
                factory::run_started("thread", "run"),
                tool_call_start_tagged("tc1", "s1"),
                Event::ToolCallArgs(ToolCallArgsEvent {
                    tool_call_id: "tc1".into(),
                    delta: "{}".into(),
                    base: BaseEventFields::default(),
                    attributable: AttributableFields {
                        subagent_run_id: Some("s2".into()),
                    },
                }),
            ])
            .await;
            assert_validation(&items[2], "does not match the tool call 'tc1'");
        }

        #[tokio::test]
        async fn rejects_reopening_a_closed_text_message_under_a_different_subagent() {
            // "should reject a TEXT_MESSAGE_START that reopens a closed id under a different subagent"
            let items = collect(vec![
                factory::run_started("thread", "run"),
                text_start_tagged("m1", "s1"),
                factory::text_message_end("m1"),
                text_start_tagged("m1", "s2"),
            ])
            .await;
            assert_validation(
                &items[3],
                "does not match the message 'm1' opener's subagent 's1'",
            );
        }

        #[tokio::test]
        async fn accepts_reopening_a_closed_id_under_the_same_subagent() {
            // "should accept reopening a closed id under the SAME subagent"
            let items = collect(vec![
                factory::run_started("thread", "run"),
                text_start_tagged("m1", "s1"),
                factory::text_message_end("m1"),
                text_start_tagged("m1", "s1"),
                factory::text_message_end("m1"),
                factory::run_finished("thread", "run"),
            ])
            .await;
            assert!(items.iter().all(VerifyResult::is_ok));
        }

        #[tokio::test]
        async fn rejects_a_tool_call_whose_owner_conflicts_with_its_parent_message() {
            // "should reject a tool call whose explicit owner conflicts with its
            // parent message's owner": the opener is the id's first writer, the
            // active-duplicate branch cannot shadow it
            // (upstream `subagent-verify.test.ts:1619-1636`).
            let items = collect(vec![
                factory::run_started("thread", "run"),
                text_start_tagged("m", "s1"),
                factory::text_message_end("m"),
                Event::ToolCallStart(ToolCallStartEvent {
                    tool_call_id: "tc".into(),
                    tool_call_name: "search".into(),
                    parent_message_id: Some("m".into()),
                    base: BaseEventFields::default(),
                    attributable: AttributableFields {
                        subagent_run_id: Some("s2".into()),
                    },
                }),
            ])
            .await;
            assert_validation(&items[3], "does not match its parent message 'm'");
        }

        #[tokio::test]
        async fn accepts_a_matching_tag_and_an_untagged_tool_call_inheriting() {
            // "should accept a matching tag and let an untagged tool call inherit
            // its parent's owner"
            let items = collect(vec![
                factory::run_started("thread", "run"),
                text_start_tagged("m", "s1"),
                factory::text_message_end("m"),
                Event::ToolCallStart(ToolCallStartEvent {
                    tool_call_id: "tc".into(),
                    tool_call_name: "search".into(),
                    parent_message_id: Some("m".into()),
                    base: BaseEventFields::default(),
                    attributable: AttributableFields {
                        subagent_run_id: Some("s1".into()),
                    },
                }),
                factory::tool_call_end("tc"),
                Event::ToolCallStart(ToolCallStartEvent {
                    tool_call_id: "tc2".into(),
                    tool_call_name: "search".into(),
                    parent_message_id: Some("m".into()),
                    base: BaseEventFields::default(),
                    attributable: AttributableFields::default(),
                }),
                Event::ToolCallArgs(ToolCallArgsEvent {
                    tool_call_id: "tc2".into(),
                    delta: "{}".into(),
                    base: BaseEventFields::default(),
                    attributable: AttributableFields {
                        subagent_run_id: Some("s1".into()),
                    },
                }),
                factory::tool_call_end("tc2"),
                factory::run_finished("thread", "run"),
            ])
            .await;
            assert!(items.iter().all(VerifyResult::is_ok));
        }

        // Step ownership. "should accept the parent and a subagent both having a
        // step of the same name open" and its two reject directions.
        #[tokio::test]
        async fn accepts_same_step_name_under_parent_and_subagent() {
            let items = collect(vec![
                factory::run_started("thread", "run"),
                factory::step_started("tools"),
                subagent_started("s1"),
                Event::StepStarted(StepStartedEvent {
                    step_name: "tools".into(),
                    base: BaseEventFields::default(),
                    attributable: AttributableFields {
                        subagent_run_id: Some("s1".into()),
                    },
                }),
                Event::StepFinished(StepFinishedEvent {
                    step_name: "tools".into(),
                    base: BaseEventFields::default(),
                    attributable: AttributableFields {
                        subagent_run_id: Some("s1".into()),
                    },
                }),
                subagent_finished("s1"),
                factory::step_finished("tools"),
                factory::run_finished("thread", "run"),
            ])
            .await;
            assert!(items.iter().all(VerifyResult::is_ok));
        }

        #[tokio::test]
        async fn names_the_owner_of_an_unfinished_subagent_step_at_run_finished() {
            // "should name the owner of an unfinished subagent step at RUN_FINISHED"
            let items = collect(vec![
                factory::run_started("thread", "run"),
                subagent_started("s1"),
                Event::StepStarted(StepStartedEvent {
                    step_name: "inner".into(),
                    base: BaseEventFields::default(),
                    attributable: AttributableFields {
                        subagent_run_id: Some("s1".into()),
                    },
                }),
                subagent_finished("s1"),
                factory::run_finished("thread", "run"),
            ])
            .await;
            assert_validation(&items[4], "steps are still active: inner (subagent 's1')");
        }

        // REASONING_ENCRYPTED_VALUE owner routing.
        #[tokio::test]
        async fn rejects_a_tool_call_encrypted_value_whose_owner_differs() {
            // "should reject a tool-call encrypted value whose owner differs from the tool call"
            let items = collect(vec![
                factory::run_started("thread", "run"),
                tool_call_start_tagged("tc1", "s1"),
                encrypted_value(ReasoningEncryptedValueSubtype::ToolCall, "tc1", "s2"),
            ])
            .await;
            assert_validation(&items[2], "does not match the tool call 'tc1'");
        }

        #[tokio::test]
        async fn keeps_a_tool_call_owner_after_tool_call_end() {
            // "should keep a tool call's owner after TOOL_CALL_END for a later encrypted value"
            let items = collect(vec![
                factory::run_started("thread", "run"),
                tool_call_start_tagged("c", "s1"),
                factory::tool_call_end("c"),
                encrypted_value(ReasoningEncryptedValueSubtype::ToolCall, "c", "s1"),
            ])
            .await;
            assert!(items.iter().all(VerifyResult::is_ok));
        }

        #[tokio::test]
        async fn rejects_a_message_encrypted_value_that_disagrees_with_its_reasoning_owner() {
            // "should reject a `message` encrypted value that disagrees with its REASONING owner"
            let items = collect(vec![
                factory::run_started("thread", "run"),
                Event::ReasoningStart(ReasoningStartEvent {
                    message_id: "r1".into(),
                    base: BaseEventFields::default(),
                    attributable: AttributableFields {
                        subagent_run_id: Some("s1".into()),
                    },
                }),
                Event::ReasoningMessageStart(ReasoningMessageStartEvent {
                    message_id: "r1".into(),
                    role: ReasoningMessageRole::Reasoning,
                    base: BaseEventFields::default(),
                    attributable: AttributableFields {
                        subagent_run_id: Some("s1".into()),
                    },
                }),
                Event::ReasoningMessageEnd(ReasoningMessageEndEvent {
                    message_id: "r1".into(),
                    base: BaseEventFields::default(),
                    attributable: AttributableFields {
                        subagent_run_id: Some("s1".into()),
                    },
                }),
                encrypted_value(ReasoningEncryptedValueSubtype::Message, "r1", "s2"),
            ])
            .await;
            // The error fires on the REASONING_ENCRYPTED_VALUE itself (index 4 of
            // 5 events; upstream `subagent-verify.test.ts:816-832`), then the
            // stream terminates.
            assert_validation(&items[4], "does not match the message 'r1'");
        }

        #[tokio::test]
        async fn rejects_a_message_encrypted_value_that_disagrees_with_its_text_owner() {
            // "should reject a `message` encrypted value that disagrees with its TEXT message owner"
            let items = collect(vec![
                factory::run_started("thread", "run"),
                text_start_tagged("m1", "s1"),
                factory::text_message_end("m1"),
                encrypted_value(ReasoningEncryptedValueSubtype::Message, "m1", "s2"),
            ])
            .await;
            assert_validation(&items[3], "does not match the message 'm1'");
        }

        #[tokio::test]
        async fn clears_the_retained_owner_buckets_on_a_new_run() {
            // "should clear the retained owner buckets on a new run": run 2 knows
            // nothing about run 1's tool call.
            let items = collect(vec![
                factory::run_started("thread", "r1"),
                tool_call_start_tagged("c", "s1"),
                factory::tool_call_end("c"),
                factory::run_finished("thread", "r1"),
                factory::run_started("thread", "r2"),
                encrypted_value(ReasoningEncryptedValueSubtype::ToolCall, "c", "s2"),
                factory::run_finished("thread", "r2"),
            ])
            .await;
            assert!(items.iter().all(VerifyResult::is_ok));
        }

        #[tokio::test]
        async fn distinguishes_an_id_shared_by_a_message_and_a_tool_call() {
            // "should reject an encrypted MESSAGE value whose id collides with a tool call":
            // ids are only unique within a kind.
            let items = collect(vec![
                factory::run_started("thread", "run"),
                text_start_tagged("x", "s1"),
                tool_call_start_tagged("x", "s2"),
                encrypted_value(ReasoningEncryptedValueSubtype::Message, "x", "s2"),
            ])
            .await;
            assert_validation(&items[3], "does not match the message 'x'");
        }

        #[tokio::test]
        async fn rejects_a_second_reasoning_opener_that_contradicts_the_first() {
            // "should reject a second reasoning opener that contradicts the first"
            let items = collect(vec![
                factory::run_started("thread", "run"),
                Event::ReasoningStart(ReasoningStartEvent {
                    message_id: "r1".into(),
                    base: BaseEventFields::default(),
                    attributable: AttributableFields {
                        subagent_run_id: Some("s1".into()),
                    },
                }),
                Event::ReasoningMessageStart(ReasoningMessageStartEvent {
                    message_id: "r1".into(),
                    role: ReasoningMessageRole::Reasoning,
                    base: BaseEventFields::default(),
                    attributable: AttributableFields {
                        subagent_run_id: Some("s2".into()),
                    },
                }),
            ])
            .await;
            // The error fires on the contradicting REASONING_MESSAGE_START itself
            // (index 2, upstream `subagent-verify.test.ts:734-749`): three events
            // are yielded, then the stream terminates.
            assert_validation(
                &items[2],
                "does not match the reasoning message 'r1' opener's subagent 's1'",
            );
        }

        #[tokio::test]
        async fn accepts_state_events_attributed_to_a_subagent() {
            // "should ACCEPT state events attributed to a subagent":
            // attribution on state is PROVENANCE, not ownership.
            let items = collect(vec![
                factory::run_started("thread", "run"),
                subagent_started_named("s1", "researcher"),
                Event::StateSnapshot(StateSnapshotEvent {
                    snapshot: serde_json::json!({"a": 1}),
                    base: BaseEventFields::default(),
                    attributable: AttributableFields {
                        subagent_run_id: Some("s1".into()),
                    },
                }),
                subagent_finished("s1"),
                factory::run_finished("thread", "run"),
            ])
            .await;
            assert!(items.iter().all(VerifyResult::is_ok));
        }
    }
}
