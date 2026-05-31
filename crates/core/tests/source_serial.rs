//! Integration tests for [`SerialSource`].
//!
//! All tests require the `serial` Cargo feature:
//! ```text
//! cargo test -p tracemux-core --features serial -- source_serial
//! ```
//!
//! **Platform coverage:**
//! - Parameter-validation tests run everywhere (no physical port needed).
//! - PTY loopback test runs on Unix only (uses `serialport::TTYPort::pair()`).
//! - Windows real-port smoke test is `#[ignore]` by default; enable by setting
//!   `TRACEMUX_TEST_SERIAL_PORT=COM3` (or whichever port is available).

#[cfg(feature = "serial")]
mod tests {
    use tracemux_core::{
        source::{serial::SerialSource, ControlEvt, Source},
        ErrorId,
    };

    // -----------------------------------------------------------------------
    // Parameter-validation tests (no physical port required)
    // -----------------------------------------------------------------------

    /// Invalid `data_bits` (9) must fail immediately with E-1101 before
    /// attempting to open the OS device.
    // REQ: FR-SRC-SERIAL
    #[tokio::test]
    async fn open_invalid_data_bits_returns_e1101() {
        let mut src = SerialSource::new("COM1", 115_200, 9, "none", 1, "none");
        let err = src
            .open()
            .await
            .expect_err("should fail: data_bits=9 is out of range");
        assert_eq!(err.id, ErrorId::E1101SourceOpen, "wrong error id: {err}");
    }

    /// Invalid `parity` string must fail immediately with E-1101.
    // REQ: FR-SRC-SERIAL
    #[tokio::test]
    async fn open_invalid_parity_returns_e1101() {
        let mut src = SerialSource::new("COM1", 115_200, 8, "INVALID", 1, "none");
        let err = src.open().await.expect_err("should fail: parity='INVALID'");
        assert_eq!(err.id, ErrorId::E1101SourceOpen, "wrong error id: {err}");
    }

    /// Invalid `stop_bits` (3) must fail immediately with E-1101.
    // REQ: FR-SRC-SERIAL
    #[tokio::test]
    async fn open_invalid_stop_bits_returns_e1101() {
        let mut src = SerialSource::new("COM1", 115_200, 8, "none", 3, "none");
        let err = src
            .open()
            .await
            .expect_err("should fail: stop_bits=3 is out of range");
        assert_eq!(err.id, ErrorId::E1101SourceOpen, "wrong error id: {err}");
    }

    /// Invalid `flow` string must fail immediately with E-1101.
    // REQ: FR-SRC-SERIAL
    #[tokio::test]
    async fn open_invalid_flow_returns_e1101() {
        let mut src = SerialSource::new("COM1", 115_200, 8, "none", 1, "BADFLOW");
        let err = src.open().await.expect_err("should fail: flow='BADFLOW'");
        assert_eq!(err.id, ErrorId::E1101SourceOpen, "wrong error id: {err}");
    }

    /// Opening a clearly nonexistent OS port must fail with E-1101.
    // REQ: FR-SRC-SERIAL
    #[tokio::test]
    async fn open_nonexistent_port_returns_e1101() {
        // COM249 on Windows, /dev/ttyTRACEMUX_NONE on Linux/macOS ? both
        // should be absent on any normal development machine.
        let port = if cfg!(windows) {
            "COM249"
        } else {
            "/dev/ttyTRACEMUX_NONE"
        };
        let mut src = SerialSource::new(port, 115_200, 8, "none", 1, "none");
        let err = src
            .open()
            .await
            .expect_err("should fail: port does not exist");
        assert_eq!(err.id, ErrorId::E1101SourceOpen, "wrong error id: {err}");
    }

    // -----------------------------------------------------------------------
    // Unix PTY loopback test
    // -----------------------------------------------------------------------

    #[cfg(unix)]
    mod unix_pty {
        use super::*;
        use std::io::Write as _;
        use tracemux_core::source::Frame;

        /// Full loopback: creates a virtual PTY pair, writes bytes from the
        /// master end, and verifies `SerialSource` receives them along with
        /// the expected control events.
        // REQ: FR-SRC-SERIAL
        #[tokio::test]
        async fn loopback_pty_bytes_and_events() {
            // Create a virtual PTY pair using the `serialport` crate.
            // The slave end has a device path like `/dev/pts/N`.
            let (mut master, slave) = serialport::TTYPort::pair()
                .expect("failed to create virtual PTY pair (is /dev/ptmx accessible?)");

            let slave_name = slave
                .name()
                .expect("slave PTY has no device name")
                .to_string();

            // Release the slave handle so SerialSource can re-open it by path.
            // The path persists as long as the master fd is open.
            drop(slave);

            // Open SerialSource on the slave path.
            let mut src = SerialSource::new(&slave_name, 115_200, 8, "none", 1, "none");
            src.open()
                .await
                .expect("open should succeed on a valid PTY slave path");

            // After open(), a Connected event must be queued.
            assert!(
                matches!(src.recv_ctl().await.unwrap(), Some(ControlEvt::Connected)),
                "expected ControlEvt::Connected after open"
            );

            // Write a payload to the master end. The PTY kernel buffer is large
            // enough that this synchronous write will not block for small payloads.
            let payload: &[u8] = b"loopback_integration_test\n";
            master.write_all(payload).expect("master PTY write failed");

            // Collect bytes from SerialSource until the full payload is received.
            let mut received: Vec<u8> = Vec::new();
            loop {
                match src.recv().await.expect("recv returned Err") {
                    Some(Frame::Bytes(b)) => {
                        received.extend_from_slice(&b);
                        if received.len() >= payload.len() {
                            break;
                        }
                    }
                    None => break,
                    other => panic!("unexpected Frame variant: {other:?}"),
                }
            }

            assert_eq!(
                &received[..payload.len()],
                payload,
                "received bytes do not match written payload"
            );

            src.close().await.unwrap();
        }
    }

    // -----------------------------------------------------------------------
    // Windows opt-in real-port smoke test
    // -----------------------------------------------------------------------

    #[cfg(windows)]
    mod windows_real_port {
        use super::*;

        /// Opt-in real COM port smoke test.
        ///
        /// Enable by setting the environment variable:
        /// ```text
        /// $env:TRACEMUX_TEST_SERIAL_PORT = "COM3"
        /// cargo test -p tracemux-core --features serial -- source_serial
        /// ```
        // REQ: FR-SRC-SERIAL
        #[tokio::test]
        #[ignore = "requires a real or virtual (com0com) COM port; \
                    set TRACEMUX_TEST_SERIAL_PORT to run"]
        async fn open_real_port_connected_event() {
            let port = std::env::var("TRACEMUX_TEST_SERIAL_PORT")
                .expect("set TRACEMUX_TEST_SERIAL_PORT to a valid COM port name");

            let mut src = SerialSource::new(&port, 115_200, 8, "none", 1, "none");
            src.open()
                .await
                .unwrap_or_else(|e| panic!("open({port}) failed: {e}"));

            assert!(
                matches!(src.recv_ctl().await.unwrap(), Some(ControlEvt::Connected)),
                "expected ControlEvt::Connected after open"
            );

            src.close().await.unwrap();
        }
    }
}
