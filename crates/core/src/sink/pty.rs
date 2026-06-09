//! PTY master-side [`Sink`] — writes bytes to the child and resizes the
//! pseudo-console.
//!
//! Paired with [`PtySource`](crate::source::pty::PtySource). The `write`
//! path sends bytes to the child (the child echoes them since it sees a
//! real terminal). The `ctl` path with kind `"resize"` changes the live
//! terminal size; the payload is `"<cols>x<rows>"` ASCII.
//!
//! The real implementation requires the `pty` crate feature.
//!
//! REQ: FR-SRC-PTY
//! REQ: FR-SINK-PROCESS

use async_trait::async_trait;
use bytes::Bytes;

use super::Sink;
use crate::source::pty::clamp_dim;
use crate::{ErrorId, Result, TraceMuxError};

#[cfg(feature = "pty")]
use {
    portable_pty::{MasterPty, PtySize},
    std::io::Write,
    std::sync::{Arc, Mutex},
};

/// Sink that writes to a PTY master and resizes the terminal.
pub struct PtySink {
    iface: String,
    cols: u16,
    rows: u16,
    #[cfg(feature = "pty")]
    writer: Option<Mutex<Box<dyn Write + Send>>>,
    #[cfg(feature = "pty")]
    master: Arc<Mutex<Box<dyn MasterPty + Send>>>,
}

impl std::fmt::Debug for PtySink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PtySink")
            .field("iface", &self.iface)
            .field("cols", &self.cols)
            .field("rows", &self.rows)
            .finish_non_exhaustive()
    }
}

impl PtySink {
    /// Construct from a PTY master writer and the shared master handle.
    #[cfg(feature = "pty")]
    #[must_use]
    pub fn new(
        iface: impl Into<String>,
        writer: Box<dyn Write + Send>,
        master: Arc<Mutex<Box<dyn MasterPty + Send>>>,
        cols: u16,
        rows: u16,
    ) -> Self {
        Self {
            iface: iface.into(),
            cols,
            rows,
            writer: Some(Mutex::new(writer)),
            master,
        }
    }

    /// Parse a `"<cols>x<rows>"` resize payload into clamped dimensions.
    #[must_use]
    pub fn parse_resize(data: Option<&Bytes>) -> Option<(u16, u16)> {
        let raw = data?;
        let text = std::str::from_utf8(raw).ok()?;
        let (cols, rows) = text.trim().split_once('x')?;
        Some((
            clamp_dim(cols.trim().parse().ok()?),
            clamp_dim(rows.trim().parse().ok()?),
        ))
    }

    #[cfg(feature = "pty")]
    fn apply_resize(&mut self, cols: u16, rows: u16) -> Result<()> {
        let master = self.master.lock().map_err(|_| {
            TraceMuxError::new(ErrorId::E1102SourceClosed, "pty master lock poisoned")
        })?;
        master
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| {
                TraceMuxError::new(ErrorId::E1102SourceClosed, format!("pty resize: {e}"))
            })?;
        self.cols = cols;
        self.rows = rows;
        Ok(())
    }
}

#[async_trait]
impl Sink for PtySink {
    #[cfg(feature = "pty")]
    async fn write(&mut self, data: Bytes) -> Result<()> {
        let Some(writer) = self.writer.as_ref() else {
            return Err(TraceMuxError::new(
                ErrorId::E1102SourceClosed,
                "pty sink not open",
            ));
        };
        let mut writer = writer.lock().map_err(|_| {
            TraceMuxError::new(ErrorId::E1102SourceClosed, "pty writer lock poisoned")
        })?;
        writer.write_all(&data).map_err(|e| {
            TraceMuxError::new(ErrorId::E1102SourceClosed, format!("pty write: {e}"))
        })?;
        Ok(())
    }

    #[cfg(not(feature = "pty"))]
    async fn write(&mut self, _data: Bytes) -> Result<()> {
        Err(TraceMuxError::new(
            ErrorId::E1107PtyUnavailable,
            "pty sink requires the `pty` feature",
        ))
    }

    async fn ctl(&mut self, kind: &str, data: Option<Bytes>) -> Result<()> {
        if kind == "resize" {
            if let Some((cols, rows)) = Self::parse_resize(data.as_ref()) {
                #[cfg(feature = "pty")]
                {
                    self.apply_resize(cols, rows)?;
                }
                #[cfg(not(feature = "pty"))]
                {
                    let _ = (cols, rows);
                }
            }
        }
        Ok(())
    }

    async fn flush(&mut self) -> Result<()> {
        #[cfg(feature = "pty")]
        {
            if let Some(writer) = self.writer.as_ref() {
                let mut writer = writer.lock().map_err(|_| {
                    TraceMuxError::new(ErrorId::E1102SourceClosed, "pty writer lock poisoned")
                })?;
                writer.flush().map_err(|e| {
                    TraceMuxError::new(ErrorId::E1102SourceClosed, format!("pty flush: {e}"))
                })?;
            }
        }
        Ok(())
    }

    async fn close(&mut self) -> Result<()> {
        #[cfg(feature = "pty")]
        {
            self.writer = None;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_resize_accepts_cols_x_rows() {
        let got = PtySink::parse_resize(Some(&Bytes::from_static(b"120x40")));
        assert_eq!(got, Some((120, 40)));
    }

    #[test]
    fn parse_resize_clamps_and_rejects_garbage() {
        // In-range values are clamped to the non-zero minimum.
        assert_eq!(
            PtySink::parse_resize(Some(&Bytes::from_static(b"0x40"))),
            Some((1, 40))
        );
        // Values that overflow u16 fail to parse and yield None.
        assert_eq!(
            PtySink::parse_resize(Some(&Bytes::from_static(b"80x99999"))),
            None
        );
        assert_eq!(
            PtySink::parse_resize(Some(&Bytes::from_static(b"nope"))),
            None
        );
        assert_eq!(PtySink::parse_resize(None), None);
    }
}
