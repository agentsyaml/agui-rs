use std::convert::Infallible;

use ::axum::body::Body;
use ag_ui_core::{factory, Event, Result};
use ag_ui_encoder::EventEncoder;
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

#[cfg(test)]
mod tests {
    use super::*;
    use ::axum::body::to_bytes;
    use ag_ui_core::{factory, AgUiError};
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
}
