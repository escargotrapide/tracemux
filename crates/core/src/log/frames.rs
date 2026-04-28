//! `frames.jsonl` — structured records with `schema_id`.
//!
//! One JSON object per [`Record`] (post-decoder). Schema documents
//! referenced by `schema_id` live in `schemas/<id>.json`.

use std::fs::{File, OpenOptions};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::decoder::Record;

/// One row of `frames.jsonl`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrameEntry {
    /// RFC3339 ts_ingest with ns precision.
    pub ts: String,
    /// Decoder kind that produced this record (e.g. `"json-lines"`).
    pub decoder: String,
    /// The decoded record.
    pub record: Record,
}

/// Append-only writer for `frames.jsonl`.
pub struct FramesWriter {
    path: PathBuf,
    inner: BufWriter<File>,
}

impl FramesWriter {
    /// Open (or create) `dir/frames.jsonl` for appending.
    ///
    /// # Errors
    /// Returns the underlying `io::Error` on failure.
    pub fn create(dir: &Path) -> io::Result<Self> {
        let path = dir.join("frames.jsonl");
        let f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;
        Ok(Self {
            path,
            inner: BufWriter::new(f),
        })
    }

    /// Append one frame entry.
    ///
    /// # Errors
    /// Returns `io::Error` on serialisation or write failure.
    pub fn append(&mut self, entry: &FrameEntry) -> io::Result<()> {
        serde_json::to_writer(&mut self.inner, entry)?;
        self.inner.write_all(b"\n")
    }

    /// Flush buffered writes.
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

    fn tempdir() -> PathBuf {
        let p = std::env::temp_dir()
            .join(format!("wanlogger-frames-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn appends_records_as_jsonl() {
        let dir = tempdir();
        let mut w = FramesWriter::create(&dir).unwrap();
        let rec = Record {
            schema_id: Some("nmea:gprmc".to_string()),
            level: None,
            text: Some("$GPRMC,...".to_string()),
            fields: serde_json::json!({"talker":"GP"}),
            tags: vec![],
            correlation_id: None,
        };
        w.append(&FrameEntry {
            ts: "2024-01-01T00:00:00Z".to_string(),
            decoder: "nmea".to_string(),
            record: rec,
        })
        .unwrap();
        w.flush().unwrap();
        let body = std::fs::read_to_string(dir.join("frames.jsonl")).unwrap();
        let v: serde_json::Value = serde_json::from_str(body.trim()).unwrap();
        assert_eq!(v["decoder"], "nmea");
        assert_eq!(v["record"]["schema_id"], "nmea:gprmc");
    }
}
