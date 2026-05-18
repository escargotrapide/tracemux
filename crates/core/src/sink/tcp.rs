//! TCP [`Sink`] implementation.
//!
//! The sink owns the write half of a TCP connection created by
//! [`crate::source::tcp::TcpSource::connect_duplex`].

use async_trait::async_trait;
use bytes::Bytes;
use tokio::io::AsyncWriteExt as _;
use tokio::net::tcp::OwnedWriteHalf;

use super::Sink;
use crate::{ErrorId, Result, WanloggerError};

/// TCP write-back sink.
#[derive(Debug)]
pub struct TcpSink {
    addr: String,
    writer: Option<OwnedWriteHalf>,
}

impl TcpSink {
    /// Construct from an owned TCP write half.
    #[must_use]
    pub fn new(addr: impl Into<String>, writer: OwnedWriteHalf) -> Self {
        Self {
            addr: addr.into(),
            writer: Some(writer),
        }
    }
}

#[async_trait]
impl Sink for TcpSink {
    async fn write(&mut self, data: Bytes) -> Result<()> {
        let Some(writer) = self.writer.as_mut() else {
            return Err(WanloggerError::new(
                ErrorId::E1102SourceClosed,
                "tcp sink not open",
            ));
        };
        writer.write_all(&data).await.map_err(|e| {
            WanloggerError::new(
                ErrorId::E1102SourceClosed,
                format!("tcp write {}: {e}", self.addr),
            )
            .with_source(e)
        })
    }

    async fn flush(&mut self) -> Result<()> {
        let Some(writer) = self.writer.as_mut() else {
            return Ok(());
        };
        writer.flush().await.map_err(|e| {
            WanloggerError::new(
                ErrorId::E1102SourceClosed,
                format!("tcp flush {}: {e}", self.addr),
            )
            .with_source(e)
        })
    }

    async fn close(&mut self) -> Result<()> {
        if let Some(mut writer) = self.writer.take() {
            let _ = writer.shutdown().await;
        }
        Ok(())
    }
}
