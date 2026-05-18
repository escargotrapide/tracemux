//! In-memory [`Sink`] implementation for tests.

use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use parking_lot::Mutex;

use super::Sink;
use crate::Result;

/// Shared byte buffer written by [`MockSink`].
pub type MockSinkBuffer = Arc<Mutex<Vec<Bytes>>>;

/// In-memory write-back sink useful for integration tests.
#[derive(Debug, Clone)]
pub struct MockSink {
    writes: MockSinkBuffer,
    closed: bool,
}

impl MockSink {
    /// Construct an empty mock sink.
    #[must_use]
    pub fn new() -> Self {
        Self::with_buffer(Arc::new(Mutex::new(Vec::new())))
    }

    /// Construct a mock sink around an externally visible buffer.
    #[must_use]
    pub fn with_buffer(writes: MockSinkBuffer) -> Self {
        Self {
            writes,
            closed: false,
        }
    }

    /// Return the shared writes buffer.
    #[must_use]
    pub fn writes(&self) -> MockSinkBuffer {
        self.writes.clone()
    }
}

impl Default for MockSink {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Sink for MockSink {
    async fn write(&mut self, data: Bytes) -> Result<()> {
        if !self.closed {
            self.writes.lock().push(data);
        }
        Ok(())
    }

    async fn close(&mut self) -> Result<()> {
        self.closed = true;
        Ok(())
    }
}
