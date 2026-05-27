//! Per-connection bounded ring buffer.
//!
//! Each WSS connection gets a [`Ring`] sized in *bytes*. New writes
//! evict the oldest entries when the limit is reached. Used by the
//! server's hold/coalesce path to bound memory regardless of slow
//! consumers. Default capacity is 8 MiB.

use std::collections::VecDeque;

use bytes::Bytes;

/// Default capacity in bytes.
pub const DEFAULT_CAPACITY: usize = 8 * 1024 * 1024;

/// Bounded byte ring.
#[derive(Debug)]
pub struct Ring {
    capacity: usize,
    used: usize,
    items: VecDeque<Bytes>,
    dropped: u64,
}

impl Ring {
    /// Construct with explicit capacity (bytes).
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            capacity,
            used: 0,
            items: VecDeque::new(),
            dropped: 0,
        }
    }

    /// Push one entry, evicting oldest entries until it fits.
    /// Returns the number of entries evicted.
    pub fn push(&mut self, b: Bytes) -> usize {
        let mut evicted = 0;
        while self.used + b.len() > self.capacity && !self.items.is_empty() {
            if let Some(old) = self.items.pop_front() {
                self.used -= old.len();
                self.dropped += 1;
                evicted += 1;
            }
        }
        if b.len() <= self.capacity {
            self.used += b.len();
            self.items.push_back(b);
        } else {
            // Single entry larger than capacity; drop it.
            self.dropped += 1;
            evicted += 1;
        }
        evicted
    }

    /// Pop the oldest entry.
    pub fn pop(&mut self) -> Option<Bytes> {
        let b = self.items.pop_front()?;
        self.used -= b.len();
        Some(b)
    }

    /// Bytes currently held.
    #[must_use]
    pub fn used(&self) -> usize {
        self.used
    }

    /// Total dropped since creation.
    #[must_use]
    pub fn dropped(&self) -> u64 {
        self.dropped
    }

    /// Number of entries.
    #[must_use]
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Empty?
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

impl Default for Ring {
    fn default() -> Self {
        Self::with_capacity(DEFAULT_CAPACITY)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evicts_when_full() {
        let mut r = Ring::with_capacity(10);
        assert_eq!(r.push(Bytes::from_static(b"abcde")), 0);
        assert_eq!(r.push(Bytes::from_static(b"fghij")), 0);
        assert_eq!(r.used(), 10);
        assert_eq!(r.push(Bytes::from_static(b"k")), 1);
        assert_eq!(r.used(), 6);
        assert_eq!(r.dropped(), 1);
    }

    #[test]
    fn drops_oversize_single_entry() {
        let mut r = Ring::with_capacity(4);
        let evicted = r.push(Bytes::from_static(b"toolarge"));
        assert_eq!(evicted, 1);
        assert!(r.is_empty());
        assert_eq!(r.dropped(), 1);
    }

    #[test]
    fn fifo_order() {
        let mut r = Ring::with_capacity(100);
        r.push(Bytes::from_static(b"1"));
        r.push(Bytes::from_static(b"2"));
        r.push(Bytes::from_static(b"3"));
        assert_eq!(r.pop().unwrap(), Bytes::from_static(b"1"));
        assert_eq!(r.pop().unwrap(), Bytes::from_static(b"2"));
        assert_eq!(r.pop().unwrap(), Bytes::from_static(b"3"));
        assert!(r.pop().is_none());
    }
}
