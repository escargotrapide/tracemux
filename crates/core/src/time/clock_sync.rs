//! WSS `clock_sync` exchange. Stub for v0.1.
//!
//! Implements Cristian-style offset estimation:
//!
//! ```text
//!   offset = ((t2 - t1) + (t3 - t4)) / 2
//!   rtt    = (t4 - t1) - (t3 - t2)
//! ```
//!
//! See `docs/protocols/timestamp.md` for the full derivation.

use serde::{Deserialize, Serialize};

/// `clock_sync` request payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClockSyncRequest {
    /// Client wallclock at send (ns since UNIX epoch).
    pub t1_ns: i64,
}

/// `clock_sync` reply payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClockSyncReply {
    /// Echoed `t1_ns`.
    pub t1_ns: i64,
    /// Server wallclock at receive (ns).
    pub t2_ns: i64,
    /// Server wallclock at send (ns).
    pub t3_ns: i64,
}

/// Compute offset (ms) and rtt (ms) given the four timestamps.
#[must_use]
pub fn estimate_offset_rtt_ms(t1_ns: i64, t2_ns: i64, t3_ns: i64, t4_ns: i64) -> (i32, u32) {
    // Cristian-style: offset = ((t2 - t1) + (t3 - t4)) / 2; rtt = (t4 - t1) - (t3 - t2).
    let offset_nanos = i64::midpoint(t2_ns - t1_ns, t3_ns - t4_ns);
    let rtt_nanos = (t4_ns - t1_ns) - (t3_ns - t2_ns);
    let offset = i32::try_from(offset_nanos / 1_000_000).unwrap_or(i32::MAX);
    let rtt = u32::try_from(rtt_nanos.max(0) / 1_000_000).unwrap_or(u32::MAX);
    (offset, rtt)
}

#[cfg(test)]
mod tests {
    use super::estimate_offset_rtt_ms;

    // REQ: FR-CORE-002 (dual timestamps + clock sync)
    #[test]
    fn offset_zero_when_clocks_aligned() {
        // Symmetric path, no skew: t1=0, t2=10ms, t3=15ms, t4=25ms.
        // offset = ((10 - 0) + (15 - 25)) / 2 = 0; rtt = 25 - 5 = 20ms.
        let (offset, rtt) = estimate_offset_rtt_ms(0, 10_000_000, 15_000_000, 25_000_000);
        assert_eq!(offset, 0);
        assert_eq!(rtt, 20);
    }

    #[test]
    fn offset_positive_when_server_ahead() {
        // Server clock is +100 ms ahead. t1=0, t2=110ms (server), t3=115ms,
        // t4=25ms (client). offset = ((110 - 0) + (115 - 25)) / 2 = 100.
        let (offset, rtt) = estimate_offset_rtt_ms(0, 110_000_000, 115_000_000, 25_000_000);
        assert_eq!(offset, 100);
        assert_eq!(rtt, 20);
    }

    #[test]
    fn rtt_clamped_to_zero_on_negative() {
        let (_, rtt) = estimate_offset_rtt_ms(100, 50, 60, 0);
        assert_eq!(rtt, 0);
    }
}
