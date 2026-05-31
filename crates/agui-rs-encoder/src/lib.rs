use agui_rs_core::{AgUiError, Event, Result};

pub use agui_rs_core::{AGUI_MEDIA_TYPE_PROTOBUF, AGUI_MEDIA_TYPE_SSE};

#[derive(Debug, Clone, Default)]
pub struct EventEncoder {
    accepts_protobuf: bool,
}

impl EventEncoder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_accept(accept: Option<&str>) -> Self {
        Self {
            accepts_protobuf: accept.map(accepts_protobuf).unwrap_or(false),
        }
    }

    pub fn accepts_protobuf(&self) -> bool {
        self.accepts_protobuf
    }

    pub fn content_type(&self) -> &'static str {
        if self.accepts_protobuf {
            AGUI_MEDIA_TYPE_PROTOBUF
        } else {
            AGUI_MEDIA_TYPE_SSE
        }
    }

    pub fn encode(&self, event: &Event) -> Result<String> {
        self.encode_sse(event)
    }

    pub fn encode_sse(&self, event: &Event) -> Result<String> {
        let json = serde_json::to_string(event).map_err(AgUiError::from)?;
        Ok(format!("data: {json}\n\n"))
    }

    pub fn encode_binary(&self, event: &Event) -> Result<Vec<u8>> {
        if self.accepts_protobuf {
            self.encode_protobuf(event)
        } else {
            Ok(self.encode_sse(event)?.into_bytes())
        }
    }

    /// Encodes an event as a length-prefixed protobuf message: a 4-byte
    /// big-endian `uint32` length header followed by the encoded `Event`
    /// message. Mirrors the canonical TypeScript `EventEncoder.encodeProtobuf`.
    pub fn encode_protobuf(&self, event: &Event) -> Result<Vec<u8>> {
        let message = agui_rs_proto::encode(event)?;
        let length = message.len() as u32;
        let mut framed = Vec::with_capacity(4 + message.len());
        framed.extend_from_slice(&length.to_be_bytes());
        framed.extend_from_slice(&message);
        Ok(framed)
    }
}

fn accepts_protobuf(accept: &str) -> bool {
    accept
        .split(',')
        .map(|part| part.split(';').next().unwrap_or("").trim())
        .any(|mt| mt.eq_ignore_ascii_case(AGUI_MEDIA_TYPE_PROTOBUF) || mt == "*/*")
}

#[cfg(test)]
mod tests {
    use super::*;
    use agui_rs_core::events::factory;

    #[test]
    fn defaults_to_sse() {
        let enc = EventEncoder::new();
        assert!(!enc.accepts_protobuf());
        assert_eq!(enc.content_type(), "text/event-stream");
    }

    #[test]
    fn detects_protobuf_accept() {
        let enc = EventEncoder::with_accept(Some(
            "application/vnd.ag-ui.event+proto, text/event-stream;q=0.5",
        ));
        assert!(enc.accepts_protobuf());
        assert_eq!(enc.content_type(), AGUI_MEDIA_TYPE_PROTOBUF);
    }

    #[test]
    fn ignores_unrelated_accept() {
        let enc = EventEncoder::with_accept(Some("application/json"));
        assert!(!enc.accepts_protobuf());
    }

    #[test]
    fn star_accept_implies_protobuf_when_offered() {
        let enc = EventEncoder::with_accept(Some("*/*"));
        assert!(enc.accepts_protobuf());
    }

    #[test]
    fn encode_sse_wraps_json_payload() {
        let enc = EventEncoder::new();
        let event = factory::run_started("thread-1", "run-1");
        let frame = enc.encode_sse(&event).unwrap();
        assert!(frame.starts_with("data: {"));
        assert!(frame.ends_with("\n\n"));
        assert!(frame.contains("\"type\":\"RUN_STARTED\""));
        assert!(frame.contains("\"threadId\":\"thread-1\""));
    }

    #[test]
    fn encode_binary_falls_back_to_sse_bytes() {
        let enc = EventEncoder::new();
        let event = factory::step_started("step-1");
        let bytes = enc.encode_binary(&event).unwrap();
        let s = std::str::from_utf8(&bytes).unwrap();
        assert!(s.starts_with("data: "));
    }

    #[test]
    fn protobuf_encode_produces_length_prefixed_bytes() {
        let enc = EventEncoder::with_accept(Some(AGUI_MEDIA_TYPE_PROTOBUF));
        let event = factory::step_started("step-1");
        let bytes = enc.encode_binary(&event).unwrap();
        // 4-byte big-endian length prefix + body of that length.
        assert!(bytes.len() > 4);
        let len = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
        assert_eq!(len, bytes.len() - 4);
    }
}
