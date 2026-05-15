use ag_ui_client::{Agent, AgentConfig, AgentRunner, RunAgentParameters};
use ag_ui_core::types::AssistantMessage;
use ag_ui_core::{
    factory, ActivityDeltaEvent, ActivitySnapshotEvent, BaseEventFields, Event, Message,
    MessagesSnapshotEvent, Result, RunAgentInput, RunFinishedEvent, RunFinishedOutcome,
};
use async_trait::async_trait;
use futures::{stream, stream::BoxStream};
use serde_json::{json, Map};

struct FakeAgent {
    events: Vec<Event>,
}

#[async_trait]
impl Agent for FakeAgent {
    async fn run(&self, _input: RunAgentInput) -> Result<BoxStream<'static, Result<Event>>> {
        Ok(Box::pin(stream::iter(self.events.clone().into_iter().map(Ok))))
    }
}

fn run_finished(run_id: &str, result: Option<serde_json::Value>, outcome: Option<RunFinishedOutcome>) -> Event {
    Event::RunFinished(RunFinishedEvent {
        thread_id: "thread-1".into(),
        run_id: run_id.into(),
        result,
        outcome,
        base: BaseEventFields::default(),
    })
}

#[tokio::test]
async fn returns_run_id_thread_id_new_messages_and_new_state() {
    let agent = FakeAgent {
        events: vec![
            factory::run_started("thread-1", "run-1"),
            Event::MessagesSnapshot(MessagesSnapshotEvent {
                messages: vec![
                Message::Assistant(AssistantMessage {
                    id: "existing-1".into(),
                    content: Some("before".into()),
                    name: None,
                    tool_calls: None,
                    encrypted_value: None,
                }),
                Message::Assistant(AssistantMessage {
                    id: "new-1".into(),
                    content: Some("after".into()),
                    name: None,
                    tool_calls: None,
                    encrypted_value: None,
                }),
                ],
                base: BaseEventFields::default(),
            }),
            factory::state_snapshot(json!({"counter": 2, "flag": true})),
            run_finished("run-1", Some(json!({"ignored": true})), Some(RunFinishedOutcome::Success)),
        ],
    };
    let mut runner = AgentRunner::new(
        agent,
        AgentConfig {
            thread_id: Some("thread-1".into()),
            initial_messages: vec![Message::Assistant(AssistantMessage {
                id: "existing-1".into(),
                content: Some("before".into()),
                name: None,
                tool_calls: None,
                encrypted_value: None,
            })],
            initial_state: json!({"counter": 0}),
            ..AgentConfig::default()
        },
    );

    let result = runner
        .run_agent(RunAgentParameters {
            run_id: Some("run-1".into()),
            ..RunAgentParameters::default()
        })
        .await
        .expect("run succeeds");

    assert_eq!(result.run_id, "run-1");
    assert_eq!(result.thread_id, "thread-1");
    assert_eq!(result.new_messages.len(), 1);
    assert_eq!(result.new_messages[0].id(), "new-1");
    assert_eq!(result.new_state, json!({"counter": 2, "flag": true}));
    assert_eq!(result.outcome, Some(RunFinishedOutcome::Success));
}

#[tokio::test]
async fn duplicate_ids_in_messages_snapshot_do_not_create_new_messages() {
    let agent = FakeAgent {
        events: vec![
            factory::run_started("thread-1", "run-1"),
            Event::MessagesSnapshot(MessagesSnapshotEvent {
                messages: vec![
                Message::Assistant(AssistantMessage {
                    id: "existing-1".into(),
                    content: Some("updated".into()),
                    name: None,
                    tool_calls: None,
                    encrypted_value: None,
                }),
                Message::Assistant(AssistantMessage {
                    id: "existing-2".into(),
                    content: Some("same".into()),
                    name: None,
                    tool_calls: None,
                    encrypted_value: None,
                }),
                ],
                base: BaseEventFields::default(),
            }),
            run_finished("run-1", None, Some(RunFinishedOutcome::Success)),
        ],
    };
    let mut runner = AgentRunner::new(
        agent,
        AgentConfig {
            initial_messages: vec![
                Message::Assistant(AssistantMessage {
                    id: "existing-1".into(),
                    content: Some("before".into()),
                    name: None,
                    tool_calls: None,
                    encrypted_value: None,
                }),
                Message::Assistant(AssistantMessage {
                    id: "existing-2".into(),
                    content: Some("same".into()),
                    name: None,
                    tool_calls: None,
                    encrypted_value: None,
                }),
            ],
            ..AgentConfig::default()
        },
    );

    let result = runner.run_agent(RunAgentParameters::default()).await.expect("run succeeds");

    assert!(result.new_messages.is_empty());
}

#[tokio::test]
async fn preserves_order_of_new_messages_from_messages_snapshot() {
    let existing = Message::Assistant(AssistantMessage {
        id: "existing".into(),
        content: Some("existing".into()),
        name: None,
        tool_calls: None,
        encrypted_value: None,
    });
    let agent = FakeAgent {
        events: vec![
            factory::run_started("thread-1", "run-1"),
            Event::MessagesSnapshot(MessagesSnapshotEvent {
                messages: vec![
                existing.clone(),
                Message::Assistant(AssistantMessage {
                    id: "new-1".into(),
                    content: Some("first".into()),
                    name: None,
                    tool_calls: None,
                    encrypted_value: None,
                }),
                Message::Assistant(AssistantMessage {
                    id: "new-2".into(),
                    content: Some("second".into()),
                    name: None,
                    tool_calls: None,
                    encrypted_value: None,
                }),
                Message::Assistant(AssistantMessage {
                    id: "new-3".into(),
                    content: Some("third".into()),
                    name: None,
                    tool_calls: None,
                    encrypted_value: None,
                }),
                ],
                base: BaseEventFields::default(),
            }),
            run_finished("run-1", None, Some(RunFinishedOutcome::Success)),
        ],
    };
    let mut runner = AgentRunner::new(
        agent,
        AgentConfig {
            initial_messages: vec![existing],
            ..AgentConfig::default()
        },
    );

    let result = runner.run_agent(RunAgentParameters::default()).await.expect("run succeeds");

    assert_eq!(
        result.new_messages.iter().map(|message| message.id()).collect::<Vec<_>>(),
        vec!["new-1", "new-2", "new-3"]
    );
}

#[tokio::test]
async fn activity_messages_are_returned_as_new_messages_with_accumulated_operations() {
    let first_operation = json!({"id": "op-1", "status": "PENDING"});
    let second_operation = json!({"id": "op-2", "status": "COMPLETE"});
    let agent = FakeAgent {
        events: vec![
            factory::run_started("thread-1", "run-ops"),
            Event::ActivitySnapshot(ActivitySnapshotEvent {
                message_id: "activity-ops".into(),
                activity_type: "PLAN".into(),
                content: Map::from_iter([(String::from("operations"), json!([]))]),
                replace: false,
                base: BaseEventFields::default(),
            }),
            Event::ActivityDelta(ActivityDeltaEvent {
                message_id: "activity-ops".into(),
                activity_type: "PLAN".into(),
                patch: vec![json!({"op": "add", "path": "/operations/-", "value": first_operation})],
                base: BaseEventFields::default(),
            }),
            Event::ActivityDelta(ActivityDeltaEvent {
                message_id: "activity-ops".into(),
                activity_type: "PLAN".into(),
                patch: vec![json!({"op": "add", "path": "/operations/-", "value": second_operation})],
                base: BaseEventFields::default(),
            }),
            run_finished("run-ops", None, Some(RunFinishedOutcome::Success)),
        ],
    };
    let mut runner = AgentRunner::new(agent, AgentConfig::default());

    let result = runner
        .run_agent(RunAgentParameters {
            run_id: Some("run-ops".into()),
            ..RunAgentParameters::default()
        })
        .await
        .expect("run succeeds");

    assert_eq!(result.new_messages.len(), 1);
    let activity_message = &result.new_messages[0];
    assert_eq!(activity_message.id(), "activity-ops");
    match activity_message {
        Message::Activity(message) => {
            assert_eq!(message.activity_type, "PLAN");
            assert_eq!(
                message.content.get("operations"),
                Some(&json!([
                    {"id": "op-1", "status": "PENDING"},
                    {"id": "op-2", "status": "COMPLETE"}
                ]))
            );
        }
        other => panic!("expected activity message, got {other:?}"),
    }
}

#[tokio::test]
async fn interrupt_outcome_is_exposed_in_result_shape() {
    let interrupt = ag_ui_core::Interrupt {
        id: "interrupt-1".into(),
        reason: "needs_human".into(),
        message: None,
        tool_call_id: None,
        response_schema: None,
        expires_at: None,
        metadata: None,
    };
    let agent = FakeAgent {
        events: vec![
            factory::run_started("thread-1", "run-1"),
            run_finished(
                "run-1",
                None,
                Some(RunFinishedOutcome::Interrupt {
                    interrupts: vec![interrupt.clone()],
                }),
            ),
        ],
    };
    let mut runner = AgentRunner::new(agent, AgentConfig::default());

    let result = runner.run_agent(RunAgentParameters::default()).await.expect("run succeeds");

    assert_eq!(
        result.outcome,
        Some(RunFinishedOutcome::Interrupt {
            interrupts: vec![interrupt],
        })
    );
}

// SKIPPED: TypeScript result.result is not exposed by Rust RunAgentResult, which only returns outcome/new_state/new_messages.
// SKIPPED: detachActiveRun tests cannot be ported because the Rust SDK does not expose a detach-active-run API on AgentRunner.
