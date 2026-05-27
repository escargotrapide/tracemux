//! Source → `LogSink` ingest pipeline.
//!
//! v0.1 ships an in-memory bookkeeping struct ([`Ingest`]) that the
//! WSS / spawn paths use to register live channels. Wiring an actual
//! `Source + Framer + Decoder + LogSink` graph through a tokio task
//! is composed at the call site so the trait surface stays small
//! and stub-free.

use std::sync::Arc;

use bytes::Bytes;
use parking_lot::RwLock;
use uuid::Uuid;
use wanlogger_core::session::registry::{Registry, SessionState};

/// Counters per ingest task.
#[derive(Debug, Default, Clone)]
pub struct IngestStats {
    /// Frames received from the source.
    pub frames_in: u64,
    /// Bytes written to the log sink.
    pub bytes_logged: u64,
    /// Records dropped before delivery to a consumer.
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

    /// Register a live session and return its stable `sid`.
    pub fn register_session(&self, state: SessionState) -> Uuid {
        self.registry.insert_if_absent(state).sid
    }

    /// Publish one already-encoded wire frame to a session fan-out.
    ///
    /// This is the narrow v0.1 bridge from ingest tasks to WSS
    /// subscribers: the source/logging path owns schema construction,
    /// while the WebSocket path only forwards frozen wire bytes.
    pub fn publish_wire(&self, sid: Uuid, bytes: Bytes) -> Option<usize> {
        let byte_count = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
        let delivered = self.registry.get(&sid)?.fanout.publish(bytes);
        self.record_frame(sid, byte_count);
        Some(delivered)
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

    #[tokio::test]
    async fn publish_wire_reaches_session_fanout_and_records_stats() {
        let ig = Ingest::new();
        let sid = ig.register_session(SessionState::new("mock", "loopback"));
        let session = ig.registry.get(&sid).unwrap();
        let mut rx = session.fanout.subscribe();

        assert_eq!(
            ig.publish_wire(sid, Bytes::from_static(b"wire")).unwrap(),
            1
        );
        assert_eq!(rx.recv().await.unwrap(), Bytes::from_static(b"wire"));

        let stats = ig.stats(&sid).unwrap();
        assert_eq!(stats.frames_in, 1);
        assert_eq!(stats.bytes_logged, 4);
    }

    #[tokio::test]
    async fn register_session_preserves_existing_fanout_for_same_sid() {
        let ig = Ingest::new();
        let sid = ig.register_session(SessionState::new("mock", "first"));
        let session = ig.registry.get(&sid).unwrap();
        let mut rx = session.fanout.subscribe();

        let mut replacement = SessionState::new("mock", "restart");
        replacement.sid = sid;
        assert_eq!(ig.register_session(replacement), sid);

        assert_eq!(
            ig.publish_wire(sid, Bytes::from_static(b"again")).unwrap(),
            1
        );
        assert_eq!(rx.recv().await.unwrap(), Bytes::from_static(b"again"));
    }

    #[test]
    fn register_session_does_not_replace_existing_metadata_for_same_sid() {
        let ig = Ingest::new();
        let sid = ig.register_session(SessionState::new("mock", "first"));

        let mut replacement = SessionState::new("mock", "restart");
        replacement.sid = sid;
        assert_eq!(ig.register_session(replacement), sid);

        let session = ig.registry.get(&sid).unwrap();
        assert_eq!(session.iface, "first");
    }
}
