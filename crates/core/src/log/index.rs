//! `index.jsonl` — per-record envelope.
//!
//! See [`docs/protocols/log-format.md`](../../../../docs/protocols/log-format.md).
//!
//! The on-disk shape of [`IndexEntry`] is part of the v0.1 log layout.
//! Field additions must remain backwards-compatible (use
//! `#[serde(default)]` and `Option<_>`).

use std::fs::{File, OpenOptions};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::time::{ClockQuality, ClockSource, DualTimestamp};

/// Direction relative to the logging node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Dir {
    /// Inbound (received from peer / device).
    In,
    /// Outbound (sent by us).
    Out,
}

/// What kind of payload this row references in `raw.bin`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// Raw byte chunk.
    Bytes,
    /// One datagram.
    Datagram,
    /// One framed payload (post-framer, pre-decoder).
    Frame,
    /// One decoded record.
    Record,
}

/// One row of `index.jsonl`.
///
/// Mirrors the JSON Schema in `docs/protocols/log-format.md`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexEntry {
    /// RFC3339 ts_origin (with ns precision).
    pub ts_origin: String,
    /// RFC3339 ts_ingest (with ns precision).
    pub ts_ingest: String,
    /// Server monotonic ns.
    pub mono_ns: u64,
    /// Server boot UUID.
    pub boot_id: Uuid,
    /// Producing node UUID.
    pub node_id: Uuid,
    /// `node — server` offset in ms.
    pub clock_offset_ms: i32,
    /// Source-side clock quality.
    pub clock_quality: ClockQuality,
    /// Drift estimate, ppm.
    pub drift_ppm: f32,
    /// Where `ts_origin` came from.
    pub clock_source: ClockSource,
    /// Session UUID.
    pub sid: Uuid,
    /// Direction.
    pub dir: Dir,
    /// Payload kind.
    pub kind: Kind,
    /// Offset into `raw.bin`.
    pub off: u64,
    /// Length in `raw.bin`.
    pub len: u32,
    /// Optional severity.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub level: Option<String>,
    /// Optional tags.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub tags: Vec<String>,
    /// Optional correlation id.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub correlation_id: Option<String>,
    /// Optional source identifier (e.g. `"serial:COM3"`).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub source: Option<String>,
    /// Optional host name.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub host: Option<String>,
    /// Optional schema id.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub schema_id: Option<String>,
}

impl IndexEntry {
    /// Build an entry from a [`DualTimestamp`] envelope.
    #[must_use]
    pub fn from_envelope(
        ts: &DualTimestamp,
        sid: Uuid,
        dir: Dir,
        kind: Kind,
        off: u64,
        len: u32,
    ) -> Self {
        Self {
            ts_origin: format_rfc3339_ns(ts.ts_origin_ns),
            ts_ingest: format_rfc3339_ns(ts.ts_ingest_ns),
            mono_ns: ts.mono_ns,
            boot_id: ts.boot_id,
            node_id: ts.node_id,
            clock_offset_ms: ts.clock_offset_ms,
            clock_quality: ts.clock_quality,
            drift_ppm: ts.drift_ppm,
            clock_source: ts.clock_source,
            sid,
            dir,
            kind,
            off,
            len,
            level: None,
            tags: Vec::new(),
            correlation_id: None,
            source: None,
            host: None,
            schema_id: None,
        }
    }
}

/// Format `ns since UNIX epoch` as RFC3339 with nanosecond precision.
#[must_use]
pub fn format_rfc3339_ns(ns: i64) -> String {
    use time::format_description::well_known::Rfc3339;
    use time::OffsetDateTime;
    OffsetDateTime::from_unix_timestamp_nanos(i128::from(ns))
        .ok()
        .and_then(|dt| dt.format(&Rfc3339).ok())
        .unwrap_or_else(|| "1970-01-01T00:00:00Z".to_string())
}

/// Append-only writer for `index.jsonl`.
pub struct IndexWriter {
    path: PathBuf,
    inner: BufWriter<File>,
}

impl IndexWriter {
    /// Open (or create) `dir/index.jsonl` for appending.
    ///
    /// # Errors
    /// Returns the underlying `io::Error` on failure.
    pub fn create(dir: &Path) -> io::Result<Self> {
        let path = dir.join("index.jsonl");
        let f = OpenOptions::new().create(true).append(true).open(&path)?;
        Ok(Self {
            path,
            inner: BufWriter::new(f),
        })
    }

    /// Append one entry as a single JSON line.
    ///
    /// # Errors
    /// Returns `io::Error` for I/O failure or `serde_json` failure.
    pub fn append(&mut self, entry: &IndexEntry) -> io::Result<()> {
        serde_json::to_writer(&mut self.inner, entry)?;
        self.inner.write_all(b"\n")
    }

    /// Flush buffered writes to the OS.
    ///
    /// # Errors
    /// Returns the underlying `io::Error` on failure.
    pub fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }

    /// Path of the file being written.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_ts() -> DualTimestamp {
        DualTimestamp {
            ts_origin_ns: 1_700_000_000_000_000_000,
            ts_ingest_ns: 1_700_000_000_000_500_000,
            mono_ns: 42,
            boot_id: Uuid::nil(),
            node_id: Uuid::nil(),
            clock_offset_ms: 0,
            clock_quality: ClockQuality::Synced,
            drift_ppm: 0.0,
            clock_source: ClockSource::System,
        }
    }

    #[test]
    fn rfc3339_round_trip() {
        let s = format_rfc3339_ns(1_700_000_000_123_456_789);
        assert!(s.starts_with("2023-11-14T"));
        assert!(s.contains(".123456789"));
    }

    #[test]
    fn writes_one_line_per_entry() {
        let dir = tempdir();
        let mut w = IndexWriter::create(&dir).unwrap();
        let e = IndexEntry::from_envelope(&sample_ts(), Uuid::nil(), Dir::In, Kind::Bytes, 0, 16);
        w.append(&e).unwrap();
        w.append(&e).unwrap();
        w.flush().unwrap();
        let body = std::fs::read_to_string(dir.join("index.jsonl")).unwrap();
        assert_eq!(body.lines().count(), 2);
        for line in body.lines() {
            let v: serde_json::Value = serde_json::from_str(line).unwrap();
            assert_eq!(v["dir"], "in");
            assert_eq!(v["kind"], "bytes");
        }
    }

    fn tempdir() -> PathBuf {
        let p = std::env::temp_dir().join(format!("tracemux-index-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&p).unwrap();
        p
    }
}
