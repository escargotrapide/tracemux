//! Broadcast event bus (drop-on-lag) used for the UI pipeline.
//!
//! The logger pipeline uses a bounded `mpsc` and is *not* this bus ?
//! see [`crate::session`]. This bus is intentionally lossy: slow
//! subscribers see a [`RecvError::Lagged`] notification with the
//! number of skipped messages.

use tokio::sync::broadcast;

/// Default channel capacity (in messages) for new buses.
pub const DEFAULT_CAPACITY: usize = 1024;

/// Receive-side errors surfaced to subscribers.
#[derive(Debug, thiserror::Error)]
pub enum RecvError {
    /// The bus was closed (no senders remaining).
    #[error("event bus closed")]
    Closed,
    /// The subscriber lagged and `n` messages were dropped.
    #[error("event bus lagged: {0} messages dropped")]
    Lagged(u64),
}

impl From<broadcast::error::RecvError> for RecvError {
    fn from(e: broadcast::error::RecvError) -> Self {
        match e {
            broadcast::error::RecvError::Closed => Self::Closed,
            broadcast::error::RecvError::Lagged(n) => Self::Lagged(n),
        }
    }
}

/// Drop-on-lag broadcast bus parameterised by message type `T`.
#[derive(Debug)]
pub struct EventBus<T: Clone> {
    tx: broadcast::Sender<T>,
}

impl<T: Clone> EventBus<T> {
    /// Build a new bus with the given capacity.
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        let (tx, _) = broadcast::channel(capacity);
        Self { tx }
    }

    /// Build a new bus with [`DEFAULT_CAPACITY`].
    #[must_use]
    pub fn new() -> Self {
        Self::with_capacity(DEFAULT_CAPACITY)
    }

    /// Publish a value. Returns the number of currently-active
    /// subscribers (0 if there are none ? the value is then dropped).
    pub fn publish(&self, value: T) -> usize {
        self.tx.send(value).unwrap_or(0)
    }

    /// Number of currently-active subscribers.
    #[must_use]
    pub fn subscriber_count(&self) -> usize {
        self.tx.receiver_count()
    }

    /// Subscribe a new receiver to the bus.
    #[must_use]
    pub fn subscribe(&self) -> Receiver<T> {
        Receiver {
            rx: self.tx.subscribe(),
        }
    }
}

impl<T: Clone> Default for EventBus<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Clone> Clone for EventBus<T> {
    fn clone(&self) -> Self {
        Self {
            tx: self.tx.clone(),
        }
    }
}

/// Subscriber side of an [`EventBus`].
#[derive(Debug)]
pub struct Receiver<T: Clone> {
    rx: broadcast::Receiver<T>,
}

impl<T: Clone> Receiver<T> {
    /// Await the next message.
    ///
    /// # Errors
    /// Returns [`RecvError::Closed`] if all senders have dropped, or
    /// [`RecvError::Lagged`] if this subscriber missed messages.
    pub async fn recv(&mut self) -> Result<T, RecvError> {
        self.rx.recv().await.map_err(Into::into)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn publish_then_receive() {
        let bus: EventBus<u32> = EventBus::new();
        let mut sub = bus.subscribe();
        bus.publish(7);
        assert_eq!(sub.recv().await.unwrap(), 7);
    }

    #[tokio::test]
    async fn lagged_subscriber_is_notified() {
        let bus: EventBus<u32> = EventBus::with_capacity(2);
        let mut sub = bus.subscribe();
        for i in 0..10 {
            bus.publish(i);
        }
        match sub.recv().await {
            Err(RecvError::Lagged(_)) => {}
            other => panic!("expected Lagged, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn no_subscribers_returns_zero() {
        let bus: EventBus<u8> = EventBus::new();
        assert_eq!(bus.publish(1), 0);
    }
}
