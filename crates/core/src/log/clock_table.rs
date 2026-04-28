//! `clock-table.jsonl` writer.
//!
//! Persists periodic [`ClockTableEntry`] rows for multi-node clock
//! reconciliation. See [`crate::time::node_clock_table`].

use std::fs::{File, OpenOptions};
use std::io::{self, BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};

use crate::time::node_clock_table::ClockTableEntry;

/// Append-only writer for `clock-table.jsonl`.
pub struct ClockTableWriter {
    path: PathBuf,
    inner: BufWriter<File>,
}

impl ClockTableWriter {
    /// Open (or create) `dir/clock-table.jsonl` for appending.
    ///
    /// # Errors
    /// Returns the underlying `io::Error` on failure.
    pub fn create(dir: &Path) -> io::Result<Self> {
        let path = dir.join("clock-table.jsonl");
        let f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;
        Ok(Self {
            path,
            inner: BufWriter::new(f),
        })
    }

    /// Append one clock-table entry.
    ///
    /// # Errors
    /// Returns `io::Error` on serialisation or write failure.
    pub fn append(&mut self, entry: &ClockTableEntry) -> io::Result<()> {
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

/// Read all rows from `dir/clock-table.jsonl`.
///
/// # Errors
/// Returns `io::Error` on read or parse failure.
pub fn read_all(dir: &Path) -> io::Result<Vec<ClockTableEntry>> {
    let path = dir.join("clock-table.jsonl");
    let f = File::open(&path)?;
    let r = BufReader::new(f);
    let mut out = Vec::new();
    for line in r.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let entry: ClockTableEntry = serde_json::from_str(&line)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        out.push(entry);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::time::ClockQuality;
    use uuid::Uuid;

    fn tempdir() -> PathBuf {
        let p = std::env::temp_dir()
            .join(format!("wanlogger-ct-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn round_trip() {
        let dir = tempdir();
        let mut w = ClockTableWriter::create(&dir).unwrap();
        let e = ClockTableEntry {
            ts: "2024-01-01T00:00:00Z".to_string(),
            node_id: Uuid::nil(),
            rtt_ms: 12,
            offset_ms: -3,
            drift_ppm: 1.5,
            quality: ClockQuality::Synced,
        };
        w.append(&e).unwrap();
        w.append(&e).unwrap();
        w.flush().unwrap();
        let rows = read_all(&dir).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].rtt_ms, 12);
        assert_eq!(rows[0].offset_ms, -3);
    }
}
