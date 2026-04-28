//! NMEA 0183 decoder.
//!
//! Recognises the standard sentence form `$<TALKER><MSG>,<fields>*<HH>`
//! (and the proprietary `!`-prefixed form). Validates the optional
//! `*HH` checksum (XOR of bytes between `$`/`!` and `*`).
//!
//! - `Record::text` carries the original sentence (without trailing
//!   `\r\n`).
//! - `Record::fields` carries `{talker, message, parts: [...], checksum_ok}`.
//! - `Record::tags` includes `"checksum_mismatch"` when the embedded
//!   checksum did not match.

use bytes::Bytes;
use serde_json::json;

use super::{Decoder, Record};
use crate::Result;

/// NMEA decoder.
#[derive(Debug, Default, Clone)]
pub struct NmeaDecoder;

impl NmeaDecoder {
    /// Construct.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

fn xor_checksum(body: &[u8]) -> u8 {
    body.iter().fold(0u8, |acc, b| acc ^ b)
}

impl Decoder for NmeaDecoder {
    fn decode(&mut self, frame: Bytes) -> Result<Option<Record>> {
        // Strip CR/LF trailers.
        let mut end = frame.len();
        while end > 0 && (frame[end - 1] == b'\r' || frame[end - 1] == b'\n') {
            end -= 1;
        }
        let body = &frame[..end];
        if body.is_empty() {
            return Ok(None);
        }
        let text = String::from_utf8_lossy(body).into_owned();

        let mut fields = json!({});
        let mut tags: Vec<String> = Vec::new();

        if matches!(body.first(), Some(b'$' | b'!')) {
            // Split off optional checksum.
            let (payload, checksum_ok) = match body.iter().rposition(|&b| b == b'*') {
                Some(idx) if idx + 3 <= body.len() => {
                    let computed = xor_checksum(&body[1..idx]);
                    let stated_hex = &body[idx + 1..idx + 3];
                    let stated = u8::from_str_radix(
                        std::str::from_utf8(stated_hex).unwrap_or(""),
                        16,
                    )
                    .ok();
                    let ok = stated == Some(computed);
                    (&body[1..idx], Some(ok))
                }
                _ => (&body[1..], None::<bool>),
            };

            let payload_str = String::from_utf8_lossy(payload);
            let mut parts = payload_str.split(',');
            let header = parts.next().unwrap_or("");
            let (talker, message) = if header.len() >= 5 {
                (&header[..2], &header[2..])
            } else {
                ("", header)
            };
            let parts: Vec<String> = parts.map(str::to_owned).collect();

            fields = json!({
                "talker": talker,
                "message": message,
                "parts": parts,
                "checksum_ok": checksum_ok,
            });
            if checksum_ok == Some(false) {
                tags.push("checksum_mismatch".into());
            }
        } else {
            tags.push("not_nmea".into());
        }

        Ok(Some(Record {
            schema_id: None,
            level: None,
            text: Some(text),
            fields,
            tags,
            correlation_id: None,
        }))
    }

    fn kind(&self) -> &'static str {
        "nmea"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gprmc_with_valid_checksum() {
        // Real-world-ish GPRMC sentence with a known checksum.
        let s = b"$GPRMC,123519,A,4807.038,N,01131.000,E,022.4,084.4,230394,003.1,W*6A\r\n";
        let mut d = NmeaDecoder::new();
        let rec = d.decode(Bytes::from_static(s)).unwrap().unwrap();
        assert_eq!(rec.fields["talker"], json!("GP"));
        assert_eq!(rec.fields["message"], json!("RMC"));
        assert_eq!(rec.fields["checksum_ok"], json!(true));
        assert!(rec.tags.is_empty());
    }

    #[test]
    fn checksum_mismatch_is_tagged() {
        let s = b"$GPGGA,1,2,3*00";
        let mut d = NmeaDecoder::new();
        let rec = d.decode(Bytes::from_static(s)).unwrap().unwrap();
        assert_eq!(rec.fields["checksum_ok"], json!(false));
        assert!(rec.tags.contains(&"checksum_mismatch".to_string()));
    }

    #[test]
    fn non_nmea_is_tagged() {
        let mut d = NmeaDecoder::new();
        let rec = d.decode(Bytes::from_static(b"hello")).unwrap().unwrap();
        assert!(rec.tags.contains(&"not_nmea".to_string()));
    }

    #[test]
    fn empty_yields_none() {
        let mut d = NmeaDecoder::new();
        assert!(d.decode(Bytes::from_static(b"\r\n")).unwrap().is_none());
    }
}
