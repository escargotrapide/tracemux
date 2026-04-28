//! Server-side coalescing buckets.
//!
//! The UI tells the server which panels are foreground / background
//! (see [`crate::panel_priority`]); the server in turn coalesces
//! outbound deltas at one of three fixed cadences:
//!
//! | Bucket   | Period |
//! |----------|--------|
//! | Live     | 16 ms (?60 fps) |
//! | Visible  | 500 ms |
//! | Hidden   | 2 s   |
//!
//! [`Coalescer::should_flush`] returns `true` when the current
//! bucket's deadline has expired since the last flush.

use std::time::{Duration, Instant};

/// Coalescing bucket.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bucket {
    /// Foreground / live (16 ms).
    Live,
    /// Visible but not foreground (500 ms).
    Visible,
    /// Hidden (2 s).
    Hidden,
}

impl Bucket {
    /// The flush period for this bucket.
    #[must_use]
    pub const fn period(self) -> Duration {
        match self {
            Self::Live => Duration::from_millis(16),
            Self::Visible => Duration::from_millis(500),
            Self::Hidden => Duration::from_secs(2),
        }
    }
}

/// Per-channel coalescer state.
#[derive(Debug)]
pub struct Coalescer {
    bucket: Bucket,
    last_flush: Instant,
}

impl Coalescer {
    /// Create with `bucket` and `last_flush = now`.
    #[must_use]
    pub fn new(bucket: Bucket) -> Self {
        Self {
            bucket,
            last_flush: Instant::now(),
        }
    }

    /// Update the bucket (e.g. panel went foreground/background).
    pub fn set_bucket(&mut self, bucket: Bucket) {
        self.bucket = bucket;
    }

    /// Current bucket.
    #[must_use]
    pub fn bucket(&self) -> Bucket {
        self.bucket
    }

    /// Returns `true` if the bucket's period has elapsed since the
    /// last flush. When it returns `true`, the caller is expected to
    /// flush and a fresh deadline starts.
    pub fn should_flush(&mut self, now: Instant) -> bool {
        if now.saturating_duration_since(self.last_flush) >= self.bucket.period() {
            self.last_flush = now;
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_period_is_16ms() {
        assert_eq!(Bucket::Live.period(), Duration::from_millis(16));
    }

    #[test]
    fn should_flush_after_period() {
        let start = Instant::now();
        let mut c = Coalescer::new(Bucket::Live);
        assert!(!c.should_flush(start + Duration::from_millis(10)));
        assert!(c.should_flush(start + Duration::from_millis(20)));
        assert!(!c.should_flush(start + Duration::from_millis(30)));
        assert!(c.should_flush(start + Duration::from_millis(40)));
    }
}
