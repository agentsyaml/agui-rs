use agui_rs_core::{factory, AgUiError, Event, Result};
use futures::{stream::BoxStream, StreamExt};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

pub type EventSink = BoxStream<'static, Result<Event>>;

/// Channel-backed event emitter for AG-UI handlers.
#[derive(Debug, Clone)]
pub struct EventEmitter {
    tx: mpsc::Sender<Result<Event>>,
}

impl EventEmitter {
    pub async fn emit(&self, event: Event) -> Result<()> {
        self.tx
            .send(Ok(event))
            .await
            .map_err(|_| AgUiError::transport("event stream closed", false))
    }

    pub async fn emit_error(&self, err: AgUiError) {
        let _ = self.tx.send(Err(err)).await;
    }

    pub async fn run_started(&self, thread_id: &str, run_id: &str) -> Result<()> {
        self.emit(factory::run_started(thread_id, run_id)).await
    }

    pub async fn run_finished_success(&self, thread_id: &str, run_id: &str) -> Result<()> {
        self.emit(factory::run_finished(thread_id, run_id)).await
    }

    pub async fn text_message(&self, message_id: &str, text: &str) -> Result<()> {
        self.emit(factory::text_message_start(message_id)).await?;
        self.emit(factory::text_message_content(message_id, text))
            .await?;
        self.emit(factory::text_message_end(message_id)).await
    }
}

/// Creates a channel-backed emitter and boxed event stream pair.
pub fn channel(buffer: usize) -> (EventEmitter, EventSink) {
    let (tx, rx) = mpsc::channel(buffer);
    let emitter = EventEmitter { tx };
    let stream = ReceiverStream::new(rx).boxed();
    (emitter, stream)
}

#[cfg(test)]
mod tests {
    use super::*;
    use agui_rs_core::{Event, RunFinishedOutcome, TextMessageRole};
    use futures::StreamExt;

    #[tokio::test]
    async fn channel_round_trip_receives_emitted_event() {
        let (emitter, mut stream) = channel(4);

        emitter
            .emit(factory::run_started("thread-1", "run-1"))
            .await
            .unwrap();

        let event = stream.next().await.unwrap().unwrap();
        assert!(matches!(event, Event::RunStarted(_)));
    }

    #[tokio::test]
    async fn text_message_helper_emits_start_content_end_in_order() {
        let (emitter, stream) = channel(8);

        emitter.text_message("msg-1", "hello").await.unwrap();
        drop(emitter);

        let events = stream.collect::<Vec<_>>().await;
        assert_eq!(events.len(), 3);

        match &events[0] {
            Ok(Event::TextMessageStart(event)) => {
                assert_eq!(event.message_id, "msg-1");
                assert_eq!(event.role, TextMessageRole::Assistant);
            }
            other => panic!("unexpected event: {other:?}"),
        }

        match &events[1] {
            Ok(Event::TextMessageContent(event)) => {
                assert_eq!(event.message_id, "msg-1");
                assert_eq!(event.delta, "hello");
            }
            other => panic!("unexpected event: {other:?}"),
        }

        match &events[2] {
            Ok(Event::TextMessageEnd(event)) => assert_eq!(event.message_id, "msg-1"),
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[tokio::test]
    async fn run_helpers_emit_expected_payloads() {
        let (emitter, stream) = channel(4);

        emitter.run_started("thread-1", "run-1").await.unwrap();
        emitter
            .run_finished_success("thread-1", "run-1")
            .await
            .unwrap();
        drop(emitter);

        let events = stream.collect::<Vec<_>>().await;
        assert_eq!(events.len(), 2);

        match &events[0] {
            Ok(Event::RunStarted(event)) => {
                assert_eq!(event.thread_id, "thread-1");
                assert_eq!(event.run_id, "run-1");
            }
            other => panic!("unexpected event: {other:?}"),
        }

        match &events[1] {
            Ok(Event::RunFinished(event)) => {
                assert_eq!(event.thread_id, "thread-1");
                assert_eq!(event.run_id, "run-1");
                assert_eq!(event.outcome, Some(RunFinishedOutcome::Success));
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }
}
