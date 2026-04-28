//! Serial-port [`Source`] implementation.
//!
//! Enabled via the `serial` Cargo feature (depends on `tokio-serial`).
//! Without the feature the struct compiles as a stub that returns `E-1101`
//! on `open()`.
//!
//! See `.github/skills/add-source/SKILL.md`.

use std::collections::BTreeMap;

use async_trait::async_trait;
#[cfg(feature = "serial")]
use bytes::Bytes;

use super::{ChannelMeta, ControlEvt, Frame, Source};
use crate::{ErrorId, Result, WanloggerError};

// ---- tokio-serial enabled path ----------------------------------------

#[cfg(feature = "serial")]
mod imp {
    use tokio_serial::{DataBits, FlowControl, Parity, SerialPortBuilderExt, StopBits};

    /// Parse the `data_bits` field (5..=8 → [`DataBits`]).
    pub(super) fn data_bits(n: u8) -> tokio_serial::Result<DataBits> {
        match n {
            5 => Ok(DataBits::Five),
            6 => Ok(DataBits::Six),
            7 => Ok(DataBits::Seven),
            8 => Ok(DataBits::Eight),
            _ => Err(tokio_serial::Error::new(
                tokio_serial::ErrorKind::InvalidInput,
                "data_bits must be 5..=8",
            )),
        }
    }

    /// Parse the `parity` field (`"none"` | `"even"` | `"odd"` → [`Parity`]).
    pub(super) fn parity(s: &str) -> tokio_serial::Result<Parity> {
        match s {
            "none" => Ok(Parity::None),
            "even" => Ok(Parity::Even),
            "odd" => Ok(Parity::Odd),
            _ => Err(tokio_serial::Error::new(
                tokio_serial::ErrorKind::InvalidInput,
                "parity must be none|even|odd",
            )),
        }
    }

    /// Parse the `stop_bits` field (1 | 2 → [`StopBits`]).
    pub(super) fn stop_bits(n: u8) -> tokio_serial::Result<StopBits> {
        match n {
            1 => Ok(StopBits::One),
            2 => Ok(StopBits::Two),
            _ => Err(tokio_serial::Error::new(
                tokio_serial::ErrorKind::InvalidInput,
                "stop_bits must be 1 or 2",
            )),
        }
    }

    /// Parse the `flow` field (`"none"` | `"hardware"` | `"software"` → [`FlowControl`]).
    pub(super) fn flow(s: &str) -> tokio_serial::Result<FlowControl> {
        match s {
            "none" => Ok(FlowControl::None),
            "hardware" => Ok(FlowControl::Hardware),
            "software" => Ok(FlowControl::Software),
            _ => Err(tokio_serial::Error::new(
                tokio_serial::ErrorKind::InvalidInput,
                "flow must be none|hardware|software",
            )),
        }
    }

    /// Open the port and return the async stream.
    pub(super) fn open_port(
        port: &str,
        baud: u32,
        db: DataBits,
        par: Parity,
        sb: StopBits,
        fc: FlowControl,
    ) -> tokio_serial::Result<tokio_serial::SerialStream> {
        tokio_serial::new(port, baud)
            .data_bits(db)
            .parity(par)
            .stop_bits(sb)
            .flow_control(fc)
            .open_native_async()
    }
}

// ---- public struct --------------------------------------------------------

/// Serial-port source.
///
/// # Feature
/// Requires `wanlogger-core` to be compiled with feature `serial`.
/// Without the feature, `open()` returns `E-1101`.
///
/// # Example
/// ```rust,ignore
/// use wanlogger_core::source::{serial::SerialSource, Source};
/// # async fn example() -> wanlogger_core::Result<()> {
/// let mut src = SerialSource::new("COM3", 115_200, 8, "none", 1, "none");
/// src.open().await?;
/// # Ok(())
/// # }
/// ```
#[derive(Debug)]
pub struct SerialSource {
    /// Port name (`COM3`, `/dev/ttyUSB0`).
    pub port: String,
    /// Baud rate.
    pub baud: u32,
    /// Data bits (5..=8).
    pub data_bits: u8,
    /// Parity: `"none"` | `"even"` | `"odd"`.
    pub parity: String,
    /// Stop bits: 1 or 2.
    pub stop_bits: u8,
    /// Flow control: `"none"` | `"hardware"` | `"software"`.
    pub flow: String,
    /// Pending control events queue.
    ctl_queue: std::collections::VecDeque<ControlEvt>,
    /// Active port handle (present after `open`, absent before / after `close`).
    #[cfg(feature = "serial")]
    inner: Option<tokio_serial::SerialStream>,
}

impl SerialSource {
    /// Create a new [`SerialSource`].
    #[must_use]
    pub fn new(
        port: impl Into<String>,
        baud: u32,
        data_bits: u8,
        parity: impl Into<String>,
        stop_bits: u8,
        flow: impl Into<String>,
    ) -> Self {
        Self {
            port: port.into(),
            baud,
            data_bits,
            parity: parity.into(),
            stop_bits,
            flow: flow.into(),
            ctl_queue: std::collections::VecDeque::new(),
            #[cfg(feature = "serial")]
            inner: None,
        }
    }
}

#[async_trait]
impl Source for SerialSource {
    /// Open the serial port.
    ///
    /// # Errors
    /// Returns [`ErrorId::E1101SourceOpen`] if the port cannot be opened.
    async fn open(&mut self) -> Result<()> {
        // REQ: FR-SRC-SERIAL
        #[cfg(feature = "serial")]
        {
            let db = imp::data_bits(self.data_bits).map_err(|e| {
                WanloggerError::new(
                    ErrorId::E1101SourceOpen,
                    format!("serial open {}: {e}", self.port),
                )
            })?;
            let par = imp::parity(&self.parity).map_err(|e| {
                WanloggerError::new(
                    ErrorId::E1101SourceOpen,
                    format!("serial open {}: {e}", self.port),
                )
            })?;
            let sb = imp::stop_bits(self.stop_bits).map_err(|e| {
                WanloggerError::new(
                    ErrorId::E1101SourceOpen,
                    format!("serial open {}: {e}", self.port),
                )
            })?;
            let fc = imp::flow(&self.flow).map_err(|e| {
                WanloggerError::new(
                    ErrorId::E1101SourceOpen,
                    format!("serial open {}: {e}", self.port),
                )
            })?;
            let stream = imp::open_port(&self.port, self.baud, db, par, sb, fc).map_err(|e| {
                WanloggerError::new(
                    ErrorId::E1101SourceOpen,
                    format!("serial open {}: {e}", self.port),
                )
            })?;
            self.inner = Some(stream);
            self.ctl_queue.push_back(ControlEvt::Connected);
            tracing::info!(port = %self.port, baud = self.baud, "serial: opened");
            Ok(())
        }
        #[cfg(not(feature = "serial"))]
        {
            Err(WanloggerError::new(
                ErrorId::E1101SourceOpen,
                "serial source requires the `serial` feature",
            ))
        }
    }

    /// Receive the next byte chunk from the port.
    ///
    /// Returns `Ok(None)` on EOF / port removal.
    async fn recv(&mut self) -> Result<Option<Frame>> {
        #[cfg(feature = "serial")]
        {
            use tokio::io::AsyncReadExt as _;
            let Some(port) = self.inner.as_mut() else {
                return Ok(None);
            };
            let mut buf = [0u8; 4096];
            match port.read(&mut buf).await {
                Ok(0) => {
                    self.ctl_queue.push_back(ControlEvt::Eof);
                    Ok(None)
                }
                Ok(n) => Ok(Some(Frame::Bytes(Bytes::copy_from_slice(&buf[..n])))),
                Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => {
                    self.ctl_queue.push_back(ControlEvt::Disconnected {
                        reason: Some(e.to_string()),
                    });
                    Ok(None)
                }
                Err(e) => {
                    self.ctl_queue.push_back(ControlEvt::Error {
                        id: ErrorId::E1001PipelineGeneric,
                        message: e.to_string(),
                    });
                    Err(WanloggerError::new(
                        ErrorId::E1001PipelineGeneric,
                        format!("serial recv {}: {e}", self.port),
                    ))
                }
            }
        }
        #[cfg(not(feature = "serial"))]
        {
            Ok(None)
        }
    }

    async fn recv_ctl(&mut self) -> Result<Option<ControlEvt>> {
        Ok(self.ctl_queue.pop_front())
    }

    fn metadata(&self) -> ChannelMeta {
        let mut tags = BTreeMap::new();
        tags.insert("baud".into(), self.baud.to_string());
        tags.insert("data_bits".into(), self.data_bits.to_string());
        tags.insert("parity".into(), self.parity.clone());
        tags.insert("stop_bits".into(), self.stop_bits.to_string());
        tags.insert("flow".into(), self.flow.clone());
        ChannelMeta {
            kind: "serial".into(),
            iface: self.port.clone(),
            tags,
        }
    }

    async fn close(&mut self) -> Result<()> {
        #[cfg(feature = "serial")]
        {
            self.inner = None;
        }
        self.ctl_queue.push_back(ControlEvt::Eof);
        tracing::debug!(port = %self.port, "serial: closed");
        Ok(())
    }
}

// ---- tests ----------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make() -> SerialSource {
        SerialSource::new("COM_TEST", 115_200, 8, "none", 1, "none")
    }

    #[test]
    fn metadata_has_expected_fields() {
        // REQ: FR-SRC-SERIAL
        let src = make();
        let meta = src.metadata();
        assert_eq!(meta.kind, "serial");
        assert_eq!(meta.iface, "COM_TEST");
        assert_eq!(meta.tags["baud"], "115200");
        assert_eq!(meta.tags["parity"], "none");
    }

    #[cfg(feature = "serial")]
    mod serial_feature {
        use super::*;

        #[test]
        fn data_bits_roundtrip() {
            use tokio_serial::DataBits;
            assert!(matches!(imp::data_bits(8), Ok(DataBits::Eight)));
            assert!(matches!(imp::data_bits(7), Ok(DataBits::Seven)));
            assert!(imp::data_bits(9).is_err());
        }

        #[test]
        fn parity_roundtrip() {
            use tokio_serial::Parity;
            assert!(matches!(imp::parity("none"), Ok(Parity::None)));
            assert!(matches!(imp::parity("even"), Ok(Parity::Even)));
            assert!(imp::parity("bad").is_err());
        }

        #[test]
        fn stop_bits_roundtrip() {
            use tokio_serial::StopBits;
            assert!(matches!(imp::stop_bits(1), Ok(StopBits::One)));
            assert!(matches!(imp::stop_bits(2), Ok(StopBits::Two)));
            assert!(imp::stop_bits(3).is_err());
        }

        #[test]
        fn flow_roundtrip() {
            use tokio_serial::FlowControl;
            assert!(matches!(imp::flow("none"), Ok(FlowControl::None)));
            assert!(matches!(imp::flow("hardware"), Ok(FlowControl::Hardware)));
            assert!(imp::flow("bad").is_err());
        }
    }
}
