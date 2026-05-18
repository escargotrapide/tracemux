use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::Path;
use std::sync::Mutex;

use anyhow::{Context, Result};
use serde::Serialize;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Direction {
    In,
    Out,
    Event,
}

#[derive(Debug)]
pub(crate) struct Transcript {
    writer: Option<Mutex<BufWriter<File>>>,
}

#[derive(Debug, Serialize)]
struct TranscriptRow<'a> {
    ts: String,
    transport: &'a str,
    direction: Direction,
    #[serde(skip_serializing_if = "Option::is_none")]
    peer: Option<&'a str>,
    len: usize,
    bytes_hex: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    event: Option<&'a str>,
}

impl Transcript {
    pub(crate) fn open(path: Option<&Path>) -> Result<Self> {
        let Some(path) = path else {
            return Ok(Self { writer: None });
        };
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("creating transcript dir {}", parent.display()))?;
            }
        }
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .with_context(|| format!("opening transcript {}", path.display()))?;
        Ok(Self {
            writer: Some(Mutex::new(BufWriter::new(file))),
        })
    }

    #[cfg(test)]
    pub(crate) const fn disabled() -> Self {
        Self { writer: None }
    }

    pub(crate) fn record_bytes(
        &self,
        transport: &str,
        direction: Direction,
        peer: Option<&str>,
        bytes: &[u8],
    ) -> Result<()> {
        self.write_row(&TranscriptRow {
            ts: timestamp(),
            transport,
            direction,
            peer,
            len: bytes.len(),
            bytes_hex: hex(bytes),
            text: std::str::from_utf8(bytes).ok().map(ToString::to_string),
            event: None,
        })
    }

    pub(crate) fn record_event(
        &self,
        transport: &str,
        peer: Option<&str>,
        event: &str,
    ) -> Result<()> {
        self.write_row(&TranscriptRow {
            ts: timestamp(),
            transport,
            direction: Direction::Event,
            peer,
            len: 0,
            bytes_hex: String::new(),
            text: None,
            event: Some(event),
        })
    }

    fn write_row(&self, row: &TranscriptRow<'_>) -> Result<()> {
        let Some(writer) = &self.writer else {
            return Ok(());
        };
        let mut guard = writer
            .lock()
            .map_err(|_| anyhow::anyhow!("transcript lock poisoned"))?;
        serde_json::to_writer(&mut *guard, &row).context("writing transcript row")?;
        guard
            .write_all(b"\n")
            .context("writing transcript newline")?;
        guard.flush().context("flushing transcript")
    }
}

fn timestamp() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
}

fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        out.push(char::from(HEX[usize::from(byte >> 4)]));
        out.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_jsonl_transcript_rows() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("transcript.jsonl");
        let transcript = Transcript::open(Some(&path)).unwrap();

        transcript
            .record_bytes("tcp", Direction::Out, Some("peer"), b"hi")
            .unwrap();
        transcript
            .record_event("tcp", Some("peer"), "connected")
            .unwrap();

        let body = std::fs::read_to_string(path).unwrap();
        let rows: Vec<_> = body.lines().collect();
        assert_eq!(rows.len(), 2);
        assert!(rows[0].contains("\"bytes_hex\":\"6869\""));
        assert!(rows[1].contains("\"event\":\"connected\""));
    }
}
