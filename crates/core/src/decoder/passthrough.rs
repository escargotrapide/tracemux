//! Passthrough decoder — emits a [`Record`] with the raw bytes
//! interpreted as lossy UTF-8 text. Useful for byte streams whose
//! encoding is unknown or already validated upstream.

use bytes::Bytes;

use super::{Decoder, Record};
use crate::Result;

/// Passthrough decoder.
#[derive(Debug, Default, Clone)]
pub struct PassthroughDecoder;

impl PassthroughDecoder {
    /// Construct.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl Decoder for PassthroughDecoder {
    fn decode(&mut self, frame: Bytes) -> Result<Option<Record>> {
        if frame.is_empty() {
            return Ok(None);
        }
        Ok(Some(Record {
            schema_id: None,
            level: None,
            text: Some(String::from_utf8_lossy(&frame).into_owned()),
            fields: serde_json::Value::Null,
            tags: Vec::new(),
            correlation_id: None,
        }))
    }

    fn kind(&self) -> &'static str {
        "passthrough"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_passthrough() {
        let mut d = PassthroughDecoder::new();
        let rec = d.decode(Bytes::from_static(b"hi")).unwrap().unwrap();
        assert_eq!(rec.text.as_deref(), Some("hi"));
    }

    #[test]
    fn invalid_utf8_is_replaced_not_dropped() {
        let mut d = PassthroughDecoder::new();
        let rec = d
            .decode(Bytes::from_static(&[0xff, 0xfe]))
            .unwrap()
            .unwrap();
        assert!(rec.text.is_some());
    }

    #[test]
    fn empty_yields_none() {
        let mut d = PassthroughDecoder::new();
        assert!(d.decode(Bytes::new()).unwrap().is_none());
    }
}
