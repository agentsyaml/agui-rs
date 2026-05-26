use ag_ui_client::{AgUiError, Agent, AgentConfig, HttpAgent, HttpAgentConfig, RunAgentInput};
use futures::StreamExt;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;

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
    let mut buffer = [0_u8; 1024];
    let mut data = Vec::new();

    loop {
        let read = stream.read(&mut buffer).expect("read request");
        if read == 0 {
            return;
        }
        data.extend_from_slice(&buffer[..read]);
        if data.windows(4).any(|window| window == b"\r\n\r\n") {
            return;
        }
    }
}

#[tokio::test]
async fn protobuf_content_type_returns_unsupported_error() {
    let (url, handle) = run_server(|mut stream| {
        read_request(&mut stream);
        let response = concat!(
            "HTTP/1.1 200 OK\r\n",
            "Content-Type: application/vnd.ag-ui.event+proto\r\n",
            "Connection: close\r\n",
            "\r\n"
        );
        stream
            .write_all(response.as_bytes())
            .expect("write response headers");
        stream.write_all(&[0x01, 0x02, 0x03]).expect("write body");
        stream.flush().expect("flush response");
    });

    let agent = HttpAgent::new(HttpAgentConfig {
        url,
        headers: HashMap::new(),
        agent: AgentConfig::default(),
        request_executor: None,
    });

    let mut stream = agent
        .run(RunAgentInput::new("thread-1", "run-1"))
        .await
        .expect("request should start");

    let first = stream
        .next()
        .await
        .expect("stream should yield unsupported error");
    assert!(matches!(
        first,
        Err(AgUiError::Unsupported(message)) if message.contains("protobuf decoding is not implemented")
    ));
    assert!(stream.next().await.is_none());

    handle.join().expect("join server thread");
}

// SKIPPED: should handle multiple protobuf events in a single chunk: protobuf decoding intentionally unsupported in Rust SDK.
// SKIPPED: should handle split protobuf event across multiple chunks: protobuf decoding intentionally unsupported in Rust SDK.
// SKIPPED: should emit error when invalid protobuf data is received: protobuf decoding intentionally unsupported in Rust SDK.
// SKIPPED: should correctly encode and decode a STATE_DELTA event with JSON patch operations: protobuf decoding intentionally unsupported in Rust SDK.
// SKIPPED: should correctly encode and decode a MESSAGES_SNAPSHOT event: protobuf decoding intentionally unsupported in Rust SDK.
