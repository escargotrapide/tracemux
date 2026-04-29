//! UI-side `clientlog` ingest forwarded to the server logger.
//!
//! Browsers / Tauri / CLI clients POST their JS / Rust errors here so
//! the server can persist them next to the session log. One JSON
//! object per line in `clientlog.jsonl`.

use std::fs::{File, OpenOptions};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

/// Severity advertised by the client.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ClientLevel {
    /// Debug.
    Debug,
    /// Informational.
    Info,
    /// Warning.
    Warn,
    /// Error.
    Error,
}

/// One row of `clientlog.jsonl`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientLogEntry {
    /// RFC3339 timestamp from the client (best-effort).
    pub ts: String,
    /// Originating component (`"web"`, `"tauri"`, `"cli"`, ...).
    pub origin: String,
    /// Severity.
    pub level: ClientLevel,
    /// Human-readable message.
    pub message: String,
    /// Optional stack / structured detail.
    #[serde(default, skip_serializing_if = "serde_json::Value::is_null")]
    pub detail: serde_json::Value,
}

/// Append-only client-log writer.
pub struct ClientLog {
    path: PathBuf,
    inner: Mutex<BufWriter<File>>,
}

impl ClientLog {
    /// Open (or create) `dir/clientlog.jsonl` for appending.
    ///
    /// # Errors
    /// Returns the underlying `io::Error` on failure.
    pub fn create(dir: &Path) -> io::Result<Self> {
        let path = dir.join("clientlog.jsonl");
        let f = OpenOptions::new().create(true).append(true).open(&path)?;
        Ok(Self {
            path,
            inner: Mutex::new(BufWriter::new(f)),
        })
    }

    /// Append one entry.
    ///
    /// # Errors
    /// Returns `io::Error` on serialisation, lock-poisoning, or write
    /// failure.
    pub fn append(&self, entry: &ClientLogEntry) -> io::Result<()> {
        let mut g = self
            .inner
            .lock()
            .map_err(|_| io::Error::other("clientlog lock poisoned"))?;
        serde_json::to_writer(&mut *g, entry)?;
        g.write_all(b"\n")?;
        g.flush()
    }

    /// Path of the file being written.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}
