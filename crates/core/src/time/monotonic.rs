//! Monotonic-only [`TimeSource`] for hostile / clock-less environments.
//! Stub for v0.1.

use std::time::Instant;

use uuid::Uuid;

use super::{ClockQuality, ClockSource, DualTimestamp, TimeSource};

/// Monotonic-only time source. `ts_origin_ns == ts_ingest_ns == 0`.
#[derive(Debug)]
pub struct MonotonicTimeSource {
    boot_id: Uuid,
    node_id: Uuid,
    origin: Instant,
}

impl MonotonicTimeSource {
    /// Construct.
    #[must_use]
    pub fn new(node_id: Uuid) -> Self {
        Self {
            boot_id: Uuid::new_v4(),
            node_id,
            origin: Instant::now(),
        }
    }

    fn mono_ns(&self) -> u64 {
        u64::try_from(self.origin.elapsed().as_nanos()).unwrap_or(u64::MAX)
    }
}

impl TimeSource for MonotonicTimeSource {
    fn stamp_origin(&self) -> DualTimestamp {
        DualTimestamp {
            ts_origin_ns: 0,
            ts_ingest_ns: 0,
            mono_ns: self.mono_ns(),
            boot_id: self.boot_id,
            node_id: self.node_id,
            clock_offset_ms: 0,
            clock_quality: ClockQuality::Unknown,
            drift_ppm: 0.0,
            clock_source: ClockSource::Monotonic,
        }
    }

    fn stamp_ingest(&self, mut origin: DualTimestamp) -> DualTimestamp {
        origin.mono_ns = self.mono_ns();
        origin.boot_id = self.boot_id;
        origin
    }

    fn boot_id(&self) -> Uuid {
        self.boot_id
    }

    fn node_id(&self) -> Uuid {
        self.node_id
    }
}
