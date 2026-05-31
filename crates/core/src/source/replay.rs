//! Replay [`Source`].
//!
//! Walks an existing `session-dir/`, emitting one [`Frame`] per row
//! of `index.jsonl` with the bytes from `raw.bin`. Uses the row's
//! `kind` to choose between [`Frame::Bytes`] and [`Frame::Datagram`].
//!
//! Rate-limited replay (`x0.5`, `x2.0`, …) and seeking live in
//! `crates/replay`; this source emits frames as fast as the consumer
//! pulls them.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{BufRead, BufReader};

use async_trait::async_trait;
use bytes::Bytes;

use super::{ChannelMeta, ControlEvt, Frame, Source};
use crate::log::index::{IndexEntry, Kind};
use crate::log::raw::RawReader;
use crate::{ErrorId, Result, TraceMuxError};

/// Replay source.
#[derive(Debug)]
pub struct ReplaySource {
    path: String,
    iter: Option<std::vec::IntoIter<IndexEntry>>,
    raw: Option<RawReader>,
    eof_sent: bool,
}

impl ReplaySource {
    /// Construct.
    #[must_use]
    pub fn new(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            iter: None,
            raw: None,
            eof_sent: false,
        }
    }
}

#[async_trait]
impl Source for ReplaySource {
    async fn open(&mut self) -> Result<()> {
        let dir = std::path::Path::new(&self.path);
        let idx_path = dir.join("index.jsonl");
        let f = File::open(&idx_path).map_err(|e| {
            TraceMuxError::new(
                ErrorId::E1101SourceOpen,
                format!("replay open {}: {e}", idx_path.display()),
            )
            .with_source(e)
        })?;
        let mut entries = Vec::new();
        for line in BufReader::new(f).lines() {
            let line = line.map_err(|e| {
                TraceMuxError::new(ErrorId::E1101SourceOpen, format!("replay read: {e}"))
                    .with_source(e)
            })?;
            if line.is_empty() {
                continue;
            }
            let entry: IndexEntry = serde_json::from_str(&line).map_err(|e| {
                TraceMuxError::new(ErrorId::E1101SourceOpen, format!("replay parse: {e}"))
                    .with_source(e)
            })?;
            entries.push(entry);
        }
        self.iter = Some(entries.into_iter());
        self.raw = Some(RawReader::open(dir).map_err(|e| {
            TraceMuxError::new(ErrorId::E1101SourceOpen, format!("replay raw.bin: {e}"))
                .with_source(e)
        })?);
        Ok(())
    }

    async fn recv(&mut self) -> Result<Option<Frame>> {
        let it = match self.iter.as_mut() {
            Some(i) => i,
            None => {
                return Err(TraceMuxError::new(
                    ErrorId::E1102SourceClosed,
                    "replay source not open",
                ))
            }
        };
        let raw = match self.raw.as_mut() {
            Some(r) => r,
            None => {
                return Err(TraceMuxError::new(
                    ErrorId::E1102SourceClosed,
                    "replay raw not open",
                ))
            }
        };
        let entry = match it.next() {
            Some(e) => e,
            None => {
                self.eof_sent = true;
                return Ok(None);
            }
        };
        let bytes = raw.read_at(entry.off, entry.len).map_err(|e| {
            TraceMuxError::new(ErrorId::E1102SourceClosed, format!("replay read: {e}"))
                .with_source(e)
        })?;
        let data = Bytes::from(bytes);
        Ok(Some(match entry.kind {
            Kind::Datagram => Frame::Datagram {
                src: entry.source.clone(),
                data,
            },
            _ => Frame::Bytes(data),
        }))
    }

    async fn recv_ctl(&mut self) -> Result<Option<ControlEvt>> {
        if self.eof_sent {
            self.eof_sent = false;
            return Ok(Some(ControlEvt::Eof));
        }
        Ok(None)
    }

    fn metadata(&self) -> ChannelMeta {
        ChannelMeta {
            kind: "replay".into(),
            iface: self.path.clone(),
            tags: BTreeMap::new(),
        }
    }

    async fn close(&mut self) -> Result<()> {
        self.iter = None;
        self.raw = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::importer::text::TextImporter;
    use crate::importer::Importer;

    #[tokio::test]
    async fn replays_three_lines() {
        let dir = std::env::temp_dir().join(format!("wlg-replay-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let src_txt = dir.join("in.txt");
        std::fs::write(&src_txt, b"a\nbb\nccc\n").unwrap();
        let session = dir.join("session");
        TextImporter.import(&src_txt, &session).await.unwrap();
        let mut r = ReplaySource::new(session.to_string_lossy().to_string());
        r.open().await.unwrap();
        let mut got = Vec::new();
        while let Some(f) = r.recv().await.unwrap() {
            if let Frame::Bytes(b) = f {
                got.push(String::from_utf8_lossy(&b).to_string());
            }
        }
        assert_eq!(got, vec!["a", "bb", "ccc"]);
        let ctl = r.recv_ctl().await.unwrap();
        assert!(matches!(ctl, Some(ControlEvt::Eof)));
    }
}
