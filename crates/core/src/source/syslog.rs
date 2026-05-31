//! Syslog [`Source`].
//!
//! Binds a UDP socket (default port 514) and emits each datagram as
//! [`Frame::Datagram`]. Trailing `\n` and the BSD/RFC-5424 prefix
//! parsing are the responsibility of a downstream framer/decoder.

use std::collections::BTreeMap;

use async_trait::async_trait;
use bytes::Bytes;
use tokio::net::UdpSocket;

use super::{ChannelMeta, ControlEvt, Frame, Source};
use crate::{ErrorId, Result, TraceMuxError};

const RECV_BUF: usize = 64 * 1024;

/// Syslog UDP source.
#[derive(Debug)]
pub struct SyslogSource {
    bind: String,
    socket: Option<UdpSocket>,
}

impl SyslogSource {
    /// Construct.
    #[must_use]
    pub fn new(bind: impl Into<String>) -> Self {
        Self {
            bind: bind.into(),
            socket: None,
        }
    }

    /// Local bound address (after `open`).
    pub fn local_addr(&self) -> Option<std::net::SocketAddr> {
        self.socket.as_ref().and_then(|s| s.local_addr().ok())
    }
}

#[async_trait]
impl Source for SyslogSource {
    async fn open(&mut self) -> Result<()> {
        let s = UdpSocket::bind(&self.bind).await.map_err(|e| {
            TraceMuxError::new(
                ErrorId::E1101SourceOpen,
                format!("syslog bind {}: {e}", self.bind),
            )
            .with_source(e)
        })?;
        self.socket = Some(s);
        Ok(())
    }

    async fn recv(&mut self) -> Result<Option<Frame>> {
        let s = match self.socket.as_ref() {
            Some(s) => s,
            None => {
                return Err(TraceMuxError::new(
                    ErrorId::E1102SourceClosed,
                    "syslog source not open",
                ))
            }
        };
        let mut buf = vec![0u8; RECV_BUF];
        let (n, peer) = s.recv_from(&mut buf).await.map_err(|e| {
            TraceMuxError::new(ErrorId::E1102SourceClosed, format!("syslog recv: {e}"))
                .with_source(e)
        })?;
        buf.truncate(n);
        Ok(Some(Frame::Datagram {
            src: Some(peer.to_string()),
            data: Bytes::from(buf),
        }))
    }

    async fn recv_ctl(&mut self) -> Result<Option<ControlEvt>> {
        Ok(None)
    }

    fn metadata(&self) -> ChannelMeta {
        ChannelMeta {
            kind: "syslog".into(),
            iface: self.bind.clone(),
            tags: BTreeMap::new(),
        }
    }

    async fn close(&mut self) -> Result<()> {
        self.socket = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn receives_syslog_datagram() {
        let mut src = SyslogSource::new("127.0.0.1:0");
        src.open().await.unwrap();
        let local = src.local_addr().unwrap();
        let sender = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        sender.connect(local).await.unwrap();
        sender
            .send(b"<13>Oct 11 22:14:15 host app: hi")
            .await
            .unwrap();
        let f = src.recv().await.unwrap().unwrap();
        match f {
            Frame::Datagram { data, src: _ } => assert!(data.starts_with(b"<13>")),
            other => panic!("wrong: {other:?}"),
        }
    }
}
