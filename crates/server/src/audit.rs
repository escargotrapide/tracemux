//! Audit log of write-back / control / auth events.
//!
//! One JSON object per event in `audit.jsonl`. Events are classified
//! by [`AuditKind`] and carry a stable [`crate::wire`]-compatible
//! envelope (`ts`, `actor`, `kind`, `target`, `result`, `detail`).

use std::fs::{File, OpenOptions};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

/// Stable kind tag for an audit event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AuditKind {
    /// Client connected / authenticated.
    Auth,
    /// Write-back was requested by a client.
    WriteBack,
    /// Session was opened / closed / rotated.
    Session,
    /// Configuration was reloaded.
    Config,
    /// Other / extension.
    Other,
}

/// Outcome of the audited operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AuditResult {
    /// Operation succeeded.
    Ok,
    /// Operation was denied (auth / policy).
    Denied,
    /// Operation failed.
    Error,
}

/// One row of `audit.jsonl`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEvent {
    /// RFC3339 timestamp (server wallclock).
    pub ts: String,
    /// Acting principal (client id, user, or `"system"`).
    pub actor: String,
    /// Kind of event.
    pub kind: AuditKind,
    /// Target identifier (sid, route, file, ...).
    pub target: String,
    /// Outcome.
    pub result: AuditResult,
    /// Free-form structured detail.
    #[serde(default, skip_serializing_if = "serde_json::Value::is_null")]
    pub detail: serde_json::Value,
}

/// Append-only audit log, internally synchronised.
pub struct AuditLog {
    path: PathBuf,
    inner: Mutex<BufWriter<File>>,
}

impl AuditLog {
    /// Open (or create) `dir/audit.jsonl` for appending.
    ///
    /// # Errors
    /// Returns the underlying `io::Error` on failure.
    pub fn create(dir: &Path) -> io::Result<Self> {
        let path = dir.join("audit.jsonl");
        let f = OpenOptions::new().create(true).append(true).open(&path)?;
        Ok(Self {
            path,
            inner: Mutex::new(BufWriter::new(f)),
        })
    }

    /// Append one event.
    ///
    /// # Errors
    /// Returns `io::Error` on serialisation, lock-poisoning, or write
    /// failure.
    pub fn append(&self, event: &AuditEvent) -> io::Result<()> {
        let mut g = self
            .inner
            .lock()
            .map_err(|_| io::Error::other("audit lock poisoned"))?;
        serde_json::to_writer(&mut *g, event)?;
        g.write_all(b"\n")?;
        g.flush()
    }

    /// Path of the audit file.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}
