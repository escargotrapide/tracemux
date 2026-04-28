//! Passthrough framer — emits whatever is buffered as a single frame.

use bytes::{Bytes, BytesMut};

use super::Framer;
use crate::Result;

/// Passthrough framer.
#[derive(Debug, Default)]
pub struct PassthroughFramer;

impl Framer for PassthroughFramer {
    fn poll_frame(&mut self, buf: &mut BytesMut) -> Result<Option<Bytes>> {
        if buf.is_empty() {
            Ok(None)
        } else {
            Ok(Some(buf.split().freeze()))
        }
    }

    fn kind(&self) -> &'static str {
        "passthrough"
    }
}
