//! `raw.bin` — concatenated raw payload bytes.
//!
//! v0.1 stores plain bytes; the zstd-frame + WAL wrapper lives in
//! [`crate::log::wal`] and [`crate::log::group_commit`] (frozen
//! critical paths). [`RawWriter`] is the fallback path used by tests
//! and importers.
//!
//! Each append returns the `(offset, len)` tuple referenced by
//! [`crate::log::index::IndexEntry`].

use std::fs::{File, OpenOptions};
use std::io::{self, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

/// Append-only writer for `raw.bin`.
pub struct RawWriter {
    path: PathBuf,
    inner: BufWriter<File>,
    /// Logical write offset (bytes already appended, including buffered).
    pos: u64,
}

impl RawWriter {
    /// Open (or create) `dir/raw.bin` for appending.
    ///
    /// # Errors
    /// Returns the underlying `io::Error` on failure.
    pub fn create(dir: &Path) -> io::Result<Self> {
        let path = dir.join("raw.bin");
        let mut f = OpenOptions::new()
            .create(true)
            .read(true)
            .append(true)
            .open(&path)?;
        let pos = f.seek(SeekFrom::End(0))?;
        Ok(Self {
            path,
            inner: BufWriter::new(f),
            pos,
        })
    }

    /// Append `data` and return its `(offset, len)`.
    ///
    /// # Errors
    /// Returns the underlying `io::Error` on failure. `len` is
    /// truncated to `u32::MAX`.
    pub fn append(&mut self, data: &[u8]) -> io::Result<(u64, u32)> {
        self.inner.write_all(data)?;
        let off = self.pos;
        let len = u32::try_from(data.len()).unwrap_or(u32::MAX);
        self.pos += data.len() as u64;
        Ok((off, len))
    }

    /// Flush the buffer to the OS.
    ///
    /// # Errors
    /// Returns the underlying `io::Error` on failure.
    pub fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }

    /// Current logical end-of-file offset (next append's `off`).
    #[must_use]
    pub fn pos(&self) -> u64 {
        self.pos
    }

    /// Path of the file being written.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// Random-access reader for `raw.bin`.
#[derive(Debug)]
pub struct RawReader {
    file: File,
}

impl RawReader {
    /// Open an existing `dir/raw.bin` read-only.
    ///
    /// # Errors
    /// Returns the underlying `io::Error` on failure.
    pub fn open(dir: &Path) -> io::Result<Self> {
        let f = File::open(dir.join("raw.bin"))?;
        Ok(Self { file: f })
    }

    /// Read `len` bytes starting at `off` into a new `Vec<u8>`.
    ///
    /// # Errors
    /// Returns `io::Error` if seek or read fails (including short
    /// read at EOF, surfaced as `io::ErrorKind::UnexpectedEof`).
    pub fn read_at(&mut self, off: u64, len: u32) -> io::Result<Vec<u8>> {
        self.file.seek(SeekFrom::Start(off))?;
        let mut buf = vec![0u8; len as usize];
        self.file.read_exact(&mut buf)?;
        Ok(buf)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tempdir() -> PathBuf {
        let p = std::env::temp_dir().join(format!("tracemux-raw-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn append_then_read_back() {
        let dir = tempdir();
        let mut w = RawWriter::create(&dir).unwrap();
        let (o1, l1) = w.append(b"hello ").unwrap();
        let (o2, l2) = w.append(b"world").unwrap();
        w.flush().unwrap();
        assert_eq!(o1, 0);
        assert_eq!(l1, 6);
        assert_eq!(o2, 6);
        assert_eq!(l2, 5);

        let mut r = RawReader::open(&dir).unwrap();
        assert_eq!(r.read_at(o1, l1).unwrap(), b"hello ");
        assert_eq!(r.read_at(o2, l2).unwrap(), b"world");
    }

    #[test]
    fn reopen_continues_offset() {
        let dir = tempdir();
        {
            let mut w = RawWriter::create(&dir).unwrap();
            w.append(b"abc").unwrap();
            w.flush().unwrap();
        }
        let mut w = RawWriter::create(&dir).unwrap();
        assert_eq!(w.pos(), 3);
        let (off, _) = w.append(b"de").unwrap();
        assert_eq!(off, 3);
    }
}
