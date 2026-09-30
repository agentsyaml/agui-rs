//! The always-on inbound compatibility boundary.
//!
//! Upstream 1.0 retires pre-1.0 shapes and keeps a boundary for the ones
//! still understood (`middleware/compatibility-boundary.ts:104-121` lists
//! them; every one has a row in the repo-root DEPRECATIONS.md). The inbound
//! ones are:
//!
//! 1. `THINKING_*` events -> their `REASONING_*` equivalents.
//! 2. `{type:"binary", mimeType, data}` content parts inside messages
//!    (`MESSAGES_SNAPSHOT`, `RUN_STARTED.input`) -> the modern media parts
//!    (`convertBinaryToNewFormat`, `middleware/legacy-content.ts:40-74`).
//! 3. `rawEvent: null` on any event -> absent.
//! 4. `result: null` on `RUN_FINISHED`/`SUBAGENT_FINISHED` -> absent.
//! 5. `parentMessageId: null` on `TOOL_CALL_START`/`TOOL_CALL_CHUNK` ->
//!    absent; `outcome: null` on `RUN_FINISHED` -> absent.
//! 6. Optional request-JSON nulls: `metadata: null` on a media part,
//!    `parameters: null` on a tool, `forwardedProps: null` on
//!    `RunAgentInput`, `payload: null` on a resume entry.
//!
//! (3)-(6) are already harmless here and cost nothing to leave alone: every
//! one of those fields is a Rust `Option`, and serde maps a JSON `null` onto
//! `None` — the same absence upstream converts to. (`Tool.parameters` is a
//! bare `Value`, which a `null` satisfies too, and which must NOT be stripped
//! as upstream does: ours is a required field, so removing the key would
//! break a frame that parses today.) So only (1) and (2) need code, and this
//! module implements those two.
//!
//! Nothing here is version-gated: a legacy-shaped event arriving is itself the
//! proof the peer is old, and on a modern stream every branch below is a
//! no-op.
//!
//! It runs on the RAW stream, before the wire bytes become typed events: the
//! 1.0 event set has no `THINKING_*` variants, and `ContentPart` has no
//! `Binary` variant either, so serde would reject such a frame before any
//! event-level code could see it. Mirrors the upstream `CompatibilityBoundary`,
//! which likewise reads the raw stream rather than the transformed one.

use serde_json::{Map, Value};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

const THINKING_START: &str = "THINKING_START";
const THINKING_END: &str = "THINKING_END";
const THINKING_TEXT_MESSAGE_START: &str = "THINKING_TEXT_MESSAGE_START";
const THINKING_TEXT_MESSAGE_CONTENT: &str = "THINKING_TEXT_MESSAGE_CONTENT";
const THINKING_TEXT_MESSAGE_END: &str = "THINKING_TEXT_MESSAGE_END";
const MESSAGES_SNAPSHOT: &str = "MESSAGES_SNAPSHOT";
const RUN_STARTED: &str = "RUN_STARTED";

/// Per-stream translation state: the ids a continuation names live here, put
/// there by the matching opener. A fresh boundary per stream keeps the state
/// per run.
#[derive(Debug, Default)]
pub(crate) struct CompatBoundary {
    current_reasoning_id: Option<String>,
    current_message_id: Option<String>,
}

/// Upgrades one raw JSON event. Non-objects and events of any other type are
/// returned unchanged.
pub(crate) fn map_raw_event(boundary: &mut CompatBoundary, event: Value) -> Value {
    let Value::Object(mut map) = event else {
        return event;
    };

    let Some(event_type) = map.get("type").and_then(Value::as_str) else {
        return Value::Object(map);
    };
    let event_type = event_type.to_owned();

    match event_type.as_str() {
        THINKING_START => {
            let message_id = mint_id("reasoning");
            boundary.current_reasoning_id = Some(message_id.clone());
            // REASONING_START has no `title`, so the span's label is dropped
            // here and nothing downstream can recover it. Named separately
            // from the conversion notice because it is a LOSS.
            if let Some(title) = map.remove("title") {
                warn_aside(&format!(
                    "Dropping {THINKING_START}.title {title}: REASONING_START has no title field, so the span's label cannot be carried and nothing downstream can recover it."
                ));
            }
            warn(THINKING_START, "REASONING_START");
            map.insert("type".to_string(), Value::String("REASONING_START".into()));
            map.insert("messageId".to_string(), Value::String(message_id));
        }
        THINKING_TEXT_MESSAGE_START => {
            let message_id = mint_id("reasoning-message");
            boundary.current_message_id = Some(message_id.clone());
            warn(THINKING_TEXT_MESSAGE_START, "REASONING_MESSAGE_START");
            map.insert(
                "type".to_string(),
                Value::String("REASONING_MESSAGE_START".into()),
            );
            map.insert("messageId".to_string(), Value::String(message_id));
            // The schema pins this role to "reasoning"; the old translation
            // said "assistant", which nothing validated.
            map.insert("role".to_string(), Value::String("reasoning".into()));
        }
        THINKING_TEXT_MESSAGE_CONTENT => {
            // A continuation keeps the opener's id; only the end closes it.
            let message_id =
                continuation_id(THINKING_TEXT_MESSAGE_CONTENT, &boundary.current_message_id);
            warn(THINKING_TEXT_MESSAGE_CONTENT, "REASONING_MESSAGE_CONTENT");
            map.insert(
                "type".to_string(),
                Value::String("REASONING_MESSAGE_CONTENT".into()),
            );
            map.insert("messageId".to_string(), Value::String(message_id));
        }
        THINKING_TEXT_MESSAGE_END => {
            let message_id = boundary
                .current_message_id
                .take()
                .unwrap_or_else(|| mint_continuation_id(THINKING_TEXT_MESSAGE_END));
            warn(THINKING_TEXT_MESSAGE_END, "REASONING_MESSAGE_END");
            map.insert(
                "type".to_string(),
                Value::String("REASONING_MESSAGE_END".into()),
            );
            map.insert("messageId".to_string(), Value::String(message_id));
        }
        THINKING_END => {
            let message_id = boundary
                .current_reasoning_id
                .take()
                .unwrap_or_else(|| mint_continuation_id(THINKING_END));
            warn(THINKING_END, "REASONING_END");
            map.insert("type".to_string(), Value::String("REASONING_END".into()));
            map.insert("messageId".to_string(), Value::String(message_id));
        }
        MESSAGES_SNAPSHOT | RUN_STARTED => {
            // Both carry messages — directly on the snapshot, under `input`
            // on the run start. Same conversion either way.
            let messages: Option<&mut Vec<Value>> = if event_type == RUN_STARTED {
                map.get_mut("input")
                    .and_then(Value::as_object_mut)
                    .and_then(|input| input.get_mut("messages"))
                    .and_then(Value::as_array_mut)
            } else {
                map.get_mut("messages").and_then(Value::as_array_mut)
            };
            let Some(messages) = messages else {
                return Value::Object(map);
            };
            for message in messages {
                let Value::Object(message) = message else {
                    continue;
                };
                if let Some(Value::Array(content)) = message.get_mut("content") {
                    upgrade_message_content(content);
                }
            }
        }
        _ => return Value::Object(map),
    }

    Value::Object(map)
}

/// Upgrades every legacy `{type:"binary"}` part in a message's content in
/// place. Mirrors `upgradeMessageContent` (`legacy-content.ts:105-128`).
fn upgrade_message_content(content: &mut Vec<Value>) {
    let original = content.clone();
    let mut upgraded: Vec<Value> = Vec::with_capacity(original.len());
    let mut converted_any = false;

    for part in &original {
        if !is_legacy_binary_part(part) {
            upgraded.push(part.clone());
            continue;
        }
        let filename = part
            .get("filename")
            .and_then(Value::as_str)
            .map(str::to_string);
        let converted = convert_binary_part(part);
        converted_any = true;
        // Match against the ORIGINAL parts, so repeated modern or legacy-only
        // attachments stay intentional repeats while a legacy mirror of a
        // modern part present alongside it is dropped.
        if converted.get("type").and_then(Value::as_str) != Some("binary")
            && original
                .iter()
                .any(|other| matches_modern_part(other, &converted, filename.as_deref()))
        {
            continue;
        }
        upgraded.push(converted);
    }

    if converted_any {
        warn("binary input content", "the modern media content parts");
    }
    *content = upgraded;
}

/// `isLegacyBinaryContent` (`legacy-content.ts:29-38`).
fn is_legacy_binary_part(part: &Value) -> bool {
    part.as_object().is_some_and(|part| {
        part.get("type").and_then(Value::as_str) == Some("binary")
            && part.get("mimeType").and_then(Value::as_str).is_some()
    })
}

/// `convertBinaryToNewFormat` (`legacy-content.ts:40-74`): the media variant
/// comes from the `mimeType` prefix, `data`/`url` become the source, and a
/// `filename` rides along as `metadata.filename`. An attachment carrying only
/// an `id` has no modern equivalent, so it stays legacy and says so.
fn convert_binary_part(binary: &Value) -> Value {
    let mime_type = binary
        .get("mimeType")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let content_type = if mime_type.starts_with("image/") {
        "image"
    } else if mime_type.starts_with("audio/") {
        "audio"
    } else if mime_type.starts_with("video/") {
        "video"
    } else {
        "document"
    };

    let source = if let Some(data) = non_empty(binary.get("data")) {
        ("data", data)
    } else if let Some(url) = non_empty(binary.get("url")) {
        ("url", url)
    } else {
        warn_aside(&format!(
            "A binary content part carries only an id ('{}') and cannot be converted to a modern media part; a 1.0 peer will not accept it. Provide data or a url. See the repo-root DEPRECATIONS.md.",
            binary.get("id").and_then(Value::as_str).unwrap_or_default()
        ));
        return binary.clone();
    };

    let mut part = Map::new();
    part.insert("type".to_string(), Value::String(content_type.to_string()));
    part.insert(
        "source".to_string(),
        serde_json::json!({ "type": source.0, "value": source.1, "mimeType": mime_type }),
    );
    if let Some(filename) = non_empty(binary.get("filename")) {
        part.insert(
            "metadata".to_string(),
            serde_json::json!({ "filename": filename }),
        );
    }
    Value::Object(part)
}

/// `matchesModernPart` (`legacy-content.ts:76-103`): a legacy filename is data
/// too, so the mirror is only a duplicate if the modern part keeps it.
fn matches_modern_part(part: &Value, converted: &Value, filename: Option<&str>) -> bool {
    if part.get("type") != converted.get("type") {
        return false;
    }
    let Some(source) = part.get("source") else {
        return false;
    };
    let Some(converted_source) = converted.get("source") else {
        return false;
    };
    for field in ["type", "value", "mimeType"] {
        if source.get(field) != converted_source.get(field) {
            return false;
        }
    }
    filename.map_or(true, |filename| {
        part.pointer("/metadata/filename").and_then(Value::as_str) == Some(filename)
    })
}

fn non_empty(value: Option<&Value>) -> Option<Value> {
    value
        .filter(|value| value.as_str().map_or(true, |text| !text.is_empty()))
        .cloned()
}

/// The id a continuation belongs to, or a freshly minted one when no opener
/// preceded it.
fn continuation_id(from: &str, established: &Option<String>) -> String {
    established
        .clone()
        .unwrap_or_else(|| mint_continuation_id(from))
}

/// A continuation with nothing to continue names nothing, and verification
/// rejects the translated event a few stages later — so the invention is
/// announced here, where it is still traceable to its cause.
fn mint_continuation_id(from: &str) -> String {
    let minted = mint_id("reasoning-message");
    warn_aside(&format!(
        "Minting a messageId ('{minted}') for {from}: no THINKING opener preceded it, so there was no id to continue. The id is this client's invention, not the producer's, and verification will reject the translated event for naming something nothing opened."
    ));
    minted
}

fn mint_id(prefix: &str) -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let counter = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{prefix}-{nanos}-{counter}")
}

fn suppressed() -> bool {
    std::env::var_os("SUPPRESS_TRANSFORMATION_WARNINGS").is_some()
}

fn warn(what: &str, replacement: &str) {
    if suppressed() {
        return;
    }
    eprintln!(
        "[ag-ui][compat] Converting deprecated {what} to {replacement}. The old shape leaves the protocol after its shim window — see the repo-root DEPRECATIONS.md. Set SUPPRESS_TRANSFORMATION_WARNINGS=true to silence."
    );
}

fn warn_aside(sentence: &str) {
    if suppressed() {
        return;
    }
    eprintln!("[ag-ui][compat] {sentence} Set SUPPRESS_TRANSFORMATION_WARNINGS=true to silence.");
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn run(events: &[Value]) -> Vec<Value> {
        let mut boundary = CompatBoundary::default();
        events
            .iter()
            .map(|event| map_raw_event(&mut boundary, event.clone()))
            .collect()
    }

    #[test]
    fn maps_full_thinking_sequence() {
        let mapped = run(&[
            json!({"type": THINKING_START, "title": "plan"}),
            json!({"type": THINKING_TEXT_MESSAGE_START, "role": "assistant"}),
            json!({"type": THINKING_TEXT_MESSAGE_CONTENT, "delta": "hello"}),
            json!({"type": THINKING_TEXT_MESSAGE_END}),
            json!({"type": THINKING_END}),
        ]);

        assert_eq!(mapped[0]["type"], "REASONING_START");
        assert!(mapped[0].get("title").is_none());
        assert_eq!(mapped[1]["type"], "REASONING_MESSAGE_START");
        assert_eq!(mapped[1]["role"], "reasoning");
        assert_eq!(mapped[2]["type"], "REASONING_MESSAGE_CONTENT");
        assert_eq!(mapped[2]["delta"], "hello");
        assert_eq!(mapped[3]["type"], "REASONING_MESSAGE_END");
        assert_eq!(mapped[4]["type"], "REASONING_END");

        let reasoning_id = mapped[0]["messageId"].as_str().unwrap();
        let message_id = mapped[1]["messageId"].as_str().unwrap();
        assert_ne!(reasoning_id, message_id);
        assert_eq!(mapped[2]["messageId"], message_id);
        assert_eq!(mapped[3]["messageId"], message_id);
        assert_eq!(mapped[4]["messageId"], reasoning_id);
    }

    #[test]
    fn mints_ids_without_an_opener() {
        let mapped = run(&[json!({"type": THINKING_TEXT_MESSAGE_CONTENT, "delta": "x"})]);
        assert!(mapped[0]["messageId"]
            .as_str()
            .is_some_and(|id| !id.is_empty()));
    }

    #[test]
    fn leaves_modern_events_untouched() {
        let event = json!({"type": "TEXT_MESSAGE_CONTENT", "messageId": "m1", "delta": "hi"});
        assert_eq!(&run(std::slice::from_ref(&event))[0], &event);
    }

    fn snapshot(content: Value) -> Value {
        json!({
            "type": MESSAGES_SNAPSHOT,
            "messages": [{"id": "m1", "role": "user", "content": content}],
        })
    }

    fn content_of(event: &Value) -> Vec<Value> {
        event["messages"][0]["content"]
            .as_array()
            .expect("content array")
            .clone()
    }

    /// A pre-1.0 peer sending a legacy attachment killed the run outright:
    /// `ContentPart` has no `Binary` variant, so the frame failed to
    /// deserialize. `convertBinaryToNewFormat` (`legacy-content.ts:40-74`)
    /// is what keeps it alive.
    #[test]
    fn upgrades_a_legacy_binary_part_in_a_messages_snapshot() {
        let mapped = run(&[snapshot(json!([
            {"type": "binary", "mimeType": "image/png", "data": "AAA", "filename": "shot.png"},
        ]))]);
        let content = content_of(&mapped[0]);

        assert_eq!(
            content[0],
            json!({
                "type": "image",
                "source": {"type": "data", "value": "AAA", "mimeType": "image/png"},
                "metadata": {"filename": "shot.png"},
            })
        );
        // The run survives: the upgraded frame deserializes.
        serde_json::from_value::<agui_rs_core::Event>(mapped[0].clone())
            .expect("upgraded snapshot must parse");
    }

    #[test]
    fn upgrades_by_mime_prefix_and_source_kind() {
        let mapped = run(&[snapshot(json!([
            {"type": "binary", "mimeType": "audio/mpeg", "data": "AAA"},
            {"type": "binary", "mimeType": "video/mp4", "url": "https://x/v"},
            {"type": "binary", "mimeType": "application/pdf", "data": "AAA"},
            {"type": "binary", "mimeType": "application/octet-stream", "data": "AAA"},
        ]))]);
        let content = content_of(&mapped[0]);

        assert_eq!(content[0]["type"], "audio");
        assert_eq!(content[1]["type"], "video");
        assert_eq!(content[1]["source"]["type"], "url");
        // No prefix match: document, the fallthrough.
        assert_eq!(content[2]["type"], "document");
        assert_eq!(content[3]["type"], "document");
        assert!(content[0].get("metadata").is_none());
    }

    /// A binary part carrying only an `id` has no modern equivalent, so it
    /// stays in its legacy shape and says so rather than being dropped
    /// silently.
    #[test]
    fn a_binary_part_with_only_an_id_stays_legacy() {
        let mapped = run(&[snapshot(json!([
            {"type": "binary", "mimeType": "image/png", "id": "file-1"},
        ]))]);
        let content = content_of(&mapped[0]);

        assert_eq!(content.len(), 1);
        assert_eq!(content[0]["type"], "binary");
        assert_eq!(content[0]["id"], "file-1");
    }

    /// A legacy mirror of a modern part already in the same message is a
    /// duplicate, not a second attachment.
    #[test]
    fn drops_a_legacy_mirror_of_a_modern_part() {
        let mapped = run(&[snapshot(json!([
            {"type": "image", "source": {"type": "data", "value": "AAA", "mimeType": "image/png"},
             "metadata": {"filename": "shot.png"}},
            {"type": "binary", "mimeType": "image/png", "data": "AAA", "filename": "shot.png"},
        ]))]);
        assert_eq!(content_of(&mapped[0]).len(), 1);

        // The modern part does not retain the legacy filename, so it is not
        // the same attachment and both stay.
        let mapped = run(&[snapshot(json!([
            {"type": "image", "source": {"type": "data", "value": "AAA", "mimeType": "image/png"}},
            {"type": "binary", "mimeType": "image/png", "data": "AAA", "filename": "shot.png"},
        ]))]);
        assert_eq!(content_of(&mapped[0]).len(), 2);
    }

    /// Legacy-only repeats are intentional, not mirrors of each other.
    #[test]
    fn keeps_repeated_legacy_attachments() {
        let mapped = run(&[snapshot(json!([
            {"type": "binary", "mimeType": "image/png", "data": "AAA"},
            {"type": "binary", "mimeType": "image/png", "data": "AAA"},
        ]))]);
        assert_eq!(content_of(&mapped[0]).len(), 2);
    }

    #[test]
    fn upgrades_binary_parts_in_run_started_input() {
        let mapped = run(&[json!({
            "type": RUN_STARTED,
            "threadId": "t1",
            "runId": "r1",
            "input": {
                "threadId": "t1",
                "runId": "r1",
                "messages": [{
                    "id": "m1",
                    "role": "user",
                    "content": [{"type": "binary", "mimeType": "image/png", "data": "AAA"}],
                }],
                "tools": [],
                "context": [],
            },
        })]);

        assert_eq!(
            mapped[0]["input"]["messages"][0]["content"][0]["type"],
            "image"
        );
        serde_json::from_value::<agui_rs_core::Event>(mapped[0].clone())
            .expect("upgraded run started must parse");
    }

    /// A 1.0 stream must be untouched: no binary part, no array to walk, and
    /// text content left exactly as it arrived.
    #[test]
    fn modern_snapshots_are_untouched() {
        let modern = snapshot(json!([
            {"type": "text", "text": "hello"},
            {"type": "image", "source": {"type": "url", "value": "https://x/i"}},
        ]));
        assert_eq!(&run(std::slice::from_ref(&modern))[0], &modern);

        // Neither a missing `messages` nor a non-array `content` is a branch
        // that can fire.
        let no_messages = json!({"type": MESSAGES_SNAPSHOT});
        assert_eq!(&run(std::slice::from_ref(&no_messages))[0], &no_messages);
        let text_content = json!({"type": MESSAGES_SNAPSHOT,
                                  "messages": [{"id": "m1", "role": "user", "content": "hi"}]});
        assert_eq!(&run(std::slice::from_ref(&text_content))[0], &text_content);
    }

    /// The rest of the boundary's list is already harmless here: those fields
    /// are `Option`s, and serde maps a JSON `null` onto `None` — the same
    /// absence upstream converts to. Asserted rather than assumed.
    #[test]
    fn the_optional_null_shims_need_no_conversion_here() {
        for event in [
            json!({"type": "TEXT_MESSAGE_CONTENT", "messageId": "m1", "delta": "x",
                   "rawEvent": null}),
            json!({"type": "TOOL_CALL_START", "toolCallId": "t", "toolCallName": "n",
                   "parentMessageId": null}),
            json!({"type": "TOOL_CALL_CHUNK", "toolCallId": "t", "delta": "x",
                   "parentMessageId": null}),
            json!({"type": "RUN_FINISHED", "threadId": "t1", "runId": "r1",
                   "result": null, "outcome": null}),
            json!({"type": "SUBAGENT_FINISHED", "subagentRunId": "s1", "result": null}),
        ] {
            let mapped = run(std::slice::from_ref(&event));
            assert_eq!(&mapped[0], &event);
            serde_json::from_value::<agui_rs_core::Event>(mapped[0].clone())
                .unwrap_or_else(|error| panic!("{event} must parse: {error}"));
        }
    }
}
