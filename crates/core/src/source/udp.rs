//! UDP [`Source`].
//!
//! Binds a UDP socket and emits each datagram as
//! [`Frame::Datagram`] with the sender's address as `src`.

use std::collections::BTreeMap;

use async_trait::async_trait;
use bytes::Bytes;
use tokio::net::UdpSocket;

use super::{ChannelMeta, ControlEvt, Frame, Source};
use crate::{ErrorId, Result, WanloggerError};

const RECV_BUF: usize = 64 * 1024;

/// UDP source.
#[derive(Debug)]
pub struct UdpSource {
    bind: String,
    socket: Option<UdpSocket>,
}

impl UdpSource {
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
impl Source for UdpSource {
    async fn open(&mut self) -> Result<()> {
        let s = UdpSocket::bind(&self.bind).await.map_err(|e| {
            WanloggerError::new(
                ErrorId::E1101SourceOpen,
                format!("udp bind {}: {e}", self.bind),
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
                return Err(WanloggerError::new(
                    ErrorId::E1102SourceClosed,
                    "udp source not open",
                ))
            }
        };
        let mut buf = vec![0u8; RECV_BUF];
        let (n, peer) = s.recv_from(&mut buf).await.map_err(|e| {
            WanloggerError::new(
                ErrorId::E1102SourceClosed,
                format!("udp recv: {e}"),
            )
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
            kind: "udp".into(),
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
    async fn receives_one_datagram() {
        let mut src = UdpSource::new("127.0.0.1:0");
        src.open().await.unwrap();
        let local = src.local_addr().unwrap();

        let sender = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        sender.connect(local).await.unwrap();
        sender.send(b"ping").await.unwrap();

        let f = src.recv().await.unwrap().unwrap();
        match f {
            Frame::Datagram { src: peer, data } => {
                assert_eq!(&data[..], b"ping");
                assert!(peer.is_some());
            }
            _ => panic!("expected Datagram"),
        }
    }

    #[tokio::test]
    async fn bad_bind_addr_is_e1101() {
        let mut src = UdpSource::new("not-a-valid-addr");
        let err = src.open().await.unwrap_err();
        assert_eq!(err.id, ErrorId::E1101SourceOpen);
    }
}
