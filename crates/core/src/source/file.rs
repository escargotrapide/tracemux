//! File-tail [`Source`].
//!
//! Reads bytes from a path. When `follow = true`, the source keeps
//! reading after EOF (poll loop with a short sleep) — equivalent to
//! `tail -f`. When `follow = false`, the first EOF terminates the
//! source.
//!
//! Notes:
//! - File rotation / truncation detection is intentionally minimal in
//!   v0.1 (the file is reopened on every read attempt that returns 0
//!   bytes). A robust implementation will be revisited alongside
//!   `crates/core/src/log/rotate.rs`.
//! - Bytes are emitted in chunks of up to [`READ_CHUNK`] without
//!   line-framing; pair this source with a [`crate::framer`] for
//!   structured output.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use async_trait::async_trait;
use bytes::Bytes;
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncSeekExt, SeekFrom};

use super::{ChannelMeta, ControlEvt, Frame, Source};
use crate::{ErrorId, Result, WanloggerError};

/// Maximum bytes read per `recv()` call.
pub const READ_CHUNK: usize = 8 * 1024;

const POLL_INTERVAL: Duration = Duration::from_millis(50);

/// File-tail source.
#[derive(Debug)]
pub struct FileSource {
    path: PathBuf,
    follow: bool,
    file: Option<File>,
    eof_reached: bool,
}

impl FileSource {
    /// Construct.
    #[must_use]
    pub fn new(path: impl Into<PathBuf>, follow: bool) -> Self {
        Self {
            path: path.into(),
            follow,
            file: None,
            eof_reached: false,
        }
    }
}

#[async_trait]
impl Source for FileSource {
    async fn open(&mut self) -> Result<()> {
        let f = File::open(&self.path).await.map_err(|e| {
            WanloggerError::new(
                ErrorId::E1101SourceOpen,
                format!("file open {}: {e}", self.path.display()),
            )
            .with_source(e)
        })?;
        self.file = Some(f);
        self.eof_reached = false;
        Ok(())
    }

    async fn recv(&mut self) -> Result<Option<Frame>> {
        loop {
            let f = match self.file.as_mut() {
                Some(f) => f,
                None => {
                    return Err(WanloggerError::new(
                        ErrorId::E1102SourceClosed,
                        "file source not open",
                    ))
                }
            };
            let mut buf = vec![0u8; READ_CHUNK];
            let n = f.read(&mut buf).await.map_err(|e| {
                WanloggerError::new(
                    ErrorId::E1102SourceClosed,
                    format!("file read: {e}"),
                )
                .with_source(e)
            })?;
            if n > 0 {
                buf.truncate(n);
                self.eof_reached = false;
                return Ok(Some(Frame::Bytes(Bytes::from(buf))));
            }
            // n == 0 → EOF for this read.
            if !self.follow {
                return Ok(None);
            }
            self.eof_reached = true;
            tokio::time::sleep(POLL_INTERVAL).await;
            // Re-arm at current EOF position. If the file shrank
            // (truncate / rotate), seek back to the start.
            let pos = f.stream_position().await.unwrap_or(0);
            if let Ok(meta) = tokio::fs::metadata(&self.path).await {
                if meta.len() < pos {
                    let _ = f.seek(SeekFrom::Start(0)).await;
                }
            }
        }
    }

    async fn recv_ctl(&mut self) -> Result<Option<ControlEvt>> {
        Ok(None)
    }

    fn metadata(&self) -> ChannelMeta {
        let mut tags = BTreeMap::new();
        tags.insert("follow".into(), self.follow.to_string());
        ChannelMeta {
            kind: "file".into(),
            iface: self.path.display().to_string(),
            tags,
        }
    }

    async fn close(&mut self) -> Result<()> {
        self.file = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_temp(contents: &[u8]) -> std::path::PathBuf {
        let mut p = std::env::temp_dir();
        let pid = std::process::id();
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        p.push(format!("wanlogger-file-src-{pid}-{nonce}.txt"));
        let mut f = std::fs::File::create(&p).unwrap();
        f.write_all(contents).unwrap();
        p
    }

    #[tokio::test]
    async fn read_then_eof_when_not_following() {
        let path = write_temp(b"hello world");
        let mut src = FileSource::new(&path, false);
        src.open().await.unwrap();
        let frame = src.recv().await.unwrap().unwrap();
        match frame {
            Frame::Bytes(b) => assert_eq!(&b[..], b"hello world"),
            _ => panic!("expected Bytes"),
        }
        assert!(src.recv().await.unwrap().is_none());
        src.close().await.unwrap();
        std::fs::remove_file(path).ok();
    }

    #[tokio::test]
    async fn open_missing_path_is_e1101() {
        let mut src = FileSource::new("definitely-not-here.xyz", false);
        let err = src.open().await.unwrap_err();
        assert_eq!(err.id, ErrorId::E1101SourceOpen);
    }

    #[test]
    fn metadata_includes_follow_flag() {
        let src = FileSource::new("a/b.txt", true);
        let meta = src.metadata();
        assert_eq!(meta.kind, "file");
        assert_eq!(meta.tags.get("follow").map(String::as_str), Some("true"));
    }
}
