//! Named pipe / FIFO [`Source`].
//!
//! Opens `path` for reading and emits the byte stream as
//! [`Frame::Bytes`]. On Unix this works with FIFOs created by
//! `mkfifo`; on Windows it works with named pipes opened via the
//! standard `\\.\pipe\name` path. Backed by [`tokio::fs::File`]
//! for portability.

use std::collections::BTreeMap;

use async_trait::async_trait;
use bytes::Bytes;
use tokio::fs::File;
use tokio::io::AsyncReadExt;

use super::{ChannelMeta, ControlEvt, Frame, Source};
use crate::{ErrorId, Result, WanloggerError};

const RECV_BUF: usize = 16 * 1024;

/// Named pipe / FIFO source.
#[derive(Debug)]
pub struct PipeSource {
    path: String,
    file: Option<File>,
    eof_sent: bool,
}

impl PipeSource {
    /// Construct.
    #[must_use]
    pub fn new(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            file: None,
            eof_sent: false,
        }
    }
}

#[async_trait]
impl Source for PipeSource {
    async fn open(&mut self) -> Result<()> {
        let f = File::open(&self.path).await.map_err(|e| {
            WanloggerError::new(
                ErrorId::E1101SourceOpen,
                format!("pipe open {}: {e}", self.path),
            )
            .with_source(e)
        })?;
        self.file = Some(f);
        Ok(())
    }

    async fn recv(&mut self) -> Result<Option<Frame>> {
        let f = match self.file.as_mut() {
            Some(f) => f,
            None => {
                return Err(WanloggerError::new(
                    ErrorId::E1102SourceClosed,
                    "pipe source not open",
                ))
            }
        };
        let mut buf = vec![0u8; RECV_BUF];
        let n = f.read(&mut buf).await.map_err(|e| {
            WanloggerError::new(ErrorId::E1102SourceClosed, format!("pipe read: {e}"))
                .with_source(e)
        })?;
        if n == 0 {
            self.eof_sent = true;
            return Ok(None);
        }
        buf.truncate(n);
        Ok(Some(Frame::Bytes(Bytes::from(buf))))
    }

    async fn recv_ctl(&mut self) -> Result<Option<ControlEvt>> {
        if self.eof_sent {
            self.eof_sent = false;
            return Ok(Some(ControlEvt::Eof));
        }
        Ok(None)
    }

    fn metadata(&self) -> ChannelMeta {
        ChannelMeta {
            kind: "pipe".into(),
            iface: self.path.clone(),
            tags: BTreeMap::new(),
        }
    }

    async fn close(&mut self) -> Result<()> {
        self.file = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reading from a regular file works the same way as a FIFO
    /// would; this exercises the open/read/eof state machine.
    #[tokio::test]
    async fn reads_then_eof() {
        let dir = std::env::temp_dir().join(format!("wlg-pipe-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("f.bin");
        std::fs::write(&p, b"hello").unwrap();
        let mut src = PipeSource::new(p.to_string_lossy().to_string());
        src.open().await.unwrap();
        let f = src.recv().await.unwrap().unwrap();
        match f {
            Frame::Bytes(b) => assert_eq!(&b[..], b"hello"),
            other => panic!("wrong: {other:?}"),
        }
        let n = src.recv().await.unwrap();
        assert!(n.is_none());
        let ctl = src.recv_ctl().await.unwrap();
        assert!(matches!(ctl, Some(ControlEvt::Eof)));
    }
}
