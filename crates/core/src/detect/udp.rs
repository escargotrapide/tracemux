//! UDP probe.
//!
//! Binds the given address and waits up to `dur` for any datagram.
//! Returns the first datagram's `(peer, payload)` or `None` on
//! timeout. Useful for the CLI's `detect` command to check whether
//! something is sending to a configured syslog/UDP listener.

use std::time::Duration;

use tokio::net::UdpSocket;
use tokio::time::timeout;

/// Bind `bind` and wait up to `dur` for a datagram.
pub async fn probe(bind: &str, dur: Duration) -> Option<(String, Vec<u8>)> {
    let s = UdpSocket::bind(bind).await.ok()?;
    let mut buf = vec![0u8; 64 * 1024];
    let r = timeout(dur, s.recv_from(&mut buf)).await.ok()?.ok()?;
    let (n, peer) = r;
    buf.truncate(n);
    Some((peer.to_string(), buf))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn probe_receives_one_datagram() {
        let listener = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let bind = listener.local_addr().unwrap().to_string();
        drop(listener);
        let bind_clone = bind.clone();
        let recv =
            tokio::spawn(async move { probe(&bind_clone, Duration::from_millis(800)).await });
        // Give the receiver time to bind.
        tokio::time::sleep(Duration::from_millis(100)).await;
        let sender = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        sender.connect(&bind).await.unwrap();
        sender.send(b"hello").await.unwrap();
        let r = recv.await.unwrap();
        assert!(r.is_some());
        assert_eq!(r.unwrap().1, b"hello");
    }

    #[tokio::test]
    async fn probe_times_out() {
        let r = probe("127.0.0.1:0", Duration::from_millis(50)).await;
        assert!(r.is_none());
    }
}
