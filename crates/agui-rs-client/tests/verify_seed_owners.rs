//! Ownership seeding from replayed history (upstream
//! `client/src/verify/verify.ts:143-184` `seedOwnersFromMessages`, wired at
//! `:906-920` for `MESSAGES_SNAPSHOT` and `:922-936` for the `RUN_STARTED`
//! input echo). Ported from
//! `client/src/verify/__tests__/subagent-verify.test.ts:1034-1260`.

use agui_rs_client::verify_events;
use agui_rs_core::types::{ActivityMessage, AssistantMessage, ReasoningMessage, ToolCall};
use agui_rs_core::*;
use futures::{stream, StreamExt};

async fn collect(events: Vec<Event>) -> Vec<std::result::Result<Event, AgUiError>> {
    verify_events(stream::iter(events.into_iter().map(Ok)))
        .collect::<Vec<_>>()
        .await
}

fn expect_validation(result: &std::result::Result<Event, AgUiError>, expected: &str) {
    match result {
        Err(AgUiError::Validation(message)) => assert!(
            message.contains(expected),
            "expected '{expected}' in '{message}'"
        ),
        other => panic!("expected validation error, got {other:?}"),
    }
}

fn tag(subagent_run_id: &str) -> AttributableFields {
    AttributableFields {
        subagent_run_id: Some(subagent_run_id.into()),
    }
}

fn snapshot(messages: Vec<Message>) -> Event {
    Event::MessagesSnapshot(MessagesSnapshotEvent {
        messages,
        base: BaseEventFields::default(),
    })
}

fn assistant(
    id: &str,
    subagent_run_id: Option<&str>,
    tool_calls: Option<Vec<ToolCall>>,
) -> Message {
    Message::Assistant(AssistantMessage {
        id: id.into(),
        content: Some("old".into()),
        name: None,
        tool_calls,
        encrypted_value: None,
        subagent_run_id: subagent_run_id.map(str::to_owned),
        metadata: None,
    })
}

fn tool_call(id: &str) -> ToolCall {
    ToolCall {
        id: id.into(),
        kind: ToolCallKind::Function,
        function: FunctionCall {
            name: "search".into(),
            arguments: "{}".into(),
        },
        encrypted_value: None,
        metadata: None,
    }
}

fn tool_call_start(tool_call_id: &str, subagent: Option<&str>) -> Event {
    Event::ToolCallStart(ToolCallStartEvent {
        tool_call_id: tool_call_id.into(),
        tool_call_name: "search".into(),
        parent_message_id: None,
        base: BaseEventFields::default(),
        attributable: subagent.map(tag).unwrap_or_default(),
    })
}

fn text_message_start(message_id: &str, subagent: Option<&str>) -> Event {
    Event::TextMessageStart(TextMessageStartEvent {
        message_id: message_id.into(),
        role: TextMessageRole::Assistant,
        name: None,
        base: BaseEventFields::default(),
        attributable: subagent.map(tag).unwrap_or_default(),
    })
}

// `subagent-verify.test.ts:1043-1051`: "should reject reopening a snapshot
// message under a different subagent" — the exact replay-corruption sequence.
#[tokio::test]
async fn rejects_reopening_a_snapshot_message_under_a_different_subagent() {
    let out = collect(vec![
        factory::run_started("t", "r"),
        snapshot(vec![assistant("m", Some("s1"), None)]),
        text_message_start("m", Some("s2")),
    ])
    .await;

    expect_validation(
        &out[2],
        "does not match the message 'm' opener's subagent 's1'",
    );
}

// `subagent-verify.test.ts:1053-1062`: "should reject a tagged reopen of a
// parent-owned snapshot message".
#[tokio::test]
async fn rejects_a_tagged_reopen_of_a_parent_owned_snapshot_message() {
    let out = collect(vec![
        factory::run_started("t", "r"),
        snapshot(vec![assistant("m", None, None)]),
        text_message_start("m", Some("s2")),
    ])
    .await;

    expect_validation(&out[2], "(the parent agent)");
}

// `subagent-verify.test.ts:1064-1083`: "should accept reopening a snapshot
// message under its own subagent, and untagged".
#[tokio::test]
async fn accepts_reopening_a_snapshot_message_under_its_own_subagent_and_untagged() {
    let out = collect(vec![
        factory::run_started("t", "r"),
        snapshot(vec![assistant("m", Some("s1"), None)]),
        text_message_start("m", Some("s1")),
        factory::text_message_content("m", "new"),
        factory::text_message_end("m"),
        text_message_start("m2", None),
        factory::text_message_end("m2"),
        factory::run_finished("t", "r"),
    ])
    .await;

    assert!(out.iter().all(Result::is_ok), "{out:?}");
}

// `subagent-verify.test.ts:1085-1116`: "should seed ownership from the
// RUN_STARTED input echo".
#[tokio::test]
async fn seeds_ownership_from_the_run_started_input_echo() {
    let started = Event::RunStarted(RunStartedEvent {
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
            resume: None,
            messages: vec![assistant("m", Some("s1"), None)],
            tools: vec![],
            context: vec![],
            forwarded_props: None,
        }),
        base: BaseEventFields::default(),
    });

    let out = collect(vec![started, text_message_start("m", Some("s2"))]).await;

    expect_validation(
        &out[1],
        "does not match the message 'm' opener's subagent 's1'",
    );
}

// `subagent-verify.test.ts:1118-1128`: "should seed a snapshot reasoning
// message into the reasoning owner map".
#[tokio::test]
async fn seeds_a_snapshot_reasoning_message_into_the_reasoning_owner_map() {
    let out = collect(vec![
        factory::run_started("t", "r"),
        snapshot(vec![Message::Reasoning(ReasoningMessage {
            id: "r1".into(),
            content: "old".into(),
            encrypted_value: None,
            subagent_run_id: Some("s1".into()),
            metadata: None,
        })]),
        Event::ReasoningMessageStart(ReasoningMessageStartEvent {
            message_id: "r1".into(),
            role: ReasoningMessageRole::Reasoning,
            base: BaseEventFields::default(),
            attributable: tag("s2"),
        }),
    ])
    .await;

    expect_validation(
        &out[2],
        "does not match the reasoning message 'r1' opener's subagent 's1'",
    );
}

// `subagent-verify.test.ts:1130-1146`: "should let a later snapshot
// authoritatively replace a recorded owner".
#[tokio::test]
async fn lets_a_later_snapshot_authoritatively_replace_a_recorded_owner() {
    let out = collect(vec![
        factory::run_started("t", "r"),
        text_message_start("m", Some("s1")),
        Event::TextMessageEnd(TextMessageEndEvent {
            message_id: "m".into(),
            base: BaseEventFields::default(),
            attributable: tag("s1"),
        }),
        snapshot(vec![assistant("m", Some("s2"), None)]),
        // The OLD owner no longer matches: the snapshot moved the message to s2.
        text_message_start("m", Some("s1")),
    ])
    .await;

    expect_validation(
        &out[4],
        "does not match the message 'm' opener's subagent 's2'",
    );
}

// `subagent-verify.test.ts:1147-1160`: "should accept the snapshot's new owner
// after it replaces the recorded one".
#[tokio::test]
async fn accepts_the_snapshots_new_owner_after_it_replaces_the_recorded_one() {
    let out = collect(vec![
        factory::run_started("t", "r"),
        text_message_start("m", Some("s1")),
        Event::TextMessageEnd(TextMessageEndEvent {
            message_id: "m".into(),
            base: BaseEventFields::default(),
            attributable: tag("s1"),
        }),
        snapshot(vec![assistant("m", Some("s2"), None)]),
        text_message_start("m", Some("s2")),
        Event::TextMessageEnd(TextMessageEndEvent {
            message_id: "m".into(),
            base: BaseEventFields::default(),
            attributable: tag("s2"),
        }),
        factory::run_finished("t", "r"),
    ])
    .await;

    assert!(out.iter().all(Result::is_ok), "{out:?}");
}

// `subagent-verify.test.ts:1162-1185`: "should reject replaying a snapshot
// tool call under a different subagent".
#[tokio::test]
async fn rejects_replaying_a_snapshot_tool_call_under_a_different_subagent() {
    let out = collect(vec![
        factory::run_started("t", "r"),
        snapshot(vec![assistant(
            "m",
            Some("s1"),
            Some(vec![tool_call("tc")]),
        )]),
        tool_call_start("tc", Some("s2")),
    ])
    .await;

    expect_validation(
        &out[2],
        "does not match the tool call 'tc' opener's subagent 's1'",
    );
}

// `subagent-verify.test.ts:1186-1230` ("verifyEvents activity ownership
// survives a replace:false snapshot"): the activity bucket is seeded from a
// snapshot, so a replace:false snapshot must NOT re-own it and the delta under
// the re-owner is rejected.
#[tokio::test]
async fn rejects_a_delta_under_the_re_owner_after_a_replace_false_snapshot_on_a_seeded_activity() {
    let out = collect(vec![
        factory::run_started("t", "r"),
        snapshot(vec![Message::Activity(ActivityMessage {
            id: "a".into(),
            activity_type: "PLAN".into(),
            content: serde_json::Map::new(),
            subagent_run_id: Some("s1".into()),
            metadata: None,
        })]),
        Event::ActivitySnapshot(ActivitySnapshotEvent {
            message_id: "a".into(),
            activity_type: "PLAN".into(),
            content: serde_json::Map::new(),
            replace: Some(false),
            base: BaseEventFields::default(),
            attributable: tag("s2"),
        }),
        Event::ActivityDelta(ActivityDeltaEvent {
            message_id: "a".into(),
            activity_type: "PLAN".into(),
            patch: vec![],
            base: BaseEventFields::default(),
            attributable: tag("s2"),
        }),
    ])
    .await;

    expect_validation(
        &out[3],
        "does not match the activity 'a' opener's subagent 's1'",
    );
}

// `subagent-verify.test.ts:1231-1250`: "accepts a REPLACING snapshot re-owning
// a seeded activity, and the new owner's delta".
#[tokio::test]
async fn accepts_a_replacing_snapshot_re_owning_a_seeded_activity_and_the_new_owners_delta() {
    let out = collect(vec![
        factory::run_started("t", "r"),
        snapshot(vec![Message::Activity(ActivityMessage {
            id: "a".into(),
            activity_type: "PLAN".into(),
            content: serde_json::Map::new(),
            subagent_run_id: Some("s1".into()),
            metadata: None,
        })]),
        Event::ActivitySnapshot(ActivitySnapshotEvent {
            message_id: "a".into(),
            activity_type: "PLAN".into(),
            content: serde_json::Map::new(),
            replace: Some(true),
            base: BaseEventFields::default(),
            attributable: tag("s2"),
        }),
        Event::ActivityDelta(ActivityDeltaEvent {
            message_id: "a".into(),
            activity_type: "PLAN".into(),
            patch: vec![],
            base: BaseEventFields::default(),
            attributable: tag("s2"),
        }),
        factory::run_finished("t", "r"),
    ])
    .await;

    assert_eq!(out.len(), 5);
    assert!(out.iter().all(Result::is_ok), "{out:?}");
}
