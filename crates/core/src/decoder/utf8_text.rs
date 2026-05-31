//! UTF-8 / labelled-encoding text decoder.
//!
//! Wraps [`crate::codec::decode`] so callers can target Shift_JIS,
//! EUC-JP, etc. by encoding label. Each frame becomes one [`Record`]
//! with the decoded text in `Record::text` and a `decode_error` tag
//! when the source encoding contained malformed sequences.

use bytes::Bytes;

use super::{Decoder, Record};
use crate::Result;

/// Text decoder.
#[derive(Debug, Clone)]
pub struct Utf8TextDecoder {
    label: String,
}

impl Default for Utf8TextDecoder {
    fn default() -> Self {
        Self {
            label: "utf-8".into(),
        }
    }
}

impl Utf8TextDecoder {
    /// Construct with the given encoding label
    /// (e.g. `"utf-8"`, `"shift_jis"`).
    #[must_use]
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
        }
    }
}

impl Decoder for Utf8TextDecoder {
    fn decode(&mut self, frame: Bytes) -> Result<Option<Record>> {
        if frame.is_empty() {
            return Ok(None);
        }
        let (text, had_errors) = crate::codec::decode(&frame, &self.label);
        let mut tags = Vec::new();
        if had_errors {
            tags.push("decode_error".into());
        }
        Ok(Some(Record {
            schema_id: None,
            level: None,
            text: Some(text),
            fields: serde_json::Value::Null,
            tags,
            correlation_id: None,
        }))
    }

    fn kind(&self) -> &'static str {
        "utf8-text"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf8_passthrough() {
        let mut d = Utf8TextDecoder::default();
        let rec = d
            .decode(Bytes::from_static("hello".as_bytes()))
            .unwrap()
            .unwrap();
        assert_eq!(rec.text.as_deref(), Some("hello"));
        assert!(rec.tags.is_empty());
    }

    #[test]
    fn shift_jis_label() {
        let mut d = Utf8TextDecoder::new("shift_jis");
        // 0x82 0xA0 = "あ" in Shift_JIS
        let rec = d
            .decode(Bytes::from_static(&[0x82, 0xA0]))
            .unwrap()
            .unwrap();
        assert_eq!(rec.text.as_deref(), Some("あ"));
    }

    #[test]
    fn empty_frame_yields_none() {
        let mut d = Utf8TextDecoder::default();
        assert!(d.decode(Bytes::new()).unwrap().is_none());
    }
}
