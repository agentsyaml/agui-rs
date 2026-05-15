use ag_ui_client::{AgUiError, Agent, AgentConfig, HttpAgent, HttpAgentConfig, RunAgentInput};
use ag_ui_core::Event;
use futures::StreamExt;
use serde_json::Value;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

#[derive(Debug)]
struct CapturedRequest {
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

fn http_agent(url: String, headers: HashMap<String, String>) -> HttpAgent {
    HttpAgent::new(HttpAgentConfig {
        url,
        headers,
        agent: AgentConfig::default(),
    })
}

fn spawn_server<F>(handler: F) -> (String, thread::JoinHandle<()>)
where
    F: FnOnce(TcpStream) + Send + 'static,
{
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind listener");
    let port = listener.local_addr().expect("listener addr").port();
    let handle = thread::spawn(move || {
        let (stream, _) = listener.accept().expect("accept connection");
        handler(stream);
    });
    (format!("http://127.0.0.1:{port}"), handle)
}

fn read_request(stream: &mut TcpStream) -> CapturedRequest {
    stream
        .set_read_timeout(Some(Duration::from_secs(1)))
        .expect("set read timeout");

    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 1024];

    let header_end = loop {
        let read = stream.read(&mut chunk).expect("read request");
        assert!(
            read > 0,
            "connection closed before request headers completed"
        );
        buffer.extend_from_slice(&chunk[..read]);
        if let Some(position) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
            break position + 4;
        }
    };

    let headers_text =
        String::from_utf8(buffer[..header_end].to_vec()).expect("headers should be utf8");
    let mut headers = HashMap::new();
    let mut content_length = 0_usize;

    for line in headers_text
        .lines()
        .skip(1)
        .filter(|line| !line.trim().is_empty())
    {
        let (name, value) = line.split_once(':').expect("header should contain colon");
        let value = value.trim().to_string();
        if name.eq_ignore_ascii_case("content-length") {
            content_length = value.parse().expect("content-length should parse");
        }
        headers.insert(name.to_ascii_lowercase(), value);
    }

    let mut body = buffer[header_end..].to_vec();
    while body.len() < content_length {
        let read = stream.read(&mut chunk).expect("read body");
        if read == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..read]);
    }

    CapturedRequest { headers, body }
}

fn write_response_headers(stream: &mut TcpStream, status: &str, content_type: &str) {
    let response = format!("{status}\r\nContent-Type: {content_type}\r\nConnection: close\r\n\r\n");
    stream
        .write_all(response.as_bytes())
        .expect("write response headers");
    stream.flush().expect("flush response headers");
}

fn write_sse_event(stream: &mut TcpStream, payload: &str) {
    stream
        .write_all(format!("data: {payload}\n\n").as_bytes())
        .expect("write sse event");
    stream.flush().expect("flush sse event");
}

#[tokio::test]
async fn should_configure_and_execute_http_requests_correctly() {
    let (request_tx, request_rx) = mpsc::channel();
    let (url, handle) = spawn_server(move |mut stream| {
        let captured = read_request(&mut stream);
        request_tx.send(captured).expect("send captured request");
        write_response_headers(&mut stream, "HTTP/1.1 200 OK", "text/event-stream");
    });

    let mut headers = HashMap::new();
    headers.insert("Content-Type".to_string(), "application/json".to_string());
    headers.insert("Authorization".to_string(), "Bearer test-token".to_string());

    let events = http_agent(url, headers)
        .run(RunAgentInput::new("thread-1", "run-1"))
        .await
        .expect("request should start")
        .collect::<Vec<_>>()
        .await;

    assert!(events.is_empty());

    let captured = request_rx.recv().expect("receive captured request");
    assert_eq!(
        captured.headers.get("accept").map(String::as_str),
        Some("text/event-stream")
    );
    assert_eq!(
        captured.headers.get("authorization").map(String::as_str),
        Some("Bearer test-token")
    );
    assert_eq!(
        captured.headers.get("content-type").map(String::as_str),
        Some("application/json")
    );

    let body: Value = serde_json::from_slice(&captured.body).expect("request body should be json");
    assert_eq!(body["threadId"], "thread-1");
    assert_eq!(body["runId"], "run-1");
    assert_eq!(body["messages"], Value::Array(vec![]));
    assert_eq!(body["tools"], Value::Array(vec![]));
    assert_eq!(body["context"], Value::Array(vec![]));

    handle.join().expect("join server thread");
}

#[tokio::test]
async fn should_process_http_response_data_end_to_end() {
    let (url, handle) = spawn_server(|mut stream| {
        let _captured = read_request(&mut stream);
        write_response_headers(&mut stream, "HTTP/1.1 200 OK", "text/event-stream");
        write_sse_event(
            &mut stream,
            r#"{"type":"RUN_STARTED","threadId":"thread-1","runId":"run-1"}"#,
        );
        write_sse_event(
            &mut stream,
            r#"{"type":"RUN_FINISHED","threadId":"thread-1","runId":"run-1","outcome":{"type":"success"}}"#,
        );
    });

    let events = http_agent(url, HashMap::new())
        .run(RunAgentInput::new("thread-1", "run-1"))
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
async fn should_surface_http_failures() {
    let (url, handle) = spawn_server(|mut stream| {
        let _captured = read_request(&mut stream);
        write_response_headers(&mut stream, "HTTP/1.1 404 Not Found", "application/json");
        stream
            .write_all(br#"{"message":"User not found"}"#)
            .expect("write error body");
        stream.flush().expect("flush error body");
    });

    let result = http_agent(url.clone(), HashMap::new())
        .run(RunAgentInput::new("thread-1", "run-1"))
        .await;

    assert!(matches!(
        result,
        Err(AgUiError::Protocol(message))
            if message.contains("HTTP 404")
                && message.contains(&url)
                && message.contains("application/json")
                && message.contains("User not found")
    ));

    handle.join().expect("join server thread");
}

// SKIPPED: TypeScript abortRun/custom AbortController behavior is not exposed by the Rust HttpAgent public API.
// SKIPPED: TypeScript transformHttpEventStream injection is not exposed by the Rust public API.
