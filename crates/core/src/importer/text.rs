//! Plain-text line importer.
//!
//! Reads `src` line-by-line and appends each line (without trailing
//! `\n`) to `dst/raw.bin`, with one [`crate::log::index::IndexEntry`]
//! per line, `Kind::Bytes`, `Dir::In`, `ClockQuality::Imported`,
//! `ClockSource::Imported`. Both timestamps are set to ingest-now.

use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use async_trait::async_trait;
use uuid::Uuid;

use crate::error_id::{ErrorId, WanloggerError};
use crate::importer::Importer;
use crate::log::index::{Dir, IndexEntry, IndexWriter, Kind};
use crate::log::raw::RawWriter;
use crate::time::{unix_ns_now, ClockQuality, ClockSource, DualTimestamp};
use crate::Result;

/// Plain-text importer.
#[derive(Debug, Default)]
pub struct TextImporter;

#[async_trait]
impl Importer for TextImporter {
    fn kind(&self) -> &'static str {
        "text"
    }

    async fn import(&mut self, src: &Path, dst: &Path) -> Result<()> {
        run(src, dst)
    }
}

fn run(src: &Path, dst: &Path) -> Result<()> {
    std::fs::create_dir_all(dst).map_err(|e| err("creating dst dir", e))?;
    let f = File::open(src).map_err(|e| err("opening src", e))?;
    let rd = BufReader::new(f);
    let mut raw = RawWriter::create(dst).map_err(|e| err("opening raw.bin", e))?;
    let mut idx = IndexWriter::create(dst).map_err(|e| err("opening index.jsonl", e))?;
    let sid = Uuid::new_v4();
    for line in rd.lines() {
        let line = line.map_err(|e| err("reading line", e))?;
        let bytes = line.as_bytes();
        let (off, len) = raw.append(bytes).map_err(|e| err("raw append", e))?;
        let ts = imported_ts();
        idx.append(&IndexEntry::from_envelope(
            &ts,
            sid,
            Dir::In,
            Kind::Bytes,
            off,
            len,
        ))
        .map_err(|e| err("index append", e))?;
    }
    raw.flush().map_err(|e| err("flush raw", e))?;
    idx.flush().map_err(|e| err("flush index", e))?;
    Ok(())
}

fn imported_ts() -> DualTimestamp {
    let now = unix_ns_now();
    DualTimestamp {
        ts_origin_ns: now,
        ts_ingest_ns: now,
        mono_ns: 0,
        boot_id: Uuid::nil(),
        node_id: Uuid::nil(),
        clock_offset_ms: 0,
        clock_quality: ClockQuality::Imported,
        drift_ppm: 0.0,
        clock_source: ClockSource::Imported,
    }
}

fn err(ctx: &str, e: std::io::Error) -> WanloggerError {
    WanloggerError::new(ErrorId::E1001PipelineGeneric, format!("text-import: {ctx}")).with_source(e)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tempdir() -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("wanlogger-import-text-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[tokio::test]
    async fn imports_three_lines() {
        let dir = tempdir();
        let src = dir.join("in.txt");
        std::fs::write(&src, b"alpha\nbeta\ngamma\n").unwrap();
        let dst = dir.join("session");
        TextImporter.import(&src, &dst).await.unwrap();
        let raw = std::fs::read(dst.join("raw.bin")).unwrap();
        assert_eq!(&raw[..], b"alphabetagamma");
        let idx = std::fs::read_to_string(dst.join("index.jsonl")).unwrap();
        assert_eq!(idx.lines().count(), 3);
    }
}
