use ag_ui_client::{default_apply_events, expand_chunks};
use ag_ui_core::{
    event_factories::create_text_message_chunk_event, factory, AgUiError, BaseEventFields, Event,
    Message, TextMessageContentEvent, TextMessageEndEvent, TextMessageRole, TextMessageStartEvent,
};
use futures::{stream, StreamExt};

async fn collect_ok(events: Vec<Event>) -> Vec<Event> {
    let input = stream::iter(events.into_iter().map(Ok::<_, AgUiError>));
    expand_chunks(input)
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

fn assistant_text(message: &Message) -> Option<&str> {
    match message {
        Message::Assistant(message) => message.content.as_deref(),
        _ => None,
    }
}

fn developer_text(message: &Message) -> Option<&str> {
    match message {
        Message::Developer(message) => Some(message.content.as_str()),
        _ => None,
    }
}

fn system_text(message: &Message) -> Option<&str> {
    match message {
        Message::System(message) => Some(message.content.as_str()),
        _ => None,
    }
}

fn user_text(message: &Message) -> Option<&str> {
    match message {
        Message::User(message) => match &message.content {
            ag_ui_core::UserMessageContent::Text(content) => Some(content.as_str()),
            _ => None,
        },
        _ => None,
    }
}

#[tokio::test]
async fn should_handle_text_messages_with_role_developer() {
    let events = collect_ok(vec![
        factory::run_started("test-thread", "test-run"),
        text_start("msg-developer", TextMessageRole::Developer),
        text_content("msg-developer", "Hello from developer"),
        text_end("msg-developer"),
        factory::run_finished("test-thread", "test-run"),
    ])
    .await;

    assert_eq!(events.len(), 5);
    let Event::TextMessageStart(event) = &events[1] else {
        panic!("expected text message start");
    };
    assert_eq!(event.role, TextMessageRole::Developer);
}

#[tokio::test]
async fn should_handle_text_messages_with_role_system() {
    let events = collect_ok(vec![
        factory::run_started("test-thread", "test-run"),
        text_start("msg-system", TextMessageRole::System),
        text_content("msg-system", "Hello from system"),
        text_end("msg-system"),
        factory::run_finished("test-thread", "test-run"),
    ])
    .await;

    assert_eq!(events.len(), 5);
    let Event::TextMessageStart(event) = &events[1] else {
        panic!("expected text message start");
    };
    assert_eq!(event.role, TextMessageRole::System);
}

#[tokio::test]
async fn should_handle_text_messages_with_role_assistant() {
    let events = collect_ok(vec![
        factory::run_started("test-thread", "test-run"),
        text_start("msg-assistant", TextMessageRole::Assistant),
        text_content("msg-assistant", "Hello from assistant"),
        text_end("msg-assistant"),
        factory::run_finished("test-thread", "test-run"),
    ])
    .await;

    assert_eq!(events.len(), 5);
    let Event::TextMessageStart(event) = &events[1] else {
        panic!("expected text message start");
    };
    assert_eq!(event.role, TextMessageRole::Assistant);
}

#[tokio::test]
async fn should_handle_text_messages_with_role_user() {
    let events = collect_ok(vec![
        factory::run_started("test-thread", "test-run"),
        text_start("msg-user", TextMessageRole::User),
        text_content("msg-user", "Hello from user"),
        text_end("msg-user"),
        factory::run_finished("test-thread", "test-run"),
    ])
    .await;

    assert_eq!(events.len(), 5);
    let Event::TextMessageStart(event) = &events[1] else {
        panic!("expected text message start");
    };
    assert_eq!(event.role, TextMessageRole::User);
}

#[tokio::test]
async fn should_handle_multiple_messages_with_different_roles_in_a_single_run() {
    let close_event = factory::run_finished("test-thread", "test-run");
    let events = collect_ok(vec![
        factory::run_started("test-thread", "test-run"),
        text_start("msg-developer", TextMessageRole::Developer),
        text_content("msg-developer", "Message from developer"),
        text_end("msg-developer"),
        text_start("msg-system", TextMessageRole::System),
        text_content("msg-system", "Message from system"),
        text_end("msg-system"),
        text_start("msg-assistant", TextMessageRole::Assistant),
        text_content("msg-assistant", "Message from assistant"),
        text_end("msg-assistant"),
        text_start("msg-user", TextMessageRole::User),
        text_content("msg-user", "Message from user"),
        text_end("msg-user"),
        close_event.clone(),
    ])
    .await;

    assert_eq!(events.len(), 14);
    assert_eq!(events.last(), Some(&close_event));
}

#[tokio::test]
async fn should_handle_text_message_chunks_with_different_roles() {
    let close_event = factory::run_finished("test-thread", "test-run");
    let events = collect_ok(vec![
        factory::run_started("test-thread", "test-run"),
        create_text_message_chunk_event(
            Some("msg-user".into()),
            Some(TextMessageRole::User),
            Some("User chunk message".into()),
            None,
            Some(1),
            None,
        ),
        create_text_message_chunk_event(
            Some("msg-system".into()),
            Some(TextMessageRole::System),
            Some("System chunk message".into()),
            None,
            Some(2),
            None,
        ),
        close_event.clone(),
    ])
    .await;

    assert_eq!(events.len(), 8);
    assert_eq!(events[1], text_start("msg-user", TextMessageRole::User));
    assert_eq!(events[2], text_content("msg-user", "User chunk message"));
    assert_eq!(events[4], text_start("msg-system", TextMessageRole::System));
    assert_eq!(
        events[5],
        text_content("msg-system", "System chunk message")
    );
}

#[tokio::test]
async fn should_default_to_assistant_role_when_not_specified() {
    let events = collect_ok(vec![
        factory::run_started("test-thread", "test-run"),
        Event::TextMessageStart(TextMessageStartEvent {
            message_id: "msg-default".into(),
            role: TextMessageRole::default(),
            name: None,
            base: BaseEventFields::default(),
        }),
        text_content("msg-default", "Default role message"),
        text_end("msg-default"),
        factory::run_finished("test-thread", "test-run"),
    ])
    .await;

    let Event::TextMessageStart(event) = &events[1] else {
        panic!("expected text message start");
    };
    assert_eq!(event.role, TextMessageRole::Assistant);
}

#[tokio::test]
async fn should_preserve_role_when_mixing_regular_and_chunk_events() {
    let close_event = factory::run_finished("test-thread", "test-run");
    let events = collect_ok(vec![
        factory::run_started("test-thread", "test-run"),
        text_start("msg-1", TextMessageRole::User),
        text_content("msg-1", "User message"),
        text_end("msg-1"),
        create_text_message_chunk_event(
            Some("msg-2".into()),
            Some(TextMessageRole::Developer),
            Some("Developer chunk".into()),
            None,
            Some(2),
            None,
        ),
        close_event.clone(),
    ])
    .await;

    assert_eq!(events.len(), 8);
    assert_eq!(events[1], text_start("msg-1", TextMessageRole::User));
    assert_eq!(events[4], text_start("msg-2", TextMessageRole::Developer));
    assert_eq!(events[5], text_content("msg-2", "Developer chunk"));
}

#[test]
fn should_create_messages_with_expected_roles_when_applied() {
    let applied = futures::executor::block_on(async {
        default_apply_events(
            stream::iter(vec![
                Ok(text_start("developer", TextMessageRole::Developer)),
                Ok(text_content("developer", "d")),
                Ok(text_start("system", TextMessageRole::System)),
                Ok(text_content("system", "s")),
                Ok(text_start("assistant", TextMessageRole::Assistant)),
                Ok(text_content("assistant", "a")),
                Ok(text_start("user", TextMessageRole::User)),
                Ok(text_content("user", "u")),
            ]),
            Vec::new(),
            serde_json::Value::Null,
        )
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .collect::<Result<Vec<_>, _>>()
        .expect("apply stream should succeed")
    });

    let messages = &applied.last().expect("final applied event").messages;
    assert_eq!(messages.len(), 4);
    assert_eq!(developer_text(&messages[0]), Some("d"));
    assert_eq!(system_text(&messages[1]), Some("s"));
    assert_eq!(assistant_text(&messages[2]), Some("a"));
    assert_eq!(user_text(&messages[3]), Some("u"));
}
