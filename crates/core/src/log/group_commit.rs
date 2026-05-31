//! Group-commit fsync (every `commit_window_ms` or `commit_size_kib`).
//! **Critical path.**
//!
//! [`GroupCommit`] is a thin scheduler wrapped around a [`WalWriter`].
//! It buffers `append` calls and triggers an fsync when **either**
//! threshold is met, whichever first:
//!
//! - the in-flight (un-synced) byte count crosses `commit_size_kib`, or
//! - `commit_window_ms` has elapsed since the last successful fsync.
//!
//! The window check is computed against an injectable "now" so tests
//! are deterministic. In production the caller passes
//! `Instant::now()` from `tokio::time::interval` or similar.

use std::time::{Duration, Instant};

use crate::error_id::TraceMuxError;

use super::wal::WalWriter;

/// Thresholds copied from `docs/protocols/log-format.md`.
#[derive(Debug, Clone, Copy)]
pub struct CommitPolicy {
    /// Maximum time between fsyncs.
    pub window: Duration,
    /// Maximum un-synced bytes before forcing an fsync.
    pub size_bytes: u64,
}

impl Default for CommitPolicy {
    fn default() -> Self {
        Self {
            window: Duration::from_millis(50),
            size_bytes: 256 * 1024,
        }
    }
}

/// Group-commit scheduler around a [`WalWriter`].
#[derive(Debug)]
pub struct GroupCommit {
    wal: WalWriter,
    policy: CommitPolicy,
    in_flight: u64,
    last_sync: Instant,
}

/// Result of a single `append` call -- whether it triggered an fsync.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommitOutcome {
    /// Bytes were appended; the buffered total has not yet hit the
    /// thresholds, so no fsync was performed.
    Buffered,
    /// Bytes were appended and an fsync was performed before returning.
    Synced,
}

impl GroupCommit {
    /// Wrap a [`WalWriter`] with the given policy.
    #[must_use]
    pub fn new(wal: WalWriter, policy: CommitPolicy) -> Self {
        Self {
            wal,
            policy,
            in_flight: 0,
            last_sync: Instant::now(),
        }
    }

    /// Append `payload` and possibly fsync. `now` lets callers / tests
    /// inject the current instant for the window check.
    pub fn append_at(
        &mut self,
        payload: &[u8],
        now: Instant,
    ) -> Result<(u64, CommitOutcome), TraceMuxError> {
        let off = self.wal.append(payload)?;
        self.in_flight = self
            .in_flight
            .saturating_add(payload.len() as u64 + 8 /* len + crc */);

        if self.should_sync(now) {
            self.wal.sync()?;
            self.in_flight = 0;
            self.last_sync = now;
            Ok((off, CommitOutcome::Synced))
        } else {
            Ok((off, CommitOutcome::Buffered))
        }
    }

    /// Convenience that uses [`Instant::now`].
    pub fn append(&mut self, payload: &[u8]) -> Result<(u64, CommitOutcome), TraceMuxError> {
        self.append_at(payload, Instant::now())
    }

    /// Force an fsync even if no thresholds were crossed (e.g. on
    /// shutdown).
    pub fn flush(&mut self) -> Result<(), TraceMuxError> {
        self.wal.sync()?;
        self.in_flight = 0;
        self.last_sync = Instant::now();
        Ok(())
    }

    /// Tick from a timer; performs an fsync if the window has elapsed
    /// and there is buffered data.
    pub fn tick(&mut self, now: Instant) -> Result<bool, TraceMuxError> {
        if self.in_flight == 0 {
            return Ok(false);
        }
        if now.duration_since(self.last_sync) >= self.policy.window {
            self.wal.sync()?;
            self.in_flight = 0;
            self.last_sync = now;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn should_sync(&self, now: Instant) -> bool {
        self.in_flight >= self.policy.size_bytes
            || now.duration_since(self.last_sync) >= self.policy.window
    }

    /// Bytes appended since the last fsync.
    #[must_use]
    pub fn in_flight(&self) -> u64 {
        self.in_flight
    }

    /// Borrow the underlying WAL (read-only access).
    #[must_use]
    pub fn wal(&self) -> &WalWriter {
        &self.wal
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh() -> (tempfile::TempDir, GroupCommit) {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("raw.wal");
        let wal = WalWriter::open(&p).unwrap();
        let gc = GroupCommit::new(
            wal,
            CommitPolicy {
                window: Duration::from_millis(50),
                size_bytes: 32,
            },
        );
        (dir, gc)
    }

    // REQ: FR-LOG-002
    #[test]
    fn buffers_until_size_threshold() {
        let (_d, mut gc) = fresh();
        let now = Instant::now();
        let (_, out) = gc.append_at(b"abc", now).unwrap();
        assert_eq!(out, CommitOutcome::Buffered);
        assert!(gc.in_flight() > 0);
    }

    // REQ: FR-LOG-002
    #[test]
    fn size_threshold_triggers_sync() {
        let (_d, mut gc) = fresh();
        let now = Instant::now();
        // policy.size_bytes = 32. 8 (header) + 30 (payload) = 38 -> sync.
        let (_, out) = gc.append_at(&[0u8; 30], now).unwrap();
        assert_eq!(out, CommitOutcome::Synced);
        assert_eq!(gc.in_flight(), 0);
    }

    // REQ: FR-LOG-002
    #[test]
    fn window_threshold_triggers_sync() {
        let (_d, mut gc) = fresh();
        let t0 = Instant::now();
        let (_, first) = gc.append_at(b"x", t0).unwrap();
        assert_eq!(first, CommitOutcome::Buffered);
        let t1 = t0 + Duration::from_millis(100);
        let (_, second) = gc.append_at(b"y", t1).unwrap();
        assert_eq!(second, CommitOutcome::Synced);
    }

    // REQ: FR-LOG-002
    #[test]
    fn tick_flushes_after_window() {
        let (_d, mut gc) = fresh();
        let t0 = Instant::now();
        gc.append_at(b"x", t0).unwrap();
        let synced = gc.tick(t0 + Duration::from_millis(1)).unwrap();
        assert!(!synced);
        let synced = gc.tick(t0 + Duration::from_millis(60)).unwrap();
        assert!(synced);
    }

    // REQ: FR-LOG-002
    #[test]
    fn tick_is_noop_when_idle() {
        let (_d, mut gc) = fresh();
        let synced = gc.tick(Instant::now()).unwrap();
        assert!(!synced);
    }

    // REQ: FR-LOG-002
    #[test]
    fn flush_forces_sync() {
        let (_d, mut gc) = fresh();
        gc.append_at(b"x", Instant::now()).unwrap();
        gc.flush().unwrap();
        assert_eq!(gc.in_flight(), 0);
    }
}
