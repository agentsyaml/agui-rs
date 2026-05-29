use agui_rs_core::{AgUiError, Event, Result};

pub const AGUI_MEDIA_TYPE_PROTOBUF: &str = "application/vnd.ag-ui.event+proto";
pub const AGUI_MEDIA_TYPE_SSE: &str = "text/event-stream";

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

    pub fn encode_protobuf(&self, _event: &Event) -> Result<Vec<u8>> {
        Err(AgUiError::Unsupported(
            "protobuf encoding is not implemented in this build".into(),
        ))
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
    fn protobuf_encode_returns_unsupported() {
        let enc = EventEncoder::with_accept(Some(AGUI_MEDIA_TYPE_PROTOBUF));
        let event = factory::step_started("step-1");
        let err = enc.encode_binary(&event).unwrap_err();
        assert!(matches!(err, AgUiError::Unsupported(_)));
    }
}
