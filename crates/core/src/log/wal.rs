//! Write-ahead log for `raw.bin`. **Critical path.**
//!
//! Frame format (little-endian):
//!
//! ```text
//!   file header (12 B): magic "WLOG" | version u16 | reserved u16 | flags u32
//!   record:
//!     len   u32   payload length in bytes
//!     crc   u32   crc32 (IEEE) of payload
//!     bytes [len]
//! ```
//!
//! Recovery rule: scan from the file header forward; stop at the first
//! record that fails to read fully or whose CRC does not match. Return
//! the byte offset at which scanning stopped. That offset is the end of
//! the valid prefix; everything after is treated as torn-write tail and
//! gets truncated by [`WalWriter::open`].
//!
//! See also `docs/protocols/log-format.md` (`WAL & group commit`).

use std::fs::{File, OpenOptions};
use std::io::{self, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use crate::error_id::{ErrorId, TraceMuxError};

/// Magic bytes at the start of every WAL file.
pub const MAGIC: &[u8; 4] = b"WLOG";

/// On-disk format version.
pub const FORMAT_VERSION: u16 = 1;

/// Header length in bytes.
pub const HEADER_LEN: u64 = 12;

/// Maximum payload length per record (1 MiB) -- keep parity with the
/// wire-protocol frame limit. Larger writes are split by the caller.
pub const MAX_PAYLOAD: u32 = 1024 * 1024;

/// Append-only WAL writer.
///
/// Construction recovers any torn tail (see module docs) and positions
/// the cursor at the end of the valid prefix.
#[derive(Debug)]
pub struct WalWriter {
    path: PathBuf,
    file: File,
    end: u64,
}

impl WalWriter {
    /// Open or create a WAL at `path`. Performs torn-tail recovery.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, TraceMuxError> {
        let path = path.as_ref().to_path_buf();
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .map_err(io_err)?;

        let len = file.metadata().map_err(io_err)?.len();

        let end = if len == 0 {
            write_header(&mut file)?;
            file.sync_data().map_err(fsync_err)?;
            HEADER_LEN
        } else {
            verify_header(&mut file)?;
            scan_valid_end(&mut file, len)?
        };

        if end != len {
            file.set_len(end).map_err(io_err)?;
        }
        file.seek(SeekFrom::Start(end)).map_err(io_err)?;

        Ok(Self { path, file, end })
    }

    /// Append one record. Returns the byte offset of the record's
    /// payload (i.e. of the first payload byte, after `len`+`crc`).
    pub fn append(&mut self, payload: &[u8]) -> Result<u64, TraceMuxError> {
        let len = u32::try_from(payload.len()).map_err(|_| {
            TraceMuxError::new(
                ErrorId::E1001PipelineGeneric,
                format!("wal: payload {} > u32::MAX", payload.len()),
            )
        })?;
        if len > MAX_PAYLOAD {
            return Err(TraceMuxError::new(
                ErrorId::E1003FramerOverflow,
                format!("wal: payload {len} > MAX_PAYLOAD={MAX_PAYLOAD}"),
            ));
        }

        let crc = crc32fast::hash(payload);
        let header_off = self.end;
        let payload_off = header_off + 8;

        let mut hdr = [0u8; 8];
        hdr[0..4].copy_from_slice(&len.to_le_bytes());
        hdr[4..8].copy_from_slice(&crc.to_le_bytes());

        self.file.write_all(&hdr).map_err(io_err)?;
        self.file.write_all(payload).map_err(io_err)?;

        self.end = payload_off + u64::from(len);
        Ok(payload_off)
    }

    /// fsync the file. Maps any error to [`ErrorId::E1401WalFsync`].
    pub fn sync(&mut self) -> Result<(), TraceMuxError> {
        self.file.sync_data().map_err(fsync_err)
    }

    /// Logical end-of-file (next append happens here).
    #[must_use]
    pub fn end(&self) -> u64 {
        self.end
    }

    /// Path the WAL was opened from.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// Read-only WAL scanner -- used by recovery and `replay`.
#[derive(Debug)]
pub struct WalReader {
    file: BufReader<File>,
}

/// One decoded WAL record.
#[derive(Debug, Clone)]
pub struct WalRecord {
    /// Byte offset of the payload in the file.
    pub offset: u64,
    /// Payload bytes.
    pub payload: Vec<u8>,
}

impl WalReader {
    /// Open `path` for reading. Verifies the header.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, TraceMuxError> {
        let mut file = File::open(path).map_err(io_err)?;
        verify_header(&mut file)?;
        Ok(Self {
            file: BufReader::new(file),
        })
    }

    /// Read all valid records up to the first torn-tail / CRC error.
    /// Returns the records read plus the offset where scanning stopped.
    pub fn read_all(mut self) -> Result<(Vec<WalRecord>, u64), TraceMuxError> {
        let mut out = Vec::new();
        let mut pos = HEADER_LEN;
        loop {
            let mut hdr = [0u8; 8];
            match self.file.read_exact(&mut hdr) {
                Ok(()) => {}
                Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => break,
                Err(e) => return Err(io_err(e)),
            }
            let len = u32::from_le_bytes([hdr[0], hdr[1], hdr[2], hdr[3]]);
            let crc = u32::from_le_bytes([hdr[4], hdr[5], hdr[6], hdr[7]]);
            if len > MAX_PAYLOAD {
                break;
            }
            let mut payload = vec![0u8; len as usize];
            if let Err(e) = self.file.read_exact(&mut payload) {
                if e.kind() == io::ErrorKind::UnexpectedEof {
                    break;
                }
                return Err(io_err(e));
            }
            if crc32fast::hash(&payload) != crc {
                break;
            }
            let payload_off = pos + 8;
            out.push(WalRecord {
                offset: payload_off,
                payload,
            });
            pos = payload_off + u64::from(len);
        }
        Ok((out, pos))
    }
}

fn write_header(file: &mut File) -> Result<(), TraceMuxError> {
    let mut h = [0u8; HEADER_LEN as usize];
    h[0..4].copy_from_slice(MAGIC);
    h[4..6].copy_from_slice(&FORMAT_VERSION.to_le_bytes());
    file.write_all(&h).map_err(io_err)
}

fn verify_header(file: &mut File) -> Result<(), TraceMuxError> {
    file.seek(SeekFrom::Start(0)).map_err(io_err)?;
    let mut h = [0u8; HEADER_LEN as usize];
    file.read_exact(&mut h).map_err(io_err)?;
    if &h[0..4] != MAGIC {
        return Err(TraceMuxError::new(
            ErrorId::E1001PipelineGeneric,
            "wal: bad magic",
        ));
    }
    let ver = u16::from_le_bytes([h[4], h[5]]);
    if ver != FORMAT_VERSION {
        return Err(TraceMuxError::new(
            ErrorId::E1001PipelineGeneric,
            format!("wal: unsupported version {ver}"),
        ));
    }
    Ok(())
}

fn scan_valid_end(file: &mut File, len: u64) -> Result<u64, TraceMuxError> {
    file.seek(SeekFrom::Start(HEADER_LEN)).map_err(io_err)?;
    let mut reader = BufReader::new(file);
    let mut pos = HEADER_LEN;
    loop {
        if pos == len {
            break;
        }
        let mut hdr = [0u8; 8];
        match reader.read_exact(&mut hdr) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(io_err(e)),
        }
        let rlen = u32::from_le_bytes([hdr[0], hdr[1], hdr[2], hdr[3]]);
        let rcrc = u32::from_le_bytes([hdr[4], hdr[5], hdr[6], hdr[7]]);
        if rlen > MAX_PAYLOAD {
            break;
        }
        let needed = u64::from(rlen);
        if pos + 8 + needed > len {
            break;
        }
        let mut payload = vec![0u8; rlen as usize];
        if let Err(e) = reader.read_exact(&mut payload) {
            if e.kind() == io::ErrorKind::UnexpectedEof {
                break;
            }
            return Err(io_err(e));
        }
        if crc32fast::hash(&payload) != rcrc {
            break;
        }
        pos += 8 + needed;
    }
    Ok(pos)
}

fn io_err(e: io::Error) -> TraceMuxError {
    TraceMuxError::new(ErrorId::E1001PipelineGeneric, format!("wal io: {e}")).with_source(e)
}

fn fsync_err(e: io::Error) -> TraceMuxError {
    TraceMuxError::new(ErrorId::E1401WalFsync, format!("wal fsync: {e}")).with_source(e)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> tempfile::TempDir {
        tempfile::tempdir().expect("tempdir")
    }

    // REQ: FR-LOG-002
    #[test]
    fn append_then_read_roundtrip() {
        let dir = tmp();
        let p = dir.path().join("raw.wal");
        let mut w = WalWriter::open(&p).unwrap();
        let off1 = w.append(b"hello").unwrap();
        let off2 = w.append(b"world!").unwrap();
        w.sync().unwrap();

        let r = WalReader::open(&p).unwrap();
        let (recs, end) = r.read_all().unwrap();
        assert_eq!(recs.len(), 2);
        assert_eq!(recs[0].offset, off1);
        assert_eq!(recs[0].payload, b"hello");
        assert_eq!(recs[1].offset, off2);
        assert_eq!(recs[1].payload, b"world!");
        assert_eq!(end, w.end());
    }

    // REQ: FR-LOG-002
    #[test]
    fn reopen_continues_appending() {
        let dir = tmp();
        let p = dir.path().join("raw.wal");
        {
            let mut w = WalWriter::open(&p).unwrap();
            w.append(b"a").unwrap();
            w.sync().unwrap();
        }
        let mut w = WalWriter::open(&p).unwrap();
        w.append(b"b").unwrap();
        w.sync().unwrap();

        let (recs, _) = WalReader::open(&p).unwrap().read_all().unwrap();
        let payloads: Vec<_> = recs.iter().map(|r| r.payload.clone()).collect();
        assert_eq!(payloads, vec![b"a".to_vec(), b"b".to_vec()]);
    }

    // REQ: FR-LOG-002
    #[test]
    fn torn_tail_is_truncated_on_open() {
        let dir = tmp();
        let p = dir.path().join("raw.wal");
        {
            let mut w = WalWriter::open(&p).unwrap();
            w.append(b"good").unwrap();
            w.sync().unwrap();
        }
        {
            let mut f = OpenOptions::new().append(true).open(&p).unwrap();
            f.write_all(&[0xff, 0x00, 0x00, 0x00]).unwrap();
            f.sync_data().unwrap();
        }
        let len_before = std::fs::metadata(&p).unwrap().len();

        let w = WalWriter::open(&p).unwrap();
        let len_after = std::fs::metadata(&p).unwrap().len();

        assert!(len_after < len_before, "torn tail should be truncated");
        assert_eq!(w.end(), len_after);

        let (recs, _) = WalReader::open(&p).unwrap().read_all().unwrap();
        assert_eq!(recs.len(), 1);
        assert_eq!(recs[0].payload, b"good");
    }

    // REQ: FR-LOG-002
    #[test]
    fn crc_mismatch_stops_recovery_at_boundary() {
        let dir = tmp();
        let p = dir.path().join("raw.wal");
        {
            let mut w = WalWriter::open(&p).unwrap();
            w.append(b"first").unwrap();
            w.append(b"second").unwrap();
            w.sync().unwrap();
        }
        let total = std::fs::metadata(&p).unwrap().len();
        {
            let mut f = OpenOptions::new().write(true).open(&p).unwrap();
            f.seek(SeekFrom::Start(total - 1)).unwrap();
            f.write_all(&[0x00]).unwrap();
            f.sync_data().unwrap();
        }

        let w = WalWriter::open(&p).unwrap();
        let (recs, _) = WalReader::open(&p).unwrap().read_all().unwrap();
        assert_eq!(recs.len(), 1);
        assert_eq!(recs[0].payload, b"first");
        assert!(w.end() < total);
    }

    // REQ: FR-LOG-002
    #[test]
    fn rejects_payload_over_max() {
        let dir = tmp();
        let p = dir.path().join("raw.wal");
        let mut w = WalWriter::open(&p).unwrap();
        let big = vec![0u8; (MAX_PAYLOAD + 1) as usize];
        let err = w.append(&big).unwrap_err();
        assert_eq!(err.id, ErrorId::E1003FramerOverflow);
    }

    // REQ: FR-CORE-003
    #[test]
    fn fsync_error_maps_to_canonical_id() {
        let err = fsync_err(io::Error::other("disk on fire"));
        assert_eq!(err.id, ErrorId::E1401WalFsync);
    }
}
