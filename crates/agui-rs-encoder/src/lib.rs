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

// Accept negotiation below is a port of the official TypeScript encoder's
// `media-type.ts` (a modified jshttp/negotiator) at
// sdks/typescript/packages/encoder/src/media-type.ts, reduced to the single
// question `EventEncoder.isProtobufAccepted` asks (encoder.ts:62-68):
// does `preferredMediaTypes(accept, [AGUI_MEDIA_TYPE])` contain AGUI_MEDIA_TYPE?

/// One parsed `Accept` entry — media-type.ts `MediaType` (line 54).
struct AcceptSpec {
    ty: String,
    subtype: String,
    params: Vec<(String, String)>,
    q: f32,
    /// Position in the header; media-type.ts `MediaType.i` (line 60).
    i: usize,
}

/// media-type.ts `parseAccept` (line 77).
fn parse_accept(accept: &str) -> Vec<AcceptSpec> {
    split_quoted(accept, ',')
        .iter()
        .enumerate()
        .filter_map(|(i, part)| parse_media_type(part.trim(), i))
        .collect()
}

/// media-type.ts `parseMediaType` (line 96), driven by `simpleMediaTypeRegExp`
/// (line 48): `^\s*([^\s/;]+)\/([^;\s]+)\s*(?:;(.*))?$`. Non-matching entries
/// are dropped.
fn parse_media_type(s: &str, i: usize) -> Option<AcceptSpec> {
    let s = s.trim_start();
    let (ty, rest) = s.split_once('/')?;
    if ty.is_empty() || ty.chars().any(|c| c.is_whitespace() || c == ';') {
        return None;
    }
    let end = rest
        .char_indices()
        .find(|(_, c)| c.is_whitespace() || *c == ';')
        .map_or(rest.len(), |(n, _)| n);
    let subtype = &rest[..end];
    if subtype.is_empty() {
        return None;
    }
    // After the subtype: optional whitespace, then either `;params` or nothing.
    let tail = rest[end..].trim_start();
    let Some(params) = tail.strip_prefix(';') else {
        if tail.is_empty() {
            return Some(AcceptSpec {
                ty: ty.to_string(),
                subtype: subtype.to_string(),
                params: Vec::new(),
                q: 1.0,
                i,
            });
        }
        return None;
    };

    let mut q: f32 = 1.0;
    let mut out = Vec::new();
    for part in split_quoted(params, ';') {
        let part = part.trim();
        // media-type.ts `splitKeyValuePair` (line 240).
        let (k, v) = part.split_once('=').unwrap_or((part, ""));
        // Unwrap quotes around the value (line 114).
        let v = if v.len() > 1 && v.starts_with('"') && v.ends_with('"') {
            &v[1..v.len() - 1]
        } else {
            v
        };
        if k.eq_ignore_ascii_case("q") {
            q = js_parse_float(v);
            break; // params after `q` are not collected (line 118)
        }
        out.push((k.to_lowercase(), v.to_string()));
    }

    Some(AcceptSpec {
        ty: ty.to_string(),
        subtype: subtype.to_string(),
        params: out,
        q,
        i,
    })
}

/// media-type.ts `splitMediaTypes` (line 259) and `splitParameters` (line 279):
/// split on `sep` unless the accumulated segment has an odd number of quotes.
fn split_quoted(s: &str, sep: char) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for part in s.split(sep) {
        let open = out
            .last()
            .is_some_and(|acc| acc.matches('"').count() % 2 == 1);
        if open {
            out.last_mut().expect("checked above").push(sep);
        } else {
            out.push(part.to_string());
        }
    }
    out
}

/// JavaScript `parseFloat` (called at line 117): parses the longest numeric
/// prefix and yields NaN when there is none. NaN fails every `q > 0` test, so
/// a malformed q value rejects the entry rather than defaulting to 1.
fn js_parse_float(v: &str) -> f32 {
    let v = v.trim_start();
    let cand: Vec<char> = v
        .chars()
        .take_while(|c| c.is_ascii_digit() || matches!(c, '+' | '-' | '.' | 'e' | 'E'))
        .collect();
    for n in (1..=cand.len()).rev() {
        if let Ok(f) = cand[..n].iter().collect::<String>().parse::<f32>() {
            return f;
        }
    }
    f32::NAN
}

/// media-type.ts `specify` (line 160) with `type` pinned to the media type we
/// offer, so `p` is the protobuf media type and only the specificity bit is
/// returned. `None` mirrors the function's `return null`.
fn specificity(spec: &AcceptSpec, offered: &AcceptSpec) -> Option<u32> {
    let mut s = 0;
    if spec.ty.eq_ignore_ascii_case(&offered.ty) {
        s |= 4;
    } else if spec.ty != "*" {
        return None;
    }
    if spec.subtype.eq_ignore_ascii_case(&offered.subtype) {
        s |= 2;
    } else if spec.subtype != "*" {
        return None;
    }
    if !spec.params.is_empty() {
        // Every param the client listed must be `*` or equal ours (line 183).
        let matches_all = spec
            .params
            .iter()
            .all(|(k, v)| v == "*" || v.eq_ignore_ascii_case(offered.param(k)));
        if !matches_all {
            return None;
        }
        s |= 1;
    }
    Some(s)
}

impl AcceptSpec {
    fn param(&self, key: &str) -> &str {
        self.params
            .iter()
            .find(|(k, _)| k == key)
            .map_or("", |(_, v)| v.as_str())
    }
}

fn accepts_protobuf(accept: &str) -> bool {
    // encoder.ts:13 — a falsy `accept` (absent or empty) never negotiates.
    if accept.is_empty() {
        return false;
    }
    let accepts = parse_accept(accept);
    let offered = parse_media_type(AGUI_MEDIA_TYPE_PROTOBUF, 0).expect("constant is valid");

    // media-type.ts `getMediaTypePriority` (line 139) starts at {o: -1, q: 0, s: 0}
    // and keeps a candidate when `(s - s || q - q || o - o) < 0`, i.e. highest
    // specificity first, then q, then header order.
    let mut best = (0u32, 0.0f32, -1i64); // (s, q, o)
    for spec in &accepts {
        let Some(s) = specificity(spec, &offered) else {
            continue;
        };
        let cand = (s, spec.q, spec.i as i64);
        let wins = [
            best.0 as f32 - cand.0 as f32,
            best.1 - cand.1,
            (best.2 - cand.2) as f32,
        ]
        .into_iter()
        .find(|d| !(*d == 0.0)) // JS falsiness: 0 and NaN both fall through
        .is_some_and(|d| d < 0.0);
        if wins {
            best = cand;
        }
    }
    // media-type.ts line 36 filters out `q <= 0` (NaN included); the single
    // remaining entry is what `includes` (encoder.ts:67) then looks for.
    best.1 > 0.0
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
    fn accepts_wildcard_when_protobuf_is_offered() {
        // `*/*` resolves to the offered protobuf type: the encoder only ever
        // offers one media type, so a wildcard match *is* a protobuf match.
        let enc = EventEncoder::with_accept(Some("*/*"));
        assert!(enc.accepts_protobuf());
        assert_eq!(enc.content_type(), AGUI_MEDIA_TYPE_PROTOBUF);
    }

    #[test]
    fn missing_or_empty_accept_stays_sse() {
        for accept in [None, Some(""), Some("   ")] {
            let enc = EventEncoder::with_accept(accept);
            assert!(!enc.accepts_protobuf(), "accept={accept:?}");
            assert_eq!(enc.content_type(), AGUI_MEDIA_TYPE_SSE);
        }
    }

    #[test]
    fn wildcards_accept_protobuf() {
        for accept in ["*/*", "application/*", "*/*;q=0.5", "application/*;q=0.5"] {
            let enc = EventEncoder::with_accept(Some(accept));
            assert!(enc.accepts_protobuf(), "accept={accept}");
            assert_eq!(enc.content_type(), AGUI_MEDIA_TYPE_PROTOBUF);
        }
    }

    #[test]
    fn q_zero_rejects_protobuf() {
        for accept in [
            "application/vnd.ag-ui.event+proto;q=0",
            "*/*;q=0",
            "application/*;q=0",
            // quoted q is unwrapped, uppercase key still recognised
            "application/vnd.ag-ui.event+proto;q=\"0\"",
            "application/vnd.ag-ui.event+proto;Q=0",
        ] {
            let enc = EventEncoder::with_accept(Some(accept));
            assert!(!enc.accepts_protobuf(), "accept={accept}");
        }
    }

    #[test]
    fn more_specific_match_wins_regardless_of_header_order() {
        // `getMediaTypePriority` compares specificity before q (media-type.ts:145),
        // so the exact entry is chosen either way and its q decides the result.
        assert!(
            EventEncoder::with_accept(Some("application/vnd.ag-ui.event+proto, */*;q=0"))
                .accepts_protobuf()
        );
        assert!(
            !EventEncoder::with_accept(Some("application/vnd.ag-ui.event+proto;q=0, */*"))
                .accepts_protobuf()
        );
        // A wildcard match is likewise chosen over a less specific alternative,
        // and a q=0 wildcard still vetoes when nothing beats it on specificity.
        assert!(!EventEncoder::with_accept(Some("text/event-stream, */*;q=0")).accepts_protobuf());
    }

    #[test]
    fn unparsable_q_is_rejected() {
        // `parseFloat("abc")` is NaN, which fails `q > 0` (media-type.ts:36).
        assert!(
            !EventEncoder::with_accept(Some("application/vnd.ag-ui.event+proto;q=abc"))
                .accepts_protobuf()
        );
        assert!(!EventEncoder::with_accept(Some("*/*;q=abc")).accepts_protobuf());
        // `parseFloat` takes the numeric prefix: "0.5x" is 0.5.
        assert!(
            EventEncoder::with_accept(Some("application/vnd.ag-ui.event+proto;q=0.5x"))
                .accepts_protobuf()
        );
    }

    #[test]
    fn parameters_before_or_after_q() {
        // Params after `q` are never collected (media-type.ts:118), so this matches.
        assert!(EventEncoder::with_accept(Some(
            "application/vnd.ag-ui.event+proto;q=0.5;version=2"
        ))
        .accepts_protobuf());
        // A param before `q` is a mismatch against a paramless offered type.
        assert!(
            !EventEncoder::with_accept(Some("application/vnd.ag-ui.event+proto;version=2"))
                .accepts_protobuf()
        );
        // Unless it is the `*` wildcard (media-type.ts:186).
        assert!(
            EventEncoder::with_accept(Some("application/vnd.ag-ui.event+proto;version=*"))
                .accepts_protobuf()
        );
        // Quoted separators do not split the entry.
        assert!(!EventEncoder::with_accept(Some(
            "application/vnd.ag-ui.event+proto;version=\"a,b\""
        ))
        .accepts_protobuf());
    }

    #[test]
    fn media_type_comparison_is_case_insensitive() {
        assert!(
            EventEncoder::with_accept(Some("APPLICATION/VND.AG-UI.EVENT+PROTO")).accepts_protobuf()
        );
        assert!(EventEncoder::with_accept(Some("APPLICATION/*")).accepts_protobuf());
    }

    #[test]
    fn non_matching_entries_are_dropped() {
        for accept in [
            "text/event-stream",
            "application/json",
            "text/*",
            "*/json",
            // unparsable: no `/`
            "application/vnd.ag-ui",
            // a non-matching entry alongside a q=0 wildcard still rejects
            "application/json, */*;q=0",
        ] {
            let enc = EventEncoder::with_accept(Some(accept));
            assert!(!enc.accepts_protobuf(), "accept={accept}");
        }
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
