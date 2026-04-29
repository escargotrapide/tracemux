//! TCP probe.
//!
//! Tries to connect to `addr` with a wall-clock timeout and returns
//! `true` on success. Used by `wanlogger detect` and tests to verify
//! that a configured channel target is reachable before the full
//! pipeline is wired up.

use std::time::Duration;

use tokio::net::TcpStream;
use tokio::time::timeout;

/// Probe `addr` with `dur` timeout.
///
/// Returns `true` if a TCP connection completed within the timeout.
pub async fn probe(addr: &str, dur: Duration) -> bool {
    timeout(dur, TcpStream::connect(addr))
        .await
        .ok()
        .and_then(|r| r.ok())
        .is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;

    #[tokio::test]
    async fn probe_open_port_succeeds() {
        let lst = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = lst.local_addr().unwrap();
        tokio::spawn(async move {
            let _ = lst.accept().await;
        });
        assert!(probe(&addr.to_string(), Duration::from_millis(500)).await);
    }

    #[tokio::test]
    async fn probe_closed_port_fails() {
        // 127.0.0.1:1 is reliably refused on every modern OS.
        assert!(!probe("127.0.0.1:1", Duration::from_millis(200)).await);
    }
}
