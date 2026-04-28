//! TCP-client [`Source`].
//!
//! Connects to `host:port` and emits incoming bytes as
//! [`Frame::Bytes`]. Disconnections surface as [`ControlEvt::Eof`]
//! followed by `recv()` returning `None`.

use std::collections::BTreeMap;
use std::collections::VecDeque;

use async_trait::async_trait;
use bytes::Bytes;
use tokio::io::AsyncReadExt;
use tokio::net::TcpStream;

use super::{ChannelMeta, ControlEvt, Frame, Source};
use crate::{ErrorId, Result, WanloggerError};

const READ_CHUNK: usize = 8 * 1024;

/// TCP client source.
#[derive(Debug)]
pub struct TcpSource {
    addr: String,
    stream: Option<TcpStream>,
    pending_ctl: VecDeque<ControlEvt>,
}

impl TcpSource {
    /// Construct.
    #[must_use]
    pub fn new(addr: impl Into<String>) -> Self {
        Self {
            addr: addr.into(),
            stream: None,
            pending_ctl: VecDeque::new(),
        }
    }
}

#[async_trait]
impl Source for TcpSource {
    async fn open(&mut self) -> Result<()> {
        let stream = TcpStream::connect(&self.addr).await.map_err(|e| {
            WanloggerError::new(
                ErrorId::E1101SourceOpen,
                format!("tcp connect {}: {e}", self.addr),
            )
            .with_source(e)
        })?;
        self.stream = Some(stream);
        self.pending_ctl.push_back(ControlEvt::Connected);
        Ok(())
    }

    async fn recv(&mut self) -> Result<Option<Frame>> {
        let stream = match self.stream.as_mut() {
            Some(s) => s,
            None => {
                return Err(WanloggerError::new(
                    ErrorId::E1102SourceClosed,
                    "tcp source not open",
                ))
            }
        };
        let mut buf = vec![0u8; READ_CHUNK];
        let n = stream.read(&mut buf).await.map_err(|e| {
            WanloggerError::new(
                ErrorId::E1102SourceClosed,
                format!("tcp read {}: {e}", self.addr),
            )
            .with_source(e)
        })?;
        if n == 0 {
            self.pending_ctl.push_back(ControlEvt::Eof);
            self.stream = None;
            return Ok(None);
        }
        buf.truncate(n);
        Ok(Some(Frame::Bytes(Bytes::from(buf))))
    }

    async fn recv_ctl(&mut self) -> Result<Option<ControlEvt>> {
        Ok(self.pending_ctl.pop_front())
    }

    fn metadata(&self) -> ChannelMeta {
        ChannelMeta {
            kind: "tcp".into(),
            iface: self.addr.clone(),
            tags: BTreeMap::new(),
        }
    }

    async fn close(&mut self) -> Result<()> {
        self.stream = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncWriteExt;
    use tokio::net::TcpListener;

    #[tokio::test]
    async fn echoes_bytes_and_eofs_on_close() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            if let Ok((mut sock, _)) = listener.accept().await {
                sock.write_all(b"hello").await.unwrap();
                sock.shutdown().await.unwrap();
            }
        });
        let mut src = TcpSource::new(addr.to_string());
        src.open().await.unwrap();
        let f = src.recv().await.unwrap().unwrap();
        match f {
            Frame::Bytes(b) => assert_eq!(&b[..], b"hello"),
            _ => panic!("expected Bytes"),
        }
        assert!(src.recv().await.unwrap().is_none());
        // Connected was recorded; then Eof.
        let evts: Vec<_> = std::iter::from_fn(|| {
            futures::executor::block_on(src.recv_ctl()).ok().flatten()
        })
        .collect();
        assert!(matches!(evts.first(), Some(ControlEvt::Connected)));
        assert!(evts
            .iter()
            .any(|e| matches!(e, ControlEvt::Eof)));
    }

    #[tokio::test]
    async fn refused_connection_is_e1101() {
        // 127.0.0.1:1 is reserved as TCPMUX; it almost never listens.
        let mut src = TcpSource::new("127.0.0.1:1");
        let err = src.open().await.unwrap_err();
        assert_eq!(err.id, ErrorId::E1101SourceOpen);
    }
}
