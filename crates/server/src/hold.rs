//! Backpressure "hold" queue for slow consumers.
//!
//! When a WSS client cannot keep up, the server queues outbound items
//! in a bounded ring. Once full, the oldest item is dropped and the
//! `dropped` counter is bumped; callers can surface this as
//! `E-1002 Backpressure` to the client.

use std::collections::VecDeque;

/// Drop policy when the queue is full.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropPolicy {
    /// Drop the oldest item (head). Default.
    DropOldest,
    /// Drop the new item being pushed.
    DropNew,
}

/// Bounded hold queue.
#[derive(Debug)]
pub struct HoldQueue<T> {
    inner: VecDeque<T>,
    capacity: usize,
    policy: DropPolicy,
    dropped: u64,
}

impl<T> HoldQueue<T> {
    /// Create a new queue with `capacity` slots.
    ///
    /// # Panics
    /// Panics if `capacity == 0`.
    #[must_use]
    pub fn new(capacity: usize, policy: DropPolicy) -> Self {
        assert!(capacity > 0, "hold queue capacity must be > 0");
        Self {
            inner: VecDeque::with_capacity(capacity),
            capacity,
            policy,
            dropped: 0,
        }
    }

    /// Push `item`. Returns `true` if it was kept, `false` if dropped.
    pub fn push(&mut self, item: T) -> bool {
        if self.inner.len() < self.capacity {
            self.inner.push_back(item);
            return true;
        }
        match self.policy {
            DropPolicy::DropOldest => {
                self.inner.pop_front();
                self.inner.push_back(item);
                self.dropped += 1;
                true
            }
            DropPolicy::DropNew => {
                self.dropped += 1;
                false
            }
        }
    }

    /// Pop the oldest item, if any.
    pub fn pop(&mut self) -> Option<T> {
        self.inner.pop_front()
    }

    /// Number of items currently queued.
    #[must_use]
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// Whether the queue is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Total drop count since creation.
    #[must_use]
    pub fn dropped(&self) -> u64 {
        self.dropped
    }

    /// Configured capacity.
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.capacity
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drop_oldest_replaces_head() {
        let mut q: HoldQueue<u32> = HoldQueue::new(3, DropPolicy::DropOldest);
        for i in 0..5 {
            assert!(q.push(i));
        }
        assert_eq!(q.dropped(), 2);
        assert_eq!(q.pop(), Some(2));
        assert_eq!(q.pop(), Some(3));
        assert_eq!(q.pop(), Some(4));
        assert_eq!(q.pop(), None);
    }

    #[test]
    fn drop_new_keeps_head() {
        let mut q: HoldQueue<u32> = HoldQueue::new(2, DropPolicy::DropNew);
        assert!(q.push(1));
        assert!(q.push(2));
        assert!(!q.push(3));
        assert!(!q.push(4));
        assert_eq!(q.dropped(), 2);
        assert_eq!(q.pop(), Some(1));
        assert_eq!(q.pop(), Some(2));
    }

    #[test]
    #[should_panic(expected = "capacity")]
    fn zero_capacity_panics() {
        let _: HoldQueue<u32> = HoldQueue::new(0, DropPolicy::DropOldest);
    }
}
