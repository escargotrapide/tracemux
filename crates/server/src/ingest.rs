//! Source → `LogSink` ingest pipeline.
//!
//! v0.1 ships an in-memory bookkeeping struct ([`Ingest`]) that the
//! WSS / spawn paths use to register live channels. Wiring an actual
//! `Source + Framer + Decoder + LogSink` graph through a tokio task
//! is composed at the call site so the trait surface stays small
//! and stub-free.

use std::sync::Arc;

use parking_lot::RwLock;
use uuid::Uuid;
use wanlogger_core::session::registry::Registry;

/// Counters per ingest task.
#[derive(Debug, Default, Clone)]
pub struct IngestStats {
    /// Frames received from the source.
    pub frames_in: u64,
    /// Bytes written to the log sink.
    pub bytes_logged: u64,
    /// Records dropped (e.g. ring eviction).
    pub dropped: u64,
}

/// Server-wide ingest bookkeeping.
#[derive(Debug, Default)]
pub struct Ingest {
    /// Shared session registry.
    pub registry: Arc<Registry>,
    stats: RwLock<std::collections::HashMap<Uuid, IngestStats>>,
}

impl Ingest {
    /// Construct on a fresh registry.
    #[must_use]
    pub fn new() -> Self {
        Self {
            registry: Arc::new(Registry::new()),
            stats: RwLock::new(std::collections::HashMap::new()),
        }
    }

    /// Construct on a shared registry (so WSS / CLI / tests see the
    /// same set of sessions).
    #[must_use]
    pub fn with_registry(registry: Arc<Registry>) -> Self {
        Self {
            registry,
            stats: RwLock::new(std::collections::HashMap::new()),
        }
    }

    /// Note that one frame was ingested for `sid`.
    pub fn record_frame(&self, sid: Uuid, bytes: u64) {
        let mut m = self.stats.write();
        let e = m.entry(sid).or_default();
        e.frames_in += 1;
        e.bytes_logged += bytes;
    }

    /// Note that one record was dropped for `sid`.
    pub fn record_drop(&self, sid: Uuid, n: u64) {
        let mut m = self.stats.write();
        m.entry(sid).or_default().dropped += n;
    }

    /// Snapshot stats for `sid`.
    #[must_use]
    pub fn stats(&self, sid: &Uuid) -> Option<IngestStats> {
        self.stats.read().get(sid).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wanlogger_core::session::registry::SessionState;

    #[test]
    fn records_frames_and_drops() {
        let ig = Ingest::new();
        let sid = ig.registry.insert(SessionState::new("tcp", "127.0.0.1:1"));
        ig.record_frame(sid, 42);
        ig.record_frame(sid, 8);
        ig.record_drop(sid, 1);
        let s = ig.stats(&sid).unwrap();
        assert_eq!(s.frames_in, 2);
        assert_eq!(s.bytes_logged, 50);
        assert_eq!(s.dropped, 1);
    }

    #[test]
    fn unknown_sid_returns_none() {
        let ig = Ingest::new();
        assert!(ig.stats(&Uuid::nil()).is_none());
    }
}
