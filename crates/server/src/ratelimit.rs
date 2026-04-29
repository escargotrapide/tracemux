//! WSS DoS rate limit ? token-bucket at most 1 KiB/s by default.
//!
//! See [`docs/protocols/wire-protocol.md`](../../../../docs/protocols/wire-protocol.md).
//! Limits enforced here:
//!
//! * `MAX_CONNS = 32` (connection cap; counted by [`ConnCounter`])
//! * `MAX_FRAME_BYTES = 1 MiB` (frame size; checked by [`crate::wire`])
//! * `RATE_BPS = 1024` (per-connection byte rate; via [`TokenBucket`])

use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Instant;

/// Maximum simultaneous WSS connections.
pub const MAX_CONNS: u32 = 32;

/// Maximum bytes per single wire frame.
pub const MAX_FRAME_BYTES: usize = 1024 * 1024;

/// Default per-connection refill rate, bytes per second.
pub const RATE_BPS: u32 = 1024;

/// Token-bucket rate limiter for one connection.
#[derive(Debug)]
pub struct TokenBucket {
    capacity: u32,
    rate_per_sec: u32,
    tokens: f64,
    last: Instant,
}

impl TokenBucket {
    /// Create a bucket. `capacity == burst size`, `rate_per_sec ==
    /// refill rate`.
    ///
    /// # Panics
    /// Panics if `rate_per_sec == 0`.
    #[must_use]
    pub fn new(capacity: u32, rate_per_sec: u32) -> Self {
        assert!(rate_per_sec > 0, "rate_per_sec must be > 0");
        Self {
            capacity,
            rate_per_sec,
            tokens: f64::from(capacity),
            last: Instant::now(),
        }
    }

    /// Try to consume `cost` tokens at `now`. Returns `true` if
    /// allowed, `false` if rate-limited.
    pub fn try_consume(&mut self, cost: u32, now: Instant) -> bool {
        let dt = now.saturating_duration_since(self.last).as_secs_f64();
        self.last = now;
        self.tokens =
            (self.tokens + dt * f64::from(self.rate_per_sec)).min(f64::from(self.capacity));
        let cost_f = f64::from(cost);
        if self.tokens >= cost_f {
            self.tokens -= cost_f;
            true
        } else {
            false
        }
    }

    /// Tokens currently available (rounded down).
    #[must_use]
    pub fn available(&self) -> u32 {
        self.tokens.max(0.0) as u32
    }
}

/// Atomic connection counter with a configurable cap.
#[derive(Debug)]
pub struct ConnCounter {
    cap: u32,
    cur: AtomicU32,
}

impl ConnCounter {
    /// New counter with cap.
    #[must_use]
    pub const fn new(cap: u32) -> Self {
        Self {
            cap,
            cur: AtomicU32::new(0),
        }
    }

    /// Try to acquire a slot. Returns a guard that releases on drop,
    /// or `None` if the cap is reached.
    pub fn acquire(&self) -> Option<ConnGuard<'_>> {
        let mut cur = self.cur.load(Ordering::Acquire);
        loop {
            if cur >= self.cap {
                return None;
            }
            match self
                .cur
                .compare_exchange_weak(cur, cur + 1, Ordering::AcqRel, Ordering::Acquire)
            {
                Ok(_) => return Some(ConnGuard { counter: self }),
                Err(observed) => cur = observed,
            }
        }
    }

    /// Current connection count.
    #[must_use]
    pub fn current(&self) -> u32 {
        self.cur.load(Ordering::Acquire)
    }

    /// Configured cap.
    #[must_use]
    pub fn cap(&self) -> u32 {
        self.cap
    }
}

/// RAII guard returned by [`ConnCounter::acquire`].
#[derive(Debug)]
pub struct ConnGuard<'a> {
    counter: &'a ConnCounter,
}

impl Drop for ConnGuard<'_> {
    fn drop(&mut self) {
        self.counter.cur.fetch_sub(1, Ordering::AcqRel);
    }
}

/// Owned variant of [`ConnGuard`] suitable for moving into spawned
/// tasks. Holds an [`Arc`] reference to the counter so the slot is
/// released only when the guard itself is dropped.
#[derive(Debug)]
pub struct OwnedConnGuard {
    counter: std::sync::Arc<ConnCounter>,
}

impl Drop for OwnedConnGuard {
    fn drop(&mut self) {
        self.counter.cur.fetch_sub(1, Ordering::AcqRel);
    }
}

impl ConnCounter {
    /// `Arc`-flavoured variant of [`Self::acquire`] suitable for
    /// moving the resulting guard into a spawned task.
    pub fn acquire_owned(self: &std::sync::Arc<Self>) -> Option<OwnedConnGuard> {
        let mut cur = self.cur.load(Ordering::Acquire);
        loop {
            if cur >= self.cap {
                return None;
            }
            match self
                .cur
                .compare_exchange_weak(cur, cur + 1, Ordering::AcqRel, Ordering::Acquire)
            {
                Ok(_) => {
                    return Some(OwnedConnGuard {
                        counter: self.clone(),
                    })
                }
                Err(observed) => cur = observed,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn bucket_refills_over_time() {
        let start = Instant::now();
        let mut b = TokenBucket::new(1024, 1024);
        assert!(b.try_consume(1024, start));
        assert!(!b.try_consume(1, start));
        assert!(b.try_consume(512, start + Duration::from_millis(500)));
    }

    #[test]
    fn conn_counter_caps_and_releases() {
        let c = ConnCounter::new(2);
        let g1 = c.acquire().unwrap();
        let _g2 = c.acquire().unwrap();
        assert!(c.acquire().is_none());
        assert_eq!(c.current(), 2);
        drop(g1);
        assert_eq!(c.current(), 1);
        let _g3 = c.acquire().unwrap();
        assert!(c.acquire().is_none());
    }
}
