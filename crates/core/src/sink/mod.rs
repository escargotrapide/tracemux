//! `Sink` trait — write-back to a connected channel. **Frozen v0.1.**
//!
//! Source-only transports (pcap, RTT, CAN sniff) **must not**
//! implement this trait. See `.github/skills/add-sink/SKILL.md`.

use async_trait::async_trait;
use bytes::Bytes;

use crate::Result;

/// Accepts bytes / control to write back to a connected channel.
#[async_trait]
pub trait Sink: Send + Sync + 'static {
    /// Write a chunk of bytes (e.g. serial TX, TCP send, MQTT publish).
    async fn write(&mut self, data: Bytes) -> Result<()>;

    /// Optional out-of-band control (resize, break signal, RTS/DTR…).
    /// Default impl is a no-op.
    async fn ctl(&mut self, _kind: &str, _data: Option<Bytes>) -> Result<()> {
        Ok(())
    }

    /// Flush any buffered output.
    async fn flush(&mut self) -> Result<()> {
        Ok(())
    }

    /// Close the sink. Idempotent.
    async fn close(&mut self) -> Result<()>;
}

pub mod mock;
pub mod process;
pub mod pty;
pub mod serial;
pub mod tcp;
pub mod udp;
