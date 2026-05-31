//! Time, monotonic clock, and dual-timestamp envelope.
//!
//! See [`docs/protocols/timestamp.md`](../../../../docs/protocols/timestamp.md).
//!
//! **Frozen v0.1.** Changing [`DualTimestamp`] or [`TimeSource`]
//! requires an ADR.

use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Quality of the source-side clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ClockQuality {
    /// Synchronised to a reference (NTP / PTP / GPS).
    Synced,
    /// Unsynchronised but stable / locally consistent.
    BestEffort,
    /// Unknown quality.
    Unknown,
    /// Imported from a foreign artefact.
    Imported,
}

/// Where a timestamp came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ClockSource {
    /// OS wallclock.
    System,
    /// NTP-disciplined wallclock.
    Ntp,
    /// PTP-disciplined wallclock.
    Ptp,
    /// Server monotonic only (no wallclock).
    Monotonic,
    /// Imported from a foreign artefact.
    Imported,
}

/// Dual timestamp + clock metadata attached to every record and frame.
///
/// **Frozen v0.1.** Field order/types are part of the on-disk and wire
/// schemas.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DualTimestamp {
    /// Best-known time at the source (ns since UNIX epoch).
    pub ts_origin_ns: i64,
    /// Time at the server when the record was received (ns since UNIX epoch).
    pub ts_ingest_ns: i64,
    /// Server-side monotonic ns; immune to wallclock jumps.
    pub mono_ns: u64,
    /// Server boot UUID — resets when monotonic does.
    pub boot_id: Uuid,
    /// Producing-node UUID.
    pub node_id: Uuid,
    /// Estimated `node.wallclock — server.wallclock` (ms).
    pub clock_offset_ms: i32,
    /// Quality of the source-side clock.
    pub clock_quality: ClockQuality,
    /// Estimated drift in ppm.
    pub drift_ppm: f32,
    /// Where `ts_origin_ns` came from.
    pub clock_source: ClockSource,
}

/// Trait that supplies the dual-timestamp envelope.
///
/// **Frozen v0.1.** Implementations live under
/// [`crate::time::system`], [`crate::time::monotonic`], and
/// [`crate::time::clock_sync`].
pub trait TimeSource: Send + Sync + 'static {
    /// Stamp `ts_origin` for a record originating *now* on this node.
    fn stamp_origin(&self) -> DualTimestamp;

    /// Stamp `ts_ingest` for a record arriving *now* at the server.
    /// `origin` carries the source-side fields; this fills in
    /// `ts_ingest_ns`, `mono_ns`, `boot_id` and updates
    /// `clock_offset_ms` from the [`crate::time::node_clock_table`].
    fn stamp_ingest(&self, origin: DualTimestamp) -> DualTimestamp;

    /// Server boot UUID. Stable for the lifetime of the server process.
    fn boot_id(&self) -> Uuid;

    /// Local node UUID. Persisted in the OS keyring or config.
    fn node_id(&self) -> Uuid;
}

/// Convenience: ns since UNIX epoch using the OS wallclock.
#[must_use]
pub fn unix_ns_now() -> i64 {
    let d = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    // i64 is fine until year 2262.
    i64::try_from(d.as_nanos()).unwrap_or(i64::MAX)
}

pub mod clock_sync;
pub mod monotonic;
pub mod node_clock_table;
pub mod system;
