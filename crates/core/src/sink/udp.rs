//! UDP [`Sink`] implementation.
//!
//! UDP write-back sends to a per-write target supplied via
//! [`Sink::ctl`] kind `"udp-next-target"`, or to the last peer observed
//! by the paired [`crate::source::udp::UdpSource`].

use std::net::SocketAddr;
use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use parking_lot::RwLock;
use tokio::net::UdpSocket;

use super::Sink;
use crate::{ErrorId, Result, TraceMuxError};

/// Shared last-peer state used by UDP source/sink pairs.
pub type SharedUdpPeer = Arc<RwLock<Option<SocketAddr>>>;

/// UDP datagram write-back sink.
#[derive(Debug)]
pub struct UdpSink {
    socket: Arc<UdpSocket>,
    last_peer: SharedUdpPeer,
    next_target: Option<SocketAddr>,
}

impl UdpSink {
    /// Construct from a shared socket and peer tracker.
    #[must_use]
    pub fn new(socket: Arc<UdpSocket>, last_peer: SharedUdpPeer) -> Self {
        Self {
            socket,
            last_peer,
            next_target: None,
        }
    }

    fn resolve_target(&mut self) -> Result<SocketAddr> {
        if let Some(target) = self.next_target.take() {
            return Ok(target);
        }
        self.last_peer.read().ok_or_else(|| {
            TraceMuxError::new(
                ErrorId::E2001WireMalformed,
                "udp write target is required before any peer has sent data",
            )
        })
    }
}

#[async_trait]
impl Sink for UdpSink {
    async fn write(&mut self, data: Bytes) -> Result<()> {
        let target = self.resolve_target()?;
        self.socket.send_to(&data, target).await.map_err(|e| {
            TraceMuxError::new(
                ErrorId::E1102SourceClosed,
                format!("udp send {target}: {e}"),
            )
            .with_source(e)
        })?;
        Ok(())
    }

    async fn ctl(&mut self, kind: &str, data: Option<Bytes>) -> Result<()> {
        if kind != "udp-next-target" {
            return Ok(());
        }
        let Some(data) = data else {
            self.next_target = None;
            return Ok(());
        };
        let text = std::str::from_utf8(&data).map_err(|e| {
            TraceMuxError::new(ErrorId::E2001WireMalformed, format!("udp target utf8: {e}"))
                .with_source(e)
        })?;
        let target = text.parse::<SocketAddr>().map_err(|e| {
            TraceMuxError::new(
                ErrorId::E2001WireMalformed,
                format!("udp target address {text:?}: {e}"),
            )
            .with_source(e)
        })?;
        self.next_target = Some(target);
        Ok(())
    }

    async fn close(&mut self) -> Result<()> {
        self.next_target = None;
        Ok(())
    }
}
