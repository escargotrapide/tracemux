//! Line framer — emits one frame per `\n` (LF), `\r\n` (CRLF) or `\r`
//! (CR) terminator depending on configuration. v0.1 minimal impl.

use bytes::{Bytes, BytesMut};

use super::Framer;
use crate::{ErrorId, Result, WanloggerError};

/// EOL handling.
#[derive(Debug, Clone, Copy)]
pub enum Eol {
    /// `\n`
    Lf,
    /// `\r\n`
    Crlf,
    /// `\r`
    Cr,
    /// Auto-detect (any of the above).
    Auto,
}

/// Line framer.
#[derive(Debug)]
pub struct LineFramer {
    /// EOL.
    pub eol: Eol,
    /// Maximum frame size.
    pub max_frame: usize,
}

impl LineFramer {
    /// Construct.
    #[must_use]
    pub fn new(eol: Eol, max_frame: usize) -> Self {
        Self { eol, max_frame }
    }
}

impl Framer for LineFramer {
    fn poll_frame(&mut self, buf: &mut BytesMut) -> Result<Option<Bytes>> {
        let pos = match self.eol {
            Eol::Lf | Eol::Auto => buf.iter().position(|b| *b == b'\n'),
            Eol::Cr => buf.iter().position(|b| *b == b'\r'),
            Eol::Crlf => {
                // Find `\n` whose preceding byte is `\r`.
                buf.windows(2).position(|w| w == b"\r\n").map(|i| i + 1)
            }
        };
        if let Some(idx) = pos {
            let mut frame = buf.split_to(idx + 1).freeze();
            // Trim trailing `\r` from CRLF / CR cases.
            if matches!(self.eol, Eol::Crlf | Eol::Auto)
                && frame.len() >= 2
                && &frame[frame.len() - 2..] == b"\r\n"
            {
                frame = frame.slice(..frame.len() - 2);
            } else if frame.last() == Some(&b'\n') || frame.last() == Some(&b'\r') {
                frame = frame.slice(..frame.len() - 1);
            }
            Ok(Some(frame))
        } else if buf.len() > self.max_frame {
            Err(WanloggerError::new(
                ErrorId::E1003FramerOverflow,
                format!(
                    "line framer: buffered {} > max {}",
                    buf.len(),
                    self.max_frame
                ),
            ))
        } else {
            Ok(None)
        }
    }

    fn kind(&self) -> &'static str {
        "line"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // REQ: FR-FRM-LINE
    #[test]
    fn lf_basic() {
        let mut f = LineFramer::new(Eol::Lf, 1024);
        let mut buf = BytesMut::from(&b"hello\nworld\n"[..]);
        let a = f.poll_frame(&mut buf).unwrap().unwrap();
        let b = f.poll_frame(&mut buf).unwrap().unwrap();
        assert_eq!(&a[..], b"hello");
        assert_eq!(&b[..], b"world");
        assert!(f.poll_frame(&mut buf).unwrap().is_none());
    }
}
