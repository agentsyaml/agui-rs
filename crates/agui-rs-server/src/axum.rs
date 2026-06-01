use std::sync::Arc;

use ::axum::{
    body::to_bytes,
    extract::{Request, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::{post, MethodRouter},
    Router,
};
use agui_rs_core::RunAgentInput;
use agui_rs_encoder::{EventEncoder, AGUI_MEDIA_TYPE_PROTOBUF, AGUI_MEDIA_TYPE_SSE};

use crate::{
    handler::RunHandler,
    sse::{proto_body, sse_body},
};

pub fn agui_router<H: RunHandler>(handler: H) -> Router {
    Router::new().route("/", agui_route(handler))
}

pub fn agui_route<H: RunHandler>(handler: H) -> MethodRouter {
    post(run_agent::<H>).with_state(Arc::new(handler))
}

/// Binds to `addr` and serves the given router until the process exits.
///
/// A convenience wrapper around `TcpListener::bind` + `axum::serve` that maps
/// I/O failures to [`agui_rs_core::AgUiError::Transport`]. Intended for examples
/// and simple binaries; production servers will usually wire up their own
/// listener with graceful shutdown.
pub async fn serve(addr: &str, router: Router) -> agui_rs_core::Result<()> {
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|error| agui_rs_core::AgUiError::transport(error.to_string(), false))?;
    ::axum::serve(listener, router)
        .await
        .map_err(|error| agui_rs_core::AgUiError::transport(error.to_string(), false))?;
    Ok(())
}

async fn run_agent<H: RunHandler>(
    State(handler): State<Arc<H>>,
    headers: HeaderMap,
    request: Request,
) -> Response {
    let accept = headers
        .get(header::ACCEPT)
        .and_then(|value| value.to_str().ok());
    let encoder = EventEncoder::with_accept(accept);

    let body = match to_bytes(request.into_body(), usize::MAX).await {
        Ok(body) => body,
        Err(error) => {
            return (
                StatusCode::BAD_REQUEST,
                format!("invalid request body: {error}"),
            )
                .into_response();
        }
    };

    let input = match serde_json::from_slice::<RunAgentInput>(&body) {
        Ok(input) => input,
        Err(error) => {
            return (
                StatusCode::BAD_REQUEST,
                format!("invalid request body: {error}"),
            )
                .into_response();
        }
    };

    let stream = match handler.handle(input).await {
        Ok(stream) => stream,
        Err(error) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response()
        }
    };

    let (content_type, body) = if encoder.accepts_protobuf() {
        (AGUI_MEDIA_TYPE_PROTOBUF, proto_body(stream, encoder))
    } else {
        (AGUI_MEDIA_TYPE_SSE, sse_body(stream, encoder))
    };

    let mut response = Response::new(body);
    *response.status_mut() = StatusCode::OK;
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    response
        .headers_mut()
        .insert(header::CONNECTION, HeaderValue::from_static("keep-alive"));
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::axum::{body::Body, http::Request as HttpRequest};
    use agui_rs_core::{factory, AgUiError, Event, Result, RunAgentInput};
    use futures::{stream, stream::BoxStream, StreamExt};
    use tower::ServiceExt;

    struct StaticHandler {
        items: Vec<Event>,
    }

    #[async_trait::async_trait]
    impl RunHandler for StaticHandler {
        async fn handle(&self, _input: RunAgentInput) -> Result<BoxStream<'static, Result<Event>>> {
            let items = self.items.clone();
            Ok(stream::iter(items.into_iter().map(Ok)).boxed())
        }
    }

    struct FailingHandler;

    #[async_trait::async_trait]
    impl RunHandler for FailingHandler {
        async fn handle(&self, _input: RunAgentInput) -> Result<BoxStream<'static, Result<Event>>> {
            Err(AgUiError::other("handler failed"))
        }
    }

    fn valid_body() -> String {
        serde_json::to_string(&RunAgentInput::new("thread-1", "run-1")).unwrap()
    }

    #[tokio::test]
    async fn returns_bad_request_on_invalid_json_body() {
        let app = agui_router(StaticHandler { items: Vec::new() });

        let response = app
            .oneshot(
                HttpRequest::post("/")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from("{"))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let text = String::from_utf8(body.to_vec()).unwrap();
        assert!(text.starts_with("invalid request body: "));
    }

    #[tokio::test]
    async fn returns_sse_response_with_single_event() {
        let app = agui_router(StaticHandler {
            items: vec![factory::run_started("thread-1", "run-1")],
        });

        let response = app
            .oneshot(
                HttpRequest::post("/")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(valid_body()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE).unwrap(),
            AGUI_MEDIA_TYPE_SSE
        );

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let text = String::from_utf8(body.to_vec()).unwrap();
        assert!(text.contains("\"type\":\"RUN_STARTED\""));
    }

    #[tokio::test]
    async fn streams_multiple_events_in_order() {
        let app = agui_router(StaticHandler {
            items: vec![
                factory::run_started("thread-1", "run-1"),
                factory::run_finished("thread-1", "run-1"),
            ],
        });

        let response = app
            .oneshot(
                HttpRequest::post("/")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(valid_body()))
                    .unwrap(),
            )
            .await
            .unwrap();

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let text = String::from_utf8(body.to_vec()).unwrap();
        let started = text.find("\"type\":\"RUN_STARTED\"").unwrap();
        let finished = text.find("\"type\":\"RUN_FINISHED\"").unwrap();
        assert!(started < finished);
    }

    #[tokio::test]
    async fn returns_protobuf_response_for_protobuf_accept_header() {
        let app = agui_router(StaticHandler {
            items: vec![factory::run_started("thread-1", "run-1")],
        });

        let response = app
            .oneshot(
                HttpRequest::post("/")
                    .header(header::CONTENT_TYPE, "application/json")
                    .header(header::ACCEPT, "application/vnd.ag-ui.event+proto")
                    .body(Body::from(valid_body()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE).unwrap(),
            "application/vnd.ag-ui.event+proto"
        );

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        // Length-prefixed protobuf: 4-byte big-endian header + body of that length.
        assert!(body.len() > 4);
        let len = u32::from_be_bytes([body[0], body[1], body[2], body[3]]) as usize;
        assert_eq!(len, body.len() - 4);
    }

    #[tokio::test]
    async fn returns_internal_server_error_when_handler_fails() {
        let app = agui_router(FailingHandler);

        let response = app
            .oneshot(
                HttpRequest::post("/")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(valid_body()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        assert_eq!(String::from_utf8(body.to_vec()).unwrap(), "handler failed");
    }
}
