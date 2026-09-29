use crate::compat::{map_raw_event, CompatBoundary};
use agui_rs_core::{AgUiError, Event, Result};
use async_stream::try_stream;
use bytes::Bytes;
use eventsource_stream::Eventsource;
use futures::{stream::BoxStream, Stream, StreamExt, TryStreamExt};
use serde_json::Value;

pub use agui_rs_core::{AGUI_MEDIA_TYPE_PROTOBUF, AGUI_MEDIA_TYPE_SSE};

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
        let mapped = stream.map_err(|error| AgUiError::transport(error.to_string(), true));
        let mut stream = mapped.eventsource();
        // The always-on compatibility boundary runs here, on the raw JSON,
        // because it must be in place before typed deserialization: the 1.0
        // event set has no THINKING_* variants, so serde would reject such a
        // frame before any event-level code could see it.
        let mut boundary = CompatBoundary::default();
        while let Some(item) = stream.next().await {
            let event = item.map_err(|error| AgUiError::transport(error.to_string(), true))?;
            let raw: Value = serde_json::from_str(&event.data)?;
            let raw = map_raw_event(&mut boundary, raw);
            // Enforcement, after the compatibility boundary and before typed
            // deserialization: it drops what the protocol does not describe and
            // strips what a known event does not name, each with a warning, and
            // leaves malformed KNOWN values for serde to reject fatally. It
            // precedes chunk expansion, so a chunk is enforced as an event of
            // its own rather than arriving here already repaired.
            let Some(raw) = crate::enforce::enforce_event(raw) else {
                continue;
            };
            let parsed = serde_json::from_value::<Event>(raw)?;
            yield parsed;
        }
    })
}

/// Parses a length-prefixed protobuf byte stream into AG-UI events.
///
/// Each frame is a 4-byte big-endian `uint32` length header followed by a
/// protobuf-encoded `Event` message of that length. Frames may be split across
/// or batched within byte chunks; this reassembles them. Mirrors the canonical
/// TypeScript `parseProtoStream`.
pub fn parse_proto_stream<S, E>(stream: S) -> BoxStream<'static, Result<Event>>
where
    S: Stream<Item = std::result::Result<Bytes, E>> + Send + Unpin + 'static,
    E: std::fmt::Display + Send + 'static,
{
    Box::pin(try_stream! {
        let mut stream = stream;
        let mut buffer: Vec<u8> = Vec::new();
        while let Some(item) = stream.next().await {
            let chunk = item.map_err(|error| AgUiError::transport(error.to_string(), true))?;
            buffer.extend_from_slice(&chunk);

            // Emit every complete frame currently buffered.
            loop {
                if buffer.len() < 4 {
                    break;
                }
                let len = u32::from_be_bytes([buffer[0], buffer[1], buffer[2], buffer[3]]) as usize;
                if buffer.len() < 4 + len {
                    break;
                }
                let body = buffer[4..4 + len].to_vec();
                buffer.drain(..4 + len);
                let event = agui_rs_proto::decode(&body)?;
                yield event;
            }
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
