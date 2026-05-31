//! Serial-port [`Sink`] implementation.
//!
//! The concrete writer is available when the `serial` Cargo feature is
//! enabled. Without that feature, the type exists so callers can build,
//! but writes return `E-1101`.

use async_trait::async_trait;
use bytes::Bytes;

use super::Sink;
use crate::{ErrorId, Result, TraceMuxError};

/// Serial-port write-back sink.
#[derive(Debug)]
pub struct SerialSink {
    port: String,
    #[cfg(feature = "serial")]
    writer: Option<tokio::io::WriteHalf<tokio_serial::SerialStream>>,
}

impl SerialSink {
    /// Construct a disabled serial sink for builds without `serial`.
    #[cfg(not(feature = "serial"))]
    #[must_use]
    pub fn unavailable(port: impl Into<String>) -> Self {
        Self { port: port.into() }
    }

    /// Construct from a split serial write half.
    #[cfg(feature = "serial")]
    #[must_use]
    pub fn new(
        port: impl Into<String>,
        writer: tokio::io::WriteHalf<tokio_serial::SerialStream>,
    ) -> Self {
        Self {
            port: port.into(),
            writer: Some(writer),
        }
    }
}

#[async_trait]
impl Sink for SerialSink {
    async fn write(&mut self, data: Bytes) -> Result<()> {
        #[cfg(feature = "serial")]
        {
            use tokio::io::AsyncWriteExt as _;

            let Some(writer) = self.writer.as_mut() else {
                return Err(TraceMuxError::new(
                    ErrorId::E1102SourceClosed,
                    "serial sink not open",
                ));
            };
            writer.write_all(&data).await.map_err(|e| {
                TraceMuxError::new(
                    ErrorId::E1102SourceClosed,
                    format!("serial write {}: {e}", self.port),
                )
                .with_source(e)
            })
        }
        #[cfg(not(feature = "serial"))]
        {
            let _ = data;
            Err(TraceMuxError::new(
                ErrorId::E1101SourceOpen,
                format!("serial sink {} requires the `serial` feature", self.port),
            ))
        }
    }

    async fn flush(&mut self) -> Result<()> {
        #[cfg(feature = "serial")]
        {
            use tokio::io::AsyncWriteExt as _;

            let Some(writer) = self.writer.as_mut() else {
                return Ok(());
            };
            writer.flush().await.map_err(|e| {
                TraceMuxError::new(
                    ErrorId::E1102SourceClosed,
                    format!("serial flush {}: {e}", self.port),
                )
                .with_source(e)
            })
        }
        #[cfg(not(feature = "serial"))]
        {
            Ok(())
        }
    }

    async fn close(&mut self) -> Result<()> {
        #[cfg(feature = "serial")]
        {
            self.writer = None;
        }
        Ok(())
    }
}
