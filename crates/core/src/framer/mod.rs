//! `Framer` trait — turns raw bytes into discrete frames.
//! **Frozen v0.1.** See `.github/skills/add-framer/SKILL.md`.

use bytes::{Bytes, BytesMut};

use crate::Result;

/// Stateful framer over a byte stream.
pub trait Framer: Send + 'static {
    /// Pull the next frame out of `buf`. Returns `Ok(None)` when more
    /// bytes are required. Implementations must consume the bytes
    /// they emit (`buf.advance(...)`) and never hold onto more than
    /// the configured max-frame-size.
    fn poll_frame(&mut self, buf: &mut BytesMut) -> Result<Option<Bytes>>;

    /// Stable kind string for logging / tracing (e.g. `"line"`).
    fn kind(&self) -> &'static str;
}

pub mod length_prefixed;
pub mod line;
pub mod passthrough;
pub mod regex_framer;
