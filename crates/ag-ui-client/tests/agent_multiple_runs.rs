use ag_ui_client::default_apply_events;
use ag_ui_core::{
    ActivitySnapshotEvent, BaseEventFields, Message, RunFinishedOutcome, TextMessageRole,
};
use futures::{stream, StreamExt};
use serde_json::{Map, Value};

use ag_ui_core::types::UserMessage;

fn activity_content(tasks: &[&str]) -> Map<String, Value> {
    Map::from_iter([(
        "tasks".to_string(),
        Value::Array(
            tasks
                .iter()
                .map(|task| Value::String((*task).to_string()))
                .collect(),
        ),
    )])
}

async fn apply_sequence(
    events: Vec<ag_ui_core::Event>,
    initial_messages: Vec<Message>,
) -> Vec<ag_ui_client::AppliedEvent> {
    default_apply_events(
        stream::iter(events.into_iter().map(Ok)),
        initial_messages,
        Value::Null,
    )
    .collect::<Vec<_>>()
    .await
    .into_iter()
    .collect::<Result<Vec<_>, _>>()
    .expect("apply stream should succeed")
}

#[tokio::test]
async fn should_accumulate_messages_across_multiple_sequential_runs() {
    let first = apply_sequence(
        vec![
            ag_ui_core::factory::run_started("test-thread", "run-1"),
            ag_ui_core::Event::TextMessageStart(ag_ui_core::TextMessageStartEvent {
                message_id: "msg-1".into(),
                role: TextMessageRole::Assistant,
                name: None,
                base: BaseEventFields::default(),
            }),
            ag_ui_core::factory::text_message_content("msg-1", "Hello from run 1"),
            ag_ui_core::factory::text_message_end("msg-1"),
            ag_ui_core::factory::run_finished("test-thread", "run-1"),
        ],
        vec![],
    )
    .await;

    let first_messages = first.last().expect("first run result").messages.clone();
    assert_eq!(first_messages.len(), 1);

    let second = apply_sequence(
        vec![
            ag_ui_core::factory::run_started("test-thread", "run-2"),
            ag_ui_core::Event::TextMessageStart(ag_ui_core::TextMessageStartEvent {
                message_id: "msg-2".into(),
                role: TextMessageRole::Assistant,
                name: None,
                base: BaseEventFields::default(),
            }),
            ag_ui_core::factory::text_message_content("msg-2", "Hello from run 2"),
            ag_ui_core::factory::text_message_end("msg-2"),
            ag_ui_core::factory::run_finished("test-thread", "run-2"),
        ],
        first_messages,
    )
    .await;

    let messages = &second.last().expect("second run result").messages;
    assert_eq!(messages.len(), 2);
    match &messages[0] {
        Message::Assistant(message) => {
            assert_eq!(message.content.as_deref(), Some("Hello from run 1"))
        }
        _ => panic!("expected assistant message"),
    }
    match &messages[1] {
        Message::Assistant(message) => {
            assert_eq!(message.content.as_deref(), Some("Hello from run 2"))
        }
        _ => panic!("expected assistant message"),
    }
}

#[tokio::test]
async fn should_handle_three_sequential_runs_with_message_accumulation() {
    let mut messages = Vec::new();

    for (index, content) in ["First message", "Second message", "Third message"]
        .into_iter()
        .enumerate()
    {
        let run = apply_sequence(
            vec![
                ag_ui_core::factory::run_started("test-thread", format!("run-{}", index + 1)),
                ag_ui_core::Event::TextMessageStart(ag_ui_core::TextMessageStartEvent {
                    message_id: format!("msg-{}", index + 1),
                    role: TextMessageRole::Assistant,
                    name: None,
                    base: BaseEventFields::default(),
                }),
                ag_ui_core::factory::text_message_content(format!("msg-{}", index + 1), content),
                ag_ui_core::factory::text_message_end(format!("msg-{}", index + 1)),
                ag_ui_core::factory::run_finished("test-thread", format!("run-{}", index + 1)),
            ],
            messages,
        )
        .await;

        messages = run.last().expect("run result").messages.clone();
        assert_eq!(messages.len(), index + 1);
    }

    let collected: Vec<_> = messages
        .iter()
        .map(|message| match message {
            Message::Assistant(message) => message.content.clone().expect("assistant content"),
            _ => panic!("expected assistant message"),
        })
        .collect();
    assert_eq!(
        collected,
        vec!["First message", "Second message", "Third message"]
    );
}

// SKIPPED: TypeScript allows multiple runs in a single event stream; Rust verify_events treats RUN_FINISHED as terminal for that stream.

#[tokio::test]
async fn should_start_with_initial_messages_and_accumulate_new_ones() {
    let initial_messages = vec![Message::User(UserMessage {
        id: "initial-1".into(),
        content: ag_ui_core::UserMessageContent::Text("Initial message".into()),
        name: None,
        encrypted_value: None,
    })];

    let applied = apply_sequence(
        vec![
            ag_ui_core::factory::run_started("test-thread", "run-1"),
            ag_ui_core::Event::TextMessageStart(ag_ui_core::TextMessageStartEvent {
                message_id: "msg-1".into(),
                role: TextMessageRole::Assistant,
                name: None,
                base: BaseEventFields::default(),
            }),
            ag_ui_core::factory::text_message_content("msg-1", "Response message"),
            ag_ui_core::factory::text_message_end("msg-1"),
            ag_ui_core::factory::run_finished("test-thread", "run-1"),
        ],
        initial_messages,
    )
    .await;

    let messages = &applied.last().expect("run result").messages;
    assert_eq!(messages.len(), 2);
    match &messages[0] {
        Message::User(message) => match &message.content {
            ag_ui_core::UserMessageContent::Text(content) => assert_eq!(content, "Initial message"),
            _ => panic!("expected text user message"),
        },
        _ => panic!("expected user message"),
    }
    match &messages[1] {
        Message::Assistant(message) => {
            assert_eq!(message.content.as_deref(), Some("Response message"))
        }
        _ => panic!("expected assistant message"),
    }
}

#[tokio::test]
async fn should_retain_activity_messages_across_runs() {
    let first = apply_sequence(
        vec![
            ag_ui_core::factory::run_started("test-thread", "run-1"),
            ag_ui_core::Event::ActivitySnapshot(ActivitySnapshotEvent {
                message_id: "activity-1".into(),
                activity_type: "PLAN".into(),
                content: activity_content(&["task 1"]),
                replace: true,
                base: BaseEventFields::default(),
            }),
            ag_ui_core::Event::RunFinished(ag_ui_core::RunFinishedEvent {
                thread_id: "test-thread".into(),
                run_id: "run-1".into(),
                result: None,
                outcome: Some(RunFinishedOutcome::Success),
                base: BaseEventFields::default(),
            }),
        ],
        vec![],
    )
    .await;

    let first_messages = first.last().expect("first run result").messages.clone();
    assert_eq!(first_messages.len(), 1);
    assert!(matches!(first_messages[0], Message::Activity(_)));

    let second = apply_sequence(
        vec![
            ag_ui_core::factory::run_started("test-thread", "run-2"),
            ag_ui_core::Event::TextMessageStart(ag_ui_core::TextMessageStartEvent {
                message_id: "msg-2".into(),
                role: TextMessageRole::Assistant,
                name: None,
                base: BaseEventFields::default(),
            }),
            ag_ui_core::factory::text_message_content("msg-2", "Hello from run 2"),
            ag_ui_core::factory::text_message_end("msg-2"),
            ag_ui_core::factory::run_finished("test-thread", "run-2"),
        ],
        first_messages,
    )
    .await;

    let messages = &second.last().expect("second run result").messages;
    assert_eq!(messages.len(), 2);
    assert!(messages.iter().any(
        |message| matches!(message, Message::Activity(activity) if activity.id == "activity-1")
    ));
}
