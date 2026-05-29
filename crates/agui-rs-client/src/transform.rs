use agui_rs_core::{AgUiError, Event, Result};
use async_stream::try_stream;
use bytes::Bytes;
use eventsource_stream::Eventsource;
use futures::{stream::BoxStream, Stream, StreamExt, TryStreamExt};

pub const AGUI_MEDIA_TYPE_PROTOBUF: &str = "application/vnd.ag-ui.event+proto";
pub const AGUI_MEDIA_TYPE_SSE: &str = "text/event-stream";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamFormat {
    Sse,
    Protobuf,
}

pub fn detect_stream_format(content_type: Option<&str>) -> StreamFormat {
    let content_type = content_type.unwrap_or_default();
    let media_type = content_type.split(';').next().unwrap_or_default().trim();
    if media_type == AGUI_MEDIA_TYPE_PROTOBUF {
        StreamFormat::Protobuf
    } else {
        StreamFormat::Sse
    }
}

pub fn parse_sse_stream<S, E>(stream: S) -> BoxStream<'static, Result<Event>>
where
    S: Stream<Item = std::result::Result<Bytes, E>> + Send + Unpin + 'static,
    E: std::fmt::Display + Send + 'static,
{
    Box::pin(try_stream! {
        let mapped = stream.map_err(|error| AgUiError::other(error.to_string()));
        let mut stream = mapped.eventsource();
        while let Some(item) = stream.next().await {
            let event = item.map_err(|error| AgUiError::other(error.to_string()))?;
            let parsed = serde_json::from_str::<Event>(&event.data)?;
            yield parsed;
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::stream;

    fn bytes(items: &[&str]) -> Vec<std::result::Result<Bytes, &'static str>> {
        items
            .iter()
            .map(|item| Ok(Bytes::from((*item).to_string())))
            .collect()
    }

    #[tokio::test]
    async fn parses_single_event_from_sse_stream() {
        let events = parse_sse_stream(stream::iter(bytes(&[
            "data: {\"type\":\"RUN_STARTED\",\"threadId\":\"t1\",\"runId\":\"r1\"}\n\n",
        ])))
        .collect::<Vec<_>>()
        .await;

        assert!(matches!(events[0], Ok(Event::RunStarted(_))));
    }

    #[tokio::test]
    async fn parses_multiple_events_from_sse_stream() {
        let events = parse_sse_stream(stream::iter(bytes(&[
            "data: {\"type\":\"RUN_STARTED\",\"threadId\":\"t1\",\"runId\":\"r1\"}\n\n",
            "data: {\"type\":\"RUN_FINISHED\",\"threadId\":\"t1\",\"runId\":\"r1\",\"outcome\":{\"type\":\"success\"}}\n\n",
        ])))
        .collect::<Vec<_>>()
        .await;

        assert_eq!(events.len(), 2);
        assert!(matches!(events[1], Ok(Event::RunFinished(_))));
    }

    #[tokio::test]
    async fn handles_multiline_data_fields() {
        let events = parse_sse_stream(stream::iter(bytes(&[
            "data: {\"type\":\"TEXT_MESSAGE_CONTENT\",\"messageId\":\"m1\",\n",
            "data: \"delta\":\"hello\"}\n\n",
        ])))
        .collect::<Vec<_>>()
        .await;

        match &events[0] {
            Ok(Event::TextMessageContent(event)) => assert_eq!(event.delta, "hello"),
            other => panic!("unexpected event: {other:?}"),
        }
    }
}
