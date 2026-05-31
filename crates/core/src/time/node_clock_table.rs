//! Per-connection / per-node clock table.
//!
//! Persisted to `session-dir/clock-table.jsonl`. One row per
//! `clock_sync` exchange (every 30 s by default).

use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error_id::{ErrorId, TraceMuxError};

use super::ClockQuality;

/// One row of `clock-table.jsonl`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClockTableEntry {
    /// RFC3339 timestamp of measurement.
    pub ts: String,
    /// Producing node UUID.
    pub node_id: Uuid,
    /// Round-trip time, ms.
    pub rtt_ms: u32,
    /// Estimated `node - server` offset, ms.
    pub offset_ms: i32,
    /// Estimated drift, ppm.
    pub drift_ppm: f32,
    /// Quality estimate.
    pub quality: ClockQuality,
}

/// In-memory aggregate of the most recent measurement per node.
#[derive(Debug, Default)]
pub struct NodeClockTable {
    inner: Mutex<HashMap<Uuid, ClockTableEntry>>,
}

impl NodeClockTable {
    /// Construct an empty table.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a measurement, replacing any prior entry for the same
    /// node. Returns the entry that was previously stored (if any).
    pub fn upsert(&self, entry: ClockTableEntry) -> Option<ClockTableEntry> {
        self.inner.lock().insert(entry.node_id, entry)
    }

    /// Best-known offset (ms) for `node`. Returns `0` when unknown.
    #[must_use]
    pub fn offset_ms(&self, node: Uuid) -> i32 {
        self.inner.lock().get(&node).map_or(0, |e| e.offset_ms)
    }

    /// Snapshot the most recent entry per node.
    #[must_use]
    pub fn snapshot(&self) -> Vec<ClockTableEntry> {
        self.inner.lock().values().cloned().collect()
    }

    /// Number of nodes currently tracked.
    #[must_use]
    pub fn len(&self) -> usize {
        self.inner.lock().len()
    }

    /// `true` when no entries have been recorded.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.inner.lock().is_empty()
    }
}

/// Append `entry` as one JSON line to `path`. The file is created if
/// missing; the directory must already exist.
pub fn append_jsonl(path: impl AsRef<Path>, entry: &ClockTableEntry) -> Result<(), TraceMuxError> {
    let mut f = OpenOptions::new()
        .append(true)
        .create(true)
        .open(path.as_ref())
        .map_err(io_err)?;
    let mut line = serde_json::to_string(entry).map_err(json_err)?;
    line.push('\n');
    f.write_all(line.as_bytes()).map_err(io_err)?;
    Ok(())
}

/// Replay every line of `path` back into a fresh [`NodeClockTable`].
/// Malformed lines are skipped (caller can compare counts to detect
/// truncation).
pub fn load_jsonl(path: impl AsRef<Path>) -> Result<(NodeClockTable, usize), TraceMuxError> {
    let f = std::fs::File::open(path.as_ref()).map_err(io_err)?;
    let reader = BufReader::new(f);
    let table = NodeClockTable::new();
    let mut skipped = 0usize;
    for line in reader.lines() {
        let line = line.map_err(io_err)?;
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<ClockTableEntry>(&line) {
            Ok(e) => {
                table.upsert(e);
            }
            Err(_) => skipped += 1,
        }
    }
    Ok((table, skipped))
}

fn io_err(e: std::io::Error) -> TraceMuxError {
    TraceMuxError::new(
        ErrorId::E1001PipelineGeneric,
        format!("clock-table io: {e}"),
    )
    .with_source(e)
}

fn json_err(e: serde_json::Error) -> TraceMuxError {
    TraceMuxError::new(
        ErrorId::E1001PipelineGeneric,
        format!("clock-table json: {e}"),
    )
    .with_source(e)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(node: Uuid, offset_ms: i32) -> ClockTableEntry {
        ClockTableEntry {
            ts: "2026-04-29T00:00:00Z".into(),
            node_id: node,
            rtt_ms: 5,
            offset_ms,
            drift_ppm: 1.0,
            quality: ClockQuality::Synced,
        }
    }

    // REQ: FR-CORE-002
    #[test]
    fn upsert_replaces_per_node() {
        let t = NodeClockTable::new();
        let n = Uuid::new_v4();
        assert!(t.upsert(entry(n, 10)).is_none());
        let prev = t.upsert(entry(n, 20)).unwrap();
        assert_eq!(prev.offset_ms, 10);
        assert_eq!(t.offset_ms(n), 20);
        assert_eq!(t.len(), 1);
    }

    // REQ: FR-CORE-002
    #[test]
    fn offset_defaults_to_zero_for_unknown_node() {
        let t = NodeClockTable::new();
        assert_eq!(t.offset_ms(Uuid::new_v4()), 0);
        assert!(t.is_empty());
    }

    // REQ: FR-CORE-002
    #[test]
    fn jsonl_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("clock-table.jsonl");
        let n = Uuid::new_v4();
        append_jsonl(&p, &entry(n, 5)).unwrap();
        append_jsonl(&p, &entry(n, 7)).unwrap();
        let other = Uuid::new_v4();
        append_jsonl(&p, &entry(other, -3)).unwrap();

        let (t, skipped) = load_jsonl(&p).unwrap();
        assert_eq!(skipped, 0);
        // Latest write wins per node.
        assert_eq!(t.offset_ms(n), 7);
        assert_eq!(t.offset_ms(other), -3);
        assert_eq!(t.len(), 2);
    }

    // REQ: FR-CORE-002
    #[test]
    fn malformed_lines_are_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("clock-table.jsonl");
        std::fs::write(&p, "{not json}\n").unwrap();
        let (t, skipped) = load_jsonl(&p).unwrap();
        assert_eq!(skipped, 1);
        assert!(t.is_empty());
    }
}
