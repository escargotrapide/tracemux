//! Session rotation. **Critical path.**
//!
//! Two pieces:
//!
//! 1. [`RotatePolicy`] / [`RotateStats`] / [`should_rotate`] -- pure
//!    decision logic, fully deterministic and unit-testable.
//! 2. [`format_session_dirname`] -- canonical session-dir naming per
//!    `docs/protocols/log-format.md` ("Directory naming").
//!
//! Closing the current dir and opening the next one is performed by
//! the session manager (`crate::session`). This module only answers
//! the question "is it time?".

use std::time::Duration;

use time::format_description::FormatItem;
use time::macros::format_description;
use time::OffsetDateTime;

/// User-configured rotation thresholds.
#[derive(Debug, Clone, Copy)]
pub struct RotatePolicy {
    /// Rotate when `raw.bin` exceeds this many bytes. `None` = no
    /// size-based rotation.
    pub size_bytes: Option<u64>,
    /// Rotate when the current session has been open this long.
    /// `None` = no time-based rotation.
    pub duration: Option<Duration>,
}

impl Default for RotatePolicy {
    fn default() -> Self {
        Self {
            size_bytes: Some(256 * 1024 * 1024),
            duration: Some(Duration::from_secs(60 * 60 * 24)),
        }
    }
}

/// Live counters that feed the rotation decision.
#[derive(Debug, Clone, Copy, Default)]
pub struct RotateStats {
    /// Current `raw.bin` size in bytes.
    pub size_bytes: u64,
    /// Time since the session was opened.
    pub age: Duration,
}

/// Pure decision: should the current session rotate now?
#[must_use]
pub fn should_rotate(stats: &RotateStats, policy: &RotatePolicy) -> bool {
    if let Some(limit) = policy.size_bytes {
        if stats.size_bytes >= limit {
            return true;
        }
    }
    if let Some(limit) = policy.duration {
        if stats.age >= limit {
            return true;
        }
    }
    false
}

/// Format a session-dir name per `docs/protocols/log-format.md`:
/// `{prefix}_{kind}_{iface}_{YYYYMMDD-HHMMSS}`.
///
/// `iface` is sanitised: any character outside `[A-Za-z0-9._-]` becomes
/// `_`. An empty `iface` becomes `unknown`.
#[must_use]
pub fn format_session_dirname(
    prefix: &str,
    kind: &str,
    iface: &str,
    now: OffsetDateTime,
) -> String {
    const FMT: &[FormatItem<'_>] = format_description!("[year][month][day]-[hour][minute][second]");
    let ts = now.format(FMT).unwrap_or_else(|_| "00000000-000000".into());
    let safe_iface = sanitize_iface(iface);
    format!("{prefix}_{kind}_{safe_iface}_{ts}")
}

fn sanitize_iface(iface: &str) -> String {
    if iface.is_empty() {
        return "unknown".into();
    }
    iface
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::datetime;

    // REQ: FR-LOG-001
    #[test]
    fn no_rotation_when_under_limits() {
        let p = RotatePolicy {
            size_bytes: Some(1024),
            duration: Some(Duration::from_secs(60)),
        };
        let s = RotateStats {
            size_bytes: 100,
            age: Duration::from_secs(1),
        };
        assert!(!should_rotate(&s, &p));
    }

    // REQ: FR-LOG-001
    #[test]
    fn size_threshold_triggers() {
        let p = RotatePolicy {
            size_bytes: Some(1024),
            duration: None,
        };
        let s = RotateStats {
            size_bytes: 1024,
            age: Duration::ZERO,
        };
        assert!(should_rotate(&s, &p));
    }

    // REQ: FR-LOG-001
    #[test]
    fn duration_threshold_triggers() {
        let p = RotatePolicy {
            size_bytes: None,
            duration: Some(Duration::from_secs(60)),
        };
        let s = RotateStats {
            size_bytes: 0,
            age: Duration::from_secs(60),
        };
        assert!(should_rotate(&s, &p));
    }

    // REQ: FR-LOG-001
    #[test]
    fn either_threshold_triggers() {
        let p = RotatePolicy {
            size_bytes: Some(10),
            duration: Some(Duration::from_secs(10)),
        };
        let big = RotateStats {
            size_bytes: 1000,
            age: Duration::ZERO,
        };
        let old = RotateStats {
            size_bytes: 0,
            age: Duration::from_secs(20),
        };
        assert!(should_rotate(&big, &p));
        assert!(should_rotate(&old, &p));
    }

    // REQ: FR-LOG-001
    #[test]
    fn dirname_matches_spec() {
        let now = datetime!(2026-04-29 10:11:12 UTC);
        let n = format_session_dirname("wanlogger", "serial", "COM3", now);
        assert_eq!(n, "wanlogger_serial_COM3_20260429-101112");
    }

    // REQ: FR-LOG-001
    #[test]
    fn dirname_sanitises_iface() {
        let now = datetime!(2026-04-29 00:00:00 UTC);
        let n = format_session_dirname("wl", "tcp", "10.0.0.1:9000", now);
        assert_eq!(n, "wl_tcp_10.0.0.1_9000_20260429-000000");
    }

    // REQ: FR-LOG-001
    #[test]
    fn dirname_handles_empty_iface() {
        let now = datetime!(2026-04-29 00:00:00 UTC);
        let n = format_session_dirname("wl", "udp", "", now);
        assert_eq!(n, "wl_udp_unknown_20260429-000000");
    }
}
