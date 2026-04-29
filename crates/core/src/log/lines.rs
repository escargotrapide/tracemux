//! `lines.jsonl` — decoded text lines.
//!
//! One JSON object per record with `ts_ingest`, optional `level`,
//! and the decoded `text`. Used by the UI's "lines" panel.

use std::fs::{File, OpenOptions};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// One row of `lines.jsonl`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LineEntry {
    /// RFC3339 ts_ingest with ns precision.
    pub ts: String,
    /// Optional severity level (lowercase).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub level: Option<String>,
    /// Decoded text body (may contain embedded newlines).
    pub text: String,
    /// Optional correlation id.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub correlation_id: Option<String>,
    /// Optional tags.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub tags: Vec<String>,
}

/// Append-only writer for `lines.jsonl`.
pub struct LinesWriter {
    path: PathBuf,
    inner: BufWriter<File>,
}

impl LinesWriter {
    /// Open (or create) `dir/lines.jsonl` for appending.
    ///
    /// # Errors
    /// Returns the underlying `io::Error` on failure.
    pub fn create(dir: &Path) -> io::Result<Self> {
        let path = dir.join("lines.jsonl");
        let f = OpenOptions::new().create(true).append(true).open(&path)?;
        Ok(Self {
            path,
            inner: BufWriter::new(f),
        })
    }

    /// Append one line entry.
    ///
    /// # Errors
    /// Returns `io::Error` on serialisation or write failure.
    pub fn append(&mut self, entry: &LineEntry) -> io::Result<()> {
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
        let p = std::env::temp_dir().join(format!("wanlogger-lines-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn appends_jsonl() {
        let dir = tempdir();
        let mut w = LinesWriter::create(&dir).unwrap();
        w.append(&LineEntry {
            ts: "2023-01-01T00:00:00Z".to_string(),
            level: Some("info".to_string()),
            text: "hello".to_string(),
            correlation_id: None,
            tags: vec![],
        })
        .unwrap();
        w.append(&LineEntry {
            ts: "2023-01-01T00:00:01Z".to_string(),
            level: None,
            text: "world".to_string(),
            correlation_id: None,
            tags: vec!["x".to_string()],
        })
        .unwrap();
        w.flush().unwrap();
        let body = std::fs::read_to_string(dir.join("lines.jsonl")).unwrap();
        assert_eq!(body.lines().count(), 2);
        let v0: serde_json::Value = serde_json::from_str(body.lines().next().unwrap()).unwrap();
        assert_eq!(v0["text"], "hello");
        assert_eq!(v0["level"], "info");
    }
}
