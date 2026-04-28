//! `LogSink` trait — persists records to durable storage. **Frozen v0.1.**
//!
//! The reference impl is [`file::FileLogSink`], writing to a
//! `session-dir/` per [`docs/protocols/log-format.md`](
//! ../../../../docs/protocols/log-format.md).

use async_trait::async_trait;
use bytes::Bytes;

use crate::{decoder::Record, time::DualTimestamp, Result};

/// Persistent log sink.
#[async_trait]
pub trait LogSink: Send + Sync + 'static {
    /// Append a raw byte slice (with envelope) to `raw.bin` + `index.jsonl`.
    async fn append_raw(&mut self, ts: &DualTimestamp, dir: Direction, data: Bytes) -> Result<()>;

    /// Append a decoded record to `lines.jsonl` / `frames.jsonl`.
    async fn append_record(&mut self, ts: &DualTimestamp, record: &Record) -> Result<()>;

    /// Force a fsync / commit. Implementations may also commit on a
    /// timer (group commit).
    async fn commit(&mut self) -> Result<()>;

    /// Close the sink. Idempotent.
    async fn close(&mut self) -> Result<()>;
}

/// Record direction relative to the channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// Inbound (received from the source).
    In,
    /// Outbound (written to the sink).
    Out,
}

pub mod fanout;
pub mod file;
