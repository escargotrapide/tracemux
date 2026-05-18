//! Process-stdin [`Sink`] implementation.

use async_trait::async_trait;
use bytes::Bytes;
use tokio::io::AsyncWriteExt as _;
use tokio::process::ChildStdin;

use super::Sink;
use crate::{ErrorId, Result, WanloggerError};

/// Sink that writes bytes to a child process' stdin.
#[derive(Debug)]
pub struct ProcessSink {
    iface: String,
    stdin: Option<ChildStdin>,
}

impl ProcessSink {
    /// Construct from a piped child stdin handle.
    #[must_use]
    pub fn new(iface: impl Into<String>, stdin: ChildStdin) -> Self {
        Self {
            iface: iface.into(),
            stdin: Some(stdin),
        }
    }
}

#[async_trait]
impl Sink for ProcessSink {
    async fn write(&mut self, data: Bytes) -> Result<()> {
        let Some(stdin) = self.stdin.as_mut() else {
            return Err(WanloggerError::new(
                ErrorId::E1102SourceClosed,
                "process stdin sink not open",
            ));
        };
        stdin.write_all(&data).await.map_err(|e| {
            WanloggerError::new(
                ErrorId::E1102SourceClosed,
                format!("process stdin write {}: {e}", self.iface),
            )
            .with_source(e)
        })
    }

    async fn flush(&mut self) -> Result<()> {
        let Some(stdin) = self.stdin.as_mut() else {
            return Ok(());
        };
        stdin.flush().await.map_err(|e| {
            WanloggerError::new(
                ErrorId::E1102SourceClosed,
                format!("process stdin flush {}: {e}", self.iface),
            )
            .with_source(e)
        })
    }

    async fn close(&mut self) -> Result<()> {
        self.stdin = None;
        Ok(())
    }
}
