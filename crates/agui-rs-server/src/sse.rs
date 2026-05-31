use std::convert::Infallible;

use ::axum::body::Body;
use agui_rs_core::{factory, Event, Result};
use agui_rs_encoder::EventEncoder;
use async_stream::stream;
use bytes::Bytes;
use futures::{stream::BoxStream, StreamExt};

/// Convert an AG-UI event stream into an SSE body.
pub fn sse_body(stream: BoxStream<'static, Result<Event>>, encoder: EventEncoder) -> Body {
    let body_stream = stream! {
        let mut stream = stream;

        while let Some(item) = stream.next().await {
            match item {
                Ok(event) => match encoder.encode_sse(&event) {
                    Ok(frame) => yield Ok::<Bytes, Infallible>(Bytes::from(frame)),
                    Err(error) => {
                        tracing::error!(error = %error, "failed to encode AG-UI event as SSE");
                        break;
                    }
                },
                Err(error) => {
                    tracing::error!(error = %error, "AG-UI handler stream returned an error");

                    match encoder.encode_sse(&factory::run_error(error.to_string())) {
                        Ok(frame) => yield Ok::<Bytes, Infallible>(Bytes::from(frame)),
                        Err(encode_error) => {
                            tracing::error!(error = %encode_error, "failed to encode AG-UI run error event");
                        }
                    }

                    break;
                }
            }
        }
    };

    Body::from_stream(body_stream)
}

/// Convert an AG-UI event stream into a length-prefixed protobuf body.
///
/// Each event is encoded with [`EventEncoder::encode_binary`], which emits a
/// 4-byte big-endian length header followed by the protobuf `Event` message,
/// matching the canonical wire format. Events that fall outside the protobuf
/// schema (reasoning / activity / thinking) surface as a terminal `RUN_ERROR`.
pub fn proto_body(stream: BoxStream<'static, Result<Event>>, encoder: EventEncoder) -> Body {
    let body_stream = stream! {
        let mut stream = stream;

        while let Some(item) = stream.next().await {
            match item {
                Ok(event) => match encoder.encode_binary(&event) {
                    Ok(frame) => yield Ok::<Bytes, Infallible>(Bytes::from(frame)),
                    Err(error) => {
                        tracing::error!(error = %error, "failed to encode AG-UI event as protobuf");
                        if let Ok(frame) =
                            encoder.encode_binary(&factory::run_error(error.to_string()))
                        {
                            yield Ok::<Bytes, Infallible>(Bytes::from(frame));
                        }
                        break;
                    }
                },
                Err(error) => {
                    tracing::error!(error = %error, "AG-UI handler stream returned an error");

                    if let Ok(frame) = encoder.encode_binary(&factory::run_error(error.to_string()))
                    {
                        yield Ok::<Bytes, Infallible>(Bytes::from(frame));
                    }

                    break;
                }
            }
        }
    };

    Body::from_stream(body_stream)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::axum::body::to_bytes;
    use agui_rs_core::{factory, AgUiError};
    use futures::{stream, StreamExt};

    #[tokio::test]
    async fn sse_body_frames_each_event_as_data_lines() {
        let stream = stream::iter(vec![
            Ok(factory::run_started("thread-1", "run-1")),
            Ok(factory::run_finished("thread-1", "run-1")),
        ])
        .boxed();

        let body = sse_body(stream, EventEncoder::new());
        let bytes = to_bytes(body, usize::MAX).await.unwrap();
        let text = String::from_utf8(bytes.to_vec()).unwrap();

        assert!(text.contains("data: {\"type\":\"RUN_STARTED\""));
        assert!(text.contains("data: {\"type\":\"RUN_FINISHED\""));
        assert_eq!(text.matches("\n\n").count(), 2);
    }

    #[tokio::test]
    async fn sse_body_emits_run_error_when_stream_yields_error() {
        let stream = stream::iter(vec![Err(AgUiError::other("boom"))]).boxed();

        let body = sse_body(stream, EventEncoder::new());
        let bytes = to_bytes(body, usize::MAX).await.unwrap();
        let text = String::from_utf8(bytes.to_vec()).unwrap();

        assert!(text.contains("\"type\":\"RUN_ERROR\""));
        assert!(text.contains("\"message\":\"boom\""));
    }

    #[tokio::test]
    async fn proto_body_frames_each_event_with_length_prefix() {
        let stream = stream::iter(vec![Ok(factory::run_started("thread-1", "run-1"))]).boxed();

        let encoder = EventEncoder::with_accept(Some("application/vnd.ag-ui.event+proto"));
        let body = proto_body(stream, encoder);
        let bytes = to_bytes(body, usize::MAX).await.unwrap();

        assert!(bytes.len() > 4);
        let len = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
        assert_eq!(len, bytes.len() - 4);
    }
}
