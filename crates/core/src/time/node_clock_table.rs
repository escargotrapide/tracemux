//! Per-connection / per-node clock table.
//!
//! Persisted to `session-dir/clock-table.jsonl`. Stub for v0.1.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

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
    /// Estimated `node — server` offset, ms.
    pub offset_ms: i32,
    /// Estimated drift, ppm.
    pub drift_ppm: f32,
    /// Quality estimate.
    pub quality: ClockQuality,
}
