//! JSON lines decoder — interprets each frame as one JSON object.
//!
//! - Frames are expected to already be one logical line (trailing
//!   `\n` / `\r\n` stripped by an upstream [`Framer`]).
//! - If the JSON is an object, well-known fields (`level`, `msg` /
//!   `message`, `text`, `tags`, `correlation_id`, `schema_id`) are
//!   lifted onto [`Record`]. Remaining fields are kept under
//!   `Record::fields`.
//! - On JSON parse error, the bytes are surfaced as `Record::text` so
//!   the pipeline never silently drops data.
//!
//! [`Framer`]: crate::framer::Framer

use bytes::Bytes;
use serde_json::Value;

use super::{Decoder, Level, Record};
use crate::Result;

/// JSON-lines decoder.
#[derive(Debug, Default)]
pub struct JsonLinesDecoder;

impl JsonLinesDecoder {
    /// Construct.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

fn parse_level(s: &str) -> Option<Level> {
    match s.to_ascii_lowercase().as_str() {
        "trace" => Some(Level::Trace),
        "debug" => Some(Level::Debug),
        "info" | "information" => Some(Level::Info),
        "warn" | "warning" => Some(Level::Warn),
        "error" | "err" => Some(Level::Error),
        "fatal" | "critical" | "crit" => Some(Level::Fatal),
        _ => None,
    }
}

fn take_string(obj: &mut serde_json::Map<String, Value>, key: &str) -> Option<String> {
    match obj.remove(key)? {
        Value::String(s) => Some(s),
        other => Some(other.to_string()),
    }
}

fn take_tags(obj: &mut serde_json::Map<String, Value>) -> Vec<String> {
    match obj.remove("tags") {
        Some(Value::Array(a)) => a
            .into_iter()
            .filter_map(|v| match v {
                Value::String(s) => Some(s),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

impl Decoder for JsonLinesDecoder {
    fn decode(&mut self, frame: Bytes) -> Result<Option<Record>> {
        let trimmed = trim_ws(&frame);
        if trimmed.is_empty() {
            return Ok(None);
        }
        let parsed: serde_json::Result<Value> = serde_json::from_slice(trimmed);
        let rec = match parsed {
            Ok(Value::Object(mut obj)) => {
                let level = take_string(&mut obj, "level")
                    .as_deref()
                    .and_then(parse_level);
                let text = take_string(&mut obj, "msg")
                    .or_else(|| take_string(&mut obj, "message"))
                    .or_else(|| take_string(&mut obj, "text"));
                let correlation_id = take_string(&mut obj, "correlation_id");
                let schema_id = take_string(&mut obj, "schema_id");
                let tags = take_tags(&mut obj);
                Record {
                    schema_id,
                    level,
                    text,
                    fields: Value::Object(obj),
                    tags,
                    correlation_id,
                }
            }
            Ok(other) => Record {
                schema_id: None,
                level: None,
                text: None,
                fields: other,
                tags: Vec::new(),
                correlation_id: None,
            },
            Err(_) => Record {
                schema_id: None,
                level: None,
                text: Some(String::from_utf8_lossy(&frame).into_owned()),
                fields: Value::Null,
                tags: vec!["json_parse_error".into()],
                correlation_id: None,
            },
        };
        Ok(Some(rec))
    }

    fn kind(&self) -> &'static str {
        "json-lines"
    }
}

fn trim_ws(bytes: &[u8]) -> &[u8] {
    let start = bytes
        .iter()
        .position(|b| !b.is_ascii_whitespace())
        .unwrap_or(bytes.len());
    let end = bytes
        .iter()
        .rposition(|b| !b.is_ascii_whitespace())
        .map_or(start, |p| p + 1);
    &bytes[start..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn structured_object_lifts_known_fields() {
        let mut d = JsonLinesDecoder::new();
        let frame = Bytes::from_static(
            br#"{"level":"info","msg":"hi","correlation_id":"c1","tags":["a","b"],"x":1}"#,
        );
        let rec = d.decode(frame).unwrap().unwrap();
        assert_eq!(rec.level, Some(Level::Info));
        assert_eq!(rec.text.as_deref(), Some("hi"));
        assert_eq!(rec.correlation_id.as_deref(), Some("c1"));
        assert_eq!(rec.tags, vec!["a".to_string(), "b".to_string()]);
        assert_eq!(rec.fields["x"], serde_json::json!(1));
    }

    #[test]
    fn parse_error_falls_back_to_text() {
        let mut d = JsonLinesDecoder::new();
        let rec = d.decode(Bytes::from_static(b"{not json")).unwrap().unwrap();
        assert!(rec.tags.contains(&"json_parse_error".to_string()));
        assert_eq!(rec.text.as_deref(), Some("{not json"));
    }

    #[test]
    fn empty_frame_yields_none() {
        let mut d = JsonLinesDecoder::new();
        assert!(d.decode(Bytes::from_static(b"   ")).unwrap().is_none());
    }

    #[test]
    fn message_alias_is_recognised() {
        let mut d = JsonLinesDecoder::new();
        let rec = d
            .decode(Bytes::from_static(br#"{"message":"hello"}"#))
            .unwrap()
            .unwrap();
        assert_eq!(rec.text.as_deref(), Some("hello"));
    }
}
