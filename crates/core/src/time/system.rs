//! `SystemTime`-backed [`TimeSource`] (default in `tracemux serve`).
//!
//! Stub: full impl integrates `node_clock_table` for cross-node offsets.

use std::time::Instant;

use parking_lot::Mutex;
use uuid::Uuid;

use super::{unix_ns_now, ClockQuality, ClockSource, DualTimestamp, TimeSource};

/// Default [`TimeSource`] using OS wallclock + monotonic.
#[derive(Debug)]
pub struct SystemTimeSource {
    boot_id: Uuid,
    node_id: Uuid,
    mono_origin: Instant,
    last_offset_ms: Mutex<i32>,
}

impl SystemTimeSource {
    /// Construct with a given node id (loaded from config / keyring).
    #[must_use]
    pub fn new(node_id: Uuid) -> Self {
        Self {
            boot_id: Uuid::new_v4(),
            node_id,
            mono_origin: Instant::now(),
            last_offset_ms: Mutex::new(0),
        }
    }

    fn mono_ns(&self) -> u64 {
        u64::try_from(self.mono_origin.elapsed().as_nanos()).unwrap_or(u64::MAX)
    }
}

impl TimeSource for SystemTimeSource {
    fn stamp_origin(&self) -> DualTimestamp {
        let now = unix_ns_now();
        DualTimestamp {
            ts_origin_ns: now,
            ts_ingest_ns: now,
            mono_ns: self.mono_ns(),
            boot_id: self.boot_id,
            node_id: self.node_id,
            clock_offset_ms: 0,
            clock_quality: ClockQuality::BestEffort,
            drift_ppm: 0.0,
            clock_source: ClockSource::System,
        }
    }

    fn stamp_ingest(&self, mut origin: DualTimestamp) -> DualTimestamp {
        origin.ts_ingest_ns = unix_ns_now();
        origin.mono_ns = self.mono_ns();
        origin.boot_id = self.boot_id;
        origin.clock_offset_ms = *self.last_offset_ms.lock();
        origin
    }

    fn boot_id(&self) -> Uuid {
        self.boot_id
    }

    fn node_id(&self) -> Uuid {
        self.node_id
    }
}
