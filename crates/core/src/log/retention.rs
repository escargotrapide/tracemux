//! Retention policy.
//!
//! Deletes session subdirectories under a parent dir whose timestamp
//! suffix is older than `keep_days` days. Session-dir naming follows
//! `docs/protocols/log-format.md`:
//!
//! ```text
//! {prefix}_{kind}_{iface}_{YYYYMMDD-HHMMSS}/
//! ```

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use time::{Duration, OffsetDateTime, PrimitiveDateTime};

/// Retention policy.
#[derive(Debug, Clone, Copy)]
pub struct RetentionPolicy {
    /// Sessions older than this many days are deleted.
    pub keep_days: u32,
}

/// Result of a retention pass.
#[derive(Debug, Default)]
pub struct RetentionReport {
    /// Paths that were deleted.
    pub deleted: Vec<PathBuf>,
    /// Paths that were kept.
    pub kept: Vec<PathBuf>,
    /// Paths whose timestamp could not be parsed and were left alone.
    pub skipped: Vec<PathBuf>,
}

impl RetentionPolicy {
    /// Apply the policy to `parent`, using `now` as the reference
    /// time. Returns a report; partial failures are ignored except
    /// for reading `parent` itself.
    ///
    /// # Errors
    /// Returns `io::Error` if `parent` cannot be read.
    pub fn apply(&self, parent: &Path, now: OffsetDateTime) -> io::Result<RetentionReport> {
        let mut report = RetentionReport::default();
        let cutoff = now - Duration::days(i64::from(self.keep_days));
        for entry in fs::read_dir(parent)? {
            let entry = entry?;
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let name = match path.file_name().and_then(|s| s.to_str()) {
                Some(n) => n.to_string(),
                None => {
                    report.skipped.push(path);
                    continue;
                }
            };
            match parse_session_timestamp(&name) {
                Some(ts) if ts < cutoff => {
                    if fs::remove_dir_all(&path).is_ok() {
                        report.deleted.push(path);
                    } else {
                        report.skipped.push(path);
                    }
                }
                Some(_) => report.kept.push(path),
                None => report.skipped.push(path),
            }
        }
        Ok(report)
    }
}

/// Parse the trailing `YYYYMMDD-HHMMSS` from a session-dir name.
///
/// Returns `None` if the suffix is missing or malformed. The result
/// is interpreted as UTC.
#[must_use]
pub fn parse_session_timestamp(name: &str) -> Option<OffsetDateTime> {
    let s = name.rsplit('_').next()?;
    if s.len() != 15 || s.as_bytes()[8] != b'-' {
        return None;
    }
    let date = &s[..8];
    let tm = &s[9..];
    let y: i32 = date.get(0..4)?.parse().ok()?;
    let mo: u8 = date.get(4..6)?.parse().ok()?;
    let d: u8 = date.get(6..8)?.parse().ok()?;
    let h: u8 = tm.get(0..2)?.parse().ok()?;
    let mi: u8 = tm.get(2..4)?.parse().ok()?;
    let se: u8 = tm.get(4..6)?.parse().ok()?;
    let date = time::Date::from_calendar_date(y, time::Month::try_from(mo).ok()?, d).ok()?;
    let tm = time::Time::from_hms(h, mi, se).ok()?;
    Some(PrimitiveDateTime::new(date, tm).assume_utc())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tempdir() -> PathBuf {
        let p = std::env::temp_dir()
            .join(format!("wanlogger-ret-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn parses_session_timestamp() {
        let ts = parse_session_timestamp("wanlogger_serial_COM3_20240115-130405").unwrap();
        assert_eq!(ts.year(), 2024);
        assert_eq!(u8::from(ts.month()), 1);
        assert_eq!(ts.day(), 15);
        assert_eq!(ts.hour(), 13);
    }

    #[test]
    fn rejects_bad_suffix() {
        assert!(parse_session_timestamp("foo_bar").is_none());
        assert!(parse_session_timestamp("foo_99999999-999999").is_none());
    }

    #[test]
    fn deletes_old_keeps_new_skips_garbage() {
        let parent = tempdir();
        let old = parent.join("wanlogger_tcp_eth0_20200101-000000");
        let new = parent.join("wanlogger_tcp_eth0_20990101-000000");
        let junk = parent.join("not-a-session");
        for p in [&old, &new, &junk] {
            std::fs::create_dir(p).unwrap();
        }
        let policy = RetentionPolicy { keep_days: 7 };
        let now = OffsetDateTime::from_unix_timestamp(1_700_000_000).unwrap();
        let r = policy.apply(&parent, now).unwrap();
        assert_eq!(r.deleted.len(), 1);
        assert!(r.deleted[0].ends_with("wanlogger_tcp_eth0_20200101-000000"));
        assert_eq!(r.kept.len(), 1);
        assert!(r.kept[0].ends_with("wanlogger_tcp_eth0_20990101-000000"));
        assert_eq!(r.skipped.len(), 1);
        assert!(!old.exists());
        assert!(new.exists());
        assert!(junk.exists());
    }
}
