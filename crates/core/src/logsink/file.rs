//! `FileLogSink` — writes to a `session-dir/` matching
//! `docs/protocols/log-format.md`.

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use bytes::Bytes;
use serde::Serialize;
use uuid::Uuid;

use super::{Direction, LogSink};
use crate::decoder::{Level, Record};
use crate::error_id::{ErrorId, TraceMuxError};
use crate::log::frames::{FrameEntry, FramesWriter};
use crate::log::index::{format_rfc3339_ns, Dir, IndexEntry, IndexWriter, Kind};
use crate::log::lines::{LineEntry, LinesWriter};
use crate::log::raw::RawWriter;
use crate::time::{unix_ns_now, DualTimestamp};
use crate::Result;

/// File-backed log sink for one session directory.
pub struct FileLogSink {
    dir: PathBuf,
    sid: Uuid,
    source: Option<String>,
    host: Option<String>,
    decoder: String,
    raw: RawWriter,
    index: IndexWriter,
    lines: LinesWriter,
    frames: FramesWriter,
    closed: bool,
}

#[derive(Debug, Serialize)]
struct MetaToml<'a> {
    log_format_version: &'static str,
    sid: Uuid,
    created: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    source: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    host: Option<&'a str>,
    decoder: &'a str,
}

impl FileLogSink {
    /// Create a sink for `sid` under `dir`.
    ///
    /// The directory is created if it does not exist. Existing files
    /// are appended to, matching the append-only writer semantics used
    /// by importers and replay fixtures.
    pub fn create(dir: impl AsRef<Path>, sid: Uuid) -> Result<Self> {
        Self::create_with_labels(dir, sid, None, None, "unknown")
    }

    /// Create a sink with optional source/host labels and decoder kind.
    pub fn create_with_labels(
        dir: impl AsRef<Path>,
        sid: Uuid,
        source: Option<String>,
        host: Option<String>,
        decoder: impl Into<String>,
    ) -> Result<Self> {
        let dir = dir.as_ref().to_path_buf();
        std::fs::create_dir_all(&dir).map_err(|e| log_err("creating session-dir", e))?;
        let decoder = decoder.into();
        write_meta(&dir, sid, source.as_deref(), host.as_deref(), &decoder)?;
        let raw = RawWriter::create(&dir).map_err(|e| log_err("opening raw.bin", e))?;
        let index = IndexWriter::create(&dir).map_err(|e| log_err("opening index.jsonl", e))?;
        let lines = LinesWriter::create(&dir).map_err(|e| log_err("opening lines.jsonl", e))?;
        let frames = FramesWriter::create(&dir).map_err(|e| log_err("opening frames.jsonl", e))?;
        Ok(Self {
            dir,
            sid,
            source,
            host,
            decoder,
            raw,
            index,
            lines,
            frames,
            closed: false,
        })
    }

    /// Session directory path.
    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Session id written to `index.jsonl`.
    #[must_use]
    pub fn sid(&self) -> Uuid {
        self.sid
    }
}

#[async_trait]
impl LogSink for FileLogSink {
    async fn append_raw(&mut self, ts: &DualTimestamp, dir: Direction, data: Bytes) -> Result<()> {
        let (off, len) = self
            .raw
            .append(&data)
            .map_err(|e| log_err("appending raw.bin", e))?;
        self.raw
            .flush()
            .map_err(|e| log_err("flushing raw.bin", e))?;
        let mut entry =
            IndexEntry::from_envelope(ts, self.sid, direction_to_dir(dir), Kind::Bytes, off, len);
        entry.source.clone_from(&self.source);
        entry.host.clone_from(&self.host);
        self.index
            .append(&entry)
            .map_err(|e| log_err("appending index.jsonl", e))?;
        self.index
            .flush()
            .map_err(|e| log_err("flushing index.jsonl", e))?;
        Ok(())
    }

    async fn append_record(&mut self, ts: &DualTimestamp, record: &Record) -> Result<()> {
        let ts_ingest = format_rfc3339_ns(ts.ts_ingest_ns);
        if let Some(text) = &record.text {
            self.lines
                .append(&LineEntry {
                    ts: ts_ingest.clone(),
                    level: record.level.map(level_token).map(ToString::to_string),
                    text: text.clone(),
                    correlation_id: record.correlation_id.clone(),
                    tags: record.tags.clone(),
                })
                .map_err(|e| log_err("appending lines.jsonl", e))?;
        }
        self.frames
            .append(&FrameEntry {
                ts: ts_ingest,
                decoder: self.decoder.clone(),
                record: record.clone(),
            })
            .map_err(|e| log_err("appending frames.jsonl", e))?;
        Ok(())
    }

    async fn commit(&mut self) -> Result<()> {
        self.raw
            .flush()
            .map_err(|e| log_err("flushing raw.bin", e))?;
        self.index
            .flush()
            .map_err(|e| log_err("flushing index.jsonl", e))?;
        self.lines
            .flush()
            .map_err(|e| log_err("flushing lines.jsonl", e))?;
        self.frames
            .flush()
            .map_err(|e| log_err("flushing frames.jsonl", e))?;
        Ok(())
    }

    async fn close(&mut self) -> Result<()> {
        if self.closed {
            return Ok(());
        }
        self.commit().await?;
        self.closed = true;
        Ok(())
    }
}

fn write_meta(
    dir: &Path,
    sid: Uuid,
    source: Option<&str>,
    host: Option<&str>,
    decoder: &str,
) -> Result<()> {
    let meta = MetaToml {
        log_format_version: "1.0.0",
        sid,
        created: format_rfc3339_ns(unix_ns_now()),
        source,
        host,
        decoder,
    };
    let body = toml::to_string_pretty(&meta).map_err(|e| {
        TraceMuxError::new(ErrorId::E1001PipelineGeneric, "serialising meta.toml").with_source(e)
    })?;
    std::fs::write(dir.join("meta.toml"), body).map_err(|e| log_err("writing meta.toml", e))
}

fn direction_to_dir(dir: Direction) -> Dir {
    match dir {
        Direction::In => Dir::In,
        Direction::Out => Dir::Out,
    }
}

const fn level_token(level: Level) -> &'static str {
    match level {
        Level::Trace => "trace",
        Level::Debug => "debug",
        Level::Info => "info",
        Level::Warn => "warn",
        Level::Error => "error",
        Level::Fatal => "fatal",
    }
}

fn log_err(ctx: &'static str, e: std::io::Error) -> TraceMuxError {
    TraceMuxError::new(ErrorId::E1001PipelineGeneric, ctx).with_source(e)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::time::{ClockQuality, ClockSource};

    fn tempdir() -> PathBuf {
        let p = std::env::temp_dir().join(format!("tracemux-file-sink-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn ts() -> DualTimestamp {
        DualTimestamp {
            ts_origin_ns: 1_700_000_000_000_000_000,
            ts_ingest_ns: 1_700_000_000_000_500_000,
            mono_ns: 42,
            boot_id: Uuid::nil(),
            node_id: Uuid::nil(),
            clock_offset_ms: 0,
            clock_quality: ClockQuality::BestEffort,
            drift_ppm: 0.0,
            clock_source: ClockSource::System,
        }
    }

    #[tokio::test]
    async fn writes_raw_index_lines_frames_and_meta() {
        let dir = tempdir();
        let sid = Uuid::new_v4();
        let mut sink = FileLogSink::create_with_labels(
            &dir,
            sid,
            Some("mock:persist".to_string()),
            Some("host-a".to_string()),
            "passthrough",
        )
        .unwrap();

        sink.append_raw(&ts(), Direction::In, Bytes::from_static(b"hello"))
            .await
            .unwrap();
        sink.append_record(
            &ts(),
            &Record {
                schema_id: Some("schema:v1".to_string()),
                level: Some(Level::Info),
                text: Some("hello".to_string()),
                fields: json!({"k":"v"}),
                tags: vec!["tag-a".to_string()],
                correlation_id: Some("corr-1".to_string()),
            },
        )
        .await
        .unwrap();
        sink.close().await.unwrap();

        assert_eq!(std::fs::read(dir.join("raw.bin")).unwrap(), b"hello");
        let index = std::fs::read_to_string(dir.join("index.jsonl")).unwrap();
        let index_row: serde_json::Value = serde_json::from_str(index.trim()).unwrap();
        assert_eq!(index_row["sid"], sid.to_string());
        assert_eq!(index_row["source"], "mock:persist");
        assert_eq!(index_row["host"], "host-a");

        let lines = std::fs::read_to_string(dir.join("lines.jsonl")).unwrap();
        let line_row: serde_json::Value = serde_json::from_str(lines.trim()).unwrap();
        assert_eq!(line_row["text"], "hello");
        assert_eq!(line_row["level"], "info");

        let frames = std::fs::read_to_string(dir.join("frames.jsonl")).unwrap();
        let frame_row: serde_json::Value = serde_json::from_str(frames.trim()).unwrap();
        assert_eq!(frame_row["decoder"], "passthrough");
        assert_eq!(frame_row["record"]["schema_id"], "schema:v1");

        let meta = std::fs::read_to_string(dir.join("meta.toml")).unwrap();
        assert!(meta.contains("log_format_version = \"1.0.0\""));
        assert!(meta.contains(&sid.to_string()));
    }

    #[tokio::test]
    async fn raw_and_index_payload_are_visible_before_close() {
        let dir = tempdir();
        let sid = Uuid::new_v4();
        let mut sink = FileLogSink::create(&dir, sid).unwrap();

        sink.append_raw(&ts(), Direction::In, Bytes::from_static(b"live"))
            .await
            .unwrap();

        assert_eq!(std::fs::read(dir.join("raw.bin")).unwrap(), b"live");
        let index = std::fs::read_to_string(dir.join("index.jsonl")).unwrap();
        let index_row: serde_json::Value = serde_json::from_str(index.trim()).unwrap();
        assert_eq!(index_row["sid"], sid.to_string());
        assert_eq!(index_row["off"], 0);
        assert_eq!(index_row["len"], 4);
        sink.close().await.unwrap();
    }
}
