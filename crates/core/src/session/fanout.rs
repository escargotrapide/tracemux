//! Per-session fan-out: one producer → N consumers.
//!
//! Thin wrapper around [`tokio::sync::broadcast`]. Slow subscribers
//! receive [`Lagged`] markers instead of stalling the producer; the
//! ring buffer in [`super::ring`] is what bounds memory.
//!
//! [`Lagged`]: tokio::sync::broadcast::error::RecvError::Lagged

use bytes::Bytes;
use tokio::sync::broadcast;

/// Default broadcast slot count.
pub const DEFAULT_CAPACITY: usize = 1024;

/// Fan-out producer.
#[derive(Debug, Clone)]
pub struct Fanout {
    tx: broadcast::Sender<Bytes>,
}

impl Fanout {
    /// Construct with `capacity` slots.
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        let (tx, _) = broadcast::channel(capacity);
        Self { tx }
    }

    /// New subscriber.
    #[must_use]
    pub fn subscribe(&self) -> broadcast::Receiver<Bytes> {
        self.tx.subscribe()
    }

    /// Publish to all subscribers. Returns the count delivered.
    pub fn publish(&self, b: Bytes) -> usize {
        self.tx.send(b).unwrap_or(0)
    }

    /// Subscriber count.
    #[must_use]
    pub fn receiver_count(&self) -> usize {
        self.tx.receiver_count()
    }
}

impl Default for Fanout {
    fn default() -> Self {
        Self::with_capacity(DEFAULT_CAPACITY)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn delivers_to_all_subscribers() {
        let f = Fanout::with_capacity(8);
        let mut a = f.subscribe();
        let mut b = f.subscribe();
        assert_eq!(f.publish(Bytes::from_static(b"x")), 2);
        assert_eq!(a.recv().await.unwrap(), Bytes::from_static(b"x"));
        assert_eq!(b.recv().await.unwrap(), Bytes::from_static(b"x"));
    }

    #[tokio::test]
    async fn no_subscribers_returns_zero() {
        let f = Fanout::with_capacity(8);
        assert_eq!(f.publish(Bytes::from_static(b"x")), 0);
    }
}
