use crate::agent::{abortable_event_stream, AbortHandle, Agent, AgentConfig, EventStream};
use crate::transform::{
    detect_stream_format, parse_proto_stream, parse_sse_stream, StreamFormat, AGUI_MEDIA_TYPE_SSE,
};
use agui_rs_core::{AgUiError, Event, Result, RunAgentInput};
use async_trait::async_trait;
use futures::future::BoxFuture;
use futures::stream::BoxStream;
use reqwest::header::{ACCEPT, CONTENT_TYPE};
use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

pub type HttpRequestExecutor = Arc<
    dyn Fn(reqwest::RequestBuilder) -> BoxFuture<'static, reqwest::Result<reqwest::Response>>
        + Send
        + Sync,
>;

#[derive(Clone)]
pub struct HttpAgentConfig {
    pub url: String,
    pub headers: HashMap<String, String>,
    pub agent: AgentConfig,
    pub request_executor: Option<HttpRequestExecutor>,
}

impl fmt::Debug for HttpAgentConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HttpAgentConfig")
            .field("url", &self.url)
            .field("headers", &self.headers)
            .field("agent", &self.agent)
            .field(
                "request_executor",
                &self.request_executor.as_ref().map(|_| "<custom>"),
            )
            .finish()
    }
}

pub struct HttpAgent {
    client: reqwest::Client,
    config: HttpAgentConfig,
}

impl HttpAgent {
    pub fn new(config: HttpAgentConfig) -> Self {
        Self::with_client(reqwest::Client::new(), config)
    }

    pub fn with_client(client: reqwest::Client, config: HttpAgentConfig) -> Self {
        Self { client, config }
    }
}

#[async_trait]
impl Agent for HttpAgent {
    async fn run(&self, input: RunAgentInput) -> Result<EventStream> {
        self.run_cancellable(input, AbortHandle::new()).await
    }

    async fn run_cancellable(
        &self,
        input: RunAgentInput,
        abort: AbortHandle,
    ) -> Result<EventStream> {
        if abort.is_aborted() {
            return Err(AgUiError::Cancelled);
        }

        let mut request = self
            .client
            .post(&self.config.url)
            .header(ACCEPT, AGUI_MEDIA_TYPE_SSE)
            .json(&input);
        for (key, value) in &self.config.headers {
            request = request.header(key, value);
        }

        let response_future: BoxFuture<'static, reqwest::Result<reqwest::Response>> =
            if let Some(executor) = &self.config.request_executor {
                executor(request)
            } else {
                Box::pin(async move { request.send().await })
            };

        let response = tokio::select! {
            _ = abort.cancelled() => return Err(AgUiError::Cancelled),
            response = response_future => response,
        }
        .map_err(|error| {
            if abort.is_aborted() {
                AgUiError::Cancelled
            } else {
                let retryable = error.is_connect() || error.is_timeout() || error.is_request();
                AgUiError::transport(format!("request failed: {error}"), retryable)
            }
        })?;

        if !response.status().is_success() {
            let status = response.status();
            let content_type = response
                .headers()
                .get(CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .map(|value| value.to_string());
            let body = response.text().await.map_err(|error| {
                AgUiError::transport(
                    format!("failed reading error response body: {error}"),
                    error.is_connect() || error.is_timeout(),
                )
            })?;
            return Err(AgUiError::Http {
                status: status.as_u16(),
                url: Some(self.config.url.clone()),
                content_type,
                body,
            });
        }

        if abort.is_aborted() {
            return Err(AgUiError::Cancelled);
        }

        let format = detect_stream_format(
            response
                .headers()
                .get(CONTENT_TYPE)
                .and_then(|value| value.to_str().ok()),
        );

        let stream: BoxStream<'static, Result<Event>> = match format {
            StreamFormat::Sse => parse_sse_stream(response.bytes_stream()),
            StreamFormat::Protobuf => parse_proto_stream(response.bytes_stream()),
        };

        Ok(abortable_event_stream(stream, abort))
    }
}

#[cfg(test)]
mod abort_tests {
    use super::*;
    use agui_rs_core::{RunFinishedEvent, RunFinishedOutcome};
    use futures::StreamExt;
    use serde_json::Value;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::thread;
    use std::time::Duration;
    use tokio::time::{sleep, timeout};

    fn sample_input() -> RunAgentInput {
        RunAgentInput {
            thread_id: "thread-1".into(),
            run_id: "run-1".into(),
            parent_run_id: None,
            state: Default::default(),
            messages: vec![],
            tools: vec![],
            context: vec![],
            forwarded_props: Value::Null,
            resume: None,
        }
    }

    fn response_headers() -> &'static [u8] {
        b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n"
    }

    fn run_server<F>(handler: F) -> (String, thread::JoinHandle<()>)
    where
        F: FnOnce(TcpStream) + Send + 'static,
    {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind listener");
        let address = format!("http://{}", listener.local_addr().expect("listener addr"));
        let handle = thread::spawn(move || {
            let (stream, _) = listener.accept().expect("accept connection");
            handler(stream);
        });
        (address, handle)
    }

    fn read_request(stream: &mut TcpStream) {
        stream
            .set_read_timeout(Some(Duration::from_secs(1)))
            .expect("set read timeout");
        let mut buffer = Vec::new();
        let mut chunk = [0_u8; 1024];

        let header_end = loop {
            let read = stream.read(&mut chunk).expect("read request");
            if read == 0 {
                return;
            }
            buffer.extend_from_slice(&chunk[..read]);
            if let Some(position) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
                break position + 4;
            }
        };

        let headers = String::from_utf8_lossy(&buffer[..header_end]);
        let content_length = headers
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().ok())
                    .flatten()
            })
            .unwrap_or(0);

        let mut remaining = content_length.saturating_sub(buffer.len().saturating_sub(header_end));
        while remaining > 0 {
            let read = stream.read(&mut chunk).expect("read request body");
            if read == 0 {
                break;
            }
            remaining = remaining.saturating_sub(read);
        }
    }

    fn write_sse_chunk(stream: &mut TcpStream, chunk: &str) {
        let _ = stream.write_all(chunk.as_bytes());
        let _ = stream.flush();
    }

    fn http_agent(url: String) -> HttpAgent {
        HttpAgent::new(HttpAgentConfig {
            url,
            headers: Default::default(),
            agent: AgentConfig::default(),
            request_executor: None,
        })
    }

    #[tokio::test]
    async fn abort_handle_new_starts_not_aborted() {
        let abort = AbortHandle::new();
        assert!(!abort.is_aborted());
    }

    #[tokio::test]
    async fn abort_handle_clones_share_state() {
        let abort = AbortHandle::new();
        let clone = abort.clone();

        clone.abort();

        assert!(abort.is_aborted());
        assert!(clone.is_aborted());
    }

    #[tokio::test]
    async fn abort_handle_abort_is_idempotent() {
        let abort = AbortHandle::new();

        abort.abort();
        abort.abort();

        assert!(abort.is_aborted());
    }

    #[tokio::test]
    async fn abort_handle_cancelled_waiter_resolves() {
        let abort = AbortHandle::new();
        let waiter = abort.clone();

        let task = tokio::spawn(async move {
            waiter.cancelled().await;
        });

        sleep(Duration::from_millis(20)).await;
        abort.abort();

        timeout(Duration::from_secs(1), task)
            .await
            .expect("waiter should finish")
            .expect("join waiter");
    }

    #[tokio::test]
    async fn pre_abort_returns_immediately() {
        let abort = AbortHandle::new();
        abort.abort();

        let result = http_agent("http://127.0.0.1:9".into())
            .run_cancellable(sample_input(), abort)
            .await;

        assert!(matches!(result, Err(AgUiError::Cancelled)));
    }

    #[tokio::test]
    async fn abort_before_response_returns_cancelled() {
        let (url, handle) = run_server(|mut stream| {
            read_request(&mut stream);
            thread::sleep(Duration::from_millis(250));
            let _ = stream.write_all(response_headers());
            let _ = stream.flush();
        });
        let agent = http_agent(url);
        let abort = AbortHandle::new();
        let request_abort = abort.clone();

        let task =
            tokio::spawn(async move { agent.run_cancellable(sample_input(), request_abort).await });

        sleep(Duration::from_millis(50)).await;
        abort.abort();

        let result = timeout(Duration::from_secs(1), task)
            .await
            .expect("request task should finish")
            .expect("join request task");

        assert!(matches!(result, Err(AgUiError::Cancelled)));
        handle.join().expect("join server thread");
    }

    #[tokio::test]
    async fn mid_stream_abort_terminates_stream() {
        let (url, handle) = run_server(|mut stream| {
            read_request(&mut stream);
            let _ = stream.write_all(response_headers());
            let _ = stream.flush();
            write_sse_chunk(&mut stream, "data: {\"type\":\"RUN_STARTED\",\"threadId\":\"thread-1\",\"runId\":\"run-1\"}\n\n");
            thread::sleep(Duration::from_millis(250));
            write_sse_chunk(
                &mut stream,
                "data: {\"type\":\"RUN_FINISHED\",\"threadId\":\"thread-1\",\"runId\":\"run-1\",\"outcome\":{\"type\":\"success\"}}\n\n",
            );
        });
        let agent = http_agent(url);
        let abort = AbortHandle::new();
        let mut stream = agent
            .run_cancellable(sample_input(), abort.clone())
            .await
            .expect("request should start");

        let first = timeout(Duration::from_secs(1), stream.next())
            .await
            .expect("first event should arrive")
            .expect("stream should yield first item")
            .expect("first event should parse");
        assert!(matches!(first, Event::RunStarted(_)));

        abort.abort();

        let second = timeout(Duration::from_secs(1), stream.next())
            .await
            .expect("second poll should finish")
            .expect("stream should yield cancellation item");
        assert!(matches!(second, Err(AgUiError::Cancelled)));
        assert!(timeout(Duration::from_secs(1), stream.next())
            .await
            .expect("stream should close")
            .is_none());

        handle.join().expect("join server thread");
    }

    #[tokio::test]
    async fn non_aborted_run_completes_normally() {
        let (url, handle) = run_server(|mut stream| {
            read_request(&mut stream);
            let _ = stream.write_all(response_headers());
            let _ = stream.flush();
            write_sse_chunk(&mut stream, "data: {\"type\":\"RUN_STARTED\",\"threadId\":\"thread-1\",\"runId\":\"run-1\"}\n\n");
            write_sse_chunk(
                &mut stream,
                "data: {\"type\":\"RUN_FINISHED\",\"threadId\":\"thread-1\",\"runId\":\"run-1\",\"outcome\":{\"type\":\"success\"}}\n\n",
            );
        });
        let agent = http_agent(url);
        let events = agent
            .run_cancellable(sample_input(), AbortHandle::new())
            .await
            .expect("request should start")
            .collect::<Vec<_>>()
            .await;

        assert_eq!(events.len(), 2);
        assert!(matches!(events[0], Ok(Event::RunStarted(_))));
        assert!(matches!(events[1], Ok(Event::RunFinished(_))));

        handle.join().expect("join server thread");
    }

    #[tokio::test]
    async fn abort_after_completion_keeps_completed_stream_intact() {
        let (url, handle) = run_server(|mut stream| {
            read_request(&mut stream);
            let _ = stream.write_all(response_headers());
            let _ = stream.flush();
            write_sse_chunk(&mut stream, "data: {\"type\":\"RUN_STARTED\",\"threadId\":\"thread-1\",\"runId\":\"run-1\"}\n\n");
            write_sse_chunk(
                &mut stream,
                "data: {\"type\":\"RUN_FINISHED\",\"threadId\":\"thread-1\",\"runId\":\"run-1\",\"outcome\":{\"type\":\"success\"}}\n\n",
            );
        });
        let agent = http_agent(url);
        let abort = AbortHandle::new();
        let mut stream = agent
            .run_cancellable(sample_input(), abort.clone())
            .await
            .expect("request should start");

        assert!(matches!(
            stream.next().await,
            Some(Ok(Event::RunStarted(_)))
        ));
        assert!(matches!(
            stream.next().await,
            Some(Ok(Event::RunFinished(RunFinishedEvent {
                outcome: Some(RunFinishedOutcome::Success),
                ..
            })))
        ));
        assert!(stream.next().await.is_none());

        abort.abort();
        assert!(abort.is_aborted());

        handle.join().expect("join server thread");
    }
}
