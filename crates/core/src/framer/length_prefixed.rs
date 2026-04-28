//! Length-prefixed framer.
//!
//! Reads a fixed-width length header (1, 2, 4 or 8 bytes, BE or LE)
//! and emits the following `len` bytes as one frame. The length
//! header may be configured to *include* itself (`include_header =
//! true`); useful for protocols whose `len` covers the whole record.

use bytes::{Bytes, BytesMut};

use super::Framer;
use crate::{ErrorId, Result, WanloggerError};

/// Width of the length prefix in bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeaderWidth {
    /// 1 byte
    U8,
    /// 2 bytes
    U16,
    /// 4 bytes
    U32,
    /// 8 bytes
    U64,
}

impl HeaderWidth {
    const fn bytes(self) -> usize {
        match self {
            Self::U8 => 1,
            Self::U16 => 2,
            Self::U32 => 4,
            Self::U64 => 8,
        }
    }
}

/// Endian-ness of the length header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Endian {
    /// Big-endian (network order).
    Big,
    /// Little-endian.
    Little,
}

/// Length-prefixed framer.
#[derive(Debug)]
pub struct LengthPrefixedFramer {
    /// Width of the length header.
    pub width: HeaderWidth,
    /// Endian-ness of the length header.
    pub endian: Endian,
    /// If true, the length value covers the header itself.
    pub include_header: bool,
    /// Reject frames whose declared length exceeds this.
    pub max_frame: usize,
}

impl LengthPrefixedFramer {
    /// Construct.
    #[must_use]
    pub fn new(
        width: HeaderWidth,
        endian: Endian,
        include_header: bool,
        max_frame: usize,
    ) -> Self {
        Self {
            width,
            endian,
            include_header,
            max_frame,
        }
    }

    fn read_len(&self, hdr: &[u8]) -> u64 {
        match (self.width, self.endian) {
            (HeaderWidth::U8, _) => u64::from(hdr[0]),
            (HeaderWidth::U16, Endian::Big) => {
                u64::from(u16::from_be_bytes([hdr[0], hdr[1]]))
            }
            (HeaderWidth::U16, Endian::Little) => {
                u64::from(u16::from_le_bytes([hdr[0], hdr[1]]))
            }
            (HeaderWidth::U32, Endian::Big) => u64::from(u32::from_be_bytes([
                hdr[0], hdr[1], hdr[2], hdr[3],
            ])),
            (HeaderWidth::U32, Endian::Little) => u64::from(u32::from_le_bytes([
                hdr[0], hdr[1], hdr[2], hdr[3],
            ])),
            (HeaderWidth::U64, Endian::Big) => u64::from_be_bytes([
                hdr[0], hdr[1], hdr[2], hdr[3], hdr[4], hdr[5], hdr[6], hdr[7],
            ]),
            (HeaderWidth::U64, Endian::Little) => u64::from_le_bytes([
                hdr[0], hdr[1], hdr[2], hdr[3], hdr[4], hdr[5], hdr[6], hdr[7],
            ]),
        }
    }
}

impl Framer for LengthPrefixedFramer {
    fn poll_frame(&mut self, buf: &mut BytesMut) -> Result<Option<Bytes>> {
        let hdr_len = self.width.bytes();
        if buf.len() < hdr_len {
            return Ok(None);
        }
        let raw_len = self.read_len(&buf[..hdr_len]);
        let payload_len_u64 = if self.include_header {
            raw_len.saturating_sub(hdr_len as u64)
        } else {
            raw_len
        };
        let max = self.max_frame as u64;
        if payload_len_u64 > max {
            return Err(WanloggerError::new(
                ErrorId::E1003FramerOverflow,
                format!(
                    "length-prefixed: declared {payload_len_u64} > max {max}"
                ),
            ));
        }
        let payload_len = payload_len_u64 as usize;
        if buf.len() < hdr_len + payload_len {
            return Ok(None);
        }
        let _ = buf.split_to(hdr_len);
        let payload = buf.split_to(payload_len).freeze();
        Ok(Some(payload))
    }

    fn kind(&self) -> &'static str {
        "length-prefixed"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn u16_be_two_frames() {
        let mut f = LengthPrefixedFramer::new(
            HeaderWidth::U16,
            Endian::Big,
            false,
            1024,
        );
        let mut buf = BytesMut::new();
        buf.extend_from_slice(&[0, 3]);
        buf.extend_from_slice(b"abc");
        buf.extend_from_slice(&[0, 5]);
        buf.extend_from_slice(b"hello");
        assert_eq!(&f.poll_frame(&mut buf).unwrap().unwrap()[..], b"abc");
        assert_eq!(&f.poll_frame(&mut buf).unwrap().unwrap()[..], b"hello");
        assert!(f.poll_frame(&mut buf).unwrap().is_none());
    }

    #[test]
    fn u32_le_partial_returns_none() {
        let mut f = LengthPrefixedFramer::new(
            HeaderWidth::U32,
            Endian::Little,
            false,
            1024,
        );
        let mut buf = BytesMut::new();
        buf.extend_from_slice(&[5, 0, 0, 0]);
        buf.extend_from_slice(b"abc"); // only 3 of 5 payload bytes
        assert!(f.poll_frame(&mut buf).unwrap().is_none());
        buf.extend_from_slice(b"de");
        assert_eq!(&f.poll_frame(&mut buf).unwrap().unwrap()[..], b"abcde");
    }

    #[test]
    fn include_header_subtracts_width() {
        let mut f = LengthPrefixedFramer::new(
            HeaderWidth::U16,
            Endian::Big,
            true,
            1024,
        );
        let mut buf = BytesMut::new();
        // total length 5: 2 hdr + 3 payload
        buf.extend_from_slice(&[0, 5]);
        buf.extend_from_slice(b"abc");
        assert_eq!(&f.poll_frame(&mut buf).unwrap().unwrap()[..], b"abc");
    }

    #[test]
    fn overflow_is_error() {
        let mut f = LengthPrefixedFramer::new(
            HeaderWidth::U32,
            Endian::Big,
            false,
            8,
        );
        let mut buf = BytesMut::from(&[0u8, 0, 0, 99, 0, 0, 0, 0, 0, 0][..]);
        let err = f.poll_frame(&mut buf).unwrap_err();
        assert_eq!(err.id, ErrorId::E1003FramerOverflow);
    }
}
