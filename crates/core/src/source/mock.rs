//! In-memory mock source. Useful for tests.
//!
//! Stub for v0.1 — implements the bare trait.

use std::collections::{BTreeMap, VecDeque};

use async_trait::async_trait;
use bytes::Bytes;
use parking_lot::Mutex;

use super::{ChannelMeta, ControlEvt, Frame, Source};
use crate::Result;

/// In-memory `Source` driven by a queue of pre-loaded frames.
#[derive(Debug, Default)]
pub struct MockSource {
    queue: Mutex<VecDeque<Frame>>,
    tag: String,
}

impl MockSource {
    /// Construct.
    #[must_use]
    pub fn new(tag: impl Into<String>) -> Self {
        Self {
            queue: Mutex::new(VecDeque::new()),
            tag: tag.into(),
        }
    }

    /// Push a byte frame.
    pub fn push_bytes(&self, b: impl Into<Bytes>) {
        self.queue.lock().push_back(Frame::Bytes(b.into()));
    }
}

#[async_trait]
impl Source for MockSource {
    async fn open(&mut self) -> Result<()> {
        Ok(())
    }
    async fn recv(&mut self) -> Result<Option<Frame>> {
        Ok(self.queue.lock().pop_front())
    }
    async fn recv_ctl(&mut self) -> Result<Option<ControlEvt>> {
        Ok(None)
    }
    fn metadata(&self) -> ChannelMeta {
        ChannelMeta {
            kind: "mock".into(),
            iface: self.tag.clone(),
            tags: BTreeMap::new(),
        }
    }
    async fn close(&mut self) -> Result<()> {
        Ok(())
    }
}
