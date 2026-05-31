//! UDP [`Source`].
//!
//! Binds a UDP socket and emits each datagram as
//! [`Frame::Datagram`] with the sender's address as `src`.

use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use parking_lot::RwLock;
use tokio::net::UdpSocket;

use super::{ChannelMeta, ControlEvt, Frame, Source};
use crate::sink::udp::{SharedUdpPeer, UdpSink};
use crate::{ErrorId, Result, TraceMuxError};

const RECV_BUF: usize = 64 * 1024;

/// UDP source.
#[derive(Debug)]
pub struct UdpSource {
    bind: String,
    socket: Option<Arc<UdpSocket>>,
    last_peer: SharedUdpPeer,
}

impl UdpSource {
    /// Construct.
    #[must_use]
    pub fn new(bind: impl Into<String>) -> Self {
        Self {
            bind: bind.into(),
            socket: None,
            last_peer: Arc::new(RwLock::new(None)),
        }
    }

    /// Bind a UDP socket and split it into a source/sink pair.
    ///
    /// The sink sends to `payload.target` when supplied by the server,
    /// otherwise to the last peer observed by this source.
    pub async fn bind_duplex(bind: impl Into<String>) -> Result<(Self, UdpSink)> {
        let bind = bind.into();
        let socket = UdpSocket::bind(&bind).await.map_err(|e| {
            TraceMuxError::new(ErrorId::E1101SourceOpen, format!("udp bind {bind}: {e}"))
                .with_source(e)
        })?;
        let socket = Arc::new(socket);
        let last_peer = Arc::new(RwLock::new(None));
        let source = Self {
            bind,
            socket: Some(socket.clone()),
            last_peer: last_peer.clone(),
        };
        Ok((source, UdpSink::new(socket, last_peer)))
    }

    /// Local bound address (after `open`).
    pub fn local_addr(&self) -> Option<std::net::SocketAddr> {
        self.socket.as_ref().and_then(|s| s.local_addr().ok())
    }
}

#[async_trait]
impl Source for UdpSource {
    async fn open(&mut self) -> Result<()> {
        if self.socket.is_some() {
            return Ok(());
        }
        let s = UdpSocket::bind(&self.bind).await.map_err(|e| {
            TraceMuxError::new(
                ErrorId::E1101SourceOpen,
                format!("udp bind {}: {e}", self.bind),
            )
            .with_source(e)
        })?;
        self.socket = Some(Arc::new(s));
        Ok(())
    }

    async fn recv(&mut self) -> Result<Option<Frame>> {
        let s = match self.socket.as_ref() {
            Some(s) => s,
            None => {
                return Err(TraceMuxError::new(
                    ErrorId::E1102SourceClosed,
                    "udp source not open",
                ))
            }
        };
        let mut buf = vec![0u8; RECV_BUF];
        let (n, peer) = s.recv_from(&mut buf).await.map_err(|e| {
            TraceMuxError::new(ErrorId::E1102SourceClosed, format!("udp recv: {e}")).with_source(e)
        })?;
        *self.last_peer.write() = Some(peer);
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
    use crate::sink::Sink;

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

    // REQ: FR-SINK-UDP
    #[tokio::test]
    async fn duplex_sink_sends_to_explicit_target() {
        let (_src, mut sink) = UdpSource::bind_duplex("127.0.0.1:0").await.unwrap();
        let receiver = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let target = receiver.local_addr().unwrap().to_string();

        sink.ctl("udp-next-target", Some(Bytes::from(target)))
            .await
            .unwrap();
        sink.write(Bytes::from_static(b"pong")).await.unwrap();

        let mut buf = [0u8; 8];
        let (n, _) = receiver.recv_from(&mut buf).await.unwrap();
        assert_eq!(&buf[..n], b"pong");
    }
}
