use ag_ui_client::expand_chunks;
use ag_ui_core::{
    event_factories::create_text_message_chunk_event, factory, AgUiError, BaseEventFields, Event,
    TextMessageContentEvent, TextMessageEndEvent, TextMessageRole, TextMessageStartEvent,
};
use futures::{stream, StreamExt};

async fn collect_ok(events: Vec<Event>) -> Vec<Event> {
    let input = stream::iter(events.into_iter().map(Ok::<_, AgUiError>));
    expand_chunks(Box::pin(input))
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .collect::<Result<Vec<_>, _>>()
        .expect("chunk expansion should succeed")
}

fn text_start(message_id: &str, role: TextMessageRole) -> Event {
    Event::TextMessageStart(TextMessageStartEvent {
        message_id: message_id.to_string(),
        role,
        name: None,
        base: BaseEventFields::default(),
    })
}

fn text_content(message_id: &str, delta: &str) -> Event {
    Event::TextMessageContent(TextMessageContentEvent {
        message_id: message_id.to_string(),
        delta: delta.to_string(),
        base: BaseEventFields::default(),
    })
}

fn text_end(message_id: &str) -> Event {
    Event::TextMessageEnd(TextMessageEndEvent {
        message_id: message_id.to_string(),
        base: BaseEventFields::default(),
    })
}

#[tokio::test]
async fn preserves_developer_role_when_transforming_text_message_chunks() {
    let close_event = factory::run_finished("thread-123", "run-123");
    let events = collect_ok(vec![
        create_text_message_chunk_event(
            Some("msg-developer".into()),
            Some(TextMessageRole::Developer),
            Some("Hello from developer".into()),
            None,
            Some(1),
            None,
        ),
        close_event.clone(),
    ])
    .await;

    assert_eq!(
        events,
        vec![
            text_start("msg-developer", TextMessageRole::Developer),
            text_content("msg-developer", "Hello from developer"),
            text_end("msg-developer"),
            close_event,
        ]
    );
}

#[tokio::test]
async fn preserves_system_role_when_transforming_text_message_chunks() {
    let close_event = factory::run_finished("thread-123", "run-123");
    let events = collect_ok(vec![
        create_text_message_chunk_event(
            Some("msg-system".into()),
            Some(TextMessageRole::System),
            Some("Hello from system".into()),
            None,
            Some(1),
            None,
        ),
        close_event.clone(),
    ])
    .await;

    assert_eq!(
        events,
        vec![
            text_start("msg-system", TextMessageRole::System),
            text_content("msg-system", "Hello from system"),
            text_end("msg-system"),
            close_event,
        ]
    );
}

#[tokio::test]
async fn preserves_assistant_role_when_transforming_text_message_chunks() {
    let close_event = factory::run_finished("thread-123", "run-123");
    let events = collect_ok(vec![
        create_text_message_chunk_event(
            Some("msg-assistant".into()),
            Some(TextMessageRole::Assistant),
            Some("Hello from assistant".into()),
            None,
            Some(1),
            None,
        ),
        close_event.clone(),
    ])
    .await;

    assert_eq!(
        events,
        vec![
            text_start("msg-assistant", TextMessageRole::Assistant),
            text_content("msg-assistant", "Hello from assistant"),
            text_end("msg-assistant"),
            close_event,
        ]
    );
}

#[tokio::test]
async fn preserves_user_role_when_transforming_text_message_chunks() {
    let close_event = factory::run_finished("thread-123", "run-123");
    let events = collect_ok(vec![
        create_text_message_chunk_event(
            Some("msg-user".into()),
            Some(TextMessageRole::User),
            Some("Hello from user".into()),
            None,
            Some(1),
            None,
        ),
        close_event.clone(),
    ])
    .await;

    assert_eq!(
        events,
        vec![
            text_start("msg-user", TextMessageRole::User),
            text_content("msg-user", "Hello from user"),
            text_end("msg-user"),
            close_event,
        ]
    );
}

// SKIPPED: should preserve role 'tool' when transforming text message chunks: Rust TextMessageRole intentionally excludes tool role.

#[tokio::test]
async fn defaults_to_assistant_role_when_chunk_has_no_role() {
    let close_event = factory::run_finished("thread-123", "run-123");
    let events = collect_ok(vec![
        create_text_message_chunk_event(
            Some("msg-default".into()),
            None,
            Some("Hello default".into()),
            None,
            Some(1),
            None,
        ),
        close_event.clone(),
    ])
    .await;

    assert_eq!(
        events,
        vec![
            text_start("msg-default", TextMessageRole::Assistant),
            text_content("msg-default", "Hello default"),
            text_end("msg-default"),
            close_event,
        ]
    );
}

#[tokio::test]
async fn handles_multiple_chunks_with_different_roles() {
    let close_event = factory::run_finished("thread-123", "run-123");
    let events = collect_ok(vec![
        create_text_message_chunk_event(
            Some("msg-user".into()),
            Some(TextMessageRole::User),
            Some("User message".into()),
            None,
            Some(1),
            None,
        ),
        create_text_message_chunk_event(
            Some("msg-system".into()),
            Some(TextMessageRole::System),
            Some("System message".into()),
            None,
            Some(2),
            None,
        ),
        close_event.clone(),
    ])
    .await;

    assert_eq!(
        events,
        vec![
            text_start("msg-user", TextMessageRole::User),
            text_content("msg-user", "User message"),
            text_end("msg-user"),
            text_start("msg-system", TextMessageRole::System),
            text_content("msg-system", "System message"),
            text_end("msg-system"),
            close_event,
        ]
    );
}
